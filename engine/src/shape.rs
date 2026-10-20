//! The shapes, ported from the Go package: random creation, mutation,
//! rasterizing at working resolution, outlines for rendering at any size,
//! SVG, and the numeric encoding shared with the Go version.

use crate::constraints::{Constraints, MIN_ORIENTED_ASPECT};
use crate::core::Scanline;
use crate::raster::Rasterizer;
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

    /// A step of a normally distributed size, truncated like Go's int().
    fn step(&mut self, scale: f64) -> i32 {
        (self.rnd.norm() * scale) as i32
    }
}

/// Points of a triangle or polygon may lie this far outside the image.
const MARGIN: i32 = 16;

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
                Shape::Quadratic {
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                    width: 0.5,
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
                if triangle_valid(*x1, *y1, *x2, *y2, *x3, *y3) {
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
                    ..
                } = self
                {
                    let (x, y) = match e.rnd.intn(3) {
                        0 => (x1, y1),
                        1 => (x2, y2),
                        _ => (x3, y3),
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
                rasterize_triangle(x1, y1, x2, y2, x3, y3, lines);
                crop_scanlines(lines, w, h);
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
            Shape::RotatedRectangle {
                x,
                y,
                sx,
                sy,
                angle,
            } => rasterize_rotated_rectangle(x, y, sx, sy, angle, w, h, lines),
            _ => {
                self.outline(r, 1.0, 0.0);
                r.fill(w, h, lines);
            }
        }
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
                let (x1, y1, x2, y2) = self.rectangle_bounds();
                let (x1, y1, x2, y2) = (x1 as f64, y1 as f64, (x2 + 1) as f64, (y2 + 1) as f64);
                r.polygon(&[t(x1, y1), t(x2, y1), t(x2, y2), t(x1, y2)]);
            }
            Shape::Ellipse { x, y, rx, ry, .. } => {
                let (cx, cy) = t(x as f64, y as f64);
                r.ellipse(cx, cy, rx as f64 * scale, ry as f64 * scale, 0.0);
            }
            Shape::RotatedRectangle {
                x,
                y,
                sx,
                sy,
                angle,
            } => {
                let (sx, sy, a) = (
                    sx as f64 / 2.0,
                    sy as f64 / 2.0,
                    (angle as f64).to_radians(),
                );
                let p = |dx: f64, dy: f64| {
                    let (rx, ry) = rotate(dx, dy, a);
                    t(x as f64 + rx, y as f64 + ry)
                };
                r.polygon(&[p(-sx, -sy), p(sx, -sy), p(sx, sy), p(-sx, sy)]);
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
                    "<rect {attrs} x=\"{x1}\" y=\"{y1}\" width=\"{w}\" height=\"{h}\" />"
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
                    "<path {attrs} fill=\"none\" d=\"M {x1:.6} {y1:.6} Q {x2:.6} {y2:.6}, {x3:.6} {y3:.6}\" stroke-width=\"{width:.6}\" />"
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

fn triangle_valid(x1: i32, y1: i32, x2: i32, y2: i32, x3: i32, y3: i32) -> bool {
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

fn crop_scanlines(lines: &mut Vec<Scanline>, w: i32, h: i32) {
    lines.retain_mut(|l| {
        if l.y < 0 || l.y >= h || l.x1 >= w || l.x2 < 0 {
            return false;
        }
        l.x1 = l.x1.clamp(0, w - 1);
        l.x2 = l.x2.clamp(0, w - 1);
        l.x1 <= l.x2
    });
}

fn rasterize_triangle(
    mut x1: i32,
    mut y1: i32,
    mut x2: i32,
    mut y2: i32,
    mut x3: i32,
    mut y3: i32,
    buf: &mut Vec<Scanline>,
) {
    if y1 > y3 {
        (x1, x3, y1, y3) = (x3, x1, y3, y1);
    }
    if y1 > y2 {
        (x1, x2, y1, y2) = (x2, x1, y2, y1);
    }
    if y2 > y3 {
        (x2, x3, y2, y3) = (x3, x2, y3, y2);
    }
    if y2 == y3 {
        triangle_bottom(x1, y1, x2, y2, x3, y3, buf);
    } else if y1 == y2 {
        triangle_top(x1, y1, x2, y2, x3, y3, buf);
    } else {
        let x4 = x1 + ((y2 - y1) as f64 / (y3 - y1) as f64 * (x3 - x1) as f64) as i32;
        triangle_bottom(x1, y1, x2, y2, x4, y2, buf);
        triangle_top(x2, y2, x4, y2, x3, y3, buf);
    }
}

fn triangle_bottom(x1: i32, y1: i32, x2: i32, y2: i32, x3: i32, y3: i32, buf: &mut Vec<Scanline>) {
    let s1 = (x2 - x1) as f64 / (y2 - y1) as f64;
    let s2 = (x3 - x1) as f64 / (y3 - y1) as f64;
    let (mut ax, mut bx) = (x1 as f64, x1 as f64);
    for y in y1..=y2 {
        let (a, b) = (ax as i32, bx as i32);
        ax += s1;
        bx += s2;
        buf.push(Scanline {
            y,
            x1: a.min(b),
            x2: a.max(b),
            alpha: 0xffff,
        });
    }
}

fn triangle_top(x1: i32, y1: i32, x2: i32, y2: i32, x3: i32, y3: i32, buf: &mut Vec<Scanline>) {
    let s1 = (x3 - x1) as f64 / (y3 - y1) as f64;
    let s2 = (x3 - x2) as f64 / (y3 - y2) as f64;
    let (mut ax, mut bx) = (x3 as f64, x3 as f64);
    let mut y = y3;
    while y > y1 {
        ax -= s1;
        bx -= s2;
        let (a, b) = (ax as i32, bx as i32);
        buf.push(Scanline {
            y,
            x1: a.min(b),
            x2: a.max(b),
            alpha: 0xffff,
        });
        y -= 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn rasterize_rotated_rectangle(
    x: i32,
    y: i32,
    sx: i32,
    sy: i32,
    angle: i32,
    w: i32,
    h: i32,
    lines: &mut Vec<Scanline>,
) {
    let (sx, sy) = (sx as f64, sy as f64);
    let a = (angle as f64).to_radians();
    let corner = |dx: f64, dy: f64| {
        let (rx, ry) = rotate(dx, dy, a);
        (rx as i32 + x, ry as i32 + y)
    };
    let c = [
        corner(-sx / 2.0, -sy / 2.0),
        corner(sx / 2.0, -sy / 2.0),
        corner(sx / 2.0, sy / 2.0),
        corner(-sx / 2.0, sy / 2.0),
    ];
    let miny = c.iter().map(|p| p.1).min().unwrap();
    let maxy = c.iter().map(|p| p.1).max().unwrap();
    let n = (maxy - miny + 1) as usize;
    let mut min = vec![w; n];
    let mut max = vec![0; n];
    for i in 0..4 {
        let (x0, y0) = (c[i].0 as f64, c[i].1 as f64);
        let (dx, dy) = (
            (c[(i + 1) % 4].0 - c[i].0) as f64,
            (c[(i + 1) % 4].1 - c[i].1) as f64,
        );
        let count = ((dx * dx + dy * dy).sqrt() as i32) * 2;
        for j in 0..count {
            let t = j as f64 / (count - 1) as f64;
            let xi = (x0 + dx * t) as i32;
            let yi = ((y0 + dy * t) as i32 - miny) as usize;
            min[yi] = min[yi].min(xi);
            max[yi] = max[yi].max(xi);
        }
    }
    for i in 0..n {
        let y = miny + i as i32;
        if y < 0 || y >= h {
            continue;
        }
        let (a, b) = (min[i].max(0), max[i].min(w - 1));
        if b >= a {
            lines.push(Scanline {
                y,
                x1: a,
                x2: b,
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
}
