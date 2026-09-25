//! `/proc/stat`: CPU time counters (in USER_HZ ticks), context switches, interrupts and the
//! current number of runnable and blocked tasks.
//!
//! ```text
//! cpu  13120 2 11810 6021544 367 14428 4610 0 0 0
//! cpu0 3251 0 2950 1504796 83 3904 1446 0 0 0
//! intr 6038201 0 43350 ...
//! ctxt 6111240
//! processes 55596
//! procs_running 1
//! procs_blocked 0
//! ```

use super::{ParseError, Result, err};

/// CPU time counters of one `cpu` line. Columns missing on old kernels are 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
    /// Already included in `user`.
    pub guest: u64,
    /// Already included in `nice`.
    pub guest_nice: u64,
}

impl CpuTimes {
    /// All elapsed time. `guest` and `guest_nice` are excluded because the kernel already
    /// accounts them in `user` and `nice`.
    pub fn total(&self) -> u64 {
        self.user
            + self.nice
            + self.system
            + self.idle
            + self.iowait
            + self.irq
            + self.softirq
            + self.steal
    }

    /// Counter increase since `earlier`, per field. A counter that went backwards counts as 0.
    pub fn since(&self, earlier: &CpuTimes) -> CpuTimes {
        CpuTimes {
            user: self.user.saturating_sub(earlier.user),
            nice: self.nice.saturating_sub(earlier.nice),
            system: self.system.saturating_sub(earlier.system),
            idle: self.idle.saturating_sub(earlier.idle),
            iowait: self.iowait.saturating_sub(earlier.iowait),
            irq: self.irq.saturating_sub(earlier.irq),
            softirq: self.softirq.saturating_sub(earlier.softirq),
            steal: self.steal.saturating_sub(earlier.steal),
            guest: self.guest.saturating_sub(earlier.guest),
            guest_nice: self.guest_nice.saturating_sub(earlier.guest_nice),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stat {
    /// The aggregate `cpu` line.
    pub total: CpuTimes,
    /// `cpuN` lines as `(N, times)`, in file order.
    pub cpus: Vec<(usize, CpuTimes)>,
    pub ctxt: Option<u64>,
    /// Total interrupts serviced (first number of the `intr` line).
    pub intr: Option<u64>,
    /// Forks since boot.
    pub processes: Option<u64>,
    pub procs_running: Option<u64>,
    pub procs_blocked: Option<u64>,
}

fn parse_times(line: &str, fields: &[&str]) -> Result<CpuTimes> {
    if fields.len() < 4 {
        return err(format!("stat: too few columns in '{line}'"));
    }
    let mut v = [0u64; 10];
    for (slot, f) in v.iter_mut().zip(fields) {
        *slot = f
            .parse()
            .map_err(|_| ParseError(format!("stat: bad number '{f}'")))?;
    }
    Ok(CpuTimes {
        user: v[0],
        nice: v[1],
        system: v[2],
        idle: v[3],
        iowait: v[4],
        irq: v[5],
        softirq: v[6],
        steal: v[7],
        guest: v[8],
        guest_nice: v[9],
    })
}

pub fn parse(input: &str) -> Result<Stat> {
    let mut stat = Stat::default();
    let mut seen_total = false;
    for line in input.lines() {
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        let rest: Vec<&str> = it.collect();
        let first = || rest.first().and_then(|s| s.parse::<u64>().ok());
        match key {
            "cpu" => {
                stat.total = parse_times(line, &rest)?;
                seen_total = true;
            }
            "ctxt" => stat.ctxt = first(),
            "intr" => stat.intr = first(),
            "processes" => stat.processes = first(),
            "procs_running" => stat.procs_running = first(),
            "procs_blocked" => stat.procs_blocked = first(),
            _ => {
                if let Some(n) = key.strip_prefix("cpu").and_then(|n| n.parse().ok()) {
                    stat.cpus.push((n, parse_times(line, &rest)?));
                }
            }
        }
    }
    if !seen_total {
        return err("stat: no aggregate cpu line");
    }
    Ok(stat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let s = parse(include_str!("../../tests/fixtures/linux-arm64/proc/stat")).unwrap();
        assert_eq!(
            s.total,
            CpuTimes {
                user: 26210,
                nice: 2,
                system: 19425,
                idle: 7783341,
                iowait: 857,
                irq: 20523,
                softirq: 6597,
                ..Default::default()
            }
        );
        assert_eq!(s.cpus.len(), 4);
        assert_eq!(s.cpus[2].0, 2);
        assert_eq!(s.cpus[0].1.user, 6007);
        assert_eq!(s.cpus[3].1.nice, 1);
        assert_eq!(s.ctxt, Some(11879317));
        assert_eq!(s.intr, Some(10375061));
        assert_eq!(s.processes, Some(55596));
        assert_eq!(s.procs_running, Some(1));
        assert_eq!(s.procs_blocked, Some(0));
    }

    #[test]
    fn parses_processes_counter() {
        let s = parse(include_str!("../../tests/fixtures/linux-legacy/proc/stat")).unwrap();
        assert_eq!(s.processes, Some(1234567));
        assert_eq!(parse("cpu 1 2 3 4\n").unwrap().processes, None);
        assert_eq!(parse("cpu 1 2 3 4\nprocesses x\n").unwrap().processes, None);
    }

    #[test]
    fn total_excludes_guest_time() {
        let s = parse("cpu 10 20 30 40 5 6 7 8 9 3\n").unwrap();
        assert_eq!(s.total.guest, 9);
        assert_eq!(s.total.guest_nice, 3);
        assert_eq!(s.total.total(), 10 + 20 + 30 + 40 + 5 + 6 + 7 + 8);
    }

    #[test]
    fn tolerates_old_kernel_columns() {
        // 2.6.0-era: 7 columns, no steal/guest; no procs_* lines.
        let s = parse("cpu 1 2 3 4 5 6 7\ncpu0 1 2 3 4\nctxt 9\n").unwrap();
        assert_eq!(s.total.softirq, 7);
        assert_eq!(s.total.steal, 0);
        assert_eq!(s.cpus[0].1.idle, 4);
        assert_eq!(s.cpus[0].1.iowait, 0);
        assert_eq!(s.ctxt, Some(9));
        assert_eq!(s.procs_running, None);
    }

    #[test]
    fn since_is_per_field_and_saturating() {
        let a = parse("cpu 10 0 10 100 0 0 0 0 0 0\n").unwrap().total;
        let b = parse("cpu 15 0 12 90 1 0 0 0 0 0\n").unwrap().total;
        let d = b.since(&a);
        assert_eq!((d.user, d.system, d.idle, d.iowait), (5, 2, 0, 1));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("").is_err());
        assert!(parse("cpu 1 2\n").is_err());
        assert!(parse("cpu a b c d\n").is_err());
        assert!(parse("ctxt 5\n").is_err());
    }
}
