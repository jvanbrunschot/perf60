//! `uptime`: load averages against CPU capacity.

use crate::check::{Check, Context, Resource, SampleError, Section};
use crate::procfs::loadavg::{self, LoadAvg};
use crate::source::Source;
use crate::units;

const PATH: &str = "/proc/loadavg";

#[derive(Default)]
pub struct Load {
    last: Option<LoadAvg>,
    error: SampleError,
}

impl Check for Load {
    fn id(&self) -> &'static str {
        "load"
    }

    fn sample(&mut self, src: &dyn Source, _t: f64) {
        if let Some(s) = self.error.read(src, PATH) {
            match loadavg::parse(&s) {
                Ok(l) => self.last = Some(l),
                Err(e) => self.error.record(PATH, &std::io::Error::other(e.0)),
            }
        }
    }

    fn evaluate(&self, ctx: &Context) -> Section {
        let s = Section::new("load", "Load averages", "uptime", Resource::Cpu);
        let Some(l) = &self.last else {
            return s.skipped(self.error.get().unwrap_or("no samples"));
        };
        evaluate(s, l, ctx.cpus())
    }
}

fn evaluate(mut s: Section, l: &LoadAvg, cpus: f64) -> Section {
    s.summary(format!(
        "{:.2} {:.2} {:.2} (1/5/15m) on {} cpus, {}/{} tasks runnable",
        l.load1,
        l.load5,
        l.load15,
        units::cpus(cpus),
        l.running,
        l.total
    ));
    s.metric("load1", l.load1);
    s.metric("load5", l.load5);
    s.metric("load15", l.load15);
    s.metric("cpus", cpus);
    s.threshold(
        l.load1,
        cpus,
        2.0 * cpus,
        format!(
            "load1 {:.2} exceeds {} cpus: CPU saturation, or tasks blocked on I/O or locks (D state)",
            l.load1,
            units::cpus(cpus)
        ),
    );
    let busy = 0.5 * cpus;
    if l.load1 > 1.5 * l.load15 && l.load1 >= busy {
        s.note("load rising (1m well above 15m)");
    } else if l.load15 > 1.5 * l.load1 && l.load15 >= busy {
        s.note("load falling (15m well above 1m): the problem may have passed");
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

    fn run(loadavg: &str, cpus: usize) -> Section {
        let src = MemSource::new().with(PATH, loadavg);
        let mut c = Load::default();
        c.sample(&src, 0.0);
        c.evaluate(&ctx(cpus))
    }

    #[test]
    fn summary_ok() {
        let s = run("1.20 0.90 0.80 2/345 999\n", 8);
        assert_eq!(s.status, Status::Ok);
        assert!(s.summary.contains("1.20 0.90 0.80"), "{}", s.summary);
        assert!(s.summary.contains("8 cpus"));
        assert_eq!(s.metrics["load1"], 1.2);
        assert_eq!(s.metrics["cpus"], 8.0);
    }

    #[test]
    fn thresholds() {
        assert_eq!(run("8.00 8.00 8.00 1/1 1", 8).status, Status::Ok);
        assert_eq!(run("9.00 8.00 8.00 1/1 1", 8).status, Status::Warn);
        assert_eq!(run("16.50 16.0 16.0 1/1 1", 8).status, Status::Crit);
    }

    #[test]
    fn trend_notes_do_not_change_status() {
        let s = run("6.00 3.00 2.00 1/1 1", 8);
        assert_eq!(s.status, Status::Ok);
        assert!(
            s.findings
                .iter()
                .any(|f| f.level == Level::Note && f.message.contains("load rising"))
        );
        let s = run("1.00 3.00 6.00 1/1 1", 8);
        assert!(
            s.findings
                .iter()
                .any(|f| f.message.contains("load falling"))
        );
        // Idle machines don't get trend noise.
        assert!(run("0.30 0.10 0.05 1/1 1", 8).findings.is_empty());
    }

    #[test]
    fn missing_loadavg_is_skipped() {
        let mut c = Load::default();
        c.sample(&MemSource::new(), 0.0);
        let s = c.evaluate(&ctx(4));
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains("/proc/loadavg"));
    }
}
