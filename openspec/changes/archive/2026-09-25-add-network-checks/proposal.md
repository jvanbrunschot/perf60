# Proposal

## Why

Steps 8 and 9 of Gregg's checklist are `sar -n DEV 1` (interface throughput against link
speed) and `sar -n TCP,ETCP 1` (connection rates and retransmits). Minimal servers rarely have
sysstat installed, yet saturated NICs, interface errors, TCP retransmits and accept-queue
overflows are common causes of latency. perf60 needs both checks built from kernel counters.

## What Changes

- New pure parser for `/proc/net/dev` (per-interface receive/transmit counters).
- New pure parser for the paired header/value format shared by `/proc/net/snmp` and
  `/proc/net/netstat` (`Tcp:`, `TcpExt:`, ...), returning signed values per (section, field).
- New check `net` ("Network interfaces", `sar -n DEV 1`): per-interface rates over the sampling
  window, utilization against `/sys/class/net/<if>/speed`, and error/drop detection.
- New check `tcp` ("TCP", `sar -n TCP,ETCP 1`): active/passive open rates, retransmit rate and
  ratio, established connections, accept-queue overflows, and a since-boot retransmit note.
- Both checks are registered after `load`.

## Capabilities

### New Capabilities
- `net-interface-check`: network interface throughput, utilization and error triage equivalent to `sar -n DEV 1`.
- `tcp-check`: TCP connection, retransmit and listen-queue triage equivalent to `sar -n TCP,ETCP 1`.

### Modified Capabilities

## Impact

New files `src/procfs/net_dev.rs`, `src/procfs/snmp.rs`, `src/checks/net.rs`; one `pub mod`
line each in `src/procfs/mod.rs` and `src/checks/mod.rs`, two registry lines. No new
dependencies.
