# Tasks

## 1. Parser

- [x] 1.1 `procfs::pid_stat::parse` for `/proc/<pid>/stat`, splitting comm at the last `)`; verify unit tests `parses_fixture_files` (fixture pids 1, 88, 89), `comm_with_spaces_and_parens` and `rejects_garbage`

## 2. Check

- [x] 2.1 `checks::processes` sampling: list `/proc`, read numeric pids, ignore vanished pids, exclude own pid, reset on starttime change; verify tests `process_disappearing_mid_window_is_ignored`, `own_pid_is_excluded` and `pid_reuse_resets_window`
- [x] 2.2 Per-process %usr/%sys/%cpu with clock ticks (sysconf on Linux, 100 in tests); verify test `cpu_percentages_over_window`
- [x] 2.3 Summary, top-5 details and metrics; verify tests `top_n_ordering`, `all_idle` and `comm_with_spaces_in_details`
- [x] 2.4 Findings: single process > 90% of capacity, D state note/WARN, zombies > 50; verify tests `single_process_over_90pct_capacity_warns`, `d_state_note_and_warn` and `zombies_over_50_warn`
- [x] 2.5 SKIPPED when `/proc` cannot be listed or no process is readable; verify tests `missing_proc_is_skipped` and `no_readable_process_is_skipped`
- [x] 2.6 Register `processes::Processes` after load in `checks::all()`; verify `cargo test` (tests/fixture_tree.rs) passes with the section not SKIPPED

## 3. Integration

- [x] 3.1 Gates pass: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
- [x] 3.2 Static musl build runs in an alpine container with a busy loop, which shows up as the top process at about 100%
