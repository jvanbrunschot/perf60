# Tasks

## 1. eBPF program

- [x] 1.1 `perf60-ebpf/src/bin/biolatency.rs`: `START` LRU hash (10240) on `block_rq_issue` (argument from `RQ_ARG`), per-disk `HIST` hash (4096 entries, sized in a comment) on `block_rq_complete`, disk name via BTF-offset globals; verify `scripts/build-deep.sh aarch64-unknown-linux-musl` builds without warnings

## 2. User space

- [x] 2.1 `deep::biolatency::evaluate` (summary, details, metrics, thresholds); verify `biolatency::tests::worst_disk_summary`, `worst_by_p99`, `empty`, `ssd_thresholds`, `hdd_thresholds`, `unknown_type_uses_ssd_limits`, `details_worst_disk_distribution_only`, `many_disks`
- [x] 2.2 `rq_issue_arg` from the kernel release; verify `biolatency::tests::rq_arg_by_release`
- [x] 2.3 `disk_offsets` BTF path choice; verify `biolatency::tests::btf_path_choice`
- [x] 2.4 `DiskBucket` key with the eBPF layout; verify `biolatency::tests::key_layout` (and the compile-time size assert in the eBPF program)
- [x] 2.5 `Biolatency` check (feature `deep`) registered in `deep::checks()` after execsnoop; verify `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked`

## 3. Verification and docs

- [x] 3.1 Privileged container runs: idle (`--deep -c 2 -i 1 -v`) and with 4 parallel `dd … oflag=direct` writers (a `vda` histogram); unprivileged run SKIPPED with the needs-root reason
- [x] 3.2 README `--deep` probe table row
