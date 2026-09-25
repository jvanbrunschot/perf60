//! `pidstat 1`: per-process CPU usage over the sampling window, plus D-state and zombie counts.

use std::collections::BTreeMap;

use crate::check::{Check, Context, SampleError, Section};
use crate::procfs::pid_stat;
use crate::source::Source;
use crate::units;

const PROC: &str = "/proc";
const TOP_N: usize = 5;
const LIST_N: usize = 5;
/// WARN when one process uses more than this percentage of total CPU capacity.
const SATURATION_PCT: f64 = 90.0;
const ZOMBIE_WARN: usize = 50;

/// Clock ticks per second for utime/stime (`USER_HZ`).
fn clock_ticks_per_sec() -> f64 {
    #[cfg(all(target_os = "linux", not(test)))]
    {
        // SAFETY: sysconf has no preconditions.
        let v = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        if v > 0 {
            return v as f64;
        }
    }
    100.0
}

/// `(t, utime, stime)` at one sample.
type Point = (f64, u64, u64);

struct Proc {
    comm: String,
    starttime: u64,
    state: char,
    first: Point,
    last: Point,
    /// Index of the last scan in which this pid was seen.
    seen: usize,
}

pub struct Processes {
    procs: BTreeMap<u32, Proc>,
    /// Number of successful `/proc` scans so far.
    scans: usize,
    error: SampleError,
    own_pid: u32,
    hz: f64,
}

impl Default for Processes {
    fn default() -> Self {
        Processes {
            procs: BTreeMap::new(),
            scans: 0,
            error: SampleError::default(),
            own_pid: std::process::id(),
            hz: clock_ticks_per_sec(),
        }
    }
}

impl Check for Processes {
    fn id(&self) -> &'static str {
        "processes"
    }

    fn sample(&mut self, src: &dyn Source, t: f64) {
        let names = match src.read_dir(PROC) {
            Ok(n) => n,
            Err(e) => {
                self.error.record(PROC, &e);
                return;
            }
        };
        self.scans += 1;
        let scan = self.scans;
        for name in names {
            let Ok(pid) = name.parse::<u32>() else {
                continue;
            };
            if pid == self.own_pid {
                continue;
            }
            // Processes exit between readdir and read all the time: ignore those errors.
            let Ok(text) = src.read_to_string(&format!("{PROC}/{pid}/stat")) else {
                continue;
            };
            let Ok(st) = pid_stat::parse(&text) else {
                continue;
            };
            let point = (t, st.utime, st.stime);
            match self.procs.get_mut(&pid) {
                Some(p) if p.starttime == st.starttime => {
                    p.last = point;
                    p.state = st.state;
                    p.comm = st.comm;
                    p.seen = scan;
                }
                _ => {
                    // New process, or the pid was reused by a different one.
                    self.procs.insert(
                        pid,
                        Proc {
                            comm: st.comm,
                            starttime: st.starttime,
                            state: st.state,
                            first: point,
                            last: point,
                            seen: scan,
                        },
                    );
                }
            }
        }
    }

    fn evaluate(&self, ctx: &Context) -> Section {
        let s = Section::new("processes", "Top processes", "pidstat 1");
        if self.scans == 0 {
            return s.skipped(self.error.get().unwrap_or("no samples"));
        }
        let current: Vec<(u32, &Proc)> = self
            .procs
            .iter()
            .filter(|(_, p)| p.seen == self.scans)
            .map(|(pid, p)| (*pid, p))
            .collect();
        if current.is_empty() {
            return s.skipped("no process readable in /proc");
        }
        let mut usage: Vec<Usage> = self
            .procs
            .iter()
            .map(|(pid, p)| usage(*pid, p, self.hz))
            .filter(|u| u.cpu > 0.0)
            .collect();
        usage.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(a.pid.cmp(&b.pid)));
        let in_state = |c: char| -> Vec<(u32, &str)> {
            current
                .iter()
                .filter(|(_, p)| p.state == c)
                .map(|(pid, p)| (*pid, p.comm.as_str()))
                .collect()
        };
        evaluate(
            s,
            current.len(),
            &usage,
            &in_state('D'),
            in_state('Z').len(),
            ctx.cpus(),
        )
    }
}

struct Usage {
    pid: u32,
    comm: String,
    usr: f64,
    sys: f64,
    cpu: f64,
}

/// %usr, %sys and %cpu over this pid's own first→last sample.
fn usage(pid: u32, p: &Proc, hz: f64) -> Usage {
    let dt = p.last.0 - p.first.0;
    let pct = |a: u64, b: u64| {
        if dt <= 0.0 || hz <= 0.0 {
            0.0
        } else {
            b.saturating_sub(a) as f64 / hz / dt * 100.0
        }
    };
    let usr = pct(p.first.1, p.last.1);
    let sys = pct(p.first.2, p.last.2);
    Usage {
        pid,
        comm: p.comm.clone(),
        usr,
        sys,
        cpu: usr + sys,
    }
}

fn list(procs: &[(u32, &str)]) -> String {
    let mut s: Vec<String> = procs
        .iter()
        .take(LIST_N)
        .map(|(pid, comm)| format!("{comm}({pid})"))
        .collect();
    if procs.len() > LIST_N {
        s.push(format!("+{} more", procs.len() - LIST_N));
    }
    s.join(", ")
}

fn evaluate(
    mut s: Section,
    total: usize,
    usage: &[Usage],
    d_state: &[(u32, &str)],
    zombies: usize,
    cpus: f64,
) -> Section {
    let noun = if total == 1 { "process" } else { "processes" };
    let mut summary = format!("{total} {noun}, ");
    match usage.first() {
        Some(top) => summary.push_str(&format!(
            "top: {} {:.0}% (pid {})",
            top.comm, top.cpu, top.pid
        )),
        None => summary.push_str("all idle"),
    }
    if !d_state.is_empty() {
        summary.push_str(&format!(", {} in D state", d_state.len()));
    }
    if zombies > 0 {
        summary.push_str(&format!(", {zombies} zombies"));
    }
    s.summary(summary);

    for u in usage.iter().take(TOP_N) {
        s.detail(format!(
            "{:>6} {:<15} %usr {:5.1} %sys {:5.1} %cpu {:5.1}",
            u.pid, u.comm, u.usr, u.sys, u.cpu
        ));
    }

    let top_cpu = usage.first().map_or(0.0, |u| u.cpu);
    s.metric("processes", total as f64);
    s.metric("d_state", d_state.len() as f64);
    s.metric("zombies", zombies as f64);
    s.metric("top_cpu_pct", top_cpu);

    if let Some(top) = usage.first() {
        let limit = SATURATION_PCT * cpus;
        if top.cpu > limit {
            s.warn(format!(
                "{} (pid {}) uses {:.0}% CPU, more than {SATURATION_PCT:.0}% of {} cpus capacity",
                top.comm,
                top.pid,
                top.cpu,
                units::cpus(cpus)
            ));
        }
    }

    if !d_state.is_empty() {
        let n = d_state.len();
        if n as f64 > cpus {
            s.warn(format!(
                "{n} tasks in D state (uninterruptible, usually I/O) exceed {} cpus: {}",
                units::cpus(cpus),
                list(d_state)
            ));
        } else {
            s.note(format!(
                "{n} in D state (uninterruptible, usually I/O): {}",
                list(d_state)
            ));
        }
    }

    if zombies > ZOMBIE_WARN {
        s.warn(format!(
            "{zombies} zombie processes (more than {ZOMBIE_WARN}): a parent is not reaping its children"
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Level, Status};
    use crate::source::MemSource;
    use crate::sysinfo::SysInfo;

    fn ctx(cpus: usize) -> Context {
        Context {
            sys: SysInfo {
                cpus_online: cpus,
                ..Default::default()
            },
            interval: 1.0,
            count: 1,
        }
    }

    fn stat(pid: u32, comm: &str, state: char, utime: u64, stime: u64) -> String {
        format!("{pid} ({comm}) {state} 1 1 1 0 -1 0 0 0 0 0 {utime} {stime} 0 0 20 0 1 0 5000 0 0")
    }

    fn path(pid: u32) -> String {
        format!("/proc/{pid}/stat")
    }

    fn check() -> Processes {
        Processes {
            // Never collides with the fake pids used in these tests.
            own_pid: u32::MAX,
            hz: 100.0,
            ..Default::default()
        }
    }

    /// `procs`: (pid, comm, state, cpu% over a 1 s window, all user time).
    fn run(procs: &[(u32, &str, char, u64)], cpus: usize) -> Section {
        let src = MemSource::new().with("/proc/stat", "cpu 0");
        for &(pid, comm, state, _) in procs {
            src.set(&path(pid), &stat(pid, comm, state, 1000, 500));
        }
        let mut c = check();
        c.sample(&src, 0.0);
        for &(pid, comm, state, pct) in procs {
            // 100 ticks/s over 1 s: pct ticks == pct %.
            src.set(&path(pid), &stat(pid, comm, state, 1000 + pct, 500));
        }
        c.sample(&src, 1.0);
        c.evaluate(&ctx(cpus))
    }

    #[test]
    fn cpu_percentages_over_window() {
        let src = MemSource::new().with(&path(1234), &stat(1234, "java", 'S', 100, 50));
        let mut c = check();
        c.sample(&src, 0.0);
        src.set(&path(1234), &stat(1234, "java", 'R', 230, 62));
        c.sample(&src, 1.0);
        let s = c.evaluate(&ctx(8));
        assert_eq!(s.status, Status::Ok);
        assert_eq!(
            s.details,
            vec!["  1234 java            %usr 130.0 %sys  12.0 %cpu 142.0"]
        );
        assert_eq!(s.summary, "1 process, top: java 142% (pid 1234)");
        assert_eq!(s.metrics["top_cpu_pct"], 142.0);
    }

    #[test]
    fn top_n_ordering() {
        let s = run(
            &[
                (10, "a", 'S', 10),
                (11, "b", 'R', 70),
                (12, "c", 'S', 30),
                (13, "d", 'S', 0),
                (14, "e", 'S', 50),
                (15, "f", 'S', 20),
                (16, "g", 'S', 60),
            ],
            8,
        );
        assert_eq!(s.status, Status::Ok);
        let pids: Vec<&str> = s
            .details
            .iter()
            .map(|d| d.split_whitespace().next().unwrap())
            .collect();
        assert_eq!(pids, vec!["11", "16", "14", "12", "15"]);
        assert_eq!(s.summary, "7 processes, top: b 70% (pid 11)");
        assert_eq!(s.metrics["top_cpu_pct"], 70.0);
        assert_eq!(s.metrics["processes"], 7.0);
    }

    #[test]
    fn all_idle() {
        let s = run(&[(1, "init", 'S', 0), (2, "sleep", 'S', 0)], 4);
        assert_eq!(s.summary, "2 processes, all idle");
        assert!(s.details.is_empty());
        assert_eq!(s.metrics["top_cpu_pct"], 0.0);
        assert!(s.findings.is_empty());
    }

    #[test]
    fn comm_with_spaces_in_details() {
        let s = run(&[(42, "my (weird) proc", 'R', 25)], 4);
        assert_eq!(s.summary, "1 process, top: my (weird) proc 25% (pid 42)");
        assert!(
            s.details[0].starts_with("    42 my (weird) proc %usr  25.0"),
            "{}",
            s.details[0]
        );
    }

    #[test]
    fn process_disappearing_mid_window_is_ignored() {
        let src = MemSource::new()
            .with(&path(1), &stat(1, "init", 'S', 0, 0))
            .with(&path(2), &stat(2, "gone", 'R', 0, 0));
        let mut c = check();
        c.sample(&src, 0.0);
        src.set(&path(1), &stat(1, "init", 'S', 10, 0));
        src.remove(&path(2));
        c.sample(&src, 1.0);
        let s = c.evaluate(&ctx(4));
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.metrics["processes"], 1.0);
        assert_eq!(s.summary, "1 process, top: init 10% (pid 1)");
    }

    #[test]
    fn own_pid_is_excluded() {
        let src = MemSource::new()
            .with(&path(1), &stat(1, "init", 'S', 0, 0))
            .with(&path(7), &stat(7, "perf60", 'R', 0, 0));
        let mut c = Processes {
            own_pid: 7,
            ..check()
        };
        c.sample(&src, 0.0);
        src.set(&path(7), &stat(7, "perf60", 'R', 50, 0));
        c.sample(&src, 1.0);
        let s = c.evaluate(&ctx(4));
        assert_eq!(s.summary, "1 process, all idle");
    }

    #[test]
    fn pid_reuse_resets_window() {
        let src = MemSource::new().with(&path(5), &stat(5, "old", 'S', 100, 0));
        let mut c = check();
        c.sample(&src, 0.0);
        // Different starttime: a new process with lower counters must not produce a delta.
        src.set(
            &path(5),
            "5 (new) R 1 1 1 0 -1 0 0 0 0 0 300 0 0 0 20 0 1 0 9000 0 0",
        );
        c.sample(&src, 1.0);
        let s = c.evaluate(&ctx(4));
        assert_eq!(s.summary, "1 process, all idle");
    }

    #[test]
    fn single_process_over_90pct_capacity_warns() {
        let s = run(&[(1, "init", 'S', 0), (99, "burner", 'R', 185)], 2);
        assert_eq!(s.status, Status::Warn);
        let f = &s.findings[0];
        assert_eq!(f.level, Level::Warn);
        assert!(f.message.contains("burner (pid 99)"), "{}", f.message);
        assert!(f.message.contains("2 cpus"), "{}", f.message);
        // Exactly 90% of capacity is not above it.
        assert_eq!(run(&[(99, "burner", 'R', 180)], 2).status, Status::Ok);
    }

    #[test]
    fn d_state_note_and_warn() {
        let s = run(
            &[
                (1, "init", 'S', 0),
                (20, "kjournald", 'D', 0),
                (21, "dd", 'D', 3),
            ],
            4,
        );
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.metrics["d_state"], 2.0);
        assert!(s.summary.ends_with(", 2 in D state"), "{}", s.summary);
        let f = &s.findings[0];
        assert_eq!(f.level, Level::Note);
        assert!(f.message.contains("kjournald(20), dd(21)"), "{}", f.message);

        let d = |n: u32| -> Vec<(u32, &'static str, char, u64)> {
            (100..100 + n).map(|pid| (pid, "io", 'D', 0)).collect()
        };
        let s = run(&d(4), 4);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.findings[0].level, Level::Note);
        let s = run(&d(7), 4);
        assert_eq!(s.status, Status::Warn);
        let f = &s.findings[0];
        assert!(f.message.contains("7 tasks in D state"), "{}", f.message);
        // At most 5 are listed by name.
        assert!(f.message.contains("io(104), +2 more"), "{}", f.message);
        assert!(!f.message.contains("io(105)"), "{}", f.message);
    }

    #[test]
    fn zombies_over_50_warn() {
        let z = |n: u32| -> Vec<(u32, &'static str, char, u64)> {
            (1000..1000 + n)
                .map(|pid| (pid, "defunct", 'Z', 0))
                .collect()
        };
        let s = run(&z(50), 4);
        assert_eq!(s.status, Status::Ok);
        assert!(s.summary.contains("50 zombies"), "{}", s.summary);
        let s = run(&z(51), 4);
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.metrics["zombies"], 51.0);
    }

    #[test]
    fn missing_proc_is_skipped() {
        let mut c = check();
        c.sample(&MemSource::new(), 0.0);
        let s = c.evaluate(&ctx(4));
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains("/proc"), "{}", s.summary);
    }

    #[test]
    fn no_readable_process_is_skipped() {
        let src = MemSource::new().with("/proc/stat", "cpu 0");
        let mut c = check();
        c.sample(&src, 0.0);
        c.sample(&src, 1.0);
        assert_eq!(c.evaluate(&ctx(4)).status, Status::Skipped);
    }
}
