//! `dmesg | tail`: recent kernel errors such as OOM kills, hung tasks, I/O errors and SYN floods.

use crate::check::{Check, Context, Resource, Section};
use crate::procfs::kmsg::{self, Record};
use crate::procfs::system;
use crate::source::{Source, describe_error};
use crate::units;

const KMSG: &str = "/dev/kmsg";
const UPTIME: &str = "/proc/uptime";
/// Only events at most this old escalate the status.
const RECENT_SECS: f64 = 3600.0;
const DETAIL_LINES: usize = 5;
const DETAIL_CHARS: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Severity {
    Warn,
    Crit,
}

/// A message matches when it contains one of `any` and, if `also` is non-empty, one of `also`.
/// Patterns are lowercase and must start at a word boundary.
struct Rule {
    label: &'static str,
    severity: Severity,
    any: &'static [&'static str],
    also: &'static [&'static str],
}

const fn rule(
    label: &'static str,
    severity: Severity,
    any: &'static [&'static str],
    also: &'static [&'static str],
) -> Rule {
    Rule {
        label,
        severity,
        any,
        also,
    }
}

use Severity::{Crit, Warn};

/// First matching rule wins. Rules sharing a label are reported together.
const RULES: &[Rule] = &[
    rule(
        "OOM kill",
        Crit,
        &["out of memory", "oom-kill", "killed process"],
        &[],
    ),
    rule(
        "hung task",
        Crit,
        &["blocked for more than", "hung_task"],
        &[],
    ),
    rule("I/O error", Crit, &["i/o error"], &[]),
    rule("filesystem error", Crit, &["ext4-fs error"], &[]),
    rule("filesystem error", Crit, &["xfs", "btrfs"], &["error"]),
    rule("kernel panic", Crit, &["kernel panic"], &[]),
    rule("kernel BUG/oops", Crit, &["bug:", "oops"], &[]),
    rule("CPU lockup", Crit, &["soft lockup", "hard lockup"], &[]),
    rule("hardware error", Crit, &["machine check", "mce:"], &[]),
    rule("memory failure", Crit, &["memory failure"], &[]),
    rule("SYN flood", Warn, &["syn flooding"], &[]),
    rule("segfault", Warn, &["segfault"], &[]),
    rule(
        "conntrack table full",
        Warn,
        &["nf_conntrack: table full"],
        &[],
    ),
    rule("link down", Warn, &["link is down"], &[]),
    rule("storage reset/timeout", Warn, &["task abort"], &[]),
    rule(
        "storage reset/timeout",
        Warn,
        &["nvme", "scsi"],
        &["reset", "timeout"],
    ),
    rule("call trace", Warn, &["call trace"], &[]),
];

/// The resource a kernel log event is about (the diagnosis groups findings by resource).
fn label_resource(label: &str) -> Resource {
    match label {
        "OOM kill" => Resource::Memory,
        "I/O error" | "filesystem error" | "storage reset/timeout" => Resource::Disk,
        "hardware error" | "memory failure" => Resource::Hardware,
        "SYN flood" | "conntrack table full" | "link down" => Resource::Network,
        _ => Resource::Kernel,
    }
}

/// `needle` occurs in `hay` at a position not preceded by an ASCII letter or digit.
fn has_word(hay: &str, needle: &str) -> bool {
    hay.match_indices(needle)
        .any(|(i, _)| i == 0 || !hay.as_bytes()[i - 1].is_ascii_alphanumeric())
}

fn classify(message: &str) -> Option<&'static Rule> {
    let m = message.to_ascii_lowercase();
    RULES.iter().find(|r| {
        r.any.iter().any(|p| has_word(&m, p))
            && (r.also.is_empty() || r.also.iter().any(|p| has_word(&m, p)))
    })
}

#[derive(Default)]
pub struct KernelLog {
    sampled: bool,
    records: Option<Vec<Record>>,
    uptime: Option<f64>,
    error: Option<String>,
}

impl Check for KernelLog {
    fn id(&self) -> &'static str {
        "kernel-log"
    }

    /// The log is not a rate: read it on the first sample only.
    fn sample(&mut self, src: &dyn Source, _t: f64) {
        if self.sampled {
            return;
        }
        self.sampled = true;
        match src.read_kmsg() {
            Ok(raw) => {
                // Live records may carry their continuation lines after a newline.
                let records = raw
                    .iter()
                    .flat_map(|r| r.lines())
                    .filter_map(|l| kmsg::parse_record(l).ok().flatten())
                    .collect();
                self.records = Some(records);
            }
            Err(e) => {
                let mut reason = describe_error(KMSG, &e);
                match e.kind() {
                    std::io::ErrorKind::PermissionDenied => {
                        reason.push_str(" (run as root or set kernel.dmesg_restrict=0)")
                    }
                    std::io::ErrorKind::NotFound => {
                        reason.push_str(" (run on the host, or use --privileged in a container)")
                    }
                    _ => {}
                }
                self.error = Some(reason);
            }
        }
        self.uptime = src
            .read_to_string(UPTIME)
            .ok()
            .and_then(|s| system::uptime_secs(&s));
    }

    fn evaluate(&self, _ctx: &Context) -> Section {
        let s = Section::new("kernel-log", "Kernel log", "dmesg | tail", Resource::Kernel);
        let Some(records) = &self.records else {
            return s.skipped(self.error.as_deref().unwrap_or("no samples"));
        };
        evaluate(s, records, self.uptime)
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// Matching events of one label, split into recent (≤ 1h) and older.
struct Group<'a> {
    label: &'static str,
    severity: Severity,
    recent: Vec<&'a Record>,
    old: Vec<&'a Record>,
}

fn evaluate(mut s: Section, records: &[Record], uptime: Option<f64>) -> Section {
    let now = uptime.unwrap_or_else(|| records.iter().map(Record::secs).fold(0.0, f64::max));
    let age = |r: &Record| (now - r.secs()).max(0.0);
    let ago = |r: &Record| units::duration(age(r));

    let errors: Vec<&Record> = records.iter().filter(|r| r.level <= 3).collect();
    let newest_error = errors.iter().max_by_key(|r| r.usec);
    s.summary(match newest_error {
        Some(r) => format!(
            "{} records, {} (prio ≤ 3), last error {} ago",
            records.len(),
            plural(errors.len(), "error"),
            ago(r)
        ),
        None => format!("{} records, no errors", records.len()),
    });

    let warnings: Vec<&Record> = records.iter().filter(|r| r.level <= 4).collect();
    for r in &warnings[warnings.len().saturating_sub(DETAIL_LINES)..] {
        s.detail(format!(
            "[-{}] {}",
            ago(r),
            kmsg::truncate(&r.message, DETAIL_CHARS)
        ));
    }

    let mut groups: Vec<Group> = Vec::new();
    for r in records {
        let Some(rule) = classify(&r.message) else {
            continue;
        };
        let i = match groups.iter().position(|g| g.label == rule.label) {
            Some(i) => i,
            None => {
                groups.push(Group {
                    label: rule.label,
                    severity: rule.severity,
                    recent: Vec::new(),
                    old: Vec::new(),
                });
                groups.len() - 1
            }
        };
        if age(r) <= RECENT_SECS {
            groups[i].recent.push(r);
        } else {
            groups[i].old.push(r);
        }
    }
    // Most severe first, then in rule order.
    groups.sort_by_key(|g| {
        (
            g.severity != Crit,
            RULES.iter().position(|r| r.label == g.label),
        )
    });

    let (mut recent_crit, mut recent_warn) = (0, 0);
    for g in &groups {
        if let Some(last) = g.recent.iter().max_by_key(|r| r.usec) {
            let msg = format!(
                "{}: {} in the last hour, last {} ago: {}",
                g.label,
                plural(g.recent.len(), "message"),
                ago(last),
                kmsg::truncate(&last.message, DETAIL_CHARS)
            );
            match g.severity {
                Crit => {
                    recent_crit += g.recent.len();
                    s.crit_on(label_resource(g.label), msg);
                }
                Warn => {
                    recent_warn += g.recent.len();
                    s.warn_on(label_resource(g.label), msg);
                }
            }
        }
    }
    for g in &groups {
        if let Some(last) = g.old.iter().max_by_key(|r| r.usec) {
            s.note(format!(
                "{}: {} older than 1h, most recent {} ago",
                g.label,
                plural(g.old.len(), "message"),
                ago(last)
            ));
        }
    }

    s.metric("records", records.len() as f64);
    s.metric("errors", errors.len() as f64);
    s.metric("recent_critical", recent_crit as f64);
    s.metric("recent_warning", recent_warn as f64);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Level, Status};
    use crate::source::{FsSource, MemSource};
    use crate::sysinfo::SysInfo;
    use std::io;

    fn ctx() -> Context {
        Context {
            sys: SysInfo::default(),
            interval: 1.0,
            count: 1,
        }
    }

    fn check(src: &dyn Source) -> Section {
        let mut c = KernelLog::default();
        c.sample(src, 0.0);
        c.sample(src, 1.0);
        c.evaluate(&ctx())
    }

    /// Kernel log at uptime `now` seconds, records as `(prio, seconds since boot, message)`.
    fn run(now: f64, records: &[(u32, f64, &str)]) -> Section {
        let kmsg: String = records
            .iter()
            .enumerate()
            .map(|(i, (p, t, m))| format!("{p},{i},{},-;{m}\n", (t * 1e6) as u64))
            .collect();
        let src = MemSource::new()
            .with(KMSG, &kmsg)
            .with(UPTIME, &format!("{now:.2} 1000.00\n"));
        check(&src)
    }

    fn has(s: &Section, level: Level, needle: &str) -> bool {
        s.findings
            .iter()
            .any(|f| f.level == level && f.message.contains(needle))
    }

    const OOM: &str = "Out of memory: Killed process 4242 (java) total-vm:8123456kB";

    #[test]
    fn recent_oom_kill_is_crit() {
        let s = run(10000.0, &[(6, 5.0, "Booting"), (3, 9867.0, OOM)]);
        assert_eq!(s.status, Status::Crit);
        assert!(has(&s, Level::Crit, "OOM kill"), "{:?}", s.findings);
        assert!(has(&s, Level::Crit, "2m13s ago"), "{:?}", s.findings);
        assert_eq!(s.metrics["recent_critical"], 1.0);
        assert_eq!(s.metrics["recent_warning"], 0.0);
    }

    #[test]
    fn old_oom_kill_is_note() {
        let s = run(20000.0, &[(3, 12620.0, OOM)]);
        assert_eq!(s.status, Status::Ok);
        assert!(
            has(
                &s,
                Level::Note,
                "OOM kill: 1 message older than 1h, most recent 2h03m ago"
            ),
            "{:?}",
            s.findings
        );
        assert_eq!(s.metrics["recent_critical"], 0.0);
    }

    #[test]
    fn one_hour_boundary() {
        assert_eq!(run(10000.0, &[(3, 6400.0, OOM)]).status, Status::Crit);
        let s = run(10000.0, &[(3, 6399.0, OOM)]);
        assert_eq!(s.status, Status::Ok);
        assert!(has(&s, Level::Note, "OOM kill"));
    }

    #[test]
    fn recent_syn_flood_is_warn() {
        let s = run(
            10000.0,
            &[(
                6,
                9940.0,
                "TCP: request_sock_TCP: Possible SYN flooding on port 80. Sending cookies.",
            )],
        );
        assert_eq!(s.status, Status::Warn);
        assert!(has(
            &s,
            Level::Warn,
            "SYN flood: 1 message in the last hour, last 1m00s ago"
        ));
        assert_eq!(s.metrics["recent_warning"], 1.0);
    }

    #[test]
    fn word_boundaries_avoid_false_matches() {
        let s = run(
            100.0,
            &[
                (7, 90.0, "usb: debug: loops done"),
                (
                    30,
                    95.0,
                    "systemd[1]: Listening on systemd-factory-reset.socket - Factory Reset Management.",
                ),
            ],
        );
        assert_eq!(s.status, Status::Ok);
        assert!(s.findings.is_empty(), "{:?}", s.findings);
    }

    #[test]
    fn classifies_every_category() {
        let crit = [
            "oom-kill:constraint=CONSTRAINT_MEMCG,task=java",
            "INFO: task kworker/0:1:123 blocked for more than 120 seconds.",
            "Buffer I/O error on dev sda1, logical block 0",
            "EXT4-fs error (device sda1): ext4_find_entry:1455: comm ls",
            "XFS (dm-0): Internal error xfs_trans_cancel at line 1005",
            "BTRFS error (device sdb): bdev /dev/sdb errs: wr 1",
            "Kernel panic - not syncing: Fatal exception",
            "BUG: unable to handle page fault for address: 0000000000001234",
            "Internal error: Oops: 0000000096000004 [#1] SMP",
            "watchdog: BUG: soft lockup - CPU#3 stuck for 22s!",
            "Watchdog detected hard LOCKUP on cpu 2",
            "mce: [Hardware Error]: Machine check events logged",
            "Memory failure: 0x12345: recovery action for dirty LRU page: Recovered",
        ];
        for m in crit {
            assert_eq!(classify(m).map(|r| r.severity), Some(Crit), "{m}");
        }
        let warn = [
            "app[311]: segfault at 0 ip 000055d0 sp 00007ffd error 4 in app",
            "nf_conntrack: nf_conntrack: table full, dropping packet",
            "e1000e: eth0 NIC Link is Down",
            "sd 0:0:0:0: [sda] tag#1 task abort called for scmd",
            "nvme nvme0: I/O 12 QID 3 timeout, reset controller",
            "scsi host0: resetting adapter",
            "Call trace:",
        ];
        for m in warn {
            assert_eq!(classify(m).map(|r| r.severity), Some(Warn), "{m}");
        }
        assert!(classify("podman0: port 3(veth2) entered forwarding state").is_none());
        assert!(classify("GPT: Use GNU Parted to correct GPT errors.").is_none());
    }

    #[test]
    fn clean_log_is_ok() {
        let s = run(
            100.0,
            &[
                (6, 0.0, "Booting Linux on physical CPU 0x0"),
                (6, 1.0, "KASLR enabled"),
            ],
        );
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.summary, "2 records, no errors");
        assert!(s.findings.is_empty());
        assert!(s.details.is_empty());
        assert_eq!(s.metrics["errors"], 0.0);
        assert_eq!(s.metrics["records"], 2.0);
    }

    #[test]
    fn errors_in_summary() {
        let s = run(
            20000.0,
            &[
                (6, 1.0, "hello"),
                (3, 12620.0, "virtio_net: something failed"),
            ],
        );
        assert!(
            s.summary
                .ends_with("2 records, 1 error (prio ≤ 3), last error 2h03m ago"),
            "{}",
            s.summary
        );
        assert_eq!(s.metrics["errors"], 1.0);
        assert_eq!(s.details, vec!["[-2h03m] virtio_net: something failed"]);
    }

    #[test]
    fn details_show_last_five_warnings() {
        let long = "x".repeat(300);
        let mut recs: Vec<(u32, f64, &str)> = vec![
            (4, 1.0, "w1"),
            (4, 2.0, "w2"),
            (6, 3.0, "info"),
            (4, 4.0, "w3"),
            (3, 5.0, "e4"),
            (4, 6.0, "w5"),
            (4, 7.0, "w6"),
        ];
        recs.push((4, 8.0, &long));
        let s = run(10.0, &recs);
        assert_eq!(s.details.len(), 5);
        assert_eq!(s.details[0], "[-0m06s] w3");
        assert_eq!(s.details[3], "[-0m03s] w6");
        for d in &s.details {
            assert!(d.starts_with("[-"), "{d}");
            let msg = d.split_once("] ").unwrap().1;
            assert!(msg.chars().count() <= DETAIL_CHARS, "{d}");
        }
        assert!(s.details[4].ends_with('…'));
    }

    #[test]
    fn continuation_lines_not_counted() {
        let src = MemSource::new()
            .with(
                KMSG,
                "6,1,1000,-;pci 0000:00:01.0: added\n SUBSYSTEM=pci\n DEVICE=+pci:0000:00:01.0\n6,2,2000,-;done\n",
            )
            .with(UPTIME, "10.00 20.00\n");
        let s = check(&src);
        assert_eq!(s.metrics["records"], 2.0);
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn missing_uptime_uses_newest_record() {
        let src = MemSource::new().with(KMSG, &format!("3,1,1000000,-;{OOM}\n6,2,5000000,-;x\n"));
        let s = check(&src);
        assert_eq!(s.status, Status::Crit);
        assert!(has(&s, Level::Crit, "0m04s ago"));
    }

    struct Denied;

    impl Source for Denied {
        fn read_to_string(&self, path: &str) -> io::Result<String> {
            Err(io::Error::new(io::ErrorKind::NotFound, path.to_owned()))
        }
        fn read_dir(&self, path: &str) -> io::Result<Vec<String>> {
            self.read_to_string(path).map(|_| Vec::new())
        }
        fn exists(&self, _: &str) -> bool {
            false
        }
        fn read_kmsg(&self) -> io::Result<Vec<String>> {
            Err(io::Error::from_raw_os_error(libc::EPERM))
        }
    }

    #[test]
    fn permission_denied_is_skipped_with_hint() {
        let s = check(&Denied);
        assert_eq!(s.status, Status::Skipped);
        assert_eq!(
            s.summary,
            "permission denied reading /dev/kmsg (run as root or set kernel.dmesg_restrict=0)"
        );
    }

    #[test]
    fn missing_kmsg_has_hint() {
        let s = check(&MemSource::new().with(UPTIME, "10.0 1.0"));
        assert_eq!(s.status, Status::Skipped);
        assert_eq!(
            s.summary,
            "/dev/kmsg not available (run on the host, or use --privileged in a container)"
        );
    }

    #[test]
    fn fixture_parses() {
        let root = format!("{}/tests/fixtures/linux-arm64", env!("CARGO_MANIFEST_DIR"));
        let s = check(&FsSource::new(root));
        assert_ne!(s.status, Status::Skipped, "{}", s.summary);
        let lines = include_str!("../../tests/fixtures/linux-arm64/dev/kmsg")
            .lines()
            .filter(|l| !l.starts_with(' '))
            .count();
        assert!(s.metrics["records"] > 0.0);
        assert_eq!(s.metrics["records"], lines as f64);
        assert!(
            s.summary.starts_with(&format!("{lines} records")),
            "{}",
            s.summary
        );
    }
}
