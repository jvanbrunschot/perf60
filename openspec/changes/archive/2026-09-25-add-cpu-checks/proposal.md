# Proposal

## Why

Steps 4 and 5 of Gregg's checklist, `vmstat 1` and `mpstat -P ALL 1`, show where CPU time goes
(user, system, I/O wait, steal), whether the run queue is saturated, and whether one CPU is hot
while the others idle. Minimal servers often lack procps and sysstat, so perf60 has to derive the
same numbers from `/proc/stat`.

## What Changes

- A pure parser for `/proc/stat`: the aggregate `cpu` line, every `cpuN` line (tolerating the
  shorter column sets of old kernels), `ctxt`, `intr`, `procs_running` and `procs_blocked`.
- A `cpu` check (`vmstat 1` equivalent): window averages of us/sy/id/wa/st, run queue, blocked
  tasks, context switch and interrupt rates, peak interval busy%, with thresholds for run-queue
  saturation, I/O wait, steal and total busy time.
- A `cpu-balance` check (`mpstat -P ALL 1` equivalent): per-CPU busy% over the window, the
  busiest CPUs, and a warning for a single hot CPU (single-threaded bottleneck or interrupt
  imbalance).
- Both checks are registered after `load`, as the 3rd and 4th entries.

## Capabilities

### New Capabilities
- `cpu-check`: system-wide CPU utilization and saturation, equivalent to `vmstat 1`.
- `cpu-balance-check`: per-CPU utilization balance, equivalent to `mpstat -P ALL 1`.

### Modified Capabilities

## Impact

New files `src/procfs/stat.rs` and `src/checks/cpu.rs`, one `pub mod` line in
`src/procfs/mod.rs`, two registry lines in `src/checks/mod.rs`. No new dependencies. Both checks
read `/proc/stat` on every tick; the duplicated read is by design (see the core framework
design).
