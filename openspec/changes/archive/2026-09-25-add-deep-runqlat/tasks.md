# Tasks

## 1. Spec

- [x] 1.1 Proposal and `runqlat-probe` delta spec; verify `openspec validate add-deep-runqlat --strict`

## 2. eBPF program

- [x] 2.1 `perf60-ebpf/src/bin/runqlat.rs` (`sched_wakeup`, `sched_wakeup_new`, `sched_switch`; `PID_OFF`/`STATE_OFF` globals; `START` LRU hash, `HIST` per-CPU log2 histogram); verify `scripts/build-deep.sh aarch64-unknown-linux-musl` builds without warnings

## 3. User space

- [x] 3.1 `deep::runqlat::evaluate` (summary, details, metrics, p99 thresholds); verify `runqlat::tests::distribution_summary`, `no_wakeups`, `thresholds`
- [x] 3.2 `Runqlat` check (BTF offsets with `__state` → `state` fallback, attach, read `HIST`) registered after execsnoop in `deep::checks()`; verify `runqlat::tests::state_offset_fallback`, `cargo clippy --all-targets -- -D warnings` and `cargo test`
- [x] 3.3 Live runs: privileged idle and with 16 CPU spinners (p99 rises into ms), unprivileged (SKIPPED with the needs-root reason); verify via `docker logs`

## 4. Docs

- [x] 4.1 README deep-mode probe table row for `runqlat`; verify by reading the rendered table
