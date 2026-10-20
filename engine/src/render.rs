//! Drawing the model's shapes at any size, anti-aliased, for PNG export
//! and animation frames.

use crate::core::{self, Scanline};
use crate::model::Model;
use crate::raster::Rasterizer;

/// Renders the model's shapes cumulatively.
pub struct Animator {
    pub width: usize,
    pub height: usize,
    scale: f64,
    pub pix: Vec<u8>,
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
        let mut pix = vec![0u8; width * height * 4];
        for p in pix.as_chunks_mut::<4>().0 {
            p.copy_from_slice(&[bg.r as u8, bg.g as u8, bg.b as u8, 255]);
        }
        Animator {
            width,
            height,
            scale,
            pix,
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
            core::draw_lines(&mut self.pix, self.width, color, &self.lines);
            self.drawn += 1;
        }
        &self.pix
    }
}
