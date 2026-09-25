//! `free -m` and the si/so columns of `vmstat 1`: available memory against RAM and the cgroup
//! limit, OOM kills, and swapping during the sampling window.

use crate::check::{Check, Context, SampleError, Section, rate};
use crate::procfs::meminfo::{self, MemInfo};
use crate::procfs::system;
use crate::procfs::vmstat::{self, VmStat};
use crate::source::Source;
use crate::units;

const MEMINFO: &str = "/proc/meminfo";
const VMSTAT: &str = "/proc/vmstat";
const CGROUP_V1: &str = "/sys/fs/cgroup/memory";

/// Available memory below these percentages of MemTotal is WARN / CRIT.
const AVAIL_WARN_PCT: f64 = 10.0;
const AVAIL_CRIT_PCT: f64 = 5.0;
/// cgroup working set above these percentages of the limit is WARN / CRIT.
const CGROUP_WARN_PCT: f64 = 90.0;
const CGROUP_CRIT_PCT: f64 = 95.0;
/// si + so above this many pages/s (about 1 MiB/s with 4 KiB pages) is CRIT; any is WARN.
const SWAP_CRIT_PAGES: f64 = 256.0;

/// `a` as a percentage of `b`; 0 when `b` is 0 (never NaN).
fn pct(a: u64, b: u64) -> f64 {
    if b == 0 {
        0.0
    } else {
        a as f64 * 100.0 / b as f64
    }
}

/// Percentages: one decimal below 10%, so a 9.9% reading doesn't print as "10%".
fn pct_str(p: f64) -> String {
    if p < 10.0 {
        let s = format!("{p:.1}");
        s.strip_suffix(".0").unwrap_or(&s).to_owned()
    } else {
        format!("{p:.0}")
    }
}

/// Rates: whole numbers as integers, small fractions with one decimal.
fn num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 || v >= 10.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

// ---------------------------------------------------------------------------------------------
// memory (free -m)

/// Memory accounting of the cgroup whose limit applies.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CgroupMem {
    usage: u64,
    limit: u64,
    inactive_file: u64,
}

impl CgroupMem {
    /// What the kernel can't easily reclaim: usage minus inactive page cache.
    fn working_set(&self) -> u64 {
        self.usage.saturating_sub(self.inactive_file)
    }
}

#[derive(Default)]
pub struct Memory {
    mem: Option<MemInfo>,
    cgroup: Option<CgroupMem>,
    /// `oom_kill` at the first and the last sample that had it.
    oom: Option<(u64, u64)>,
    error: SampleError,
}

impl Check for Memory {
    fn id(&self) -> &'static str {
        "memory"
    }

    fn sample(&mut self, src: &dyn Source, _t: f64) {
        if let Some(s) = self.error.read(src, MEMINFO) {
            match meminfo::parse(&s) {
                Ok(m) => {
                    self.cgroup = cgroup_mem(src, m.get("MemTotal"));
                    self.mem = Some(m);
                }
                Err(e) => self.error.record(MEMINFO, &std::io::Error::other(e.0)),
            }
        }
        // OOM kills are optional: older kernels have no counter, and vmstat may be unreadable.
        if let Some(n) = src
            .read_to_string(VMSTAT)
            .ok()
            .and_then(|s| vmstat::parse(&s).ok())
            .and_then(|v| v.get("oom_kill"))
        {
            let first = self.oom.map_or(n, |(f, _)| f);
            self.oom = Some((first, n));
        }
    }

    fn evaluate(&self, _ctx: &Context) -> Section {
        let s = Section::new("memory", "Memory", "free -m");
        let Some(m) = &self.mem else {
            return s.skipped(self.error.get().unwrap_or("no samples"));
        };
        evaluate_memory(s, m, self.cgroup, self.oom)
    }
}

fn evaluate_memory(
    mut s: Section,
    m: &MemInfo,
    cg: Option<CgroupMem>,
    oom: Option<(u64, u64)>,
) -> Section {
    let get = |k: &str| m.get(k).unwrap_or(0);
    let total = get("MemTotal");
    let free = get("MemFree");
    let buffers = get("Buffers");
    let cached = get("Cached");
    // MemAvailable appeared in 3.14; before that, free + buffers + page cache is the estimate.
    let available = m
        .get("MemAvailable")
        .unwrap_or_else(|| free + buffers + cached)
        .min(total);
    let avail_pct = pct(available, total);
    let (swap_total, swap_free) = (get("SwapTotal"), get("SwapFree"));
    let swap_used = swap_total.saturating_sub(swap_free);

    s.summary(format!(
        "available {} of {} ({}%), buffers {}, cached {}, swap {}/{}",
        units::bytes(available),
        units::bytes(total),
        pct_str(avail_pct),
        units::bytes(buffers),
        units::bytes(cached),
        units::bytes(swap_used),
        units::bytes(swap_total),
    ));
    // Same columns as free(1): buff/cache includes reclaimable slab.
    let buff_cache = buffers + cached + get("SReclaimable");
    let used = total
        .checked_sub(free + buff_cache)
        .unwrap_or_else(|| total.saturating_sub(free));
    s.detail(format!(
        "mem: total {}, used {}, free {}, shared {}, buff/cache {}, available {}",
        units::bytes(total),
        units::bytes(used),
        units::bytes(free),
        units::bytes(get("Shmem")),
        units::bytes(buff_cache),
        units::bytes(available),
    ));
    if swap_total > 0 {
        s.detail(format!(
            "swap: total {}, used {}, free {}",
            units::bytes(swap_total),
            units::bytes(swap_used),
            units::bytes(swap_free),
        ));
    }
    s.metric("total_bytes", total as f64);
    s.metric("available_bytes", available as f64);
    s.metric("available_pct", avail_pct);
    s.metric("cached_bytes", cached as f64);
    s.metric("swap_used_bytes", swap_used as f64);

    if total > 0 {
        let msg = format!(
            "only {} available ({}% of {}): low memory, expect reclaim, swapping or OOM kills",
            units::bytes(available),
            pct_str(avail_pct),
            units::bytes(total)
        );
        if avail_pct < AVAIL_CRIT_PCT {
            s.crit(msg);
        } else if avail_pct < AVAIL_WARN_PCT {
            s.warn(msg);
        }
    }

    if let Some(cg) = cg {
        let ws = cg.working_set();
        let used_pct = pct(ws, cg.limit);
        s.detail(format!(
            "cgroup: {} of {} ({}%)",
            units::bytes(ws),
            units::bytes(cg.limit),
            pct_str(used_pct)
        ));
        s.metric("cgroup_used_pct", used_pct);
        s.threshold(
            used_pct,
            CGROUP_WARN_PCT,
            CGROUP_CRIT_PCT,
            format!(
                "cgroup working set {} is {}% of its {} limit: close to the cgroup OOM killer",
                units::bytes(ws),
                pct_str(used_pct),
                units::bytes(cg.limit)
            ),
        );
    }

    if let Some((first, last)) = oom {
        s.metric("oom_kills", last as f64);
        let during = last.saturating_sub(first);
        if during > 0 {
            s.crit(format!(
                "{during} OOM kills during the sampling window ({last} since boot): the kernel is killing processes to free memory"
            ));
        } else if last > 0 {
            s.note(format!("{last} OOM kills since boot"));
        }
    }
    s
}

fn read_u64(src: &dyn Source, path: &str) -> Option<u64> {
    src.read_to_string(path)
        .ok()
        .and_then(|s| system::cgroup_mem_limit(&s))
}

/// First of `keys` present in a cgroup `memory.stat` (same `name value` format as vmstat).
fn stat_value(src: &dyn Source, path: &str, keys: &[&str]) -> Option<u64> {
    let stat = vmstat::parse(&src.read_to_string(path).ok()?).ok()?;
    keys.iter().find_map(|k| stat.get(k))
}

/// The cgroup memory limit that applies to us, with its usage. Limits at or above RAM (v1
/// reports "unlimited" as a huge number) are no limit at all.
fn cgroup_mem(src: &dyn Source, mem_total: Option<u64>) -> Option<CgroupMem> {
    let cg = cgroup_v2(src).or_else(|| cgroup_v1(src))?;
    match mem_total {
        _ if cg.limit == 0 => None,
        Some(t) if cg.limit >= t => None,
        _ => Some(cg),
    }
}

/// Walk from our own cgroup up to the root and take the tightest `memory.max`.
fn cgroup_v2(src: &dyn Source) -> Option<CgroupMem> {
    let own = src
        .read_to_string("/proc/self/cgroup")
        .ok()
        .and_then(|s| system::cgroup2_path(&s))
        .unwrap_or_else(|| "/".to_owned());
    let mut path = own.trim_end_matches('/').to_owned();
    let mut best: Option<(u64, String)> = None;
    loop {
        let dir = format!("/sys/fs/cgroup{path}");
        if let Some(limit) = read_u64(src, &format!("{dir}/memory.max"))
            && best.as_ref().is_none_or(|(b, _)| limit < *b)
        {
            best = Some((limit, dir));
        }
        match path.rfind('/') {
            Some(i) => path.truncate(i),
            None => break,
        }
    }
    let (limit, dir) = best?;
    Some(CgroupMem {
        usage: read_u64(src, &format!("{dir}/memory.current"))?,
        limit,
        inactive_file: stat_value(src, &format!("{dir}/memory.stat"), &["inactive_file"])
            .unwrap_or(0),
    })
}

fn cgroup_v1(src: &dyn Source) -> Option<CgroupMem> {
    Some(CgroupMem {
        limit: read_u64(src, &format!("{CGROUP_V1}/memory.limit_in_bytes"))?,
        usage: read_u64(src, &format!("{CGROUP_V1}/memory.usage_in_bytes"))?,
        inactive_file: stat_value(
            src,
            &format!("{CGROUP_V1}/memory.stat"),
            &["total_inactive_file", "inactive_file"],
        )
        .unwrap_or(0),
    })
}

// ---------------------------------------------------------------------------------------------
// swap (vmstat 1, si/so)

pub struct Swap {
    first: Option<(f64, VmStat)>,
    last: Option<(f64, VmStat)>,
    swap_total: Option<u64>,
    page_size: u64,
    error: SampleError,
}

impl Default for Swap {
    fn default() -> Self {
        Swap {
            first: None,
            last: None,
            swap_total: None,
            page_size: page_size(),
            error: SampleError::default(),
        }
    }
}

#[cfg(target_os = "linux")]
fn page_size() -> u64 {
    // SAFETY: sysconf has no preconditions.
    let p = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if p > 0 { p as u64 } else { 4096 }
}

#[cfg(not(target_os = "linux"))]
fn page_size() -> u64 {
    4096
}

impl Check for Swap {
    fn id(&self) -> &'static str {
        "swap"
    }

    fn sample(&mut self, src: &dyn Source, t: f64) {
        if let Some(s) = self.error.read(src, VMSTAT) {
            match vmstat::parse(&s) {
                Ok(v) => {
                    if self.first.is_none() {
                        self.first = Some((t, v.clone()));
                    }
                    self.last = Some((t, v));
                }
                Err(e) => self.error.record(VMSTAT, &std::io::Error::other(e.0)),
            }
        }
        if let Some(total) = src
            .read_to_string(MEMINFO)
            .ok()
            .and_then(|s| meminfo::parse(&s).ok())
            .and_then(|m| m.get("SwapTotal"))
        {
            self.swap_total = Some(total);
        }
    }

    fn evaluate(&self, _ctx: &Context) -> Section {
        let s = Section::new("swap", "Swapping", "vmstat 1 (si/so)");
        let (Some(first), Some(last)) = (&self.first, &self.last) else {
            return s.skipped(self.error.get().unwrap_or("no samples"));
        };
        evaluate_swap(s, first, last, self.swap_total, self.page_size)
    }
}

/// Per-second rate of a vmstat counter between two samples; 0 when the kernel lacks it.
fn counter_rate(a: &(f64, VmStat), b: &(f64, VmStat), f: impl Fn(&VmStat) -> Option<u64>) -> f64 {
    match (f(&a.1), f(&b.1)) {
        (Some(x), Some(y)) => rate((a.0, x), (b.0, y)),
        _ => 0.0,
    }
}

fn evaluate_swap(
    mut s: Section,
    first: &(f64, VmStat),
    last: &(f64, VmStat),
    swap_total: Option<u64>,
    page_size: u64,
) -> Section {
    let si = counter_rate(first, last, |v| v.get("pswpin"));
    let so = counter_rate(first, last, |v| v.get("pswpout"));
    let majflt = counter_rate(first, last, |v| v.get("pgmajfault"));
    let direct = counter_rate(first, last, VmStat::pgscan_direct);
    let page = page_size as f64;

    if swap_total == Some(0) && si + so == 0.0 {
        s.summary("no swap configured, no swapping");
    } else {
        s.summary(format!(
            "si {} so {} pages/s ({} in, {} out)",
            num(si),
            num(so),
            units::bytes_rate(si * page),
            units::bytes_rate(so * page),
        ));
    }
    s.detail(format!(
        "major faults {}/s, direct reclaim {} pages/s{}",
        num(majflt),
        num(direct),
        swap_total
            .filter(|t| *t > 0)
            .map(|t| format!(", swap size {}", units::bytes(t)))
            .unwrap_or_default()
    ));
    s.metric("si_pages_per_sec", si);
    s.metric("so_pages_per_sec", so);
    s.metric("majflt_per_sec", majflt);
    s.metric("pgscan_direct_per_sec", direct);

    s.threshold(
        si + so,
        0.0,
        SWAP_CRIT_PAGES,
        format!(
            "system is swapping: memory pressure (si {} so {} pages/s)",
            num(si),
            num(so)
        ),
    );
    if majflt > 0.0 {
        s.note(format!(
            "{} major faults/s: pages read from disk (page cache misses or swap-ins)",
            num(majflt)
        ));
    }
    if direct > 0.0 {
        s.note(format!(
            "direct reclaim scanning {} pages/s: allocations stall to free memory",
            num(direct)
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Level, Status};
    use crate::source::{FsSource, MemSource};
    use crate::sysinfo::SysInfo;

    const MIB: u64 = 1 << 20;

    fn ctx() -> Context {
        Context {
            sys: SysInfo::default(),
            interval: 1.0,
            count: 1,
        }
    }

    fn meminfo(total_kb: u64, avail_kb: u64) -> String {
        format!(
            "MemTotal: {total_kb} kB\nMemFree: {avail_kb} kB\nMemAvailable: {avail_kb} kB\n\
             Buffers: 0 kB\nCached: 0 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n"
        )
    }

    fn memory(src: &MemSource) -> Section {
        let mut c = Memory::default();
        c.sample(src, 0.0);
        c.sample(src, 1.0);
        c.evaluate(&ctx())
    }

    fn has(s: &Section, level: Level, needle: &str) -> bool {
        s.findings
            .iter()
            .any(|f| f.level == level && f.message.contains(needle))
    }

    #[test]
    fn summary_ok() {
        let src = MemSource::new().with(
            MEMINFO,
            include_str!("../../tests/fixtures/linux-arm64/proc/meminfo"),
        );
        let s = memory(&src);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(
            s.summary,
            "available 1.4 GiB of 1.9 GiB (74%), buffers 1.3 MiB, cached 440 MiB, swap 0 B/0 B"
        );
        assert!(s.details[0].contains("shared 1.1 MiB"), "{:?}", s.details);
        assert_eq!(s.metrics["total_bytes"], 1986320.0 * 1024.0);
        assert_eq!(s.metrics["swap_used_bytes"], 0.0);
        assert!(!s.metrics.contains_key("cgroup_used_pct"));
        assert!(!s.metrics.contains_key("oom_kills"));
    }

    #[test]
    fn old_kernel_without_memavailable() {
        let src = MemSource::new().with(
            MEMINFO,
            "MemTotal: 1000000 kB\nMemFree: 50000 kB\nBuffers: 10000 kB\nCached: 40000 kB\n",
        );
        let s = memory(&src);
        assert_eq!(s.metrics["available_bytes"], 100000.0 * 1024.0);
        assert_eq!(s.metrics["available_pct"], 10.0);
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn available_boundaries() {
        let at = |avail_kb| memory(&MemSource::new().with(MEMINFO, &meminfo(1000000, avail_kb)));
        assert_eq!(at(100000).status, Status::Ok);
        let s = at(99000);
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "9.9%"), "{:?}", s.findings);
        assert_eq!(at(50000).status, Status::Warn);
        assert_eq!(at(49000).status, Status::Crit);
    }

    fn cgroup_v2(limit: &str, current: u64, inactive: u64) -> MemSource {
        MemSource::new()
            .with(MEMINFO, &meminfo(4000000, 3000000))
            .with("/proc/self/cgroup", "0::/\n")
            .with("/sys/fs/cgroup/cgroup.controllers", "cpu memory\n")
            .with("/sys/fs/cgroup/memory.max", limit)
            .with("/sys/fs/cgroup/memory.current", &format!("{current}\n"))
            .with(
                "/sys/fs/cgroup/memory.stat",
                &format!("anon 1\nfile 2\ninactive_anon 0\ninactive_file {inactive}\n"),
            )
    }

    #[test]
    fn cgroup_v2_detail() {
        let s = memory(&cgroup_v2("536870912\n", 450 * MIB, 40 * MIB));
        assert!(
            s.details
                .iter()
                .any(|d| d == "cgroup: 410 MiB of 512 MiB (80%)"),
            "{:?}",
            s.details
        );
        assert_eq!(s.status, Status::Ok);
        assert!((s.metrics["cgroup_used_pct"] - 80.078125).abs() < 1e-9);
    }

    #[test]
    fn cgroup_v2_boundaries() {
        let limit = format!("{}\n", 1000 * MIB);
        let at = |mib| memory(&cgroup_v2(&limit, mib * MIB + 5 * MIB, 5 * MIB)).status;
        assert_eq!(at(900), Status::Ok);
        assert_eq!(at(910), Status::Warn);
        assert_eq!(at(950), Status::Warn);
        assert_eq!(at(960), Status::Crit);
    }

    #[test]
    fn cgroup_v2_no_limit() {
        let s = memory(&cgroup_v2("max\n", 450 * MIB, 0));
        assert!(!s.metrics.contains_key("cgroup_used_pct"));
        assert!(!s.details.iter().any(|d| d.starts_with("cgroup")));
    }

    #[test]
    fn cgroup_v2_nested_tightest() {
        let src = MemSource::new()
            .with(MEMINFO, &meminfo(4000000, 3000000))
            .with("/proc/self/cgroup", "0::/a/b\n")
            .with("/sys/fs/cgroup/cgroup.controllers", "memory\n")
            .with("/sys/fs/cgroup/a/memory.max", &format!("{}\n", 512 * MIB))
            .with(
                "/sys/fs/cgroup/a/memory.current",
                &format!("{}\n", 500 * MIB),
            )
            .with("/sys/fs/cgroup/a/b/memory.max", "max\n")
            .with("/sys/fs/cgroup/a/b/memory.current", &format!("{}\n", MIB));
        let s = memory(&src);
        assert!(
            s.details
                .iter()
                .any(|d| d == "cgroup: 500 MiB of 512 MiB (98%)"),
            "{:?}",
            s.details
        );
        assert_eq!(s.status, Status::Crit);
    }

    #[test]
    fn cgroup_v1_limit() {
        let src = MemSource::new()
            .with(MEMINFO, &meminfo(4000000, 3000000))
            .with(
                "/sys/fs/cgroup/memory/memory.limit_in_bytes",
                &format!("{}\n", 1000 * MIB),
            )
            .with(
                "/sys/fs/cgroup/memory/memory.usage_in_bytes",
                &format!("{}\n", 980 * MIB),
            )
            .with(
                "/sys/fs/cgroup/memory/memory.stat",
                "cache 0\ninactive_file 999999999\ntotal_inactive_file 0\n",
            );
        let s = memory(&src);
        assert_eq!(s.metrics["cgroup_used_pct"], 98.0);
        assert_eq!(s.status, Status::Crit);
    }

    #[test]
    fn cgroup_v1_unlimited_ignored() {
        let src = MemSource::new()
            .with(MEMINFO, &meminfo(1000000, 900000))
            .with(
                "/sys/fs/cgroup/memory/memory.limit_in_bytes",
                "9223372036854771712\n",
            )
            .with("/sys/fs/cgroup/memory/memory.usage_in_bytes", "1000\n");
        let s = memory(&src);
        assert!(!s.metrics.contains_key("cgroup_used_pct"));
        assert_eq!(s.status, Status::Ok);
    }

    fn oom_run(before: u64, after: u64) -> Section {
        let src = MemSource::new()
            .with(MEMINFO, &meminfo(1000000, 900000))
            .with(VMSTAT, &format!("pswpin 0\noom_kill {before}\n"));
        let mut c = Memory::default();
        c.sample(&src, 0.0);
        src.set(VMSTAT, &format!("pswpin 0\noom_kill {after}\n"));
        c.sample(&src, 1.0);
        c.evaluate(&ctx())
    }

    #[test]
    fn oom_kill_during_window_is_crit() {
        let s = oom_run(3, 4);
        assert_eq!(s.status, Status::Crit);
        assert!(
            has(&s, Level::Crit, "1 OOM kills during"),
            "{:?}",
            s.findings
        );
        assert_eq!(s.metrics["oom_kills"], 4.0);
    }

    #[test]
    fn oom_kills_since_boot_note() {
        let s = oom_run(2, 2);
        assert_eq!(s.status, Status::Ok);
        assert!(has(&s, Level::Note, "2 OOM kills since boot"));
        assert!(oom_run(0, 0).findings.is_empty());
    }

    #[test]
    fn no_oom_counter() {
        let src = MemSource::new()
            .with(MEMINFO, &meminfo(1000000, 900000))
            .with(VMSTAT, "pswpin 0\n");
        let s = memory(&src);
        assert!(!s.metrics.contains_key("oom_kills"));
        assert!(s.findings.is_empty());
    }

    #[test]
    fn missing_meminfo_is_skipped() {
        let s = memory(&MemSource::new().with(VMSTAT, "oom_kill 1\n"));
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains("/proc/meminfo"), "{}", s.summary);
    }

    // --- swap

    fn swap(t0: &str, t1: &str, swap_total_kb: Option<u64>) -> Section {
        let src = MemSource::new().with(VMSTAT, t0);
        if let Some(kb) = swap_total_kb {
            src.set(
                MEMINFO,
                &format!("MemTotal: 1000 kB\nSwapTotal: {kb} kB\nSwapFree: {kb} kB\n"),
            );
        }
        let mut c = Swap {
            page_size: 4096,
            ..Swap::default()
        };
        c.sample(&src, 0.0);
        src.set(VMSTAT, t1);
        c.sample(&src, 1.0);
        c.evaluate(&ctx())
    }

    fn counters(si: u64, so: u64) -> String {
        format!("pswpin {si}\npswpout {so}\npgmajfault 100\npgscan_direct 7\n")
    }

    #[test]
    fn swap_summary_ok() {
        let s = swap(&counters(5, 5), &counters(5, 5), Some(1024));
        assert_eq!(s.status, Status::Ok);
        assert!(s.summary.starts_with("si 0 so 0 pages/s"), "{}", s.summary);
        assert!(s.findings.is_empty());
        for k in [
            "si_pages_per_sec",
            "so_pages_per_sec",
            "majflt_per_sec",
            "pgscan_direct_per_sec",
        ] {
            assert_eq!(s.metrics[k], 0.0, "{k}");
        }
    }

    #[test]
    fn no_swap_configured() {
        let s = swap(&counters(0, 0), &counters(0, 0), Some(0));
        assert_eq!(s.summary, "no swap configured, no swapping");
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn swap_boundaries() {
        let base = counters(1000, 1000);
        assert_eq!(
            swap(&base, &counters(1000, 1000), Some(1)).status,
            Status::Ok
        );
        let s = swap(&base, &counters(1001, 1000), Some(1));
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "system is swapping: memory pressure"));
        assert_eq!(s.metrics["si_pages_per_sec"], 1.0);
        assert!(s.summary.contains("4 KiB/s in"), "{}", s.summary);
        assert_eq!(
            swap(&base, &counters(1128, 1128), Some(1)).status,
            Status::Warn
        );
        let s = swap(&base, &counters(1000, 1257), Some(1));
        assert_eq!(s.status, Status::Crit);
        assert_eq!(s.metrics["so_pages_per_sec"], 257.0);
    }

    #[test]
    fn majflt_note() {
        let s = swap(
            "pswpin 0\npswpout 0\npgmajfault 100\n",
            "pswpin 0\npswpout 0\npgmajfault 150\n",
            Some(0),
        );
        assert_eq!(s.status, Status::Ok);
        assert!(
            has(&s, Level::Note, "50 major faults/s"),
            "{:?}",
            s.findings
        );
        assert_eq!(s.metrics["majflt_per_sec"], 50.0);
    }

    #[test]
    fn direct_reclaim_per_zone_note() {
        let zones = |n, d| {
            format!(
                "pswpin 0\npswpout 0\npgscan_direct_normal {n}\npgscan_direct_dma32 {d}\n\
                 pgscan_direct_throttle 0\n"
            )
        };
        let s = swap(&zones(0, 0), &zones(30, 10), Some(0));
        assert_eq!(s.metrics["pgscan_direct_per_sec"], 40.0);
        assert!(has(&s, Level::Note, "direct reclaim"), "{:?}", s.findings);
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn missing_vmstat_is_skipped() {
        let mut c = Swap::default();
        c.sample(&MemSource::new().with(MEMINFO, "MemTotal: 1 kB\n"), 0.0);
        let s = c.evaluate(&ctx());
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains("/proc/vmstat"), "{}", s.summary);
    }

    #[test]
    fn fixture_tree_renders_without_nan() {
        let src = FsSource::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/linux-arm64"
        ));
        let mut m = Memory::default();
        let mut w = Swap::default();
        for t in [0.0, 1.0] {
            m.sample(&src, t);
            w.sample(&src, t);
        }
        for s in [m.evaluate(&ctx()), w.evaluate(&ctx())] {
            assert_eq!(s.status, Status::Ok, "{}: {}", s.id, s.summary);
            assert!(s.metrics.values().all(|v| v.is_finite()), "{:?}", s.metrics);
        }
        assert_eq!(
            w.evaluate(&ctx()).summary,
            "no swap configured, no swapping"
        );
    }
}
