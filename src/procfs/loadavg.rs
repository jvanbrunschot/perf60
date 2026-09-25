//! `/proc/loadavg`: `0.06 0.09 0.05 1/208 12345`

use super::{Result, err};

#[derive(Debug, Clone, PartialEq)]
pub struct LoadAvg {
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub running: u64,
    pub total: u64,
}

pub fn parse(input: &str) -> Result<LoadAvg> {
    let f: Vec<&str> = input.split_whitespace().collect();
    if f.len() < 3 {
        return err("loadavg: expected at least 3 fields");
    }
    let num = |s: &str| {
        s.parse::<f64>()
            .map_err(|_| super::ParseError(format!("loadavg: bad number '{s}'")))
    };
    let (running, total) = f
        .get(3)
        .and_then(|s| s.split_once('/'))
        .and_then(|(r, t)| Some((r.parse().ok()?, t.parse().ok()?)))
        .unwrap_or((0, 0));
    Ok(LoadAvg {
        load1: num(f[0])?,
        load5: num(f[1])?,
        load15: num(f[2])?,
        running,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let l = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/loadavg"
        ))
        .unwrap();
        assert!(l.load1 >= 0.0 && l.total > 0);
    }

    #[test]
    fn parses_fields() {
        let l = parse("1.20 0.90 0.80 2/345 999\n").unwrap();
        assert_eq!(
            l,
            LoadAvg {
                load1: 1.2,
                load5: 0.9,
                load15: 0.8,
                running: 2,
                total: 345
            }
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("").is_err());
        assert!(parse("a b c").is_err());
    }
}
