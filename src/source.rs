//! Filesystem abstraction. All checks read kernel interfaces through [`Source`] so they can be
//! tested against fixture trees ([`FsSource`] with a custom root) or in-memory maps
//! ([`MemSource`]) on any OS.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub trait Source {
    /// Read a whole file, e.g. `/proc/stat`.
    fn read_to_string(&self, path: &str) -> io::Result<String>;
    /// Names of the entries in a directory (not full paths), sorted.
    fn read_dir(&self, path: &str) -> io::Result<Vec<String>>;
    fn exists(&self, path: &str) -> bool;
    /// Kernel log records (the `/dev/kmsg` format: `prio,seq,usec,flags;message`).
    fn read_kmsg(&self) -> io::Result<Vec<String>>;
    /// Filesystem capacity of the filesystem mounted at `path` (`statvfs(3)`).
    fn statvfs(&self, path: &str) -> io::Result<FsStat> {
        Err(unsupported(&format!("statvfs {path}")))
    }
    /// Kernel clock discipline state (`adjtimex(2)` with no modifications).
    fn clock_status(&self) -> io::Result<ClockStatus> {
        Err(unsupported("adjtimex"))
    }
}

/// Filesystem capacity, as returned by `statvfs(3)`. Block counts are in `frsize` units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FsStat {
    pub frsize: u64,
    pub blocks: u64,
    pub bfree: u64,
    /// Blocks available to unprivileged users (excludes the root reserve).
    pub bavail: u64,
    pub files: u64,
    pub ffree: u64,
    pub favail: u64,
    pub readonly: bool,
}

/// Kernel clock state from `adjtimex(2)`. Offsets and errors are in microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClockStatus {
    /// Return value of adjtimex: `TIME_OK` (0) … `TIME_ERROR` (5).
    pub state: i32,
    /// `timex.status` bits (`STA_*`).
    pub status: i32,
    pub offset_us: i64,
    pub maxerror_us: i64,
    pub esterror_us: i64,
}

impl ClockStatus {
    pub const TIME_ERROR: i32 = 5;
    pub const STA_UNSYNC: i32 = 0x0040;
    pub const STA_NANO: i32 = 0x2000;

    /// True unless the kernel reports the clock as unsynchronized.
    pub fn synchronized(&self) -> bool {
        self.state != Self::TIME_ERROR && self.status & Self::STA_UNSYNC == 0
    }
}

fn unsupported(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, format!("{what}: not supported"))
}

/// Parse a fixture `statvfs.txt`: one line per mount point,
/// `<mountpoint> <frsize> <blocks> <bfree> <bavail> <files> <ffree> <favail> <ro|rw>`.
/// Mount points use `/proc/mounts` escaping (`\040` for space). `#` starts a comment.
pub fn parse_statvfs_table(input: &str) -> Vec<(String, FsStat)> {
    input
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 9 {
                return None;
            }
            let n = |i: usize| f[i].parse::<u64>().ok();
            Some((
                unescape_mount(f[0]),
                FsStat {
                    frsize: n(1)?,
                    blocks: n(2)?,
                    bfree: n(3)?,
                    bavail: n(4)?,
                    files: n(5)?,
                    ffree: n(6)?,
                    favail: n(7)?,
                    readonly: f[8] == "ro",
                },
            ))
        })
        .collect()
}

/// Undo `/proc/mounts` octal escapes (`\040` space, `\011` tab, `\012` newline, `\134` backslash).
pub fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && i + 3 < b.len()
            && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c))
        {
            out.push((b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0'));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse a fixture `adjtimex.txt` (`key value` lines: state, status, offset, maxerror,
/// esterror; offset in µs, or ns when `STA_NANO` is set in status).
pub fn parse_clock_status(input: &str) -> Option<ClockStatus> {
    let mut c = ClockStatus::default();
    let mut seen = false;
    for (k, v) in input
        .lines()
        .filter_map(|l| l.split_once(char::is_whitespace))
    {
        let Ok(v) = v.trim().parse::<i64>() else {
            continue;
        };
        seen = true;
        match k {
            "state" => c.state = v as i32,
            "status" => c.status = v as i32,
            "offset" => c.offset_us = v,
            "maxerror" => c.maxerror_us = v,
            "esterror" => c.esterror_us = v,
            _ => {}
        }
    }
    if c.status & ClockStatus::STA_NANO != 0 {
        c.offset_us /= 1000;
    }
    seen.then_some(c)
}

/// Reads the real filesystem below `root` (`/` in production).
pub struct FsSource {
    root: PathBuf,
}

impl FsSource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        FsSource { root: root.into() }
    }

    pub fn root() -> Self {
        FsSource::new("/")
    }

    fn path(&self, path: &str) -> PathBuf {
        self.root.join(path.trim_start_matches('/'))
    }

    fn is_live(&self) -> bool {
        self.root == Path::new("/")
    }
}

impl Source for FsSource {
    fn read_to_string(&self, path: &str) -> io::Result<String> {
        std::fs::read_to_string(self.path(path))
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<String>> {
        let mut names: Vec<String> = std::fs::read_dir(self.path(path))?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        Ok(names)
    }

    fn exists(&self, path: &str) -> bool {
        self.path(path).exists()
    }

    fn read_kmsg(&self) -> io::Result<Vec<String>> {
        if self.is_live() {
            read_live_kmsg()
        } else {
            // Fixture trees store kmsg as a plain text file, one record per line.
            Ok(self
                .read_to_string("/dev/kmsg")?
                .lines()
                .map(str::to_owned)
                .collect())
        }
    }

    fn statvfs(&self, path: &str) -> io::Result<FsStat> {
        if self.is_live() {
            return live_statvfs(path);
        }
        let table = self.read_to_string("/statvfs.txt")?;
        parse_statvfs_table(&table)
            .into_iter()
            .find(|(m, _)| m == path)
            .map(|(_, st)| st)
            .ok_or_else(|| not_found(&format!("statvfs {path}")))
    }

    fn clock_status(&self) -> io::Result<ClockStatus> {
        if self.is_live() {
            return live_clock_status();
        }
        let text = self.read_to_string("/adjtimex.txt")?;
        parse_clock_status(&text)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "adjtimex.txt: no values"))
    }
}

#[cfg(target_os = "linux")]
fn live_statvfs(path: &str) -> io::Result<FsStat> {
    let c = std::ffi::CString::new(path)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
    // SAFETY: `st` is a plain C struct that statvfs fills in; `c` is a valid C string.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(FsStat {
        frsize: st.f_frsize as u64,
        blocks: st.f_blocks as u64,
        bfree: st.f_bfree as u64,
        bavail: st.f_bavail as u64,
        files: st.f_files as u64,
        ffree: st.f_ffree as u64,
        favail: st.f_favail as u64,
        readonly: st.f_flag & libc::ST_RDONLY != 0,
    })
}

#[cfg(not(target_os = "linux"))]
fn live_statvfs(path: &str) -> io::Result<FsStat> {
    Err(unsupported(&format!("statvfs {path}")))
}

#[cfg(target_os = "linux")]
fn live_clock_status() -> io::Result<ClockStatus> {
    // SAFETY: modes = 0 makes adjtimex read-only; `tx` is a plain C struct it fills in.
    let mut tx: libc::timex = unsafe { std::mem::zeroed() };
    let state = unsafe { libc::adjtimex(&mut tx) };
    if state < 0 {
        return Err(io::Error::last_os_error());
    }
    let status = tx.status as i32;
    let mut offset_us = tx.offset as i64;
    if status & ClockStatus::STA_NANO != 0 {
        offset_us /= 1000;
    }
    Ok(ClockStatus {
        state,
        status,
        offset_us,
        maxerror_us: tx.maxerror as i64,
        esterror_us: tx.esterror as i64,
    })
}

#[cfg(not(target_os = "linux"))]
fn live_clock_status() -> io::Result<ClockStatus> {
    Err(unsupported("adjtimex"))
}

/// Read `/dev/kmsg`; when that fails (device missing, no permission) fall back to the
/// `syslog(2)` syscall. If both fail, the `/dev/kmsg` error is returned.
#[cfg(target_os = "linux")]
fn read_live_kmsg() -> io::Result<Vec<String>> {
    read_dev_kmsg().or_else(|e| read_klogctl().map_err(|_| e))
}

/// `klogctl(SYSLOG_ACTION_READ_ALL)`, converted to the `/dev/kmsg` record format.
#[cfg(target_os = "linux")]
fn read_klogctl() -> io::Result<Vec<String>> {
    const SYSLOG_ACTION_READ_ALL: libc::c_int = 3;
    const SYSLOG_ACTION_SIZE_BUFFER: libc::c_int = 10;

    // SAFETY: SIZE_BUFFER ignores the buffer arguments.
    let size = unsafe { libc::klogctl(SYSLOG_ACTION_SIZE_BUFFER, std::ptr::null_mut(), 0) };
    if size < 0 {
        return Err(io::Error::last_os_error());
    }
    // The text output adds a `<prio>[ timestamp] ` prefix per line, so leave some headroom.
    let len = (size as usize).clamp(1 << 14, 1 << 26) * 2;
    let mut buf = vec![0u8; len];
    // SAFETY: `buf` is valid for writes of `len` bytes, and `len` fits in c_int.
    let n = unsafe { libc::klogctl(SYSLOG_ACTION_READ_ALL, buf.as_mut_ptr().cast(), len as _) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    buf.truncate(n as usize);
    Ok(crate::procfs::kmsg::from_syslog(&String::from_utf8_lossy(
        &buf,
    )))
}

/// `/dev/kmsg` returns one record per `read()` and blocks at the end unless opened
/// non-blocking, in which case it returns `EAGAIN`.
#[cfg(target_os = "linux")]
fn read_dev_kmsg() -> io::Result<Vec<String>> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;

    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open("/dev/kmsg")?;
    let mut records = Vec::new();
    let mut buf = vec![0u8; 8192];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => records.push(String::from_utf8_lossy(&buf[..n]).trim_end().to_owned()),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            // EPIPE: the record was overwritten while we read. Skip it and continue.
            Err(e) if e.raw_os_error() == Some(libc::EPIPE) => continue,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                if records.is_empty() {
                    return Err(e);
                }
                break;
            }
        }
    }
    Ok(records)
}

#[cfg(not(target_os = "linux"))]
fn read_live_kmsg() -> io::Result<Vec<String>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "/dev/kmsg is Linux only",
    ))
}

/// In-memory source for tests. Mutate with [`MemSource::set`] between samples to simulate
/// counters moving.
#[derive(Default)]
pub struct MemSource {
    files: Mutex<BTreeMap<String, String>>,
    statvfs: Mutex<BTreeMap<String, FsStat>>,
    clock: Mutex<Option<ClockStatus>>,
}

impl MemSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(self, path: &str, content: &str) -> Self {
        self.set(path, content);
        self
    }

    pub fn set(&self, path: &str, content: &str) {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_owned(), content.to_owned());
    }

    pub fn remove(&self, path: &str) {
        self.files.lock().unwrap().remove(path);
    }

    pub fn set_statvfs(&self, mountpoint: &str, st: FsStat) {
        self.statvfs
            .lock()
            .unwrap()
            .insert(mountpoint.to_owned(), st);
    }

    pub fn set_clock(&self, c: ClockStatus) {
        *self.clock.lock().unwrap() = Some(c);
    }
}

fn not_found(path: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("{path}: not found"))
}

impl Source for MemSource {
    fn read_to_string(&self, path: &str) -> io::Result<String> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| not_found(path))
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<String>> {
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let files = self.files.lock().unwrap();
        let names: Vec<String> = files
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .map(|rest| rest.split('/').next().unwrap_or(rest).to_owned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if names.is_empty() {
            return Err(not_found(path));
        }
        Ok(names)
    }

    fn exists(&self, path: &str) -> bool {
        let files = self.files.lock().unwrap();
        let prefix = format!("{}/", path.trim_end_matches('/'));
        files.contains_key(path) || files.keys().any(|k| k.starts_with(&prefix))
    }

    fn read_kmsg(&self) -> io::Result<Vec<String>> {
        Ok(self
            .read_to_string("/dev/kmsg")?
            .lines()
            .map(str::to_owned)
            .collect())
    }

    fn statvfs(&self, path: &str) -> io::Result<FsStat> {
        self.statvfs
            .lock()
            .unwrap()
            .get(path)
            .copied()
            .ok_or_else(|| not_found(&format!("statvfs {path}")))
    }

    fn clock_status(&self) -> io::Result<ClockStatus> {
        self.clock
            .lock()
            .unwrap()
            .ok_or_else(|| not_found("adjtimex"))
    }
}

/// Human-readable reason for a failed read, used in SKIPPED sections.
pub fn describe_error(path: &str, e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::NotFound => format!("{path} not available"),
        io::ErrorKind::PermissionDenied => format!("permission denied reading {path}"),
        _ => format!("cannot read {path}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_source_read_dir_lists_children_once() {
        let src = MemSource::new()
            .with("/proc/1/stat", "a")
            .with("/proc/1/status", "b")
            .with("/proc/22/stat", "c")
            .with("/proc/stat", "d");
        assert_eq!(src.read_dir("/proc").unwrap(), vec!["1", "22", "stat"]);
        assert!(src.exists("/proc/22"));
        assert!(!src.exists("/proc/3"));
        assert_eq!(src.read_to_string("/proc/stat").unwrap(), "d");
        assert!(src.read_to_string("/nope").is_err());
    }

    #[test]
    fn fs_source_uses_root_prefix() {
        let src = FsSource::new(env!("CARGO_MANIFEST_DIR"));
        assert!(src.exists("/Cargo.toml"));
        assert!(src.read_to_string("Cargo.toml").unwrap().contains("perf60"));
        assert!(
            src.read_dir("/src")
                .unwrap()
                .contains(&"main.rs".to_owned())
        );
    }

    #[test]
    fn statvfs_table_parses_and_unescapes() {
        let t = "# mountpoint frsize blocks bfree bavail files ffree favail ro\n\
                 / 4096 1000 400 350 5000 4000 4000 rw\n\
                 /mnt/my\\040disk 4096 10 0 0 100 1 1 ro\n\
                 bad line\n";
        let rows = parse_statvfs_table(t);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "/");
        assert_eq!(rows[0].1.bavail, 350);
        assert!(!rows[0].1.readonly);
        assert_eq!(rows[1].0, "/mnt/my disk");
        assert!(rows[1].1.readonly);
        assert_eq!(unescape_mount("a\\134b\\011c"), "a\\b\tc");
    }

    #[test]
    fn clock_status_parses_and_normalizes_nano() {
        let c =
            parse_clock_status("state 0\nstatus 8193\noffset -250000\nmaxerror 16000\n").unwrap();
        // 8193 = STA_PLL | STA_NANO: offset is in ns and becomes -250 µs.
        assert_eq!(c.offset_us, -250);
        assert!(c.synchronized());
        let unsync = parse_clock_status("state 5\nstatus 64\n").unwrap();
        assert!(!unsync.synchronized());
        assert_eq!(parse_clock_status(""), None);
    }

    #[test]
    fn mem_source_statvfs_and_clock() {
        let src = MemSource::new();
        assert!(src.statvfs("/").is_err());
        assert!(src.clock_status().is_err());
        let st = FsStat {
            frsize: 4096,
            blocks: 10,
            ..Default::default()
        };
        src.set_statvfs("/", st);
        src.set_clock(ClockStatus {
            state: 0,
            ..Default::default()
        });
        assert_eq!(src.statvfs("/").unwrap(), st);
        assert!(src.clock_status().unwrap().synchronized());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn live_syscalls_work() {
        let src = FsSource::root();
        let st = src.statvfs("/").unwrap();
        assert!(st.frsize > 0 && st.blocks > 0 && st.bavail <= st.bfree);
        assert!(src.statvfs("/definitely/not/here").is_err());
        let c = src.clock_status().unwrap();
        assert!((0..=5).contains(&c.state), "{c:?}");
    }

    #[test]
    fn describe_error_mentions_path() {
        let e = io::Error::new(io::ErrorKind::PermissionDenied, "x");
        assert_eq!(
            describe_error("/dev/kmsg", &e),
            "permission denied reading /dev/kmsg"
        );
    }
}
