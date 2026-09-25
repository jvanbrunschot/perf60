# Tasks

## 1. Parser

- [x] 1.1 `procfs::stat` parser for the aggregate and per-CPU lines (4 to 10 columns), `ctxt`, `intr`, `procs_running`, `procs_blocked`, with `total()` excluding guest time and a saturating per-field delta; verify `procfs::stat::tests` (`parses_fixture`, `total_excludes_guest_time`, `tolerates_old_kernel_columns`, `since_is_per_field_and_saturating`, `rejects_garbage`)

## 2. cpu check (vmstat 1)

- [x] 2.1 `checks::cpu::Cpu` sampling, window percentages, r/b averages, cs/in rates, peak interval busy and the summary; verify `checks::cpu::tests::cpu_summary`, `cpu_guest_not_double_counted`, `cpu_peak_interval_busy`, `cpu_run_queue_excludes_self`, `cpu_identical_snapshots`
- [x] 2.2 Thresholds for run queue (> cpus WARN, > 2×cpus CRIT), wa (> 20 WARN, > 50 CRIT), st (> 10 WARN, > 25 CRIT) and busy (> 90 WARN); verify `cpu_run_queue_thresholds`, `cpu_iowait_thresholds`, `cpu_steal_thresholds`, `cpu_busy_threshold`
- [x] 2.3 Notes for sy > 20% and b > 0; verify `cpu_system_time_note`, `cpu_blocked_note`
- [x] 2.4 SKIPPED on missing `/proc/stat` and with one sample; verify `cpu_missing_stat_is_skipped`, `cpu_single_sample_is_skipped`

## 3. cpu-balance check (mpstat -P ALL 1)

- [x] 3.1 `checks::cpu::CpuBalance` per-CPU busy%, summary, top-4 detail lines and metrics; verify `balance_summary`, `balance_top_four_details`, `balance_single_cpu`, `balance_identical_snapshots`
- [x] 3.2 Hot CPU WARN (at least 2 CPUs, max > 90%, mean < 50%) and interrupt imbalance wording (irq+soft > 50% of busy); verify `balance_hot_cpu_threshold`, `balance_mean_threshold`, `balance_interrupt_imbalance`
- [x] 3.3 SKIPPED on missing `/proc/stat` and with one sample; verify `balance_missing_stat_is_skipped`, `balance_single_sample_is_skipped`

## 4. Integration

- [x] 4.1 Register `cpu` and `cpu-balance` after `load` in `src/checks/mod.rs` and add `pub mod stat` to `src/procfs/mod.rs`; verify `tests/fixture_tree.rs` (no section SKIPPED against the fixture tree)
- [x] 4.2 Static musl build run in an alpine container, idle and with 6 busy loops; verify both sections render and the loaded run reports a finding
