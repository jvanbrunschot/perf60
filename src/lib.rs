//! perf60: Brendan Gregg's "Linux Performance Analysis in 60,000 Milliseconds" as one binary.

pub mod check;
pub mod checks;
pub mod cli;
pub mod deep;
pub mod procfs;
pub mod report;
pub mod sample;
pub mod source;
pub mod sysinfo;
pub mod units;

use check::Context;
use report::{Report, Sampling};
use source::Source;

/// Run every registered check against `src` and build the report.
pub fn analyze(src: &dyn Source, interval: f64, count: usize) -> Report {
    analyze_with(src, interval, count, false)
}

/// Like [`analyze`], optionally adding the `--deep` eBPF checks (live systems only).
pub fn analyze_with(src: &dyn Source, interval: f64, count: usize, deep: bool) -> Report {
    let sys = sysinfo::collect(src);
    let mut checks = checks::all();
    if deep {
        checks.extend(deep::checks());
    }
    sample::run(&mut checks, src, interval, count);
    let ctx = Context {
        sys,
        interval,
        count,
    };
    let sections = checks.iter().map(|c| c.evaluate(&ctx)).collect();
    Report::new(ctx.sys, Sampling { interval, count }, sections)
}
