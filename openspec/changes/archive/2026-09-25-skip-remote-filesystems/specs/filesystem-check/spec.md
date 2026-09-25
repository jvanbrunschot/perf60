# Spec Delta

## MODIFIED Requirements

### Requirement: Filesystem selection
The filesystems check SHALL read the mount table from `/proc/self/mounts`, undoing its octal
escapes in mount points (`\040` for a space). It SHALL ignore mounts of the pseudo filesystem
types proc, sysfs, devtmpfs, devpts, cgroup, cgroup2, mqueue, debugfs, tracefs, securityfs,
pstore, bpf, autofs, configfs, fusectl, hugetlbfs, binfmt_misc, nsfs, rpc_pipefs, selinuxfs,
efivarfs, ramfs, rootfs and squashfs (squashfs is always 100% full by design). When the same
mount point appears more than once, only the last (visible) mount SHALL be used. It SHALL NOT call
`statvfs` on network or FUSE filesystems (types nfs, nfs4, cifs, smb3, smbfs, ceph, glusterfs,
9p, afs, lustre, gpfs, beegfs, ocfs2, gfs2, davfs, and any `fuse.*` type), because statvfs
blocks indefinitely when their server or daemon is unresponsive. It SHALL list those mounts in
a detail line `not checked (network/FUSE, statvfs can hang): <mount points>`. It SHALL call
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

#### Scenario: Network filesystems are never stat'ed
- **WHEN** the mount table lists nfs4 at `/mnt/share`, fuse.sshfs at `/mnt/remote` and xfs at `/`
- **THEN** statvfs is only called for `/`, and a detail line lists `/mnt/share` and `/mnt/remote` as not checked

#### Scenario: fuseblk is still checked
- **WHEN** the mount table lists fuseblk at `/mnt/usb`
- **THEN** `/mnt/usb` is stat'ed and reported like any local filesystem
