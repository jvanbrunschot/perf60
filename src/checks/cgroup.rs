//! Container/cgroup saturation, invisible in host-wide tools: CPU quota throttling and memory
//! limit events of our own cgroup (`cgroup`), and per-cgroup CPU and memory across the
//! hierarchy (`cgroups-top`, like `systemd-cgtop`).

use std::collections::{BTreeMap, VecDeque};

use crate::check::{Check, Context, Resource, SampleError, Section, rate};
use crate::procfs::cgroup::{self, CpuStat, MemoryEvents, OwnCgroup};
use crate::procfs::system;
use crate::source::Source;
use crate::units;

const ROOT: &str = "/sys/fs/cgroup";
const PROC_CGROUP: &str = "/proc/self/cgroup";

/// Own cgroup: throttled share of quota periods above these is WARN / CRIT.
const THROTTLE_WARN_PCT: f64 = 10.0;
const THROTTLE_CRIT_PCT: f64 = 25.0;
/// Top cgroups: any leaf throttled above this share of its periods is WARN.
const TOP_THROTTLE_WARN_PCT: f64 = 25.0;
/// Walk limits: levels below the hierarchy root, and directories per walk.
const MAX_DEPTH: usize = 6;
const MAX_CGROUPS: usize = 2000;
const TOP_N: usize = 5;
const MAX_NAMED: usize = 3;
const PATH_WIDTH: usize = 60;

/// Percentages: one decimal below 10, so 9.9% doesn't print as "10%".
fn pct_str(p: f64) -> String {
    if p < 10.0 {
        let s = format!("{p:.1}");
        s.strip_suffix(".0").unwrap_or(&s).to_owned()
    } else {
        format!("{p:.0}")
    }
}

/// `base` + cgroup `path`, without a trailing slash (`/` is the base itself).
fn join(base: &str, path: &str) -> String {
    format!("{base}{}", path.trim_end_matches('/'))
}

fn read_value(src: &dyn Source, path: &str) -> Option<u64> {
    src.read_to_string(path)
        .ok()
        .and_then(|s| cgroup::parse_value(&s).ok())
}

/// Throttling between two `cpu.stat` samples.
#[derive(Debug, Clone, Copy)]
struct Throttle {
    periods: u64,
    throttled: u64,
    pct: f64,
    ms_per_sec: f64,
}

fn throttle(a: (f64, CpuStat), b: (f64, CpuStat)) -> Throttle {
    let periods = b.1.nr_periods.saturating_sub(a.1.nr_periods);
    let throttled = b.1.nr_throttled.saturating_sub(a.1.nr_throttled);
    let pct = if periods > 0 {
        (throttled as f64 * 100.0 / periods as f64).min(100.0)
    } else {
        0.0
    };
    Throttle {
        periods,
        throttled,
        pct,
        ms_per_sec: rate((a.0, a.1.throttled_usec), (b.0, b.1.throttled_usec)) / 1000.0,
    }
}

// ---------------------------------------------------------------------------------------------
// cgroup (own cgroup)

/// Where the own cgroup's files are.
#[derive(Debug, Clone, PartialEq)]
struct OwnDirs {
    v1: bool,
    /// Path as listed in `/proc/self/cgroup`.
    path: String,
    cpu_dir: Option<String>,
    mem_dir: Option<String>,
    /// The hierarchy is mounted at the own cgroup (container without a cgroup namespace).
    mount_root: bool,
}

fn resolve_own(src: &dyn Source, error: &mut SampleError) -> Option<OwnDirs> {
    let text = error.read(src, PROC_CGROUP)?;
    let own = cgroup::parse_proc_cgroup(&text)
        .ok()
        .and_then(|l| cgroup::own_cgroup(&l));
    let Some(own) = own else {
        error.record(
            PROC_CGROUP,
            &std::io::Error::other("no cpu, memory or unified hierarchy line"),
        );
        return None;
    };
    Some(match own {
        OwnCgroup::V2(path) => {
            let dir = join(ROOT, &path);
            let mount_root = path != "/"
                && !src.exists(&format!("{dir}/cpu.stat"))
                && src.exists(&format!("{ROOT}/cpu.stat"));
            let dir = if mount_root { ROOT.to_owned() } else { dir };
            OwnDirs {
                v1: false,
                path,
                cpu_dir: Some(dir.clone()),
                mem_dir: Some(dir),
                mount_root,
            }
        }
        OwnCgroup::V1 { cpu, memory } => {
            let mut mount_root = false;
            let cpu_dir = cpu.as_ref().and_then(|(ctrls, path)| {
                let mut bases: Vec<String> = Vec::new();
                for c in [ctrls.as_str(), "cpu,cpuacct", "cpu", "cpuacct"] {
                    let b = format!("{ROOT}/{c}");
                    if !bases.contains(&b) {
                        bases.push(b);
                    }
                }
                if let Some(d) = bases
                    .iter()
                    .map(|b| join(b, path))
                    .find(|d| src.exists(&format!("{d}/cpu.stat")))
                {
                    return Some(d);
                }
                let root = bases
                    .into_iter()
                    .find(|b| src.exists(&format!("{b}/cpu.stat")))?;
                mount_root = path != "/";
                Some(root)
            });
            let mem_path = memory.clone().or_else(|| cpu.as_ref().map(|c| c.1.clone()));
            let mem_dir = mem_path.as_ref().map(|path| {
                let base = format!("{ROOT}/memory");
                let dir = join(&base, path);
                if path != "/"
                    && !src.exists(&format!("{dir}/memory.usage_in_bytes"))
                    && src.exists(&format!("{base}/memory.usage_in_bytes"))
                {
                    mount_root = true;
                    base
                } else {
                    dir
                }
            });
            OwnDirs {
                v1: true,
                path: cpu.map(|c| c.1).or(mem_path).unwrap_or_else(|| "/".into()),
                cpu_dir,
                mem_dir,
                mount_root,
            }
        }
    })
}

/// Counters compared between the first and the last sample.
#[derive(Debug, Clone, Copy, Default)]
struct OwnSnap {
    t: f64,
    cpu: Option<CpuStat>,
    events: Option<MemoryEvents>,
    failcnt: Option<u64>,
}

/// Values reported from the last sample.
#[derive(Debug, Clone, Copy, Default)]
struct OwnState {
    quota: Option<f64>,
    usage: Option<u64>,
    limit: Option<u64>,
    high: Option<u64>,
}

#[derive(Default)]
pub struct Cgroup {
    /// Resolved on the first sample.
    own: Option<Option<OwnDirs>>,
    first: Option<OwnSnap>,
    last: Option<OwnSnap>,
    state: OwnState,
    error: SampleError,
}

impl Check for Cgroup {
    fn id(&self) -> &'static str {
        "cgroup"
    }

    fn sample(&mut self, src: &dyn Source, t: f64) {
        let own = self
            .own
            .get_or_insert_with(|| resolve_own(src, &mut self.error))
            .clone();
        let Some(own) = own else {
            return;
        };
        let mut snap = OwnSnap {
            t,
            ..Default::default()
        };
        let mut state = OwnState::default();
        if let Some(dir) = &own.cpu_dir {
            let path = format!("{dir}/cpu.stat");
            snap.cpu = self
                .error
                .read(src, &path)
                .and_then(|s| cgroup::parse_cpu_stat(&s).ok());
            state.quota = if own.v1 {
                match (
                    src.read_to_string(&format!("{dir}/cpu.cfs_quota_us")),
                    src.read_to_string(&format!("{dir}/cpu.cfs_period_us")),
                ) {
                    (Ok(q), Ok(p)) => system::cgroup1_cpu_quota(&q, &p),
                    _ => None,
                }
            } else {
                src.read_to_string(&format!("{dir}/cpu.max"))
                    .ok()
                    .and_then(|s| system::cgroup2_cpu_max(&s))
            };
        }
        if let Some(dir) = &own.mem_dir {
            if own.v1 {
                snap.failcnt = read_value(src, &format!("{dir}/memory.failcnt"));
                state.usage = self
                    .error
                    .read(src, &format!("{dir}/memory.usage_in_bytes"))
                    .and_then(|s| cgroup::parse_value(&s).ok());
                state.limit = read_value(src, &format!("{dir}/memory.limit_in_bytes"));
            } else {
                snap.events = self
                    .error
                    .read(src, &format!("{dir}/memory.events"))
                    .and_then(|s| cgroup::parse_memory_events(&s).ok());
                state.usage = read_value(src, &format!("{dir}/memory.current"));
                let limit = |f: &str| {
                    src.read_to_string(&format!("{dir}/{f}"))
                        .ok()
                        .and_then(|s| cgroup::parse_limit(&s).ok())
                        .flatten()
                };
                state.limit = limit("memory.max");
                state.high = limit("memory.high");
            }
        }
        if self.first.is_none() {
            self.first = Some(snap);
        }
        self.last = Some(snap);
        self.state = state;
    }

    fn evaluate(&self, ctx: &Context) -> Section {
        let mut s = Section::new(
            "cgroup",
            "Own cgroup",
            "cgroup cpu.stat / memory.events",
            Resource::Cpu,
        );
        let skip = |s: Section| s.skipped(self.error.get().unwrap_or("no samples"));
        let (Some(Some(own)), Some(first), Some(last)) = (&self.own, self.first, self.last) else {
            return skip(s);
        };
        // At `/` on a host we are in the root cgroup, which has no limits of its own. Inside a
        // container `/` is the cgroup namespace root, i.e. the container's cgroup.
        if own.path == "/" && !ctx.sys.container && !own.mount_root {
            s.summary("host root cgroup (no own limits)");
            return s;
        }
        let st = self.state;
        if last.cpu.is_none()
            && last.events.is_none()
            && last.failcnt.is_none()
            && st.usage.is_none()
        {
            return skip(s);
        }

        let label = if own.mount_root {
            format!("{} (hierarchy mounted at own cgroup)", own.path)
        } else if own.path == "/" {
            "/ (container cgroup namespace root)".to_owned()
        } else {
            own.path.clone()
        };
        s.detail(format!(
            "cgroup {label}, {}",
            if own.v1 { "v1" } else { "v2" }
        ));

        let mut summary = Vec::new();
        summary.push(cpu_part(&mut s, &first, &last, st.quota));
        if let Some(m) = memory_part(&mut s, &st, ctx.sys.mem_total_bytes) {
            summary.push(m);
        }
        if let Some(e) = events_part(&mut s, &first, &last) {
            summary.push(e);
        }
        s.summary(summary.join(", "));
        s
    }
}

/// CPU throttling details, metrics and findings; returns the summary part.
fn cpu_part(s: &mut Section, first: &OwnSnap, last: &OwnSnap, quota: Option<f64>) -> String {
    let (Some(a), Some(b)) = (first.cpu, last.cpu) else {
        return "cpu.stat not available".to_owned();
    };
    let w = throttle((first.t, a), (last.t, b));
    s.metric("throttled_pct", w.pct);
    s.metric("throttled_ms_per_sec", w.ms_per_sec);
    let quota_str = quota.map(units::cpus);
    let quota_detail = quota_str
        .as_ref()
        .map_or("no quota".to_owned(), |q| format!("quota {q} CPUs"));
    if w.periods > 0 {
        s.detail(format!(
            "cpu: {quota_detail}, {} of {} periods throttled ({:.1}%), {:.0} ms/s throttled (summed over CPUs)",
            w.throttled, w.periods, w.pct, w.ms_per_sec
        ));
        s.threshold(
            w.pct,
            THROTTLE_WARN_PCT,
            THROTTLE_CRIT_PCT,
            format!(
                "CPU quota throttling: {:.1}% of periods throttled ({:.0} ms/s): raise cpu.max / limits.cpu or reduce parallelism",
                w.pct, w.ms_per_sec
            ),
        );
    } else if quota.is_some() {
        s.detail(format!(
            "cpu: {quota_detail}, no CPU quota active in the window (no runnable periods)"
        ));
    } else {
        s.detail("cpu: no CPU quota active");
    }
    if b.nr_periods > 0 {
        s.detail(format!(
            "since creation: {:.1}% of {} periods throttled, {:.1} s throttled",
            b.nr_throttled as f64 * 100.0 / b.nr_periods as f64,
            b.nr_periods,
            b.throttled_usec as f64 / 1e6
        ));
    }
    let throttled = format!(
        "throttled {}% of periods, {:.0} ms/s",
        pct_str(w.pct),
        w.ms_per_sec
    );
    match (quota_str, w.periods > 0) {
        (Some(q), true) => format!("cpu quota {q} ({throttled})"),
        (Some(q), false) => format!("cpu quota {q} (not throttled)"),
        (None, true) => throttled,
        (None, false) => "no cpu quota".to_owned(),
    }
}

/// Memory usage against the limit; returns the summary part.
fn memory_part(s: &mut Section, st: &OwnState, mem_total: Option<u64>) -> Option<String> {
    let usage = st.usage?;
    // v1 reports "unlimited" as a huge number: a limit at or above RAM is no limit.
    let limit = st
        .limit
        .filter(|l| *l > 0 && *l < 1 << 62 && mem_total.is_none_or(|t| *l < t));
    let high = st.high.map_or(String::new(), |h| {
        format!(", memory.high {}", units::bytes(h))
    });
    match limit {
        Some(l) => {
            s.detail(format!(
                "memory: {} of {} ({}%){high}",
                units::bytes(usage),
                units::bytes(l),
                pct_str(usage as f64 * 100.0 / l as f64)
            ));
            Some(format!(
                "memory {} of {}",
                units::bytes(usage),
                units::bytes(l)
            ))
        }
        None => {
            s.detail(format!("memory: {}, no limit{high}", units::bytes(usage)));
            Some(format!("memory {} (no limit)", units::bytes(usage)))
        }
    }
}

/// Memory limit events (v2 `memory.events`, v1 `memory.failcnt`); returns the summary part.
fn events_part(s: &mut Section, first: &OwnSnap, last: &OwnSnap) -> Option<String> {
    if let (Some(a), Some(b)) = (first.events, last.events) {
        let oom = b.oom_kill.saturating_sub(a.oom_kill);
        let max = b.max.saturating_sub(a.max);
        let high = b.high.saturating_sub(a.high);
        s.metric("oom_kills", oom as f64);
        s.metric("memory_max_events", max as f64);
        s.detail(format!(
            "memory.events in window: oom_kill {oom}, max {max}, high {high} (since creation: oom_kill {}, max {}, high {})",
            b.oom_kill, b.max, b.high
        ));
        if oom > 0 {
            s.crit(format!(
                "{oom} OOM kills in this cgroup during the window: its memory limit is too low for the workload"
            ));
        }
        if max > 0 {
            s.warn(format!(
                "hit memory.max {max} times during the window: reclaim or OOM imminent"
            ));
        }
        if high > 0 {
            s.note(format!(
                "throttled at memory.high {high} times during the window"
            ));
        }
        let parts: Vec<String> = [
            (oom, "OOM kills"),
            (max, "max events"),
            (high, "high events"),
        ]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect();
        return Some(if parts.is_empty() {
            "no memory events".to_owned()
        } else {
            parts.join(", ")
        });
    }
    let (Some(a), Some(b)) = (first.failcnt, last.failcnt) else {
        return None;
    };
    let hits = b.saturating_sub(a);
    s.metric("memory_max_events", hits as f64);
    s.detail(format!(
        "memory.failcnt in window: {hits} (since creation: {b})"
    ));
    if hits > 0 {
        s.warn(format!(
            "hit the memory limit {hits} times during the window (memory.failcnt): reclaim or OOM imminent"
        ));
        Some(format!("{hits} memory limit hits"))
    } else {
        Some("no memory limit hits".to_owned())
    }
}

// ---------------------------------------------------------------------------------------------
// cgroups-top (systemd-cgtop)

#[derive(Debug, Clone, PartialEq)]
enum Hierarchy {
    V2,
    /// v1 cpu hierarchy mount point.
    V1(String),
}

impl Hierarchy {
    fn detect(src: &dyn Source) -> Option<Self> {
        if src.exists(&format!("{ROOT}/cgroup.controllers"))
            || src.exists(&format!("{ROOT}/cpu.stat"))
        {
            return Some(Hierarchy::V2);
        }
        ["cpu,cpuacct", "cpuacct,cpu", "cpu"]
            .iter()
            .map(|c| format!("{ROOT}/{c}"))
            .find(|b| src.exists(b))
            .map(Hierarchy::V1)
    }

    fn base(&self) -> &str {
        match self {
            Hierarchy::V2 => ROOT,
            Hierarchy::V1(b) => b,
        }
    }
}

/// Could this `read_dir` entry be a child cgroup? Interface files are `<controller>.<name>` or
/// one of the v1 core files; everything else is tried as a directory.
fn maybe_cgroup_dir(name: &str) -> bool {
    const CORE_FILES: [&str; 3] = ["tasks", "notify_on_release", "release_agent"];
    const PREFIXES: [&str; 17] = [
        "cgroup",
        "cpu",
        "cpuacct",
        "cpuset",
        "memory",
        "io",
        "blkio",
        "pids",
        "hugetlb",
        "rdma",
        "misc",
        "devices",
        "freezer",
        "net_cls",
        "net_prio",
        "perf_event",
        "irq",
    ];
    if CORE_FILES.contains(&name) {
        return false;
    }
    match name.split_once('.') {
        Some((prefix, _)) => !PREFIXES.contains(&prefix),
        None => true,
    }
}

/// One directory found by the walk; `rel` is relative to the hierarchy root (`""` = root).
struct Visit {
    rel: String,
    cgroup: bool,
    has_mem: bool,
    has_children: bool,
}

/// Breadth-first walk, at most `MAX_DEPTH` levels and `MAX_CGROUPS` directories. Returns the
/// directories and whether the cap was hit.
fn walk(src: &dyn Source, base: &str, v1: bool, error: &mut SampleError) -> (Vec<Visit>, bool) {
    let mut out: Vec<Visit> = Vec::new();
    let mut queue: VecDeque<(String, usize, Option<usize>)> =
        VecDeque::from([(String::new(), 0, None)]);
    while let Some((rel, depth, parent)) = queue.pop_front() {
        let dir = format!("{base}{rel}");
        let names = match src.read_dir(&dir) {
            Ok(n) => n,
            Err(e) => {
                // Not a directory (or gone); only the root's failure is worth reporting.
                if rel.is_empty() {
                    error.record(&dir, &e);
                }
                continue;
            }
        };
        if out.len() == MAX_CGROUPS {
            return (out, true);
        }
        if let Some(p) = parent {
            out[p].has_children = true;
        }
        let has = |f: &str| names.iter().any(|n| n == f);
        let idx = out.len();
        if depth < MAX_DEPTH {
            for n in names.iter().filter(|n| maybe_cgroup_dir(n)) {
                queue.push_back((format!("{rel}/{n}"), depth + 1, Some(idx)));
            }
        }
        out.push(Visit {
            cgroup: has("cpu.stat") || (v1 && has("cpuacct.usage")),
            has_mem: has("memory.current"),
            has_children: false,
            rel,
        });
    }
    (out, false)
}

#[derive(Debug, Clone, Copy, Default)]
struct CgSample {
    t: f64,
    usage_us: Option<u64>,
    periods: u64,
    throttled: u64,
    mem: Option<u64>,
}

struct Entry {
    first: CgSample,
    last: CgSample,
    leaf: bool,
}

/// A leaf cgroup with its window values.
struct Row<'a> {
    path: &'a str,
    cpu: f64,
    mem: Option<u64>,
    throttled: f64,
}

#[derive(Default)]
pub struct CgroupsTop {
    /// Detected on the first sample.
    hierarchy: Option<Option<Hierarchy>>,
    /// Cgroups present in the latest walk, keyed by path (`/` = hierarchy root).
    entries: BTreeMap<String, Entry>,
    capped: bool,
    error: SampleError,
}

impl Check for CgroupsTop {
    fn id(&self) -> &'static str {
        "cgroups-top"
    }

    fn sample(&mut self, src: &dyn Source, t: f64) {
        let Some(h) = self
            .hierarchy
            .get_or_insert_with(|| Hierarchy::detect(src))
            .clone()
        else {
            return;
        };
        let v1 = matches!(h, Hierarchy::V1(_));
        let base = h.base();
        let (visits, capped) = walk(src, base, v1, &mut self.error);
        self.capped = capped;
        let mut seen = BTreeMap::new();
        for v in visits.iter().filter(|v| v.cgroup) {
            let dir = format!("{base}{}", v.rel);
            let stat = src
                .read_to_string(&format!("{dir}/cpu.stat"))
                .ok()
                .and_then(|s| cgroup::parse_cpu_stat(&s).ok())
                .unwrap_or_default();
            let (usage_us, mem) = if v1 {
                (
                    read_value(src, &format!("{dir}/cpuacct.usage")).map(|ns| ns / 1000),
                    read_value(
                        src,
                        &format!("{ROOT}/memory{}/memory.usage_in_bytes", v.rel),
                    ),
                )
            } else {
                (
                    stat.usage_usec,
                    v.has_mem
                        .then(|| read_value(src, &format!("{dir}/memory.current")))
                        .flatten(),
                )
            };
            let sample = CgSample {
                t,
                usage_us,
                periods: stat.nr_periods,
                throttled: stat.nr_throttled,
                mem,
            };
            let path = if v.rel.is_empty() {
                "/".to_owned()
            } else {
                v.rel.clone()
            };
            let first = self.entries.get(&path).map_or(sample, |e| e.first);
            seen.insert(
                path,
                Entry {
                    first,
                    last: sample,
                    leaf: !v.has_children,
                },
            );
        }
        // Keep only cgroups that still exist.
        self.entries = seen;
    }

    fn evaluate(&self, ctx: &Context) -> Section {
        let mut s = Section::new("cgroups-top", "Top cgroups", "systemd-cgtop", Resource::Cpu);
        let Some(Some(h)) = &self.hierarchy else {
            return s.skipped(format!("no cgroup hierarchy at {ROOT}"));
        };
        if self.entries.is_empty() {
            let reason = self.error.get().map_or_else(
                || format!("no cgroups found under {}", h.base()),
                str::to_owned,
            );
            return s.skipped(reason);
        }
        let n = self.entries.len();
        let mut rows: Vec<Row> = self
            .entries
            .iter()
            .filter(|(_, e)| e.leaf)
            .map(|(path, e)| {
                let (a, b) = (e.first, e.last);
                let cpu = match (a.usage_us, b.usage_us) {
                    // µs of CPU per second of wall time → percent of one CPU.
                    (Some(x), Some(y)) => rate((a.t, x), (b.t, y)) / 1e4,
                    _ => 0.0,
                };
                let periods = b.periods.saturating_sub(a.periods);
                let throttled = if periods > 0 {
                    b.throttled.saturating_sub(a.throttled) as f64 * 100.0 / periods as f64
                } else {
                    0.0
                };
                Row {
                    path,
                    cpu,
                    mem: b.mem,
                    throttled,
                }
            })
            .collect();
        rows.sort_by(|x, y| y.cpu.total_cmp(&x.cpu).then(x.path.cmp(y.path)));
        let mut by_mem: Vec<&Row> = rows.iter().filter(|r| r.mem.is_some()).collect();
        by_mem.sort_by(|x, y| y.mem.cmp(&x.mem).then(x.path.cmp(y.path)));

        let top_cpu = rows.first();
        let top_mem = by_mem.first();
        let summary = if n == 1 && self.entries.contains_key("/") && ctx.sys.container {
            "1 cgroup visible (container cgroup namespace)".to_owned()
        } else {
            let mut sum = format!("{n} cgroup{}", if n == 1 { "" } else { "s" });
            if let Some(r) = top_cpu {
                sum.push_str(&format!(
                    ", top cpu: {} {}%",
                    shorten(r.path, PATH_WIDTH),
                    pct_str(r.cpu)
                ));
            }
            if let Some(r) = top_mem {
                sum.push_str(&format!(
                    ", top memory: {} {}",
                    shorten(r.path, PATH_WIDTH),
                    units::bytes(r.mem.unwrap_or(0))
                ));
            }
            sum
        };
        s.summary(summary);

        s.detail(format!(
            "top cpu ({} leaf cgroup{} of {n}, 100% = one CPU):",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" }
        ));
        let cpu_top: Vec<&Row> = rows.iter().take(TOP_N).collect();
        for r in &cpu_top {
            s.detail(row_line(r));
        }
        let mem_top: Vec<&Row> = by_mem.iter().take(TOP_N).copied().collect();
        let same = mem_top
            .iter()
            .all(|m| cpu_top.iter().any(|c| c.path == m.path));
        if !mem_top.is_empty() && !same {
            s.detail("top memory:");
            for r in &mem_top {
                s.detail(row_line(r));
            }
        }
        if self.capped {
            s.detail(format!("walk stopped at {MAX_CGROUPS} cgroups"));
        }

        let mut throttled: Vec<&Row> = rows
            .iter()
            .filter(|r| r.throttled > TOP_THROTTLE_WARN_PCT)
            .collect();
        throttled.sort_by(|x, y| y.throttled.total_cmp(&x.throttled).then(x.path.cmp(y.path)));
        s.metric("cgroups", n as f64);
        s.metric("top_cpu_pct", top_cpu.map_or(0.0, |r| r.cpu));
        s.metric("throttled_cgroups", throttled.len() as f64);
        if !throttled.is_empty() {
            let named: Vec<String> = throttled
                .iter()
                .take(MAX_NAMED)
                .map(|r| format!("{} {:.0}%", shorten(r.path, PATH_WIDTH), r.throttled))
                .collect();
            let more = match throttled.len().saturating_sub(MAX_NAMED) {
                0 => String::new(),
                k => format!(" +{k} more"),
            };
            s.warn(format!(
                "cgroups throttled on their CPU quota (> {TOP_THROTTLE_WARN_PCT:.0}% of periods): {}{more}: raise cpu.max / limits.cpu or reduce parallelism",
                named.join(", ")
            ));
        }
        s
    }
}

fn row_line(r: &Row) -> String {
    let mem = r.mem.map_or("-".to_owned(), units::bytes);
    format!(
        "{:6.1}% cpu {mem:>8}  {}",
        r.cpu,
        shorten(r.path, PATH_WIDTH)
    )
}

/// Shorten `path` to `max` characters by replacing its middle with `…` (keeps more of the end,
/// where container ids and unit names are).
fn shorten(path: &str, max: usize) -> String {
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= max || max < 3 {
        return path.to_owned();
    }
    let head = (max - 1) / 3;
    let tail = max - 1 - head;
    let start: String = chars[..head].iter().collect();
    let end: String = chars[chars.len() - tail..].iter().collect();
    format!("{start}…{end}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Level, Status};
    use crate::source::{FsSource, MemSource};
    use crate::sysinfo::SysInfo;

    const LEGACY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/linux-legacy");
    const MIB: u64 = 1 << 20;

    fn ctx(container: bool) -> Context {
        Context {
            sys: SysInfo {
                cpus_online: 4,
                container,
                mem_total_bytes: Some(8 << 30),
                ..Default::default()
            },
            interval: 1.0,
            count: 1,
        }
    }

    type Files = Vec<(String, String)>;

    fn f(path: &str, content: impl Into<String>) -> (String, String) {
        (path.to_owned(), content.into())
    }

    /// Sample `C` at t=0 with `before`, apply `after`, sample at t=1 and evaluate.
    fn run<C: Check + Default>(before: &Files, after: &Files, container: bool) -> Section {
        let src = MemSource::new();
        for (p, v) in before {
            src.set(p, v);
        }
        let mut c = C::default();
        c.sample(&src, 0.0);
        for (p, v) in after {
            src.set(p, v);
        }
        c.sample(&src, 1.0);
        c.evaluate(&ctx(container))
    }

    fn cpu_stat(usage: u64, periods: u64, throttled: u64, throttled_us: u64) -> String {
        format!(
            "usage_usec {usage}\nuser_usec 0\nsystem_usec 0\nnr_periods {periods}\n\
             nr_throttled {throttled}\nthrottled_usec {throttled_us}\n"
        )
    }

    fn events(high: u64, max: u64, oom_kill: u64) -> String {
        format!("low 0\nhigh {high}\nmax {max}\noom {oom_kill}\noom_kill {oom_kill}\n")
    }

    fn has(s: &Section, level: Level, needle: &str) -> bool {
        s.findings
            .iter()
            .any(|f| f.level == level && f.message.contains(needle))
    }

    // --- own cgroup -----------------------------------------------------------------------

    /// Own v2 cgroup `/app` with `periods` of which `throttled` in a 1 s window.
    fn app_throttle(periods: u64, throttled: u64) -> Section {
        run::<Cgroup>(
            &vec![
                f(PROC_CGROUP, "0::/app\n"),
                f("/sys/fs/cgroup/app/cpu.stat", cpu_stat(0, 0, 0, 0)),
            ],
            &vec![f(
                "/sys/fs/cgroup/app/cpu.stat",
                cpu_stat(0, periods, throttled, 0),
            )],
            false,
        )
    }

    /// Own v2 cgroup `/app` whose memory.events change from `a` to `b`.
    fn app_events(a: String, b: String) -> Section {
        run::<Cgroup>(
            &vec![
                f(PROC_CGROUP, "0::/app\n"),
                f("/sys/fs/cgroup/app/cpu.stat", cpu_stat(0, 0, 0, 0)),
                f("/sys/fs/cgroup/app/memory.events", a),
            ],
            &vec![f("/sys/fs/cgroup/app/memory.events", b)],
            false,
        )
    }

    #[test]
    fn v2_own_cgroup_path() {
        let dir = "/sys/fs/cgroup/kubepods.slice/pod1/cri-containerd-abc.scope";
        let s = run::<Cgroup>(
            &vec![
                f(
                    PROC_CGROUP,
                    "0::/kubepods.slice/pod1/cri-containerd-abc.scope\n",
                ),
                f(&format!("{dir}/cpu.stat"), cpu_stat(0, 0, 0, 0)),
                f(&format!("{dir}/memory.events"), events(0, 0, 0)),
                // The root must not be read instead.
                f("/sys/fs/cgroup/cpu.stat", cpu_stat(0, 0, 0, 0)),
            ],
            &vec![f(&format!("{dir}/cpu.stat"), cpu_stat(0, 100, 50, 0))],
            false,
        );
        assert_eq!(
            s.details[0],
            "cgroup /kubepods.slice/pod1/cri-containerd-abc.scope, v2"
        );
        assert_eq!(s.metrics["throttled_pct"], 50.0);
        assert_eq!(s.metrics["oom_kills"], 0.0);
    }

    #[test]
    fn host_root_cgroup_is_ok() {
        let files = vec![
            f(PROC_CGROUP, "0::/\n"),
            f("/sys/fs/cgroup/cpu.stat", cpu_stat(0, 0, 0, 0)),
        ];
        let after = vec![f("/sys/fs/cgroup/cpu.stat", cpu_stat(0, 100, 40, 0))];
        let s = run::<Cgroup>(&files, &after, false);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.summary, "host root cgroup (no own limits)");
        assert!(s.findings.is_empty());
    }

    #[test]
    fn container_root_is_evaluated() {
        let files = vec![
            f(PROC_CGROUP, "0::/\n"),
            f("/sys/fs/cgroup/cpu.stat", cpu_stat(0, 0, 0, 0)),
        ];
        let after = vec![f("/sys/fs/cgroup/cpu.stat", cpu_stat(0, 100, 40, 0))];
        let s = run::<Cgroup>(&files, &after, true);
        assert_eq!(s.status, Status::Crit);
        assert!(
            s.details[0].contains("container cgroup namespace root"),
            "{}",
            s.details[0]
        );
        assert_eq!(s.metrics["throttled_pct"], 40.0);
    }

    fn v1_files(cgroup: &str, cpu: &str, mem: &str) -> Files {
        vec![
            f(PROC_CGROUP, cgroup),
            f(
                &format!("{cpu}/cpu.stat"),
                "nr_periods 0\nnr_throttled 0\nthrottled_time 0\n",
            ),
            f(&format!("{cpu}/cpu.cfs_quota_us"), "50000\n"),
            f(&format!("{cpu}/cpu.cfs_period_us"), "100000\n"),
            f(
                &format!("{mem}/memory.usage_in_bytes"),
                (100 * MIB).to_string(),
            ),
            f(
                &format!("{mem}/memory.limit_in_bytes"),
                (200 * MIB).to_string(),
            ),
            f(&format!("{mem}/memory.failcnt"), "3\n"),
        ]
    }

    #[test]
    fn v1_controller_paths() {
        let cpu = "/sys/fs/cgroup/cpu,cpuacct/system.slice/app.service";
        let mem = "/sys/fs/cgroup/memory/system.slice/app.service";
        let s = run::<Cgroup>(
            &v1_files(
                "10:memory:/system.slice/app.service\n4:cpu,cpuacct:/system.slice/app.service\n",
                cpu,
                mem,
            ),
            &vec![f(
                &format!("{cpu}/cpu.stat"),
                "nr_periods 10\nnr_throttled 2\nthrottled_time 30000000\n",
            )],
            false,
        );
        assert_eq!(s.details[0], "cgroup /system.slice/app.service, v1");
        assert_eq!(s.metrics["throttled_pct"], 20.0);
        assert_eq!(s.metrics["throttled_ms_per_sec"], 30.0);
        assert_eq!(s.status, Status::Warn);
        assert_eq!(
            s.summary,
            "cpu quota 0.5 (throttled 20% of periods, 30 ms/s), memory 100 MiB of 200 MiB, no memory limit hits"
        );
    }

    #[test]
    fn hybrid_prefers_v1() {
        let cpu = "/sys/fs/cgroup/cpu,cpuacct/app";
        let mut files = v1_files(
            "4:cpu,cpuacct:/app\n10:memory:/app\n0::/app\n",
            cpu,
            "/sys/fs/cgroup/memory/app",
        );
        files.push(f("/sys/fs/cgroup/app/cpu.stat", cpu_stat(0, 0, 0, 0)));
        let after = vec![
            f(
                &format!("{cpu}/cpu.stat"),
                "nr_periods 10\nnr_throttled 1\nthrottled_time 0\n",
            ),
            f("/sys/fs/cgroup/app/cpu.stat", cpu_stat(0, 10, 9, 0)),
        ];
        let s = run::<Cgroup>(&files, &after, false);
        assert!(s.details[0].ends_with("v1"), "{}", s.details[0]);
        assert_eq!(s.metrics["throttled_pct"], 10.0);
    }

    #[test]
    fn v1_mounted_at_own_cgroup() {
        let cpu = "/sys/fs/cgroup/cpu,cpuacct";
        let s = run::<Cgroup>(
            &v1_files(
                "4:cpu,cpuacct:/docker/abc\n9:memory:/docker/abc\n",
                cpu,
                "/sys/fs/cgroup/memory",
            ),
            &vec![f(
                &format!("{cpu}/cpu.stat"),
                "nr_periods 100\nnr_throttled 30\nthrottled_time 0\n",
            )],
            false,
        );
        assert_eq!(
            s.details[0],
            "cgroup /docker/abc (hierarchy mounted at own cgroup), v1"
        );
        assert_eq!(s.metrics["throttled_pct"], 30.0);
        assert!(
            s.summary.contains("memory 100 MiB of 200 MiB"),
            "{}",
            s.summary
        );
    }

    #[test]
    fn throttled_share_and_time() {
        let s = run::<Cgroup>(
            &vec![
                f(PROC_CGROUP, "0::/app\n"),
                f("/sys/fs/cgroup/app/cpu.stat", cpu_stat(0, 1000, 10, 5000)),
            ],
            &vec![f(
                "/sys/fs/cgroup/app/cpu.stat",
                cpu_stat(0, 1100, 44, 185_000),
            )],
            false,
        );
        assert_eq!(s.metrics["throttled_pct"], 34.0);
        assert_eq!(s.metrics["throttled_ms_per_sec"], 180.0);
        assert!(
            s.details
                .iter()
                .any(|d| d
                    == "cpu: no quota, 34 of 100 periods throttled (34.0%), 180 ms/s throttled (summed over CPUs)"),
            "{:?}",
            s.details
        );
    }

    #[test]
    fn no_quota_periods() {
        let s = app_throttle(0, 0);
        assert_eq!(s.metrics["throttled_pct"], 0.0);
        assert!(s.details.iter().any(|d| d == "cpu: no CPU quota active"));
        assert!(s.findings.is_empty());
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.summary, "no cpu quota");
    }

    /// The v1 legacy tree's own cgroup, with its counters moved on at t=1.
    #[test]
    fn legacy_tree_throttling_from_cpu_stat() {
        let cpu = "/sys/fs/cgroup/cpu,cpuacct/system.slice/app.service";
        let mem = "/sys/fs/cgroup/memory/system.slice/app.service";
        let legacy = FsSource::new(LEGACY);
        let mut files: Files = vec![f(PROC_CGROUP, legacy.read_to_string(PROC_CGROUP).unwrap())];
        for p in [
            format!("{cpu}/cpu.stat"),
            format!("{cpu}/cpu.cfs_quota_us"),
            format!("{cpu}/cpu.cfs_period_us"),
            format!("{mem}/memory.usage_in_bytes"),
            format!("{mem}/memory.limit_in_bytes"),
            format!("{mem}/memory.failcnt"),
        ] {
            files.push(f(&p, legacy.read_to_string(&p).unwrap()));
        }
        let s = run::<Cgroup>(
            &files,
            &vec![f(
                &format!("{cpu}/cpu.stat"),
                "nr_periods 864010\nnr_throttled 43204\nthrottled_time 912395678901\n",
            )],
            false,
        );
        assert_eq!(s.metrics["throttled_pct"], 40.0);
        assert_eq!(s.metrics["throttled_ms_per_sec"], 50.0);
        assert_eq!(s.status, Status::Crit);
        assert_eq!(
            s.summary,
            "cpu quota 1.5 (throttled 40% of periods, 50 ms/s), memory 1.5 GiB of 2 GiB, no memory limit hits"
        );
    }

    #[test]
    fn legacy_fixture_since_creation() {
        let src = FsSource::new(LEGACY);
        let mut c = Cgroup::default();
        c.sample(&src, 0.0);
        c.sample(&src, 1.0);
        let s = c.evaluate(&ctx(false));
        assert_eq!(s.status, Status::Ok);
        assert_eq!(
            s.summary,
            "cpu quota 1.5 (not throttled), memory 1.5 GiB of 2 GiB, no memory limit hits"
        );
        assert!(
            s.details
                .iter()
                .any(|d| d.starts_with("since creation: 5.0% of 864000 periods throttled")),
            "{:?}",
            s.details
        );
        assert!(s.details.iter().any(|d| d.contains("no CPU quota active")));
    }

    #[test]
    fn throttling_threshold_boundaries() {
        assert_eq!(app_throttle(100, 10).status, Status::Ok);
        let s = app_throttle(1000, 101);
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "CPU quota throttling: 10.1%"));
        assert!(has(
            &s,
            Level::Warn,
            "raise cpu.max / limits.cpu or reduce parallelism"
        ));
        assert_eq!(app_throttle(100, 25).status, Status::Warn);
        let s = app_throttle(1000, 251);
        assert_eq!(s.status, Status::Crit);
        assert!(has(&s, Level::Crit, "CPU quota throttling: 25.1%"));
    }

    #[test]
    fn oom_kill_is_crit() {
        let s = app_events(events(0, 0, 0), events(0, 0, 1));
        assert_eq!(s.status, Status::Crit);
        assert_eq!(s.metrics["oom_kills"], 1.0);
        assert!(has(&s, Level::Crit, "1 OOM kills in this cgroup"));
        assert!(s.summary.ends_with("1 OOM kills"), "{}", s.summary);
    }

    #[test]
    fn memory_max_is_warn() {
        let s = app_events(events(0, 5, 1), events(0, 7, 1));
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.metrics["memory_max_events"], 2.0);
        assert_eq!(s.metrics["oom_kills"], 0.0);
        assert!(has(&s, Level::Warn, "hit memory.max 2 times"));
        assert!(s.summary.ends_with("2 max events"), "{}", s.summary);
    }

    #[test]
    fn memory_high_is_note() {
        let s = app_events(events(1, 0, 0), events(4, 0, 0));
        assert_eq!(s.status, Status::Ok);
        assert!(has(&s, Level::Note, "throttled at memory.high 3 times"));
        assert!(s.summary.ends_with("3 high events"), "{}", s.summary);
    }

    #[test]
    fn old_events_are_ok() {
        let s = app_events(events(0, 9, 2), events(0, 9, 2));
        assert_eq!(s.status, Status::Ok);
        assert!(s.summary.ends_with("no memory events"), "{}", s.summary);
        assert!(s.findings.is_empty());
    }

    #[test]
    fn v1_failcnt_is_warn() {
        let cpu = "/sys/fs/cgroup/cpu,cpuacct/app";
        let mem = "/sys/fs/cgroup/memory/app";
        let files = v1_files("4:cpu,cpuacct:/app\n10:memory:/app\n", cpu, mem);
        let s = run::<Cgroup>(
            &files,
            &vec![f(&format!("{mem}/memory.failcnt"), "4\n")],
            false,
        );
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.metrics["memory_max_events"], 1.0);
        assert!(has(&s, Level::Warn, "memory.failcnt"));
        assert!(s.summary.ends_with("1 memory limit hits"), "{}", s.summary);
        let s = run::<Cgroup>(&files, &vec![], false);
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn summary_of_throttled_container() {
        let s = run::<Cgroup>(
            &vec![
                f(PROC_CGROUP, "0::/\n"),
                f("/sys/fs/cgroup/cpu.stat", cpu_stat(5, 0, 0, 0)),
                f("/sys/fs/cgroup/cpu.max", "150000 100000\n"),
                f("/sys/fs/cgroup/memory.current", (410 * MIB).to_string()),
                f("/sys/fs/cgroup/memory.max", (512 * MIB).to_string()),
                f("/sys/fs/cgroup/memory.high", "max\n"),
                f("/sys/fs/cgroup/memory.events", events(0, 0, 0)),
            ],
            &vec![f(
                "/sys/fs/cgroup/cpu.stat",
                cpu_stat(900_000, 100, 34, 180_000),
            )],
            true,
        );
        assert_eq!(
            s.summary,
            "cpu quota 1.5 (throttled 34% of periods, 180 ms/s), memory 410 MiB of 512 MiB, no memory events"
        );
        assert_eq!(s.status, Status::Crit);
        for k in [
            "throttled_pct",
            "throttled_ms_per_sec",
            "memory_max_events",
            "oom_kills",
        ] {
            assert!(s.metrics.contains_key(k), "{k}");
        }
        assert!(
            s.details
                .iter()
                .any(|d| d == "memory: 410 MiB of 512 MiB (80%)")
        );
    }

    #[test]
    fn skipped_without_cgroup_files() {
        let s = run::<Cgroup>(&vec![], &vec![], false);
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains(PROC_CGROUP), "{}", s.summary);
        let s = run::<Cgroup>(&vec![f(PROC_CGROUP, "0::/app\n")], &vec![], false);
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains("/sys/fs/cgroup/app"), "{}", s.summary);
    }

    // --- top cgroups ----------------------------------------------------------------------

    /// v2 cgroup `dir` (relative, `""` = root) with its cpu.stat and optional memory.current.
    fn cg(dir: &str, usage: u64, periods: u64, throttled: u64, mem: Option<u64>) -> Files {
        let mut v = vec![f(
            &format!("{ROOT}{dir}/cpu.stat"),
            cpu_stat(usage, periods, throttled, 0),
        )];
        if let Some(m) = mem {
            v.push(f(&format!("{ROOT}{dir}/memory.current"), m.to_string()));
        }
        v
    }

    /// A v2 tree: root plus `(dir, Δusage µs, Δperiods, Δthrottled, memory)` per cgroup.
    fn top_tree(cgroups: &[(&str, u64, u64, u64, Option<u64>)], container: bool) -> Section {
        let mut before = vec![f(&format!("{ROOT}/cgroup.controllers"), "cpu memory\n")];
        before.extend(cg("", 0, 0, 0, None));
        let mut after = Vec::new();
        for (dir, usage, periods, throttled, mem) in cgroups {
            before.extend(cg(dir, 1000, 0, 0, *mem));
            after.extend(cg(dir, 1000 + usage, *periods, *throttled, *mem));
        }
        run::<CgroupsTop>(&before, &after, container)
    }

    fn listed(s: &Section, path: &str) -> bool {
        s.details.iter().any(|d| d.ends_with(&format!("  {path}")))
    }

    #[test]
    fn top_lists_leaves_only() {
        let s = top_tree(
            &[
                ("/system.slice", 2_000_000, 0, 0, Some(300 * MIB)),
                ("/system.slice/a.service", 1_800_000, 0, 0, Some(200 * MIB)),
                ("/system.slice/b.service", 100_000, 0, 0, Some(100 * MIB)),
            ],
            false,
        );
        assert_eq!(s.status, Status::Ok);
        assert_eq!(
            s.details[0],
            "top cpu (2 leaf cgroups of 4, 100% = one CPU):"
        );
        assert_eq!(
            s.details[1],
            " 180.0% cpu  200 MiB  /system.slice/a.service"
        );
        assert_eq!(
            s.details[2],
            "  10.0% cpu  100 MiB  /system.slice/b.service"
        );
        assert!(!listed(&s, "/system.slice") && !listed(&s, "/"));
        assert_eq!(s.metrics["top_cpu_pct"], 180.0);
        assert_eq!(s.metrics["cgroups"], 4.0);
        // Same set by memory: no second list.
        assert!(!s.details.iter().any(|d| d == "top memory:"));
    }

    #[test]
    fn top_host_summary() {
        let s = top_tree(
            &[
                ("/system.slice", 0, 0, 0, None),
                ("/system.slice/a.service", 1_800_000, 0, 0, Some(100 * MIB)),
                ("/user.slice", 0, 0, 0, Some(2_254_857_830)),
            ],
            false,
        );
        assert_eq!(
            s.summary,
            "4 cgroups, top cpu: /system.slice/a.service 180%, top memory: /user.slice 2.1 GiB"
        );
    }

    #[test]
    fn top_memory_list_when_different() {
        let s = top_tree(
            &[
                ("/c1", 600_000, 0, 0, Some(MIB)),
                ("/c2", 500_000, 0, 0, Some(MIB)),
                ("/c3", 400_000, 0, 0, Some(MIB)),
                ("/c4", 300_000, 0, 0, Some(MIB)),
                ("/c5", 200_000, 0, 0, Some(MIB)),
                ("/c6", 0, 0, 0, Some(900 * MIB)),
            ],
            false,
        );
        let i = s.details.iter().position(|d| d == "top memory:").unwrap();
        assert_eq!(s.details[i + 1], "   0.0% cpu  900 MiB  /c6");
        assert_eq!(s.details.len(), i + 1 + 5);
    }

    #[test]
    fn top_shortens_long_paths() {
        let long = format!(
            "/kubepods.slice/{}/cri-containerd-abc.scope",
            "x".repeat(80)
        );
        let short = shorten(&long, PATH_WIDTH);
        assert_eq!(short.chars().count(), PATH_WIDTH);
        assert!(short.starts_with("/kubepods.slice/"), "{short}");
        assert!(short.ends_with("/cri-containerd-abc.scope"), "{short}");
        assert!(short.contains('…'));
        assert_eq!(shorten("/a", PATH_WIDTH), "/a");
        let s = top_tree(&[(&long, 100_000, 0, 0, None)], false);
        assert!(
            s.details[1].ends_with(&format!("       -  {short}")),
            "{}",
            s.details[1]
        );
    }

    #[test]
    fn top_throttle_boundaries() {
        let s = top_tree(&[("/a", 0, 100, 25, None)], false);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.metrics["throttled_cgroups"], 0.0);
        let s = top_tree(&[("/a", 0, 1000, 251, None)], false);
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.metrics["throttled_cgroups"], 1.0);
        assert!(has(&s, Level::Warn, ": /a 25%"), "{:?}", s.findings);
    }

    #[test]
    fn top_names_three_throttled() {
        let s = top_tree(
            &[
                ("/a", 0, 100, 30, None),
                ("/b", 0, 100, 70, None),
                ("/c", 0, 100, 40, None),
                ("/d", 0, 100, 60, None),
                ("/e", 0, 100, 50, None),
                ("/f", 0, 100, 10, None),
            ],
            false,
        );
        assert_eq!(s.metrics["throttled_cgroups"], 5.0);
        assert_eq!(s.findings.len(), 1);
        assert!(
            s.findings[0]
                .message
                .contains("/b 70%, /d 60%, /e 50% +2 more"),
            "{}",
            s.findings[0].message
        );
    }

    #[test]
    fn top_container_namespace() {
        let s = top_tree(&[], true);
        assert_eq!(s.summary, "1 cgroup visible (container cgroup namespace)");
        assert_eq!(s.metrics["cgroups"], 1.0);
        assert!(listed(&s, "/"));
        // On a host the lone root is just a count.
        assert!(
            top_tree(&[], false)
                .summary
                .starts_with("1 cgroup, top cpu: / ")
        );
    }

    #[test]
    fn top_depth_limit() {
        let chain = [
            "/a",
            "/a/b",
            "/a/b/c",
            "/a/b/c/d",
            "/a/b/c/d/e",
            "/a/b/c/d/e/f",
        ];
        let mut cgroups: Vec<(&str, u64, u64, u64, Option<u64>)> =
            chain.iter().map(|d| (*d, 0, 0, 0, None)).collect();
        cgroups.push(("/a/b/c/d/e/f/g", 0, 0, 0, None));
        let s = top_tree(&cgroups, false);
        assert_eq!(s.metrics["cgroups"], 7.0);
        assert!(listed(&s, "/a/b/c/d/e/f"));
        assert!(!listed(&s, "/a/b/c/d/e/f/g"));
    }

    #[test]
    fn top_cap_at_2000() {
        let names: Vec<String> = (0..2001).map(|i| format!("/c{i:04}")).collect();
        let cgroups: Vec<(&str, u64, u64, u64, Option<u64>)> =
            names.iter().map(|n| (n.as_str(), 0, 0, 0, None)).collect();
        let s = top_tree(&cgroups, false);
        assert_eq!(s.metrics["cgroups"], 2000.0);
        assert!(
            s.details
                .iter()
                .any(|d| d == "walk stopped at 2000 cgroups")
        );
    }

    #[test]
    fn top_v1_paths() {
        let cpu = "/sys/fs/cgroup/cpu,cpuacct/system.slice/app.service";
        let mem = "/sys/fs/cgroup/memory/system.slice/app.service/memory.usage_in_bytes";
        let stat = "nr_periods 0\nnr_throttled 0\nthrottled_time 0\n";
        let s = run::<CgroupsTop>(
            &vec![
                f(&format!("{cpu}/cpu.stat"), stat),
                f(&format!("{cpu}/cpuacct.usage"), "1000000000\n"),
                f(mem, (300 * MIB).to_string()),
            ],
            &vec![f(&format!("{cpu}/cpuacct.usage"), "1500000000\n")],
            false,
        );
        assert_eq!(
            s.summary,
            "1 cgroup, top cpu: /system.slice/app.service 50%, top memory: /system.slice/app.service 300 MiB"
        );
        assert_eq!(
            s.details[1],
            "  50.0% cpu  300 MiB  /system.slice/app.service"
        );

        // The legacy fixture tree renders the same way.
        let src = FsSource::new(LEGACY);
        let mut c = CgroupsTop::default();
        c.sample(&src, 0.0);
        c.sample(&src, 1.0);
        let s = c.evaluate(&ctx(false));
        assert_eq!(
            s.summary,
            "1 cgroup, top cpu: /system.slice/app.service 0%, top memory: /system.slice/app.service 1.5 GiB"
        );
    }

    #[test]
    fn top_skipped_without_hierarchy() {
        let s = run::<CgroupsTop>(&vec![], &vec![], false);
        assert_eq!(s.status, Status::Skipped);
        assert_eq!(s.summary, "no cgroup hierarchy at /sys/fs/cgroup");
    }

    #[test]
    fn interface_files_are_not_walked() {
        for name in [
            "cgroup.procs",
            "cpu.stat",
            "memory.current",
            "io.pressure",
            "tasks",
        ] {
            assert!(!maybe_cgroup_dir(name), "{name}");
        }
        for name in ["system.slice", "docker-abc.scope", "kubepods", "init.scope"] {
            assert!(maybe_cgroup_dir(name), "{name}");
        }
    }
}
