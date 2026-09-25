//! `vmstat 1` and `mpstat -P ALL 1`: where CPU time goes, run-queue saturation and per-CPU
//! balance, all from `/proc/stat` deltas over the sampling window.

use crate::check::{Check, Context, Resource, SampleError, Section, rate};
use crate::procfs::schedstat::{self, SchedStat};
use crate::procfs::stat::{self, CpuTimes, Stat};
use crate::source::Source;
use crate::units;

const PATH: &str = "/proc/stat";
const SCHEDSTAT: &str = "/proc/schedstat";
/// Average run-queue wait per timeslice above these many milliseconds is WARN / CRIT.
const RUNQ_WARN_MS: f64 = 2.0;
const RUNQ_CRIT_MS: f64 = 10.0;

/// Timestamped `/proc/stat` snapshots. Each check keeps its own (see the core design).
#[derive(Default)]
struct Samples {
    v: Vec<(f64, Stat)>,
    error: SampleError,
}

impl Samples {
    fn sample(&mut self, src: &dyn Source, t: f64) {
        if let Some(s) = self.error.read(src, PATH) {
            match stat::parse(&s) {
                Ok(st) => self.v.push((t, st)),
                Err(e) => self.error.record(PATH, &std::io::Error::other(e.0)),
            }
        }
    }

    /// All snapshots, or the SKIPPED reason when there are fewer than two.
    fn window(&self) -> Result<&[(f64, Stat)], String> {
        if self.v.len() >= 2 {
            return Ok(&self.v);
        }
        Err(self.error.get().unwrap_or("not enough samples").to_owned())
    }
}

/// Percentages of one CPU time delta. With no elapsed ticks the CPU counts as idle.
#[derive(Debug, Clone, Copy)]
struct Split {
    /// user + nice (guest time is already inside these).
    us: f64,
    /// system + irq + softirq.
    sy: f64,
    id: f64,
    wa: f64,
    st: f64,
    /// system only.
    sys: f64,
    /// irq + softirq.
    irq: f64,
}

impl Split {
    fn of(d: &CpuTimes) -> Split {
        let total = d.total();
        if total == 0 {
            return Split {
                us: 0.0,
                sy: 0.0,
                id: 100.0,
                wa: 0.0,
                st: 0.0,
                sys: 0.0,
                irq: 0.0,
            };
        }
        let pct = |n: u64| n as f64 * 100.0 / total as f64;
        Split {
            us: pct(d.user + d.nice),
            sy: pct(d.system + d.irq + d.softirq),
            id: pct(d.idle),
            wa: pct(d.iowait),
            st: pct(d.steal),
            sys: pct(d.system),
            irq: pct(d.irq + d.softirq),
        }
    }

    /// Non-idle time: everything except idle and iowait.
    fn busy(&self) -> f64 {
        (100.0 - self.id - self.wa).max(0.0)
    }
}

/// Compact count: `950` → `950`, `1500` → `1.5k`, `12000` → `12k`, `2500000` → `2.5M`.
fn count(v: f64) -> String {
    let v = v.max(0.0);
    let short = |x: f64, unit: &str| {
        if x < 10.0 {
            let s = format!("{x:.1}");
            format!("{}{unit}", s.strip_suffix(".0").unwrap_or(&s))
        } else {
            format!("{x:.0}{unit}")
        }
    };
    if v < 1000.0 {
        format!("{v:.0}")
    } else if v < 1_000_000.0 {
        short(v / 1000.0, "k")
    } else {
        short(v / 1_000_000.0, "M")
    }
}

// ---------------------------------------------------------------------------------------------
// vmstat 1

#[derive(Default)]
pub struct Cpu {
    samples: Samples,
    /// First and latest readable `/proc/schedstat`. Optional: without it there is no
    /// run-queue latency, but the section still renders.
    sched: Option<(SchedStat, SchedStat)>,
}

impl Check for Cpu {
    fn id(&self) -> &'static str {
        "cpu"
    }

    fn sample(&mut self, src: &dyn Source, t: f64) {
        self.samples.sample(src, t);
        if let Some(st) = src
            .read_to_string(SCHEDSTAT)
            .ok()
            .and_then(|s| schedstat::parse(&s).ok())
        {
            match &mut self.sched {
                Some((_, last)) => *last = st,
                None => self.sched = Some((st.clone(), st)),
            }
        }
    }

    fn evaluate(&self, ctx: &Context) -> Section {
        let s = Section::new("cpu", "CPU utilization", "vmstat 1", Resource::Cpu);
        match self.samples.window() {
            Ok(w) => {
                let mut s = evaluate_cpu(s, w, ctx.cpus(), ctx.sys.cpus_online.max(1) as f64);
                if let Some((first, last)) = &self.sched {
                    runq_latency(&mut s, first, last);
                }
                s
            }
            Err(reason) => s.skipped(reason),
        }
    }
}

fn evaluate_cpu(mut s: Section, w: &[(f64, Stat)], cpus: f64, online: f64) -> Section {
    let (first, last) = (&w[0], &w[w.len() - 1]);
    let p = Split::of(&last.1.total.since(&first.1.total));
    let n = w.len() as f64;
    let avg = |f: fn(&Stat) -> Option<u64>| {
        w.iter().map(|(_, st)| f(st).unwrap_or(0)).sum::<u64>() as f64 / n
    };
    let r = (avg(|st| st.procs_running) - 1.0).max(0.0);
    let b = avg(|st| st.procs_blocked);
    let counter = |f: fn(&Stat) -> Option<u64>| {
        rate(
            (first.0, f(&first.1).unwrap_or(0)),
            (last.0, f(&last.1).unwrap_or(0)),
        )
    };
    let cs = counter(|st| st.ctxt);
    let intr = counter(|st| st.intr);
    let busy_peak = w
        .windows(2)
        .map(|pair| Split::of(&pair[1].1.total.since(&pair[0].1.total)).busy())
        .fold(0.0, f64::max);

    s.summary(format!(
        "us {:.0}% sy {:.0}% id {:.0}% wa {:.0}% st {:.0}%  r={r:.1} b={b:.1}  cs {}/s",
        p.us,
        p.sy,
        p.id,
        p.wa,
        p.st,
        count(cs)
    ));
    s.detail(format!(
        "peak interval busy {busy_peak:.0}%, interrupts {}/s",
        count(intr)
    ));
    for (k, v) in [
        ("us", p.us),
        ("sy", p.sy),
        ("id", p.id),
        ("wa", p.wa),
        ("st", p.st),
        ("r", r),
        ("b", b),
        ("cs_per_sec", cs),
        ("intr_per_sec", intr),
        ("busy_peak", busy_peak),
    ] {
        s.metric(k, v);
    }

    // procs_running is an instantaneous count: a long queue on mostly idle CPUs is sampling
    // noise, not saturation. Judge it only when at least half the capacity is in use.
    let in_use = (p.us + p.sy + p.st) / 100.0 * online;
    if in_use >= 0.5 * cpus {
        s.threshold(
            r,
            cpus,
            2.0 * cpus,
            format!(
                "run queue r={r:.1} exceeds {} cpus: CPU saturation, runnable tasks wait for a CPU",
                units::cpus(cpus)
            ),
        );
    }
    s.threshold(
        p.wa,
        20.0,
        50.0,
        format!(
            "iowait {:.0}%: I/O bound, CPUs sit idle waiting on I/O",
            p.wa
        ),
    );
    s.threshold(
        p.st,
        10.0,
        25.0,
        format!(
            "steal {:.0}%: hypervisor steal, a noisy neighbour or an overcommitted host",
            p.st
        ),
    );
    let busy = p.us + p.sy;
    if busy > 90.0 {
        s.warn(format!("CPU {busy:.0}% busy (us+sy): CPUs near saturation"));
    }
    if p.sy > 20.0 {
        s.note(format!(
            "high system time, worth investigating (sy {:.0}%)",
            p.sy
        ));
    }
    if b > 0.0 {
        s.note(format!(
            "b={b:.1}: tasks blocked on I/O (uninterruptible sleep)"
        ));
    }
    s
}

/// Run-queue wait per timeslice in ms: `(all CPUs, worst CPU, worst CPU's average)`. `None`
/// when no CPU ran a timeslice during the window.
fn runq_wait(first: &SchedStat, last: &SchedStat) -> Option<(f64, usize, f64)> {
    let (mut delay, mut slices) = (0u64, 0u64);
    let mut worst: Option<(usize, f64)> = None;
    for (n, b) in &last.cpus {
        let Some((_, a)) = first.cpus.iter().find(|(m, _)| m == n) else {
            continue;
        };
        let d = b.run_delay_ns.saturating_sub(a.run_delay_ns);
        let k = b.timeslices.saturating_sub(a.timeslices);
        if k == 0 {
            continue;
        }
        delay += d;
        slices += k;
        let ms = d as f64 / k as f64 / 1e6;
        if worst.is_none_or(|(_, w)| ms > w) {
            worst = Some((*n, ms));
        }
    }
    let (cpu, max) = worst?;
    Some((delay as f64 / slices as f64 / 1e6, cpu, max))
}

/// runqlat-lite: how long runnable tasks wait for a CPU, from `/proc/schedstat` deltas.
fn runq_latency(s: &mut Section, first: &SchedStat, last: &SchedStat) {
    let Some((avg, cpu, max)) = runq_wait(first, last) else {
        return;
    };
    s.detail(format!(
        "run queue wait {avg:.2} ms per timeslice, worst cpu{cpu} {max:.2} ms"
    ));
    s.metric("runq_wait_ms", avg);
    s.metric("runq_wait_max_cpu_ms", max);
    s.threshold(
        avg,
        RUNQ_WARN_MS,
        RUNQ_CRIT_MS,
        format!("tasks wait {avg:.2} ms for a CPU on average: CPU saturation / run queue latency"),
    );
}

// ---------------------------------------------------------------------------------------------
// mpstat -P ALL 1

#[derive(Default)]
pub struct CpuBalance {
    samples: Samples,
}

impl Check for CpuBalance {
    fn id(&self) -> &'static str {
        "cpu-balance"
    }

    fn sample(&mut self, src: &dyn Source, t: f64) {
        self.samples.sample(src, t);
    }

    fn evaluate(&self, _ctx: &Context) -> Section {
        let s = Section::new(
            "cpu-balance",
            "Per-CPU balance",
            "mpstat -P ALL 1",
            Resource::Cpu,
        );
        match self.samples.window() {
            Ok(w) => evaluate_balance(s, &w[0].1, &w[w.len() - 1].1),
            Err(reason) => s.skipped(reason),
        }
    }
}

fn evaluate_balance(mut s: Section, first: &Stat, last: &Stat) -> Section {
    // CPUs present at both ends of the window (hotplug can add or remove some).
    let mut per_cpu: Vec<(usize, Split)> = last
        .cpus
        .iter()
        .filter_map(|(n, end)| {
            let (_, start) = first.cpus.iter().find(|(m, _)| m == n)?;
            Some((*n, Split::of(&end.since(start))))
        })
        .collect();
    if per_cpu.is_empty() {
        return s.skipped(format!("no per-CPU lines in {PATH}"));
    }
    // Busiest first; the stable sort keeps the lower index first on ties.
    per_cpu.sort_by(|a, b| b.1.busy().total_cmp(&a.1.busy()));
    let n = per_cpu.len();
    let mean = per_cpu.iter().map(|(_, p)| p.busy()).sum::<f64>() / n as f64;
    let (hot, hp) = per_cpu[0];
    let max = hp.busy();

    if n == 1 {
        s.summary(format!("1 cpu, {max:.0}% busy"));
    } else {
        s.summary(format!("{n} cpus, avg {mean:.0}%, max cpu{hot} {max:.0}%"));
    }
    for (i, p) in per_cpu.iter().take(4) {
        s.detail(format!(
            "cpu{i} {:.0}% busy: usr {:.0}% sys {:.0}% irq+soft {:.0}% iowait {:.0}%",
            p.busy(),
            p.us,
            p.sys,
            p.irq,
            p.wa
        ));
    }
    s.metric("avg_busy", mean);
    s.metric("max_busy", max);
    s.metric("max_cpu", hot as f64);

    if n >= 2 && max > 90.0 && mean < 50.0 {
        if hp.irq > 0.5 * max {
            s.warn(format!(
                "cpu{hot} {max:.0}% busy, mostly irq+softirq, while the mean is {mean:.0}%: \
                 interrupt imbalance (check IRQ affinity / RPS)"
            ));
        } else {
            s.warn(format!(
                "cpu{hot} {max:.0}% busy while the mean is {mean:.0}%: \
                 single-threaded bottleneck or IRQ imbalance"
            ));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Level, Status};
    use crate::source::MemSource;
    use crate::sysinfo::SysInfo;

    /// Starting value of every counter, so deltas don't start from zero.
    const BASE: u64 = 1000;

    /// Ticks per CPU in `/proc/stat` column order: user nice system idle iowait irq softirq steal.
    type Ticks = [u64; 8];

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

    /// Build `/proc/stat` from per-CPU tick counters; the aggregate line is their sum.
    fn stat_text(cpus: &[Ticks], ctxt: u64, intr: u64, running: u64, blocked: u64) -> String {
        let line = |name: &str, t: &Ticks| {
            let cols: Vec<String> = t.iter().map(u64::to_string).collect();
            format!("{name} {} 0 0\n", cols.join(" "))
        };
        let mut total = [0u64; 8];
        for c in cpus {
            for (sum, v) in total.iter_mut().zip(c) {
                *sum += v;
            }
        }
        let mut s = line("cpu ", &total);
        for (i, c) in cpus.iter().enumerate() {
            s += &line(&format!("cpu{i}"), c);
        }
        s += &format!(
            "intr {intr} 0 1 2\nctxt {ctxt}\nbtime 1\nprocesses 100\n\
             procs_running {running}\nprocs_blocked {blocked}\n"
        );
        s
    }

    fn base(n: usize) -> Vec<Ticks> {
        vec![[BASE; 8]; n]
    }

    fn plus(a: &[Ticks], d: &[Ticks]) -> Vec<Ticks> {
        a.iter()
            .zip(d)
            .map(|(a, d)| std::array::from_fn(|i| a[i] + d[i]))
            .collect()
    }

    /// Feed each text as one sample, one second apart.
    fn feed<C: Check>(mut c: C, texts: &[String]) -> C {
        let src = MemSource::new();
        for (i, t) in texts.iter().enumerate() {
            src.set(PATH, t);
            c.sample(&src, i as f64);
        }
        c
    }

    /// One 1-second window where each CPU accrues `deltas` ticks.
    fn window(deltas: &[Ticks]) -> [String; 2] {
        let a = base(deltas.len());
        [
            stat_text(&a, 0, 0, 1, 0),
            stat_text(&plus(&a, deltas), 0, 0, 1, 0),
        ]
    }

    fn cpu(texts: &[String], cpus: usize) -> Section {
        feed(Cpu::default(), texts).evaluate(&ctx(cpus))
    }

    fn balance(deltas: &[Ticks]) -> Section {
        feed(CpuBalance::default(), &window(deltas)).evaluate(&ctx(deltas.len()))
    }

    /// One CPU, 100 ticks, split as given (user, system, idle, iowait, steal).
    fn split(user: u64, system: u64, iowait: u64, steal: u64) -> Section {
        let idle = 100 - user - system - iowait - steal;
        cpu(&window(&[[user, 0, system, idle, iowait, 0, 0, steal]]), 1)
    }

    fn has(s: &Section, level: Level, text: &str) -> bool {
        s.findings
            .iter()
            .any(|f| f.level == level && f.message.contains(text))
    }

    /// A CPU `busy`% busy in user time.
    fn user(busy: u64) -> Ticks {
        [busy, 0, 0, 100 - busy, 0, 0, 0, 0]
    }

    // --- cpu ---------------------------------------------------------------------------------

    #[test]
    fn cpu_summary() {
        let a = base(1);
        let b = plus(&a, &[[50, 10, 5, 30, 2, 2, 1, 0]]);
        let s = cpu(
            &[
                stat_text(&a, 1_000, 500, 3, 0),
                stat_text(&b, 13_000, 3_700, 4, 0),
            ],
            4,
        );
        assert_eq!(s.status, Status::Ok, "{:?}", s.findings);
        assert!(
            s.summary.contains("us 60% sy 8% id 30% wa 2% st 0%"),
            "{}",
            s.summary
        );
        assert!(s.summary.contains("r=2.5"), "{}", s.summary);
        assert!(s.summary.contains("cs 12k/s"), "{}", s.summary);
        assert_eq!(s.metrics["cs_per_sec"], 12_000.0);
        assert_eq!(s.metrics["intr_per_sec"], 3_200.0);
        assert_eq!(s.metrics["busy_peak"], 68.0);
        assert!(
            s.details[0].contains("interrupts 3.2k/s"),
            "{:?}",
            s.details
        );
        for k in [
            "us",
            "sy",
            "id",
            "wa",
            "st",
            "r",
            "b",
            "cs_per_sec",
            "intr_per_sec",
            "busy_peak",
        ] {
            assert!(s.metrics.contains_key(k), "missing metric {k}");
        }
    }

    #[test]
    fn cpu_guest_not_double_counted() {
        let s = cpu(
            &[
                "cpu 0 0 0 0 0 0 0 0 0 0\ncpu0 0 0 0 0 0 0 0 0 0 0\n".to_owned(),
                "cpu 50 0 0 50 0 0 0 0 40 0\ncpu0 50 0 0 50 0 0 0 0 40 0\n".to_owned(),
            ],
            1,
        );
        assert_eq!(s.metrics["us"], 50.0);
        assert_eq!(s.metrics["id"], 50.0);
        let sum: f64 = ["us", "sy", "id", "wa", "st"]
            .iter()
            .map(|k| s.metrics[*k])
            .sum();
        assert_eq!(sum, 100.0);
    }

    #[test]
    fn cpu_peak_interval_busy() {
        let a = base(1);
        let b = plus(&a, &[user(20)]);
        let c = plus(&b, &[user(80)]);
        let s = cpu(
            &[
                stat_text(&a, 0, 0, 1, 0),
                stat_text(&b, 0, 0, 1, 0),
                stat_text(&c, 0, 0, 1, 0),
            ],
            1,
        );
        assert_eq!(s.metrics["busy_peak"], 80.0);
        assert_eq!(s.metrics["us"], 50.0);
        assert!(
            s.details[0].contains("peak interval busy 80%"),
            "{:?}",
            s.details
        );
    }

    #[test]
    fn cpu_run_queue_excludes_self() {
        let s = split(10, 0, 0, 0);
        assert_eq!(s.metrics["r"], 0.0);
        assert!(s.summary.contains("r=0.0"), "{}", s.summary);
        // procs_running missing or 0 is floored at 0, not negative.
        let s = cpu(&["cpu 1 0 0 1\n".to_owned(), "cpu 2 0 0 2\n".to_owned()], 1);
        assert_eq!(s.metrics["r"], 0.0);
    }

    #[test]
    fn cpu_identical_snapshots() {
        let a = stat_text(&base(2), 500, 500, 1, 0);
        let s = cpu(&[a.clone(), a], 2);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.metrics["id"], 100.0);
        for k in [
            "us",
            "sy",
            "wa",
            "st",
            "cs_per_sec",
            "intr_per_sec",
            "busy_peak",
        ] {
            assert_eq!(s.metrics[k], 0.0, "{k}");
        }
        assert!(s.metrics.values().all(|v| v.is_finite()));
        assert!(s.summary.contains("id 100%"), "{}", s.summary);
        assert!(s.summary.contains("cs 0/s"), "{}", s.summary);
    }

    #[test]
    fn cpu_run_queue_thresholds() {
        let at = |running: u64, busy: u64| {
            let a = base(4);
            let d = vec![user(busy); 4];
            cpu(
                &[
                    stat_text(&a, 0, 0, running, 0),
                    stat_text(&plus(&a, &d), 0, 0, running, 0),
                ],
                4,
            )
        };
        let saturated = |s: &Section, level: Level| has(s, level, "CPU saturation");
        let s = at(5, 95); // r = 4.0
        assert!(!saturated(&s, Level::Warn) && !saturated(&s, Level::Crit));
        let s = at(6, 95); // r = 5.0
        assert!(saturated(&s, Level::Warn), "{:?}", s.findings);
        let s = at(10, 95); // r = 9.0
        assert!(saturated(&s, Level::Crit));
        assert_eq!(s.status, Status::Crit);
    }

    #[test]
    fn run_queue_on_idle_cpus() {
        let a = base(4);
        let d = vec![user(19); 4];
        let s = cpu(
            &[
                stat_text(&a, 0, 0, 6, 0),
                stat_text(&plus(&a, &d), 0, 0, 6, 0),
            ],
            4,
        );
        assert_eq!(s.metrics["r"], 5.0);
        assert!(!has(&s, Level::Warn, "CPU saturation"), "{:?}", s.findings);
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn run_queue_under_cgroup_quota() {
        let a = base(4);
        let d = vec![user(25); 4]; // 1 of 4 CPUs in use = 67% of a 1.5 CPU quota
        let texts = [
            stat_text(&a, 0, 0, 5, 0),
            stat_text(&plus(&a, &d), 0, 0, 5, 0),
        ]; // r = 4.0 > 2 × 1.5
        let ctx = Context {
            sys: SysInfo {
                cpus_online: 4,
                cgroup_cpu_limit: Some(1.5),
                ..Default::default()
            },
            interval: 1.0,
            count: 1,
        };
        let s = feed(Cpu::default(), &texts).evaluate(&ctx);
        assert_eq!(s.status, Status::Crit, "{:?}", s.findings);
    }

    #[test]
    fn cpu_iowait_thresholds() {
        assert_eq!(split(5, 0, 20, 0).status, Status::Ok);
        let s = split(5, 0, 21, 0);
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "I/O bound"), "{:?}", s.findings);
        assert_eq!(split(5, 0, 51, 0).status, Status::Crit);
    }

    #[test]
    fn cpu_steal_thresholds() {
        assert_eq!(split(5, 0, 0, 10).status, Status::Ok);
        let s = split(5, 0, 0, 11);
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "steal"), "{:?}", s.findings);
        assert_eq!(split(5, 0, 0, 26).status, Status::Crit);
    }

    #[test]
    fn cpu_busy_threshold() {
        assert_eq!(split(75, 15, 0, 0).status, Status::Ok);
        let s = split(76, 15, 0, 0);
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "91% busy"), "{:?}", s.findings);
    }

    #[test]
    fn cpu_system_time_note() {
        let s = split(10, 21, 0, 0);
        assert_eq!(s.status, Status::Ok);
        assert!(has(&s, Level::Note, "high system time"), "{:?}", s.findings);
        assert!(!has(&split(10, 20, 0, 0), Level::Note, "high system time"));
    }

    #[test]
    fn cpu_blocked_note() {
        let a = base(1);
        let s = cpu(
            &[
                stat_text(&a, 0, 0, 1, 0),
                stat_text(&plus(&a, &[user(10)]), 0, 0, 1, 1),
            ],
            1,
        );
        assert_eq!(s.metrics["b"], 0.5);
        assert_eq!(s.status, Status::Ok);
        assert!(has(&s, Level::Note, "blocked on I/O"), "{:?}", s.findings);
        assert!(!has(&split(10, 0, 0, 0), Level::Note, "blocked"));
    }

    #[test]
    fn cpu_missing_stat_is_skipped() {
        let mut c = Cpu::default();
        c.sample(&MemSource::new(), 0.0);
        c.sample(&MemSource::new(), 1.0);
        let s = c.evaluate(&ctx(4));
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains("/proc/stat"), "{}", s.summary);
    }

    #[test]
    fn cpu_single_sample_is_skipped() {
        let s = cpu(&[stat_text(&base(1), 0, 0, 1, 0)], 1);
        assert_eq!(s.status, Status::Skipped);
        assert_eq!(s.summary, "not enough samples");
    }

    #[test]
    fn counts_are_compact() {
        assert_eq!(count(0.0), "0");
        assert_eq!(count(950.0), "950");
        assert_eq!(count(1000.0), "1k");
        assert_eq!(count(3200.0), "3.2k");
        assert_eq!(count(12_000.0), "12k");
        assert_eq!(count(2_500_000.0), "2.5M");
    }

    // --- cpu: run-queue latency -------------------------------------------------------------

    /// `/proc/schedstat` with `(run_delay ns, timeslices)` per CPU.
    fn schedstat_text(cpus: &[(u64, u64)]) -> String {
        let mut s = String::from("version 17\ntimestamp 4314327485\n");
        for (i, (delay, slices)) in cpus.iter().enumerate() {
            s += &format!("cpu{i} 0 0 0 0 0 0 5000 {delay} {slices}\ndomain0 MC f 0 0 0\n");
        }
        s
    }

    /// A 1 s window with 10% busy CPUs; each CPU's schedstat grows by `(run_delay, timeslices)`.
    fn runq(deltas: &[(u64, u64)]) -> Section {
        let stat = window(&vec![user(10); deltas.len()]);
        let start: Vec<(u64, u64)> = deltas.iter().map(|_| (BASE, BASE)).collect();
        let end: Vec<(u64, u64)> = deltas.iter().map(|(d, k)| (BASE + d, BASE + k)).collect();
        let src = MemSource::new();
        let mut c = Cpu::default();
        for (i, (st, sc)) in [(&stat[0], &start), (&stat[1], &end)].iter().enumerate() {
            src.set(PATH, st);
            src.set(SCHEDSTAT, &schedstat_text(sc));
            c.sample(&src, i as f64);
        }
        c.evaluate(&ctx(deltas.len()))
    }

    #[test]
    fn runq_wait_thresholds() {
        let s = runq(&[(2_000_000, 1)]);
        assert_eq!(s.metrics["runq_wait_ms"], 2.0);
        assert_eq!(s.status, Status::Ok, "{:?}", s.findings);
        let s = runq(&[(2_010_000, 1)]);
        assert_eq!(s.status, Status::Warn);
        assert!(
            has(&s, Level::Warn, "tasks wait 2.01 ms for a CPU on average"),
            "{:?}",
            s.findings
        );
        let s = runq(&[(10_000_000, 1)]);
        assert_eq!(s.status, Status::Warn);
        let s = runq(&[(10_010_000, 1)]);
        assert_eq!(s.status, Status::Crit);
        assert!(
            has(&s, Level::Crit, "run queue latency"),
            "{:?}",
            s.findings
        );
    }

    #[test]
    fn runq_wait_average_and_worst_cpu() {
        let s = runq(&[(1_000_000, 100), (9_000_000, 100)]);
        assert_eq!(s.metrics["runq_wait_ms"], 0.05);
        assert_eq!(s.metrics["runq_wait_max_cpu_ms"], 0.09);
        assert_eq!(s.status, Status::Ok);
        assert!(
            s.details
                .iter()
                .any(|d| d == "run queue wait 0.05 ms per timeslice, worst cpu1 0.09 ms"),
            "{:?}",
            s.details
        );
    }

    #[test]
    fn runq_wait_needs_timeslices() {
        let s = runq(&[(0, 0), (0, 0)]);
        assert!(!s.metrics.contains_key("runq_wait_ms"));
        assert!(!s.metrics.contains_key("runq_wait_max_cpu_ms"));
        assert!(!has(&s, Level::Warn, "tasks wait"));
        assert_eq!(s.status, Status::Ok);
        // A CPU with no timeslices is left out of the worst-CPU search.
        let s = runq(&[(5_000_000, 0), (1_000_000, 10)]);
        assert_eq!(s.metrics["runq_wait_max_cpu_ms"], 0.1);
    }

    #[test]
    fn runq_wait_missing_schedstat() {
        // No schedstat at all: the default helpers never write it.
        let s = split(10, 0, 0, 0);
        assert_eq!(s.status, Status::Ok);
        assert!(!s.metrics.contains_key("runq_wait_ms"));
        // Version 14 (pre-CFS layout) is not used.
        let a = base(1);
        let src = MemSource::new().with(SCHEDSTAT, "version 14\ncpu0 0 0 0 0 0 0 0 0 0 1 2 3\n");
        let mut c = Cpu::default();
        src.set(PATH, &stat_text(&a, 0, 0, 1, 0));
        c.sample(&src, 0.0);
        src.set(PATH, &stat_text(&plus(&a, &[user(10)]), 0, 0, 1, 0));
        c.sample(&src, 1.0);
        let s = c.evaluate(&ctx(1));
        assert_ne!(s.status, Status::Skipped);
        assert!(!s.metrics.contains_key("runq_wait_ms"));
        assert!(s.findings.is_empty(), "{:?}", s.findings);
    }

    // --- cpu-balance -------------------------------------------------------------------------

    #[test]
    fn balance_summary() {
        let s = balance(&[user(10), user(20), user(97), user(5)]);
        assert_eq!(s.summary, "4 cpus, avg 33%, max cpu2 97%");
        assert_eq!(s.metrics["max_cpu"], 2.0);
        assert_eq!(s.metrics["max_busy"], 97.0);
        assert_eq!(s.metrics["avg_busy"], 33.0);
        // Mean is 33%, so the 97% CPU is a hot spot.
        assert_eq!(s.status, Status::Warn);
    }

    #[test]
    fn balance_top_four_details() {
        let s = balance(&[
            user(1),
            [10, 0, 5, 80, 5, 0, 0, 0],
            user(3),
            user(40),
            user(2),
            user(30),
        ]);
        assert_eq!(s.details.len(), 4);
        assert!(s.details[0].starts_with("cpu3 40% busy"), "{:?}", s.details);
        assert!(
            s.details[2].starts_with("cpu1 15% busy: usr 10% sys 5% irq+soft 0% iowait 5%"),
            "{:?}",
            s.details
        );
        assert!(s.details[3].starts_with("cpu2 3%"), "{:?}", s.details);
    }

    #[test]
    fn balance_single_cpu() {
        let s = balance(&[user(99)]);
        assert!(s.summary.starts_with("1 cpu"), "{}", s.summary);
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn balance_identical_snapshots() {
        let a = stat_text(&base(4), 0, 0, 1, 0);
        let s = feed(CpuBalance::default(), &[a.clone(), a]).evaluate(&ctx(4));
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.metrics["max_busy"], 0.0);
        assert_eq!(s.metrics["avg_busy"], 0.0);
        assert!(s.metrics.values().all(|v| v.is_finite()));
    }

    #[test]
    fn balance_hot_cpu_threshold() {
        assert_eq!(
            balance(&[user(90), user(5), user(5), user(5)]).status,
            Status::Ok
        );
        let s = balance(&[user(5), user(5), user(91), user(5)]);
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "cpu2"), "{:?}", s.findings);
        assert!(has(&s, Level::Warn, "single-threaded"), "{:?}", s.findings);
    }

    #[test]
    fn balance_mean_threshold() {
        assert_eq!(balance(&[user(95), user(5)]).status, Status::Ok);
        assert_eq!(balance(&[user(95), user(4)]).status, Status::Warn);
    }

    #[test]
    fn balance_interrupt_imbalance() {
        let s = balance(&[user(1), [30, 0, 5, 5, 0, 20, 40, 0]]);
        assert_eq!(s.status, Status::Warn);
        assert!(
            has(
                &s,
                Level::Warn,
                "interrupt imbalance (check IRQ affinity / RPS)"
            ),
            "{:?}",
            s.findings
        );
        let s = balance(&[user(1), [40, 0, 8, 4, 0, 24, 24, 0]]);
        assert_eq!(s.status, Status::Warn);
        assert!(
            !has(&s, Level::Warn, "interrupt imbalance"),
            "{:?}",
            s.findings
        );
    }

    #[test]
    fn balance_missing_stat_is_skipped() {
        let mut c = CpuBalance::default();
        c.sample(&MemSource::new(), 0.0);
        let s = c.evaluate(&ctx(4));
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains("/proc/stat"), "{}", s.summary);
        // An aggregate line without cpuN lines can't be balanced.
        let s = feed(
            CpuBalance::default(),
            &["cpu 1 0 0 1\n".to_owned(), "cpu 2 0 0 2\n".to_owned()],
        )
        .evaluate(&ctx(1));
        assert_eq!(s.status, Status::Skipped);
    }

    #[test]
    fn balance_single_sample_is_skipped() {
        let s = feed(CpuBalance::default(), &[stat_text(&base(2), 0, 0, 1, 0)]).evaluate(&ctx(2));
        assert_eq!(s.status, Status::Skipped);
        assert_eq!(s.summary, "not enough samples");
    }
}
