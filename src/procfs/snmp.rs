//! `/proc/net/snmp` and `/proc/net/netstat`: pairs of lines with the same prefix, a header line
//! of field names followed by a line of values:
//!
//! ```text
//! Tcp: RtoAlgorithm RtoMin RtoMax MaxConn ActiveOpens ...
//! Tcp: 1 200 120000 -1 0 ...
//! ```
//!
//! Values are signed because some fields (Tcp MaxConn) are -1.

use std::collections::BTreeMap;

use super::{Result, err};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snmp(pub BTreeMap<(String, String), i64>);

impl Snmp {
    /// Value of `field` in `section` (the prefix without the colon, e.g. `Tcp`, `TcpExt`).
    pub fn get(&self, section: &str, field: &str) -> Option<i64> {
        self.0.get(&(section.to_owned(), field.to_owned())).copied()
    }

    pub fn has_section(&self, section: &str) -> bool {
        self.0.keys().any(|(s, _)| s == section)
    }
}

fn value(s: &str) -> Option<i64> {
    // Counters are unsigned in the kernel; clamp the (theoretical) values above i64::MAX.
    s.parse::<i64>()
        .ok()
        .or_else(|| s.parse::<u64>().ok().map(|v| v.min(i64::MAX as u64) as i64))
}

pub fn parse(input: &str) -> Result<Snmp> {
    let mut map = BTreeMap::new();
    let mut header: Option<(&str, Vec<&str>)> = None;
    for line in input.lines() {
        let Some((prefix, rest)) = line.split_once(':') else {
            continue;
        };
        let prefix = prefix.trim();
        match header.take() {
            Some((h, names)) if h == prefix => {
                let values: Vec<&str> = rest.split_whitespace().collect();
                if values.len() != names.len() {
                    return err(format!(
                        "{prefix}: {} names but {} values",
                        names.len(),
                        values.len()
                    ));
                }
                for (n, v) in names.into_iter().zip(values) {
                    let Some(v) = value(v) else {
                        return err(format!("{prefix}: bad value {v:?} for {n}"));
                    };
                    map.insert((prefix.to_owned(), n.to_owned()), v);
                }
            }
            _ => header = Some((prefix, rest.split_whitespace().collect())),
        }
    }
    if map.is_empty() {
        return err("no header/value line pairs");
    }
    Ok(Snmp(map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_snmp_fixture() {
        let s = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/net/snmp"
        ))
        .unwrap();
        assert_eq!(s.get("Tcp", "MaxConn"), Some(-1));
        assert_eq!(s.get("Tcp", "RtoMax"), Some(120000));
        assert_eq!(s.get("Tcp", "OutSegs"), Some(0));
        assert_eq!(s.get("Ip", "DefaultTTL"), Some(64));
        assert!(s.has_section("Udp"));
        assert_eq!(s.get("Tcp", "Nope"), None);
    }

    #[test]
    fn parses_netstat_fixture() {
        let s = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/net/netstat"
        ))
        .unwrap();
        assert_eq!(s.get("TcpExt", "ListenOverflows"), Some(0));
        assert_eq!(s.get("TcpExt", "ListenDrops"), Some(0));
        assert!(s.has_section("IpExt") && s.has_section("MPTcpExt"));
        assert!(!s.has_section("Tcp"));
    }

    #[test]
    fn pairs_by_prefix() {
        let s = parse("Tcp: A B\nTcp: 1 -2\nTcpExt: A\nTcpExt: 7\n").unwrap();
        assert_eq!(s.get("Tcp", "A"), Some(1));
        assert_eq!(s.get("Tcp", "B"), Some(-2));
        assert_eq!(s.get("TcpExt", "A"), Some(7));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("").is_err());
        assert!(parse("Tcp: A B\nTcp: 1\n").is_err());
        assert!(parse("Tcp: A\nTcp: x\n").is_err());
    }
}
