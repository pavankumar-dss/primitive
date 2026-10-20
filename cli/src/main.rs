//! The primitive command-line tool: reproduces an image with geometric
//! shapes, using the same engine as the browser version (../engine).

mod args;

use args::{Args, USAGE};
use image::imageops::FilterType;
use primitive_engine::constraints::{Constraints, Options};
use primitive_engine::core::{self, Color};
use primitive_engine::gif;
use primitive_engine::model::{Model, State};
use primitive_engine::render::Animator;
use primitive_engine::shape::ShapeType;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Independent searches per shape, spread over the workers.
const SEARCHES: i32 = 16;

fn main() {
    let args = match args::parse(std::env::args().skip(1)) {
        Ok(Some(a)) => a,
        Ok(None) => {
            println!("{USAGE}");
            return;
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            eprintln!("{USAGE}");
            std::process::exit(1);
        }
    };
    if let Err(e) = run(&args) {
        eprintln!("ERROR: {e}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<(), String> {
    let log = |level: u8, msg: &str| {
        if args.verbose >= level {
            println!("{msg}");
        }
    };

    log(1, &format!("reading {}", args.input));
    let input = load_image(&args.input)?;
    let input = if args.input_size > 0 {
        thumbnail(input, args.input_size)
    } else {
        input.to_rgba8()
    };
    let (w, h) = (input.width() as usize, input.height() as usize);
    let pixels = input.into_raw();

    let background = match &args.background {
        Some(hex) => {
            parse_hex(hex).ok_or_else(|| format!("-bg must be a hex color, not {hex:?}"))?
        }
        None => core::average_color(&pixels),
    };
    let region = match &args.region {
        Some(path) => {
            let r = load_image(path).map_err(|e| format!("reading -region: {e}"))?;
            let r = image::imageops::resize(
                &r.to_luma_alpha8(),
                w as u32,
                h as u32,
                FilterType::Triangle,
            );
            // transparent counts as black
            Some(
                r.pixels()
                    .map(|p| (p[0] as u32 * p[1] as u32 / 255) as u8)
                    .collect::<Vec<u8>>(),
            )
        }
        None => None,
    };
    let options = Options {
        focus: args.focus,
        angle: args.angle,
        min_size: args.min_size,
        max_size: args.max_size,
        region: region.as_deref(),
    };

    // one model per worker, all holding the same shapes (see step)
    let workers = match args.workers {
        0 => std::thread::available_parallelism().map_or(1, |n| n.get()),
        n => n,
    }
    .clamp(1, SEARCHES as usize);
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64);
    let mut models: Vec<Model> = (0..workers as u64)
        .map(|k| {
            let mut m = Model::new(pixels.clone(), w, h, background, args.output_size, seed + k);
            m.set_constraints(Constraints::new(&options, w, h));
            m
        })
        .collect();
    log(2, &format!("{workers} workers, working size {w}x{h}"));
    log(1, &format!("0: t=0.000, score={:.6}", models[0].score));

    let start = Instant::now();
    let mut renderer: Option<Animator> = None;
    let total: usize = args.configs.iter().map(|c| c.count).sum();
    let mut frame = 0;
    for config in &args.configs {
        log(
            1,
            &format!(
                "count={}, mode={}, alpha={}, repeat={}",
                config.count, config.mode, config.alpha, config.repeat
            ),
        );
        for _ in 0..config.count {
            frame += 1;
            let t = Instant::now();
            let before: u64 = models.iter().map(|m| m.evaluations).sum();
            step(
                &mut models,
                ShapeType::from_i32(config.mode),
                config.alpha,
                config.repeat,
            );
            let n = models.iter().map(|m| m.evaluations).sum::<u64>() - before;
            log(
                1,
                &format!(
                    "{frame}: t={:.3}, score={:.6}, n={n}, n/s={}",
                    start.elapsed().as_secs_f64(),
                    models[0].score,
                    number_string(n as f64 / t.elapsed().as_secs_f64())
                ),
            );

            let model = &models[0];
            for output in &args.outputs {
                let ext = if output == "-" {
                    "svg".to_string()
                } else {
                    Path::new(output)
                        .extension()
                        .map(|e| e.to_string_lossy().to_lowercase())
                        .unwrap_or_default()
                };
                let numbered = output.contains('%');
                let save_frame = numbered && ext != "gif" && frame % args.nth == 0;
                if !(save_frame || frame == total) {
                    continue;
                }
                let path = if numbered {
                    frame_path(output, frame)
                } else {
                    output.clone()
                };
                log(1, &format!("writing {path}"));
                let mut render = || {
                    renderer
                        .get_or_insert_with(|| Animator::new(model, args.output_size, false))
                        .frame(model, model.shapes.len())
                        .to_vec()
                };
                let (sw, sh) = (model.sw as u32, model.sh as u32);
                let written = match ext.as_str() {
                    "png" => image::save_buffer(&path, &render(), sw, sh, image::ColorType::Rgba8)
                        .map_err(|e| e.to_string()),
                    "jpg" | "jpeg" => save_jpeg(&path, render(), sw, sh),
                    "svg" if path == "-" => std::io::stdout()
                        .write_all(model.svg().as_bytes())
                        .map_err(|e| e.to_string()),
                    "svg" => std::fs::write(&path, model.svg()).map_err(|e| e.to_string()),
                    "gif" => std::fs::write(
                        &path,
                        gif::encode(model, model.shapes.len(), args.output_size, 100, 6, 250),
                    )
                    .map_err(|e| e.to_string()),
                    _ => Err(format!("unrecognized file extension: .{ext}")),
                };
                written.map_err(|e| format!("writing {path}: {e}"))?;
            }
        }
    }
    Ok(())
}

/// Finds the next shape and adds it to every model. Each worker runs
/// searches until the SEARCHES are used up, so faster cores do more of
/// them; the model that found the best shape commits it (plus any
/// repeats), and the others add the same shapes.
fn step(models: &mut [Model], t: ShapeType, alpha: i32, repeat: i32) {
    let left = AtomicI32::new(SEARCHES);
    let found: Vec<Option<(f64, State)>> = std::thread::scope(|s| {
        let handles: Vec<_> = models
            .iter_mut()
            .map(|m| {
                let left = &left;
                s.spawn(move || {
                    let mut best: Option<(f64, State)> = None;
                    while left.fetch_sub(1, Ordering::Relaxed) > 0 {
                        let mut state = m.search(t, alpha, 1);
                        let energy = m.energy_of(&mut state);
                        if best.is_none_or(|(e, _)| energy < e) {
                            best = Some((energy, state));
                        }
                    }
                    best
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let Some((k, state)) = found
        .into_iter()
        .enumerate()
        .filter_map(|(k, f)| f.map(|(e, s)| (k, e, s)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _, s)| (k, s))
    else {
        return;
    };
    let added = models[k].commit(state, repeat);
    for (i, m) in models.iter_mut().enumerate() {
        if i != k {
            for s in &added {
                m.add(s.shape, s.alpha);
            }
        }
    }
}

fn load_image(path: &str) -> Result<image::DynamicImage, String> {
    let read = if path == "-" {
        let mut buf = vec![];
        std::io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        image::load_from_memory(&buf)
    } else {
        image::ImageReader::open(path)
            .map_err(|e| format!("{path}: {e}"))?
            .with_guessed_format()
            .map_err(|e| format!("{path}: {e}"))?
            .decode()
    };
    read.map_err(|e| format!("{path}: {e}"))
}

/// The image scaled down (bilinear, keeping its aspect ratio) to fit within
/// size x size, if it is larger.
fn thumbnail(im: image::DynamicImage, size: usize) -> image::RgbaImage {
    let (w, h) = (im.width() as f64, im.height() as f64);
    let im = im.to_rgba8();
    if w <= size as f64 && h <= size as f64 {
        return im;
    }
    let scale = (size as f64 / w).min(size as f64 / h);
    let nw = ((w * scale).round() as u32).max(1);
    let nh = ((h * scale).round() as u32).max(1);
    image::imageops::resize(&im, nw, nh, FilterType::Triangle)
}

fn save_jpeg(path: &str, rgba: Vec<u8>, w: u32, h: u32) -> Result<(), String> {
    let im = image::RgbaImage::from_raw(w, h, rgba).ok_or("bad frame size")?;
    let rgb = image::DynamicImage::ImageRgba8(im).to_rgb8();
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 95)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())
}

/// A color from hex: RGB, RGBA, RRGGBB or RRGGBBAA, with or without "#".
fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    let digits: Vec<i32> = s
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as i32))
        .collect::<Option<_>>()?;
    let channel = |i: usize| match digits.len() {
        3 | 4 => digits.get(i).map(|d| d * 17),
        6 | 8 => Some(digits.get(2 * i)? * 16 + digits.get(2 * i + 1)?),
        _ => None,
    };
    Some(Color {
        r: channel(0)?,
        g: channel(1)?,
        b: channel(2)?,
        a: if digits.len().is_multiple_of(4) {
            channel(3)?
        } else {
            255
        },
    })
}

/// The output path for a frame: the first %d (or %0Nd, %Nd) in the pattern
/// replaced by the frame number, as Go's Sprintf did.
fn frame_path(pattern: &str, frame: usize) -> String {
    if let Some(i) = pattern.find('%') {
        let rest = &pattern[i + 1..];
        if let Some(d) = rest.find('d') {
            let spec = &rest[..d];
            if spec.chars().all(|c| c.is_ascii_digit()) {
                let width: usize = spec.parse().unwrap_or(0);
                let number = if spec.starts_with('0') {
                    format!("{frame:0width$}")
                } else {
                    format!("{frame:width$}")
                };
                return format!("{}{number}{}", &pattern[..i], &rest[d + 1..]);
            }
        }
    }
    pattern.to_string()
}

/// x with a k, M or G suffix, as the Go tool printed speeds.
fn number_string(mut x: f64) -> String {
    for suffix in ["", "k", "M", "G"] {
        if x < 1000.0 {
            return format!("{x:.1}{suffix}");
        }
        x /= 1000.0;
    }
    format!("{x:.1}T")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colors() {
        let c = |r, g, b, a| Some(Color { r, g, b, a });
        assert_eq!(parse_hex("#ff8000"), c(255, 128, 0, 255));
        assert_eq!(parse_hex("f80"), c(255, 136, 0, 255));
        assert_eq!(parse_hex("ff800080"), c(255, 128, 0, 128));
        assert_eq!(parse_hex("12345"), None);
        assert_eq!(parse_hex("zzzzzz"), None);
    }

    #[test]
    fn frame_paths() {
        assert_eq!(frame_path("out%d.png", 7), "out7.png");
        assert_eq!(frame_path("out%03d.png", 7), "out007.png");
        assert_eq!(frame_path("f/%d-x.svg", 12), "f/12-x.svg");
        assert_eq!(frame_path("100%.png", 3), "100%.png");
    }

    #[test]
    fn number_strings() {
        assert_eq!(number_string(950.0), "950.0");
        assert_eq!(number_string(12345.0), "12.3k");
        assert_eq!(number_string(2.5e6), "2.5M");
    }
}
