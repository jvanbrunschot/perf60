//! `/proc/<pid>/stat`: `1234 (comm) S 1 ... utime stime ... num_threads 0 starttime ...`
//!
//! The comm field is in parentheses and may itself contain spaces and `)`, so the fields after
//! it are found by splitting at the LAST `)`.

use super::{ParseError, Result, err};

#[derive(Debug, Clone, PartialEq)]
pub struct PidStat {
    pub pid: u32,
    pub comm: String,
    /// `R`, `S`, `D`, `Z`, `T`, `I`, ...
    pub state: char,
    /// Clock ticks spent in user mode (field 14).
    pub utime: u64,
    /// Clock ticks spent in kernel mode (field 15).
    pub stime: u64,
    /// Field 20.
    pub num_threads: u64,
    /// Clock ticks after boot at which the process started (field 22). Detects pid reuse.
    pub starttime: u64,
}

pub fn parse(input: &str) -> Result<PidStat> {
    let (Some(open), Some(close)) = (input.find('('), input.rfind(')')) else {
        return err("pid stat: missing (comm)");
    };
    if close < open {
        return err("pid stat: malformed (comm)");
    }
    let pid = input[..open]
        .trim()
        .parse()
        .map_err(|_| ParseError("pid stat: bad pid".into()))?;
    let comm = input[open + 1..close].to_owned();
    // rest[0] is field 3 (state), so field N is rest[N - 3].
    let rest: Vec<&str> = input[close + 1..].split_whitespace().collect();
    if rest.len() < 20 {
        return err(format!(
            "pid stat: expected at least 22 fields, got {}",
            rest.len() + 2
        ));
    }
    let num = |field: usize| {
        let s = rest[field - 3];
        s.parse::<u64>()
            .map_err(|_| ParseError(format!("pid stat: bad field {field} '{s}'")))
    };
    let state = rest[0].chars().next().unwrap_or('?');
    Ok(PidStat {
        pid,
        comm,
        state,
        utime: num(14)?,
        stime: num(15)?,
        num_threads: num(20)?,
        starttime: num(22)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture_files() {
        // Every captured process parses; pid 1 is the capture shell, the others include sleeps.
        let proc = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/linux-arm64/proc"
        );
        let mut comms = Vec::new();
        for e in std::fs::read_dir(proc).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(pid) = name.parse::<u32>() else {
                continue;
            };
            let text = std::fs::read_to_string(e.path().join("stat")).unwrap();
            let p = parse(&text).unwrap();
            assert_eq!(p.pid, pid);
            assert!(p.starttime > 0 && p.num_threads >= 1);
            comms.push(p.comm);
        }
        assert!(comms.contains(&"sh".to_owned()), "{comms:?}");
        assert!(comms.contains(&"sleep".to_owned()), "{comms:?}");
    }

    #[test]
    fn comm_with_spaces_and_parens() {
        let p = parse("42 (my (weird) proc) R 1 42 42 0 -1 0 0 0 0 0 7 3 0 0 20 0 4 0 999 0 0\n")
            .unwrap();
        assert_eq!(p.pid, 42);
        assert_eq!(p.comm, "my (weird) proc");
        assert_eq!(p.state, 'R');
        assert_eq!((p.utime, p.stime), (7, 3));
        assert_eq!(p.num_threads, 4);
        assert_eq!(p.starttime, 999);
        // A comm that ends in ") " must not shift the fields either.
        let p = parse("7 (a) b) S 1 7 7 0 -1 0 0 0 0 0 5 6 0 0 20 0 1 0 11").unwrap();
        assert_eq!(p.comm, "a) b");
        assert_eq!((p.utime, p.stime, p.starttime), (5, 6, 11));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("").is_err());
        assert!(parse("1 sh S 0").is_err());
        assert!(parse("x (sh) S 0 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 1").is_err());
        assert!(parse("1 (sh) S 0 1 1").is_err());
        assert!(parse("1 (sh) S 0 1 1 0 -1 0 0 0 0 0 x 0 0 0 20 0 1 0 1").is_err());
    }
}
