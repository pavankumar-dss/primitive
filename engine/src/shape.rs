//! The shapes, ported from the original Go package: random creation,
//! mutation, rasterizing at working resolution, outlines for rendering at
//! any size, SVG, and a numeric encoding for passing shapes between models.

use crate::constraints::{Constraints, MIN_ORIENTED_ASPECT};
use crate::core::Scanline;
use crate::raster::{self, Rasterizer};
use crate::rng::Rng;
use std::fmt::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeType {
    Any = 0,
    Triangle,
    Rectangle,
    Ellipse,
    Circle,
    RotatedRectangle,
    Quadratic,
    RotatedEllipse,
    Polygon,
}

impl ShapeType {
    pub fn from_i32(t: i32) -> ShapeType {
        use ShapeType::*;
        [
            Any,
            Triangle,
            Rectangle,
            Ellipse,
            Circle,
            RotatedRectangle,
            Quadratic,
            RotatedEllipse,
            Polygon,
        ]
        .get(t as usize)
        .copied()
        .unwrap_or(Any)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Triangle {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        x3: i32,
        y3: i32,
    },
    Rectangle {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
    },
    Ellipse {
        x: i32,
        y: i32,
        rx: i32,
        ry: i32,
        circle: bool,
    },
    RotatedRectangle {
        x: i32,
        y: i32,
        sx: i32,
        sy: i32,
        angle: i32,
    },
    Quadratic {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x3: f64,
        y3: f64,
        width: f64,
    },
    RotatedEllipse {
        x: f64,
        y: f64,
        rx: f64,
        ry: f64,
        angle: f64,
    },
    Polygon {
        convex: bool,
        x: [f64; 4],
        y: [f64; 4],
    },
}

/// What creating and mutating shapes needs to know.
pub struct Env<'a> {
    pub w: i32,
    pub h: i32,
    pub rnd: &'a mut Rng,
    pub constraints: Option<&'a Constraints>,
}

impl Env<'_> {
    fn direction(&self, x: f64, y: f64) -> Option<f64> {
        self.constraints.and_then(|c| c.direction(x, y))
    }

    fn oriented(&self) -> bool {
        self.constraints.is_some_and(|c| c.oriented())
    }

    /// A step of a normally distributed size, truncated like Go's int().
    fn step(&mut self, scale: f64) -> i32 {
        (self.rnd.norm() * scale) as i32
    }
}

/// Points of a triangle or polygon may lie this far outside the image.
const MARGIN: i32 = 16;

/// Curve widths in working pixels, from the half pixel that Go always used
/// (its width mutation was never reached) to a broad brush stroke. Wider
/// curves still score a little better, but look like blobs rather than
/// strokes.
const MIN_WIDTH: f64 = 0.5;
const MAX_WIDTH: f64 = 8.0;

impl Shape {
    pub fn random(t: ShapeType, e: &mut Env) -> Shape {
        let (w, h) = (e.w, e.h);
        let mut s = match t {
            ShapeType::Any => {
                let t = ShapeType::from_i32(e.rnd.intn(8) + 1);
                return Shape::random(t, e);
            }
            ShapeType::Triangle => {
                let (x1, y1) = (e.rnd.intn(w), e.rnd.intn(h));
                let (x2, y2) = (x1 + e.rnd.intn(31) - 15, y1 + e.rnd.intn(31) - 15);
                let (x3, y3) = (x1 + e.rnd.intn(31) - 15, y1 + e.rnd.intn(31) - 15);
                Shape::Triangle {
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                }
            }
            ShapeType::Rectangle => {
                let (x1, y1) = (e.rnd.intn(w), e.rnd.intn(h));
                let x2 = (x1 + e.rnd.intn(32) + 1).clamp(0, w - 1);
                let y2 = (y1 + e.rnd.intn(32) + 1).clamp(0, h - 1);
                return Shape::Rectangle { x1, y1, x2, y2 };
            }
            ShapeType::Ellipse => {
                let (x, y) = (e.rnd.intn(w), e.rnd.intn(h));
                let (rx, ry) = (e.rnd.intn(32) + 1, e.rnd.intn(32) + 1);
                return Shape::Ellipse {
                    x,
                    y,
                    rx,
                    ry,
                    circle: false,
                };
            }
            ShapeType::Circle => {
                let (x, y) = (e.rnd.intn(w), e.rnd.intn(h));
                let r = e.rnd.intn(32) + 1;
                return Shape::Ellipse {
                    x,
                    y,
                    rx: r,
                    ry: r,
                    circle: true,
                };
            }
            ShapeType::RotatedRectangle => {
                let (x, y) = (e.rnd.intn(w), e.rnd.intn(h));
                let (sx, sy) = (e.rnd.intn(32) + 1, e.rnd.intn(32) + 1);
                let angle = e.rnd.intn(360);
                Shape::RotatedRectangle {
                    x,
                    y,
                    sx,
                    sy,
                    angle,
                }
            }
            ShapeType::Quadratic => {
                let x1 = e.rnd.float() * w as f64;
                let y1 = e.rnd.float() * h as f64;
                let x2 = x1 + e.rnd.float() * 40.0 - 20.0;
                let y2 = y1 + e.rnd.float() * 40.0 - 20.0;
                let x3 = x2 + e.rnd.float() * 40.0 - 20.0;
                let y3 = y2 + e.rnd.float() * 40.0 - 20.0;
                let width = MIN_WIDTH + e.rnd.float() * (MAX_WIDTH - MIN_WIDTH);
                Shape::Quadratic {
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                    width,
                }
            }
            ShapeType::RotatedEllipse => {
                let x = e.rnd.float() * w as f64;
                let y = e.rnd.float() * h as f64;
                let rx = e.rnd.float() * 32.0 + 1.0;
                let ry = e.rnd.float() * 32.0 + 1.0;
                let angle = e.rnd.float() * 360.0;
                let mut s = Shape::RotatedEllipse {
                    x,
                    y,
                    rx,
                    ry,
                    angle,
                };
                s.orient(e);
                return s;
            }
            ShapeType::Polygon => {
                let mut x = [0.0; 4];
                let mut y = [0.0; 4];
                x[0] = e.rnd.float() * w as f64;
                y[0] = e.rnd.float() * h as f64;
                for i in 1..4 {
                    x[i] = x[0] + e.rnd.float() * 40.0 - 20.0;
                    y[i] = y[0] + e.rnd.float() * 40.0 - 20.0;
                }
                Shape::Polygon {
                    convex: false,
                    x,
                    y,
                }
            }
        };
        s.mutate(e);
        s
    }

    pub fn mutate(&mut self, e: &mut Env) {
        let (w, h) = (e.w, e.h);
        let m = MARGIN;
        match self {
            Shape::Triangle { .. } if e.oriented() => self.mutate_arrowhead(e),
            Shape::Triangle {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => loop {
                let (x, y) = match e.rnd.intn(3) {
                    0 => (&mut *x1, &mut *y1),
                    1 => (&mut *x2, &mut *y2),
                    _ => (&mut *x3, &mut *y3),
                };
                *x = (*x + e.step(16.0)).clamp(-m, w - 1 + m);
                *y = (*y + e.step(16.0)).clamp(-m, h - 1 + m);
                if triangle_angles_valid(*x1, *y1, *x2, *y2, *x3, *y3) {
                    break;
                }
            },
            Shape::Rectangle { x1, y1, x2, y2 } => {
                let (x, y) = if e.rnd.intn(2) == 0 {
                    (x1, y1)
                } else {
                    (x2, y2)
                };
                *x = (*x + e.step(16.0)).clamp(0, w - 1);
                *y = (*y + e.step(16.0)).clamp(0, h - 1);
            }
            Shape::Ellipse {
                x,
                y,
                rx,
                ry,
                circle,
            } => match e.rnd.intn(3) {
                0 => {
                    *x = (*x + e.step(16.0)).clamp(0, w - 1);
                    *y = (*y + e.step(16.0)).clamp(0, h - 1);
                }
                1 => {
                    *rx = (*rx + e.step(16.0)).clamp(1, w - 1);
                    if *circle {
                        *ry = *rx;
                    }
                }
                _ => {
                    *ry = (*ry + e.step(16.0)).clamp(1, h - 1);
                    if *circle {
                        *rx = *ry;
                    }
                }
            },
            Shape::RotatedRectangle {
                x,
                y,
                sx,
                sy,
                angle,
            } => {
                match e.rnd.intn(3) {
                    0 => {
                        *x = (*x + e.step(16.0)).clamp(0, w - 1);
                        *y = (*y + e.step(16.0)).clamp(0, h - 1);
                    }
                    1 => {
                        *sx = (*sx + e.step(16.0)).clamp(1, w - 1);
                        *sy = (*sy + e.step(16.0)).clamp(1, h - 1);
                    }
                    _ => *angle += e.step(32.0),
                }
                if let Some(mut a) = e.direction(*x as f64, *y as f64) {
                    // keep it elongated so that its direction is visible
                    if *sx >= *sy {
                        *sy = (*sy).min(1.max((*sx as f64 / MIN_ORIENTED_ASPECT) as i32));
                    } else {
                        *sx = (*sx).min(1.max((*sy as f64 / MIN_ORIENTED_ASPECT) as i32));
                        a += 90.0;
                    }
                    *angle = a.round() as i32;
                }
            }
            Shape::Quadratic { .. } => loop {
                if let Shape::Quadratic {
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                    width,
                } = self
                {
                    let (x, y) = match e.rnd.intn(4) {
                        0 => (x1, y1),
                        1 => (x2, y2),
                        2 => (x3, y3),
                        _ => {
                            // the points stay put, so the curve stays valid
                            *width = (*width + e.rnd.norm()).clamp(MIN_WIDTH, MAX_WIDTH);
                            break;
                        }
                    };
                    *x = (*x + e.rnd.norm() * 16.0).clamp(-m as f64, (w - 1 + m) as f64);
                    *y = (*y + e.rnd.norm() * 16.0).clamp(-m as f64, (h - 1 + m) as f64);
                }
                self.orient(e);
                if self.quadratic_valid() {
                    break;
                }
            },
            Shape::RotatedEllipse { .. } => {
                if let Shape::RotatedEllipse {
                    x,
                    y,
                    rx,
                    ry,
                    angle,
                } = self
                {
                    match e.rnd.intn(3) {
                        0 => {
                            *x = (*x + e.rnd.norm() * 16.0).clamp(0.0, (w - 1) as f64);
                            *y = (*y + e.rnd.norm() * 16.0).clamp(0.0, (h - 1) as f64);
                        }
                        1 => {
                            *rx = (*rx + e.rnd.norm() * 16.0).clamp(1.0, (w - 1) as f64);
                            *ry = (*ry + e.rnd.norm() * 16.0).clamp(1.0, (w - 1) as f64);
                        }
                        _ => *angle += e.rnd.norm() * 32.0,
                    }
                }
                self.orient(e);
            }
            Shape::Polygon { convex, x, y } => loop {
                if e.rnd.float() < 0.25 {
                    let i = e.rnd.intn(4) as usize;
                    let j = e.rnd.intn(4) as usize;
                    x.swap(i, j);
                    y.swap(i, j);
                } else {
                    let i = e.rnd.intn(4) as usize;
                    x[i] = (x[i] + e.rnd.norm() * 16.0).clamp(-m as f64, (w - 1 + m) as f64);
                    y[i] = (y[i] + e.rnd.norm() * 16.0).clamp(-m as f64, (h - 1 + m) as f64);
                }
                if !*convex || polygon_convex(x, y) {
                    break;
                }
            },
        }
    }

    /// Mutates a triangle under a direction constraint. Triangles are then
    /// arrowheads: isosceles, with the apex pointing along the direction,
    /// and at least MIN_ORIENTED_ASPECT times as long as the base is wide.
    /// Moving a vertex would rarely leave an arrowhead, so instead a move
    /// changes the centroid, the length or the aspect, and the vertices
    /// are rebuilt from those on whole pixels. Rounding leaves the apex off
    /// by up to asin(1.42 / length): 4 degrees at 20 pixels long.
    fn mutate_arrowhead(&mut self, e: &mut Env) {
        const MIN_LENGTH: f64 = 4.0;
        // an aspect over 3.8 would make the apex sharper than the 15
        // degrees all triangles keep; this leaves room for rounding
        const MAX_ASPECT: f64 = 3.6;
        let Shape::Triangle {
            x1,
            y1,
            x2,
            y2,
            x3,
            y3,
        } = *self
        else {
            unreachable!()
        };
        let p = [(x1, y1), (x2, y2), (x3, y3)].map(|(x, y)| (x as f64, y as f64));
        let k = triangle_apex(&p);
        let (a, b, c) = (p[k], p[(k + 1) % 3], p[(k + 2) % 3]);
        let (cx, cy) = ((a.0 + b.0 + c.0) / 3.0, (a.1 + b.1 + c.1) / 3.0);
        let (mx, my) = ((b.0 + c.0) / 2.0, (b.1 + c.1) / 2.0);
        let len = (a.0 - mx).hypot(a.1 - my);
        let aspect = len / (c.0 - b.0).hypot(c.1 - b.1).max(1.0);
        let (w, h) = ((e.w - 1) as f64, (e.h - 1) as f64);
        loop {
            let (mut cx, mut cy, mut len, mut aspect) = (cx, cy, len, aspect);
            match e.rnd.intn(3) {
                0 => {
                    cx = (cx + e.rnd.norm() * 16.0).clamp(0.0, w);
                    cy = (cy + e.rnd.norm() * 16.0).clamp(0.0, h);
                }
                1 => len += e.rnd.norm() * 16.0,
                _ => aspect += e.rnd.norm() * 0.5,
            }
            let len = len.clamp(MIN_LENGTH, w.max(h));
            let half = len / aspect.clamp(MIN_ORIENTED_ASPECT, MAX_ASPECT) / 2.0;
            let (sin, cos) = e.direction(cx, cy).unwrap_or(0.0).to_radians().sin_cos();
            // a point this far along the direction and across it from the centroid
            let at = |along: f64, across: f64| {
                (
                    (cx + along * cos - across * sin).round() as i32,
                    (cy + along * sin + across * cos).round() as i32,
                )
            };
            let ((x1, y1), (x2, y2), (x3, y3)) = (
                at(len * 2.0 / 3.0, 0.0),
                at(-len / 3.0, half),
                at(-len / 3.0, -half),
            );
            let s = Shape::Triangle {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            };
            if s.triangle_valid(e) {
                *self = s;
                return;
            }
        }
    }

    /// Applies the direction constraint, if any, to rotated ellipses and
    /// curves.
    fn orient(&mut self, e: &Env) {
        match self {
            Shape::RotatedEllipse {
                x,
                y,
                rx,
                ry,
                angle,
            } => {
                let Some(mut a) = e.direction(*x, *y) else {
                    return;
                };
                if *rx >= *ry {
                    *ry = ry.min(1f64.max(*rx / MIN_ORIENTED_ASPECT));
                } else {
                    *rx = rx.min(1f64.max(*ry / MIN_ORIENTED_ASPECT));
                    a += 90.0;
                }
                *angle = a;
            }
            Shape::Quadratic {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
                ..
            } => {
                // rotate about the middle of the chord so that the chord
                // follows the direction
                let (mx, my) = ((*x1 + *x3) / 2.0, (*y1 + *y3) / 2.0);
                let Some(a) = e.direction(mx, my) else { return };
                let d = a.to_radians() - (*y3 - *y1).atan2(*x3 - *x1);
                for (x, y) in [(x1, y1), (x2, y2), (x3, y3)] {
                    let (rx, ry) = rotate(*x - mx, *y - my, d);
                    *x = rx + mx;
                    *y = ry + my;
                }
            }
            _ => {}
        }
    }

    /// Whether a triangle has no angle under 15 degrees and, under a
    /// direction constraint, is an arrowhead at least MIN_ORIENTED_ASPECT
    /// times as long, from apex to base, as its base is wide.
    fn triangle_valid(&self, e: &Env) -> bool {
        let Shape::Triangle {
            x1,
            y1,
            x2,
            y2,
            x3,
            y3,
        } = *self
        else {
            return true;
        };
        if !triangle_angles_valid(x1, y1, x2, y2, x3, y3) {
            return false;
        }
        if !e.oriented() {
            return true;
        }
        let p = [(x1, y1), (x2, y2), (x3, y3)].map(|(x, y)| (x as f64, y as f64));
        let k = triangle_apex(&p);
        let (a, b, c) = (p[k], p[(k + 1) % 3], p[(k + 2) % 3]);
        // twice the area is the base times the length
        let area2 = ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs();
        let base2 = (c.0 - b.0).powi(2) + (c.1 - b.1).powi(2);
        area2 >= MIN_ORIENTED_ASPECT * base2
    }

    fn quadratic_valid(&self) -> bool {
        let Shape::Quadratic {
            x1,
            y1,
            x2,
            y2,
            x3,
            y3,
            ..
        } = *self
        else {
            return true;
        };
        let d = |ax: f64, ay: f64, bx: f64, by: f64| {
            let (dx, dy) = ((ax - bx) as i64, (ay - by) as i64);
            dx * dx + dy * dy
        };
        let (d12, d23, d13) = (d(x1, y1, x2, y2), d(x2, y2, x3, y3), d(x1, y1, x3, y3));
        d13 > d12 && d13 > d23
    }

    /// The pixels the shape covers in a w x h image, into lines.
    pub fn rasterize(&self, w: i32, h: i32, r: &mut Rasterizer, lines: &mut Vec<Scanline>) {
        lines.clear();
        match *self {
            Shape::Triangle {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => {
                let p = |x: i32, y: i32| (x as f64, y as f64);
                raster::sample_polygon(&[p(x1, y1), p(x2, y2), p(x3, y3)], w, h, lines);
            }
            Shape::Rectangle { .. } => {
                let (x1, y1, x2, y2) = self.rectangle_bounds();
                for y in y1..=y2 {
                    lines.push(Scanline {
                        y,
                        x1,
                        x2,
                        alpha: 0xffff,
                    });
                }
            }
            Shape::Ellipse { x, y, rx, ry, .. } => {
                let aspect = rx as f64 / ry as f64;
                for dy in 0..ry {
                    let (y1, y2) = (y - dy, y + dy);
                    if (y1 < 0 || y1 >= h) && (y2 < 0 || y2 >= h) {
                        continue;
                    }
                    let s = (((ry * ry - dy * dy) as f64).sqrt() * aspect) as i32;
                    let x1 = (x - s).max(0);
                    let x2 = (x + s).min(w - 1);
                    if y1 >= 0 && y1 < h {
                        lines.push(Scanline {
                            y: y1,
                            x1,
                            x2,
                            alpha: 0xffff,
                        });
                    }
                    if y2 >= 0 && y2 < h && dy > 0 {
                        lines.push(Scanline {
                            y: y2,
                            x1,
                            x2,
                            alpha: 0xffff,
                        });
                    }
                }
            }
            Shape::RotatedRectangle { .. } => {
                raster::sample_polygon(&self.rotated_rectangle_corners(), w, h, lines);
            }
            Shape::RotatedEllipse {
                x,
                y,
                rx,
                ry,
                angle,
            } => rasterize_rotated_ellipse(x, y, rx, ry, angle, w, h, lines),
            Shape::Polygon { x, y, .. } => {
                let points = [(x[0], y[0]), (x[1], y[1]), (x[2], y[2]), (x[3], y[3])];
                raster::sample_polygon(&points, w, h, lines);
            }
            Shape::Quadratic { .. } => {
                // Curves can be half a pixel wide, too thin to search without
                // anti-aliasing. Offset 0.5 as in rendering centers the
                // shape's point (x, y) on pixel (x, y); Go used 0, off by
                // half a pixel.
                self.outline(r, 1.0, 0.5);
                r.fill(w, h, lines);
            }
        }
    }

    fn rotated_rectangle_corners(&self) -> [(f64, f64); 4] {
        let Shape::RotatedRectangle {
            x,
            y,
            sx,
            sy,
            angle,
        } = *self
        else {
            unreachable!()
        };
        let (sx, sy, a) = (
            sx as f64 / 2.0,
            sy as f64 / 2.0,
            (angle as f64).to_radians(),
        );
        [(-sx, -sy), (sx, -sy), (sx, sy), (-sx, sy)].map(|(dx, dy)| {
            let (rx, ry) = rotate(dx, dy, a);
            (x as f64 + rx, y as f64 + ry)
        })
    }

    fn rectangle_bounds(&self) -> (i32, i32, i32, i32) {
        let Shape::Rectangle { x1, y1, x2, y2 } = *self else {
            unreachable!()
        };
        (x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2))
    }

    /// Adds the shape's outline to r, mapping each point p to
    /// (p + offset) * scale; rendering uses offset 0.5, like the SVG.
    pub fn outline(&self, r: &mut Rasterizer, scale: f64, offset: f64) {
        let t = |x: f64, y: f64| ((x + offset) * scale, (y + offset) * scale);
        match *self {
            Shape::Triangle {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => {
                r.polygon(&[
                    t(x1 as f64, y1 as f64),
                    t(x2 as f64, y2 as f64),
                    t(x3 as f64, y3 as f64),
                ]);
            }
            Shape::Rectangle { .. } => {
                // pixels x1 to x2 inclusive, centered on their coordinates
                let (x1, y1, x2, y2) = self.rectangle_bounds();
                let (x1, y1) = (x1 as f64 - 0.5, y1 as f64 - 0.5);
                let (x2, y2) = (x2 as f64 + 0.5, y2 as f64 + 0.5);
                r.polygon(&[t(x1, y1), t(x2, y1), t(x2, y2), t(x1, y2)]);
            }
            Shape::Ellipse { x, y, rx, ry, .. } => {
                let (cx, cy) = t(x as f64, y as f64);
                r.ellipse(cx, cy, rx as f64 * scale, ry as f64 * scale, 0.0);
            }
            Shape::RotatedRectangle { .. } => {
                let c = self.rotated_rectangle_corners();
                r.polygon(&c.map(|(x, y)| t(x, y)));
            }
            Shape::Quadratic {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
                width,
            } => {
                r.stroke_quadratic(t(x1, y1), t(x2, y2), t(x3, y3), width * scale);
            }
            Shape::RotatedEllipse {
                x,
                y,
                rx,
                ry,
                angle,
            } => {
                let (cx, cy) = t(x, y);
                r.ellipse(cx, cy, rx * scale, ry * scale, angle.to_radians());
            }
            Shape::Polygon { x, y, .. } => {
                r.polygon(&[t(x[0], y[0]), t(x[1], y[1]), t(x[2], y[2]), t(x[3], y[3])]);
            }
        }
    }

    /// The shape as an SVG element, with attrs (its fill) inserted.
    pub fn svg(&self, attrs: &str, out: &mut String) {
        let _ = match *self {
            Shape::Triangle {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => {
                write!(
                    out,
                    "<polygon {attrs} points=\"{x1},{y1} {x2},{y2} {x3},{y3}\" />"
                )
            }
            Shape::Rectangle { .. } => {
                let (x1, y1, x2, y2) = self.rectangle_bounds();
                let (w, h) = (x2 - x1 + 1, y2 - y1 + 1);
                write!(
                    out,
                    "<rect {attrs} x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" />",
                    x = x1 as f64 - 0.5,
                    y = y1 as f64 - 0.5
                )
            }
            Shape::Ellipse { x, y, rx, ry, .. } => {
                write!(
                    out,
                    "<ellipse {attrs} cx=\"{x}\" cy=\"{y}\" rx=\"{rx}\" ry=\"{ry}\" />"
                )
            }
            Shape::RotatedRectangle {
                x,
                y,
                sx,
                sy,
                angle,
            } => write!(
                out,
                "<g transform=\"translate({x} {y}) rotate({angle}) scale({sx} {sy})\"><rect {attrs} x=\"-0.5\" y=\"-0.5\" width=\"1\" height=\"1\" /></g>"
            ),
            Shape::Quadratic {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
                width,
            } => {
                let attrs = attrs.replace("fill", "stroke");
                write!(
                    out,
                    "<path {attrs} fill=\"none\" stroke-linecap=\"round\" d=\"M {x1:.6} {y1:.6} Q {x2:.6} {y2:.6}, {x3:.6} {y3:.6}\" stroke-width=\"{width:.6}\" />"
                )
            }
            Shape::RotatedEllipse {
                x,
                y,
                rx,
                ry,
                angle,
            } => write!(
                out,
                "<g transform=\"translate({x:.6} {y:.6}) rotate({angle:.6}) scale({rx:.6} {ry:.6})\"><ellipse {attrs} cx=\"0\" cy=\"0\" rx=\"1\" ry=\"1\" /></g>"
            ),
            Shape::Polygon { x, y, .. } => write!(
                out,
                "<polygon {attrs} points=\"{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}\" />",
                x[0], y[0], x[1], y[1], x[2], y[2], x[3], y[3]
            ),
        };
    }

    /// Appends the shape's type and fields, as EncodeShape in Go does.
    pub fn encode(&self, out: &mut Vec<f64>) {
        match *self {
            Shape::Triangle {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => out.extend([1, x1, y1, x2, y2, x3, y3].map(|v| v as f64)),
            Shape::Rectangle { x1, y1, x2, y2 } => {
                out.extend([2, x1, y1, x2, y2].map(|v| v as f64))
            }
            Shape::Ellipse {
                x,
                y,
                rx,
                ry,
                circle,
            } => out.extend([if circle { 4 } else { 3 }, x, y, rx, ry].map(|v| v as f64)),
            Shape::RotatedRectangle {
                x,
                y,
                sx,
                sy,
                angle,
            } => out.extend([5, x, y, sx, sy, angle].map(|v| v as f64)),
            Shape::Quadratic {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
                width,
            } => out.extend([6.0, x1, y1, x2, y2, x3, y3, width]),
            Shape::RotatedEllipse {
                x,
                y,
                rx,
                ry,
                angle,
            } => out.extend([7.0, x, y, rx, ry, angle]),
            Shape::Polygon { convex, x, y } => {
                out.extend([8.0, if convex { 1.0 } else { 0.0 }]);
                for i in 0..4 {
                    out.extend([x[i], y[i]]);
                }
            }
        }
    }

    /// Reads a shape written by encode from the start of v and returns it
    /// with the number of values used.
    pub fn decode(v: &[f64]) -> Option<(Shape, usize)> {
        let i = |k: usize| v[k] as i32;
        let t = ShapeType::from_i32(*v.first()? as i32);
        let n = match t {
            ShapeType::Any => return None,
            ShapeType::Triangle => 7,
            ShapeType::Rectangle | ShapeType::Ellipse | ShapeType::Circle => 5,
            ShapeType::RotatedRectangle | ShapeType::RotatedEllipse => 6,
            ShapeType::Quadratic => 8,
            ShapeType::Polygon => 10,
        };
        if v.len() < n {
            return None;
        }
        let s = match t {
            ShapeType::Triangle => Shape::Triangle {
                x1: i(1),
                y1: i(2),
                x2: i(3),
                y2: i(4),
                x3: i(5),
                y3: i(6),
            },
            ShapeType::Rectangle => Shape::Rectangle {
                x1: i(1),
                y1: i(2),
                x2: i(3),
                y2: i(4),
            },
            ShapeType::Ellipse | ShapeType::Circle => Shape::Ellipse {
                x: i(1),
                y: i(2),
                rx: i(3),
                ry: i(4),
                circle: t == ShapeType::Circle,
            },
            ShapeType::RotatedRectangle => Shape::RotatedRectangle {
                x: i(1),
                y: i(2),
                sx: i(3),
                sy: i(4),
                angle: i(5),
            },
            ShapeType::Quadratic => Shape::Quadratic {
                x1: v[1],
                y1: v[2],
                x2: v[3],
                y2: v[4],
                x3: v[5],
                y3: v[6],
                width: v[7],
            },
            ShapeType::RotatedEllipse => Shape::RotatedEllipse {
                x: v[1],
                y: v[2],
                rx: v[3],
                ry: v[4],
                angle: v[5],
            },
            _ => Shape::Polygon {
                convex: v[1] != 0.0,
                x: [v[2], v[4], v[6], v[8]],
                y: [v[3], v[5], v[7], v[9]],
            },
        };
        Some((s, n))
    }
}

fn rotate(x: f64, y: f64, theta: f64) -> (f64, f64) {
    let (sin, cos) = theta.sin_cos();
    (x * cos - y * sin, x * sin + y * cos)
}

/// The apex of a triangle as an arrowhead: the vertex opposite the
/// shortest side, which has the sharpest angle.
fn triangle_apex(p: &[(f64, f64); 3]) -> usize {
    let side = |k: usize| {
        let (b, c) = (p[(k + 1) % 3], p[(k + 2) % 3]);
        (c.0 - b.0).powi(2) + (c.1 - b.1).powi(2)
    };
    (0..3).min_by(|&i, &j| side(i).total_cmp(&side(j))).unwrap()
}

fn triangle_angles_valid(x1: i32, y1: i32, x2: i32, y2: i32, x3: i32, y3: i32) -> bool {
    const MIN_DEGREES: f64 = 15.0;
    let angle = |ax: i32, ay: i32, bx: i32, by: i32, cx: i32, cy: i32| {
        let (x1, y1) = ((bx - ax) as f64, (by - ay) as f64);
        let (x2, y2) = ((cx - ax) as f64, (cy - ay) as f64);
        let (d1, d2) = ((x1 * x1 + y1 * y1).sqrt(), (x2 * x2 + y2 * y2).sqrt());
        ((x1 / d1) * (x2 / d2) + (y1 / d1) * (y2 / d2))
            .acos()
            .to_degrees()
    };
    let a1 = angle(x1, y1, x2, y2, x3, y3);
    let a2 = angle(x2, y2, x1, y1, x3, y3);
    let a3 = 180.0 - a1 - a2;
    a1 > MIN_DEGREES && a2 > MIN_DEGREES && a3 > MIN_DEGREES
}

fn polygon_convex(x: &[f64; 4], y: &[f64; 4]) -> bool {
    let mut sign = false;
    for a in 0..4 {
        let (i, j, k) = (a, (a + 1) % 4, (a + 2) % 4);
        let c = (x[j] - x[i]) * (y[k] - y[j]) - (y[j] - y[i]) * (x[k] - x[j]);
        if a == 0 {
            sign = c > 0.0;
        } else if (c > 0.0) != sign {
            return false;
        }
    }
    true
}

/// The pixels whose coordinates lie inside the ellipse, one line per row.
/// Like the other shapes' search rasterizers (and unlike Go's, which
/// anti-aliased rotated ellipses), it leaves out partly covered pixels: with
/// a single run per row, scoring a candidate is several times faster. The
/// rendered shape is still anti-aliased; pixel (px, py) spans px + 0.5 ±
/// 0.5 there, so its center is inside exactly when (px, py) is here.
#[allow(clippy::too_many_arguments)]
fn rasterize_rotated_ellipse(
    x: f64,
    y: f64,
    rx: f64,
    ry: f64,
    angle: f64,
    w: i32,
    h: i32,
    lines: &mut Vec<Scanline>,
) {
    // Point (dx, dy) from the center is inside when
    // a dx^2 + b dx dy + c dy^2 <= 1; each row solves that for dx.
    let (sin, cos) = angle.to_radians().sin_cos();
    let (irx, iry) = ((rx * rx).recip(), (ry * ry).recip());
    let a = cos * cos * irx + sin * sin * iry;
    let b = 2.0 * sin * cos * (irx - iry);
    let c = sin * sin * irx + cos * cos * iry;
    let half = (rx * rx * sin * sin + ry * ry * cos * cos).sqrt();
    let y1 = ((y - half).ceil() as i32).max(0);
    let y2 = ((y + half).floor() as i32).min(h - 1);
    for py in y1..=y2 {
        let dy = py as f64 - y;
        let disc = b * b * dy * dy - 4.0 * a * (c * dy * dy - 1.0);
        if disc < 0.0 {
            continue;
        }
        let (mid, s) = (-b * dy / (2.0 * a), disc.sqrt() / (2.0 * a));
        let x1 = ((x + mid - s).ceil() as i32).max(0);
        let x2 = ((x + mid + s).floor() as i32).min(w - 1);
        if x1 <= x2 {
            lines.push(Scanline {
                y: py,
                x1,
                x2,
                alpha: 0xffff,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_round_trip() {
        let mut rnd = Rng::new(1);
        let mut e = Env {
            w: 64,
            h: 48,
            rnd: &mut rnd,
            constraints: None,
        };
        for t in 1..=8 {
            for _ in 0..20 {
                let s = Shape::random(ShapeType::from_i32(t), &mut e);
                let mut v = vec![];
                s.encode(&mut v);
                assert_eq!(Shape::decode(&v), Some((s, v.len())), "type {t}");
            }
        }
        assert_eq!(Shape::decode(&[]), None);
        assert_eq!(Shape::decode(&[1.0, 2.0]), None);
    }

    #[test]
    fn arrowheads_point_along_the_direction() {
        use crate::constraints::Options;
        let (w, h) = (64, 48);
        let toward = Options {
            focus: Some((0.25, 0.75)),
            angle: None,
            min_size: 0.0,
            max_size: 0.0,
            region: None,
        };
        let along = Options {
            focus: None,
            angle: Some(-120.0),
            ..toward
        };
        let mut rnd = Rng::new(5);
        for opts in [toward, along] {
            let c = Constraints::new(&opts, w as usize, h as usize).unwrap();
            let mut e = Env {
                w,
                h,
                rnd: &mut rnd,
                constraints: Some(&c),
            };
            for _ in 0..300 {
                let mut s = Shape::random(ShapeType::Triangle, &mut e);
                for _ in 0..10 {
                    s.mutate(&mut e);
                }
                let Shape::Triangle {
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                } = s
                else {
                    unreachable!()
                };
                assert!(s.triangle_valid(&e), "{s:?}");
                let p = [(x1, y1), (x2, y2), (x3, y3)].map(|(x, y)| (x as f64, y as f64));
                let k = triangle_apex(&p);
                let (b, c) = (p[(k + 1) % 3], p[(k + 2) % 3]);
                let (dx, dy) = (p[k].0 - (b.0 + c.0) / 2.0, p[k].1 - (b.1 + c.1) / 2.0);
                let (cx, cy) = ((x1 + x2 + x3) as f64 / 3.0, (y1 + y2 + y3) as f64 / 3.0);
                let (want, focus_distance) = match opts.focus {
                    Some(_) => ((36.0 - cy).atan2(16.0 - cx), (36.0 - cy).hypot(16.0 - cx)),
                    None => (120f64.to_radians(), f64::INFINITY),
                };
                let mut off = (dy.atan2(dx) - want).to_degrees().rem_euclid(360.0);
                if off > 180.0 {
                    off = 360.0 - off;
                }
                // Each rounded vertex is within 0.71 pixels of where it was
                // turned to, which turns the apex by up to this angle, and
                // moves the centroid, which turns the way to the focus.
                let turn = |d: f64, at: f64| (d / at).min(1.0).asin().to_degrees();
                let limit = turn(1.42, dx.hypot(dy)) + turn(0.71, focus_distance) + 1e-6;
                assert!(off <= limit, "{off} > {limit}: {s:?}");
            }
        }
    }

    #[test]
    fn curve_widths_are_searched() {
        let mut rnd = Rng::new(4);
        let mut e = Env {
            w: 64,
            h: 48,
            rnd: &mut rnd,
            constraints: None,
        };
        let mut widths = vec![];
        for _ in 0..50 {
            let mut s = Shape::random(ShapeType::Quadratic, &mut e);
            for _ in 0..20 {
                s.mutate(&mut e);
            }
            let Shape::Quadratic { width, .. } = s else {
                unreachable!()
            };
            assert!((MIN_WIDTH..=MAX_WIDTH).contains(&width), "{width}");
            widths.push(width);
        }
        assert!(widths.iter().any(|&w| w < 2.0) && widths.iter().any(|&w| w > 5.0));
    }

    #[test]
    fn rasterized_lines_stay_inside() {
        let mut rnd = Rng::new(2);
        let mut r = Rasterizer::default();
        let mut lines = vec![];
        for t in 1..=8 {
            for _ in 0..200 {
                let mut e = Env {
                    w: 64,
                    h: 48,
                    rnd: &mut rnd,
                    constraints: None,
                };
                let mut s = Shape::random(ShapeType::from_i32(t), &mut e);
                for _ in 0..20 {
                    s.mutate(&mut e);
                }
                s.rasterize(64, 48, &mut r, &mut lines);
                for l in &lines {
                    assert!(
                        l.y >= 0 && l.y < 48 && l.x1 >= 0 && l.x1 <= l.x2 && l.x2 < 64,
                        "type {t}: {l:?} from {s:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn rotated_ellipse_covers_the_points_inside() {
        let mut rnd = Rng::new(3);
        let mut r = Rasterizer::default();
        let mut lines = vec![];
        for _ in 0..500 {
            let (x, y) = (rnd.float() * 80.0 - 8.0, rnd.float() * 60.0 - 6.0);
            let (rx, ry) = (rnd.float() * 30.0 + 1.0, rnd.float() * 30.0 + 1.0);
            let angle = rnd.float() * 720.0 - 360.0;
            let s = Shape::RotatedEllipse {
                x,
                y,
                rx,
                ry,
                angle,
            };
            s.rasterize(64, 48, &mut r, &mut lines);
            let (sin, cos) = angle.to_radians().sin_cos();
            for py in 0..48 {
                for px in 0..64 {
                    let (dx, dy) = (px as f64 - x, py as f64 - y);
                    let (u, v) = (dx * cos + dy * sin, dy * cos - dx * sin);
                    let d = (u / rx).powi(2) + (v / ry).powi(2);
                    if (d - 1.0).abs() < 1e-9 {
                        continue; // on the edge: either answer is fine
                    }
                    let covered = lines.iter().any(|l| l.y == py && l.x1 <= px && px <= l.x2);
                    assert_eq!(covered, d < 1.0, "({px}, {py}) in {s:?}");
                }
            }
        }
    }
}
