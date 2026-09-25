# Tasks

## 1. Crate setup

- [x] 1.1 Add `libc`, `serde` (derive) and `serde_json` to Cargo.toml, with a release profile (lto, strip, panic=abort, opt-level="s"); verify `cargo build` succeeds
- [x] 1.2 Create module skeleton (`check`, `source`, `sample`, `checks`, `procfs`, `sysinfo`, `report`, `cli`); verify `cargo build`

## 2. Framework

- [x] 2.1 `source.rs`: `Source` trait, `FsSource` (root prefix, non-blocking `/dev/kmsg` reader on Linux), `MemSource`; verify unit tests for read/exists/read_dir
- [x] 2.2 `check.rs`: `Status`, `Finding`, `Section`, `Check` trait, `Context`; verify status-escalation and overall-status tests (spec: Check status, Overall status)
- [x] 2.3 `cli.rs`: argument parsing with defaults and errors; verify unit tests for defaults, `--count 0`, unknown flag
- [x] 2.4 `sample.rs`: sampling loop with monotonic timestamps; verify a test with interval 0 that each check receives count+1 samples

## 3. System info

- [x] 3.1 Pure parsers for os-release, cpuinfo model, cpu online list, meminfo totals, cgroup v1/v2 limits, uptime; verify fixture tests
- [x] 3.2 `sysinfo.rs` collection via `Source`, including effective CPUs, virtualization hint, block devices and NICs; verify a test against the fixture tree and the quota scenario (150000 100000 → 1.5)

## 4. Load check

- [x] 4.1 `procfs::loadavg` parser; verify fixture test
- [x] 4.2 `checks::load` with thresholds and trend notes; verify tests for every load-check scenario (OK at 8.0, WARN at 9.0, CRIT at 16.5, rising, missing → SKIPPED)

## 5. Reports and main

- [x] 5.1 Text renderer with color handling; verify a test that the output has no ANSI codes when color is disabled
- [x] 5.2 JSON renderer; verify a test that the output parses and contains all sections
- [x] 5.3 `main.rs`: orchestration, Linux-only guard, exit codes 0/1/2/3; verify `cargo run` on macOS prints the Linux-only message with exit code 3

## 6. Integration

- [x] 6.1 Capture a fixture tree from a Linux container into `tests/fixtures/linux-arm64/`; verify the end-to-end test over the fixture tree produces all sections
- [x] 6.2 `scripts/verify.sh`: build a static musl binary (rust-lld, see .cargo/config.toml) and run it in alpine, debian:stable-slim and busybox with text and `--json`; verify the script passes
