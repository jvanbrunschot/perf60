//! Kernel and process limits: `/proc/<pid>/limits`, `/proc/sys/fs/file-nr`, single-number sysctls
//! (`pid_max`, `threads-max`), cgroup `pids.max`, and the v1 controller lines of
//! `/proc/self/cgroup`.

use super::{Result, err};

/// One row of `/proc/<pid>/limits`. `None` means `unlimited`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limit {
    pub name: String,
    pub soft: Option<u64>,
    pub hard: Option<u64>,
}

/// Column offsets of the kernel's `"%-25s %-20s %-20s %-10s"` format, used when the header is
/// missing.
const SOFT_COL: usize = 26;
const HARD_COL: usize = 47;

fn limit_value(s: &str) -> Result<Option<u64>> {
    match s.trim() {
        "unlimited" => Ok(None),
        v => v
            .parse()
            .map(Some)
            .map_err(|_| super::ParseError(format!("limits: bad value '{v}'"))),
    }
}

/// Parse `/proc/<pid>/limits`. The limit names contain spaces and the units column is sometimes
/// empty, so the columns are cut at the offsets of the `Soft Limit` and `Hard Limit` headers.
pub fn parse(input: &str) -> Result<Vec<Limit>> {
    let mut lines = input.lines().peekable();
    let (soft_col, hard_col) = match lines.peek() {
        Some(h) if h.starts_with("Limit") => {
            let cols = (h.find("Soft Limit"), h.find("Hard Limit"));
            lines.next();
            match cols {
                (Some(s), Some(h)) if s < h => (s, h),
                _ => return err("limits: header without Soft Limit / Hard Limit columns"),
            }
        }
        _ => (SOFT_COL, HARD_COL),
    };
    let mut out = Vec::new();
    for l in lines.filter(|l| !l.trim().is_empty()) {
        let (Some(name), Some(soft)) = (l.get(..soft_col), l.get(soft_col..hard_col.min(l.len())))
        else {
            return err(format!("limits: short line '{l}'"));
        };
        let hard = l
            .get(hard_col..)
            .and_then(|r| r.split_whitespace().next())
            .ok_or_else(|| super::ParseError(format!("limits: no hard limit in '{l}'")))?;
        out.push(Limit {
            name: name.trim().to_owned(),
            soft: limit_value(soft)?,
            hard: limit_value(hard)?,
        });
    }
    if out.is_empty() {
        return err("limits: no rows");
    }
    Ok(out)
}

/// The soft `Max open files` limit (`RLIMIT_NOFILE`); `Ok(None)` when unlimited.
pub fn max_open_files(input: &str) -> Result<Option<u64>> {
    parse(input)?
        .into_iter()
        .find(|l| l.name == "Max open files")
        .map(|l| l.soft)
        .ok_or_else(|| super::ParseError("limits: no 'Max open files' row".into()))
}

/// `/proc/sys/fs/file-nr`: allocated handles, allocated-but-unused (0 since 2.6), and file-max.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileNr {
    pub allocated: u64,
    pub unused: u64,
    pub max: u64,
}

pub fn parse_file_nr(input: &str) -> Result<FileNr> {
    let f: Vec<u64> = input
        .split_whitespace()
        .map(|s| s.parse())
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| super::ParseError(format!("file-nr: bad number in '{}'", input.trim())))?;
    match f[..] {
        [allocated, unused, max] => Ok(FileNr {
            allocated,
            unused,
            max,
        }),
        _ => err("file-nr: expected 3 fields"),
    }
}

/// A single-number file such as `/proc/sys/kernel/pid_max` or `pids.current`.
pub fn parse_number(input: &str) -> Result<u64> {
    input
        .trim()
        .parse()
        .map_err(|_| super::ParseError(format!("bad number '{}'", input.trim())))
}

/// cgroup `pids.max`: a number, or `max` for no limit (`Ok(None)`).
pub fn parse_pids_max(input: &str) -> Result<Option<u64>> {
    match input.trim() {
        "max" => Ok(None),
        v => parse_number(v).map(Some),
    }
}

/// The cgroup v1 path of `controller` in `/proc/self/cgroup` (`3:pids:/system.slice/x`).
pub fn cgroup1_path(input: &str, controller: &str) -> Option<String> {
    input.lines().find_map(|l| {
        let mut f = l.splitn(3, ':');
        let (_, ctrls, path) = (f.next()?, f.next()?, f.next()?);
        ctrls
            .split(',')
            .any(|c| c == controller)
            .then(|| path.trim().to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_fixture_limits() {
        let s = include_str!("../../tests/fixtures/linux-legacy/proc/4211/limits");
        let l = parse(s).unwrap();
        assert_eq!(l[0].name, "Max cpu time");
        assert_eq!((l[0].soft, l[0].hard), (None, None));
        assert_eq!(max_open_files(s).unwrap(), Some(256));
        let nofile = l.iter().find(|l| l.name == "Max open files").unwrap();
        assert_eq!(nofile.hard, Some(4096));
    }

    #[test]
    fn parses_arm64_fixture_limits() {
        let s = include_str!("../../tests/fixtures/linux-arm64/proc/1/limits");
        assert!(max_open_files(s).unwrap().unwrap() > 0);
    }

    #[test]
    fn rows_without_units_and_unlimited_soft() {
        let s = "Limit                     Soft Limit           Hard Limit           Units     \n\
                 Max open files            unlimited            unlimited            files     \n\
                 Max nice priority         0                    0                    \n\
                 Max realtime timeout      unlimited            unlimited            us        \n";
        let l = parse(s).unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(l[1].name, "Max nice priority");
        assert_eq!((l[1].soft, l[1].hard), (Some(0), Some(0)));
        assert_eq!(max_open_files(s).unwrap(), None);
    }

    #[test]
    fn limits_garbage() {
        assert!(parse("").is_err());
        assert!(parse("Limit Foo\nMax open files 1 2\n").is_err());
        assert!(
            max_open_files("Max cpu time              unlimited            unlimited").is_err()
        );
        assert!(parse("Max open files            lots                 4096").is_err());
    }

    #[test]
    fn file_nr() {
        let n = parse_file_nr(include_str!(
            "../../tests/fixtures/linux-legacy/proc/sys/fs/file-nr"
        ))
        .unwrap();
        assert_eq!(
            n,
            FileNr {
                allocated: 3072,
                unused: 0,
                max: 377212
            }
        );
        let n = parse_file_nr("1604\t0\t9223372036854775807\n").unwrap();
        assert_eq!(n.max, i64::MAX as u64);
        assert!(parse_file_nr("1 2").is_err());
        assert!(parse_file_nr("a b c").is_err());
    }

    #[test]
    fn numbers_and_pids_max() {
        assert_eq!(parse_number("32768\n").unwrap(), 32768);
        assert!(parse_number("x").is_err());
        assert_eq!(parse_pids_max("max\n").unwrap(), None);
        assert_eq!(parse_pids_max("4096\n").unwrap(), Some(4096));
        assert!(parse_pids_max("").is_err());
    }

    #[test]
    fn cgroup1_controller_path() {
        let s = include_str!("../../tests/fixtures/linux-legacy/proc/self/cgroup");
        assert_eq!(
            cgroup1_path(s, "pids").as_deref(),
            Some("/system.slice/app.service")
        );
        assert_eq!(
            cgroup1_path(s, "cpuacct").as_deref(),
            Some("/system.slice/app.service")
        );
        assert_eq!(cgroup1_path(s, "blkio"), None);
        assert_eq!(cgroup1_path("0::/\n", "pids"), None);
    }
}
