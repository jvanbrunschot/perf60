# Proposal

## Why

Two steps of Gregg's checklist, `free -m` and the si/so columns of `vmstat 1`, answer "is this
box out of memory, and is it paying for it by swapping?". Minimal servers and containers often
lack procps, and inside a container `free` reports host memory rather than the cgroup limit that
actually triggers the OOM killer. perf60 needs both answers from /proc and /sys.

## What Changes

- New pure parser for `/proc/vmstat` (`name value` lines).
- New `memory` check (equivalent of `free -m`): total, available, buffers, cached, shared and
  swap from `/proc/meminfo`; WARN/CRIT on low available memory; cgroup v2/v1 working set against
  the memory limit; OOM kills from `/proc/vmstat` `oom_kill`.
- New `swap` check (equivalent of `vmstat 1` si/so): swap-in/swap-out page rates over the
  sampling window, with notes for major faults and direct reclaim.
- Both checks are registered after the load check.

## Capabilities

### New Capabilities
- `memory-check`: memory capacity, cgroup memory limit and OOM-kill triage equivalent to `free -m`.
- `swap-check`: swapping, major-fault and direct-reclaim triage equivalent to the si/so columns of `vmstat 1`.

### Modified Capabilities

## Impact

New files `src/procfs/vmstat.rs`, `src/checks/memory.rs`; one line each in `src/procfs/mod.rs`
and `src/checks/mod.rs`. No new dependencies (`libc::sysconf` for the page size on Linux).
