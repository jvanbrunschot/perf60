# Proposal

## Why

Step 7 of Gregg's checklist is `iostat -xz 1`: per-device IOPS, throughput, latency (await),
queue length and utilization, to spot saturated or slow disks. Minimal hosts rarely have
sysstat installed, so perf60 has to compute the same numbers from `/proc/diskstats`.

## What Changes

- New pure parser for `/proc/diskstats` that accepts the 14-, 18- (discard) and 20-field
  (flush) line formats.
- New `disk` check (`Disk I/O`, equivalent `iostat -xz 1`) that selects whole devices
  (via `/sys/block`, or a partition-name heuristic when `/sys/block` is missing), computes
  r/s, w/s, rkB/s, wkB/s, r_await, w_await, aqu-sz and %util over the sampling window plus
  the peak per-interval %util, and omits idle devices like `-z`.
- Thresholds: %util > 60 WARN, > 90 CRIT; await > 10/50 ms (non-rotational or unknown) or
  > 50/200 ms (rotational) WARN/CRIT; notes for aqu-sz > 1 and for %util on parallel devices.
- Registry entry after `load`.

## Capabilities

### New Capabilities
- `disk-check`: disk I/O triage equivalent to `iostat -xz 1`.

### Modified Capabilities

## Impact

New files `src/procfs/diskstats.rs` and `src/checks/disk.rs`, one `pub mod` line in
`src/procfs/mod.rs`, and two lines in the `src/checks/mod.rs` registry. No new dependencies.
