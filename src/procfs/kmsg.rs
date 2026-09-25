//! `/dev/kmsg` records: `prio,seq,usec,flags[,...];message`, optionally followed by
//! continuation lines that start with a space (` SUBSYSTEM=pci`).
//!
//! Also converts the `syslog(2)` / `klogctl` text format (`<prio>[ secs.usecs] message`) into
//! kmsg records so the check only has to understand one format.

use super::{ParseError, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Facility and level: `facility << 3 | level`.
    pub prio: u32,
    /// Log level, `prio & 7`: 0 emerg, 1 alert, 2 crit, 3 err, 4 warning, 5 notice, 6 info,
    /// 7 debug.
    pub level: u8,
    pub seq: u64,
    /// Microseconds since boot.
    pub usec: u64,
    pub message: String,
}

impl Record {
    pub fn secs(&self) -> f64 {
        self.usec as f64 / 1e6
    }
}

/// Parse one line. Continuation lines and blank lines give `Ok(None)`.
pub fn parse_record(line: &str) -> Result<Option<Record>> {
    if line.starts_with(' ') || line.trim().is_empty() {
        return Ok(None);
    }
    let bad = || ParseError(format!("kmsg: bad record '{}'", truncate(line, 60)));
    let (header, message) = line.split_once(';').ok_or_else(bad)?;
    let mut f = header.split(',');
    let mut num = || {
        f.next()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .ok_or_else(bad)
    };
    let prio = num()?;
    let seq = num()?;
    let usec = num()?;
    let prio = u32::try_from(prio).map_err(|_| bad())?;
    Ok(Some(Record {
        prio,
        level: (prio & 7) as u8,
        seq,
        usec,
        message: message.trim_end().to_owned(),
    }))
}

/// Parse every line of a kmsg dump. Returns the records and the number of malformed lines.
pub fn parse(input: &str) -> (Vec<Record>, usize) {
    let mut records = Vec::new();
    let mut bad = 0;
    for line in input.lines() {
        match parse_record(line) {
            Ok(Some(r)) => records.push(r),
            Ok(None) => {}
            Err(_) => bad += 1,
        }
    }
    (records, bad)
}

/// Convert `klogctl(SYSLOG_ACTION_READ_ALL)` output into kmsg records
/// (`prio,seq,usec,-;message`, `seq` being the line index). Lines without a `<prio>` prefix
/// (e.g. the first line after the ring buffer wrapped) are dropped.
pub fn from_syslog(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in input.lines() {
        let Some(rest) = line.strip_prefix('<') else {
            continue;
        };
        let Some((prio, rest)) = rest.split_once('>') else {
            continue;
        };
        let Ok(prio) = prio.parse::<u32>() else {
            continue;
        };
        let (usec, msg) = match rest
            .strip_prefix('[')
            .and_then(|r| r.split_once(']'))
            .and_then(|(ts, msg)| Some((parse_timestamp(ts)?, msg)))
        {
            Some((usec, msg)) => (usec, msg.strip_prefix(' ').unwrap_or(msg)),
            None => (0, rest),
        };
        out.push(format!("{prio},{},{usec},-;{msg}", out.len()));
    }
    out
}

/// `"   12.345678"` → 12345678 microseconds.
fn parse_timestamp(ts: &str) -> Option<u64> {
    let (secs, frac) = ts.trim().split_once('.')?;
    let secs: u64 = secs.parse().ok()?;
    if frac.is_empty() || frac.len() > 6 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let micros: u64 = format!("{frac:0<6}").parse().ok()?;
    Some(secs * 1_000_000 + micros)
}

/// Shorten `s` to at most `max` characters, appending `…` when cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/linux-arm64/dev/kmsg");

    #[test]
    fn parses_record() {
        let r =
            parse_record("6,1147,15177211736,-;podman0: port 3(veth2) entered forwarding state")
                .unwrap()
                .unwrap();
        assert_eq!(r.prio, 6);
        assert_eq!(r.level, 6);
        assert_eq!(r.seq, 1147);
        assert_eq!(r.usec, 15177211736);
        assert_eq!(r.message, "podman0: port 3(veth2) entered forwarding state");
        assert!((r.secs() - 15177.211736).abs() < 1e-9);
        // Facility bits are masked off for the level; extra header fields are allowed.
        let r = parse_record("30,923,5869838,-,caller=T1;systemd[1]: a;b")
            .unwrap()
            .unwrap();
        assert_eq!((r.prio, r.level), (30, 6));
        assert_eq!(r.message, "systemd[1]: a;b");
    }

    #[test]
    fn continuation_lines_are_ignored() {
        assert_eq!(parse_record(" SUBSYSTEM=pci").unwrap(), None);
        assert_eq!(parse_record("").unwrap(), None);
        let (r, bad) = parse("6,1,10,-;a\n SUBSYSTEM=pci\n DEVICE=+pci:0000:00:01.0\n3,2,20,-;b\n");
        assert_eq!(r.len(), 2);
        assert_eq!(bad, 0);
        assert_eq!(r[1].level, 3);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_record("hello world").is_err());
        assert!(parse_record("x,1,2,-;msg").is_err());
        assert!(parse_record("6,1;msg").is_err());
        assert_eq!(parse("junk\n6,1,2,-;ok\n").1, 1);
    }

    #[test]
    fn parses_fixture() {
        let (records, bad) = parse(FIXTURE);
        assert_eq!(bad, 0);
        let expected = FIXTURE
            .lines()
            .filter(|l| !l.starts_with(' ') && !l.trim().is_empty())
            .count();
        assert_eq!(records.len(), expected);
        assert!(!records.is_empty());
        assert!(records.windows(2).all(|w| w[1].seq > w[0].seq));
    }

    #[test]
    fn converts_syslog_lines() {
        let out = from_syslog(
            "<6>[    0.000000] Booting Linux\n<6>[   12.345678] eth0: link is down\n<14>[15177.2] user msg\n",
        );
        assert_eq!(
            out,
            vec![
                "6,0,0,-;Booting Linux",
                "6,1,12345678,-;eth0: link is down",
                "14,2,15177200000,-;user msg",
            ]
        );
        let r = parse_record(&out[1]).unwrap().unwrap();
        assert_eq!((r.prio, r.usec), (6, 12345678));
        assert_eq!(r.message, "eth0: link is down");
    }

    #[test]
    fn syslog_line_without_timestamp() {
        // printk.time=0, a line cut by ring buffer wrap-around, and a malformed timestamp.
        let out = from_syslog("ot prefix, dropped\n<3>disk failure\n<4>[abc] odd\n");
        assert_eq!(out, vec!["3,0,0,-;disk failure", "4,1,0,-;[abc] odd"]);
    }

    #[test]
    fn truncates_by_chars() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("ééééé", 3).chars().count(), 3);
    }
}
