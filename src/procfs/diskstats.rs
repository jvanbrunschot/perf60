//! `/proc/diskstats`: one line per block device.
//!
//! ```text
//!  253  0 vda 7085 991 848430 4806 36482 2447 876051 18802 0 6085 25279 0 0 0 0 2248 1670
//! ```
//!
//! Fields: major, minor, name, then 11 counters (reads, reads merged, sectors read, ms reading,
//! writes, writes merged, sectors written, ms writing, I/Os in progress, ms doing I/O, weighted
//! ms). Kernel 4.18 adds 4 discard counters (18 fields), 5.5 adds 2 flush counters (20 fields).
//! Very old kernels print partitions with only 4 counters; those lines are ignored.

use super::{ParseError, Result};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiskStat {
    pub major: u32,
    pub minor: u32,
    pub name: String,
    pub reads: u64,
    pub reads_merged: u64,
    pub sectors_read: u64,
    pub ms_reading: u64,
    pub writes: u64,
    pub writes_merged: u64,
    pub sectors_written: u64,
    pub ms_writing: u64,
    pub in_progress: u64,
    pub ms_io: u64,
    pub weighted_ms: u64,
}

pub fn parse(input: &str) -> Result<Vec<DiskStat>> {
    let mut out = Vec::new();
    for line in input.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 14 {
            continue;
        }
        let num = |i: usize| {
            f[i].parse::<u64>()
                .map_err(|_| ParseError(format!("diskstats: bad number '{}' for {}", f[i], f[2])))
        };
        let small = |i: usize| {
            f[i].parse::<u32>()
                .map_err(|_| ParseError(format!("diskstats: bad device number '{}'", f[i])))
        };
        out.push(DiskStat {
            major: small(0)?,
            minor: small(1)?,
            name: f[2].to_owned(),
            reads: num(3)?,
            reads_merged: num(4)?,
            sectors_read: num(5)?,
            ms_reading: num(6)?,
            writes: num(7)?,
            writes_merged: num(8)?,
            sectors_written: num(9)?,
            ms_writing: num(10)?,
            in_progress: num(11)?,
            ms_io: num(12)?,
            weighted_ms: num(13)?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let d = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/diskstats"
        ))
        .unwrap();
        let names: Vec<&str> = d.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["vda", "vda1", "vda2", "vda3", "vda4"]);
        assert_eq!(
            d[0],
            DiskStat {
                major: 253,
                minor: 0,
                name: "vda".into(),
                reads: 7085,
                reads_merged: 991,
                sectors_read: 848430,
                ms_reading: 4806,
                writes: 36482,
                writes_merged: 2447,
                sectors_written: 876051,
                ms_writing: 18802,
                in_progress: 0,
                ms_io: 6085,
                weighted_ms: 25279,
            }
        );
    }

    #[test]
    fn accepts_14_and_18_fields_and_skips_short_lines() {
        let d = parse(
            "   8  0 sda 1 2 3 4 5 6 7 8 9 10 11\n\
             8  1 sda1 1 2 3 4\n\
             8 16 sdb 1 2 3 4 5 6 7 8 9 10 11 0 0 0 0\n",
        )
        .unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].name, "sda");
        assert_eq!((d[0].reads, d[0].ms_io, d[0].weighted_ms), (1, 10, 11));
        assert_eq!(d[1].name, "sdb");
        assert_eq!(d[1].writes, 5);
        assert!(parse("").unwrap().is_empty());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("8 0 sda 1 2 x 4 5 6 7 8 9 10 11").is_err());
        assert!(parse("a 0 sda 1 2 3 4 5 6 7 8 9 10 11").is_err());
    }
}
