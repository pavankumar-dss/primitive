//! An anti-aliasing rasterizer for paths made of straight segments. It
//! accumulates each segment's signed area into a buffer and sums every row
//! (the approach of font-rs), and emits runs of equal coverage as
//! scanlines. Overlapping subpaths must all wind the same way; coverage
//! above 1 is clamped, which unions them.

use crate::core::Scanline;
use std::f64::consts::PI;

#[derive(Default)]
pub struct Rasterizer {
    segments: Vec<[f64; 4]>,
    start: (f64, f64),
    cur: (f64, f64),
    open: bool,
    acc: Vec<f32>,
    points: Vec<(f64, f64)>,
}

impl Rasterizer {
    pub fn clear(&mut self) {
        self.segments.clear();
        self.open = false;
    }

    pub fn move_to(&mut self, x: f64, y: f64) {
        self.close();
        self.start = (x, y);
        self.cur = (x, y);
        self.open = true;
    }

    pub fn line_to(&mut self, x: f64, y: f64) {
        if !self.open {
            self.move_to(x, y);
            return;
        }
        let (x0, y0) = self.cur;
        self.segments.push([x0, y0, x, y]);
        self.cur = (x, y);
    }

    pub fn close(&mut self) {
        if self.open {
            let (x, y) = self.start;
            if self.cur != (x, y) {
                self.line_to(x, y);
            }
            self.open = false;
        }
    }

    /// Adds a closed polygon, reversed if needed so that it winds the same
    /// way as every other polygon added with this method.
    pub fn polygon(&mut self, points: &[(f64, f64)]) {
        let mut area = 0.0;
        for (i, &(x0, y0)) in points.iter().enumerate() {
            let (x1, y1) = points[(i + 1) % points.len()];
            area += x0 * y1 - x1 * y0;
        }
        let mut add = |p: (f64, f64), first: bool| {
            if first {
                self.move_to(p.0, p.1)
            } else {
                self.line_to(p.0, p.1)
            }
        };
        if area >= 0.0 {
            for (i, &p) in points.iter().enumerate() {
                add(p, i == 0);
            }
        } else {
            for (i, &p) in points.iter().rev().enumerate() {
                add(p, i == 0);
            }
        }
        self.close();
    }

    /// Adds an ellipse centered at (cx, cy) with radii rx and ry, rotated
    /// by angle radians, as a polygon close enough to look smooth.
    pub fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, angle: f64) {
        let n = arc_segments(rx.abs().max(ry.abs()));
        let (sin, cos) = angle.sin_cos();
        let mut points = std::mem::take(&mut self.points);
        points.clear();
        for i in 0..n {
            let t = 2.0 * PI * i as f64 / n as f64;
            let (x, y) = (rx * t.cos(), ry * t.sin());
            points.push((cx + x * cos - y * sin, cy + x * sin + y * cos));
        }
        self.polygon(&points);
        self.points = points;
    }

    /// Adds the outline of a quadratic Bézier curve stroked with the given
    /// width, with round caps and joins.
    pub fn stroke_quadratic(&mut self, p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), width: f64) {
        let hw = width / 2.0;
        let dd = ((p0.0 - 2.0 * p1.0 + p2.0).powi(2) + (p0.1 - 2.0 * p1.1 + p2.1).powi(2)).sqrt();
        // a quadratic flattened into n lines strays at most dd / (4 n^2)
        let n = ((dd / 0.4).sqrt().ceil() as usize).clamp(1, 64);
        let mut points = std::mem::take(&mut self.points);
        points.clear();
        for i in 0..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            points.push((
                u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0,
                u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1,
            ));
        }
        for i in 0..n {
            let (a, b) = (points[i], points[i + 1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len == 0.0 {
                continue;
            }
            let (nx, ny) = (-dy / len * hw, dx / len * hw);
            self.polygon(&[
                (a.0 + nx, a.1 + ny),
                (b.0 + nx, b.1 + ny),
                (b.0 - nx, b.1 - ny),
                (a.0 - nx, a.1 - ny),
            ]);
        }
        for &(x, y) in &points {
            self.ellipse(x, y, hw, hw, 0.0);
        }
        self.points = points;
    }

    /// Rasterizes the path within a w x h image into lines (which are
    /// cleared first), and clears the path.
    pub fn fill(&mut self, w: i32, h: i32, lines: &mut Vec<Scanline>) {
        lines.clear();
        self.close();
        if self.segments.is_empty() {
            return;
        }
        let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for s in &self.segments {
            minx = minx.min(s[0]).min(s[2]);
            maxx = maxx.max(s[0]).max(s[2]);
            miny = miny.min(s[1]).min(s[3]);
            maxy = maxy.max(s[1]).max(s[3]);
        }
        let x0 = (minx.floor().max(0.0) as i32).min(w);
        let x1 = (maxx.ceil() as i32).clamp(0, w);
        let y0 = (miny.floor().max(0.0) as i32).min(h);
        let y1 = (maxy.ceil() as i32).clamp(0, h);
        if x0 >= x1 || y0 >= y1 {
            self.clear();
            return;
        }
        let lw = (x1 - x0) as usize;
        let lh = (y1 - y0) as usize;
        let stride = lw + 2;
        self.acc.clear();
        self.acc.resize(stride * lh, 0.0);
        let (ox, oy, xmax) = (x0 as f64, y0 as f64, lw as f64);
        for i in 0..self.segments.len() {
            let [ax, ay, bx, by] = self.segments[i];
            clip_x(ax - ox, ay - oy, bx - ox, by - oy, xmax, &mut |p, q| {
                draw_line(&mut self.acc, stride, lh, xmax as f32, p, q)
            });
        }
        for row in 0..lh {
            let acc = &self.acc[row * stride..row * stride + lw];
            let y = y0 + row as i32;
            let mut sum = 0.0f32;
            let mut run: Option<Scanline> = None;
            for (x, &a) in acc.iter().enumerate() {
                sum += a;
                let alpha = (sum.abs().min(1.0) * 65535.0 + 0.5) as u32;
                let x = x0 + x as i32;
                match run {
                    Some(ref mut r) if r.alpha == alpha => r.x2 = x,
                    _ => {
                        if let Some(r) = run.take().filter(|r| r.alpha > 0) {
                            lines.push(r);
                        }
                        run = Some(Scanline {
                            y,
                            x1: x,
                            x2: x,
                            alpha,
                        });
                    }
                }
            }
            if let Some(r) = run.filter(|r| r.alpha > 0) {
                lines.push(r);
            }
        }
        self.clear();
    }
}

/// How many segments approximate a circle of radius r (in pixels) within
/// a tenth of a pixel.
fn arc_segments(r: f64) -> usize {
    let step = (0.8 / r.max(0.01)).sqrt();
    ((2.0 * PI / step).ceil() as usize).clamp(6, 256)
}

/// Calls f with the parts of the segment from a to b, with x clamped to
/// [0, xmax]. Parts outside become vertical segments on the edge, which
/// keeps the coverage inside right.
fn clip_x(
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    xmax: f64,
    f: &mut impl FnMut((f32, f32), (f32, f32)),
) {
    let mut ts = [0.0, 1.0, 1.0, 1.0];
    let mut n = 1;
    if ax != bx {
        for edge in [0.0, xmax] {
            let t = (edge - ax) / (bx - ax);
            if t > 0.0 && t < 1.0 {
                ts[n] = t;
                n += 1;
            }
        }
    }
    ts[n] = 1.0;
    ts[..=n].sort_by(|a, b| a.total_cmp(b));
    let at = |t: f64| {
        (
            ((ax + (bx - ax) * t).clamp(0.0, xmax)) as f32,
            (ay + (by - ay) * t) as f32,
        )
    };
    for k in 0..n {
        f(at(ts[k]), at(ts[k + 1]));
    }
}

/// Accumulates the signed area of the line from p0 to p1 into rows
/// [0, h) of acc. x must lie within [0, xmax], and acc rows hold at least
/// xmax + 2 entries.
fn draw_line(acc: &mut [f32], stride: usize, h: usize, xmax: f32, p0: (f32, f32), p1: (f32, f32)) {
    if (p0.1 - p1.1).abs() <= f32::EPSILON {
        return;
    }
    let (dir, p0, p1) = if p0.1 < p1.1 {
        (1.0, p0, p1)
    } else {
        (-1.0, p1, p0)
    };
    let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
    let mut x = p0.0;
    if p0.1 < 0.0 {
        x = (x - p0.1 * dxdy).clamp(0.0, xmax);
    }
    let ystart = p0.1.max(0.0) as usize;
    let yend = (p1.1.ceil().max(0.0) as usize).min(h);
    for y in ystart..yend {
        let row = &mut acc[y * stride..(y + 1) * stride];
        let dy = ((y + 1) as f32).min(p1.1) - (y as f32).max(p0.1);
        let xnext = (x + dxdy * dy).clamp(0.0, xmax);
        let d = dy * dir;
        let (x0, x1) = if x < xnext { (x, xnext) } else { (xnext, x) };
        let x0floor = x0.floor();
        let x0i = x0floor as usize;
        let x1ceil = x1.ceil();
        let x1i = x1ceil as usize;
        if x1i <= x0i + 1 {
            let xmf = 0.5 * (x + xnext) - x0floor;
            row[x0i] += d - d * xmf;
            row[x0i + 1] += d * xmf;
        } else {
            let s = (x1 - x0).recip();
            let x0f = x0 - x0floor;
            let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
            let x1f = x1 - x1ceil + 1.0;
            let am = 0.5 * s * x1f * x1f;
            row[x0i] += d * a0;
            if x1i == x0i + 2 {
                row[x0i + 1] += d * (1.0 - a0 - am);
            } else {
                let a1 = s * (1.5 - x0f);
                row[x0i + 1] += d * (a1 - a0);
                for v in &mut row[x0i + 2..x1i - 1] {
                    *v += d * s;
                }
                let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                row[x1i - 1] += d * (1.0 - a2 - am);
            }
            row[x1i] += d * am;
        }
        x = xnext;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(lines: &[Scanline]) -> f64 {
        lines
            .iter()
            .map(|l| (l.x2 - l.x1 + 1) as f64 * l.alpha as f64 / 65535.0)
            .sum()
    }

    #[test]
    fn square_coverage() {
        let mut r = Rasterizer::default();
        let mut lines = Vec::new();
        r.polygon(&[(2.5, 2.5), (7.5, 2.5), (7.5, 7.5), (2.5, 7.5)]);
        r.fill(10, 10, &mut lines);
        assert!((area(&lines) - 25.0).abs() < 1e-3, "area {}", area(&lines));
        // interior pixels are fully covered
        assert!(
            lines
                .iter()
                .any(|l| l.y == 4 && l.x1 <= 3 && l.x2 >= 6 && l.alpha == 65535)
        );
    }

    #[test]
    fn winding_either_way_and_union() {
        let mut r = Rasterizer::default();
        let mut lines = Vec::new();
        r.polygon(&[(1.0, 1.0), (1.0, 5.0), (5.0, 5.0), (5.0, 1.0)]);
        r.polygon(&[(3.0, 1.0), (7.0, 1.0), (7.0, 5.0), (3.0, 5.0)]);
        r.fill(10, 10, &mut lines);
        assert!((area(&lines) - 24.0).abs() < 1e-3, "area {}", area(&lines));
    }

    #[test]
    fn clipped_to_the_image() {
        let mut r = Rasterizer::default();
        let mut lines = Vec::new();
        r.polygon(&[(-5.0, -5.0), (15.0, -5.0), (15.0, 3.0), (-5.0, 3.0)]);
        r.fill(10, 10, &mut lines);
        assert!((area(&lines) - 30.0).abs() < 1e-3, "area {}", area(&lines));
        assert!(
            lines
                .iter()
                .all(|l| l.x1 >= 0 && l.x2 < 10 && l.y >= 0 && l.y < 10)
        );
        r.polygon(&[(-5.0, 1.0), (15.0, 9.0), (-5.0, 9.0)]);
        r.fill(10, 10, &mut lines);
        assert!(
            lines
                .iter()
                .all(|l| l.x1 >= 0 && l.x2 < 10 && l.y >= 0 && l.y < 10)
        );
        assert!(!lines.is_empty());
        r.polygon(&[(20.0, 1.0), (30.0, 1.0), (30.0, 9.0)]);
        r.fill(10, 10, &mut lines);
        assert!(lines.is_empty());
    }

    #[test]
    fn circle_area() {
        let mut r = Rasterizer::default();
        let mut lines = Vec::new();
        r.ellipse(50.0, 50.0, 20.0, 20.0, 0.3);
        r.fill(100, 100, &mut lines);
        let want = PI * 400.0;
        assert!(
            (area(&lines) - want).abs() / want < 0.01,
            "area {} want {}",
            area(&lines),
            want
        );
    }

    #[test]
    fn thin_stroke() {
        let mut r = Rasterizer::default();
        let mut lines = Vec::new();
        r.stroke_quadratic((10.0, 10.0), (30.0, 10.0), (50.0, 10.0), 2.0);
        r.fill(64, 64, &mut lines);
        // a 40 x 2 band plus round caps
        let want = 80.0 + PI;
        assert!((area(&lines) - want).abs() < 1.0, "area {}", area(&lines));
    }
}
