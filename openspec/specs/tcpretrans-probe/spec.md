# tcpretrans-probe Specification

## Purpose
The eBPF equivalent of BCC `tcpretrans`: which remote TCP endpoints the retransmits of the
sampling window go to. It adds context to the counter-based `tcp` check, which stays
authoritative for the retransmit ratio.

## Requirements

### Requirement: Retransmit counting by endpoint
The tcpretrans probe SHALL attach to the raw tracepoint `tcp_retransmit_skb` and count, over
the sampling window, every retransmitted segment in total and per remote endpoint (address
family, remote address and remote port). It SHALL read the socket fields from
`struct sock_common` at offsets taken from the running kernel's BTF. It SHALL expose metrics
`retransmits`, `retransmits_per_sec`, `endpoints` and `top_endpoint_share_pct` (the top
endpoint's percentage of all retransmits, 0 when there are none).

#### Scenario: Summary with endpoints
- **WHEN** 17 retransmits happen in a 2-second window, 12 to `10.0.0.5:443`, 3 to `10.0.0.6:80` and 2 to `[2001:db8::1]:443`
- **THEN** the summary is `17 retransmits (8.5/s) to 3 endpoints, top 10.0.0.5:443 12` and the metric `endpoints` is 3

#### Scenario: Quiet window
- **WHEN** no retransmit happens during the window
- **THEN** the summary is `no retransmits`, the status is OK and `top_endpoint_share_pct` is 0

### Requirement: Endpoint formatting
The tcpretrans section SHALL print IPv4 endpoints as `address:port` (e.g. `10.0.0.5:443`) and
IPv6 endpoints as `[address]:port` (e.g. `[2001:db8::1]:443`), with the port converted from
network byte order. An IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) SHALL be printed as IPv4.

#### Scenario: IPv4-mapped IPv6
- **WHEN** an AF_INET6 socket retransmits to `::ffff:10.0.0.5` port 443
- **THEN** the endpoint is shown as `10.0.0.5:443`

### Requirement: Details
The tcpretrans section SHALL list up to 5 endpoints as detail lines, ordered by retransmit
count (highest first, ties by endpoint text). When there are more, a final detail line SHALL
say `<N> more endpoints`.

#### Scenario: Many endpoints
- **WHEN** 8 distinct endpoints retransmit during the window
- **THEN** there are 5 endpoint lines in descending count order followed by `3 more endpoints`

### Requirement: Concentration note
The tcpretrans section SHALL add a note (not a WARN) `retransmits concentrated on <endpoint>
…: suspect that path or host` when the window has at least 20 retransmits and one endpoint
has more than 50% of them.

#### Scenario: Below the minimum count
- **WHEN** 19 retransmits happen, all to one endpoint
- **THEN** there is no concentration note

#### Scenario: At the minimum count
- **WHEN** 20 retransmits happen, all to one endpoint
- **THEN** there is a concentration note naming that endpoint and the status stays OK

#### Scenario: Exactly half
- **WHEN** 1000 retransmits happen and the top endpoint has 500 of them (50%)
- **THEN** there is no concentration note

#### Scenario: Just over half
- **WHEN** 1000 retransmits happen and the top endpoint has 501 of them (50.1%)
- **THEN** there is a concentration note

### Requirement: Retransmit rate threshold
The tcpretrans section SHALL report WARN when retransmits per second over the window exceed
100, naming the top endpoint.

#### Scenario: At the threshold
- **WHEN** 200 retransmits happen in 2 seconds (100/s)
- **THEN** there is no WARN finding

#### Scenario: Above the threshold
- **WHEN** 201 retransmits happen in 2 seconds
- **THEN** the status is WARN and the finding names the top endpoint

### Requirement: Skipped probe
When the probe cannot run, the tcpretrans section SHALL be SKIPPED with the reason, and the
rest of the report SHALL be unaffected: the deep-mode needs-root reason without privileges,
the kernel BTF error when `/sys/kernel/btf/vmlinux` is missing or lacks a `sock_common`
member, and the load or attach error otherwise.

#### Scenario: Unprivileged container
- **WHEN** perf60 runs with `--deep` in a container with default capabilities
- **THEN** the tcpretrans section is SKIPPED with the needs-root reason
