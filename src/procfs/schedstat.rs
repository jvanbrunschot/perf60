//! `/proc/schedstat`: per-CPU scheduler statistics. From version 15 on, each `cpuN` line has
//! nine counters; the last three are time spent running, time spent waiting on the runqueue
//! (run_delay) and the number of timeslices run. `domainN` lines are ignored.
//!
//! ```text
//! version 17
//! timestamp 4314327485
//! cpu0 0 0 0 0 0 0 137596991176 28156758512 1600003
//! domain0 MC f 0 0 0 ...
//! ```

use super::{ParseError, Result, err};

/// Oldest format with nanosecond run_delay in fields 7-9.
const MIN_VERSION: u32 = 15;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuSched {
    /// Time spent running (ns).
    pub run_ns: u64,
    /// Time spent waiting on a runqueue (ns).
    pub run_delay_ns: u64,
    /// Timeslices run on this CPU.
    pub timeslices: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SchedStat {
    pub version: u32,
    /// `cpuN` lines as `(N, counters)`, in file order.
    pub cpus: Vec<(usize, CpuSched)>,
}

pub fn parse(input: &str) -> Result<SchedStat> {
    let mut version = None;
    let mut cpus = Vec::new();
    for line in input.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let Some(key) = f.first() else { continue };
        if *key == "version" {
            version = f.get(1).and_then(|v| v.parse::<u32>().ok());
            continue;
        }
        let Some(n) = key.strip_prefix("cpu").and_then(|n| n.parse().ok()) else {
            continue;
        };
        if f.len() < 10 {
            return err(format!("schedstat: too few columns in '{line}'"));
        }
        let num = |i: usize| {
            f[i].parse::<u64>()
                .map_err(|_| ParseError(format!("schedstat: bad number '{}'", f[i])))
        };
        cpus.push((
            n,
            CpuSched {
                run_ns: num(7)?,
                run_delay_ns: num(8)?,
                timeslices: num(9)?,
            },
        ));
    }
    let Some(version) = version else {
        return err("schedstat: no version line");
    };
    if version < MIN_VERSION {
        return err(format!("schedstat: unsupported version {version}"));
    }
    if cpus.is_empty() {
        return err("schedstat: no cpu lines");
    }
    Ok(SchedStat { version, cpus })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let s = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/schedstat"
        ))
        .unwrap();
        assert_eq!(s.version, 17);
        assert_eq!(s.cpus.len(), 4);
        assert_eq!(
            s.cpus[0],
            (
                0,
                CpuSched {
                    run_ns: 137596991176,
                    run_delay_ns: 28156758512,
                    timeslices: 1600003,
                }
            )
        );
        assert_eq!(s.cpus[1].0, 1);
    }

    #[test]
    fn parses_legacy_version_15() {
        let s = parse(include_str!(
            "../../tests/fixtures/linux-legacy/proc/schedstat"
        ))
        .unwrap();
        assert_eq!(s.version, 15);
        assert_eq!(s.cpus.len(), 2);
        assert_eq!(s.cpus[1].1.run_delay_ns, 1134567890123);
        assert_eq!(s.cpus[1].1.timeslices, 97765432);
    }

    #[test]
    fn rejects_old_versions_and_garbage() {
        // Version 14 had 12 fields per cpu line.
        assert!(parse("version 14\ncpu0 0 0 0 0 0 0 0 0 0 1 2 3\n").is_err());
        assert!(parse("").is_err());
        assert!(parse("cpu0 0 0 0 0 0 0 1 2 3\n").is_err());
        assert!(parse("version 15\n").is_err());
        assert!(parse("version 15\ncpu0 0 0 0\n").is_err());
        assert!(parse("version 15\ncpu0 0 0 0 0 0 0 x 2 3\n").is_err());
    }
}
