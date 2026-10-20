//! Drawing the model's shapes at any size, anti-aliased, for PNG export
//! and animation frames.

use crate::core::{self, Scanline};
use crate::model::Model;
use crate::raster::Rasterizer;

/// An image shown around the region shapes are kept to (see
/// Model::keep_outside), at any size: RGBA pixels, and one mask byte per
/// pixel, 255 inside the region.
pub struct Base {
    pub w: usize,
    pub h: usize,
    pub pix: Vec<u8>,
    pub mask: Vec<u8>,
}

/// Renders the model's shapes cumulatively.
pub struct Animator {
    pub width: usize,
    pub height: usize,
    scale: f64,
    pub pix: Vec<u8>,
    mask: Option<Vec<u8>>,
    drawn: usize,
    raster: Rasterizer,
    lines: Vec<Scanline>,
}

impl Animator {
    /// An Animator whose frames fit within max_side x max_side. If even is
    /// set, both dimensions are rounded down to even numbers, as most video
    /// encoders require.
    pub fn new(model: &Model, max_side: usize, even: bool) -> Animator {
        let s = max_side as f64 / model.sw.max(model.sh) as f64;
        let mut width = 2.max((model.sw as f64 * s) as usize);
        let mut height = 2.max((model.sh as f64 * s) as usize);
        if even {
            width &= !1;
            height &= !1;
        }
        let scale = model.scale * width as f64 / model.sw as f64;
        let bg = model.background;
        let bg = [bg.r as u8, bg.g as u8, bg.b as u8, 255];
        let mut pix = vec![0u8; width * height * 4];
        let mut mask = None;
        if let Some(base) = &model.base {
            // the base outside the region, the background inside
            let b = resample(&base.pix, base.w, base.h, 4, width, height);
            let m = resample(&base.mask, base.w, base.h, 1, width, height);
            for ((p, b), &m) in pix
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(b.as_chunks::<4>().0)
                .zip(&m)
            {
                for c in 0..3 {
                    p[c] = ((b[c] as u32 * (255 - m as u32) + bg[c] as u32 * m as u32 + 127) / 255)
                        as u8;
                }
                p[3] = 255;
            }
            mask = Some(m);
        } else {
            for p in pix.as_chunks_mut::<4>().0 {
                p.copy_from_slice(&bg);
            }
        }
        Animator {
            width,
            height,
            scale,
            pix,
            mask,
            drawn: 0,
            raster: Rasterizer::default(),
            lines: vec![],
        }
    }

    /// Draws shapes until count are visible and returns the frame's RGBA
    /// pixels. Counts must not decrease between calls.
    pub fn frame(&mut self, model: &Model, count: usize) -> &[u8] {
        while self.drawn < count.min(model.shapes.len()) {
            let (shape, color) = (model.shapes[self.drawn], model.colors[self.drawn]);
            shape.outline(&mut self.raster, self.scale, 0.5);
            self.raster
                .fill(self.width as i32, self.height as i32, &mut self.lines);
            match &self.mask {
                Some(m) => {
                    core::draw_lines_masked(&mut self.pix, self.width, color, &self.lines, m)
                }
                None => core::draw_lines(&mut self.pix, self.width, color, &self.lines),
            }
            self.drawn += 1;
        }
        &self.pix
    }
}

/// Bilinear resampling of a w x h image with ch bytes per pixel to dw x dh.
fn resample(src: &[u8], w: usize, h: usize, ch: usize, dw: usize, dh: usize) -> Vec<u8> {
    let mut out = vec![0u8; dw * dh * ch];
    // the source position of a destination pixel's center, and its neighbors
    let at = |d: usize, dn: usize, n: usize| {
        let f = ((d as f64 + 0.5) * n as f64 / dn as f64 - 0.5).clamp(0.0, (n - 1) as f64);
        let i = f as usize;
        (i, (i + 1).min(n - 1), f - i as f64)
    };
    for y in 0..dh {
        let (y0, y1, ty) = at(y, dh, h);
        for x in 0..dw {
            let (x0, x1, tx) = at(x, dw, w);
            for c in 0..ch {
                let p = |x: usize, y: usize| src[(y * w + x) * ch + c] as f64;
                let top = p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx;
                let bottom = p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx;
                out[(y * dw + x) * ch + c] = (top * (1.0 - ty) + bottom * ty).round() as u8;
            }
        }
    }
    out
}
