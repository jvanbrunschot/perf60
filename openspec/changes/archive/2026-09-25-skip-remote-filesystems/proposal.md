# Proposal

## Why

`statvfs` on a hard-mounted NFS/CIFS share whose server is unreachable blocks forever, in
uninterruptible sleep, just as `df` hangs. Such a task can't even be killed. perf60 would never
print a report, which breaks "degrade, don't die", and it happens exactly when an operator most
needs a quick triage. The same applies to FUSE filesystems whose daemon is stuck.

## What Changes

- The filesystems check never calls `statvfs` on network or FUSE filesystems:
  - nfs, nfs4, cifs, smb3, smbfs, ceph, glusterfs, 9p, afs, lustre, gpfs, beegfs, ocfs2, gfs2,
    davfs
  - any `fuse.*` type (`fuseblk`, used for local ntfs-3g disks, is still checked)
- Skipped mounts are listed in a detail line, so the omission is visible.

## Capabilities

### New Capabilities

### Modified Capabilities
- `filesystem-check`: "Filesystem selection" excludes network and FUSE filesystems from statvfs.

## Impact

`src/checks/capacity.rs` only.
