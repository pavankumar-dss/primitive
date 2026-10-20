//! The primitive algorithm (github.com/fogleman/primitive), ported from Go
//! to Rust. Compiled to WebAssembly it runs the browser version, several
//! times faster than the Go code compiled to WebAssembly did, and as a
//! library it runs the command-line tool (../cli).
//!
//! The exported functions below are the WebAssembly interface; site/engine.js
//! wraps them. Inputs (pixels, regions, shapes) go through a buffer from
//! input_buffer, and variable-size results come back through output_ptr or
//! shapes_ptr, valid until the next call.

pub mod constraints;
pub mod core;
pub mod gif;
pub mod model;
pub mod palette;
pub mod raster;
pub mod render;
pub mod rng;
pub mod shape;

use constraints::{Constraints, Options};
use model::{Model, State};
use palette::Palette;
use render::Animator;
use shape::{Shape, ShapeType};
use std::cell::RefCell;

#[derive(Default)]
struct Engine {
    model: Option<Model>,
    best: Option<State>,
    input: Vec<u8>,
    output: Vec<u8>,
    shapes: Vec<f64>,
    animator: Option<Animator>,
    counts: Vec<usize>,
}

thread_local! {
    static ENGINE: RefCell<Engine> = RefCell::new(Engine::default());
}

fn with<T>(f: impl FnOnce(&mut Engine) -> T) -> T {
    ENGINE.with_borrow_mut(f)
}

/// Returns a buffer of len bytes for the next call's input.
#[unsafe(no_mangle)]
pub extern "C" fn input_buffer(len: usize) -> *mut u8 {
    with(|e| {
        e.input.resize(len, 0);
        e.input.as_mut_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn output_ptr() -> *const u8 {
    with(|e| e.output.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn shapes_ptr() -> *const f64 {
    with(|e| e.shapes.as_ptr())
}

/// Starts a model of the w x h RGBA pixels in the input buffer, drawn at
/// out_size pixels along its longest side. background is 0xRRGGBB, or -1
/// for the image's average color. If palette is more than 0, shapes only
/// take that many colors: chosen from the image, or if given is set, the
/// colors after the pixels in the input buffer, 3 bytes (RGB) each; an
/// average background becomes the nearest of them. Returns the starting
/// score.
#[unsafe(no_mangle)]
pub extern "C" fn init(
    w: usize,
    h: usize,
    out_size: usize,
    background: i32,
    seed: f64,
    palette: usize,
    given: u32,
) -> f64 {
    with(|e| {
        let pix = e.input[..w * h * 4].to_vec();
        let palette = match (palette, given) {
            (0, _) => None,
            (n, 0) => Some(Palette::from_image(&pix, n)),
            (n, _) => Palette::new(
                e.input[w * h * 4..][..n * 3]
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|&[r, g, b]| core::Color {
                        r: r as i32,
                        g: g as i32,
                        b: b as i32,
                        a: 255,
                    })
                    .collect(),
            ),
        };
        let bg = if background < 0 {
            let average = core::average_color(&pix);
            palette.as_ref().map_or(average, |p| p.nearest(average))
        } else {
            core::Color {
                r: background >> 16 & 255,
                g: background >> 8 & 255,
                b: background & 255,
                a: 255,
            }
        };
        let mut model = Model::new(pix, w, h, bg, out_size, seed as u64);
        model.set_palette(palette);
        let score = model.score;
        e.model = Some(model);
        e.best = None;
        e.animator = None;
        score
    })
}

/// Sets constraints (see constraints::Options). flags: 1 focus at (fx,
/// fy), 2 a fixed angle, 4 a region of w*h bytes in the input buffer.
#[unsafe(no_mangle)]
pub extern "C" fn set_constraints(
    flags: u32,
    fx: f64,
    fy: f64,
    angle: f64,
    min_size: f64,
    max_size: f64,
) {
    with(|e| {
        let Some(m) = e.model.as_mut() else { return };
        let opts = Options {
            focus: (flags & 1 != 0).then_some((fx, fy)),
            angle: (flags & 2 != 0).then_some(angle),
            min_size,
            max_size,
            region: (flags & 4 != 0).then_some(&e.input[..]),
        };
        let (w, h) = (m.w, m.h);
        m.set_constraints(Constraints::new(&opts, w, h));
    })
}

/// Runs searches for a shape of the given mode without adding it, keeping
/// the best found since the last call with fresh set (or the last commit),
/// and returns that shape's energy (lower is better).
#[unsafe(no_mangle)]
pub extern "C" fn search(mode: i32, alpha: i32, searches: i32, fresh: u32) -> f64 {
    with(|e| {
        let m = e.model.as_mut().expect("init first");
        let mut s = m.search(ShapeType::from_i32(mode), alpha, searches);
        let energy = m.energy_of(&mut s);
        if let Some(mut b) = e.best.filter(|_| fresh == 0)
            && m.energy_of(&mut b) <= energy
        {
            return m.energy_of(&mut b);
        }
        e.best = Some(s);
        energy
    })
}

/// Adds the best shape found since the last commit, plus up to repeat
/// more, and returns how many numbers describe them at shapes_ptr: for
/// each, its alpha, then the shape as Shape::encode writes it.
#[unsafe(no_mangle)]
pub extern "C" fn commit(repeat: i32) -> usize {
    with(|e| {
        e.shapes.clear();
        let (Some(m), Some(best)) = (e.model.as_mut(), e.best.take()) else {
            return 0;
        };
        for s in m.commit(best, repeat) {
            e.shapes.push(s.alpha as f64);
            s.shape.encode(&mut e.shapes);
        }
        e.shapes.len()
    })
}

/// Adds shapes returned by commit in another model: n numbers, as f64s
/// in the input buffer. Returns the new score.
#[unsafe(no_mangle)]
pub extern "C" fn apply(n: usize) -> f64 {
    with(|e| {
        let m = e.model.as_mut().expect("init first");
        let v: Vec<f64> = e.input[..n * 8]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| f64::from_le_bytes(*b))
            .collect();
        let mut i = 0;
        while i < v.len() {
            let (shape, used) = Shape::decode(&v[i + 1..]).expect("bad shape");
            m.add(shape, v[i] as i32);
            i += 1 + used;
        }
        m.score
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn shape_count() -> usize {
    with(|e| e.model.as_ref().map_or(0, |m| m.shapes.len()))
}

fn output(e: &mut Engine, bytes: Vec<u8>) -> usize {
    e.output = bytes;
    e.output.len()
}

/// Writes the model as an SVG document to output_ptr and returns its length.
#[unsafe(no_mangle)]
pub extern "C" fn svg() -> usize {
    with(|e| {
        let s = e.model.as_ref().map(|m| m.svg()).unwrap_or_default();
        output(e, s.into_bytes())
    })
}

/// Like svg, but the shapes fade in over duration seconds.
#[unsafe(no_mangle)]
pub extern "C" fn animated_svg(duration: f64) -> usize {
    with(|e| {
        let s = e
            .model
            .as_ref()
            .map(|m| m.animated_svg(duration))
            .unwrap_or_default();
        output(e, s.into_bytes())
    })
}

/// Writes an animated GIF to output_ptr and returns its length; delays are
/// in hundredths of a second.
#[unsafe(no_mangle)]
pub extern "C" fn gif(max_side: usize, frames: usize, delay: u32, last_delay: u32) -> usize {
    with(|e| {
        let bytes = match &e.model {
            Some(m) => gif::encode(m, max_side, frames, delay as u16, last_delay as u16),
            None => vec![],
        };
        output(e, bytes)
    })
}

/// Prepares to render frames fitting within max_side x max_side (0 for the
/// output size), rounded to even dimensions if even is set, and returns
/// width << 16 | height.
#[unsafe(no_mangle)]
pub extern "C" fn anim_start(max_side: usize, frames: usize, even: u32) -> u32 {
    with(|e| {
        let Some(m) = &e.model else { return 0 };
        let side = if max_side == 0 {
            m.sw.max(m.sh)
        } else {
            max_side
        };
        let a = Animator::new(m, side, even != 0);
        let size = (a.width as u32) << 16 | a.height as u32;
        e.counts = m.animation_counts(frames);
        e.animator = Some(a);
        size
    })
}

/// Renders frame k (frames must be requested in order) and returns a
/// pointer to its RGBA pixels.
#[unsafe(no_mangle)]
pub extern "C" fn anim_frame(k: usize) -> *const u8 {
    with(|e| {
        let (Some(m), Some(a)) = (&e.model, e.animator.as_mut()) else {
            return std::ptr::null();
        };
        a.frame(m, e.counts[k]).as_ptr()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Colors given after the pixels, as the page passes them, limit the
    // shapes' colors and the average background.
    #[test]
    fn init_with_given_colors() {
        let (w, h) = (32, 24);
        let mut input: Vec<u8> = (0..w * h)
            .flat_map(|i| [(i % w * 8) as u8, (i / w * 10) as u8, 90, 255])
            .collect();
        input.extend([255, 0, 0, 0, 0, 255]);
        with(|e| e.input = input);
        init(w, h, 64, -1, 1.0, 2, 1);
        for _ in 0..5 {
            search(1, 128, 1, 1);
            commit(0);
        }
        with(|e| {
            let m = e.model.as_ref().unwrap();
            let given = |c: &core::Color| [(255, 0, 0), (0, 0, 255)].contains(&(c.r, c.g, c.b));
            assert!(given(&m.background));
            assert_eq!(m.shapes.len(), 5);
            assert!(m.colors.iter().all(given));
        });
    }
}
