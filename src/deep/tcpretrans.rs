//! `tcpretrans` (BCC): which remote endpoints the TCP retransmits of the window go to. The
//! counter-based `tcp` check owns the retransmit *ratio*; this section says where to look.

use std::collections::BTreeMap;
use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

use crate::check::{Resource, Section};
#[cfg(feature = "deep")]
use crate::{
    check::{Check, Context},
    source::Source,
};

/// Retransmits per second above which the section warns.
pub const RETRANS_WARN_PER_SEC: f64 = 100.0;
/// Minimum retransmits in the window before judging their concentration.
pub const CONCENTRATION_MIN: u64 = 20;
const TOP_N: usize = 5;

const AF_INET: u16 = 2;
const AF_INET6: u16 = 10;

/// Map key of the eBPF program (`perf60-ebpf/src/bin/tcpretrans.rs` defines the same layout):
/// the remote end of a socket. `dport` is in network byte order; IPv4 addresses use the first
/// 4 bytes of `daddr`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Endpoint {
    pub family: u16,
    pub dport: u16,
    pub daddr: [u8; 16],
}

const _: () = assert!(size_of::<Endpoint>() == 20);

// SAFETY: plain `#[repr(C)]` data without padding or pointers.
#[cfg(feature = "deep")]
unsafe impl aya::Pod for Endpoint {}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let port = u16::from_be(self.dport);
        match self.family {
            AF_INET => {
                let [a, b, c, d, ..] = self.daddr;
                write!(f, "{}:{port}", Ipv4Addr::new(a, b, c, d))
            }
            AF_INET6 => {
                let ip = Ipv6Addr::from(self.daddr);
                match ip.to_ipv4_mapped() {
                    Some(v4) => write!(f, "{v4}:{port}"),
                    None => write!(f, "[{ip}]:{port}"),
                }
            }
            other => write!(f, "family {other} port {port}"),
        }
    }
}

/// Build the section from the window's counts. `by_endpoint` is (endpoint, retransmits);
/// `total` counts every retransmit, including any the full hash map could not key.
pub fn evaluate(
    mut s: Section,
    by_endpoint: Vec<(Endpoint, u64)>,
    total: u64,
    secs: f64,
) -> Section {
    // Merge keys that print the same (an IPv4 peer seen over AF_INET and AF_INET6).
    let mut merged: BTreeMap<String, u64> = BTreeMap::new();
    for (e, n) in by_endpoint {
        *merged.entry(e.to_string()).or_default() += n;
    }
    let mut rows: Vec<(String, u64)> = merged.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    // The hash map and the total are read one after the other; never report a share > 100%.
    let total = total.max(rows.iter().map(|r| r.1).sum());
    let rate = if secs > 0.0 { total as f64 / secs } else { 0.0 };
    let top = rows.first().map(|(e, n)| (e.as_str(), *n));
    let share = match top {
        Some((_, n)) if total > 0 => n as f64 * 100.0 / total as f64,
        _ => 0.0,
    };

    s.summary(match (total, top) {
        (0, _) => "no retransmits".to_owned(),
        (_, None) => format!("{total} retransmits ({rate:.1}/s)"),
        (_, Some((e, n))) => format!(
            "{total} retransmits ({rate:.1}/s) to {} endpoint{}, top {e} {n}",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" }
        ),
    });
    for (e, n) in rows.iter().take(TOP_N) {
        s.detail(format!("{n:>7} retransmits  {e}"));
    }
    if rows.len() > TOP_N {
        s.detail(format!("{} more endpoints", rows.len() - TOP_N));
    }
    s.metric("retransmits", total as f64);
    s.metric("retransmits_per_sec", rate);
    s.metric("endpoints", rows.len() as f64);
    s.metric("top_endpoint_share_pct", share);

    if let Some((e, n)) = top
        && total >= CONCENTRATION_MIN
        && n * 2 > total
    {
        s.note(format!(
            "retransmits concentrated on {e} ({share:.1}% of {total}): suspect that path or host"
        ));
    }
    if rate > RETRANS_WARN_PER_SEC {
        s.warn(format!(
            "{rate:.1} retransmits/s (top {}): heavy packet loss or congestion on that path or host, see the tcp section for the ratio",
            top.map_or("?", |(e, _)| e)
        ));
    }
    s
}

pub fn section() -> Section {
    Section::new(
        "tcpretrans",
        "TCP retransmits by endpoint (eBPF)",
        "tcpretrans (BCC)",
        Resource::Network,
    )
}

#[cfg(feature = "deep")]
#[derive(Default)]
pub struct Tcpretrans {
    probe: Option<Result<super::probe::Probe, String>>,
    window: super::Window,
}

#[cfg(feature = "deep")]
static OBJECT: &[u8] = aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/tcpretrans"));

/// BTF members the program reads, in the order of its offset globals.
#[cfg(feature = "deep")]
const MEMBERS: [(&str, &str, &str); 5] = [
    ("SK_COMMON_OFF", "sock", "__sk_common"),
    ("SKC_FAMILY_OFF", "sock_common", "skc_family"),
    ("SKC_DPORT_OFF", "sock_common", "skc_dport"),
    ("SKC_DADDR_OFF", "sock_common", "skc_daddr"),
    ("SKC_V6_DADDR_OFF", "sock_common", "skc_v6_daddr"),
];

#[cfg(feature = "deep")]
fn attach() -> Result<super::probe::Probe, String> {
    // Check privileges first, so an unprivileged run gets the needs-root reason.
    super::caps::require_bpf()?;
    let wanted: Vec<(&str, &str)> = MEMBERS.iter().map(|(_, s, m)| (*s, *m)).collect();
    let offsets = super::btf::kernel_offsets(&wanted)?;
    let globals: Vec<(&str, u64)> = MEMBERS
        .iter()
        .zip(offsets)
        .map(|((g, _, _), off)| (*g, u64::from(off)))
        .collect();
    super::probe::Probe::attach_with(OBJECT, &[("tcpretrans", "tcp_retransmit_skb")], &globals)
}

#[cfg(feature = "deep")]
impl Check for Tcpretrans {
    fn id(&self) -> &'static str {
        "tcpretrans"
    }

    fn sample(&mut self, _src: &dyn Source, t: f64) {
        if self.probe.is_none() {
            self.probe = Some(attach());
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
        let read = || -> Result<(Vec<(Endpoint, u64)>, u64), String> {
            let by_endpoint = probe.hash_map::<Endpoint, u64>("RETRANS")?;
            let total = probe.per_cpu_sums("TOTAL", 1)?;
            Ok((by_endpoint, total[0]))
        };
        match read() {
            Ok((by_endpoint, total)) => evaluate(s, by_endpoint, total, self.window.secs()),
            Err(e) => s.skipped(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Level, Status};

    fn v4(a: [u8; 4], port: u16) -> Endpoint {
        let mut daddr = [0; 16];
        daddr[..4].copy_from_slice(&a);
        Endpoint {
            family: AF_INET,
            dport: port.to_be(),
            daddr,
        }
    }

    fn v6(ip: &str, port: u16) -> Endpoint {
        Endpoint {
            family: AF_INET6,
            dport: port.to_be(),
            daddr: ip.parse::<Ipv6Addr>().unwrap().octets(),
        }
    }

    fn notes(s: &Section) -> Vec<&str> {
        s.findings
            .iter()
            .filter(|f| f.level == Level::Note)
            .map(|f| f.message.as_str())
            .collect()
    }

    #[test]
    fn key_layout() {
        assert_eq!(size_of::<Endpoint>(), 20);
        assert_eq!(align_of::<Endpoint>(), 2);
        assert_eq!(std::mem::offset_of!(Endpoint, dport), 2);
        assert_eq!(std::mem::offset_of!(Endpoint, daddr), 4);
    }

    #[test]
    fn format_v4() {
        assert_eq!(v4([10, 0, 0, 5], 443).to_string(), "10.0.0.5:443");
        assert_eq!(v4([127, 0, 0, 1], 9000).to_string(), "127.0.0.1:9000");
    }

    #[test]
    fn format_v6() {
        assert_eq!(v6("2001:db8::1", 443).to_string(), "[2001:db8::1]:443");
        assert_eq!(v6("::1", 8080).to_string(), "[::1]:8080");
    }

    #[test]
    fn format_v4_mapped() {
        assert_eq!(v6("::ffff:10.0.0.5", 443).to_string(), "10.0.0.5:443");
    }

    #[test]
    fn quiet_window() {
        let s = evaluate(section(), vec![], 0, 2.0);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.summary, "no retransmits");
        assert!(s.findings.is_empty());
        assert_eq!(s.metrics["top_endpoint_share_pct"], 0.0);
        assert_eq!(s.metrics["endpoints"], 0.0);
    }

    #[test]
    fn summary_and_metrics() {
        let counts = vec![
            (v4([10, 0, 0, 6], 80), 3),
            (v4([10, 0, 0, 5], 443), 12),
            (v6("2001:db8::1", 443), 2),
        ];
        let s = evaluate(section(), counts, 17, 2.0);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(
            s.summary,
            "17 retransmits (8.5/s) to 3 endpoints, top 10.0.0.5:443 12"
        );
        assert_eq!(s.metrics["retransmits"], 17.0);
        assert_eq!(s.metrics["retransmits_per_sec"], 8.5);
        assert_eq!(s.metrics["endpoints"], 3.0);
        assert!((s.metrics["top_endpoint_share_pct"] - 70.588).abs() < 0.01);
        assert!(notes(&s).is_empty(), "17 is below the note minimum");
        assert_eq!(
            s.details,
            [
                "     12 retransmits  10.0.0.5:443",
                "      3 retransmits  10.0.0.6:80",
                "      2 retransmits  [2001:db8::1]:443",
            ]
        );
    }

    #[test]
    fn single_endpoint_and_mapped_merge() {
        let counts = vec![(v4([10, 0, 0, 5], 443), 2), (v6("::ffff:10.0.0.5", 443), 3)];
        let s = evaluate(section(), counts, 5, 1.0);
        assert_eq!(
            s.summary,
            "5 retransmits (5.0/s) to 1 endpoint, top 10.0.0.5:443 5"
        );
    }

    #[test]
    fn top_n_ordering() {
        // Counts 1..=8, with a tie at the top broken by endpoint text.
        let mut counts: Vec<(Endpoint, u64)> =
            (1..=8).map(|i| (v4([10, 0, 0, i as u8], 80), i)).collect();
        counts.push((v4([10, 0, 0, 0], 80), 8));
        let s = evaluate(section(), counts, 44, 1.0);
        assert_eq!(s.details.len(), 6);
        assert_eq!(s.details[0], "      8 retransmits  10.0.0.0:80");
        assert_eq!(s.details[1], "      8 retransmits  10.0.0.8:80");
        assert_eq!(s.details[2], "      7 retransmits  10.0.0.7:80");
        assert_eq!(s.details[4], "      5 retransmits  10.0.0.5:80");
        assert_eq!(s.details[5], "4 more endpoints");
    }

    #[test]
    fn concentration_count_boundary() {
        let e = v4([10, 0, 0, 5], 443);
        let s = evaluate(section(), vec![(e, 19)], 19, 10.0);
        assert!(notes(&s).is_empty(), "19 retransmits: below the minimum");
        let s = evaluate(section(), vec![(e, 20)], 20, 10.0);
        assert_eq!(s.status, Status::Ok, "a note does not raise the status");
        assert_eq!(
            notes(&s),
            ["retransmits concentrated on 10.0.0.5:443 (100.0% of 20): suspect that path or host"]
        );
    }

    #[test]
    fn concentration_share_boundary() {
        let (a, b) = (v4([10, 0, 0, 5], 443), v4([10, 0, 0, 6], 443));
        let s = evaluate(section(), vec![(a, 500), (b, 500)], 1000, 100.0);
        assert!(notes(&s).is_empty(), "exactly 50% is not concentrated");
        let s = evaluate(section(), vec![(a, 501), (b, 499)], 1000, 100.0);
        assert_eq!(notes(&s).len(), 1);
        assert!(notes(&s)[0].contains("10.0.0.5:443 (50.1% of 1000)"));
        // The share is of all retransmits, not only of the keyed ones.
        let s = evaluate(section(), vec![(a, 501)], 1002, 100.0);
        assert!(notes(&s).is_empty());
    }

    #[test]
    fn rate_boundary() {
        let a = v4([10, 0, 0, 5], 443);
        let s = evaluate(section(), vec![(a, 200)], 200, 2.0);
        assert!(
            s.findings.iter().all(|f| f.level != Level::Warn),
            "100/s is not above"
        );
        assert_eq!(s.status, Status::Ok);
        let s = evaluate(section(), vec![(a, 201)], 201, 2.0);
        assert_eq!(s.status, Status::Warn);
        let warn = s.findings.iter().find(|f| f.level == Level::Warn).unwrap();
        assert!(
            warn.message
                .starts_with("100.5 retransmits/s (top 10.0.0.5:443)"),
            "{}",
            warn.message
        );
    }
}
