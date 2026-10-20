//! Command-line arguments, parsed like Go's flag package parsed them for
//! the original tool: single- or double-dash flags, with values either
//! after "=" or as the next argument.

pub const USAGE: &str = "\
Usage: primitive [OPTIONS] -i input -o output -n count

  -i path       input image (\"-\" for stdin)
  -o path       output image: .png, .jpg, .svg or .gif (\"-\" for SVG on
                stdout); repeat for several. Put %d in the path to save
                numbered frames as shapes are added.
  -n count      number of shapes; repeat to add more with other settings
                (each -n uses the -m, -a and -rep given before it)
  -m mode       0=combo 1=triangle 2=rect 3=ellipse 4=circle
                5=rotatedrect 6=beziers 7=rotatedellipse 8=polygon (default 1)
  -a alpha      shape opacity, 1-255, or 0 to let the search pick (default 128)
  -rep n        add n extra shapes per step with a reduced search (default 0)
  -bg hex       background color (default: the image's average color)
  -palette p    limit shape colors to a palette: a number of colors chosen
                from the image (up to 256, e.g. 8), or hex colors separated
                by commas (e.g. 264653,e9c46a,e76f51)
  -r size       shrink the input to this size before searching (default 256)
  -s size       output image size (default 1024)
  -j n          parallel workers (default: all cores, up to 16)
  -nth n        with %d in an output path, save every nth frame (default 1)
  -focus x,y    point oriented shapes toward x,y (fractions of width and
                height, e.g. 0.5,0.2)
  -angle deg    point oriented shapes along this angle (counterclockwise)
  -minsize f    smallest shape, as a fraction of the image's longest side
  -maxsize f    largest shape, as a fraction of the image's longest side
  -region path  image whose white areas are the only places shapes may go
  -keep         with -region, keep the input image outside the region
                (shapes are clipped to it) instead of the background color
  -v, -vv       verbose, very verbose";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeConfig {
    pub count: usize,
    pub mode: i32,
    pub alpha: i32,
    pub repeat: i32,
}

#[derive(Debug, PartialEq)]
pub struct Args {
    pub input: String,
    pub outputs: Vec<String>,
    pub configs: Vec<ShapeConfig>,
    pub background: Option<String>,
    pub palette: Option<String>,
    pub input_size: usize,
    pub output_size: usize,
    pub workers: usize,
    pub nth: usize,
    pub focus: Option<(f64, f64)>,
    pub angle: Option<f64>,
    pub min_size: f64,
    pub max_size: f64,
    pub region: Option<String>,
    pub keep: bool,
    pub verbose: u8,
}

fn number<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, String> {
    value
        .trim()
        .parse()
        .map_err(|_| format!("-{flag} must be a number, not {value:?}"))
}

/// The arguments, or None if the usage was asked for (-h).
pub fn parse(argv: impl IntoIterator<Item = String>) -> Result<Option<Args>, String> {
    let mut argv = argv.into_iter();
    let mut input = None;
    let mut outputs = vec![];
    let mut counts = vec![];
    let mut configs = vec![];
    let (mut mode, mut alpha, mut repeat) = (1, 128, 0);
    let mut a = Args {
        input: String::new(),
        outputs: vec![],
        configs: vec![],
        background: None,
        palette: None,
        input_size: 256,
        output_size: 1024,
        workers: 0,
        nth: 1,
        focus: None,
        angle: None,
        min_size: 0.0,
        max_size: 0.0,
        region: None,
        keep: false,
        verbose: 0,
    };
    while let Some(arg) = argv.next() {
        let Some(flag) = arg.strip_prefix("--").or_else(|| arg.strip_prefix('-')) else {
            return Err(format!("unexpected argument {arg:?}"));
        };
        let (name, inline) = match flag.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (flag, None),
        };
        match name {
            "h" | "help" => return Ok(None),
            "v" => a.verbose = a.verbose.max(1),
            "keep" => a.keep = true,
            "vv" => a.verbose = 2,
            _ => {
                let value = match inline.or_else(|| argv.next()) {
                    Some(v) => v,
                    None => return Err(format!("-{name} needs a value")),
                };
                match name {
                    "i" => input = Some(value),
                    "o" => outputs.push(value),
                    "n" => {
                        // Go parsed this leniently; anything but a positive
                        // count is reported below
                        let count = value.trim().parse().unwrap_or(0);
                        counts.push(count);
                        configs.push(ShapeConfig {
                            count,
                            mode,
                            alpha,
                            repeat,
                        });
                    }
                    "m" => mode = number(name, &value)?,
                    "a" => alpha = number(name, &value)?,
                    "rep" => repeat = number(name, &value)?,
                    "bg" => a.background = Some(value),
                    "palette" => a.palette = Some(value),
                    "r" => a.input_size = number(name, &value)?,
                    "s" => a.output_size = number(name, &value)?,
                    "j" => a.workers = number(name, &value)?,
                    "nth" => a.nth = number::<usize>(name, &value)?.max(1),
                    "focus" => {
                        let parts: Vec<&str> = value.split(',').collect();
                        let [x, y] = parts[..] else {
                            return Err("-focus must be x,y".into());
                        };
                        match (x.trim().parse(), y.trim().parse()) {
                            (Ok(x), Ok(y)) => a.focus = Some((x, y)),
                            _ => return Err("-focus must be two numbers, like 0.5,0.2".into()),
                        }
                    }
                    "angle" => {
                        a.angle = Some(
                            value
                                .trim()
                                .parse()
                                .map_err(|_| "-angle must be a number of degrees")?,
                        )
                    }
                    "minsize" => a.min_size = number(name, &value)?,
                    "maxsize" => a.max_size = number(name, &value)?,
                    "region" => a.region = Some(value),
                    _ => return Err(format!("unknown flag -{name}")),
                }
            }
        }
    }
    let mut errors = vec![];
    match input {
        Some(i) => a.input = i,
        None => errors.push("input argument required".to_string()),
    }
    if outputs.is_empty() {
        errors.push("output argument required".into());
    }
    if configs.is_empty() {
        errors.push("number argument required".into());
    }
    if let [only] = &mut configs[..] {
        // with a single -n, the other settings may come after it
        (only.mode, only.alpha, only.repeat) = (mode, alpha, repeat);
    }
    if counts.contains(&0) {
        errors.push("number argument must be > 0".into());
    }
    if let Some(c) = configs.iter().find(|c| !(0..=8).contains(&c.mode)) {
        errors.push(format!("-m must be from 0 to 8, not {}", c.mode));
    }
    if let Some(c) = configs.iter().find(|c| !(0..=255).contains(&c.alpha)) {
        errors.push(format!("-a must be from 0 to 255, not {}", c.alpha));
    }
    if a.min_size < 0.0 || a.max_size < 0.0 || (a.max_size > 0.0 && a.min_size > a.max_size) {
        errors.push("-minsize and -maxsize must be positive, with minsize <= maxsize".into());
    }
    if a.keep && a.region.is_none() {
        errors.push("-keep needs -region".into());
    }
    if !errors.is_empty() {
        return Err(errors.join("\nERROR: "));
    }
    a.outputs = outputs;
    a.configs = configs;
    Ok(Some(a))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &str) -> Result<Args, String> {
        parse(args.split_whitespace().map(String::from))?.ok_or_else(|| "help".into())
    }

    #[test]
    fn defaults() {
        let a = run("-i in.png -o out.png -n 100").unwrap();
        assert_eq!(a.input, "in.png");
        assert_eq!(a.outputs, ["out.png"]);
        assert_eq!(
            a.configs,
            [ShapeConfig {
                count: 100,
                mode: 1,
                alpha: 128,
                repeat: 0
            }]
        );
        assert_eq!(
            (a.input_size, a.output_size, a.nth, a.workers),
            (256, 1024, 1, 0)
        );
    }

    #[test]
    fn settings_apply_to_the_counts_after_them() {
        let a = run("-i a -o b -m 1 -n 50 -m 3 -a 0 -n 20 -rep=2 -n 5").unwrap();
        let got: Vec<_> = a
            .configs
            .iter()
            .map(|c| (c.count, c.mode, c.alpha, c.repeat))
            .collect();
        assert_eq!(got, [(50, 1, 128, 0), (20, 3, 0, 0), (5, 3, 0, 2)]);
        // with one count, settings after it still apply
        let a = run("-i a -o b -n 50 -m 6 --a=40").unwrap();
        assert_eq!((a.configs[0].mode, a.configs[0].alpha), (6, 40));
    }

    #[test]
    fn several_outputs_and_constraints() {
        let a = run(
            "-i a -o x.png -o y.svg -n 1 -focus 0.5,0.25 -angle -30 -minsize 0.1 -palette 8 -vv",
        )
        .unwrap();
        assert_eq!(a.outputs, ["x.png", "y.svg"]);
        assert_eq!(a.palette.as_deref(), Some("8"));
        assert_eq!(a.focus, Some((0.5, 0.25)));
        assert_eq!(a.angle, Some(-30.0));
        assert_eq!(a.min_size, 0.1);
        assert_eq!(a.verbose, 2);
    }

    #[test]
    fn errors() {
        let e = run("-o b").unwrap_err();
        assert!(e.contains("input argument required") && e.contains("number argument required"));
        assert!(run("-i a -o b -n 0").unwrap_err().contains("> 0"));
        assert!(run("-i a -o b -n 5 -m 9").unwrap_err().contains("-m"));
        assert!(run("-i a -o b -n 5 -focus 1").unwrap_err().contains("x,y"));
        assert!(run("-i a -o b -n 5 -s big").unwrap_err().contains("-s"));
        assert!(
            run("-i a -o b -n 5 -what 1")
                .unwrap_err()
                .contains("unknown")
        );
        assert!(run("-i a -o b -n").unwrap_err().contains("needs a value"));
        assert!(
            run("-i a -o b -n 5 extra")
                .unwrap_err()
                .contains("unexpected")
        );
        assert!(
            run("-i a -o b -n 5 -keep")
                .unwrap_err()
                .contains("-keep needs -region")
        );
        assert_eq!(run("-h").unwrap_err(), "help");
    }
}
