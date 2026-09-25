# Tasks

## 1. Parser

- [x] 1.1 `procfs::diskstats` parser for 14/18/20-field lines, ignoring short lines; verify `procfs::diskstats::tests` (fixture `tests/fixtures/linux-arm64/proc/diskstats`, 14-field line, short line ignored, garbage rejected)

## 2. Disk check

- [x] 2.1 Whole-device selection via `/sys/block` with the name heuristic fallback; verify `checks::disk::tests::partitions_filtered_via_sys_block` and `checks::disk::tests::heuristic_without_sys_block`
- [x] 2.2 Window statistics, peak interval %util, idle omission, summary, details and metrics; verify `checks::disk::tests::busy_device_summary_and_metrics`, `peak_interval_util`, `idle_device_omitted` and `all_disks_idle`
- [x] 2.3 %util thresholds (60/90) and the misleading-util note; verify `checks::disk::tests::util_boundaries` and `util_note_only_for_non_rotational`
- [x] 2.4 Await thresholds by rotational type (10/50 ms, 50/200 ms); verify `checks::disk::tests::ssd_await_boundaries`, `hdd_await_boundaries` and `unknown_type_uses_ssd_thresholds`
- [x] 2.5 aqu-sz > 1 note; verify `checks::disk::tests::queue_note`
- [x] 2.6 SKIPPED when `/proc/diskstats` is unreadable; verify `checks::disk::tests::missing_diskstats_is_skipped`

## 3. Integration

- [x] 3.1 Register `disk` after `load` in `src/checks/mod.rs`; verify `tests/fixture_tree.rs` passes (disk not SKIPPED, `all disks idle (1 device)`)
- [x] 3.2 Run the static musl binary in an alpine container, idle and under `dd` load; verify the disk section reports idle, then vda utilization
