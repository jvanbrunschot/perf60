# Proposal

## Why

Containers and systemd services run under cgroup CPU quotas and memory limits, and the
host-wide tools in Gregg's checklist cannot see them: a pod that is throttled on `cpu.max` or
keeps hitting `memory.max` looks idle in `vmstat` and `mpstat`. CFS quota throttling is the most
common hidden slowdown on Kubernetes, and the kernel counts it in every cgroup's `cpu.stat`.
The same counters for every cgroup also give a `systemd-cgtop` view that shows which service or
container uses the machine.

## What Changes

- New pure parsers for `/proc/self/cgroup` lines, `cpu.stat` (v2 `usage_usec`/`throttled_usec`,
  v1 `throttled_time` in ns) and `memory.events`, plus single-value cgroup files.
- New `cgroup` check ("Own cgroup", equivalent `cgroup cpu.stat / memory.events`): resolves the
  process's own cgroup (v2, v1, host root, container namespace root), measures CPU quota
  throttling over the sampling window, and reports memory limit events (v2 `memory.events`,
  v1 `memory.failcnt`).
- New `cgroups-top` check ("Top cgroups", equivalent `systemd-cgtop`): walks the cgroup
  hierarchy on every sample, and lists the leaf cgroups with the most CPU and memory,
  naming the ones throttled on their quota.
- Thresholds: own throttling > 10% WARN, > 25% CRIT; `oom_kill` in the window CRIT; `max` in
  the window WARN; `high` in the window a note; v1 `failcnt` in the window WARN; any leaf cgroup
  throttled > 25% WARN in `cgroups-top`.

## Capabilities

### New Capabilities
- `cgroup-check`: CPU throttling and memory limit events of the process's own cgroup.
- `cgroups-top-check`: per-cgroup CPU and memory usage across the hierarchy (systemd-cgtop).

### Modified Capabilities

## Impact

New files `src/procfs/cgroup.rs` and `src/checks/cgroup.rs`, one `pub mod` line in
`src/procfs/mod.rs`, and a `pub mod` line plus two registry entries in `src/checks/mod.rs`. No
new dependencies. Both checks render on both fixture trees without new fixture files.
