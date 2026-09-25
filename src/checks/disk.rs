//! `iostat -xz 1`: per-device IOPS, throughput, await, queue size and utilization.

use std::collections::{BTreeMap, BTreeSet};

use crate::check::{Check, Context, SampleError, Section};
use crate::procfs::diskstats::{self, DiskStat};
use crate::source::Source;

const PATH: &str = "/proc/diskstats";
const SYS_BLOCK: &str = "/sys/block";

/// Name prefixes of devices that are never interesting (virtual or removable).
const IGNORED: [&str; 5] = ["loop", "ram", "zram", "fd", "sr"];

const UTIL_WARN: f64 = 60.0;
const UTIL_CRIT: f64 = 90.0;
/// (warn, crit) await in ms for non-rotational (or unknown) and rotational devices.
const AWAIT_SSD: (f64, f64) = (10.0, 50.0);
const AWAIT_HDD: (f64, f64) = (50.0, 200.0);
const QUEUE_NOTE: f64 = 1.0;

struct Snapshot {
    t: f64,
    devs: BTreeMap<String, DiskStat>,
}

#[derive(Default)]
pub struct Disk {
    /// Whole devices listed in `/sys/block`: `None` until read, `Some(None)` if unavailable.
    sys_block: Option<Option<BTreeSet<String>>>,
    rotational: BTreeMap<String, Option<bool>>,
    first: Option<Snapshot>,
    last: Option<Snapshot>,
    /// Highest %util of any single interval, per device.
    peak: BTreeMap<String, f64>,
    error: SampleError,
}

impl Check for Disk {
    fn id(&self) -> &'static str {
        "disk"
    }

    fn sample(&mut self, src: &dyn Source, t: f64) {
        let Some(s) = self.error.read(src, PATH) else {
            return;
        };
        let stats = match diskstats::parse(&s) {
            Ok(d) => d,
            Err(e) => {
                self.error.record(PATH, &std::io::Error::other(e.0));
                return;
            }
        };
        let sys_block = self.sys_block.get_or_insert_with(|| {
            src.read_dir(SYS_BLOCK)
                .ok()
                .map(|names| names.into_iter().collect())
        });
        let devs: BTreeMap<String, DiskStat> = stats
            .into_iter()
            .filter(|d| is_whole_device(&d.name, sys_block.as_ref()))
            .map(|d| (d.name.clone(), d))
            .collect();
        for name in devs.keys() {
            self.rotational
                .entry(name.clone())
                .or_insert_with(|| rotational(src, name));
        }
        let snap = Snapshot { t, devs };
        if let Some(prev) = &self.last {
            let dt = snap.t - prev.t;
            for (name, cur) in &snap.devs {
                if let Some(old) = prev.devs.get(name)
                    && dt > 0.0
                {
                    let u = util(old, cur, dt);
                    let p = self.peak.entry(name.clone()).or_insert(0.0);
                    *p = p.max(u);
                }
            }
        }
        if self.first.is_none() {
            self.first = Some(Snapshot {
                t: snap.t,
                devs: snap.devs.clone(),
            });
        }
        self.last = Some(snap);
    }

    fn evaluate(&self, _ctx: &Context) -> Section {
        let s = Section::new("disk", "Disk I/O", "iostat -xz 1");
        let (Some(first), Some(last)) = (&self.first, &self.last) else {
            return s.skipped(self.error.get().unwrap_or("no samples"));
        };
        let dt = last.t - first.t;
        let n = last.devs.len();
        let active: Vec<Stats> = if dt > 0.0 {
            last.devs
                .iter()
                .filter_map(|(name, cur)| {
                    let old = first.devs.get(name)?;
                    Stats::between(name, old, cur, dt).map(|mut st| {
                        st.peak = self.peak.get(name).copied().unwrap_or(0.0).max(st.util);
                        st.rotational = self.rotational.get(name).copied().flatten();
                        st
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        evaluate(s, &active, n, dt > 0.0)
    }
}

/// Window statistics for one device, like one `iostat -x` row.
#[derive(Debug, Clone, Default)]
struct Stats {
    name: String,
    r_s: f64,
    w_s: f64,
    rkb_s: f64,
    wkb_s: f64,
    r_await: f64,
    w_await: f64,
    await_ms: f64,
    aqu: f64,
    util: f64,
    peak: f64,
    rotational: Option<bool>,
}

impl Stats {
    /// `None` when the device did no I/O in the window (`iostat -z`).
    fn between(name: &str, a: &DiskStat, b: &DiskStat, dt: f64) -> Option<Stats> {
        let d = |f: fn(&DiskStat) -> u64| f(b).saturating_sub(f(a)) as f64;
        let (reads, writes, ms_io) = (d(|x| x.reads), d(|x| x.writes), d(|x| x.ms_io));
        if reads == 0.0 && writes == 0.0 && ms_io == 0.0 {
            return None;
        }
        let (ms_r, ms_w) = (d(|x| x.ms_reading), d(|x| x.ms_writing));
        let per = |ms: f64, n: f64| if n > 0.0 { ms / n } else { 0.0 };
        Some(Stats {
            name: name.to_owned(),
            r_s: reads / dt,
            w_s: writes / dt,
            rkb_s: d(|x| x.sectors_read) * 512.0 / 1024.0 / dt,
            wkb_s: d(|x| x.sectors_written) * 512.0 / 1024.0 / dt,
            r_await: per(ms_r, reads),
            w_await: per(ms_w, writes),
            await_ms: per(ms_r + ms_w, reads + writes),
            aqu: d(|x| x.weighted_ms) / (dt * 1000.0),
            util: util(a, b, dt),
            ..Default::default()
        })
    }
}

/// %util between two snapshots `dt` seconds apart, capped at 100.
fn util(a: &DiskStat, b: &DiskStat, dt: f64) -> f64 {
    // ms / (dt * 1000) * 100, written so exact inputs give exact percentages.
    (b.ms_io.saturating_sub(a.ms_io) as f64 / (dt * 10.0)).min(100.0)
}

fn evaluate(mut s: Section, active: &[Stats], devices: usize, have_window: bool) -> Section {
    let max_util = active.iter().map(|d| d.util).fold(0.0, f64::max);
    s.metric("max_util_pct", max_util);
    let plural = if devices == 1 { "device" } else { "devices" };
    let busiest = active.iter().max_by(|a, b| {
        a.util
            .total_cmp(&b.util)
            .then(a.await_ms.total_cmp(&b.await_ms))
    });
    match busiest {
        Some(d) => s.summary(format!(
            "{} util {:.0}% (peak {:.0}%) r/s {} w/s {} await {:.1}ms aqu {:.1}",
            d.name,
            d.util,
            d.peak,
            rate(d.r_s),
            rate(d.w_s),
            d.await_ms,
            d.aqu
        )),
        None if !have_window => s.summary(format!(
            "need two samples to compute rates ({devices} {plural})"
        )),
        None => s.summary(format!("all disks idle ({devices} {plural})")),
    }

    for d in active {
        s.detail(format!(
            "{:<8} r/s {:.1}  w/s {:.1}  rkB/s {:.1}  wkB/s {:.1}  r_await {:.2}  w_await {:.2}  \
             aqu-sz {:.2}  %util {:.1} (peak {:.1})",
            d.name, d.r_s, d.w_s, d.rkb_s, d.wkb_s, d.r_await, d.w_await, d.aqu, d.util, d.peak
        ));
        s.metric(format!("{}.util_pct", d.name), d.util);
        s.metric(format!("{}.await_ms", d.name), d.await_ms);
        s.metric(format!("{}.r_per_sec", d.name), d.r_s);
        s.metric(format!("{}.w_per_sec", d.name), d.w_s);
        s.metric(format!("{}.aqu_sz", d.name), d.aqu);
    }

    for d in active {
        let msg = if d.util > UTIL_CRIT {
            format!(
                "{} saturated: %util {:.1}% (peak {:.0}%)",
                d.name, d.util, d.peak
            )
        } else {
            format!(
                "{} busy: %util {:.1}% (peak {:.0}%)",
                d.name, d.util, d.peak
            )
        };
        if s.threshold(d.util, UTIL_WARN, UTIL_CRIT, msg) && d.rotational != Some(true) {
            s.note(format!(
                "{}: %util can be misleading for RAID, NVMe and virtual disks that serve \
                 requests in parallel; judge by await and aqu-sz",
                d.name
            ));
        }

        let (kind, (warn, crit)) = match d.rotational {
            Some(true) => ("rotational", AWAIT_HDD),
            Some(false) => ("non-rotational", AWAIT_SSD),
            None => ("unknown-type", AWAIT_SSD),
        };
        let limit = if d.await_ms > crit { crit } else { warn };
        s.threshold(
            d.await_ms,
            warn,
            crit,
            format!(
                "{} await {:.1}ms exceeds {limit:.0}ms for a {kind} device (r_await {:.1}ms, \
                 w_await {:.1}ms)",
                d.name, d.await_ms, d.r_await, d.w_await
            ),
        );

        if d.aqu > QUEUE_NOTE {
            s.note(format!(
                "{} aqu-sz {:.1}: requests queueing (can be normal for devices that serve I/O \
                 in parallel)",
                d.name, d.aqu
            ));
        }
    }
    s
}

/// `10` for whole or large rates, `0.5` for small fractional ones.
fn rate(v: f64) -> String {
    if v >= 10.0 || v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

fn rotational(src: &dyn Source, name: &str) -> Option<bool> {
    let r = src
        .read_to_string(&format!("{SYS_BLOCK}/{name}/queue/rotational"))
        .ok()?;
    match r.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// Whole devices only: those listed in `/sys/block`, or, when it can't be listed, names that
/// don't look like partitions.
fn is_whole_device(name: &str, sys_block: Option<&BTreeSet<String>>) -> bool {
    if IGNORED.iter().any(|p| name.starts_with(p)) {
        return false;
    }
    match sys_block {
        Some(set) => set.contains(name),
        None => !is_partition(name),
    }
}

/// Partition names: sdXN, vdXN, xvdXN, hdXN, nvmeXnYpZ, mmcblkXpY.
fn is_partition(name: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    for prefix in ["sd", "vd", "xvd", "hd"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            let letters = rest.trim_end_matches(|c: char| c.is_ascii_digit());
            return !letters.is_empty()
                && letters.len() < rest.len()
                && letters.bytes().all(|b| b.is_ascii_lowercase());
        }
    }
    if let Some(rest) = name.strip_prefix("nvme") {
        return match rest.rsplit_once('p') {
            Some((disk, part)) => {
                digits(part)
                    && disk
                        .split_once('n')
                        .is_some_and(|(c, ns)| digits(c) && digits(ns))
            }
            None => false,
        };
    }
    if let Some(rest) = name.strip_prefix("mmcblk") {
        return rest
            .split_once('p')
            .is_some_and(|(disk, part)| digits(disk) && digits(part));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Level, Status};
    use crate::source::{FsSource, MemSource};
    use crate::sysinfo::SysInfo;

    fn ctx() -> Context {
        Context {
            sys: SysInfo::default(),
            interval: 1.0,
            count: 1,
        }
    }

    /// Counters for one device: reads, ms reading, writes, ms writing, ms doing I/O, weighted ms.
    #[derive(Clone, Copy, Default)]
    struct Io(u64, u64, u64, u64, u64, u64);

    fn line(name: &str, io: Io) -> String {
        let Io(r, ms_r, w, ms_w, ms_io, wt) = io;
        format!(
            " 253 0 {name} {r} 0 {} {ms_r} {w} 0 {} {ms_w} 0 {ms_io} {wt} 0 0 0 0 0 0\n",
            r * 8,
            w * 8
        )
    }

    fn diskstats(devs: &[(&str, Io)]) -> String {
        devs.iter().map(|(n, io)| line(n, *io)).collect()
    }

    /// A MemSource with `/sys/block/<dev>` entries for `sys` (name, rotational file content).
    fn source(sys: &[(&str, Option<&str>)]) -> MemSource {
        let src = MemSource::new();
        for (name, rot) in sys {
            src.set(&format!("/sys/block/{name}/size"), "2097152\n");
            if let Some(r) = rot {
                src.set(&format!("/sys/block/{name}/queue/rotational"), r);
            }
        }
        src
    }

    /// Sample `steps` one second apart and evaluate.
    fn run_steps(src: &MemSource, steps: &[Vec<(&str, Io)>]) -> Section {
        let mut c = Disk::default();
        for (i, devs) in steps.iter().enumerate() {
            src.set(PATH, &diskstats(devs));
            c.sample(src, i as f64);
        }
        c.evaluate(&ctx())
    }

    /// One device going from zero counters to `io` in 1 second.
    fn run_one(rot: Option<&str>, io: Io) -> Section {
        let src = source(&[("sda", rot)]);
        run_steps(&src, &[vec![("sda", Io::default())], vec![("sda", io)]])
    }

    fn has_note(s: &Section, needle: &str) -> bool {
        s.findings
            .iter()
            .any(|f| f.level == Level::Note && f.message.contains(needle))
    }

    #[test]
    fn busy_device_summary_and_metrics() {
        let src = source(&[("vda", Some("1"))]);
        let s = run_steps(
            &src,
            &[
                vec![("vda", Io::default())],
                vec![("vda", Io(10, 10, 900, 16370, 930, 3100))],
            ],
        );
        assert_eq!(
            s.summary,
            "vda util 93% (peak 93%) r/s 10 w/s 900 await 18.0ms aqu 3.1"
        );
        assert_eq!(s.metrics["vda.util_pct"], 93.0);
        assert_eq!(s.metrics["vda.r_per_sec"], 10.0);
        assert_eq!(s.metrics["vda.w_per_sec"], 900.0);
        assert_eq!(s.metrics["vda.await_ms"], 18.0);
        assert_eq!(s.metrics["vda.aqu_sz"], 3.1);
        assert_eq!(s.metrics["max_util_pct"], 93.0);
        assert_eq!(s.details.len(), 1);
        // 900 writes of 8 sectors = 3600 kB.
        assert!(s.details[0].contains("wkB/s 3600.0"), "{}", s.details[0]);
        assert_eq!(s.status, Status::Crit);
    }

    #[test]
    fn peak_interval_util() {
        let src = source(&[("vda", Some("1"))]);
        let s = run_steps(
            &src,
            &[
                vec![("vda", Io::default())],
                vec![("vda", Io(10, 10, 0, 0, 200, 200))],
                vec![("vda", Io(20, 20, 0, 0, 1100, 1100))],
            ],
        );
        assert_eq!(s.metrics["vda.util_pct"], 55.0);
        assert!(s.summary.contains("(peak 90%)"), "{}", s.summary);
        // Thresholds use the window average, not the peak.
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn idle_device_omitted() {
        let src = source(&[("vda", Some("0")), ("vdb", Some("0"))]);
        let idle = Io(5, 5, 5, 5, 5, 5);
        let s = run_steps(
            &src,
            &[
                vec![("vda", idle), ("vdb", Io::default())],
                vec![("vda", idle), ("vdb", Io(10, 10, 0, 0, 100, 10))],
            ],
        );
        assert!(s.summary.starts_with("vdb "), "{}", s.summary);
        assert!(!s.metrics.contains_key("vda.util_pct"));
        assert_eq!(s.metrics["vdb.util_pct"], 10.0);
        assert_eq!(s.details.len(), 1);
    }

    #[test]
    fn all_disks_idle() {
        let src = source(&[("vda", Some("0"))]);
        let io = Io(7, 7, 7, 7, 7, 7);
        let s = run_steps(&src, &[vec![("vda", io)], vec![("vda", io)]]);
        assert_eq!(s.summary, "all disks idle (1 device)");
        assert_eq!(s.metrics["max_util_pct"], 0.0);
        assert_eq!(s.status, Status::Ok);
        assert!(s.details.is_empty() && s.findings.is_empty());
    }

    #[test]
    fn fixture_with_identical_samples_is_idle() {
        let src = FsSource::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/linux-arm64"
        ));
        let mut c = Disk::default();
        c.sample(&src, 0.0);
        c.sample(&src, 0.001);
        let s = c.evaluate(&ctx());
        assert_eq!(s.summary, "all disks idle (1 device)");
        assert!(s.metrics.values().all(|v| v.is_finite()));
    }

    #[test]
    fn single_sample_has_no_rates() {
        let src = source(&[("vda", Some("0"))]).with(PATH, &line("vda", Io(1, 1, 1, 1, 1, 1)));
        let mut c = Disk::default();
        c.sample(&src, 0.0);
        let s = c.evaluate(&ctx());
        assert_eq!(s.status, Status::Ok);
        assert!(s.summary.contains("need two samples"), "{}", s.summary);
    }

    #[test]
    fn partitions_filtered_via_sys_block() {
        let src = source(&[("vda", Some("0"))]);
        let io = Io(10, 10, 10, 10, 100, 20);
        let s = run_steps(
            &src,
            &[
                vec![("vda", Io::default()), ("vda1", Io::default())],
                vec![("vda", io), ("vda1", io)],
            ],
        );
        assert!(s.metrics.contains_key("vda.util_pct"));
        assert!(!s.metrics.contains_key("vda1.util_pct"));
    }

    #[test]
    fn heuristic_without_sys_block() {
        let names = [
            "sda",
            "sda1",
            "nvme0n1",
            "nvme0n1p1",
            "mmcblk0",
            "mmcblk0p1",
            "dm-0",
            "md0",
            "loop0",
        ];
        let io = Io(10, 10, 10, 10, 100, 20);
        let s = run_steps(
            &MemSource::new(),
            &[
                names.iter().map(|n| (*n, Io::default())).collect(),
                names.iter().map(|n| (*n, io)).collect(),
            ],
        );
        let reported: Vec<&str> = s
            .metrics
            .keys()
            .filter_map(|k| k.strip_suffix(".util_pct"))
            .collect();
        assert_eq!(reported, ["dm-0", "md0", "mmcblk0", "nvme0n1", "sda"]);
    }

    #[test]
    fn partition_name_patterns() {
        for p in [
            "sda1",
            "sdab12",
            "vdb3",
            "xvda1",
            "hdb2",
            "nvme10n2p3",
            "mmcblk1p2",
        ] {
            assert!(is_partition(p), "{p}");
        }
        for d in [
            "sda", "vdb", "xvdf", "hda", "nvme0n1", "mmcblk0", "dm-3", "md127",
        ] {
            assert!(!is_partition(d), "{d}");
        }
        assert!(!is_whole_device("zram0", None));
        assert!(!is_whole_device("sr0", None));
    }

    #[test]
    fn util_boundaries() {
        // Rotational, so the only finding is the %util one.
        let at = |ms_io: u64| run_one(Some("1"), Io(100, 100, 0, 0, ms_io, 100));
        assert_eq!(at(600).status, Status::Ok);
        assert_eq!(at(600).metrics["sda.util_pct"], 60.0);
        assert_eq!(at(601).status, Status::Warn);
        let s = at(901);
        assert_eq!(s.status, Status::Crit);
        assert!(
            s.findings
                .iter()
                .any(|f| f.level == Level::Crit && f.message.contains("saturated"))
        );
        // %util is capped at 100 even if the counter overshoots.
        assert_eq!(at(1500).metrics["sda.util_pct"], 100.0);
    }

    #[test]
    fn util_note_only_for_non_rotational() {
        let io = Io(100, 100, 0, 0, 601, 100);
        let ssd = run_one(Some("0"), io);
        assert_eq!(ssd.status, Status::Warn);
        assert!(has_note(&ssd, "misleading"));
        assert!(!has_note(&run_one(Some("1"), io), "misleading"));
        // No note when %util is below the threshold.
        assert!(!has_note(
            &run_one(Some("0"), Io(100, 100, 0, 0, 600, 100)),
            "misleading"
        ));
    }

    #[test]
    fn ssd_await_boundaries() {
        let at = |ms: u64| run_one(Some("0"), Io(50, ms / 2, 50, ms - ms / 2, 100, 500));
        assert_eq!(at(1000).metrics["sda.await_ms"], 10.0);
        assert_eq!(at(1000).status, Status::Ok);
        assert_eq!(at(1001).status, Status::Warn);
        assert_eq!(at(5000).status, Status::Warn);
        assert_eq!(at(5001).status, Status::Crit);
    }

    #[test]
    fn hdd_await_boundaries() {
        let at = |ms: u64| run_one(Some("1"), Io(100, ms, 0, 0, 100, 500));
        assert_eq!(at(2000).status, Status::Ok);
        assert_eq!(at(5000).status, Status::Ok);
        assert_eq!(at(5010).status, Status::Warn);
        assert_eq!(at(20000).status, Status::Warn);
        assert_eq!(at(20010).status, Status::Crit);
    }

    #[test]
    fn unknown_type_uses_ssd_thresholds() {
        let s = run_one(None, Io(100, 2000, 0, 0, 100, 500));
        assert_eq!(s.status, Status::Warn);
        assert!(
            s.findings
                .iter()
                .any(|f| f.message.contains("unknown-type"))
        );
    }

    #[test]
    fn queue_note() {
        let s = run_one(Some("1"), Io(100, 100, 0, 0, 100, 1500));
        assert_eq!(s.metrics["sda.aqu_sz"], 1.5);
        assert!(has_note(&s, "requests queueing"));
        assert_eq!(s.status, Status::Ok);
        let s = run_one(Some("1"), Io(100, 100, 0, 0, 100, 1000));
        assert!(!has_note(&s, "requests queueing"));
    }

    #[test]
    fn missing_diskstats_is_skipped() {
        let mut c = Disk::default();
        c.sample(&MemSource::new(), 0.0);
        c.sample(&MemSource::new(), 1.0);
        let s = c.evaluate(&ctx());
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains("/proc/diskstats"), "{}", s.summary);
    }
}
