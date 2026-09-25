//! `runqlat` (BCC): run queue latency, how long runnable tasks wait for a CPU. A long tail is
//! direct evidence of CPU saturation, even when utilization averages look fine.

use super::hist::{self, Hist};
use crate::check::{Resource, Section};
#[cfg(feature = "deep")]
use crate::{
    check::{Check, Context},
    source::Source,
};

/// p99 (bucket upper bound, µs) above which the section warns.
pub const P99_WARN_US: f64 = 10_000.0;
/// p99 (bucket upper bound, µs) above which the section is critical.
pub const P99_CRIT_US: f64 = 50_000.0;
const HIST_WIDTH: usize = 30;

/// Build the section from the window's run queue latency histogram (µs).
pub fn evaluate(mut s: Section, h: &Hist, _secs: f64) -> Section {
    s.summary(hist::summary_us(h, "wakeups"));
    for line in h.lines_us(HIST_WIDTH) {
        s.detail(line);
    }
    s.metric("runq_events", h.count() as f64);
    let (Some(p50), Some(p99), Some(max)) = (h.percentile(50.0), h.percentile(99.0), h.max())
    else {
        return s;
    };
    s.metric("runq_p50_us", p50 as f64);
    s.metric("runq_p99_us", p99 as f64);
    s.metric("runq_max_us", max as f64);
    s.threshold(
        p99 as f64,
        P99_WARN_US,
        P99_CRIT_US,
        format!(
            "tasks wait up to {} for a CPU (p99): CPU saturation or a runaway high-priority task",
            hist::us(p99)
        ),
    );
    s
}

pub fn section() -> Section {
    Section::new(
        "runqlat",
        "Run queue latency (eBPF)",
        "runqlat (BCC)",
        Resource::Cpu,
    )
}

/// `(pid, state)` offsets in `task_struct` via `lookup` (e.g. [`super::btf::kernel_offsets`]):
/// `__state` (kernel 5.14+), falling back to `state` on older kernels.
pub fn task_offsets<F>(lookup: F) -> Result<(u32, u32), String>
where
    F: Fn(&[(&str, &str)]) -> Result<Vec<u32>, String>,
{
    let pair = |state: &str| {
        lookup(&[("task_struct", "pid"), ("task_struct", state)]).and_then(|o| match o[..] {
            [pid, state] => Ok((pid, state)),
            _ => Err("kernel BTF lookup returned the wrong number of offsets".to_owned()),
        })
    };
    pair("__state").or_else(|_| pair("state"))
}

#[cfg(feature = "deep")]
#[derive(Default)]
pub struct Runqlat {
    probe: Option<Result<super::probe::Probe, String>>,
    window: super::Window,
}

#[cfg(feature = "deep")]
static OBJECT: &[u8] = aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/runqlat"));

#[cfg(feature = "deep")]
fn attach() -> Result<super::probe::Probe, String> {
    // Privileges first, so an unprivileged run gets the needs-root reason.
    super::caps::require_bpf()?;
    let (pid, state) = task_offsets(super::btf::kernel_offsets)?;
    super::probe::Probe::attach_with(
        OBJECT,
        &[
            ("runqlat_wakeup", "sched_wakeup"),
            ("runqlat_wakeup_new", "sched_wakeup_new"),
            ("runqlat_switch", "sched_switch"),
        ],
        &[("PID_OFF", pid.into()), ("STATE_OFF", state.into())],
    )
}

#[cfg(feature = "deep")]
impl Check for Runqlat {
    fn id(&self) -> &'static str {
        "runqlat"
    }

    fn sample(&mut self, _src: &dyn Source, t: f64) {
        if self.probe.is_none() {
            self.probe = Some(attach());
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
        match probe.per_cpu_sums("HIST", hist::BUCKETS as u32) {
            Ok(counts) => evaluate(s, &Hist::from_counts(&counts), self.window.secs()),
            Err(e) => s.skipped(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::Status;

    /// All `n` waits in the bucket with upper bound `2 * lo`.
    fn at(lo: u64, n: u64) -> Hist {
        let mut h = Hist::default();
        h.add(lo.trailing_zeros() as usize, n);
        h
    }

    #[test]
    fn distribution_summary() {
        let mut h = Hist::default();
        h.add(3, 90); // 8..15 µs
        h.add(10, 9); // 1024..2047 µs
        h.add(13, 1); // 8192..16383 µs
        let s = evaluate(section(), &h, 2.0);
        assert_eq!(s.summary, "p50 16µs p99 2ms max 16.4ms (100 wakeups)");
        assert_eq!(s.status, Status::Ok, "p99 2048µs is below 10ms");
        assert_eq!(s.details.len(), 11);
        assert!(s.details[0].contains("8µs -> 15µs"), "{}", s.details[0]);
        assert_eq!(s.metrics["runq_p50_us"], 16.0);
        assert_eq!(s.metrics["runq_p99_us"], 2048.0);
        assert_eq!(s.metrics["runq_max_us"], 16384.0);
        assert_eq!(s.metrics["runq_events"], 100.0);
    }

    #[test]
    fn no_wakeups() {
        let s = evaluate(section(), &Hist::default(), 2.0);
        assert_eq!(s.summary, "no wakeups");
        assert_eq!(s.status, Status::Ok);
        assert!(s.details.is_empty() && s.findings.is_empty());
        assert_eq!(s.metrics["runq_events"], 0.0);
        assert!(!s.metrics.contains_key("runq_p99_us"));
    }

    #[test]
    fn thresholds() {
        // 4096..8191 µs: upper bound 8192 is not above 10 000.
        assert_eq!(evaluate(section(), &at(4096, 10), 1.0).status, Status::Ok);
        // 8192..16383 µs: upper bound 16384 > 10 000.
        let s = evaluate(section(), &at(8192, 10), 1.0);
        assert_eq!(s.status, Status::Warn);
        assert!(
            s.findings[0]
                .message
                .starts_with("tasks wait up to 16.4ms for a CPU (p99)"),
            "{}",
            s.findings[0].message
        );
        // 16384..32767 µs: upper bound 32768 is not above 50 000.
        assert_eq!(
            evaluate(section(), &at(16384, 10), 1.0).status,
            Status::Warn
        );
        // 32768..65535 µs: upper bound 65536 > 50 000.
        assert_eq!(
            evaluate(section(), &at(32768, 10), 1.0).status,
            Status::Crit
        );
        // The p99, not the max, decides: 1 slow wait in 100 is the p100.
        let mut h = at(8, 99);
        h.add(20, 1);
        assert_eq!(evaluate(section(), &h, 1.0).status, Status::Ok);
    }

    #[test]
    fn state_offset_fallback() {
        let new_kernel = |w: &[(&str, &str)]| Ok(w.iter().map(|(_, m)| m.len() as u32).collect());
        assert_eq!(task_offsets(new_kernel), Ok((3, 7)), "uses __state");
        let old_kernel = |w: &[(&str, &str)]| match w[1].1 {
            "__state" => Err("kernel BTF has no task_struct.__state".to_owned()),
            _ => Ok(vec![2440, 24]),
        };
        assert_eq!(task_offsets(old_kernel), Ok((2440, 24)));
        let no_btf = |_: &[(&str, &str)]| Err("kernel BTF not available".to_owned());
        assert_eq!(task_offsets(no_btf), Err("kernel BTF not available".into()));
    }
}
