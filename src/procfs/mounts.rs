//! `/proc/self/mounts` (fstab format): `source mountpoint fstype options dump pass`.
//! Fields use octal escapes (`\040` for a space), undone with [`crate::source::unescape_mount`].

use super::{Result, err};
use crate::source::unescape_mount;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub source: String,
    pub mount_point: String,
    pub fstype: String,
    pub options: Vec<String>,
}

impl Mount {
    /// Mounted with the `ro` option.
    pub fn read_only(&self) -> bool {
        self.options.iter().any(|o| o == "ro")
    }
}

/// Parse the mount table. Lines with fewer than 4 fields are skipped; a table without any valid
/// line is an error.
pub fn parse(input: &str) -> Result<Vec<Mount>> {
    let mounts: Vec<Mount> = input
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 4 {
                return None;
            }
            Some(Mount {
                source: unescape_mount(f[0]),
                mount_point: unescape_mount(f[1]),
                fstype: f[2].to_owned(),
                options: f[3].split(',').map(str::to_owned).collect(),
            })
        })
        .collect();
    if mounts.is_empty() && !input.trim().is_empty() {
        return err("mounts: no line with source, mount point, type and options");
    }
    Ok(mounts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arm64_fixture() {
        let m = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/self/mounts"
        ))
        .unwrap();
        assert_eq!(m.len(), 13);
        let root = m.iter().find(|m| m.mount_point == "/").unwrap();
        assert_eq!(
            (root.source.as_str(), root.fstype.as_str()),
            ("overlay", "overlay")
        );
        assert!(!root.read_only());
        // The SELinux context option contains a comma inside quotes; it is only split, never
        // interpreted, so `rw` is still found.
        assert!(root.options.contains(&"rw".to_owned()));
    }

    #[test]
    fn parses_legacy_fixture() {
        let m = parse(include_str!(
            "../../tests/fixtures/linux-legacy/proc/self/mounts"
        ))
        .unwrap();
        let app = m.iter().find(|m| m.mount_point == "/var/lib/app").unwrap();
        assert_eq!(app.source, "/dev/mapper/centos-data");
        assert_eq!(app.fstype, "xfs");
        assert_eq!(m[0].fstype, "rootfs");
    }

    #[test]
    fn unescapes_and_detects_ro() {
        let m = parse("/dev/sdb1 /mnt/my\\040disk ext4 ro,relatime 0 0\n").unwrap();
        assert_eq!(m[0].mount_point, "/mnt/my disk");
        assert!(m[0].read_only());
        // `ro` must be a whole option, not a prefix.
        let m = parse("x /a ext4 rw,rootcontext=foo 0 0\n").unwrap();
        assert!(!m[0].read_only());
    }

    #[test]
    fn short_lines_and_garbage() {
        let m = parse("junk\n/dev/sda1 / ext4 rw 0 0\n\n").unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(parse("").unwrap(), vec![]);
        assert!(parse("garbage\n").is_err());
    }
}
