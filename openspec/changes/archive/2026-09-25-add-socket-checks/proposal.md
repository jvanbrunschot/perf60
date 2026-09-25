# Proposal

## Why

Running out of socket capacity is a classic outage cause, especially on NAT gateways and
Kubernetes nodes. A full conntrack table silently drops new connections, TIME_WAIT sockets can
use up the ephemeral port range so outbound connects fail, and orphaned sockets or TCP memory
near `tcp_mem` make the kernel reset or collapse connections. None of these show in the
`sar -n TCP,ETCP` rates. `ss -s` and `conntrack -S` are the usual first look, and neither is
installed on a minimal server.

## What Changes

- New `sockets` check ("Sockets and conntrack", equivalent `ss -s / conntrack -S`, resource
  network). It reads `/proc/net/sockstat` and `/proc/net/sockstat6` at every sample and judges
  the last one. The first sample is kept for a TIME_WAIT trend note.
- Capacity ratios against the kernel limits in `/proc/sys`:
  - conntrack `nf_conntrack_count` ÷ `nf_conntrack_max`: WARN above 80%, CRIT above 90%
  - TIME_WAIT sockets ÷ the `ip_local_port_range` size: WARN above 50%
  - orphans ÷ `tcp_max_orphans`: WARN above 50%
  - TCP memory pages ÷ the third `tcp_mem` value: WARN above 80%
- A missing sysctl drops only its finding and metric. Missing conntrack files mean conntrack is
  not in use. The check is SKIPPED only when `/proc/net/sockstat` is unusable.
- New pure parser `src/procfs/sockstat.rs` for sockstat files and whitespace-separated sysctl
  numbers.

## Capabilities

### New Capabilities
- `socket-check`: socket counts from sockstat and capacity findings for conntrack, ephemeral
  ports (TIME_WAIT), orphans and TCP memory.

### Modified Capabilities

## Impact

- New files `src/checks/sockets.rs` and `src/procfs/sockstat.rs`. One registry line in
  `src/checks/mod.rs` and one `pub mod` line in `src/procfs/mod.rs`.
- The report gets one more section, and the fixture trees render it (not SKIPPED).
- No new dependencies.
