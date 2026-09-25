//! The check abstraction: every triage step implements [`Check`] and produces a [`Section`].

use std::collections::BTreeMap;
use std::io;

use serde::Serialize;

use crate::source::{Source, describe_error};
use crate::sysinfo::SysInfo;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Status {
    Ok,
    Warn,
    Crit,
    Skipped,
}

impl Status {
    /// Severity used for aggregation. SKIPPED never escalates.
    pub fn severity(self) -> u8 {
        match self {
            Status::Ok | Status::Skipped => 0,
            Status::Warn => 1,
            Status::Crit => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Ok => "OK",
            Status::Warn => "WARN",
            Status::Crit => "CRIT",
            Status::Skipped => "SKIP",
        }
    }

    pub fn exit_code(self) -> i32 {
        self.severity() as i32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Note,
    Warn,
    Crit,
}

#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub level: Level,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Section {
    pub id: &'static str,
    pub title: &'static str,
    /// The command from the article this section replaces, e.g. `iostat -xz 1`.
    pub equivalent: &'static str,
    pub status: Status,
    pub summary: String,
    pub details: Vec<String>,
    pub findings: Vec<Finding>,
    pub metrics: BTreeMap<String, f64>,
}

impl Section {
    pub fn new(id: &'static str, title: &'static str, equivalent: &'static str) -> Self {
        Section {
            id,
            title,
            equivalent,
            status: Status::Ok,
            summary: String::new(),
            details: Vec::new(),
            findings: Vec::new(),
            metrics: BTreeMap::new(),
        }
    }

    pub fn skipped(mut self, reason: impl Into<String>) -> Self {
        self.status = Status::Skipped;
        self.summary = reason.into();
        self
    }

    pub fn summary(&mut self, s: impl Into<String>) {
        self.summary = s.into();
    }

    pub fn detail(&mut self, s: impl Into<String>) {
        self.details.push(s.into());
    }

    pub fn metric(&mut self, key: impl Into<String>, value: f64) {
        self.metrics.insert(key.into(), value);
    }

    pub fn note(&mut self, msg: impl Into<String>) {
        self.push(Level::Note, msg.into());
    }

    pub fn warn(&mut self, msg: impl Into<String>) {
        self.push(Level::Warn, msg.into());
    }

    pub fn crit(&mut self, msg: impl Into<String>) {
        self.push(Level::Crit, msg.into());
    }

    /// Record a finding at WARN or CRIT depending on which threshold `value` crosses (if any).
    /// Returns true when a finding was recorded.
    pub fn threshold(&mut self, value: f64, warn: f64, crit: f64, msg: impl Into<String>) -> bool {
        if value > crit {
            self.crit(msg);
            true
        } else if value > warn {
            self.warn(msg);
            true
        } else {
            false
        }
    }

    fn push(&mut self, level: Level, message: String) {
        let escalate = match level {
            Level::Note => Status::Ok,
            Level::Warn => Status::Warn,
            Level::Crit => Status::Crit,
        };
        if self.status != Status::Skipped && escalate.severity() > self.status.severity() {
            self.status = escalate;
        }
        self.findings.push(Finding { level, message });
    }
}

/// Shared read-only context for evaluation.
pub struct Context {
    pub sys: SysInfo,
    pub interval: f64,
    pub count: usize,
}

impl Context {
    /// CPU capacity to compare against: online CPUs, lowered to the cgroup quota.
    pub fn cpus(&self) -> f64 {
        self.sys.effective_cpus()
    }
}

pub trait Check {
    fn id(&self) -> &'static str;
    /// Called `count + 1` times. `t` is seconds since the first sample (monotonic).
    fn sample(&mut self, src: &dyn Source, t: f64);
    /// Called once after sampling.
    fn evaluate(&self, ctx: &Context) -> Section;
}

/// Remembers the first read error of a check so it can report SKIPPED.
#[derive(Default, Debug, Clone)]
pub struct SampleError(Option<String>);

impl SampleError {
    pub fn record(&mut self, path: &str, e: &io::Error) {
        if self.0.is_none() {
            self.0 = Some(describe_error(path, e));
        }
    }

    pub fn get(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// Read `path`, remembering the error on failure.
    pub fn read(&mut self, src: &dyn Source, path: &str) -> Option<String> {
        match src.read_to_string(path) {
            Ok(s) => Some(s),
            Err(e) => {
                self.record(path, &e);
                None
            }
        }
    }
}

/// Overall status: the most severe non-SKIPPED section status.
pub fn overall(sections: &[Section]) -> Status {
    sections
        .iter()
        .map(|s| s.status)
        .filter(|s| *s != Status::Skipped)
        .max_by_key(|s| s.severity())
        .unwrap_or(Status::Ok)
}

/// Per-second rate of a monotonically increasing counter between two `(t, value)` samples.
pub fn rate(prev: (f64, u64), cur: (f64, u64)) -> f64 {
    let dt = cur.0 - prev.0;
    if dt <= 0.0 {
        return 0.0;
    }
    cur.1.saturating_sub(prev.1) as f64 / dt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warn_finding_escalates_section() {
        let mut s = Section::new("x", "X", "x");
        s.note("fyi");
        assert_eq!(s.status, Status::Ok);
        s.warn("hmm");
        assert_eq!(s.status, Status::Warn);
        s.note("fyi");
        assert_eq!(s.status, Status::Warn);
        s.crit("bad");
        s.warn("hmm");
        assert_eq!(s.status, Status::Crit);
    }

    #[test]
    fn threshold_boundaries_are_exclusive() {
        let mut s = Section::new("x", "X", "x");
        assert!(!s.threshold(60.0, 60.0, 90.0, "at warn"));
        assert_eq!(s.status, Status::Ok);
        assert!(s.threshold(60.1, 60.0, 90.0, "above warn"));
        assert_eq!(s.status, Status::Warn);
        assert!(s.threshold(90.1, 60.0, 90.0, "above crit"));
        assert_eq!(s.status, Status::Crit);
    }

    #[test]
    fn overall_ignores_skipped() {
        let ok = Section::new("a", "A", "a");
        let skip = Section::new("b", "B", "b").skipped("gone");
        assert_eq!(overall(&[ok.clone(), skip.clone()]), Status::Ok);
        assert_eq!(overall(&[ok.clone(), skip.clone()]).exit_code(), 0);
        let mut warn = Section::new("c", "C", "c");
        warn.warn("w");
        assert_eq!(overall(&[ok, skip, warn]), Status::Warn);
        assert_eq!(Status::Warn.exit_code(), 1);
        assert_eq!(Status::Crit.exit_code(), 2);
        assert_eq!(overall(&[]), Status::Ok);
    }

    #[test]
    fn skipped_section_is_not_escalated() {
        let mut s = Section::new("a", "A", "a").skipped("no file");
        s.crit("x");
        assert_eq!(s.status, Status::Skipped);
    }

    #[test]
    fn rate_uses_elapsed_time() {
        assert_eq!(rate((0.0, 100), (2.0, 300)), 100.0);
        assert_eq!(rate((1.0, 100), (1.0, 300)), 0.0);
        // Counter reset/wrap is treated as zero rather than a huge value.
        assert_eq!(rate((0.0, 300), (1.0, 100)), 0.0);
    }
}
