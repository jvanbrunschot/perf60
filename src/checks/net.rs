//! `sar -n DEV 1` (per-interface throughput, utilization, errors) and `sar -n TCP,ETCP 1`
//! (TCP connection rates, retransmits, accept-queue overflows).

use std::collections::BTreeMap;
use std::io;

use crate::check::{Check, Context, Resource, SampleError, Section, rate};
use crate::procfs::net_dev::{self, IfStats};
use crate::procfs::snmp::{self, Snmp};
use crate::procfs::softirqs::{self, SoftIrqs};
use crate::procfs::softnet::{self, SoftnetRow};
use crate::source::Source;
use crate::units;

const DEV: &str = "/proc/net/dev";
const SNMP: &str = "/proc/net/snmp";
const NETSTAT: &str = "/proc/net/netstat";
const SOFTNET: &str = "/proc/net/softnet_stat";
const SOFTIRQS: &str = "/proc/softirqs";

const UTIL_WARN: f64 = 70.0;
const UTIL_CRIT: f64 = 90.0;
const RETRANS_WARN: f64 = 1.0;
const RETRANS_CRIT: f64 = 5.0;
/// Fewer segments than this in the window (or since boot) is too little to judge a ratio.
const MIN_SEGS: u64 = 100;
/// NET_RX softirqs needed in the window before judging their spread over CPUs.
const NET_RX_MIN: u64 = 1000;
/// One CPU handling more than this share of NET_RX gets a note.
const NET_RX_SHARE_PCT: f64 = 80.0;

/// A timestamped sample and the first/last pair of a window.
type Sample<T> = (f64, T);
type Ends<'a, T> = (&'a Sample<T>, &'a Sample<T>);

/// Keeps the first and the latest successful sample.
struct Window<T> {
    first: Option<(f64, T)>,
    last: Option<(f64, T)>,
}

impl<T> Default for Window<T> {
    fn default() -> Self {
        Window {
            first: None,
            last: None,
        }
    }
}

impl<T> Window<T> {
    fn push(&mut self, t: f64, v: T) {
        if self.first.is_none() {
            self.first = Some((t, v));
        } else {
            self.last = Some((t, v));
        }
    }

    /// `(first, last)`; with a single sample both are the same.
    fn ends(&self) -> Option<Ends<'_, T>> {
        let first = self.first.as_ref()?;
        Some((first, self.last.as_ref().unwrap_or(first)))
    }
}

fn pct(v: f64) -> String {
    if v > 0.0 && v < 1.0 {
        "<1%".to_owned()
    } else {
        format!("{v:.0}%")
    }
}

// ---------------------------------------------------------------------------------------------
// sar -n DEV 1

#[derive(Default)]
pub struct NetDev {
    window: Window<Vec<IfStats>>,
    /// Link speed in Mb/s per interface; `None` when unknown (virtual NICs report -1).
    speeds: BTreeMap<String, Option<u64>>,
    /// Optional per-CPU sources: missing ones only omit their signals.
    softnet: Window<Vec<SoftnetRow>>,
    softirqs: Window<SoftIrqs>,
    error: SampleError,
}

impl Check for NetDev {
    fn id(&self) -> &'static str {
        "net"
    }

    fn sample(&mut self, src: &dyn Source, t: f64) {
        if let Some(rows) = src
            .read_to_string(SOFTNET)
            .ok()
            .and_then(|s| softnet::parse(&s).ok())
        {
            self.softnet.push(t, rows);
        }
        if let Some(irqs) = src
            .read_to_string(SOFTIRQS)
            .ok()
            .and_then(|s| softirqs::parse(&s).ok())
        {
            self.softirqs.push(t, irqs);
        }
        let Some(s) = self.error.read(src, DEV) else {
            return;
        };
        match net_dev::parse(&s) {
            Ok(ifs) => {
                for i in &ifs {
                    if i.name != "lo" && !self.speeds.contains_key(&i.name) {
                        let speed = src
                            .read_to_string(&format!("/sys/class/net/{}/speed", i.name))
                            .ok()
                            .and_then(|s| s.trim().parse::<i64>().ok())
                            .filter(|s| *s > 0)
                            .map(|s| s as u64);
                        self.speeds.insert(i.name.clone(), speed);
                    }
                }
                self.window.push(t, ifs);
            }
            Err(e) => self.error.record(DEV, &io::Error::other(e.0)),
        }
    }

    fn evaluate(&self, _ctx: &Context) -> Section {
        let s = Section::new(
            "net",
            "Network interfaces",
            "sar -n DEV 1",
            Resource::Network,
        );
        let Some((first, last)) = self.window.ends() else {
            return s.skipped(self.error.get().unwrap_or("no samples"));
        };
        let mut s = evaluate_dev(s, first, last, &self.speeds);
        if let Some((a, b)) = self.softnet.ends() {
            softnet_signals(&mut s, &a.1, &b.1);
        }
        if let Some((a, b)) = self.softirqs.ends() {
            net_rx_spread(&mut s, &a.1, &b.1);
        }
        s
    }
}

/// Backlog drops and NAPI time squeezes, summed over all CPUs.
fn softnet_signals(s: &mut Section, a: &[SoftnetRow], b: &[SoftnetRow]) {
    let sum = |rows: &[SoftnetRow], f: fn(&SoftnetRow) -> u64| {
        rows.iter().fold(0u64, |acc, r| acc.saturating_add(f(r)))
    };
    let delta = |f: fn(&SoftnetRow) -> u64| sum(b, f).saturating_sub(sum(a, f));
    let dropped = delta(|r| r.dropped);
    let squeezed = delta(|r| r.time_squeeze);
    s.metric("softnet_dropped", dropped as f64);
    s.metric("softnet_squeezed", squeezed as f64);
    if dropped > 0 {
        s.warn(format!(
            "{dropped} packets dropped at the per-CPU backlog: raise net.core.netdev_max_backlog \
             / check RPS"
        ));
    }
    if squeezed > 0 {
        s.note(format!(
            "NAPI budget exhausted {squeezed} times: net.core.netdev_budget"
        ));
    }
}

/// NET_RX softirqs concentrated on one CPU point at IRQ affinity or RSS problems.
fn net_rx_spread(s: &mut Section, a: &SoftIrqs, b: &SoftIrqs) {
    let (Some(ra), Some(rb)) = (a.get("NET_RX"), b.get("NET_RX")) else {
        return;
    };
    // CPUs present at both ends of the window, matched by number.
    let per_cpu: Vec<(usize, u64)> = b
        .cpus
        .iter()
        .zip(rb)
        .filter_map(|(cpu, end)| {
            let i = a.cpus.iter().position(|c| c == cpu)?;
            Some((*cpu, end.saturating_sub(ra[i])))
        })
        .collect();
    let total: u64 = per_cpu.iter().map(|(_, d)| d).sum();
    if per_cpu.len() < 2 || total <= NET_RX_MIN {
        return;
    }
    // Largest first; ties keep the lower CPU number.
    let (cpu, max) = per_cpu.iter().fold(
        (0, 0),
        |best, &(c, d)| if d > best.1 { (c, d) } else { best },
    );
    let share = max as f64 * 100.0 / total as f64;
    s.metric("net_rx_max_cpu_share_pct", share);
    if share > NET_RX_SHARE_PCT {
        s.note(format!(
            "NET_RX concentrated on cpu{cpu} ({share:.0}% of {total} softirqs): check IRQ \
             affinity / RSS"
        ));
    }
}

struct IfRate {
    name: String,
    rx_bps: f64,
    tx_bps: f64,
    rx_pps: f64,
    tx_pps: f64,
    errors: u64,
    drops: u64,
    errors_ps: f64,
    drops_ps: f64,
    speed: Option<u64>,
    util: Option<f64>,
    active: bool,
}

fn if_rate(t0: f64, a: &IfStats, t1: f64, b: &IfStats, speed: Option<u64>) -> IfRate {
    let r = |x: u64, y: u64| rate((t0, x), (t1, y));
    let d = |x: u64, y: u64| y.saturating_sub(x);
    let (rx_bps, tx_bps) = (r(a.rx_bytes, b.rx_bytes), r(a.tx_bytes, b.tx_bytes));
    let errors = d(a.rx_errs + a.tx_errs, b.rx_errs + b.tx_errs);
    let drops = d(a.rx_drop + a.tx_drop, b.rx_drop + b.tx_drop);
    let moved = d(a.rx_bytes, b.rx_bytes)
        + d(a.tx_bytes, b.tx_bytes)
        + d(a.rx_packets, b.rx_packets)
        + d(a.tx_packets, b.tx_packets);
    IfRate {
        name: b.name.clone(),
        rx_bps,
        tx_bps,
        rx_pps: r(a.rx_packets, b.rx_packets),
        tx_pps: r(a.tx_packets, b.tx_packets),
        errors,
        drops,
        errors_ps: r(a.rx_errs + a.tx_errs, b.rx_errs + b.tx_errs),
        drops_ps: r(a.rx_drop + a.tx_drop, b.rx_drop + b.tx_drop),
        speed,
        // Multiply before dividing so round boundaries (70%, 90%) stay exact.
        util: speed.map(|mbps| rx_bps.max(tx_bps) * 8.0 * 100.0 / (mbps as f64 * 1e6)),
        active: moved + errors + drops > 0,
    }
}

fn evaluate_dev(
    mut s: Section,
    first: &(f64, Vec<IfStats>),
    last: &(f64, Vec<IfStats>),
    speeds: &BTreeMap<String, Option<u64>>,
) -> Section {
    let (t0, t1) = (first.0, last.0);
    let rates: Vec<IfRate> = last
        .1
        .iter()
        .filter(|b| b.name != "lo")
        .filter_map(|b| {
            let a = first.1.iter().find(|a| a.name == b.name)?;
            Some(if_rate(
                t0,
                a,
                t1,
                b,
                speeds.get(&b.name).copied().flatten(),
            ))
        })
        .collect();

    for r in &rates {
        let n = &r.name;
        s.metric(format!("{n}.rx_bytes_per_sec"), r.rx_bps);
        s.metric(format!("{n}.tx_bytes_per_sec"), r.tx_bps);
        s.metric(format!("{n}.rx_packets_per_sec"), r.rx_pps);
        s.metric(format!("{n}.tx_packets_per_sec"), r.tx_pps);
        s.metric(format!("{n}.errors"), r.errors as f64);
        s.metric(format!("{n}.drops"), r.drops as f64);
        if let Some(u) = r.util {
            s.metric(format!("{n}.util_pct"), u);
        }
        if !r.active {
            continue;
        }
        let util = match (r.util, r.speed) {
            (Some(u), Some(sp)) => format!(", util {} of {sp} Mb/s", pct(u)),
            _ => String::new(),
        };
        s.detail(format!(
            "{n}: rx {} {:.1} pkt/s, tx {} {:.1} pkt/s, errs {:.1}/s, drops {:.1}/s{util}",
            units::bytes_rate(r.rx_bps),
            r.rx_pps,
            units::bytes_rate(r.tx_bps),
            r.tx_pps,
            r.errors_ps,
            r.drops_ps,
        ));
        if let (Some(u), Some(sp)) = (r.util, r.speed) {
            s.threshold(
                u,
                UTIL_WARN,
                UTIL_CRIT,
                format!("{n} at {} of {sp} Mb/s: interface saturation", pct(u)),
            );
        }
        if r.errors > 0 || r.drops > 0 {
            s.warn(format!(
                "{n}: {} errors, {} drops during the window (check NIC, driver, ring buffers)",
                r.errors, r.drops
            ));
        }
    }

    let busiest = rates
        .iter()
        .filter(|r| r.active)
        .max_by(|a, b| (a.rx_bps + a.tx_bps).total_cmp(&(b.rx_bps + b.tx_bps)));
    match busiest {
        Some(r) => s.summary(format!(
            "{} rx {} tx {}{}",
            r.name,
            units::bytes_rate(r.rx_bps),
            units::bytes_rate(r.tx_bps),
            r.util
                .map(|u| format!(" (util {})", pct(u)))
                .unwrap_or_default()
        )),
        None => s.summary(format!("all interfaces idle ({})", rates.len())),
    }
    s
}

// ---------------------------------------------------------------------------------------------
// sar -n TCP,ETCP 1

#[derive(Default)]
pub struct Tcp {
    snmp: Window<Snmp>,
    /// `/proc/net/netstat` (TcpExt). Optional: missing just omits those findings.
    netstat: Window<Snmp>,
    error: SampleError,
}

impl Check for Tcp {
    fn id(&self) -> &'static str {
        "tcp"
    }

    fn sample(&mut self, src: &dyn Source, t: f64) {
        if let Some(s) = self.error.read(src, SNMP) {
            match snmp::parse(&s) {
                Ok(m) if m.has_section("Tcp") => self.snmp.push(t, m),
                Ok(_) => self
                    .error
                    .record(SNMP, &io::Error::other("no Tcp counters")),
                Err(e) => self.error.record(SNMP, &io::Error::other(e.0)),
            }
        }
        if let Some(m) = src
            .read_to_string(NETSTAT)
            .ok()
            .and_then(|s| snmp::parse(&s).ok())
            .filter(|m| m.has_section("TcpExt"))
        {
            self.netstat.push(t, m);
        }
    }

    fn evaluate(&self, _ctx: &Context) -> Section {
        let s = Section::new("tcp", "TCP", "sar -n TCP,ETCP 1", Resource::Network);
        let Some((first, last)) = self.snmp.ends() else {
            return s.skipped(self.error.get().unwrap_or("no samples"));
        };
        evaluate_tcp(s, first, last, self.netstat.ends())
    }
}

fn counter(m: &Snmp, section: &str, field: &str) -> u64 {
    m.get(section, field).unwrap_or(0).max(0) as u64
}

fn evaluate_tcp(
    mut s: Section,
    first: &(f64, Snmp),
    last: &(f64, Snmp),
    netstat: Option<Ends<'_, Snmp>>,
) -> Section {
    let (t0, a) = (first.0, &first.1);
    let (t1, b) = (last.0, &last.1);
    let r = |f: &str| rate((t0, counter(a, "Tcp", f)), (t1, counter(b, "Tcp", f)));
    let d = |f: &str| counter(b, "Tcp", f).saturating_sub(counter(a, "Tcp", f));

    let active = r("ActiveOpens");
    let passive = r("PassiveOpens");
    let retrans = r("RetransSegs");
    let out_segs = r("OutSegs");
    let estab = b.get("Tcp", "CurrEstab").unwrap_or(0).max(0);
    s.metric("active_per_sec", active);
    s.metric("passive_per_sec", passive);
    s.metric("retrans_per_sec", retrans);
    s.metric("out_segs_per_sec", out_segs);
    s.metric("curr_estab", estab as f64);

    s.detail(format!(
        "segments in {:.1}/s out {:.1}/s, retransmitted {:.1}/s",
        r("InSegs"),
        out_segs,
        retrans
    ));
    s.detail(format!(
        "attempt fails {:.1}/s, estab resets {:.1}/s, in errs {:.1}/s, out resets {:.1}/s",
        r("AttemptFails"),
        r("EstabResets"),
        r("InErrs"),
        r("OutRsts")
    ));

    let (d_out, d_retrans) = (d("OutSegs"), d("RetransSegs"));
    let mut ratio = String::new();
    if d_out >= MIN_SEGS {
        let p = d_retrans as f64 * 100.0 / d_out as f64;
        s.metric("retrans_pct", p);
        ratio = format!(" ({p:.2}%)");
        s.threshold(
            p,
            RETRANS_WARN,
            RETRANS_CRIT,
            format!(
                "{p:.2}% of {d_out} segments retransmitted: network or remote-host problems \
                 (loss, congestion, overloaded peer)"
            ),
        );
    } else {
        s.detail(format!(
            "retransmit ratio: too little traffic to judge ({d_out} segments sent, need {MIN_SEGS})"
        ));
    }

    let (boot_out, boot_retrans) = (
        counter(b, "Tcp", "OutSegs"),
        counter(b, "Tcp", "RetransSegs"),
    );
    if boot_out >= MIN_SEGS {
        let p = boot_retrans as f64 * 100.0 / boot_out as f64;
        if p > RETRANS_WARN {
            s.note(format!(
                "since boot {p:.2}% of {boot_out} segments were retransmitted"
            ));
        }
    }

    udp_errors(&mut s, a, b);

    if let Some(((_, na), (_, nb))) = netstat {
        let d = |f: &str| counter(nb, "TcpExt", f).saturating_sub(counter(na, "TcpExt", f));
        for (field, metric, what) in [
            (
                "TCPBacklogDrop",
                "tcp_backlog_drops",
                "segments dropped because the socket backlog was full: the application \
                 does not read fast enough",
            ),
            (
                "TCPAbortOnMemory",
                "tcp_abort_on_memory",
                "connections aborted for lack of socket memory: check net.ipv4.tcp_mem \
                 and orphaned sockets",
            ),
        ] {
            if nb.get("TcpExt", field).is_none() {
                continue;
            }
            let n = d(field);
            s.metric(metric, n as f64);
            if n > 0 {
                s.warn(format!("{n} {what} ({field})"));
            }
        }
        let (overflows, drops) = (d("ListenOverflows"), d("ListenDrops"));
        s.metric("listen_overflows", overflows as f64);
        s.metric("listen_drops", drops as f64);
        if overflows > 0 || drops > 0 {
            s.warn(format!(
                "accept queue overflow: application not accepting fast enough \
                 (check somaxconn/backlog): {overflows} overflows, {drops} drops during the window"
            ));
        }
    }

    s.summary(format!(
        "active {active:.1}/s passive {passive:.1}/s retrans {retrans:.1}/s{ratio} estab {estab}"
    ));
    s
}

/// UDP buffer and input errors, summed over the `Udp:` and `UdpLite:` sections.
fn udp_errors(s: &mut Section, a: &Snmp, b: &Snmp) {
    if !b.has_section("Udp") {
        return;
    }
    for (field, metric, what) in [
        (
            "RcvbufErrors",
            "udp_rcvbuf_errors",
            "UDP receive buffer overflows: application too slow or rmem too small",
        ),
        (
            "SndbufErrors",
            "udp_sndbuf_errors",
            "UDP send buffer errors: sending faster than the buffer drains or wmem too small",
        ),
        (
            "InErrors",
            "udp_in_errors",
            "UDP input errors: datagrams dropped on receive (buffer overflows or bad checksums)",
        ),
    ] {
        let n: u64 = ["Udp", "UdpLite"]
            .iter()
            .map(|sec| counter(b, sec, field).saturating_sub(counter(a, sec, field)))
            .sum();
        s.metric(metric, n as f64);
        if n > 0 {
            s.warn(format!("{what} ({field} +{n} during the window)"));
        }
    }
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

    // --- net ---

    /// `(name, rx_bytes, tx_bytes, rx_errs, tx_drop)`; packets are bytes / 1000.
    type Row<'a> = (&'a str, u64, u64, u64, u64);

    fn dev(rows: &[Row]) -> String {
        let mut s = String::from(
            "Inter-|   Receive                                                |  Transmit\n \
             face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n",
        );
        for (n, rx, tx, errs, drop) in rows {
            s += &format!(
                "{n:>6}: {rx} {} {errs} 0 0 0 0 0 {tx} {} 0 {drop} 0 0 0 0\n",
                rx / 1000,
                tx / 1000
            );
        }
        s
    }

    fn run_net(t0: &[Row], t1: &[Row], speeds: &[(&str, &str)]) -> Section {
        let src = MemSource::new().with(DEV, &dev(t0));
        for (n, sp) in speeds {
            src.set(&format!("/sys/class/net/{n}/speed"), sp);
        }
        let mut c = NetDev::default();
        c.sample(&src, 0.0);
        src.set(DEV, &dev(t1));
        c.sample(&src, 1.0);
        c.evaluate(&ctx())
    }

    fn gig(rx: u64, tx: u64) -> Section {
        run_net(
            &[("eth0", 0, 0, 0, 0)],
            &[("eth0", rx, tx, 0, 0)],
            &[("eth0", "1000\n")],
        )
    }

    #[test]
    fn summary_names_busiest_interface() {
        let s = run_net(
            &[
                ("lo", 0, 0, 0, 0),
                ("eth0", 0, 0, 0, 0),
                ("eth1", 0, 0, 0, 0),
            ],
            &[
                ("lo", 0, 0, 0, 0),
                ("eth0", 1_258_291, 348_160, 0, 0),
                ("eth1", 1_000, 0, 0, 0),
            ],
            &[],
        );
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.summary, "eth0 rx 1.2 MiB/s tx 340 KiB/s");
        assert_eq!(s.details.len(), 2, "{:?}", s.details);
        assert!(s.details[0].starts_with("eth0: rx 1.2 MiB/s"));
        assert_eq!(s.metrics["eth0.rx_bytes_per_sec"], 1_258_291.0);
        assert_eq!(s.metrics["eth0.tx_bytes_per_sec"], 348_160.0);
        assert!(!s.metrics.contains_key("eth0.util_pct"));
    }

    #[test]
    fn summary_shows_util() {
        let s = gig(12_500_000, 0);
        assert!(s.summary.ends_with("(util 10%)"), "{}", s.summary);
        assert!(
            s.details[0].contains("util 10% of 1000 Mb/s"),
            "{:?}",
            s.details
        );
    }

    #[test]
    fn all_idle() {
        let rows = [("eth0", 5, 6, 0, 0), ("eth1", 7, 8, 0, 0)];
        let s = run_net(&rows, &rows, &[]);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.summary, "all interfaces idle (2)");
        assert!(s.details.is_empty());
    }

    #[test]
    fn loopback_ignored() {
        let s = run_net(
            &[("lo", 0, 0, 0, 0), ("eth0", 0, 0, 0, 0)],
            &[("lo", 1 << 30, 1 << 30, 5, 5), ("eth0", 0, 0, 0, 0)],
            &[("lo", "1\n")],
        );
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.summary, "all interfaces idle (1)");
        assert!(s.findings.is_empty() && s.details.is_empty());
        assert!(!s.metrics.keys().any(|k| k.starts_with("lo.")));
    }

    #[test]
    fn util_boundaries() {
        let s = gig(87_500_000, 0);
        assert_eq!(s.metrics["eth0.util_pct"], 70.0);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(gig(0, 88_750_000).status, Status::Warn);
        assert_eq!(gig(112_500_000, 0).status, Status::Warn);
        let s = gig(113_750_000, 0);
        assert_eq!(s.status, Status::Crit);
        assert!(s.findings[0].message.contains("eth0 at 91% of 1000 Mb/s"));
    }

    #[test]
    fn unknown_speed_has_no_util() {
        for speeds in [&[("eth0", "-1\n")][..], &[]] {
            let s = run_net(
                &[("eth0", 0, 0, 0, 0)],
                &[("eth0", 1_000_000_000, 0, 0, 0)],
                speeds,
            );
            assert!(!s.metrics.contains_key("eth0.util_pct"));
            assert_eq!(s.status, Status::Ok);
            assert!(!s.summary.contains("util"));
        }
    }

    #[test]
    fn errors_warn() {
        let s = run_net(&[("eth0", 0, 0, 0, 0)], &[("eth0", 0, 0, 3, 0)], &[]);
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.metrics["eth0.errors"], 3.0);
        assert!(s.findings[0].message.contains("eth0: 3 errors, 0 drops"));
    }

    #[test]
    fn drops_warn() {
        let s = run_net(
            &[("eth0", 100, 100, 0, 10)],
            &[("eth0", 200, 200, 0, 15)],
            &[],
        );
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.metrics["eth0.drops"], 5.0);
        assert!(s.findings[0].message.contains("eth0: 0 errors, 5 drops"));
    }

    #[test]
    fn old_errors_are_ok() {
        let rows = [("eth0", 0, 0, 100, 0)];
        let s = run_net(&rows, &rows, &[]);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.metrics["eth0.errors"], 0.0);
    }

    /// Net check over one 1 s window with idle interfaces and the given optional sources.
    fn run_net_extra(extra: &[(&str, &str, &str)]) -> Section {
        let rows = [("eth0", 0, 0, 0, 0)];
        let src = MemSource::new().with(DEV, &dev(&rows));
        for (path, t0, _) in extra {
            src.set(path, t0);
        }
        let mut c = NetDev::default();
        c.sample(&src, 0.0);
        for (path, _, t1) in extra {
            src.set(path, t1);
        }
        c.sample(&src, 1.0);
        c.evaluate(&ctx())
    }

    /// softnet_stat rows `(processed, dropped, time_squeeze)` in hex, 13 columns.
    fn softnet(rows: &[(u64, u64, u64)]) -> String {
        rows.iter()
            .enumerate()
            .map(|(i, (p, d, q))| {
                format!(
                    "{p:08x} {d:08x} {q:08x} 00000000 00000000 00000000 00000000 00000000 \
                     00000000 00000000 00000000 00000000 {i:08x}\n"
                )
            })
            .collect()
    }

    fn softirqs(net_rx: &[u64]) -> String {
        let cols: Vec<String> = (0..net_rx.len()).map(|i| format!("CPU{i}")).collect();
        let vals: Vec<String> = net_rx.iter().map(u64::to_string).collect();
        format!(
            "  {}\n  HI: {}\n  NET_RX: {}\n",
            cols.join(" "),
            vec!["0"; net_rx.len()].join(" "),
            vals.join(" ")
        )
    }

    #[test]
    fn softnet_drops_warn() {
        let s = run_net_extra(&[(
            SOFTNET,
            &softnet(&[(100, 0, 0), (100, 3, 0)]),
            &softnet(&[(200, 0, 0), (200, 4, 0)]),
        )]);
        assert_eq!(s.metrics["softnet_dropped"], 1.0);
        assert_eq!(s.metrics["softnet_squeezed"], 0.0);
        assert_eq!(s.status, Status::Warn);
        assert!(
            s.findings[0]
                .message
                .contains("dropped at the per-CPU backlog: raise net.core.netdev_max_backlog"),
            "{:?}",
            s.findings
        );
        let quiet = softnet(&[(1, 2, 3), (4, 5, 6)]);
        let s = run_net_extra(&[(SOFTNET, &quiet, &quiet)]);
        assert_eq!(s.metrics["softnet_dropped"], 0.0);
        assert_eq!(s.metrics["softnet_squeezed"], 0.0);
        assert_eq!(s.status, Status::Ok);
        assert!(s.findings.is_empty());
    }

    #[test]
    fn softnet_squeeze_note() {
        let s = run_net_extra(&[(
            SOFTNET,
            &softnet(&[(100, 0, 10)]),
            &softnet(&[(900, 0, 15)]),
        )]);
        assert_eq!(s.metrics["softnet_squeezed"], 5.0);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.findings[0].level, Level::Note);
        assert!(
            s.findings[0].message.contains("net.core.netdev_budget"),
            "{:?}",
            s.findings
        );
    }

    #[test]
    fn softnet_missing_is_fine() {
        let s = run_net_extra(&[]);
        assert_eq!(s.status, Status::Ok);
        for k in [
            "softnet_dropped",
            "softnet_squeezed",
            "net_rx_max_cpu_share_pct",
        ] {
            assert!(!s.metrics.contains_key(k), "{k}");
        }
        let s = run_net_extra(&[(SOFTNET, "garbage\n", "garbage\n")]);
        assert_eq!(s.status, Status::Ok);
        assert!(!s.metrics.contains_key("softnet_dropped"));
    }

    #[test]
    fn net_rx_concentration() {
        let rx = |d0: u64, d1: u64| {
            run_net_extra(&[(
                SOFTIRQS,
                &softirqs(&[500, 500]),
                &softirqs(&[500 + d0, 500 + d1]),
            )])
        };
        let s = rx(1600, 400);
        assert_eq!(s.metrics["net_rx_max_cpu_share_pct"], 80.0);
        assert!(s.findings.is_empty(), "{:?}", s.findings);
        let s = rx(1601, 399);
        assert_eq!(s.status, Status::Ok);
        assert!(
            s.findings.iter().any(
                |f| f.level == Level::Note && f.message.contains("NET_RX concentrated on cpu0")
            ),
            "{:?}",
            s.findings
        );
        // 1000 in total is too few to judge.
        let s = rx(0, 1000);
        assert!(!s.metrics.contains_key("net_rx_max_cpu_share_pct"));
        assert!(s.findings.is_empty());
        // A single CPU can't be imbalanced.
        let s = run_net_extra(&[(SOFTIRQS, &softirqs(&[0]), &softirqs(&[5000]))]);
        assert!(!s.metrics.contains_key("net_rx_max_cpu_share_pct"));
        assert!(s.findings.is_empty());
    }

    #[test]
    fn missing_net_dev_is_skipped() {
        let mut c = NetDev::default();
        c.sample(&MemSource::new(), 0.0);
        let s = c.evaluate(&ctx());
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains("/proc/net/dev"), "{}", s.summary);
    }

    // --- tcp ---

    /// Tcp counters: `(active, passive, estab, out_segs, retrans)`.
    fn snmp(c: (u64, u64, u64, u64, u64)) -> String {
        let (active, passive, estab, out, retrans) = c;
        format!(
            "Ip: Forwarding DefaultTTL\nIp: 1 64\n\
             Tcp: RtoAlgorithm RtoMin RtoMax MaxConn ActiveOpens PassiveOpens AttemptFails EstabResets CurrEstab InSegs OutSegs RetransSegs InErrs OutRsts InCsumErrors\n\
             Tcp: 1 200 120000 -1 {active} {passive} 0 0 {estab} {out} {out} {retrans} 0 0 0\n"
        )
    }

    fn netstat(overflows: u64, drops: u64) -> String {
        format!(
            "TcpExt: SyncookiesSent ListenOverflows ListenDrops\nTcpExt: 0 {overflows} {drops}\n"
        )
    }

    fn run_tcp(
        t0: (u64, u64, u64, u64, u64),
        t1: (u64, u64, u64, u64, u64),
        ns: Option<((u64, u64), (u64, u64))>,
    ) -> Section {
        let src = MemSource::new().with(SNMP, &snmp(t0));
        if let Some(((o, d), _)) = ns {
            src.set(NETSTAT, &netstat(o, d));
        }
        let mut c = Tcp::default();
        c.sample(&src, 0.0);
        src.set(SNMP, &snmp(t1));
        if let Some((_, (o, d))) = ns {
            src.set(NETSTAT, &netstat(o, d));
        }
        c.sample(&src, 1.0);
        c.evaluate(&ctx())
    }

    /// Window with `out` segments sent and `retrans` retransmitted, from zero.
    fn window(out: u64, retrans: u64) -> Section {
        run_tcp((0, 0, 0, 0, 0), (0, 0, 0, out, retrans), None)
    }

    #[test]
    fn tcp_summary() {
        let s = run_tcp((10, 20, 40, 1000, 5), (13, 32, 42, 5000, 7), None);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(
            s.summary,
            "active 3.0/s passive 12.0/s retrans 2.0/s (0.05%) estab 42"
        );
        assert_eq!(s.metrics["active_per_sec"], 3.0);
        assert_eq!(s.metrics["passive_per_sec"], 12.0);
        assert_eq!(s.metrics["retrans_per_sec"], 2.0);
        assert_eq!(s.metrics["curr_estab"], 42.0);
        assert_eq!(s.metrics["retrans_pct"], 0.05);
    }

    #[test]
    fn retrans_not_judged_below_minimum() {
        let s = window(99, 50);
        assert_eq!(s.status, Status::Ok);
        assert!(!s.metrics.contains_key("retrans_pct"));
        assert!(
            s.details
                .iter()
                .any(|d| d.contains("too little traffic to judge")),
            "{:?}",
            s.details
        );
        assert!(!s.summary.contains('%'));
        assert_eq!(window(100, 2).status, Status::Warn);
    }

    #[test]
    fn retrans_boundaries() {
        let s = window(1000, 10);
        assert_eq!(s.metrics["retrans_pct"], 1.0);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(window(1000, 11).status, Status::Warn);
        assert_eq!(window(1000, 50).status, Status::Warn);
        let s = window(1000, 51);
        assert_eq!(s.status, Status::Crit);
        assert!(s.findings[0].message.contains("network or remote-host"));
    }

    #[test]
    fn since_boot_retrans_note() {
        let s = run_tcp((0, 0, 0, 10_000, 500), (0, 0, 0, 11_000, 500), None);
        assert_eq!(s.status, Status::Ok);
        assert!(
            s.findings
                .iter()
                .any(|f| f.level == Level::Note && f.message.contains("since boot"))
        );
        let s = run_tcp((0, 0, 0, 10_000, 110), (0, 0, 0, 11_000, 110), None);
        assert!(s.findings.is_empty(), "{:?}", s.findings);
    }

    #[test]
    fn listen_overflow_warns() {
        let s = run_tcp((0, 0, 0, 0, 0), (0, 0, 0, 0, 0), Some(((5, 5), (8, 8))));
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.metrics["listen_overflows"], 3.0);
        assert!(s.findings[0].message.contains("accept queue overflow"));
        let quiet = run_tcp((0, 0, 0, 0, 0), (0, 0, 0, 0, 0), Some(((5, 5), (5, 5))));
        assert_eq!(quiet.status, Status::Ok);
    }

    #[test]
    fn listen_drops_warn() {
        let s = run_tcp((0, 0, 0, 0, 0), (0, 0, 0, 0, 0), Some(((5, 5), (5, 6))));
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.metrics["listen_overflows"], 0.0);
    }

    #[test]
    fn missing_netstat_is_fine() {
        let s = window(0, 0);
        assert_eq!(s.status, Status::Ok);
        assert!(!s.metrics.contains_key("listen_overflows"));
    }

    /// Tcp plus Udp and optional UdpLite sections, each `(InErrors, RcvbufErrors, SndbufErrors)`.
    fn snmp_udp(udp: (u64, u64, u64), lite: Option<(u64, u64, u64)>) -> String {
        let mut s = snmp((0, 0, 0, 0, 0));
        let sec = |name: &str, (ie, rb, sb): (u64, u64, u64)| {
            format!(
                "{name}: InDatagrams NoPorts InErrors OutDatagrams RcvbufErrors SndbufErrors\n\
                 {name}: 10 0 {ie} 10 {rb} {sb}\n"
            )
        };
        s += &sec("Udp", udp);
        if let Some(l) = lite {
            s += &sec("UdpLite", l);
        }
        s
    }

    fn run_udp(t0: &str, t1: &str) -> Section {
        let src = MemSource::new().with(SNMP, t0);
        let mut c = Tcp::default();
        c.sample(&src, 0.0);
        src.set(SNMP, t1);
        c.sample(&src, 1.0);
        c.evaluate(&ctx())
    }

    #[test]
    fn udp_errors_warn() {
        let s = run_udp(&snmp_udp((5, 5, 0), None), &snmp_udp((5, 9, 0), None));
        assert_eq!(s.metrics["udp_rcvbuf_errors"], 4.0);
        assert_eq!(s.metrics["udp_in_errors"], 0.0);
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.findings.len(), 1, "{:?}", s.findings);
        assert!(
            s.findings[0].message.contains(
                "UDP receive buffer overflows: application too slow or rmem too small (RcvbufErrors"
            ),
            "{:?}",
            s.findings
        );
        let s = run_udp(&snmp_udp((0, 0, 0), None), &snmp_udp((2, 0, 0), None));
        assert_eq!(s.metrics["udp_in_errors"], 2.0);
        assert_eq!(s.status, Status::Warn);
        assert!(s.findings[0].message.contains("InErrors"));
        let quiet = snmp_udp((3, 3, 3), Some((1, 1, 1)));
        let s = run_udp(&quiet, &quiet);
        assert_eq!(s.status, Status::Ok);
        for k in ["udp_rcvbuf_errors", "udp_sndbuf_errors", "udp_in_errors"] {
            assert_eq!(s.metrics[k], 0.0, "{k}");
        }
    }

    #[test]
    fn udplite_counts() {
        let s = run_udp(
            &snmp_udp((0, 0, 0), Some((0, 0, 0))),
            &snmp_udp((0, 0, 0), Some((0, 0, 1))),
        );
        assert_eq!(s.metrics["udp_sndbuf_errors"], 1.0);
        assert_eq!(s.status, Status::Warn);
        assert!(s.findings[0].message.contains("SndbufErrors +1"));
    }

    #[test]
    fn no_udp_section() {
        let s = window(0, 0);
        assert_eq!(s.status, Status::Ok);
        assert!(!s.metrics.contains_key("udp_rcvbuf_errors"));
    }

    #[test]
    fn tcp_socket_memory_drops() {
        let ns = |backlog: u64, abort: u64| {
            format!(
                "TcpExt: ListenOverflows ListenDrops TCPBacklogDrop TCPAbortOnMemory\n\
                 TcpExt: 0 0 {backlog} {abort}\n"
            )
        };
        let run = |t0: &str, t1: &str| {
            let src = MemSource::new()
                .with(SNMP, &snmp((0, 0, 0, 0, 0)))
                .with(NETSTAT, t0);
            let mut c = Tcp::default();
            c.sample(&src, 0.0);
            src.set(NETSTAT, t1);
            c.sample(&src, 1.0);
            c.evaluate(&ctx())
        };
        let s = run(&ns(10, 0), &ns(17, 0));
        assert_eq!(s.metrics["tcp_backlog_drops"], 7.0);
        assert_eq!(s.metrics["tcp_abort_on_memory"], 0.0);
        assert_eq!(s.status, Status::Warn);
        assert!(s.findings[0].message.contains("TCPBacklogDrop"));
        let s = run(&ns(0, 2), &ns(0, 3));
        assert_eq!(s.metrics["tcp_abort_on_memory"], 1.0);
        assert_eq!(s.status, Status::Warn);
        assert!(s.findings[0].message.contains("TCPAbortOnMemory"));
        let s = run(&ns(4, 4), &ns(4, 4));
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.metrics["tcp_backlog_drops"], 0.0);
        // Counters absent (the default netstat helper has neither).
        let s = run_tcp((0, 0, 0, 0, 0), (0, 0, 0, 0, 0), Some(((0, 0), (0, 0))));
        assert!(!s.metrics.contains_key("tcp_backlog_drops"));
        assert!(!s.metrics.contains_key("tcp_abort_on_memory"));
    }

    #[test]
    fn missing_snmp_is_skipped() {
        let mut c = Tcp::default();
        c.sample(&MemSource::new().with(NETSTAT, &netstat(0, 0)), 0.0);
        let s = c.evaluate(&ctx());
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains("/proc/net/snmp"), "{}", s.summary);

        let mut c = Tcp::default();
        c.sample(&MemSource::new().with(SNMP, "Udp: A\nUdp: 1\n"), 0.0);
        assert_eq!(c.evaluate(&ctx()).status, Status::Skipped);
    }

    // --- both, against the captured fixture with identical snapshots ---

    #[test]
    fn fixture_tree_zero_deltas() {
        let src = FsSource::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/linux-arm64"
        ));
        let mut net = NetDev::default();
        let mut tcp = Tcp::default();
        for t in [0.0, 0.0] {
            net.sample(&src, t);
            tcp.sample(&src, t);
        }
        let n = net.evaluate(&ctx());
        assert_eq!(n.status, Status::Ok);
        assert_eq!(n.summary, "all interfaces idle (1)");
        assert_eq!(n.metrics["eth0.util_pct"], 0.0);
        let t = tcp.evaluate(&ctx());
        assert_eq!(t.status, Status::Ok);
        assert_eq!(
            t.summary,
            "active 0.0/s passive 0.0/s retrans 0.0/s estab 0"
        );
        assert_eq!(t.metrics["listen_overflows"], 0.0);
        for s in [&n, &t] {
            assert!(s.metrics.values().all(|v| v.is_finite()), "{:?}", s.metrics);
        }
    }
}
