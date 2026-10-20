//! The model: the target image, the picture built so far, and the search
//! for the next shape, ported from model.go, worker.go, state.go and
//! optimize.go in the original Go package.

use crate::constraints::Constraints;
use crate::core::{self, Clip, Color, Scanline};
use crate::raster::Rasterizer;
use crate::render::Base;
use crate::rng::Rng;
use crate::shape::{Env, Shape, ShapeType};
use std::fmt::Write;

/// A candidate shape and its energy: the score the picture would have with
/// it (plus a penalty if it breaks the constraints). Lower is better.
#[derive(Clone, Copy, Debug)]
pub struct State {
    pub shape: Shape,
    pub alpha: i32,
    mutate_alpha: bool,
    energy: Option<f64>,
}

/// Search scratch space.
struct Worker {
    buffer: Vec<u8>,
    lines: Vec<Scanline>,
    scratch: Vec<Scanline>,
    raster: Rasterizer,
    rnd: Rng,
}

pub struct Model {
    /// working image size
    pub w: usize,
    pub h: usize,
    /// output image size, and the scale from working to output pixels
    pub sw: usize,
    pub sh: usize,
    pub scale: f64,
    pub background: Color,
    target: Vec<u8>,
    current: Vec<u8>,
    pub score: f64,
    /// how many candidate shapes have been scored, for speed reports
    pub evaluations: u64,
    pub shapes: Vec<Shape>,
    pub colors: Vec<Color>,
    constraints: Option<Constraints>,
    /// the region shapes are clipped to, when the image outside it is kept
    clip: Option<Clip>,
    /// what renders show outside that region (see keep_outside)
    pub base: Option<Base>,
    worker: Worker,
}

impl Model {
    /// A model of target (w x h RGBA pixels), drawn at size pixels along
    /// its longest side.
    pub fn new(
        target: Vec<u8>,
        w: usize,
        h: usize,
        background: Color,
        size: usize,
        seed: u64,
    ) -> Model {
        let aspect = w as f64 / h as f64;
        let (sw, sh, scale) = if aspect >= 1.0 {
            (
                size,
                (size as f64 / aspect) as usize,
                size as f64 / w as f64,
            )
        } else {
            (
                (size as f64 * aspect) as usize,
                size,
                size as f64 / h as f64,
            )
        };
        let mut current = vec![0u8; w * h * 4];
        for p in current.as_chunks_mut::<4>().0 {
            p.copy_from_slice(&[
                background.r as u8,
                background.g as u8,
                background.b as u8,
                background.a as u8,
            ]);
        }
        let score = core::difference_full(&target, &current);
        Model {
            w,
            h,
            sw,
            sh,
            scale,
            background,
            target,
            current,
            score,
            evaluations: 0,
            shapes: vec![],
            colors: vec![],
            constraints: None,
            clip: None,
            base: None,
            worker: Worker {
                buffer: vec![0; w * h * 4],
                lines: Vec::with_capacity(4096),
                scratch: Vec::with_capacity(4096),
                raster: Rasterizer::default(),
                rnd: Rng::new(seed),
            },
        }
    }

    pub fn set_constraints(&mut self, c: Option<Constraints>) {
        self.constraints = c;
    }

    /// Keeps the target image outside a region and puts shapes only inside
    /// it: the picture starts as the target outside and as background
    /// inside (the average color there if None), and shapes are clipped to
    /// the region. mask is one byte per working pixel, 128 and up inside.
    /// Renders show the target outside, scaled up, unless base is replaced
    /// with a sharper image.
    pub fn keep_outside(&mut self, mask: &[u8], background: Option<Color>) {
        let n = self.w * self.h;
        let inside = |i: usize| mask[i] >= 128;
        let bg = background.unwrap_or_else(|| {
            let (mut sum, mut count) = ([0usize; 3], 0);
            for i in (0..n).filter(|&i| inside(i)) {
                for (s, &v) in sum.iter_mut().zip(&self.target[i * 4..i * 4 + 3]) {
                    *s += v as usize;
                }
                count += 1;
            }
            match count {
                0 => self.background,
                _ => Color {
                    r: (sum[0] / count) as i32,
                    g: (sum[1] / count) as i32,
                    b: (sum[2] / count) as i32,
                    a: 255,
                },
            }
        });
        for i in 0..n {
            let p = &mut self.current[i * 4..i * 4 + 4];
            if inside(i) {
                p.copy_from_slice(&[bg.r as u8, bg.g as u8, bg.b as u8, 255]);
            } else {
                p.copy_from_slice(&self.target[i * 4..i * 4 + 4]);
            }
        }
        self.background = bg;
        self.score = core::difference_full(&self.target, &self.current);
        self.clip = Some(Clip::new(mask, self.w, self.h));
        self.base = Some(Base {
            w: self.w,
            h: self.h,
            pix: self.target.clone(),
            mask: mask[..n]
                .iter()
                .map(|&m| if m >= 128 { 255 } else { 0 })
                .collect(),
        });
    }

    /// The pixels shape covers, into the worker's lines.
    fn rasterize(&mut self, shape: &Shape) {
        let wk = &mut self.worker;
        shape.rasterize(self.w as i32, self.h as i32, &mut wk.raster, &mut wk.lines);
        if let Some(clip) = &self.clip {
            clip.apply(&mut wk.lines, &mut wk.scratch);
        }
    }

    fn env(&mut self) -> Env<'_> {
        Env {
            w: self.w as i32,
            h: self.h as i32,
            rnd: &mut self.worker.rnd,
            constraints: self.constraints.as_ref(),
        }
    }

    fn energy(&mut self, state: &mut State) -> f64 {
        if let Some(e) = state.energy {
            return e;
        }
        self.evaluations += 1;
        self.rasterize(&state.shape);
        let (w, h) = (self.w, self.h);
        let wk = &mut self.worker;
        let lines = &wk.lines;
        let color = core::compute_color(&self.target, &self.current, w, lines, state.alpha);
        core::copy_lines(&mut wk.buffer, &self.current, w, lines);
        core::draw_lines(&mut wk.buffer, w, color, lines);
        let mut energy = core::difference_partial(
            &self.target,
            &self.current,
            &wk.buffer,
            w,
            h,
            self.score,
            lines,
        );
        if let Some(c) = &self.constraints {
            let v = c.violation(lines, w);
            if v > 0.0 {
                // scores are at most 1, so any allowed shape beats a
                // disallowed one, and smaller violations still guide the search
                energy += 1.0 + v;
            }
        }
        state.energy = Some(energy);
        energy
    }

    fn random_state(&mut self, t: ShapeType, alpha: i32) -> State {
        let shape = Shape::random(t, &mut self.env());
        let (alpha, mutate_alpha) = if alpha == 0 {
            (128, true)
        } else {
            (alpha, false)
        };
        State {
            shape,
            alpha,
            mutate_alpha,
            energy: None,
        }
    }

    fn do_move(&mut self, state: &mut State) {
        state.shape.mutate(&mut self.env());
        if state.mutate_alpha {
            state.alpha = (state.alpha + self.worker.rnd.intn(21) - 10).clamp(1, 255);
        }
        state.energy = None;
    }

    fn hill_climb(&mut self, mut state: State, max_age: i32) -> State {
        let mut best_energy = self.energy(&mut state);
        let mut best = state;
        let mut age = 0;
        while age < max_age {
            let undo = state;
            self.do_move(&mut state);
            let energy = self.energy(&mut state);
            if energy >= best_energy {
                state = undo;
            } else {
                best_energy = energy;
                best = state;
                age = -1;
            }
            age += 1;
        }
        best
    }

    /// Runs m independent searches, each the best of n random shapes
    /// improved by hill climbing until age moves in a row fail, and returns
    /// the best shape found without adding it.
    pub fn search(&mut self, t: ShapeType, alpha: i32, m: i32) -> State {
        const N: i32 = 1000;
        const AGE: i32 = 100;
        let mut best: Option<State> = None;
        for _ in 0..m.max(1) {
            let mut start = self.random_state(t, alpha);
            self.energy(&mut start);
            for _ in 1..N {
                let mut s = self.random_state(t, alpha);
                if self.energy(&mut s) < start.energy.unwrap() {
                    start = s;
                }
            }
            let mut s = self.hill_climb(start, AGE);
            if best.is_none_or(|b| self.energy(&mut s) < b.energy.unwrap()) {
                best = Some(s);
            }
        }
        best.unwrap()
    }

    pub fn energy_of(&mut self, state: &mut State) -> f64 {
        self.energy(state)
    }

    fn allowed(&mut self, shape: &Shape) -> bool {
        if self.constraints.is_none() {
            return true;
        }
        self.rasterize(shape);
        let c = self.constraints.as_ref().unwrap();
        c.violation(&self.worker.lines, self.w) == 0.0
    }

    /// Adds the shape in state, found by a search, followed by up to repeat
    /// more shapes found by hill climbing from it. Returns the states added,
    /// which is none if the shape breaks the constraints.
    pub fn commit(&mut self, mut state: State, repeat: i32) -> Vec<State> {
        if !self.allowed(&state.shape) {
            // no shape satisfying the constraints was found this time
            return vec![];
        }
        self.add(state.shape, state.alpha);
        let mut added = vec![state];
        for _ in 0..repeat {
            // as in Go, the cached energy is from before the last shape
            let a = self.energy(&mut state);
            state = self.hill_climb(state, 100);
            let b = self.energy(&mut state);
            if a == b || !self.allowed(&state.shape) {
                break;
            }
            self.add(state.shape, state.alpha);
            added.push(state);
        }
        added
    }

    pub fn add(&mut self, shape: Shape, alpha: i32) {
        self.rasterize(&shape);
        let (w, h) = (self.w, self.h);
        let wk = &mut self.worker;
        let lines = &wk.lines;
        let color = core::compute_color(&self.target, &self.current, w, lines, alpha);
        // the buffer keeps the pixels from before the shape for scoring
        core::copy_lines(&mut wk.buffer, &self.current, w, lines);
        core::draw_lines(&mut self.current, w, color, lines);
        self.score = core::difference_partial(
            &self.target,
            &wk.buffer,
            &self.current,
            w,
            h,
            self.score,
            lines,
        );
        self.shapes.push(shape);
        self.colors.push(color);
    }

    fn svg_with(&self, style: &str, group: &str, delay: impl Fn(usize) -> Option<f64>) -> String {
        let bg = self.background;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" width=\"{}\" height=\"{}\">",
            self.sw, self.sh
        );
        out.push_str(style);
        let _ = writeln!(
            out,
            "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#{:02x}{:02x}{:02x}\" />",
            self.sw, self.sh, bg.r, bg.g, bg.b
        );
        let _ = writeln!(
            out,
            "<g{group} transform=\"scale({:.6}) translate(0.5 0.5)\">",
            self.scale
        );
        let mut line = String::new();
        for (i, (shape, c)) in self.shapes.iter().zip(&self.colors).enumerate() {
            let attrs = format!(
                "fill=\"#{:02x}{:02x}{:02x}\" fill-opacity=\"{:.6}\"",
                c.r,
                c.g,
                c.b,
                c.a as f64 / 255.0
            );
            line.clear();
            shape.svg(&attrs, &mut line);
            if let Some(d) = delay(i) {
                // every shape's SVG starts with "<tag ": delay that element
                let j = line.find(' ').unwrap_or(line.len());
                line.insert_str(j, &format!(" style=\"animation-delay:{d:.3}s\""));
            }
            out.push_str(&line);
            out.push('\n');
        }
        out.push_str("</g>\n</svg>");
        out
    }

    pub fn svg(&self) -> String {
        self.svg_with("", "", |_| None)
    }

    /// An SVG in which the shapes fade in one after another over duration
    /// seconds, then stay.
    pub fn animated_svg(&self, duration: f64) -> String {
        let n = self.shapes.len();
        self.svg_with(
            "<style>.p>*{opacity:0;animation:f .3s ease-out forwards}@keyframes f{to{opacity:1}}@media (prefers-reduced-motion:reduce){.p>*{animation:none;opacity:1}}</style>\n",
            " class=\"p\"",
            |i| Some(duration * shape_time(i, n)),
        )
    }

    /// For each of the given number of frames, how many shapes are visible:
    /// the first frame shows only the background, the last every shape.
    pub fn animation_counts(&self, frames: usize) -> Vec<usize> {
        let n = self.shapes.len() as f64;
        (0..frames)
            .map(|k| {
                if frames > 1 {
                    let t = k as f64 / (frames - 1) as f64;
                    (n * t.powf(1.0 / ANIMATION_EASE)).round() as usize
                } else {
                    n as usize
                }
            })
            .collect()
    }
}

/// Shape i of n appears at fraction (i/n)^ANIMATION_EASE of an animation,
/// so more time goes to the first shapes, which change the picture most.
const ANIMATION_EASE: f64 = 0.6;

fn shape_time(i: usize, n: usize) -> f64 {
    ((i + 1) as f64 / n as f64).powf(ANIMATION_EASE)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn test_model(seed: u64) -> Model {
        let (w, h) = (64, 48);
        let mut pix = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                pix[(y * w + x) * 4..][..4].copy_from_slice(&[
                    (x * 4) as u8,
                    (y * 5) as u8,
                    128,
                    255,
                ]);
            }
        }
        let bg = core::average_color(&pix);
        Model::new(pix, w, h, bg, 128, seed)
    }

    #[test]
    fn steps_improve_the_score() {
        for t in 0..=8 {
            let mut m = test_model(t as u64);
            let start = m.score;
            for _ in 0..5 {
                let s = m.search(ShapeType::from_i32(t), 128, 2);
                m.commit(s, 0);
            }
            assert_eq!(m.shapes.len(), 5);
            assert!(m.score < start, "type {t}: {} -> {}", start, m.score);
        }
    }

    // Shapes found by one model and replayed by another, as the browser's
    // workers do, must leave both with the same picture.
    #[test]
    fn commit_and_replay() {
        let mut searcher = test_model(1);
        let mut follower = test_model(2);
        for _ in 0..10 {
            let s = searcher.search(ShapeType::Any, 0, 2);
            for s in searcher.commit(s, 2) {
                let mut v = vec![];
                s.shape.encode(&mut v);
                follower.add(Shape::decode(&v).unwrap().0, s.alpha);
            }
        }
        assert!(searcher.shapes.len() >= 10);
        assert_eq!(follower.score, searcher.score);
        assert!(follower.current == searcher.current);
        assert_eq!(follower.svg(), searcher.svg());
    }

    #[test]
    fn score_matches_full_difference() {
        let mut m = test_model(3);
        for _ in 0..10 {
            let s = m.search(ShapeType::RotatedEllipse, 0, 1);
            m.commit(s, 0);
        }
        let full = core::difference_full(&m.target, &m.current);
        // as in Go, the running total is truncated each step, so it drifts a little
        assert!((m.score - full).abs() < 1e-6, "{} vs {}", m.score, full);
    }

    /// A disc in the middle of the test model's image.
    fn disc(m: &Model) -> Vec<u8> {
        (0..m.w * m.h)
            .map(|i| {
                let (x, y) = ((i % m.w) as f64 - 32.0, (i / m.w) as f64 - 24.0);
                if x * x + y * y < 15.0 * 15.0 { 255 } else { 0 }
            })
            .collect()
    }

    #[test]
    fn keep_outside() {
        let mut m = test_model(5);
        let mask = disc(&m);
        let before = m.score;
        m.keep_outside(&mask, None);
        assert!(
            m.score < before,
            "keeping the image outside starts closer to it"
        );
        // the background is now the disc's average color
        let inside: Vec<usize> = (0..m.w * m.h).filter(|&i| mask[i] == 255).collect();
        let red = inside
            .iter()
            .map(|&i| m.target[i * 4] as usize)
            .sum::<usize>()
            / inside.len();
        assert_eq!(m.background.r, red as i32);
        let start = m.score;
        for t in [
            ShapeType::Triangle,
            ShapeType::RotatedEllipse,
            ShapeType::Quadratic,
        ] {
            for _ in 0..4 {
                let s = m.search(t, 128, 2);
                m.commit(s, 0);
            }
        }
        assert!(m.score < start);
        for i in (0..m.w * m.h).filter(|&i| mask[i] == 0) {
            assert_eq!(
                m.current[i * 4..i * 4 + 4],
                m.target[i * 4..i * 4 + 4],
                "pixel {i} outside changed"
            );
        }
        assert!((m.score - core::difference_full(&m.target, &m.current)).abs() < 1e-6);

        // a follower that keeps the same region replays to the same picture
        let mut follower = test_model(6);
        follower.keep_outside(&mask, None);
        for (s, c) in m.shapes.iter().zip(&m.colors) {
            follower.add(*s, c.a);
        }
        assert!(follower.current == m.current);
    }

    #[test]
    fn renders_show_the_base_outside() {
        let mut m = test_model(7);
        let mask = disc(&m);
        m.keep_outside(&mask, None);
        for _ in 0..6 {
            let s = m.search(ShapeType::Rectangle, 255, 2);
            m.commit(s, 0);
        }
        // a sharper base at twice the size: flat green outside the disc
        let (bw, bh) = (m.w * 2, m.h * 2);
        let base_mask: Vec<u8> = (0..bw * bh)
            .map(|i| mask[(i / bw / 2) * m.w + (i % bw) / 2])
            .collect();
        m.base = Some(Base {
            w: bw,
            h: bh,
            pix: [0, 200, 0, 255].repeat(bw * bh),
            mask: base_mask,
        });
        let mut a = crate::render::Animator::new(&m, 128, false);
        let (w, h) = (a.width, a.height);
        let pix = a.frame(&m, m.shapes.len()).to_vec();
        // corners are far outside the disc: pure base
        for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
            assert_eq!(pix[(y * w + x) * 4..][..4], [0, 200, 0, 255]);
        }
        // the center is inside: not the base
        let c = (h / 2 * w + w / 2) * 4;
        assert_ne!(pix[c..c + 3], [0, 200, 0]);
    }

    #[test]
    fn region_constraint() {
        let mut m = test_model(4);
        let (w, h) = (m.w, m.h);
        // only the left half is allowed
        let region: Vec<u8> = (0..w * h)
            .map(|i| if i % w < w / 2 { 255 } else { 0 })
            .collect();
        let opts = crate::constraints::Options {
            focus: None,
            angle: None,
            min_size: 0.0,
            max_size: 0.3,
            region: Some(&region),
        };
        m.set_constraints(Constraints::new(&opts, w, h));
        for _ in 0..10 {
            let s = m.search(ShapeType::Triangle, 128, 2);
            m.commit(s, 0);
        }
        assert!(!m.shapes.is_empty());
        let mut r = Rasterizer::default();
        let mut lines = vec![];
        for s in &m.shapes {
            s.rasterize(w as i32, h as i32, &mut r, &mut lines);
            let total: i32 = lines.iter().map(|l| l.x2 - l.x1 + 1).sum();
            let outside: i32 = lines
                .iter()
                .map(|l| (l.x2 + 1).max(w as i32 / 2) - l.x1.max(w as i32 / 2))
                .sum();
            assert!(outside as f64 <= 0.1 * total as f64 + 1e-9, "{s:?}");
        }
    }

    #[test]
    fn animation_counts() {
        let mut m = test_model(5);
        for _ in 0..7 {
            let s = m.search(ShapeType::Rectangle, 128, 1);
            m.commit(s, 0);
        }
        let c = m.animation_counts(5);
        assert_eq!((c[0], c[4]), (0, 7));
        assert!(c.windows(2).all(|w| w[0] <= w[1]));
        let svg = m.animated_svg(4.0);
        assert_eq!(svg.matches("animation-delay").count(), 7);
        assert!(svg.contains("animation-delay:4.000s"));
    }
}
