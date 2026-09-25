# Design

## Context

See proposal.md. perf60's rules are one static binary, no external commands, and "degrade,
don't die". The plan's first idea was classic tracepoints with field offsets from tracefs
`format` files. The first container test showed that aya only looks for tracefs at
`/sys/kernel/tracing` and `/sys/kernel/debug/tracing`, and tracefs is not mounted in containers
or on some minimal hosts. Mounting it ourselves would change system state.

## Goals / Non-Goals

**Goals:** probes that work on any kernel with BTF, including in `--privileged` containers,
without mounting anything. Tests that run on macOS without eBPF tooling. A pinned,
reproducible eBPF build.

**Non-Goals:** streaming per-event output (bpftrace/BCC style). CPU stack profiling, which needs
symbolization. Kernels older than 4.17, which lack raw tracepoints.

## Decisions

- **Raw tracepoints plus BTF offsets instead of classic tracepoints plus tracefs.**
  - Raw tracepoints attach by name via `bpf(BPF_RAW_TRACEPOINT_OPEN)`, so no tracefs is needed,
    and they avoid the argument copy of classic tracepoints.
  - Their arguments are the raw `TP_PROTO` values (e.g. `struct task_struct *`). Probes read
    fields with `bpf_probe_read_kernel` at offsets that user space resolves from
    `/sys/kernel/btf/vmlinux` and passes as program globals (`EbpfLoader::override_global`).
  - aya's CO-RE support for Rust programs is limited, and its BTF member lookup isn't public
    API. So `src/deep/btf.rs` is a small pure reader (~150 lines, unit-tested with synthetic
    blobs) that also descends into anonymous struct/union members (e.g. `sock_common`).
  - Alternatives considered: mounting tracefs (a system side effect), and kprobes (unstable
    function names across kernels).
- **Maps only, read at the end.** Probes aggregate into hash, array and per-CPU arrays, with
  log2 histograms in 64 buckets. There are no ring or perf buffers: there is no per-event
  user-space cost, and it works on kernels without ringbuf.
- **Probes are `Check`s.** They attach on the first `sample()`, the kernel counts during the
  window, `evaluate()` reads the maps, and dropping the check detaches it. The evaluation (map
  contents → `Section`) is a pure function that is always compiled and unit-tested; only
  loading and attaching is behind `feature = "deep"`.
- **One eBPF program file per probe.** `perf60-ebpf/src/bin/<probe>.rs` is auto-discovered by
  Cargo, and `aya-build` builds every bin into `$OUT_DIR/<probe>`. Parallel probe work touches
  no shared manifest.
- **Build toolchain.**
  - The nightly is pinned in `build.rs` (`EBPF_TOOLCHAIN`, with `rust-src` for
    `-Z build-std=core`). CI and `build-deep.sh` read it from there.
  - bpf-linker is installed from its static release binary, pinned by version and per-arch
    SHA-256. Building it from source needs a matching LLVM.
  - On macOS, `scripts/build-deep.sh` builds in a `rust` container with named volumes as
    caches, so nothing is installed on the host.
  - `perf60-ebpf` gets `opt-level = 3` (bpf-linker rejects `-Os`), `debug = 2` and no
    stripping (needed for BTF).
- **Privileges.** The effective capabilities come from `/proc/self/status` `CapEff`: we need
  CAP_SYS_ADMIN, or CAP_BPF with CAP_PERFMON. When they're missing, the section is SKIPPED
  with "needs root … --privileged" before any load. When they're unknown, loading is attempted
  and its error becomes the reason. `RLIMIT_MEMLOCK` is raised for pre-5.11 kernels.
- **Licensing.** The kernel refuses GPL-only helpers (e.g. `bpf_probe_read_kernel`) to
  non-GPL programs. The eBPF crate declares "Dual MIT/GPL", matching aya's template.

## Risks / Trade-offs

- **The eBPF verifier differs across kernels.** → The programs are kept small and bounded;
  CI runs them on two kernels (x86_64 and arm64 runners), plus the local VM.
- **Nightly pin rot.** → The pin is one constant, bumped deliberately. CI proves each bump.
- **Binary size roughly doubles (aya).** → Acceptable for the feature. A build without `deep`
  stays small.
- **Emulated x86_64 containers can't run eBPF** (qemu-user has no bpf syscall). → The x86_64
  `--deep` run is verified on CI's native runner only.
