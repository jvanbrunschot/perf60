//! `/proc/net/softnet_stat`: one row of hexadecimal counters per CPU. Column 0 is packets
//! processed, column 1 packets dropped because the per-CPU backlog was full, column 2 the
//! number of times `net_rx_action` ran out of budget or time (time_squeeze). Newer kernels
//! add more columns (RPS, flow limit, backlog length, CPU index); old ones have 10.
//!
//! ```text
//! 0005b317 00000000 00000045 00000000 ...
//! ```

use super::{Result, err};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SoftnetRow {
    pub processed: u64,
    pub dropped: u64,
    pub time_squeeze: u64,
}

pub fn parse(input: &str) -> Result<Vec<SoftnetRow>> {
    let mut rows = Vec::new();
    for line in input.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.is_empty() {
            continue;
        }
        if f.len() < 3 {
            return err(format!("softnet_stat: too few columns in '{line}'"));
        }
        let hex = |s: &str| u64::from_str_radix(s, 16);
        match (hex(f[0]), hex(f[1]), hex(f[2])) {
            (Ok(processed), Ok(dropped), Ok(time_squeeze)) => rows.push(SoftnetRow {
                processed,
                dropped,
                time_squeeze,
            }),
            _ => return err(format!("softnet_stat: bad hex in '{line}'")),
        }
    }
    if rows.is_empty() {
        return err("softnet_stat: no rows");
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let r = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/net/softnet_stat"
        ))
        .unwrap();
        assert_eq!(r.len(), 4);
        assert_eq!(
            r[0],
            SoftnetRow {
                processed: 0x5b317,
                dropped: 0,
                time_squeeze: 0x45,
            }
        );
        assert_eq!(r[3].time_squeeze, 1);
    }

    #[test]
    fn parses_legacy_ten_columns() {
        let r = parse(include_str!(
            "../../tests/fixtures/linux-legacy/proc/net/softnet_stat"
        ))
        .unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].processed, 0x0a1b2c3d);
        assert_eq!(r[1].time_squeeze, 0x10);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("").is_err());
        assert!(parse("00000001 00000002\n").is_err());
        assert!(parse("zz 0 0\n").is_err());
    }
}
