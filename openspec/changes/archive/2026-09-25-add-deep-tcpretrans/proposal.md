# Proposal

## Why

The counter-based `tcp` check (`sar -n TCP,ETCP`) tells you *that* TCP retransmits, and how
often relative to the segments sent. It can't tell you *where* they go. BCC `tcpretrans`
(*BPF Performance Tools*, ch. 3 checklist) answers that: one bad path or overloaded remote
host usually accounts for most retransmits, and naming it turns "the network is lossy" into
"look at 10.0.0.5:443".

## What Changes

- New eBPF program `perf60-ebpf/src/bin/tcpretrans.rs` on the raw tracepoint
  `tcp_retransmit_skb(sk, skb)`. It reads the socket's family, remote port and remote address
  (IPv4 or IPv6) from `struct sock_common` at offsets resolved from the kernel's BTF, counts
  retransmits per remote endpoint in a hash map (4096 entries) and in total in a per-CPU array.
- New `--deep` section `tcpretrans` (network): retransmits and rate over the window, the
  number of endpoints, the top 5 endpoints, a note when one endpoint takes most of them and a
  WARN on a high absolute rate. The `tcp` check stays authoritative for the retransmit ratio.
- README deep-mode table gets a `tcpretrans` row.

## Capabilities

### New Capabilities
- `tcpretrans-probe`: the tcpretrans section.

### Modified Capabilities
None.

## Impact

`perf60-ebpf/src/bin/tcpretrans.rs`, `src/deep/tcpretrans.rs`, two registration lines in
`src/deep/mod.rs`, README. No new dependencies and no framework changes: the map key type is
defined locally in both crates with the same `#[repr(C)]` layout (checked by a size test).
