# Proposal

## Why

Counters show averages. Gregg's own follow-up (*BPF Performance Tools*, 2019, ch. 3) adds a BCC
checklist of eBPF tools: execsnoop, runqlat, biolatency, tcpretrans and others. They show what
counters can't: short-lived processes, latency distributions and the outliers averages hide,
and which connections retransmit. perf60 should offer that on demand, with the same single
static binary and the same "degrade, don't die" behavior.

## What Changes

- New `--deep` option that adds eBPF probe sections to the report.
- A workspace with an eBPF crate (`perf60-ebpf`, one program per probe) and a shared
  `no_std` types crate (`perf60-common`), embedded into perf60 behind the `deep` cargo
  feature. Without the feature nothing changes, and `--deep` reports why it is unavailable.
- Probes attach to raw tracepoints and read kernel structs at offsets from the kernel's BTF.
  This needs no tracefs.
- Privileges are checked before loading, so a missing capability gives an actionable SKIPPED
  reason.
- First probe: `execsnoop` (execs by command and fork rate over the window).
- Build: `scripts/build-deep.sh` builds inside a rust container. `scripts/install-bpf-linker.sh`
  installs a pinned, SHA-256-verified bpf-linker release. CI builds and runs `--deep`
  natively on x86_64 and arm64, and releases ship with `deep`.
- Licensing: `perf60-ebpf` is Dual MIT/GPL-2.0, the rest stays MIT. LICENSE files are added.

## Capabilities

### New Capabilities
- `deep-mode`: the `--deep` option, probe lifecycle, privilege and availability handling, and
  build and licensing constraints.
- `execsnoop-probe`: the execsnoop section.

### Modified Capabilities
- `core-report`: the CLI gains `--deep`.

## Impact

Workspace `Cargo.toml`, `build.rs`, new crates `perf60-common/` and `perf60-ebpf/`,
`src/deep/`, `src/cli.rs`, `src/lib.rs`, `src/main.rs`, scripts, both workflows, README,
CLAUDE.md, LICENSE files. New dependencies behind `deep`: `aya` 0.14, `aya-build` 0.2
(build), `aya-ebpf` 0.2.1 (eBPF crate). The binary grows from about 0.6 MB to about 1.2 MB.
