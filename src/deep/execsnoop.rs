//! `execsnoop` (BCC): new processes during the window, by command. Catches the short-lived
//! processes that `pidstat` and `top` never see.

use crate::check::{Resource, Section};
#[cfg(feature = "deep")]
use crate::{
    check::{Check, Context},
    source::Source,
};

/// Execs per second above which the section warns.
pub const EXECS_WARN_PER_SEC: f64 = 100.0;
const TOP_N: usize = 5;

/// Build the section from the window's counts. `by_comm` is (command, execs).
pub fn evaluate(
    mut s: Section,
    mut by_comm: Vec<(String, u64)>,
    execs: u64,
    forks: u64,
    secs: f64,
) -> Section {
    let per_sec = |n: u64| if secs > 0.0 { n as f64 / secs } else { 0.0 };
    let (exec_rate, fork_rate) = (per_sec(execs), per_sec(forks));
    by_comm.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let top: Vec<String> = by_comm
        .iter()
        .take(TOP_N)
        .map(|(c, n)| format!("{c} {n}"))
        .collect();
    s.summary(if execs == 0 {
        format!("no new processes ({fork_rate:.0} forks/s)")
    } else {
        format!(
            "{execs} execs ({exec_rate:.0}/s), {fork_rate:.0} forks/s, top: {}",
            top.join(", ")
        )
    });
    for (c, n) in by_comm.iter().take(TOP_N) {
        s.detail(format!("{n:>7} execs  {c}"));
    }
    if by_comm.len() > TOP_N {
        s.detail(format!("{} more commands", by_comm.len() - TOP_N));
    }
    s.metric("execs", execs as f64);
    s.metric("execs_per_sec", exec_rate);
    s.metric("forks_per_sec", fork_rate);
    s.metric("commands", by_comm.len() as f64);
    if exec_rate > EXECS_WARN_PER_SEC {
        s.warn(format!(
            "{exec_rate:.0} execs/s: short-lived process churn (top {}): fork+exec overhead, check scripts, health checks or crash loops",
            by_comm.first().map_or("?", |(c, _)| c.as_str())
        ));
    }
    s
}

pub fn section() -> Section {
    Section::new(
        "execsnoop",
        "New processes (eBPF)",
        "execsnoop (BCC)",
        Resource::Cpu,
    )
}

#[cfg(feature = "deep")]
#[derive(Default)]
pub struct Execsnoop {
    probe: Option<Result<super::probe::Probe, String>>,
    window: super::Window,
}

#[cfg(feature = "deep")]
static OBJECT: &[u8] = aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/execsnoop"));

#[cfg(feature = "deep")]
impl Check for Execsnoop {
    fn id(&self) -> &'static str {
        "execsnoop"
    }

    fn sample(&mut self, _src: &dyn Source, t: f64) {
        if self.probe.is_none() {
            self.probe = Some(super::probe::Probe::attach(
                OBJECT,
                &[
                    ("execsnoop_exec", "sched_process_exec"),
                    ("execsnoop_fork", "sched_process_fork"),
                ],
            ));
        }
        self.window.tick(t);
    }

    fn evaluate(&self, _ctx: &Context) -> Section {
        let s = section();
        let probe = match &self.probe {
            Some(Ok(p)) => p,
            Some(Err(reason)) => return s.skipped(reason.clone()),
            None => return s.skipped("no samples"),
        };
        /// (execs by command, total execs, total forks).
        type Counts = (Vec<(String, u64)>, u64, u64);
        let read = || -> Result<Counts, String> {
            let by_comm = probe
                .hash_map::<perf60_common::CommKey, u64>("EXECS")?
                .into_iter()
                .map(|(k, v)| (super::comm_str(&k.comm), v))
                .collect();
            let totals = probe.per_cpu_sums("TOTALS", 2)?;
            Ok((by_comm, totals[0], totals[1]))
        };
        match read() {
            Ok((by_comm, execs, forks)) => evaluate(s, by_comm, execs, forks, self.window.secs()),
            Err(e) => s.skipped(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::Status;

    #[test]
    fn quiet_window() {
        let s = evaluate(section(), vec![], 0, 3, 2.0);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.summary, "no new processes (2 forks/s)");
    }

    #[test]
    fn top_commands_and_threshold() {
        let counts = vec![
            ("true".to_owned(), 150),
            ("sh".to_owned(), 30),
            ("date".to_owned(), 20),
        ];
        let s = evaluate(section(), counts.clone(), 200, 210, 2.0);
        assert_eq!(s.status, Status::Ok, "100/s is not above the threshold");
        assert!(
            s.summary
                .starts_with("200 execs (100/s), 105 forks/s, top: true 150, sh 30, date 20")
        );
        let s = evaluate(section(), counts, 201, 210, 2.0);
        assert_eq!(s.status, Status::Warn);
        assert!(s.findings[0].message.contains("(top true)"));
        assert_eq!(s.metrics["commands"], 3.0);
    }

    #[test]
    fn more_than_five_commands() {
        let counts = (0..8).map(|i| (format!("c{i}"), 10 - i)).collect();
        let s = evaluate(section(), counts, 52, 52, 1.0);
        assert_eq!(s.details.len(), 6);
        assert_eq!(s.details[5], "3 more commands");
    }
}
