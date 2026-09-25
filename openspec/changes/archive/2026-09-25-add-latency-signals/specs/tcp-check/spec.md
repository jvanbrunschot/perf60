## ADDED Requirements

### Requirement: UDP buffer errors
The tcp check SHALL read the `Udp:` and `UdpLite:` counters from `/proc/net/snmp` at every
sample and compute, over the window, ΔRcvbufErrors, ΔSndbufErrors and ΔInErrors summed over
both sections, exposed as metrics `udp_rcvbuf_errors`, `udp_sndbuf_errors` and
`udp_in_errors`. It SHALL report WARN for each counter whose delta is above 0, with a finding
that names the counter; for RcvbufErrors the finding SHALL say "UDP receive buffer overflows:
application too slow or rmem too small". When there is no `Udp:` section, the check SHALL
report no UDP metric or finding and SHALL NOT be SKIPPED because of it.

#### Scenario: UDP receive buffer overflows
- **WHEN** Udp RcvbufErrors grows by 4 during the window
- **THEN** `udp_rcvbuf_errors` is 4 and the status is WARN with a finding naming RcvbufErrors

#### Scenario: UdpLite counts too
- **WHEN** only UdpLite SndbufErrors grows by 1 during the window
- **THEN** `udp_sndbuf_errors` is 1 and the status is WARN with a finding naming SndbufErrors

#### Scenario: UDP input errors
- **WHEN** Udp InErrors grows by 2 during the window
- **THEN** `udp_in_errors` is 2 and the status is WARN with a finding naming InErrors

#### Scenario: No UDP errors
- **WHEN** the UDP error counters do not change
- **THEN** the UDP metrics are 0 and there is no UDP finding

#### Scenario: No Udp section
- **WHEN** `/proc/net/snmp` has only `Tcp:` counters
- **THEN** there is no `udp_rcvbuf_errors` metric and the tcp section is not SKIPPED

### Requirement: TCP socket memory drops
The tcp check SHALL compute, from the `TcpExt:` counters of `/proc/net/netstat`, ΔTCPBacklogDrop
and ΔTCPAbortOnMemory over the window, exposed as metrics `tcp_backlog_drops` and
`tcp_abort_on_memory`. It SHALL report WARN when ΔTCPBacklogDrop is above 0 (segments dropped
because the socket backlog was full) and WARN when ΔTCPAbortOnMemory is above 0 (connections
aborted for lack of socket memory). A missing file or counter SHALL omit that metric and
finding only.

#### Scenario: Backlog drops
- **WHEN** TCPBacklogDrop grows by 7 during the window
- **THEN** `tcp_backlog_drops` is 7 and the status is WARN with a finding naming TCPBacklogDrop

#### Scenario: Abort on memory
- **WHEN** TCPAbortOnMemory grows by 1 during the window
- **THEN** `tcp_abort_on_memory` is 1 and the status is WARN with a finding naming TCPAbortOnMemory

#### Scenario: No socket memory drops
- **WHEN** TCPBacklogDrop and TCPAbortOnMemory do not change
- **THEN** both metrics are 0 and the status is OK

#### Scenario: Counters absent
- **WHEN** `/proc/net/netstat` has no TCPBacklogDrop or TCPAbortOnMemory field
- **THEN** there is no `tcp_backlog_drops` or `tcp_abort_on_memory` metric
