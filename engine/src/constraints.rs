//! Restrictions on the shapes the search may choose.

use crate::core::Scanline;

/// The fraction of a shape allowed outside the region.
const REGION_TOLERANCE: f64 = 0.1;

/// The smallest length-to-width ratio of rectangles, ellipses and
/// arrowhead triangles under a direction constraint; rounder shapes show
/// no direction.
pub const MIN_ORIENTED_ASPECT: f64 = 2.0;

#[derive(Clone, Default)]
pub struct Constraints {
    orient: bool,
    focus: bool,
    fx: f64,
    fy: f64,
    angle: f64, // degrees, clockwise (image y points down)
    min_size: f64,
    max_size: f64,
    region: Option<Vec<i32>>, // per-row prefix sums of allowed pixels, (w+1) per row
}

/// Constraints as the page passes them: focus and region are optional,
/// focus in fractions of the image size, angle in degrees counterclockwise,
/// sizes in fractions of the image's longest side (0 for no limit), and
/// region one byte per pixel (128 and up is allowed).
pub struct Options<'a> {
    pub focus: Option<(f64, f64)>,
    pub angle: Option<f64>,
    pub min_size: f64,
    pub max_size: f64,
    pub region: Option<&'a [u8]>,
}

impl Constraints {
    /// Resolves options for a w x h image; None if they restrict nothing.
    pub fn new(o: &Options, w: usize, h: usize) -> Option<Constraints> {
        let long = w.max(h) as f64;
        let mut c = Constraints {
            orient: o.focus.is_some() || o.angle.is_some(),
            focus: o.focus.is_some(),
            angle: -o.angle.unwrap_or(0.0),
            min_size: o.min_size * long,
            max_size: o.max_size * long,
            ..Default::default()
        };
        if let Some((fx, fy)) = o.focus {
            c.fx = fx * w as f64;
            c.fy = fy * h as f64;
        }
        if let Some(region) = o.region.filter(|r| r.len() >= w * h) {
            let mut sums = vec![0i32; (w + 1) * h];
            for y in 0..h {
                let row = &mut sums[y * (w + 1)..(y + 1) * (w + 1)];
                for x in 0..w {
                    row[x + 1] = row[x] + (region[y * w + x] >= 128) as i32;
                }
            }
            c.region = Some(sums);
        }
        if !c.orient && c.min_size == 0.0 && c.max_size == 0.0 && c.region.is_none() {
            return None;
        }
        Some(c)
    }

    /// Whether a direction is set.
    pub fn oriented(&self) -> bool {
        self.orient
    }

    /// The angle in degrees that the long axis of an oriented shape
    /// centered at (x, y) must have, if a direction is set. Arrowheads
    /// (triangles) point this way, toward the focus.
    pub fn direction(&self, x: f64, y: f64) -> Option<f64> {
        if !self.orient {
            None
        } else if self.focus {
            Some((self.fy - y).atan2(self.fx - x).to_degrees())
        } else {
            Some(self.angle)
        }
    }

    /// How far rasterized lines break the size and region constraints; 0
    /// means the shape is allowed.
    pub fn violation(&self, lines: &[Scanline], w: usize) -> f64 {
        if self.min_size == 0.0 && self.max_size == 0.0 && self.region.is_none() {
            return 0.0;
        }
        let Some(first) = lines.first() else {
            return 1.0;
        };
        let mut v = 0.0;
        if self.min_size > 0.0 || self.max_size > 0.0 {
            let (mut x0, mut y0, mut x1, mut y1) = (first.x1, first.y, first.x2, first.y);
            for l in &lines[1..] {
                x0 = x0.min(l.x1);
                x1 = x1.max(l.x2);
                y0 = y0.min(l.y);
                y1 = y1.max(l.y);
            }
            let size = ((x1 - x0).max(y1 - y0) + 1) as f64;
            if self.min_size > 0.0 && size < self.min_size {
                v += (self.min_size - size) / self.min_size;
            }
            if self.max_size > 0.0 && size > self.max_size {
                v += (size - self.max_size) / self.max_size;
            }
        }
        if let Some(region) = &self.region {
            let stride = w + 1;
            let (mut inside, mut total) = (0i64, 0i64);
            for l in lines {
                let row = &region[l.y as usize * stride..];
                inside += (row[l.x2 as usize + 1] - row[l.x1 as usize]) as i64;
                total += (l.x2 - l.x1 + 1) as i64;
            }
            let outside = 1.0 - inside as f64 / total as f64;
            if outside > REGION_TOLERANCE {
                v += outside;
            }
        }
        v
    }
}
