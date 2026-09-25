# filesystem-check Specification

## Purpose
Equivalent of `df -h` and `df -i`: shows space and inode use of every real mounted filesystem,
so a filesystem that is (nearly) full or unexpectedly read-only is found before it causes
ENOSPC or EROFS errors.

## Requirements

### Requirement: Filesystem selection
The filesystems check SHALL read the mount table from `/proc/self/mounts`, undoing its octal
escapes in mount points (`\040` for a space). It SHALL ignore mounts of the pseudo filesystem
types proc, sysfs, devtmpfs, devpts, cgroup, cgroup2, mqueue, debugfs, tracefs, securityfs,
pstore, bpf, autofs, configfs, fusectl, hugetlbfs, binfmt_misc, nsfs, rpc_pipefs, selinuxfs,
efivarfs, ramfs, rootfs and squashfs (squashfs is always 100% full by design). When the same
mount point appears more than once, only the last (visible) mount SHALL be used. It SHALL call
`statvfs` for every remaining mount and ignore mounts that report 0 blocks.

#### Scenario: Pseudo filesystems are not reported
- **WHEN** the mount table lists proc at `/proc`, sysfs at `/sys`, cgroup2 at `/sys/fs/cgroup`, squashfs at `/snap/core/1` and xfs at `/`
- **THEN** only `/` is reported, as 1 filesystem

#### Scenario: Zero-block filesystem is ignored
- **WHEN** a tmpfs-like mount of an unlisted type reports 0 blocks from statvfs
- **THEN** it is not reported and does not count as a filesystem

#### Scenario: Escaped mount point
- **WHEN** the mount table lists ext4 at `/mnt/my\040disk`
- **THEN** statvfs is called for `/mnt/my disk` and the mount is reported as `/mnt/my disk`

### Requirement: Bind mounts are deduplicated
Mounts with the same source device, filesystem type, block count and inode count SHALL be
reported once, under the shortest mount point (the lexically first on a tie). Containers
bind-mount `/etc/hosts`, `/etc/hostname`, `/etc/resolv.conf` and `/run/secrets` from one
filesystem.

#### Scenario: Container bind mounts
- **WHEN** one tmpfs (same source, 99316 blocks, 819200 inodes) is mounted at `/run/secrets`, `/etc/hostname`, `/etc/resolv.conf` and `/etc/hosts`
- **THEN** it is reported once, as `/etc/hosts`

#### Scenario: Different filesystems with the same type are kept
- **WHEN** two xfs filesystems with different sources are mounted at `/` and `/var/lib/app`
- **THEN** both are reported

### Requirement: Usage summary, details and metrics
Space use SHALL be computed like df: used = blocks − bfree, use% = used ÷ (used + bavail) × 100,
so the root reserve counts as unavailable. Inode use SHALL be (files − ffree) ÷ files × 100, and
only when files > 0 (btrfs reports 0 inodes). Percentages are shown rounded up to a whole number,
like df. The summary SHALL be `<root> <use>% used (<avail> free), <n> filesystems, fullest
<mount> <use>%`, where `<root>` is `/` when it is reported (else the first reported mount) and the
`fullest` part is omitted when the fullest filesystem is `<root>` itself. There SHALL be one detail
line per filesystem with mount point, type, size, use%, available space and inode use%. The check
SHALL expose the metrics `<mount>.used_pct`, `<mount>.inodes_used_pct` (only when files > 0) and
`max_used_pct`. The values come from the last sample.

#### Scenario: Summary
- **WHEN** `/` (xfs, 4 KiB blocks, 10000000 blocks, 5500000 free and available) is 45% used, `/var/lib/app` is 80% used, `/boot` is 10% used and `/run` is 1% used
- **THEN** the summary is `/ 45% used (21 GiB free), 4 filesystems, fullest /var/lib/app 80%`, the status is OK and `max_used_pct` is 80

#### Scenario: Root reserve counts as unavailable
- **WHEN** a filesystem has 1000 blocks, 100 free and 50 available to unprivileged users
- **THEN** its use is 900 ÷ 950 = 94.7%, shown as 95%, and the status is WARN

#### Scenario: Filesystem without inode counts
- **WHEN** a btrfs filesystem reports 0 files
- **THEN** its detail line shows no inode percentage and there is no `inodes_used_pct` metric for it

### Requirement: Space thresholds
The filesystems check SHALL report WARN when a filesystem's space use is above 85% and CRIT when
it is above 95%. The finding SHALL name the mount point, the use% and the free space in human
units.

#### Scenario: Space exactly 85%
- **WHEN** a filesystem is 85.0% used
- **THEN** the status is OK

#### Scenario: Space above 85%
- **WHEN** a filesystem mounted at `/data` is 85.1% used
- **THEN** the status is WARN and the finding names `/data`, `86%` and the free space

#### Scenario: Space exactly 95%
- **WHEN** a filesystem is 95.0% used
- **THEN** the status is WARN

#### Scenario: Space above 95%
- **WHEN** a filesystem is 95.1% used
- **THEN** the status is CRIT

### Requirement: Inode thresholds
The filesystems check SHALL report WARN when a filesystem's inode use is above 85% and CRIT when
it is above 95%, naming the mount point, the inode use% and the free inodes.

#### Scenario: Inodes exactly 85%
- **WHEN** a filesystem uses 85.0% of its inodes
- **THEN** the status is OK

#### Scenario: Inodes above 85%
- **WHEN** a filesystem uses 85.1% of its inodes
- **THEN** the status is WARN

#### Scenario: Inodes exactly 95%
- **WHEN** a filesystem uses 95.0% of its inodes
- **THEN** the status is WARN

#### Scenario: Inodes above 95%
- **WHEN** a filesystem uses 95.1% of its inodes
- **THEN** the status is CRIT

### Requirement: Unexpected read-only mount
A mount with the `ro` option (or that statvfs reports read-only) whose type is normally writable
(ext2, ext3, ext4, xfs, btrfs, f2fs, vfat) SHALL get the note `mounted read-only (possibly
remounted after errors, check kernel-log)`. Inside a container, `/` is often read-only on
purpose, so it SHALL get no note there. The note does not change the status.

#### Scenario: ext4 mounted read-only
- **WHEN** an ext4 filesystem at `/data` is mounted `ro`
- **THEN** there is a note naming `/data` with `mounted read-only (possibly remounted after errors, check kernel-log)` and the status stays OK

#### Scenario: Read-only tmpfs
- **WHEN** a tmpfs is mounted `ro`
- **THEN** there is no read-only note

#### Scenario: Read-only root in a container
- **WHEN** perf60 runs in a container and `/` is an ext4 filesystem mounted `ro`
- **THEN** there is no read-only note

### Requirement: Filesystems SKIPPED behavior
The filesystems check SHALL be SKIPPED with the reason when `/proc/self/mounts` cannot be read,
when statvfs fails for every candidate mount (for example with permission denied), or when no
candidate mount remains.

#### Scenario: Mount table unreadable
- **WHEN** `/proc/self/mounts` does not exist
- **THEN** the section is SKIPPED and the reason names `/proc/self/mounts`

#### Scenario: statvfs fails everywhere
- **WHEN** the mount table lists `/` and `/data` but statvfs fails for both
- **THEN** the section is SKIPPED and the reason names statvfs

#### Scenario: statvfs fails for one mount
- **WHEN** statvfs fails for `/data` but succeeds for `/`
- **THEN** only `/` is reported and the section is not SKIPPED
