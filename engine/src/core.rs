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
    weights: Option<&Weights>,
) -> Color {
    let (mut rsum, mut gsum, mut bsum, mut count) = (0i64, 0i64, 0i64, 0i64);
    let a = (0x101 * 255 / alpha) as i64;
    for line in lines {
        let i = offset(w, line.x1, line.y);
        let n = (line.x2 - line.x1 + 1) as usize;
        let t = &target[i..i + n * 4];
        let c = &current[i..i + n * 4];
        let (mut rs, mut gs, mut bs) = (0i64, 0i64, 0i64);
        let pixels = t.as_chunks::<4>().0.iter().zip(c.as_chunks::<4>().0);
        if let Some(wt) = weights {
            // the weighted mean of each pixel's best color
            let wt = &wt.pix[i / 4..i / 4 + n];
            for ((t, c), &k) in pixels.zip(wt) {
                let (cr, cg, cb, k) = (c[0] as i64, c[1] as i64, c[2] as i64, k as i64);
                rs += ((t[0] as i64 - cr) * a + cr * 0x101) * k;
                gs += ((t[1] as i64 - cg) * a + cg * 0x101) * k;
                bs += ((t[2] as i64 - cb) * a + cb * 0x101) * k;
                count += k;
            }
        } else {
            for (t, c) in pixels {
                // a is up to 0xffff at alpha 1, so per-pixel terms need 64 bits
                let (cr, cg, cb) = (c[0] as i64, c[1] as i64, c[2] as i64);
                rs += (t[0] as i64 - cr) * a + cr * 0x101;
                gs += (t[1] as i64 - cg) * a + cg * 0x101;
                bs += (t[2] as i64 - cb) * a + cb * 0x101;
            }
            count += n as i64;
        }
        rsum += rs;
        gsum += gs;
        bsum += bs;
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

/// Like draw_lines, with each pixel's coverage also scaled by mask (one
/// byte per pixel, 255 for full), which softens the edges of a region.
pub fn draw_lines_masked(im: &mut [u8], w: usize, c: Color, lines: &[Scanline], mask: &[u8]) {
    const M: u32 = 0xffff;
    let sa = c.a as u32 * 0x101;
    let sr = c.r as u32 * 0x101 * sa / M;
    let sg = c.g as u32 * 0x101 * sa / M;
    let sb = c.b as u32 * 0x101 * sa / M;
    for line in lines {
        let i = offset(w, line.x1, line.y);
        let n = (line.x2 - line.x1 + 1) as usize;
        let m = &mask[i / 4..i / 4 + n];
        for (p, &m) in im[i..i + n * 4].as_chunks_mut::<4>().0.iter_mut().zip(m) {
            let ma = line.alpha * m as u32 / 255;
            let a = (M - sa * ma / M) * 0x101;
            p[0] = (((p[0] as u32 * a + sr * ma) / M) >> 8) as u8;
            p[1] = (((p[1] as u32 * a + sg * ma) / M) >> 8) as u8;
            p[2] = (((p[2] as u32 * a + sb * ma) / M) >> 8) as u8;
            p[3] = (((p[3] as u32 * a + sa * ma) / M) >> 8) as u8;
        }
    }
}

/// The runs of pixels inside a mask, row by row, for clipping scanlines to
/// it.
pub struct Clip {
    runs: Vec<(i32, i32)>,
    rows: Vec<usize>, // the runs in row y are runs[rows[y]..rows[y + 1]]
}

impl Clip {
    /// The mask is one byte per pixel of a w x h image; 128 and up is inside.
    pub fn new(mask: &[u8], w: usize, h: usize) -> Clip {
        let mut runs = vec![];
        let mut rows = vec![0];
        for row in mask[..w * h].chunks(w) {
            let mut start = None;
            for (x, &m) in row.iter().enumerate() {
                match (m >= 128, start) {
                    (true, None) => start = Some(x as i32),
                    (false, Some(s)) => {
                        runs.push((s, x as i32 - 1));
                        start = None;
                    }
                    _ => {}
                }
            }
            if let Some(s) = start {
                runs.push((s, w as i32 - 1));
            }
            rows.push(runs.len());
        }
        Clip { runs, rows }
    }

    /// Replaces lines with their parts inside the mask, using scratch as
    /// space for the result.
    pub fn apply(&self, lines: &mut Vec<Scanline>, scratch: &mut Vec<Scanline>) {
        scratch.clear();
        for l in lines.iter() {
            let y = l.y as usize;
            for &(a, b) in &self.runs[self.rows[y]..self.rows[y + 1]] {
                if a > l.x2 {
                    break;
                }
                if b >= l.x1 {
                    scratch.push(Scanline {
                        x1: a.max(l.x1),
                        x2: b.min(l.x2),
                        ..*l
                    });
                }
            }
        }
        std::mem::swap(lines, scratch);
    }
}

/// How much each pixel's error counts, for extra detail in some areas: a
/// weight of 1 or more per pixel.
pub struct Weights {
    pix: Vec<u8>,
    total: u64,
}

impl Weights {
    pub fn new(pix: Vec<u8>) -> Weights {
        let pix: Vec<u8> = pix.into_iter().map(|k| k.max(1)).collect();
        let total = pix.iter().map(|&k| k as u64).sum();
        Weights { pix, total }
    }

    /// Weights from a mask (one byte per pixel, 255 for full detail): 1
    /// outside it, rising to boost inside.
    pub fn from_mask(mask: &[u8], boost: u32) -> Weights {
        let extra = boost.saturating_sub(1).min(254);
        Weights::new(
            mask.iter()
                .map(|&m| (1 + (extra * m as u32 + 127) / 255) as u8)
                .collect(),
        )
    }
}

/// Root mean square difference of two images, from 0 to 1, with each
/// pixel's error counted as many times as its weight, if any.
pub fn difference_full(a: &[u8], b: &[u8], weights: Option<&Weights>) -> f64 {
    let mut total = 0u64;
    let n = match weights {
        Some(wt) => {
            for ((a, b), &k) in a
                .as_chunks::<4>()
                .0
                .iter()
                .zip(b.as_chunks::<4>().0)
                .zip(&wt.pix)
            {
                let d: u64 = (0..4)
                    .map(|c| (a[c] as i64 - b[c] as i64).pow(2) as u64)
                    .sum();
                total += d * k as u64;
            }
            wt.total * 4
        }
        None => {
            for (a, b) in a.iter().zip(b) {
                let d = *a as i32 - *b as i32;
                total += (d * d) as u64;
            }
            a.len() as u64
        }
    };
    (total as f64 / n as f64).sqrt() / 255.0
}

/// Updates score, the difference between target and before, for after,
/// which differs from before only on lines (weighted as in
/// difference_full).
#[allow(clippy::too_many_arguments)]
pub fn difference_partial(
    target: &[u8],
    before: &[u8],
    after: &[u8],
    w: usize,
    h: usize,
    score: f64,
    lines: &[Scanline],
    weights: Option<&Weights>,
) -> f64 {
    let n = weights.map_or((w * h * 4) as f64, |wt| (wt.total * 4) as f64);
    let mut total = ((score * 255.0).powi(2) * n) as i64;
    for line in lines {
        let i = offset(w, line.x1, line.y);
        let len = (line.x2 - line.x1 + 1) as usize * 4;
        let t = &target[i..i + len];
        let b = &before[i..i + len];
        let a = &after[i..i + len];
        if let Some(wt) = weights {
            let wt = &wt.pix[i / 4..i / 4 + len / 4];
            let mut sum = 0i64;
            for (p, &k) in wt.iter().enumerate() {
                let mut d = 0i32;
                for c in p * 4..p * 4 + 4 {
                    let d1 = t[c] as i32 - b[c] as i32;
                    let d2 = t[c] as i32 - a[c] as i32;
                    d += d2 * d2 - d1 * d1;
                }
                sum += d as i64 * k as i64;
            }
            total += sum;
        } else {
            let mut sum = 0i32;
            for k in 0..len {
                let d1 = t[k] as i32 - b[k] as i32;
                let d2 = t[k] as i32 - a[k] as i32;
                sum += d2 * d2 - d1 * d1;
            }
            total += sum as i64;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_keeps_the_parts_inside() {
        // row 0: inside at 2..=3 and 6..=8; row 1: nothing; row 2: everything
        let mask: Vec<u8> = [[0, 0, 255, 200, 0, 0, 128, 255, 255, 0], [0; 10], [255; 10]].concat();
        let clip = Clip::new(&mask, 10, 3);
        let line = |y, x1, x2| Scanline {
            y,
            x1,
            x2,
            alpha: 0xffff,
        };
        let mut lines = vec![line(0, 0, 9), line(0, 3, 6), line(1, 0, 9), line(2, 4, 5)];
        clip.apply(&mut lines, &mut vec![]);
        assert_eq!(
            lines,
            [
                line(0, 2, 3),
                line(0, 6, 8),
                line(0, 3, 3),
                line(0, 6, 6),
                line(2, 4, 5)
            ]
        );
    }

    #[test]
    fn weights_from_a_mask() {
        let w = Weights::from_mask(&[0, 128, 255], 9);
        assert_eq!(w.pix, [1, 5, 9]);
        assert_eq!(w.total, 15);
        assert_eq!(Weights::from_mask(&[255], 1).pix, [1]);
    }
}
