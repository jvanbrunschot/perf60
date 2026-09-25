//! `/proc/meminfo`: `MemTotal:  1986320 kB`. Values are returned in bytes.

use std::collections::BTreeMap;

use super::{Result, err};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MemInfo(pub BTreeMap<String, u64>);

impl MemInfo {
    /// Value in bytes, if present.
    pub fn get(&self, key: &str) -> Option<u64> {
        self.0.get(key).copied()
    }
}

pub fn parse(input: &str) -> Result<MemInfo> {
    let mut map = BTreeMap::new();
    for (k, v) in super::key_values(input, ':') {
        let mut parts = v.split_whitespace();
        let Some(Ok(n)) = parts.next().map(str::parse::<u64>) else {
            continue;
        };
        let mult = match parts.next() {
            Some("kB") => 1024,
            _ => 1,
        };
        map.insert(k.to_owned(), n * mult);
    }
    if !map.contains_key("MemTotal") {
        return err("meminfo: MemTotal missing");
    }
    Ok(MemInfo(map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let m = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/meminfo"
        ))
        .unwrap();
        assert!(m.get("MemTotal").unwrap() > m.get("MemAvailable").unwrap());
    }

    #[test]
    fn converts_kb_to_bytes() {
        let m = parse("MemTotal:  2 kB\nHugePages_Total:  3\n").unwrap();
        assert_eq!(m.get("MemTotal"), Some(2048));
        assert_eq!(m.get("HugePages_Total"), Some(3));
        assert_eq!(m.get("Nope"), None);
    }

    #[test]
    fn requires_mem_total() {
        assert!(parse("MemFree: 1 kB").is_err());
    }
}
