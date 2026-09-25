# Tasks

## 1. Workspace and build

- [x] 1.1 Workspace (`perf60`, `perf60-common`, `perf60-ebpf`), optional `deep` feature, `build.rs` with pinned nightly; verify `cargo test` on macOS is unaffected
- [x] 1.2 `scripts/install-bpf-linker.sh` (version + per-arch SHA-256) and `scripts/build-deep.sh` (container build); verify both musl targets build and embed the eBPF object
- [x] 1.3 LICENSE (MIT) and `perf60-ebpf/LICENSE-{MIT,GPL2}`; verify the eBPF object declares `Dual MIT/GPL`

## 2. Framework

- [x] 2.1 `deep::btf` reader; verify `btf::tests` (synthetic blobs incl. anonymous unions and bitfields; running-kernel test on Linux)
- [x] 2.2 `deep::caps` capability check; verify `capability_masks`
- [x] 2.3 `deep::probe` loader for raw tracepoints with globals; `--deep` in CLI, `analyze_with`; `Unavailable` stand-in; verify `verbose_and_deep`, `unavailable_without_feature`

## 3. execsnoop

- [x] 3.1 eBPF program `perf60-ebpf/src/bin/execsnoop.rs` and `deep::execsnoop` evaluation; verify `execsnoop::tests` and a privileged container run with an exec storm (WARN, `true`/`date` counted) and an unprivileged run (needs-root reason)

## 4. CI and docs

- [x] 4.1 CI verify jobs: pinned nightly + bpf-linker, `clippy --features deep`, `verify.sh --deep`; release builds with `PERF60_FEATURES=deep`; verify actionlint and the PR's CI run
- [x] 4.2 README deep-mode section, license note; CLAUDE.md probe rules, dependency policy and build commands
