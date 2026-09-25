//! `biolatency -D` (BCC): block I/O latency from issue to completion, as a log2 histogram per
//! disk. Shows the outliers that the average await of `iostat` hides.

use std::collections::BTreeMap;

use super::hist::{Hist, summary_us, us};
#[cfg(feature = "deep")]
use crate::check::{Check, Context};
use crate::check::{Resource, Section};
use crate::source::Source;

/// (warn, crit) p99 limits in µs for non-rotational (or unknown) and rotational disks.
pub const P99_SSD_US: (f64, f64) = (20_000.0, 100_000.0);
pub const P99_HDD_US: (f64, f64) = (100_000.0, 500_000.0);
const MAX_DISKS: usize = 8;
const HIST_WIDTH: usize = 30;
const SYS_BLOCK: &str = "/sys/block";
pub const OSRELEASE: &str = "/proc/sys/kernel/osrelease";

/// `DISK_NAME_LEN` in the kernel.
pub const DISK_NAME_LEN: usize = 32;

/// Map key of the eBPF `HIST` map; same `#[repr(C)]` layout as in
/// `perf60-ebpf/src/bin/biolatency.rs` (36 bytes, no padding).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskBucket {
    pub name: [u8; DISK_NAME_LEN],
    pub bucket: u32,
}

// SAFETY: plain `#[repr(C)]` data without padding or pointers.
#[cfg(feature = "deep")]
unsafe impl aya::Pod for DiskBucket {}

/// Index of the `struct request *` argument of `block_rq_issue` for a kernel release such as
/// `6.10.14-linuxkit`: 0 on 5.11 and newer, 1 before (`TP_PROTO(q, rq)`). Unparseable
/// releases are taken as current kernels.
pub fn rq_issue_arg(release: &str) -> u64 {
    let mut parts = release.trim().split('.');
    let num = |p: Option<&str>| -> Option<u32> {
        let p = p?;
        let end = p.find(|c: char| !c.is_ascii_digit()).unwrap_or(p.len());
        p[..end].parse().ok()
    };
    match (num(parts.next()), num(parts.next())) {
        (Some(major), Some(minor)) if (major, minor) < (5, 11) => 1,
        _ => 0,
    }
}

/// How the probe finds a request's disk name, as BTF byte offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskOffsets {
    /// `rq->q->disk` (true) or `rq->rq_disk` (false).
    pub via_queue: bool,
    /// Offset of `request.q` or `request.rq_disk`.
    pub rq: u32,
    /// Offset of `request_queue.disk` (via queue only).
    pub queue_disk: u32,
    /// Offset of `gendisk.disk_name`.
    pub disk_name: u32,
}

impl DiskOffsets {
    /// The eBPF program's globals, plus `RQ_ARG`.
    pub fn globals(&self, rq_arg: u64) -> [(&'static str, u64); 5] {
        [
            ("RQ_ARG", rq_arg),
            ("DISK_VIA_QUEUE", self.via_queue as u64),
            ("RQ_OFF", self.rq as u64),
            ("QUEUE_DISK_OFF", self.queue_disk as u64),
            ("DISK_NAME_OFF", self.disk_name as u64),
        ]
    }
}

/// Choose the disk path from BTF member offsets (`member(struct, member)`): `request.q` →
/// `request_queue.disk` on current kernels, else `request.rq_disk`; then `gendisk.disk_name`.
pub fn disk_offsets(member: impl Fn(&str, &str) -> Option<u32>) -> Result<DiskOffsets, String> {
    let disk_name = member("gendisk", "disk_name")
        .ok_or_else(|| "kernel BTF has no gendisk.disk_name".to_owned())?;
    if let (Some(rq), Some(queue_disk)) = (member("request", "q"), member("request_queue", "disk"))
    {
        return Ok(DiskOffsets {
            via_queue: true,
            rq,
            queue_disk,
            disk_name,
        });
    }
    match member("request", "rq_disk") {
        Some(rq) => Ok(DiskOffsets {
            via_queue: false,
            rq,
            queue_disk: 0,
            disk_name,
        }),
        None => {
            Err("kernel BTF has neither request.q + request_queue.disk nor request.rq_disk".into())
        }
    }
}

/// `queue/rotational` of every disk in `/sys/block` (disks without a readable value are left
/// out, i.e. unknown).
pub fn rotational(src: &dyn Source) -> BTreeMap<String, bool> {
    let names = src.read_dir(SYS_BLOCK).unwrap_or_default();
    names
        .into_iter()
        .filter_map(|name| {
            let r = src
                .read_to_string(&format!("{SYS_BLOCK}/{name}/queue/rotational"))
                .ok()?;
            match r.trim() {
                "1" => Some((name, true)),
                "0" => Some((name, false)),
                _ => None,
            }
        })
        .collect()
}

struct Disk {
    name: String,
    hist: Hist,
    p99: u64,
    rotational: Option<bool>,
}

/// Build the section from the window's per-disk histograms (µs). `rotational(disk)` is
/// `/sys/block/<disk>/queue/rotational`, `None` when unknown.
pub fn evaluate(
    mut s: Section,
    per_disk: Vec<(String, Hist)>,
    rotational: impl Fn(&str) -> Option<bool>,
    secs: f64,
) -> Section {
    let mut disks: Vec<Disk> = per_disk
        .into_iter()
        .filter_map(|(name, hist)| {
            let p99 = hist.percentile(99.0)?;
            let rotational = rotational(&name);
            Some(Disk {
                name,
                hist,
                p99,
                rotational,
            })
        })
        .collect();
    disks.sort_by(|a, b| {
        b.p99
            .cmp(&a.p99)
            .then(b.hist.count().cmp(&a.hist.count()))
            .then(a.name.cmp(&b.name))
    });
    s.metric("max_p99_us", disks.first().map_or(0, |d| d.p99) as f64);
    let Some(worst) = disks.first() else {
        s.summary("no block I/O");
        return s;
    };
    let plural = if disks.len() == 1 { "disk" } else { "disks" };
    s.summary(format!(
        "{} {}, {} {plural}",
        worst.name,
        summary_us(&worst.hist, "I/Os"),
        disks.len()
    ));

    for (i, d) in disks.iter().take(MAX_DISKS).enumerate() {
        let rate = if secs > 0.0 {
            format!("  {:.0} I/O/s", d.hist.count() as f64 / secs)
        } else {
            String::new()
        };
        s.detail(format!(
            "{:<8} {}{rate}  {}",
            d.name,
            summary_us(&d.hist, "I/Os"),
            kind(d.rotational)
        ));
        if i == 0 {
            for line in d.hist.lines_us(HIST_WIDTH) {
                s.detail(line);
            }
        }
    }
    if disks.len() > MAX_DISKS {
        s.detail(format!("{} more disks", disks.len() - MAX_DISKS));
    }

    for d in &disks {
        let h = &d.hist;
        s.metric(
            format!("{}.p50_us", d.name),
            h.percentile(50.0).unwrap_or(0) as f64,
        );
        s.metric(format!("{}.p99_us", d.name), d.p99 as f64);
        s.metric(format!("{}.max_us", d.name), h.max().unwrap_or(0) as f64);
        s.metric(format!("{}.ios", d.name), h.count() as f64);
    }

    for d in &disks {
        let (warn, crit) = if d.rotational == Some(true) {
            P99_HDD_US
        } else {
            P99_SSD_US
        };
        let p99 = d.p99 as f64;
        let limit = if p99 > crit { crit } else { warn };
        s.threshold(
            p99,
            warn,
            crit,
            format!(
                "I/O latency outliers on {}, p99 {}: above {} for a {} disk (p50 {}, max {}, \
                 {} I/Os); see the distribution and the disk section's await",
                d.name,
                us(d.p99),
                us(limit as u64),
                kind(d.rotational),
                us(d.hist.percentile(50.0).unwrap_or(0)),
                us(d.hist.max().unwrap_or(0)),
                d.hist.count()
            ),
        );
    }
    s
}

fn kind(rotational: Option<bool>) -> &'static str {
    match rotational {
        Some(true) => "rotational",
        Some(false) => "non-rotational",
        None => "unknown-type",
    }
}

pub fn section() -> Section {
    Section::new(
        "biolatency",
        "Block I/O latency (eBPF)",
        "biolatency (BCC)",
        Resource::Disk,
    )
}

#[cfg(feature = "deep")]
#[derive(Default)]
pub struct Biolatency {
    probe: Option<Result<super::probe::Probe, String>>,
    rotational: BTreeMap<String, bool>,
    window: super::Window,
}

#[cfg(feature = "deep")]
static OBJECT: &[u8] = aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/biolatency"));

#[cfg(feature = "deep")]
impl Biolatency {
    fn attach(release: &str) -> Result<super::probe::Probe, String> {
        // Privileges first, so an unprivileged run gets the needs-root reason.
        super::caps::require_bpf()?;
        // One BTF read for all lookups (`kernel_offsets` fails on the first missing member,
        // and the path is chosen by which members exist).
        const BTF: &str = "/sys/kernel/btf/vmlinux";
        let data = std::fs::read(BTF).map_err(|e| {
            format!("kernel BTF not available ({BTF}: {e}); needs CONFIG_DEBUG_INFO_BTF")
        })?;
        let btf = super::btf::Btf::parse(&data)?;
        let offsets = disk_offsets(|s, m| btf.member_offset(s, m))?;
        super::probe::Probe::attach_with(
            OBJECT,
            &[
                ("biolatency_issue", "block_rq_issue"),
                ("biolatency_complete", "block_rq_complete"),
            ],
            &offsets.globals(rq_issue_arg(release)),
        )
    }
}

#[cfg(feature = "deep")]
impl Check for Biolatency {
    fn id(&self) -> &'static str {
        "biolatency"
    }

    fn sample(&mut self, src: &dyn Source, t: f64) {
        if self.probe.is_none() {
            let release = src.read_to_string(OSRELEASE).unwrap_or_default();
            self.probe = Some(Self::attach(&release));
            self.rotational = rotational(src);
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
        let per_disk = match probe.hash_map::<DiskBucket, u64>("HIST") {
            Ok(entries) => {
                let mut by_disk: BTreeMap<String, Hist> = BTreeMap::new();
                for (k, n) in entries {
                    by_disk
                        .entry(super::comm_str(&k.name))
                        .or_default()
                        .add(k.bucket as usize, n);
                }
                by_disk.into_iter().collect()
            }
            Err(e) => return s.skipped(e),
        };
        evaluate(
            s,
            per_disk,
            |d| self.rotational.get(d).copied(),
            self.window.secs(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::Status;
    use crate::source::MemSource;

    fn hist(buckets: &[(usize, u64)]) -> Hist {
        let mut h = Hist::default();
        for &(b, n) in buckets {
            h.add(b, n);
        }
        h
    }

    /// One disk whose p99 (and every I/O) is in `bucket`, i.e. p99 = 2^(bucket+1) µs.
    fn one(name: &str, bucket: usize, rot: Option<bool>) -> Section {
        evaluate(
            section(),
            vec![(name.to_owned(), hist(&[(bucket, 100)]))],
            |_| rot,
            2.0,
        )
    }

    #[test]
    fn worst_disk_summary() {
        let disks = vec![
            ("vdb".to_owned(), hist(&[(6, 100)])),
            ("vda".to_owned(), hist(&[(8, 5000), (12, 300), (14, 21)])),
        ];
        let s = evaluate(section(), disks, |_| Some(false), 2.0);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(
            s.summary,
            "vda p50 512µs p99 8.2ms max 32.8ms (5321 I/Os), 2 disks"
        );
        assert_eq!(s.metrics["vda.p50_us"], 512.0);
        assert_eq!(s.metrics["vda.p99_us"], 8192.0);
        assert_eq!(s.metrics["vda.max_us"], 32768.0);
        assert_eq!(s.metrics["vda.ios"], 5321.0);
        assert_eq!(s.metrics["vdb.p99_us"], 128.0);
        assert_eq!(s.metrics["max_p99_us"], 8192.0);
    }

    #[test]
    fn worst_by_p99() {
        let disks = vec![
            ("sda".to_owned(), hist(&[(10, 10_000)])),
            ("sdb".to_owned(), hist(&[(12, 50)])),
        ];
        let s = evaluate(section(), disks, |_| None, 1.0);
        assert!(s.summary.starts_with("sdb "), "{}", s.summary);
        let s = evaluate(
            section(),
            vec![("sda".into(), hist(&[(3, 1)]))],
            |_| None,
            1.0,
        );
        assert!(s.summary.ends_with("(1 I/Os), 1 disk"), "{}", s.summary);
    }

    #[test]
    fn empty() {
        let s = evaluate(section(), vec![], |_| None, 2.0);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.summary, "no block I/O");
        assert_eq!(s.metrics["max_p99_us"], 0.0);
        assert!(s.details.is_empty());
        // A disk without counts is no disk.
        let s = evaluate(
            section(),
            vec![("vda".into(), Hist::default())],
            |_| None,
            2.0,
        );
        assert_eq!(s.summary, "no block I/O");
    }

    #[test]
    fn ssd_thresholds() {
        let ssd = Some(false);
        assert_eq!(one("vda", 13, ssd).status, Status::Ok, "p99 16384µs");
        let s = one("vda", 14, ssd);
        assert_eq!(s.status, Status::Warn, "p99 32768µs");
        assert!(
            s.findings[0].message.starts_with(
                "I/O latency outliers on vda, p99 32.8ms: above 20ms for a non-rotational disk"
            ),
            "{}",
            s.findings[0].message
        );
        assert_eq!(one("vda", 15, ssd).status, Status::Warn, "p99 65536µs");
        let s = one("vda", 16, ssd);
        assert_eq!(s.status, Status::Crit, "p99 131072µs");
        assert!(s.findings[0].message.contains("above 100ms"));
    }

    #[test]
    fn hdd_thresholds() {
        let hdd = Some(true);
        assert_eq!(one("sda", 15, hdd).status, Status::Ok, "p99 65536µs");
        let s = one("sda", 16, hdd);
        assert_eq!(s.status, Status::Warn, "p99 131072µs");
        assert!(
            s.findings[0]
                .message
                .contains("above 100ms for a rotational disk")
        );
        assert_eq!(one("sda", 17, hdd).status, Status::Warn, "p99 262144µs");
        let s = one("sda", 18, hdd);
        assert_eq!(s.status, Status::Crit, "p99 524288µs");
        assert!(s.findings[0].message.contains("above 500ms"));
    }

    #[test]
    fn unknown_type_uses_ssd_limits() {
        let s = one("xvda", 14, None);
        assert_eq!(s.status, Status::Warn);
        assert!(s.findings[0].message.contains("unknown-type disk"));
    }

    #[test]
    fn details_worst_disk_distribution_only() {
        let disks = vec![
            ("vdb".to_owned(), hist(&[(4, 10), (6, 10)])),
            ("vda".to_owned(), hist(&[(8, 90), (10, 10)])),
        ];
        let s = evaluate(section(), disks, |d| (d == "vda").then_some(false), 2.0);
        assert!(s.details[0].starts_with("vda "), "{:?}", s.details);
        assert!(
            s.details[0].ends_with("50 I/O/s  non-rotational"),
            "{}",
            s.details[0]
        );
        // Buckets 8..=10 of vda.
        assert!(s.details[1].contains("256µs -> 511µs"), "{}", s.details[1]);
        assert!(s.details[3].contains("1ms -> 2ms"), "{}", s.details[3]);
        assert!(s.details[4].starts_with("vdb "));
        assert!(s.details[4].ends_with("unknown-type"));
        assert_eq!(s.details.len(), 5);
    }

    #[test]
    fn many_disks() {
        let disks = (0..10)
            .map(|i| (format!("sd{i}"), hist(&[(i, 1)])))
            .collect();
        let s = evaluate(section(), disks, |_| None, 1.0);
        let lines: Vec<_> = s.details.iter().filter(|l| l.starts_with("sd")).collect();
        assert_eq!(lines.len(), 8);
        assert_eq!(s.details.last().unwrap(), "2 more disks");
        assert_eq!(s.metrics["sd0.ios"], 1.0, "metrics cover every disk");
    }

    #[test]
    fn rq_arg_by_release() {
        assert_eq!(rq_issue_arg("5.10.0-28-amd64"), 1);
        assert_eq!(rq_issue_arg("4.19.0"), 1);
        assert_eq!(rq_issue_arg("5.11.0"), 0);
        assert_eq!(rq_issue_arg("6.10.14-linuxkit\n"), 0);
        assert_eq!(rq_issue_arg("5.15"), 0);
        assert_eq!(rq_issue_arg(""), 0);
        assert_eq!(rq_issue_arg("garbage"), 0);
    }

    #[test]
    fn btf_path_choice() {
        let btf = |members: &'static [(&'static str, &'static str, u32)]| {
            move |s: &str, m: &str| {
                members
                    .iter()
                    .find(|(a, b, _)| *a == s && *b == m)
                    .map(|x| x.2)
            }
        };
        // Current kernels, also when rq_disk still exists (5.15/5.16).
        let o = disk_offsets(btf(&[
            ("request", "q", 0),
            ("request", "rq_disk", 160),
            ("request_queue", "disk", 88),
            ("gendisk", "disk_name", 12),
        ]))
        .unwrap();
        assert_eq!(
            o,
            DiskOffsets {
                via_queue: true,
                rq: 0,
                queue_disk: 88,
                disk_name: 12
            }
        );
        assert_eq!(o.globals(0)[1], ("DISK_VIA_QUEUE", 1));
        // Older kernels.
        let o = disk_offsets(btf(&[
            ("request", "q", 0),
            ("request", "rq_disk", 160),
            ("gendisk", "disk_name", 12),
        ]))
        .unwrap();
        assert!(!o.via_queue);
        assert_eq!(o.rq, 160);
        assert_eq!(o.globals(1)[0], ("RQ_ARG", 1));
        // Nothing usable.
        let e = disk_offsets(btf(&[("request", "q", 0), ("gendisk", "disk_name", 12)]));
        assert!(e.unwrap_err().contains("request.rq_disk"));
        let e = disk_offsets(btf(&[("request", "rq_disk", 160)]));
        assert!(e.unwrap_err().contains("gendisk.disk_name"));
    }

    #[test]
    fn key_layout() {
        assert_eq!(std::mem::size_of::<DiskBucket>(), 36);
        assert_eq!(std::mem::align_of::<DiskBucket>(), 4);
        assert_eq!(std::mem::offset_of!(DiskBucket, bucket), DISK_NAME_LEN);
    }

    #[test]
    fn rotational_from_sys_block() {
        let src = MemSource::new()
            .with("/sys/block/vda/queue/rotational", "0\n")
            .with("/sys/block/sda/queue/rotational", "1\n")
            .with("/sys/block/odd/queue/rotational", "x\n")
            .with("/sys/block/nometa/size", "1\n");
        let r = rotational(&src);
        assert_eq!(r.get("vda"), Some(&false));
        assert_eq!(r.get("sda"), Some(&true));
        assert_eq!(r.len(), 2);
        assert!(rotational(&MemSource::new()).is_empty());
    }
}
