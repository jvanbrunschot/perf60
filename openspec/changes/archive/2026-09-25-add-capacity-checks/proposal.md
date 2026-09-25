# Proposal

## Why

Gregg's 60-second checklist looks at utilization, saturation and errors of CPU, memory, disk and
network, but never at capacity limits. A full filesystem, a filesystem remounted read-only after
errors, a process at its open-files limit, or a cgroup at its pids limit cause outages
(ENOSPC, EROFS, EMFILE, EAGAIN on fork) while every checklist step looks healthy. `df` and
`ulimit` are also not a substitute: `ulimit` only shows the own shell, and `df` lists a
container's bind-mounted `/etc/hosts` five times.

## What Changes

- New pure parsers for the mount table (`/proc/self/mounts`), `/proc/<pid>/limits`,
  `/proc/sys/fs/file-nr`, `pids.max` and the v1 controller lines of `/proc/self/cgroup`.
- New `filesystems` check (equivalent of `df -h` / `df -i`): space and inode use per real
  filesystem via `statvfs`, pseudo filesystems skipped, bind mounts of one filesystem
  deduplicated, WARN above 85% and CRIT above 95%, a note for writable filesystem types mounted
  read-only.
- New `limits` check (equivalent of `ulimit` / `file-nr` / `pid_max`): system-wide file handles,
  tasks against `pid_max` and `threads-max`, the own cgroup's pids limit (v2 and v1), each WARN
  above 80% and CRIT above 90%; and every readable process's open fds against its soft
  open-files limit, WARN above 90%.
- Both checks are registered in `checks::all()` with resource `capacity`.

## Capabilities

### New Capabilities
- `filesystem-check`: filesystem space and inode capacity, and unexpected read-only mounts, equivalent to `df -h` / `df -i`.
- `limits-check`: kernel and process limits (file handles, pids, threads, cgroup pids, per-process open files), equivalent to `ulimit` / `file-nr` / `pid_max`.

### Modified Capabilities

## Impact

New files `src/checks/capacity.rs`, `src/procfs/mounts.rs`, `src/procfs/limits.rs`; one
`pub mod` line and two registry lines in `src/checks/mod.rs`, two `pub mod` lines in
`src/procfs/mod.rs`. Uses the existing `Source::statvfs`. No new dependencies.
