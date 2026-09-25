//! Hand-rolled argument parsing (six flags do not justify a dependency).

pub const USAGE: &str = "\
Usage: perf60 [OPTIONS]

Linux performance triage in 60 seconds, after Brendan Gregg's checklist.
Reads /proc, /sys and /dev/kmsg directly; runs no external commands.

Options:
  -i, --interval <SECONDS>  Seconds between samples [default: 1]
  -c, --count <N>           Number of sampling intervals [default: 5]
  -j, --json                Print a JSON report
      --no-color            Disable ANSI colors (also honors NO_COLOR)
  -v, --verbose             Show detail lines for every section, not only problems
  -h, --help                Print help
  -V, --version             Print version

Exit status: 0 OK, 1 WARN, 2 CRIT, 3 usage error or unsupported OS.";

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub interval: f64,
    pub count: usize,
    pub json: bool,
    pub color: bool,
    pub verbose: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            interval: 1.0,
            count: 5,
            json: false,
            color: true,
            verbose: false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Run(Options),
    Help,
    Version,
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Command, String> {
    let mut opts = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_owned(), Some(v.to_owned())),
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| args.next())
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match flag.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "-j" | "--json" => opts.json = true,
            "--no-color" => opts.color = false,
            "-v" | "--verbose" => opts.verbose = true,
            "-i" | "--interval" => {
                let v = value("--interval")?;
                opts.interval = v
                    .parse::<f64>()
                    .ok()
                    .filter(|x| x.is_finite() && *x > 0.0)
                    .ok_or_else(|| format!("invalid --interval '{v}': expected seconds > 0"))?;
            }
            "-c" | "--count" => {
                let v = value("--count")?;
                opts.count =
                    v.parse::<usize>().ok().filter(|x| *x >= 1).ok_or_else(|| {
                        format!("invalid --count '{v}': expected an integer >= 1")
                    })?;
            }
            _ => return Err(format!("unknown argument '{arg}'")),
        }
    }
    Ok(Command::Run(opts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults() {
        assert_eq!(p(&[]), Ok(Command::Run(Options::default())));
        let Ok(Command::Run(o)) = p(&[]) else {
            panic!()
        };
        assert_eq!(
            (o.interval, o.count, o.json, o.color),
            (1.0, 5, false, true)
        );
    }

    #[test]
    fn flags_and_values() {
        let Ok(Command::Run(o)) = p(&["--interval", "0.5", "-c", "4", "--json", "--no-color"])
        else {
            panic!()
        };
        assert_eq!(
            (o.interval, o.count, o.json, o.color),
            (0.5, 4, true, false)
        );
        let Ok(Command::Run(o)) = p(&["--count=2", "--interval=2"]) else {
            panic!()
        };
        assert_eq!((o.interval, o.count), (2.0, 2));
    }

    #[test]
    fn errors() {
        assert!(p(&["--count", "0"]).is_err());
        assert!(p(&["--interval", "-1"]).is_err());
        assert!(p(&["--interval", "nan"]).is_err());
        assert!(p(&["--count"]).is_err());
        assert!(p(&["--bogus"]).unwrap_err().contains("--bogus"));
    }

    #[test]
    fn help_and_version() {
        assert_eq!(p(&["-h"]), Ok(Command::Help));
        assert_eq!(p(&["--version"]), Ok(Command::Version));
    }
}
