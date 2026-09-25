# Design

## Context

This is a greenfield crate (see proposal.md). The binary must run on minimal Linux, while
development happens on macOS. Seven more checks will be built in parallel worktrees on top of
this foundation, so the extension surface must be small and must avoid merge conflicts.

## Goals / Non-Goals

**Goals:**
- One `Check` trait that every later check implements, with no edits to shared code other than
  a single registry line.
- Tests that run on macOS without a Linux VM, both for parsers and for full checks.

**Non-Goals:**
- Continuous/streaming mode, history and remote collection.
- Per-process I/O accounting, which needs root and `/proc/<pid>/io`.

## Decisions

- **Filesystem abstraction (`Source` trait).** Checks read through
  `trait Source { read_to_string(path), read_dir(path), exists(path), read_kmsg() }`.
  `FsSource { root }` reads the real filesystem under a root prefix (`/` in production, a
  fixture tree in tests). `MemSource` is an in-memory map that tests mutate between samples to
  simulate deltas. Alternative considered: parse directly with `std::fs`. Rejected because it
  makes delta logic untestable off-Linux.
- **Checks own their samples.** The engine calls `check.sample(src, t)` on every tick, with `t`
  as seconds since start from a monotonic clock. `check.evaluate(ctx)` is called once at the end.
  Each check keeps only the raw counters it needs, so there is no global snapshot struct that every
  feature would have to modify (a merge-conflict hotspot). Cost: `/proc/stat` is read by two
  checks per tick. That is negligible.
- **Errors → SKIPPED.** `sample` records the first I/O error. `evaluate` turns it into
  `Section::skipped(reason)`.
- **Status ordering.** `Ok < Warn < Crit`. `Skipped` is excluded from the overall status. Findings
  carry a status. `Note` findings are informational and never escalate.
- **CLI parsing by hand.** There are only six flags, so it isn't worth a dependency.
- **Colors.** Detect with `libc::isatty(1)` and honor `NO_COLOR`.
- **Non-Linux.** `main` checks `cfg!(target_os = "linux")` at runtime and exits 3. The whole crate
  still compiles, so `cargo test` works on macOS.
- **Static build.** `*-unknown-linux-musl` targets linked with Rust's bundled `rust-lld` (`.cargo/config.toml`). `cross` was rejected because it needs an x86_64 toolchain that cannot be installed on Apple Silicon hosts. `scripts/verify.sh` runs the
  result in `alpine`, `debian:stable-slim` and `busybox`.

## Risks / Trade-offs

- [Fixtures from one kernel don't cover old formats] → Parsers tolerate missing and extra columns.
  Fixtures include a trimmed old-kernel variant where the format differs.
- [Timing jitter at short intervals] → Use the measured elapsed time, not the nominal interval.
