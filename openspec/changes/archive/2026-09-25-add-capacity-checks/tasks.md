# Tasks

## 1. Parsers

- [x] 1.1 `procfs::mounts` parser for `/proc/self/mounts` (unescaped source and mount point, fstype, options, `ro`); verify `procfs::mounts::tests` (both fixtures, escapes, short lines)
- [x] 1.2 `procfs::limits` parsers for `/proc/<pid>/limits` (soft/hard, `unlimited`, missing units), `/proc/sys/fs/file-nr`, single numbers, `pids.max` and v1 controller lines of `/proc/self/cgroup`; verify `procfs::limits::tests`

## 2. Filesystems check

- [x] 2.1 Pseudo-fs skip, last-mount-wins per mount point, zero-block skip and escaped mount points; verify `checks::capacity::tests::pseudo_filesystems_are_skipped`, `zero_block_mount_is_ignored`, `escaped_mount_point`
- [x] 2.2 Bind-mount dedupe (same source, type, blocks, files → shortest mount point); verify `bind_mounts_are_deduplicated`, `different_filesystems_are_kept`
- [x] 2.3 df-style use%, inode use%, summary, details and metrics; verify `filesystems_summary`, `root_reserve_counts_as_unavailable`, `btrfs_without_inodes`
- [x] 2.4 Space thresholds (above 85% WARN, above 95% CRIT); verify `space_boundaries` (85.0, 85.1, 95.0, 95.1)
- [x] 2.5 Inode thresholds (above 85% WARN, above 95% CRIT); verify `inode_boundaries` (85.0, 85.1, 95.0, 95.1)
- [x] 2.6 Read-only note for writable types, not for tmpfs or a container's `/`; verify `readonly_ext4_note`, `readonly_tmpfs_no_note`, `readonly_root_in_container_no_note`
- [x] 2.7 SKIPPED without mount table or when statvfs fails everywhere; verify `missing_mounts_is_skipped`, `statvfs_failing_everywhere_is_skipped`, `statvfs_failing_for_one_mount`

## 3. Limits check

- [x] 3.1 Summary, details and metrics, unlimited file-max; verify `checks::capacity::tests::limits_summary_legacy`, `unlimited_file_max`
- [x] 3.2 File handle thresholds (above 80% WARN, above 90% CRIT); verify `file_nr_boundaries` (80.0, 80.1, 90.0, 90.1)
- [x] 3.3 Task thresholds against pid_max and threads-max; verify `tasks_pid_max_boundaries`, `tasks_threads_max_boundaries`
- [x] 3.4 cgroup pids (v2 walk with highest ratio, v1 pids controller, `max`); verify `cgroup_pids_boundaries`, `cgroup_pids_unlimited`, `cgroup_v1_pids`, `cgroup_v2_nested_pids`
- [x] 3.5 Per-process fds against the soft open-files limit (above 90% WARN, up to 3 named); verify `process_fd_boundaries`, `at_most_three_processes_named`, `unreadable_fd_dirs_are_ignored`
- [x] 3.6 SKIPPED only when nothing is readable; verify `nothing_readable_is_skipped`, `only_file_nr_readable`

## 4. Integration

- [x] 4.1 Register `filesystems` and `limits` in `checks::all()`; verify `cargo test --test fixture_tree` (not SKIPPED on either tree, no NaN) and `checks::capacity::tests::legacy_fixture_limits_warn`
- [x] 4.2 Gates; verify `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked`
- [x] 4.3 Static musl build in alpine containers: idle, a full 10 MiB tmpfs (filesystems CRIT for `/fill`) and a shell with 62 of 64 fds open (limits WARN naming `sh`); verify the container output
