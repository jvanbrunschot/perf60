//! `/proc/vmstat`: `pswpin 0` lines, one counter per line (mostly since boot).

use std::collections::BTreeMap;

use super::{Result, err};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VmStat(pub BTreeMap<String, u64>);

impl VmStat {
    pub fn get(&self, key: &str) -> Option<u64> {
        self.0.get(key).copied()
    }

    /// Pages scanned by direct reclaim: `pgscan_direct`, or on older kernels the sum of the
    /// per-zone `pgscan_direct_<zone>` counters. `pgscan_direct_throttle` counts throttling
    /// events, not pages, so it is never included.
    pub fn pgscan_direct(&self) -> Option<u64> {
        if let Some(v) = self.get("pgscan_direct") {
            return Some(v);
        }
        let zones: Vec<u64> = self
            .0
            .iter()
            .filter(|(k, _)| k.starts_with("pgscan_direct_") && *k != "pgscan_direct_throttle")
            .map(|(_, v)| *v)
            .collect();
        (!zones.is_empty()).then(|| zones.iter().fold(0u64, |a, b| a.saturating_add(*b)))
    }
}

pub fn parse(input: &str) -> Result<VmStat> {
    let mut map = BTreeMap::new();
    for line in input.lines() {
        let mut f = line.split_whitespace();
        if let (Some(k), Some(Ok(v))) = (f.next(), f.next().map(str::parse::<u64>)) {
            map.insert(k.to_owned(), v);
        }
    }
    if map.is_empty() {
        return err("vmstat: no counters");
    }
    Ok(VmStat(map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let v = parse(include_str!("../../tests/fixtures/linux-arm64/proc/vmstat")).unwrap();
        assert_eq!(v.get("pswpin"), Some(0));
        assert_eq!(v.get("pswpout"), Some(0));
        assert_eq!(v.get("oom_kill"), Some(2));
        assert_eq!(v.get("pgmajfault"), Some(1113));
        assert_eq!(v.pgscan_direct(), Some(6814));
        assert_eq!(v.get("nope"), None);
    }

    #[test]
    fn sums_per_zone_direct_scans_without_throttle() {
        let v = parse(
            "pgscan_direct_dma 1\npgscan_direct_dma32 10\npgscan_direct_normal 30\n\
             pgscan_direct_movable 0\npgscan_direct_throttle 99\n",
        )
        .unwrap();
        assert_eq!(v.pgscan_direct(), Some(41));
        assert_eq!(parse("pswpin 1\n").unwrap().pgscan_direct(), None);
        assert_eq!(
            parse("pgscan_direct 5\npgscan_direct_throttle 7\n")
                .unwrap()
                .pgscan_direct(),
            Some(5)
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("").is_err());
        assert!(parse("hello world\n").is_err());
        // Malformed lines are skipped, good ones kept.
        assert_eq!(parse("x y\npswpout 3\n").unwrap().get("pswpout"), Some(3));
    }
}
