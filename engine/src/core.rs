//! Colors, scanlines and the image arithmetic at the heart of the search:
//! the best color for a shape, drawing it, and scoring the result. Images
//! are RGBA bytes, 4 per pixel, row by row.

use crate::palette::Palette;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub r: i32,
    pub g: i32,
    pub b: i32,
    pub a: i32,
}

/// A horizontal run of pixels from x1 to x2 inclusive, covered by alpha
/// (0 to 0xffff).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scanline {
    pub y: i32,
    pub x1: i32,
    pub x2: i32,
    pub alpha: u32,
}

#[inline]
fn offset(w: usize, x: i32, y: i32) -> usize {
    (y as usize * w + x as usize) * 4
}

/// The color that, drawn over current with the given alpha, best matches
/// target on lines; with a palette, the best of its colors.
pub fn compute_color(
    target: &[u8],
    current: &[u8],
    w: usize,
    lines: &[Scanline],
    alpha: i32,
    palette: Option<&Palette>,
) -> Color {
    let (mut rsum, mut gsum, mut bsum, mut count) = (0i64, 0i64, 0i64, 0i64);
    let a = (0x101 * 255 / alpha) as i64;
    for line in lines {
        let i = offset(w, line.x1, line.y);
        let n = (line.x2 - line.x1 + 1) as usize;
        let t = &target[i..i + n * 4];
        let c = &current[i..i + n * 4];
        let (mut rs, mut gs, mut bs) = (0i64, 0i64, 0i64);
        for (t, c) in t.as_chunks::<4>().0.iter().zip(c.as_chunks::<4>().0) {
            // a is up to 0xffff at alpha 1, so per-pixel terms need 64 bits
            let (cr, cg, cb) = (c[0] as i64, c[1] as i64, c[2] as i64);
            rs += (t[0] as i64 - cr) * a + cr * 0x101;
            gs += (t[1] as i64 - cg) * a + cg * 0x101;
            bs += (t[2] as i64 - cb) * a + cb * 0x101;
        }
        rsum += rs;
        gsum += gs;
        bsum += bs;
        count += n as i64;
    }
    if count == 0 {
        return Color::default();
    }
    let best = Color {
        r: (rsum / count) as i32 >> 8,
        g: (gsum / count) as i32 >> 8,
        b: (bsum / count) as i32 >> 8,
        a: alpha,
    };
    if let Some(p) = palette {
        // The squared error grows with the squared distance from the best
        // color, equally in every channel, so the palette color nearest to
        // it fits best, as long as the best color is not clamped first.
        // (Drawing and scoring every palette color instead did no better,
        // only rounding differs, and took 3 to 6 times as long.)
        return Color {
            a: alpha,
            ..p.nearest(best)
        };
    }
    Color {
        r: best.r.clamp(0, 255),
        g: best.g.clamp(0, 255),
        b: best.b.clamp(0, 255),
        a: alpha,
    }
}

pub fn copy_lines(dst: &mut [u8], src: &[u8], w: usize, lines: &[Scanline]) {
    for line in lines {
        let a = offset(w, line.x1, line.y);
        let b = a + (line.x2 - line.x1 + 1) as usize * 4;
        dst[a..b].copy_from_slice(&src[a..b]);
    }
}

/// Composites c over im on lines, with the same arithmetic as Go's
/// image/draw.
pub fn draw_lines(im: &mut [u8], w: usize, c: Color, lines: &[Scanline]) {
    const M: u32 = 0xffff;
    let sa = c.a as u32 * 0x101;
    let sr = c.r as u32 * 0x101 * sa / M;
    let sg = c.g as u32 * 0x101 * sa / M;
    let sb = c.b as u32 * 0x101 * sa / M;
    for line in lines {
        let ma = line.alpha;
        let a = (M - sa * ma / M) * 0x101;
        let i = offset(w, line.x1, line.y);
        let n = (line.x2 - line.x1 + 1) as usize;
        for p in im[i..i + n * 4].as_chunks_mut::<4>().0 {
            p[0] = (((p[0] as u32 * a + sr * ma) / M) >> 8) as u8;
            p[1] = (((p[1] as u32 * a + sg * ma) / M) >> 8) as u8;
            p[2] = (((p[2] as u32 * a + sb * ma) / M) >> 8) as u8;
            p[3] = (((p[3] as u32 * a + sa * ma) / M) >> 8) as u8;
        }
    }
}

/// Root mean square difference of two images, from 0 to 1.
pub fn difference_full(a: &[u8], b: &[u8]) -> f64 {
    let mut total = 0u64;
    for (a, b) in a.iter().zip(b) {
        let d = *a as i32 - *b as i32;
        total += (d * d) as u64;
    }
    (total as f64 / a.len() as f64).sqrt() / 255.0
}

/// Updates score, the difference between target and before, for after,
/// which differs from before only on lines.
pub fn difference_partial(
    target: &[u8],
    before: &[u8],
    after: &[u8],
    w: usize,
    h: usize,
    score: f64,
    lines: &[Scanline],
) -> f64 {
    let n = (w * h * 4) as f64;
    let mut total = ((score * 255.0).powi(2) * n) as i64;
    for line in lines {
        let i = offset(w, line.x1, line.y);
        let len = (line.x2 - line.x1 + 1) as usize * 4;
        let t = &target[i..i + len];
        let b = &before[i..i + len];
        let a = &after[i..i + len];
        let mut sum = 0i32;
        for k in 0..len {
            let d1 = t[k] as i32 - b[k] as i32;
            let d2 = t[k] as i32 - a[k] as i32;
            sum += d2 * d2 - d1 * d1;
        }
        total += sum as i64;
    }
    (total.max(0) as f64 / n).sqrt() / 255.0
}

pub fn average_color(pix: &[u8]) -> Color {
    let (mut r, mut g, mut b) = (0usize, 0usize, 0usize);
    for p in pix.as_chunks::<4>().0 {
        r += p[0] as usize;
        g += p[1] as usize;
        b += p[2] as usize;
    }
    let n = (pix.len() / 4).max(1);
    Color {
        r: (r / n) as i32,
        g: (g / n) as i32,
        b: (b / n) as i32,
        a: 255,
    }
}
