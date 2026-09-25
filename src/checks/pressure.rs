//! `top / PSI`: Pressure Stall Information for CPU, memory and I/O, system-wide and for our own
//! cgroup v2. The window percentages come from the cumulative `total=` counters (µs).

use crate::check::{Check, Context, Resource, SampleError, Section, rate};
use crate::procfs::pressure::{self, Psi};
use crate::procfs::system;
use crate::source::Source;

const CPU_PATH: &str = "/proc/pressure/cpu";
const SKIP_REASON: &str = "PSI not available (kernel < 4.20, or booted with psi=0)";

/// Resource name and what its `some` pressure means.
const RESOURCES: [(&str, &str); 3] = [
    ("cpu", "runnable tasks waiting for CPU"),
    ("memory", "tasks stalled on memory reclaim/swap"),
    ("io", "tasks stalled on I/O"),
];

const SOME_WARN: f64 = 10.0;
const SOME_CRIT: f64 = 25.0;
const FULL_WARN: f64 = 5.0;

/// First and last sample of one pressure file.
#[derive(Default, Clone, Copy)]
struct Series {
    first: Option<(f64, Psi)>,
    last: Option<(f64, Psi)>,
}

/// Stall percentages over the sampling window, plus the last sample for the kernel averages.
struct Window {
    some: f64,
    full: Option<f64>,
    last: Psi,
}

impl Series {
    fn push(&mut self, t: f64, p: Psi) {
        if self.first.is_none() {
            self.first = Some((t, p));
        }
        self.last = Some((t, p));
    }

    fn window(&self) -> Option<Window> {
        let ((t0, a), (t1, b)) = (self.first?, self.last?);
        // µs of stall per second of wall time → percent: ÷ 1e6 × 100.
        let pct = |x: u64, y: u64| (rate((t0, x), (t1, y)) / 1e4).clamp(0.0, 100.0);
        Some(Window {
            some: pct(a.some.total, b.some.total),
            full: match (a.full, b.full) {
                (Some(x), Some(y)) => Some(pct(x.total, y.total)),
                _ => None,
            },
            last: b,
        })
    }
}

#[derive(Default)]
pub struct Pressure {
    system: [Series; 3],
    cgroup: [Series; 3],
    /// Own cgroup v2 path from `/proc/self/cgroup`, resolved on the first sample.
    cgroup_path: Option<Option<String>>,
    no_psi: bool,
    error: SampleError,
}

impl Check for Pressure {
    fn id(&self) -> &'static str {
        "pressure"
    }

    fn sample(&mut self, src: &dyn Source, t: f64) {
        if !src.exists(CPU_PATH) {
            self.no_psi = true;
            return;
        }
        for (i, (name, _)) in RESOURCES.iter().enumerate() {
            let path = format!("/proc/pressure/{name}");
            if let Some(s) = self.error.read(src, &path) {
                match pressure::parse(&s) {
                    Ok(p) => self.system[i].push(t, p),
                    Err(e) => self.error.record(&path, &std::io::Error::other(e.0)),
                }
            }
        }
        let cg = self.cgroup_path.get_or_insert_with(|| {
            src.read_to_string("/proc/self/cgroup")
                .ok()
                .and_then(|s| system::cgroup2_path(&s))
        });
        if let Some(path) = cg {
            let dir = format!("/sys/fs/cgroup{}", path.trim_end_matches('/'));
            for (i, (name, _)) in RESOURCES.iter().enumerate() {
                // Cgroup pressure is optional context: ignore missing files and EOPNOTSUPP
                // (cgroup.pressure = 0).
                if let Some(p) = src
                    .read_to_string(&format!("{dir}/{name}.pressure"))
                    .ok()
                    .and_then(|s| pressure::parse(&s).ok())
                {
                    self.cgroup[i].push(t, p);
                }
            }
        }
    }

    fn evaluate(&self, ctx: &Context) -> Section {
        let mut s = Section::new(
            "pressure",
            "Pressure stall (PSI)",
            "top / PSI",
            Resource::Pressure,
        );
        if self.system[0].last.is_none() {
            let reason = if self.no_psi {
                SKIP_REASON
            } else {
                self.error.get().unwrap_or("no samples")
            };
            return s.skipped(reason);
        }
        let mut summary = Vec::new();
        for (i, (name, why)) in RESOURCES.iter().enumerate() {
            let Some(w) = self.system[i].window() else {
                continue;
            };
            summary.push(format!("{name} {:.1}%", w.some));
            // System-wide cpu `full` is not meaningful before kernel 5.13: ignore it.
            let full = if i == 0 { None } else { w.full };
            report(&mut s, "", name, why, &w, full);
        }
        s.summary(format!("{} (some, sampled)", summary.join(" ")));

        let own = self.cgroup_path.as_ref().and_then(|p| p.as_deref());
        // At `/` on a host the root cgroup duplicates /proc/pressure; inside a container `/` is
        // the cgroup namespace root, i.e. the container's own cgroup.
        if let Some(path) = own.filter(|p| *p != "/" || ctx.sys.container)
            && self.cgroup.iter().any(|c| c.last.is_some())
        {
            let label = if path == "/" {
                "/ (container cgroup namespace root)"
            } else {
                path
            };
            s.detail(format!("own cgroup {label}:"));
            for (i, (name, why)) in RESOURCES.iter().enumerate() {
                if let Some(w) = self.cgroup[i].window() {
                    report(&mut s, "cgroup ", name, why, &w, w.full);
                }
            }
        }
        s
    }
}

/// Detail line, metrics and threshold findings for one resource. `full` is None when it should
/// not be shown (system cpu).
fn report(s: &mut Section, prefix: &str, name: &str, why: &str, w: &Window, full: Option<f64>) {
    let key = prefix.replace(' ', "_");
    let mut d = format!("{prefix}{name:<6} some {:5.1}%", w.some);
    if let Some(f) = full {
        d.push_str(&format!("  full {f:5.1}%"));
    }
    let l = &w.last;
    d.push_str(&format!(
        "  avg10/60/300 some {:.2}/{:.2}/{:.2}",
        l.some.avg10, l.some.avg60, l.some.avg300
    ));
    if let (Some(_), Some(lf)) = (full, l.full) {
        d.push_str(&format!(
            " full {:.2}/{:.2}/{:.2}",
            lf.avg10, lf.avg60, lf.avg300
        ));
    }
    s.detail(d);

    s.metric(format!("{key}{name}_some_pct"), w.some);
    s.threshold(
        w.some,
        SOME_WARN,
        SOME_CRIT,
        format!(
            "{prefix}{name} some pressure {:.1}% of the window: {why}",
            w.some
        ),
    );
    // cpu `full` never gets a threshold: undefined system-wide, and for a cgroup it overlaps
    // with `some` (throttling), which is already evaluated.
    if name == "cpu" {
        return;
    }
    if let Some(f) = full {
        s.metric(format!("{key}{name}_full_pct"), f);
        if f > FULL_WARN {
            s.warn(format!(
                "{prefix}{name} full pressure {f:.1}% of the window: all non-idle tasks stalled at once"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Level, Status};
    use crate::source::MemSource;
    use crate::sysinfo::SysInfo;

    const MEM: &str = "/proc/pressure/memory";
    const IO: &str = "/proc/pressure/io";

    fn ctx(container: bool) -> Context {
        Context {
            sys: SysInfo {
                cpus_online: 4,
                container,
                ..Default::default()
            },
            interval: 1.0,
            count: 1,
        }
    }

    fn psi(some: u64, full: u64) -> String {
        format!(
            "some avg10=1.00 avg60=0.50 avg300=0.25 total={some}\n\
             full avg10=0.20 avg60=0.10 avg300=0.05 total={full}\n"
        )
    }

    /// Two samples at t=0 and t=1. `files` are `(path, Δsome µs, Δfull µs)`; `extra` are static
    /// files such as `/proc/self/cgroup`.
    fn run(files: &[(&str, u64, u64)], extra: &[(&str, &str)], container: bool) -> Section {
        let src = MemSource::new();
        for (p, v) in extra {
            src.set(p, v);
        }
        for (p, _, _) in files {
            src.set(p, &psi(1_000, 500));
        }
        let mut c = Pressure::default();
        c.sample(&src, 0.0);
        for (p, ds, df) in files {
            src.set(p, &psi(1_000 + ds, 500 + df));
        }
        c.sample(&src, 1.0);
        c.evaluate(&ctx(container))
    }

    fn system(cpu: u64, mem: u64, io: u64) -> Section {
        run(
            &[(CPU_PATH, cpu, 0), (MEM, mem, 0), (IO, io, 0)],
            &[],
            false,
        )
    }

    fn has(s: &Section, level: Level, needle: &str) -> bool {
        s.findings
            .iter()
            .any(|f| f.level == level && f.message.contains(needle))
    }

    #[test]
    fn summary_and_metrics() {
        let s = system(32_000, 0, 125_000);
        assert_eq!(s.summary, "cpu 3.2% memory 0.0% io 12.5% (some, sampled)");
        assert_eq!(s.metrics["cpu_some_pct"], 3.2);
        assert_eq!(s.metrics["memory_some_pct"], 0.0);
        assert_eq!(s.metrics["io_some_pct"], 12.5);
        assert_eq!(s.metrics["memory_full_pct"], 0.0);
        assert_eq!(s.metrics["io_full_pct"], 0.0);
        assert_eq!(s.details.len(), 3);
        assert!(s.details[0].starts_with("cpu"), "{}", s.details[0]);
        assert!(s.details[0].contains("avg10/60/300 some 1.00/0.50/0.25"));
        assert!(s.details[1].contains("full   0.0%"), "{}", s.details[1]);
        assert!(s.details[1].contains("full 0.20/0.10/0.05"));
        // Only io crosses 10%.
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.findings.len(), 1);
        assert!(has(&s, Level::Warn, "io some"));
        assert_eq!(system(0, 0, 0).status, Status::Ok);
    }

    #[test]
    fn zero_window_is_zero() {
        let src = MemSource::new()
            .with(CPU_PATH, &psi(1_000, 0))
            .with(MEM, &psi(1_000, 0));
        let mut c = Pressure::default();
        c.sample(&src, 0.5);
        src.set(CPU_PATH, &psi(900_000, 0));
        c.sample(&src, 0.5);
        let s = c.evaluate(&ctx(false));
        assert_eq!(s.status, Status::Ok);
        assert!(s.metrics.values().all(|v| *v == 0.0), "{:?}", s.metrics);
    }

    #[test]
    fn partial_resources_reported() {
        let s = run(&[(CPU_PATH, 20_000, 0)], &[], false);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.summary, "cpu 2.0% (some, sampled)");
        assert_eq!(s.details.len(), 1);
        assert!(!s.metrics.contains_key("memory_some_pct"));
    }

    #[test]
    fn some_threshold_boundaries() {
        assert_eq!(system(100_000, 0, 0).status, Status::Ok);

        let s = system(0, 0, 101_000);
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "tasks stalled on I/O"));

        let s = system(0, 250_000, 0);
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "tasks stalled on memory reclaim/swap"));

        let s = system(251_000, 0, 0);
        assert_eq!(s.status, Status::Crit);
        assert!(has(&s, Level::Crit, "runnable tasks waiting for CPU"));
    }

    #[test]
    fn full_threshold() {
        let s = run(
            &[(CPU_PATH, 0, 0), (MEM, 80_000, 51_000), (IO, 0, 0)],
            &[],
            false,
        );
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.metrics["memory_full_pct"], 5.1);
        assert!(has(&s, Level::Warn, "all non-idle tasks stalled at once"));

        let s = run(
            &[(CPU_PATH, 0, 0), (MEM, 0, 0), (IO, 50_000, 50_000)],
            &[],
            false,
        );
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.metrics["io_full_pct"], 5.0);
    }

    #[test]
    fn cpu_full_ignored_at_system_level() {
        let s = run(&[(CPU_PATH, 1_000, 900_000)], &[], false);
        assert_eq!(s.status, Status::Ok);
        assert!(!s.metrics.contains_key("cpu_full_pct"));
        assert!(!s.details[0].contains("full"), "{}", s.details[0]);
    }

    #[test]
    fn cgroup_pressure_reported() {
        let s = run(
            &[
                (CPU_PATH, 0, 0),
                ("/sys/fs/cgroup/app/cpu.pressure", 400_000, 300_000),
                ("/sys/fs/cgroup/app/memory.pressure", 0, 0),
            ],
            &[("/proc/self/cgroup", "0::/app\n")],
            false,
        );
        assert_eq!(s.metrics["cgroup_cpu_some_pct"], 40.0);
        assert_eq!(s.metrics["cgroup_memory_some_pct"], 0.0);
        assert_eq!(s.metrics["cpu_some_pct"], 0.0);
        assert_eq!(s.status, Status::Crit);
        assert!(
            s.findings
                .iter()
                .all(|f| f.message.starts_with("cgroup cpu"))
        );
        assert!(s.details.iter().any(|d| d == "own cgroup /app:"));
        // Cgroup cpu full is shown as context but never evaluated.
        let cpu = s
            .details
            .iter()
            .find(|d| d.starts_with("cgroup cpu"))
            .unwrap();
        assert!(cpu.contains("full  30.0%"), "{cpu}");
        assert!(!s.metrics.contains_key("cgroup_cpu_full_pct"));
    }

    #[test]
    fn host_root_cgroup_not_reported() {
        let s = run(
            &[
                (CPU_PATH, 0, 0),
                ("/sys/fs/cgroup/cpu.pressure", 400_000, 0),
            ],
            &[("/proc/self/cgroup", "0::/\n")],
            false,
        );
        assert_eq!(s.status, Status::Ok);
        assert!(!s.details.iter().any(|d| d.starts_with("cgroup")));
        assert!(!s.metrics.contains_key("cgroup_cpu_some_pct"));
    }

    #[test]
    fn container_namespace_root_reported() {
        let s = run(
            &[
                (CPU_PATH, 0, 0),
                ("/sys/fs/cgroup/cpu.pressure", 150_000, 0),
            ],
            &[("/proc/self/cgroup", "0::/\n")],
            true,
        );
        assert_eq!(s.metrics["cgroup_cpu_some_pct"], 15.0);
        assert!(s.details.iter().any(|d| d.contains("namespace root")));
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "cgroup cpu some"));
    }

    #[test]
    fn missing_psi_is_skipped() {
        let mut c = Pressure::default();
        c.sample(&MemSource::new(), 0.0);
        c.sample(&MemSource::new(), 1.0);
        let s = c.evaluate(&ctx(false));
        assert_eq!(s.status, Status::Skipped);
        assert_eq!(s.summary, SKIP_REASON);

        // Present but malformed: skipped with the parse error.
        let src = MemSource::new().with(CPU_PATH, "garbage\n");
        let mut c = Pressure::default();
        c.sample(&src, 0.0);
        let s = c.evaluate(&ctx(false));
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains(CPU_PATH), "{}", s.summary);
    }
}
