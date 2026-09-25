# Tasks

## 1. Parser

- [x] 1.1 `procfs::pressure` parser for `some`/`full` lines with optional `full`; verify `procfs::pressure::tests::{parses_fixture_files, parses_some_and_full, cpu_without_full_line, rejects_missing_some}`

## 2. Check

- [x] 2.1 `checks::pressure` sampling of `/proc/pressure/*` and window percentages; verify `checks::pressure::tests::{summary_and_metrics, zero_window_is_zero, partial_resources_reported}`
- [x] 2.2 Some thresholds (> 10 WARN, > 25 CRIT) with interpretations; verify `checks::pressure::tests::some_threshold_boundaries`
- [x] 2.3 Memory/io full > 5 WARN, system cpu full ignored; verify `checks::pressure::tests::{full_threshold, cpu_full_ignored_at_system_level}`
- [x] 2.4 Own cgroup v2 pressure (non-root path, or namespace root in a container); verify `checks::pressure::tests::{cgroup_pressure_reported, host_root_cgroup_not_reported, container_namespace_root_reported}`
- [x] 2.5 SKIPPED without PSI; verify `checks::pressure::tests::missing_psi_is_skipped`
- [x] 2.6 Register the check last in `src/checks/mod.rs`; verify `tests/fixture_tree.rs` (not SKIPPED on the fixture tree)

## 3. Verification

- [x] 3.1 Gates: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
- [x] 3.2 Static build `cargo build --release --target aarch64-unknown-linux-musl` and run in an idle and a CPU-throttled alpine container; verify cgroup cpu pressure shows in the throttled run
