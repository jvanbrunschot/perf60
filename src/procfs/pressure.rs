//! `/proc/pressure/{cpu,memory,io}` and cgroup v2 `*.pressure` (Pressure Stall Information):
//!
//! ```text
//! some avg10=0.00 avg60=0.00 avg300=0.00 total=18561780
//! full avg10=0.00 avg60=0.00 avg300=0.00 total=0
//! ```
//!
//! `total` is the cumulative stall time in microseconds. `full` may be absent for cpu on older
//! kernels.

use super::{ParseError, Result, err};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PsiLine {
    pub avg10: f64,
    pub avg60: f64,
    pub avg300: f64,
    /// Cumulative stall time in microseconds.
    pub total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Psi {
    pub some: PsiLine,
    pub full: Option<PsiLine>,
}

pub fn parse(input: &str) -> Result<Psi> {
    let mut some = None;
    let mut full = None;
    for line in input.lines() {
        let mut f = line.split_whitespace();
        let slot = match f.next() {
            Some("some") => &mut some,
            Some("full") => &mut full,
            _ => continue,
        };
        *slot = Some(parse_line(f)?);
    }
    match some {
        Some(some) => Ok(Psi { some, full }),
        None => err("pressure: missing 'some' line"),
    }
}

fn parse_line<'a>(fields: impl Iterator<Item = &'a str>) -> Result<PsiLine> {
    let mut l = PsiLine::default();
    let mut seen_total = false;
    for kv in fields {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        let bad = || ParseError(format!("pressure: bad value '{kv}'"));
        match k {
            "avg10" => l.avg10 = v.parse().map_err(|_| bad())?,
            "avg60" => l.avg60 = v.parse().map_err(|_| bad())?,
            "avg300" => l.avg300 = v.parse().map_err(|_| bad())?,
            "total" => {
                l.total = v.parse().map_err(|_| bad())?;
                seen_total = true;
            }
            _ => {}
        }
    }
    if !seen_total {
        return err("pressure: missing total=");
    }
    Ok(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture_files() {
        let cpu = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/pressure/cpu"
        ))
        .unwrap();
        assert_eq!(cpu.some.total, 18561780);
        assert_eq!(cpu.full.unwrap().total, 0);
        let mem = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/pressure/memory"
        ))
        .unwrap();
        assert_eq!(mem.some.total, 225);
        assert_eq!(mem.full.unwrap().total, 170);
        let io = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/pressure/io"
        ))
        .unwrap();
        assert_eq!(io.some.total, 1775149);
        assert_eq!(io.full.unwrap().total, 1659609);
    }

    #[test]
    fn parses_some_and_full() {
        let p = parse(
            "some avg10=1.50 avg60=0.75 avg300=0.25 total=12345\n\
             full avg10=0.10 avg60=0.05 avg300=0.01 total=678\n",
        )
        .unwrap();
        assert_eq!(
            p.some,
            PsiLine {
                avg10: 1.5,
                avg60: 0.75,
                avg300: 0.25,
                total: 12345
            }
        );
        let full = p.full.unwrap();
        assert_eq!(full.total, 678);
        assert_eq!(full.avg10, 0.1);
    }

    #[test]
    fn cpu_without_full_line() {
        let p = parse("some avg10=0.00 avg60=0.00 avg300=0.00 total=42\n").unwrap();
        assert_eq!(p.some.total, 42);
        assert_eq!(p.full, None);
    }

    #[test]
    fn rejects_missing_some() {
        assert!(parse("").is_err());
        assert!(parse("full avg10=0.00 avg60=0.00 avg300=0.00 total=1\n").is_err());
        assert!(parse("some avg10=x avg60=0.00 avg300=0.00 total=1\n").is_err());
        assert!(parse("some avg10=0.00 avg60=0.00 avg300=0.00\n").is_err());
    }
}
