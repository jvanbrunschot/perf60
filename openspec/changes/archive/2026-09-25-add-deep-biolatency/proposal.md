# Proposal

## Why

The `disk` check (iostat) reports the average await per device. Averages hide the outliers
that make applications stall: a disk with a 2 ms average can still take 200 ms for one I/O in a
hundred. Gregg's BCC checklist (*BPF Performance Tools*, ch. 3) uses `biolatency` for this: a
per-disk histogram of block I/O latency from issue to completion. perf60 `--deep` should
include it.

## What Changes

- New eBPF program `perf60-ebpf/src/bin/biolatency.rs` on the raw tracepoints
  `block_rq_issue` and `block_rq_complete`. It records the issue time per request in an LRU
  hash and adds the latency to a per-disk log2 histogram (µs) at completion.
- The request argument of `block_rq_issue` moved from index 1 to 0 in kernel 5.11. User space
  picks the index from `/proc/sys/kernel/osrelease` and passes it as a global.
- The disk name is read at BTF offsets: `request.q` → `request_queue.disk` on current kernels,
  `request.rq_disk` on older ones, then `gendisk.disk_name`.
- New section `biolatency` (Disk): the worst disk by p99 in the summary, per-disk lines, and
  the distribution of the worst disk. p99 thresholds depend on whether the disk is rotational.

## Capabilities

### New Capabilities
- `biolatency-probe`: the biolatency section.

### Modified Capabilities
None.

## Impact

`perf60-ebpf/src/bin/biolatency.rs`, `src/deep/biolatency.rs`, one module line and one
registration line in `src/deep/mod.rs`, and a README row. No new dependencies and no framework
changes.
