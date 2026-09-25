//! `/proc/net/sockstat` and `/proc/net/sockstat6`: one line per protocol with `key value` pairs:
//!
//! ```text
//! sockets: used 412
//! TCP: inuse 38 orphan 0 tw 1204 alloc 45 mem 12
//! UDP: inuse 6 mem 3
//! FRAG: inuse 0 memory 0
//! ```
//!
//! `mem` values are in pages. Also parses the whitespace-separated numbers of sysctls such as
//! `ip_local_port_range` (`32768\t60999`) and `tcp_mem` (`88491\t117991\t176982`).

use super::{Result, err};

/// Protocol lines in file order, each with its `(key, value)` pairs in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sockstat(pub Vec<(String, Vec<(String, u64)>)>);

impl Sockstat {
    /// Value of `key` on the `protocol` line (without the colon, e.g. `TCP`, `TCP6`).
    pub fn get(&self, protocol: &str, key: &str) -> Option<u64> {
        self.fields(protocol)?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| *v)
    }

    pub fn fields(&self, protocol: &str) -> Option<&[(String, u64)]> {
        self.0
            .iter()
            .find(|(p, _)| p == protocol)
            .map(|(_, f)| f.as_slice())
    }

    pub fn has(&self, protocol: &str) -> bool {
        self.fields(protocol).is_some()
    }
}

pub fn parse(input: &str) -> Result<Sockstat> {
    let mut lines = Vec::new();
    for line in input.lines().filter(|l| !l.trim().is_empty()) {
        let Some((proto, rest)) = line.split_once(':') else {
            return err(format!("no protocol prefix in {line:?}"));
        };
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        if tokens.is_empty() || !tokens.len().is_multiple_of(2) {
            return err(format!("{proto}: expected key/value pairs, got {rest:?}"));
        }
        let mut fields = Vec::with_capacity(tokens.len() / 2);
        for kv in tokens.chunks(2) {
            let Ok(v) = kv[1].parse::<u64>() else {
                return err(format!("{proto}: bad value {:?} for {}", kv[1], kv[0]));
            };
            fields.push((kv[0].to_owned(), v));
        }
        lines.push((proto.trim().to_owned(), fields));
    }
    if lines.is_empty() {
        return err("no protocol lines");
    }
    Ok(Sockstat(lines))
}

/// Whitespace-separated unsigned numbers of a sysctl file, e.g. `tcp_mem`.
pub fn parse_numbers(input: &str) -> Result<Vec<u64>> {
    let values = input
        .split_whitespace()
        .map(|t| t.parse::<u64>())
        .collect::<std::result::Result<Vec<_>, _>>();
    match values {
        Ok(v) if !v.is_empty() => Ok(v),
        Ok(_) => err("no values"),
        Err(e) => err(format!("bad number in {:?}: {e}", input.trim())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sockstat_fixtures() {
        let s = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/net/sockstat"
        ))
        .unwrap();
        assert_eq!(s.get("TCP", "alloc"), Some(3));
        assert_eq!(s.get("TCP", "mem"), Some(73));
        assert_eq!(s.get("UDP", "mem"), Some(512));
        assert_eq!(s.get("sockets", "used"), Some(0));
        assert_eq!(s.get("TCP", "nope"), None);
        let protos: Vec<&str> = s.0.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(protos, ["sockets", "TCP", "UDP", "RAW", "FRAG"]);

        let s6 = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/net/sockstat6"
        ))
        .unwrap();
        assert_eq!(s6.get("TCP6", "inuse"), Some(0));
        assert_eq!(s6.get("TCP6", "tw"), None);
        assert_eq!(s6.get("FRAG6", "memory"), Some(0));
    }

    #[test]
    fn parses_legacy_sockstat() {
        let s = parse(include_str!(
            "../../tests/fixtures/linux-legacy/proc/net/sockstat"
        ))
        .unwrap();
        assert_eq!(s.get("TCP", "inuse"), Some(38));
        assert_eq!(s.get("TCP", "tw"), Some(1204));
        assert_eq!(s.get("TCP", "orphan"), Some(0));
        assert!(s.has("UDPLITE"));
        assert_eq!(
            s.fields("TCP").unwrap().first(),
            Some(&("inuse".to_owned(), 38))
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("").is_err());
        assert!(parse("TCP: inuse\n").is_err());
        assert!(parse("TCP: inuse x\n").is_err());
        assert!(parse("TCP inuse 1\n").is_err());
        assert!(parse("TCP:\n").is_err());
    }

    #[test]
    fn parses_sysctl_numbers() {
        assert_eq!(
            parse_numbers(include_str!(
                "../../tests/fixtures/linux-arm64/proc/sys/net/ipv4/ip_local_port_range"
            ))
            .unwrap(),
            vec![32768, 60999]
        );
        assert_eq!(
            parse_numbers(include_str!(
                "../../tests/fixtures/linux-legacy/proc/sys/net/ipv4/tcp_mem"
            ))
            .unwrap(),
            vec![88491, 117991, 176982]
        );
        assert_eq!(parse_numbers("65536\n").unwrap(), vec![65536]);
        assert!(parse_numbers("").is_err());
        assert!(parse_numbers("12 x\n").is_err());
        assert!(parse_numbers("-1\n").is_err());
    }
}
