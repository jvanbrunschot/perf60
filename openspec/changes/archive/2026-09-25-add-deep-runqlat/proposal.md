# Proposal

## Why

CPU utilization and the load average say that CPUs are busy, not how long runnable tasks wait
for one. BCC `runqlat`, the second tool in Gregg's BCC checklist (*BPF Performance Tools*,
ch. 3), measures exactly that: the time from a task becoming runnable (wakeup or preemption)
until it runs. A long tail there is direct evidence of CPU saturation, even when averages look
fine.

## What Changes

- New eBPF program `perf60-ebpf/src/bin/runqlat.rs` on the raw tracepoints `sched_wakeup`,
  `sched_wakeup_new` and `sched_switch`. It stores the enqueue time per pid in an LRU hash and
  adds each wait, in microseconds, to a per-CPU log2 histogram.
- `task_struct.pid` and `task_struct.__state` (kernel 5.14+) or `task_struct.state` (older)
  offsets come from the running kernel's BTF and are passed as globals.
- New `--deep` section `runqlat` with p50/p99/max, a BCC-style distribution, metrics, and
  WARN/CRIT thresholds on the p99.
- README probe table gains a `runqlat` row.

## Capabilities

### New Capabilities
- `runqlat-probe`: the run queue latency section.

### Modified Capabilities
None.

## Impact

`perf60-ebpf/src/bin/runqlat.rs`, `src/deep/runqlat.rs`, two lines in `src/deep/mod.rs`,
README. No new dependencies and no framework changes.
