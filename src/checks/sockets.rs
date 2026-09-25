//! `ss -s` / `conntrack -S`: socket counts from sockstat and how close conntrack, the ephemeral
//! port range (TIME_WAIT), orphans and TCP memory are to their kernel limits.

use std::io;

use crate::check::{Check, Context, Resource, SampleError, Section};
use crate::procfs::sockstat::{self, Sockstat};
use crate::source::Source;

const SOCKSTAT: &str = "/proc/net/sockstat";
const SOCKSTAT6: &str = "/proc/net/sockstat6";
const CT_COUNT: &str = "/proc/sys/net/netfilter/nf_conntrack_count";
const CT_MAX: &str = "/proc/sys/net/netfilter/nf_conntrack_max";
const PORT_RANGE: &str = "/proc/sys/net/ipv4/ip_local_port_range";
const MAX_ORPHANS: &str = "/proc/sys/net/ipv4/tcp_max_orphans";
const TCP_MEM: &str = "/proc/sys/net/ipv4/tcp_mem";

const CONNTRACK_WARN: f64 = 80.0;
const CONNTRACK_CRIT: f64 = 90.0;
const TW_PORTS_WARN: f64 = 50.0;
const ORPHANS_WARN: f64 = 50.0;
const TCP_MEM_WARN: f64 = 80.0;
/// A TIME_WAIT rise over the window of more than this share of the port range gets a note.
const TW_RISING_PCT: f64 = 5.0;

/// One sample: sockstat plus the limits it is compared against.
#[derive(Debug, Clone)]
struct Snapshot {
    v4: Sockstat,
    v6: Option<Sockstat>,
    /// `(nf_conntrack_count, nf_conntrack_max)`; `None` when conntrack is not in use.
    conntrack: Option<(u64, u64)>,
    /// `(lo, hi)` of `ip_local_port_range`.
    ports: Option<(u64, u64)>,
    max_orphans: Option<u64>,
    /// Third `tcp_mem` value (pages).
    tcp_mem_max: Option<u64>,
}

impl Snapshot {
    /// `TCP` plus `TCP6` value of `key`.
    fn tcp(&self, key: &str) -> u64 {
        let v6 = self.v6.as_ref().and_then(|s| s.get("TCP6", key));
        self.v4.get("TCP", key).unwrap_or(0) + v6.unwrap_or(0)
    }

    /// TIME_WAIT sockets. Mainline kernels count both families in `TCP: tw` and have no
    /// `TCP6: tw`; it is added where a kernel reports it.
    fn tw(&self) -> u64 {
        self.tcp("tw")
    }
}

fn numbers(src: &dyn Source, path: &str) -> Option<Vec<u64>> {
    sockstat::parse_numbers(&src.read_to_string(path).ok()?).ok()
}

fn number(src: &dyn Source, path: &str) -> Option<u64> {
    numbers(src, path).and_then(|v| v.first().copied())
}

#[derive(Default)]
pub struct Sockets {
    first: Option<Snapshot>,
    last: Option<Snapshot>,
    error: SampleError,
}

impl Check for Sockets {
    fn id(&self) -> &'static str {
        "sockets"
    }

    fn sample(&mut self, src: &dyn Source, _t: f64) {
        let Some(text) = self.error.read(src, SOCKSTAT) else {
            return;
        };
        let v4 = match sockstat::parse(&text) {
            Ok(s) if s.has("TCP") => s,
            Ok(_) => {
                return self
                    .error
                    .record(SOCKSTAT, &io::Error::other("no TCP line"));
            }
            Err(e) => return self.error.record(SOCKSTAT, &io::Error::other(e.0)),
        };
        let snap = Snapshot {
            v4,
            v6: src
                .read_to_string(SOCKSTAT6)
                .ok()
                .and_then(|s| sockstat::parse(&s).ok()),
            conntrack: number(src, CT_COUNT).zip(number(src, CT_MAX)),
            ports: numbers(src, PORT_RANGE)
                .filter(|v| v.len() == 2 && v[0] <= v[1])
                .map(|v| (v[0], v[1])),
            max_orphans: number(src, MAX_ORPHANS),
            tcp_mem_max: numbers(src, TCP_MEM).filter(|v| v.len() == 3).map(|v| v[2]),
        };
        if self.first.is_none() {
            self.first = Some(snap);
        } else {
            self.last = Some(snap);
        }
    }

    fn evaluate(&self, _ctx: &Context) -> Section {
        let s = Section::new(
            "sockets",
            "Sockets and conntrack",
            "ss -s / conntrack -S",
            Resource::Network,
        );
        let Some(first) = &self.first else {
            return s.skipped(self.error.get().unwrap_or("no samples"));
        };
        evaluate(s, first, self.last.as_ref().unwrap_or(first))
    }
}

/// `0%`, `<0.1%`, `0.5%` below 1, `4%` otherwise.
fn pct(v: f64) -> String {
    if v > 0.0 && v < 0.1 {
        "<0.1%".to_owned()
    } else if v > 0.0 && v < 1.0 {
        format!("{v:.1}%")
    } else {
        format!("{v:.0}%")
    }
}

/// `a` as a percentage of `b`; multiplied first so round boundaries stay exact.
fn ratio(a: u64, b: u64) -> f64 {
    a as f64 * 100.0 / b as f64
}

fn evaluate(mut s: Section, first: &Snapshot, last: &Snapshot) -> Section {
    let inuse = last.tcp("inuse");
    let tw = last.tw();
    let orphans = last.v4.get("TCP", "orphan").unwrap_or(0);
    let mem = last.v4.get("TCP", "mem").unwrap_or(0);
    s.metric("tcp_inuse", inuse as f64);
    s.metric("tcp_tw", tw as f64);

    for (proto, fields) in last.v4.0.iter().chain(last.v6.iter().flat_map(|v| &v.0)) {
        let kv: Vec<String> = fields.iter().map(|(k, v)| format!("{k} {v}")).collect();
        s.detail(format!("{proto}: {}", kv.join(" ")));
    }

    let mut tw_part = String::new();
    if let Some((lo, hi)) = last.ports {
        let n = hi - lo + 1;
        let p = ratio(tw, n);
        s.metric("tw_port_pct", p);
        tw_part = format!(" ({} of ports)", pct(p));
        s.detail(format!("ephemeral ports {lo}-{hi} ({n})"));
        if p > TW_PORTS_WARN {
            s.warn(format!(
                "many TIME_WAIT sockets vs the ephemeral port range: outbound connection \
                 failures likely; reuse connections / tcp_tw_reuse ({tw} of {n} ports, {p:.1}%)"
            ));
        }
        let tw0 = first.tw();
        if tw > tw0 && ratio(tw - tw0, n) > TW_RISING_PCT {
            s.note(format!("time-wait rising: {tw0} → {tw} during the window"));
        }
    } else {
        s.detail(format!("{PORT_RANGE} not available: time-wait not judged"));
    }

    if let Some(max) = last.max_orphans.filter(|m| *m > 0) {
        let p = ratio(orphans, max);
        s.metric("orphan_pct", p);
        if p > ORPHANS_WARN {
            s.warn(format!(
                "many orphaned TCP sockets vs tcp_max_orphans: the kernel will reset \
                 connections ({orphans} of {max}, {p:.1}%)"
            ));
        }
    }

    if let Some(max) = last.tcp_mem_max.filter(|m| *m > 0) {
        let p = ratio(mem, max);
        s.metric("tcp_mem_pct", p);
        s.detail(format!(
            "tcp memory {mem} of {max} pages (tcp_mem max, {})",
            pct(p)
        ));
        if p > TCP_MEM_WARN {
            s.warn(format!(
                "TCP socket memory near tcp_mem limit: the kernel will start \
                 dropping/collapsing ({mem} of {max} pages, {p:.1}%)"
            ));
        }
    }

    let mut ct_part = String::new();
    match last.conntrack.filter(|(_, max)| *max > 0) {
        Some((count, max)) => {
            let p = ratio(count, max);
            s.metric("conntrack_pct", p);
            ct_part = format!(", conntrack {count}/{max} ({})", pct(p));
            s.threshold(
                p,
                CONNTRACK_WARN,
                CONNTRACK_CRIT,
                format!(
                    "conntrack table nearly full: new connections will be dropped; raise \
                     nf_conntrack_max or shorten timeouts ({count} of {max}, {p:.1}%)"
                ),
            );
        }
        None => s.detail("conntrack not in use"),
    }

    s.summary(format!(
        "tcp {inuse} in use, {tw} time-wait{tw_part}, {orphans} orphans{ct_part}"
    ));
    s
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

    /// sockstat with the given TCP `(inuse, orphan, tw, mem)`.
    fn sockstat(inuse: u64, orphan: u64, tw: u64, mem: u64) -> String {
        format!(
            "sockets: used 412\n\
             TCP: inuse {inuse} orphan {orphan} tw {tw} alloc 45 mem {mem}\n\
             UDP: inuse 6 mem 3\nRAW: inuse 0\nFRAG: inuse 0 memory 0\n"
        )
    }

    /// Only sockstat; no sockstat6, sysctls or conntrack.
    fn bare(inuse: u64, orphan: u64, tw: u64, mem: u64) -> MemSource {
        MemSource::new().with(SOCKSTAT, &sockstat(inuse, orphan, tw, mem))
    }

    fn run(src: &MemSource) -> Section {
        let mut c = Sockets::default();
        c.sample(src, 0.0);
        c.sample(src, 1.0);
        c.evaluate(&ctx())
    }

    fn conntrack(count: u64, max: u64) -> Section {
        let src = bare(1, 0, 0, 0);
        src.set(CT_COUNT, &format!("{count}\n"));
        src.set(CT_MAX, &format!("{max}\n"));
        run(&src)
    }

    fn with_ports(src: MemSource) -> MemSource {
        src.with(PORT_RANGE, "1000\t1999\n")
    }

    #[test]
    fn summary() {
        let src = bare(38, 0, 1204, 12)
            .with(PORT_RANGE, "32768\t60999\n")
            .with(CT_COUNT, "1203\n")
            .with(CT_MAX, "262144\n");
        let s = run(&src);
        assert_eq!(s.status, Status::Ok, "{:?}", s.findings);
        assert_eq!(
            s.summary,
            "tcp 38 in use, 1204 time-wait (4% of ports), 0 orphans, conntrack 1203/262144 (0.5%)"
        );
        assert_eq!(s.resource, Resource::Network);
        for line in [
            "sockets: used 412",
            "TCP: inuse 38 orphan 0 tw 1204 alloc 45 mem 12",
            "UDP: inuse 6 mem 3",
            "RAW: inuse 0",
            "FRAG: inuse 0 memory 0",
        ] {
            assert!(
                s.details.iter().any(|d| d == line),
                "{line}: {:?}",
                s.details
            );
        }
        assert_eq!(s.metrics["tcp_inuse"], 38.0);
        assert_eq!(s.metrics["tcp_tw"], 1204.0);
        assert!(s.findings.is_empty(), "{:?}", s.findings);
    }

    #[test]
    fn percent_format() {
        assert_eq!(pct(0.0), "0%");
        assert_eq!(pct(0.0068), "<0.1%");
        assert_eq!(pct(0.459), "0.5%");
        assert_eq!(pct(4.26), "4%");
        assert_eq!(pct(50.0), "50%");
    }

    #[test]
    fn ipv6_sockets_counted() {
        let src = bare(10, 0, 300, 0).with(SOCKSTAT6, "TCP6: inuse 5 tw 201\nUDP6: inuse 2\n");
        let s = run(&src);
        assert_eq!(s.metrics["tcp_inuse"], 15.0);
        assert_eq!(s.metrics["tcp_tw"], 501.0);
        assert!(s.details.iter().any(|d| d == "TCP6: inuse 5 tw 201"));
        assert!(s.details.iter().any(|d| d == "UDP6: inuse 2"));
    }

    #[test]
    fn conntrack_boundaries() {
        let s = conntrack(800, 1000);
        assert_eq!(s.metrics["conntrack_pct"], 80.0);
        assert_eq!(s.status, Status::Ok);
        let s = conntrack(801, 1000);
        assert_eq!(s.status, Status::Warn);
        assert!(
            s.findings[0]
                .message
                .contains("conntrack table nearly full")
        );
        assert!(s.findings[0].message.contains("raise nf_conntrack_max"));
        assert_eq!(conntrack(900, 1000).status, Status::Warn);
        assert_eq!(conntrack(901, 1000).status, Status::Crit);
    }

    #[test]
    fn conntrack_not_in_use() {
        let s = run(&bare(1, 0, 0, 0));
        assert_eq!(s.status, Status::Ok);
        assert!(s.details.iter().any(|d| d == "conntrack not in use"));
        assert!(!s.metrics.contains_key("conntrack_pct"));
        assert!(!s.summary.contains("conntrack"), "{}", s.summary);
        // Only one of the two files is also "not in use", as is a zero max.
        let src = bare(1, 0, 0, 0).with(CT_COUNT, "5\n");
        assert!(!run(&src).metrics.contains_key("conntrack_pct"));
        assert!(!conntrack(5, 0).metrics.contains_key("conntrack_pct"));
    }

    #[test]
    fn tw_port_boundaries() {
        let s = run(&with_ports(bare(1, 0, 500, 0)));
        assert_eq!(s.metrics["tw_port_pct"], 50.0);
        assert_eq!(s.status, Status::Ok);
        let s = run(&with_ports(bare(1, 0, 501, 0)));
        assert_eq!(s.status, Status::Warn);
        assert!(s.findings[0].message.contains("ephemeral port range"));
        assert!(s.findings[0].message.contains("tcp_tw_reuse"));
        assert!(
            s.summary.contains("501 time-wait (50% of ports)"),
            "{}",
            s.summary
        );
    }

    #[test]
    fn ipv6_time_wait_counted() {
        let src = with_ports(bare(1, 0, 300, 0)).with(SOCKSTAT6, "TCP6: inuse 0 tw 201\n");
        assert_eq!(run(&src).status, Status::Warn);
    }

    #[test]
    fn time_wait_trend_note() {
        let trend = |from: u64, to: u64| {
            let src = with_ports(bare(1, 0, from, 0));
            let mut c = Sockets::default();
            c.sample(&src, 0.0);
            src.set(SOCKSTAT, &sockstat(1, 0, to, 0));
            c.sample(&src, 1.0);
            c.evaluate(&ctx())
        };
        let s = trend(100, 151);
        assert_eq!(s.status, Status::Ok);
        assert!(
            s.findings
                .iter()
                .any(|f| f.level == Level::Note && f.message.contains("time-wait rising")),
            "{:?}",
            s.findings
        );
        assert!(trend(100, 150).findings.is_empty());
        assert!(trend(151, 100).findings.is_empty());
    }

    #[test]
    fn orphan_boundaries() {
        let orphans = |n: u64| run(&bare(1, n, 0, 0).with(MAX_ORPHANS, "1000\n"));
        let s = orphans(500);
        assert_eq!(s.metrics["orphan_pct"], 50.0);
        assert_eq!(s.status, Status::Ok);
        let s = orphans(501);
        assert_eq!(s.status, Status::Warn);
        assert!(s.findings[0].message.contains("orphan"));
    }

    #[test]
    fn tcp_mem_boundaries() {
        let mem = |n: u64| run(&bare(1, 0, 0, n).with(TCP_MEM, "100\t200\t1000\n"));
        let s = mem(800);
        assert_eq!(s.metrics["tcp_mem_pct"], 80.0);
        assert_eq!(s.status, Status::Ok);
        let s = mem(801);
        assert_eq!(s.status, Status::Warn);
        assert!(s.findings[0].message.contains("tcp_mem limit"));
    }

    #[test]
    fn missing_sysctls_skip_only_their_finding() {
        let src = bare(1, 900, 900, 900)
            .with(CT_COUNT, "95\n")
            .with(CT_MAX, "100\n");
        let s = run(&src);
        assert_eq!(s.status, Status::Crit);
        assert_eq!(s.findings.len(), 1, "{:?}", s.findings);
        assert!(s.findings[0].message.contains("conntrack"));
        for k in ["tw_port_pct", "orphan_pct", "tcp_mem_pct"] {
            assert!(!s.metrics.contains_key(k), "{k}");
        }
        assert!(
            s.summary.contains("900 time-wait, 900 orphans"),
            "{}",
            s.summary
        );
        // Malformed sysctls count as missing.
        let src = bare(1, 900, 900, 900)
            .with(PORT_RANGE, "2000 1000\n")
            .with(TCP_MEM, "100 200\n")
            .with(MAX_ORPHANS, "0\n");
        let s = run(&src);
        assert_eq!(s.status, Status::Ok);
        for k in ["tw_port_pct", "orphan_pct", "tcp_mem_pct"] {
            assert!(!s.metrics.contains_key(k), "{k}");
        }
    }

    #[test]
    fn missing_sockstat_is_skipped() {
        let mut c = Sockets::default();
        c.sample(
            &MemSource::new().with(CT_COUNT, "1\n").with(CT_MAX, "2\n"),
            0.0,
        );
        let s = c.evaluate(&ctx());
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains("/proc/net/sockstat"), "{}", s.summary);

        for bad in ["TCP: inuse x\n", "UDP: inuse 1\n"] {
            let mut c = Sockets::default();
            c.sample(&MemSource::new().with(SOCKSTAT, bad), 0.0);
            let s = c.evaluate(&ctx());
            assert_eq!(s.status, Status::Skipped, "{bad}");
            assert!(s.summary.contains("/proc/net/sockstat"), "{}", s.summary);
        }
    }

    #[test]
    fn no_sockstat6_is_fine() {
        let src = bare(3, 0, 4, 0).with(SOCKSTAT6, "garbage");
        let s = run(&src);
        assert_ne!(s.status, Status::Skipped);
        assert_eq!(s.metrics["tcp_inuse"], 3.0);
        assert_eq!(s.metrics["tcp_tw"], 4.0);
        let s = run(&bare(3, 0, 4, 0));
        assert_eq!(s.metrics["tcp_inuse"], 3.0);
    }

    #[test]
    fn fixture_trees() {
        let tree = |name: &str| {
            let src = FsSource::new(format!(
                "{}/tests/fixtures/{name}",
                env!("CARGO_MANIFEST_DIR")
            ));
            let mut c = Sockets::default();
            c.sample(&src, 0.0);
            c.sample(&src, 0.0);
            c.evaluate(&ctx())
        };
        let arm = tree("linux-arm64");
        assert_eq!(arm.status, Status::Ok);
        assert_eq!(
            arm.summary,
            "tcp 0 in use, 0 time-wait (0% of ports), 0 orphans, conntrack 0/65536 (0%)"
        );
        assert!(arm.details.iter().any(|d| d.starts_with("TCP6: inuse 0")));
        let legacy = tree("linux-legacy");
        assert_eq!(legacy.status, Status::Ok);
        assert_eq!(
            legacy.summary,
            "tcp 38 in use, 1204 time-wait (4% of ports), 0 orphans"
        );
        assert!(legacy.details.iter().any(|d| d == "conntrack not in use"));
        assert_eq!(legacy.metrics["orphan_pct"], 0.0);
        assert!(legacy.metrics.contains_key("tcp_mem_pct"));
    }
}
