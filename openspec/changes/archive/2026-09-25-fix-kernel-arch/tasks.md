# Tasks

## 1. Architecture

- [x] 1.1 Parser for the release suffix and sysinfo lookup order (arch file → release suffix → build arch), `SysInfo.arch` plus `binary_arch`; verify unit tests for all three scenarios
- [x] 1.2 Text header renders the emulated hint; add `proc/sys/kernel/arch` to the linux-arm64 fixture; verify the fixture test and `cargo run --example fixture_report -- linux-legacy` shows `x86_64`
