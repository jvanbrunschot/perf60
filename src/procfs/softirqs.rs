//! `/proc/softirqs`: softirq counts per type and CPU. The header names the CPU columns.
//!
//! ```text
//!                     CPU0       CPU1       CPU2       CPU3
//!           HI:          0          0          0          0
//!       NET_RX:     275927       4160       4075       2919
//! ```

use std::collections::BTreeMap;

use super::{ParseError, Result, err};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SoftIrqs {
    /// CPU numbers of the columns, from the header.
    pub cpus: Vec<usize>,
    /// Counts per softirq name, one per column.
    pub rows: BTreeMap<String, Vec<u64>>,
}

impl SoftIrqs {
    pub fn get(&self, name: &str) -> Option<&[u64]> {
        self.rows.get(name).map(Vec::as_slice)
    }
}

pub fn parse(input: &str) -> Result<SoftIrqs> {
    let mut lines = input.lines();
    let header = lines.next().unwrap_or_default();
    let cpus: Vec<usize> = header
        .split_whitespace()
        .map(|c| {
            c.strip_prefix("CPU")
                .and_then(|n| n.parse().ok())
                .ok_or_else(|| ParseError(format!("softirqs: bad header column '{c}'")))
        })
        .collect::<Result<_>>()?;
    if cpus.is_empty() {
        return err("softirqs: no CPU columns");
    }
    let mut rows = BTreeMap::new();
    for line in lines {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let values: Vec<u64> = rest
            .split_whitespace()
            .map(|v| {
                v.parse()
                    .map_err(|_| ParseError(format!("softirqs: bad number '{v}'")))
            })
            .collect::<Result<_>>()?;
        if values.len() != cpus.len() {
            return err(format!(
                "softirqs: {} has {} values for {} cpus",
                name.trim(),
                values.len(),
                cpus.len()
            ));
        }
        rows.insert(name.trim().to_owned(), values);
    }
    if rows.is_empty() {
        return err("softirqs: no rows");
    }
    Ok(SoftIrqs { cpus, rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let s = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/softirqs"
        ))
        .unwrap();
        assert_eq!(s.cpus, vec![0, 1, 2, 3]);
        assert_eq!(s.get("NET_RX"), Some(&[275927, 4160, 4075, 2919][..]));
        assert!(s.get("TIMER").is_some());
        assert_eq!(s.get("NOPE"), None);
    }

    #[test]
    fn parses_legacy_fixture() {
        let s = parse(include_str!(
            "../../tests/fixtures/linux-legacy/proc/softirqs"
        ))
        .unwrap();
        assert_eq!(s.cpus, vec![0, 1]);
        assert_eq!(s.get("NET_RX"), Some(&[5123456, 5023456][..]));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("").is_err());
        assert!(parse("CPU0 CPU1\n").is_err());
        assert!(parse("CPU0 CPU1\nNET_RX: 1\n").is_err());
        assert!(parse("CPU0\nNET_RX: x\n").is_err());
        assert!(parse("hello\nNET_RX: 1\n").is_err());
    }
}
