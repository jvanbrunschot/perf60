# Tasks

## 1. Parser

- [x] 1.1 `src/procfs/platform.rs`: `parse_counter` (single sysfs number), `parse_governor`, `cpu_index` / `mc_index` for `cpuN` / `mcN` directory names, and `decode_taint` / `Taint` with the 19 known letters and `?` for unknown bits; verified by `procfs::platform::tests` (`parses_counters`, `parses_governor`, `directory_indexes`, `decodes_taint_letters`, `unknown_taint_bits`, `parses_fixture_files`)
- [x] 1.2 Register `pub mod platform;` in `src/procfs/mod.rs`; verified by `cargo build --locked`

## 2. Check

- [x] 2.1 `src/checks/hardware.rs`: `Hardware` check sampling EDAC, thermal, cpufreq, taint and clock, with the joined summary, details and metrics; verified by `checks::hardware::tests::legacy_summary` and `container_summary`
- [x] 2.2 EDAC: UE 0 / 1 (CRIT), ΔCE (WARN), CE since boot (note), no controllers; verified by `edac_ue_boundary`, `edac_ce_rise_warns`, `edac_ce_since_boot_note` and `no_edac_controllers`
- [x] 2.3 Thermal throttling Δ (WARN), totals with packages counted once, no counters; verified by `thermal_rise_warns`, `thermal_total_detail` and `no_thermal_counters`
- [x] 2.4 cpufreq detail and `powersave` on 2 vs 4 CPUs, no cpufreq; verified by `powersave_cpu_boundary`, `mixed_governors` and `no_cpufreq`
- [x] 2.5 Taint 0 / 1 (P) / 128 (D) / 16 (M) / 4608 (W+O); verified by `taint_levels`
- [x] 2.6 Clock unsynchronized (WARN), maxerror 1000000 vs 1000001 µs, unavailable; verified by `clock_unsynced_warns`, `clock_maxerror_boundary` and `clock_unavailable`
- [x] 2.7 SKIPPED only when all five sources are missing; verified by `all_sources_absent_skipped`
- [x] 2.8 Register `Box::new(hardware::Hardware::default())` last in `src/checks/mod.rs`; verified by `checks::hardware::tests::fixture_trees`, `tests/fixture_tree.rs` (not SKIPPED on linux-arm64 and linux-legacy) and `cargo run --example fixture_report -- linux-legacy` / `-- linux-arm64`

## 3. Verification

- [x] 3.1 Gates: `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked`
- [x] 3.2 Static aarch64 musl binary in an alpine container, unprivileged and `--privileged`, with the clock values compared against `busybox adjtimex`
