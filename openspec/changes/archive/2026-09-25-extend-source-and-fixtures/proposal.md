# Proposal

## Why

The next wave of checks goes beyond the 2015 checklist: capacity and limits, cgroup
saturation, latency signals, and hardware and platform checks, with a "likely bottleneck"
diagnosis after them. They need three things the framework lacks:
- **System calls** (`statvfs` for filesystem capacity, `adjtimex` for clock sync). These must be
  testable the same way file reads are.
- **A resource category per section**, so findings can be correlated.
- **Fixture trees** that contain the newer kernel interfaces, plus one with old-kernel formats.
  The first build never tested old formats.

Doing this first and serially keeps the five parallel feature changes from conflicting in
shared code.

## What Changes

- Every section carries a `resource` (cpu, memory, disk, network, capacity, hardware, kernel,
  pressure), also exposed in the JSON report.
- The `Source` abstraction gains `statvfs(path)` and `clock_status()`. Fixture trees provide them
  through `statvfs.txt` and `adjtimex.txt`, and tests through `MemSource` setters.
- `scripts/capture-fixture.sh` captures the newer interfaces:
  - schedstat, softnet_stat, sockstat, conntrack, cgroup cpu.stat/memory.events/pids/io.stat
  - fd/pid/thread limits, per-process limits and fds, softirqs, mounts, EDAC, taint
  - statvfs for every mount

  `tests/fixtures/linux-arm64` is recaptured.
- A synthetic `tests/fixtures/linux-legacy` tree with 3.x/4.x-era formats: cgroup v1, no PSI,
  no MemAvailable, 14-column diskstats, legacy `workingset_refault`. The fixture test runs every
  check against both trees.
- `scripts/resolve-registry.py` and `scripts/integrate-worktree.sh` make integrating parallel
  worktree branches reproducible. They were previously ad-hoc.

## Capabilities

### New Capabilities

### Modified Capabilities
- `core-report`: JSON section objects gain a `resource` field.

## Impact

`src/check.rs`, `src/source.rs`, every check's `Section::new` call, `tests/fixture_tree.rs`,
`tests/fixtures/`, `scripts/`, CLAUDE.md. No behavior change in the text report.
