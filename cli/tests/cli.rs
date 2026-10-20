//! Runs the built primitive binary on a small generated image.

use std::path::PathBuf;
use std::process::Command;

fn primitive() -> Command {
    Command::new(env!("CARGO_BIN_EXE_primitive"))
}

/// A fresh directory for one test's files, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("primitive-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A 300 x 200 image: a dark disc on a horizontal gradient.
fn write_input(dir: &TempDir) -> String {
    let im = image::RgbImage::from_fn(300, 200, |x, y| {
        let (dx, dy) = (x as f64 - 150.0, y as f64 - 100.0);
        if dx * dx + dy * dy < 60.0 * 60.0 {
            image::Rgb([30, 20, 90])
        } else {
            image::Rgb([(x * 255 / 300) as u8, 180, 60])
        }
    });
    let path = dir.path("in.png");
    im.save(&path).unwrap();
    path
}

#[test]
fn writes_every_format() {
    let dir = TempDir::new("formats");
    let input = write_input(&dir);
    let out = primitive()
        .args([
            "-i", &input, "-n", "12", "-m", "0", "-r", "64", "-s", "240", "-j", "3", "-v",
        ])
        .args(["-o", &dir.path("out.png"), "-o", &dir.path("out.jpg")])
        .args(["-o", &dir.path("out.svg"), "-o", &dir.path("out.gif")])
        .args(["-o", &dir.path("frame%02d.png"), "-nth", "5"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log = String::from_utf8_lossy(&out.stdout);
    assert!(log.contains("12: t="), "{log}");

    // outputs keep the input's aspect ratio (that of the 64 x 43 working
    // image, as in Go)
    let png = image::open(dir.path("out.png")).unwrap();
    assert_eq!((png.width(), png.height()), (240, 161));
    let jpg = image::open(dir.path("out.jpg")).unwrap();
    assert_eq!((jpg.width(), jpg.height()), (240, 161));
    let svg = std::fs::read_to_string(dir.path("out.svg")).unwrap();
    assert!(svg.starts_with("<svg") && svg.matches("fill-opacity").count() >= 12);
    let gif = std::fs::read(dir.path("out.gif")).unwrap();
    assert!(gif.starts_with(b"GIF89a"));
    image::load_from_memory(&gif).unwrap();
    for n in ["05", "10", "12"] {
        assert!(
            std::fs::exists(dir.path(&format!("frame{n}.png"))).unwrap(),
            "frame {n}"
        );
    }
    assert!(!std::fs::exists(dir.path("frame04.png")).unwrap());

    // the shapes reproduce the image better than its average color alone
    let target =
        image::open(&input)
            .unwrap()
            .resize_exact(240, 161, image::imageops::FilterType::Triangle);
    let diff = |a: &image::DynamicImage| {
        let (a, b) = (a.to_rgb8(), target.to_rgb8());
        a.as_raw()
            .iter()
            .zip(b.as_raw())
            .map(|(x, y)| (*x as f64 - *y as f64).powi(2))
            .sum::<f64>()
    };
    let flat = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        240,
        161,
        image::Rgb([128, 140, 70]),
    ));
    assert!(diff(&png) < diff(&flat) * 0.7);
}

#[test]
fn constraints_and_stdout() {
    let dir = TempDir::new("constraints");
    let input = write_input(&dir);
    // only the left half may hold shapes
    let region = image::GrayImage::from_fn(300, 200, |x, _| {
        image::Luma([if x < 150 { 255 } else { 0 }])
    });
    region.save(dir.path("region.png")).unwrap();
    let out = primitive()
        .args([
            "-i", &input, "-o", "-", "-n", "15", "-m", "5", "-r", "60", "-s", "120",
        ])
        .args([
            "-region",
            &dir.path("region.png"),
            "-angle",
            "30",
            "-bg",
            "#808080",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let svg = String::from_utf8(out.stdout).unwrap();
    assert!(svg.contains("fill=\"#808080\""), "background");
    // every rotated rectangle is centered in the left half, along 30 degrees
    let mut shapes = 0;
    for part in svg.split("translate(").skip(1) {
        let nums: Vec<f64> = part
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .filter_map(|s| s.parse().ok())
            .collect();
        if nums.len() < 3 || part.starts_with("0.5 0.5") {
            continue; // the group's own transform
        }
        shapes += 1;
        assert!(nums[0] < 33.0, "center x {} outside the region", nums[0]);
        // the long side follows the angle, whichever side that is
        assert!(nums[2] == -30.0 || nums[2] == 60.0, "angle {}", nums[2]);
    }
    assert!(shapes > 0);
}

#[test]
fn keeps_the_image_outside_the_region() {
    let dir = TempDir::new("keep");
    let input = write_input(&dir);
    // convert only the disc; the gradient around it stays
    let region = image::GrayImage::from_fn(300, 200, |x, y| {
        let (dx, dy) = (x as f64 - 150.0, y as f64 - 100.0);
        image::Luma([if dx * dx + dy * dy < 60.0 * 60.0 {
            255
        } else {
            0
        }])
    });
    region.save(dir.path("region.png")).unwrap();
    let out = primitive()
        .args([
            "-i",
            &input,
            "-o",
            &dir.path("out.png"),
            "-n",
            "10",
            "-r",
            "100",
            "-s",
            "300",
        ])
        .args(["-region", &dir.path("region.png"), "-keep"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let png = image::open(dir.path("out.png")).unwrap().to_rgb8();
    let original = image::open(&input).unwrap().to_rgb8();
    // the shape of the 100 x 67 working image
    assert_eq!(png.dimensions(), (300, 201));
    // far from the disc, the output is the input
    for (x, y) in [(5, 5), (290, 10), (20, 190), (280, 180)] {
        let (a, b) = (png.get_pixel(x, y), original.get_pixel(x, y));
        let diff = (0..3)
            .map(|c| (a[c] as i32 - b[c] as i32).abs())
            .max()
            .unwrap();
        assert!(diff <= 3, "({x}, {y}): {a:?} vs {b:?}");
    }
}

#[test]
fn reports_bad_arguments() {
    let out = primitive()
        .args(["-i", "x.png", "-n", "5"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("output argument required") && err.contains("Usage:"),
        "{err}"
    );

    let out = primitive()
        .args(["-i", "missing.png", "-o", "x.png", "-n", "5"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("missing.png"));
}
