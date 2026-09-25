# Tasks

## 1. Parsers

- [x] 1.1 `procfs::cgroup` parsers for `/proc/self/cgroup`, v1/v2 `cpu.stat`, `memory.events` and single-value files; verify `procfs::cgroup::tests::{parses_proc_self_cgroup, own_cgroup_versions, parses_v2_cpu_stat_fixture, parses_v1_cpu_stat_fixture, parses_memory_events, single_values, rejects_garbage}`

## 2. Own cgroup check

- [x] 2.1 Own cgroup resolution (v2, v1, hybrid, host root, container root, mount-root fallback); verify `checks::cgroup::tests::{v2_own_cgroup_path, host_root_cgroup_is_ok, container_root_is_evaluated, v1_controller_paths, hybrid_prefers_v1, v1_mounted_at_own_cgroup}`
- [x] 2.2 Window throttling, ms/s, `no CPU quota active` and since-creation detail; verify `checks::cgroup::tests::{throttled_share_and_time, no_quota_periods, legacy_tree_throttling_from_cpu_stat, legacy_fixture_since_creation}`
- [x] 2.3 Throttling thresholds 10/10.1/25/25.1; verify `checks::cgroup::tests::throttling_threshold_boundaries`
- [x] 2.4 Memory events (v2 oom_kill CRIT, max WARN, high note, old events OK) and v1 failcnt WARN; verify `checks::cgroup::tests::{oom_kill_is_crit, memory_max_is_warn, memory_high_is_note, old_events_are_ok, v1_failcnt_is_warn}`
- [x] 2.5 Summary format and metrics; verify `checks::cgroup::tests::summary_of_throttled_container`
- [x] 2.6 SKIPPED without readable cgroup files; verify `checks::cgroup::tests::skipped_without_cgroup_files`

## 3. Top cgroups check

- [x] 3.1 v2/v1 hierarchy walk with interface-file filter, depth 6 and 2000 cap; verify `checks::cgroup::tests::{top_depth_limit, top_cap_at_2000, top_v1_paths, interface_files_are_not_walked}`
- [x] 3.2 CPU%, memory and leaf-only listing with top-memory list and middle truncation; verify `checks::cgroup::tests::{top_lists_leaves_only, top_memory_list_when_different, top_shortens_long_paths}`
- [x] 3.3 Throttled leaves > 25% WARN, max 3 named plus `+N more`; verify `checks::cgroup::tests::{top_throttle_boundaries, top_names_three_throttled}`
- [x] 3.4 Summary (host and container namespace), metrics and SKIPPED; verify `checks::cgroup::tests::{top_host_summary, top_container_namespace, top_skipped_without_hierarchy}`

## 4. Integration

- [x] 4.1 Register `cgroup` and `cgroups-top` in `src/checks/mod.rs` and `pub mod cgroup` in `src/procfs/mod.rs`; verify `tests/fixture_tree.rs` (neither SKIPPED on `linux-arm64` or `linux-legacy`) and `cargo run --example fixture_report -- linux-legacy` / `-- linux-arm64`
- [x] 4.2 Gates: `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked`
- [x] 4.3 Static aarch64 musl build run in alpine containers: idle, `--cpus 0.5` throttled (cgroup CRIT), `--cgroupns host --privileged` (cgroups-top lists host cgroups), `-m 64m` memory hog (oom_kill or max event)
