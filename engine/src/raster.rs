//! An anti-aliasing rasterizer for paths made of straight segments. It
//! accumulates each segment's signed area into a buffer and sums each row
//! over the cells the path touched (the approach of font-rs), and emits runs
//! of equal coverage as scanlines. Overlapping subpaths must all wind the
//! same way; coverage above 1 is clamped, which unions them where either
//! covers a pixel fully but overcounts partly covered pixels, so shapes are
//! drawn as single subpaths where possible.
//!
//! sample_polygon is the exact (aliased) counterpart, for the search.

use crate::core::Scanline;
use std::f64::consts::PI;

#[derive(Default)]
pub struct Rasterizer {
    segments: Vec<[f64; 4]>,
    start: (f64, f64),
    cur: (f64, f64),
    open: bool,
    acc: Vec<f32>, // all zero between fills
    spans: Vec<(usize, usize)>,
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
    /// width, with round caps: one polygon, the flattened curve offset to
    /// either side.
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
        // each point's normal, from the direction between its neighbors
        // (or the last defined one where they coincide)
        let normal = |pts: &[(f64, f64)], i: usize| {
            let (a, b) = (pts[i.saturating_sub(1)], pts[(i + 1).min(n)]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = (dx * dx + dy * dy).sqrt();
            (len > 0.0).then(|| (-dy / len * hw, dx / len * hw))
        };
        let Some(mut nrm) = (0..=n).find_map(|i| normal(&points, i)) else {
            self.points = points;
            return; // no length: nothing to draw
        };
        for i in 0..=n {
            nrm = normal(&points, i).unwrap_or(nrm);
            points.push((points[i].0 + nrm.0, points[i].1 + nrm.1));
        }
        // a half circle from one side to the other around end i, bulging
        // in direction sign along the curve
        let caps = arc_segments(hw) / 2;
        let cap = |points: &mut Vec<(f64, f64)>, i: usize, sign: f64| {
            let (p, l) = (points[i], points[n + 1 + i]);
            let (nx, ny) = (l.0 - p.0, l.1 - p.1);
            for k in 1..caps {
                let (sin, cos) = (PI * k as f64 / caps as f64).sin_cos();
                let (cos, sin) = (cos * sign, sin * sign);
                points.push((p.0 + nx * cos + ny * sin, p.1 + ny * cos - nx * sin));
            }
        };
        cap(&mut points, n, 1.0);
        for i in (0..=n).rev() {
            // mirror the left side's point through the curve's
            let (p, l) = (points[i], points[n + 1 + i]);
            points.push((2.0 * p.0 - l.0, 2.0 * p.1 - l.1));
        }
        cap(&mut points, 0, -1.0);
        self.polygon(&points[n + 1..]);
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
        if self.acc.len() < stride * lh {
            self.acc.resize(stride * lh, 0.0);
        }
        self.spans.clear();
        self.spans.resize(lh, (usize::MAX, 0));
        let (ox, oy, xmax) = (x0 as f64, y0 as f64, lw as f64);
        for i in 0..self.segments.len() {
            let [ax, ay, bx, by] = self.segments[i];
            clip_x(ax - ox, ay - oy, bx - ox, by - oy, xmax, &mut |p, q| {
                draw_line(
                    &mut self.acc,
                    &mut self.spans,
                    stride,
                    lh,
                    xmax as f32,
                    p,
                    q,
                )
            });
        }
        for row in 0..lh {
            // the path is closed, so the sum is 0 outside the touched cells
            let (first, last) = self.spans[row];
            if first > last {
                continue;
            }
            let acc = &mut self.acc[row * stride..row * stride + stride];
            let y = y0 + row as i32;
            let end = lw.min(last + 1);
            let mut push = |x1: usize, x2: usize, alpha: u32| {
                if alpha > 0 {
                    lines.push(Scanline {
                        y,
                        x1: x0 + x1 as i32,
                        x2: x0 + x2 as i32,
                        alpha,
                    });
                }
            };
            // coverage changes only at nonzero cells, so runs are found by
            // skipping from one to the next (the gaps can be wide: inside
            // shapes, and between a curve's arms)
            let (mut sum, mut alpha, mut start) = (0.0f32, 0, first);
            let mut x = first;
            while x < end {
                let a = acc[x];
                if a != 0.0 {
                    sum += a;
                    let next = (sum.abs().min(1.0) * 65535.0 + 0.5) as u32;
                    if next != alpha {
                        if x > start {
                            push(start, x - 1, alpha);
                        }
                        (alpha, start) = (next, x);
                    }
                }
                x += 1;
            }
            if end > start {
                push(start, end - 1, alpha);
            }
            acc[first..=last].fill(0.0);
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

/// The pixels of a w x h image whose coordinates lie inside the polygon
/// (of at most 8 points), under the nonzero rule (as SVG fills it), into
/// lines (cleared first). Pixel (px, py) is inside when the point (px, py)
/// is: like the other shapes' search rasterizers, this leaves out partly
/// covered pixels.
pub fn sample_polygon(points: &[(f64, f64)], w: i32, h: i32, lines: &mut Vec<Scanline>) {
    const MAX: usize = 8;
    assert!(points.len() <= MAX, "too many points");
    lines.clear();
    // each edge covers rows y with top <= y < bottom, so shared vertices
    // count once: (top, bottom, x at top, dx/dy, direction)
    let mut edges = [(0.0, 0.0, 0.0, 0.0, 0); MAX];
    let mut n = 0;
    let (mut miny, mut maxy) = (f64::MAX, f64::MIN);
    for (i, &a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        if a.1 == b.1 {
            continue;
        }
        let (top, bottom, dir) = if a.1 < b.1 { (a, b, 1) } else { (b, a, -1) };
        let slope = (bottom.0 - top.0) / (bottom.1 - top.1);
        edges[n] = (top.1, bottom.1, top.0, slope, dir);
        n += 1;
        miny = miny.min(top.1);
        maxy = maxy.max(bottom.1);
    }
    let y1 = (miny.ceil().max(0.0) as i32).min(h);
    let y2 = (maxy.ceil() as i32).min(h) - 1;
    // the pixels with start <= px < end
    let mut push = |y: i32, start: f64, end: f64| {
        let x1 = (start.ceil().max(0.0) as i32).min(w);
        let x2 = (end.ceil() as i32).min(w) - 1;
        if x1 <= x2 {
            lines.push(Scanline {
                y,
                x1,
                x2,
                alpha: 0xffff,
            });
        }
    };
    let mut crossings = [(0.0, 0); MAX];
    for py in y1..=y2 {
        let y = py as f64;
        let mut k = 0;
        for &(top, bottom, x, slope, dir) in &edges[..n] {
            if top <= y && y < bottom {
                crossings[k] = (x + (y - top) * slope, dir);
                k += 1;
            }
        }
        let crossings = &mut crossings[..k];
        if k == 2 {
            // always, for convex shapes: the two crossings wind oppositely
            let (a, b) = (crossings[0].0, crossings[1].0);
            push(py, a.min(b), a.max(b));
            continue;
        }
        // insertion sort: there are only a few (and it keeps the
        // WebAssembly smaller than the standard library's sort)
        for i in 1..k {
            let mut j = i;
            while j > 0 && crossings[j - 1].0 > crossings[j].0 {
                crossings.swap(j - 1, j);
                j -= 1;
            }
        }
        let (mut winding, mut start) = (0, 0.0);
        for &(x, dir) in crossings.iter() {
            if winding == 0 {
                start = x;
            }
            winding += dir;
            if winding == 0 {
                push(py, start, x);
            }
        }
    }
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
    if (0.0..=xmax).contains(&ax) && (0.0..=xmax).contains(&bx) {
        f((ax as f32, ay as f32), (bx as f32, by as f32));
        return;
    }
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
/// [0, h) of acc, widening each row's span of touched cells. x must lie
/// within [0, xmax], and acc rows hold at least xmax + 2 entries.
fn draw_line(
    acc: &mut [f32],
    spans: &mut [(usize, usize)],
    stride: usize,
    h: usize,
    xmax: f32,
    p0: (f32, f32),
    p1: (f32, f32),
) {
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
        let span = &mut spans[y];
        span.0 = span.0.min(x0i);
        span.1 = span.1.max(x1i.max(x0i + 1));
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
        // a 40 x 2 band plus round caps (polygons, within a tenth of a pixel)
        let want = 80.0 + PI;
        assert!((area(&lines) - want).abs() < 0.4, "area {}", area(&lines));
        // a parabola, half a pixel wide
        r.stroke_quadratic((10.0, 40.0), (30.0, 0.0), (50.0, 40.0), 0.5);
        r.fill(64, 64, &mut lines);
        let len = 59.1577; // its arc length
        assert!(
            (area(&lines) - (len * 0.5 + PI / 16.0)).abs() < 0.2,
            "area {}",
            area(&lines)
        );
        // nothing for a curve of no length
        r.stroke_quadratic((5.0, 5.0), (5.0, 5.0), (5.0, 5.0), 1.0);
        r.fill(64, 64, &mut lines);
        assert!(lines.is_empty());
    }

    #[test]
    fn fills_leave_no_residue() {
        let mut r = Rasterizer::default();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        r.ellipse(30.0, 20.0, 12.0, 5.0, 0.7);
        r.fill(64, 48, &mut a);
        r.polygon(&[(-10.0, -10.0), (80.0, 0.0), (40.0, 60.0)]);
        r.fill(64, 48, &mut b);
        r.stroke_quadratic((0.0, 0.0), (60.0, 5.0), (10.0, 40.0), 0.5);
        r.fill(64, 48, &mut b);
        r.ellipse(30.0, 20.0, 12.0, 5.0, 0.7);
        r.fill(64, 48, &mut b);
        assert_eq!(a, b);
    }

    /// Winding number of the polygon around (x, y).
    fn winding(points: &[(f64, f64)], x: f64, y: f64) -> i32 {
        let mut w = 0;
        for (i, &(ax, ay)) in points.iter().enumerate() {
            let (bx, by) = points[(i + 1) % points.len()];
            let cross = (bx - ax) * (y - ay) - (x - ax) * (by - ay);
            if ay <= y && y < by && cross > 0.0 {
                w += 1;
            } else if by <= y && y < ay && cross < 0.0 {
                w -= 1;
            }
        }
        w
    }

    #[test]
    fn sampled_polygons_cover_the_points_inside() {
        let mut rnd = crate::rng::Rng::new(4);
        let mut lines = Vec::new();
        for _ in 0..2000 {
            // quadrilaterals of every kind: convex, concave, self-crossing
            let points: Vec<(f64, f64)> = (0..4)
                .map(|_| (rnd.float() * 90.0 - 15.0, rnd.float() * 70.0 - 11.0))
                .collect();
            sample_polygon(&points, 64, 48, &mut lines);
            for py in 0..48 {
                for px in 0..64 {
                    let covered = lines.iter().any(|l| l.y == py && l.x1 <= px && px <= l.x2);
                    let inside = winding(&points, px as f64, py as f64) != 0;
                    assert_eq!(covered, inside, "({px}, {py}) in {points:?}");
                }
            }
            assert!(lines.iter().all(|l| l.x1 >= 0 && l.x1 <= l.x2 && l.x2 < 64));
        }
    }
}
