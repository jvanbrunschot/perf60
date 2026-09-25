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
}

/// `/dev/kmsg` returns one record per `read()` and blocks at the end unless opened
/// non-blocking, in which case it returns `EAGAIN`.
#[cfg(target_os = "linux")]
fn read_live_kmsg() -> io::Result<Vec<String>> {
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
    fn describe_error_mentions_path() {
        let e = io::Error::new(io::ErrorKind::PermissionDenied, "x");
        assert_eq!(
            describe_error("/dev/kmsg", &e),
            "permission denied reading /dev/kmsg"
        );
    }
}
