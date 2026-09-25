# Proposal

## Why

The 60-second checklist only shows averages: `vmstat` says the CPUs are busy, not how long
tasks wait for one; `pidstat` misses processes that live shorter than a sample; `free` and
`sar` miss thrashing, backlog drops and UDP buffer overflows. The kernel already counts these
latency and saturation signals, so perf60 can report them without BPF.

## What Changes

- `cpu`: run-queue latency (runqlat-lite) from `/proc/schedstat`: average wait per timeslice
  over the window and the worst CPU. WARN above 2 ms, CRIT above 10 ms.
- `processes`: fork rate from the `processes` counter in `/proc/stat`. Note above 100/s, WARN
  above 1000/s.
- `memory`: `/proc/vmstat` is sampled every tick. Page-cache refault rate (WARN above 1000
  pages/s), memory compaction stalls (note) and, on NUMA machines, the NUMA miss ratio (note
  above 10%).
- `net`: per-CPU backlog drops (WARN) and NAPI time squeezes (note) from
  `/proc/net/softnet_stat`, and NET_RX softirqs concentrated on one CPU (note above 80%) from
  `/proc/softirqs`.
- `tcp`: UDP `RcvbufErrors`, `SndbufErrors` and `InErrors` (Udp and UdpLite) and TcpExt
  `TCPBacklogDrop` and `TCPAbortOnMemory` during the window (WARN).
- New pure parsers for `/proc/schedstat`, `/proc/net/softnet_stat` and `/proc/softirqs`; the
  `/proc/stat` parser gains the `processes` counter.
- Every signal is optional: a missing or unparsable source omits that signal only. It never
  skips the section.

## Capabilities

### New Capabilities

### Modified Capabilities
- `cpu-check`: new requirement for run-queue latency.
- `process-check`: new requirement for the fork rate.
- `memory-check`: new requirements for page-cache thrashing, compaction stalls and NUMA misses.
- `net-interface-check`: new requirements for softnet backlog drops, time squeezes and NET_RX
  concentration.
- `tcp-check`: new requirements for UDP buffer errors and TCP socket memory drops.

## Impact

`src/checks/{cpu,processes,memory,net}.rs`, `src/procfs/stat.rs`, new
`src/procfs/{schedstat,softnet,softirqs}.rs` and their `pub mod` lines. New metrics:
`runq_wait_ms`, `runq_wait_max_cpu_ms`, `forks_per_sec`, `refaults_per_sec`,
`compact_stalls`, `numa_miss_pct`, `softnet_dropped`, `softnet_squeezed`,
`net_rx_max_cpu_share_pct`, `udp_rcvbuf_errors`, `udp_sndbuf_errors`, `udp_in_errors`,
`tcp_backlog_drops`, `tcp_abort_on_memory`. No new dependencies, no new checks.
