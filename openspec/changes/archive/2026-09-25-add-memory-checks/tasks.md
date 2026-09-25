# Tasks

## 1. Parser

- [x] 1.1 `procfs::vmstat` parser for `name value` lines plus a direct-reclaim helper (`pgscan_direct` or the sum of `pgscan_direct_*`); verify `procfs::vmstat::tests` (fixture, per-zone sum, garbage input)

## 2. Memory check

- [x] 2.1 `checks::memory::Memory`: meminfo summary, details and metrics, MemAvailable fallback; verify `checks::memory::tests::summary_ok` and `old_kernel_without_memavailable`
- [x] 2.2 Low available thresholds (below 10% WARN, below 5% CRIT); verify `checks::memory::tests::available_boundaries` (10.0%, 9.9%, 5.0%, 4.9%)
- [x] 2.3 cgroup v2 walk and v1 fallback with working set = usage − inactive_file (above 90% WARN, above 95% CRIT); verify `cgroup_v2_detail`, `cgroup_v2_boundaries`, `cgroup_v2_no_limit`, `cgroup_v2_nested_tightest`, `cgroup_v1_limit`, `cgroup_v1_unlimited_ignored`
- [x] 2.4 OOM kills from vmstat (increase in window → CRIT, since boot → note); verify `oom_kill_during_window_is_crit`, `oom_kills_since_boot_note`, `no_oom_counter`
- [x] 2.5 SKIPPED without /proc/meminfo; verify `missing_meminfo_is_skipped`

## 3. Swap check

- [x] 3.1 `checks::memory::Swap`: si/so rates, byte rates via page size, no-swap summary, metrics; verify `swap_summary_ok` and `no_swap_configured`
- [x] 3.2 Swapping thresholds (above 0 WARN, above 256 pages/s CRIT); verify `swap_boundaries` (0, 1, 256, 257 pages/s)
- [x] 3.3 Major fault and direct reclaim notes; verify `majflt_note` and `direct_reclaim_per_zone_note`
- [x] 3.4 SKIPPED without /proc/vmstat; verify `missing_vmstat_is_skipped`

## 4. Integration

- [x] 4.1 Register `memory` and `swap` after `load` in `checks::all()`; verify `cargo test --test fixture_tree` (not SKIPPED, no NaN)
- [x] 4.2 Gates: verify `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
- [x] 4.3 Static musl build run in an alpine container with `-m 256m` and under memory pressure with `-m 128m`; verify both sections render with the cgroup detail
