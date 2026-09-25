# Spec Delta

## MODIFIED Requirements

### Requirement: UDP buffer errors
The tcp check SHALL read the `Udp:` and `UdpLite:` counters from `/proc/net/snmp` at every
sample and compute, over the window, ΔRcvbufErrors, ΔSndbufErrors and ΔInErrors summed over
both sections, exposed as metrics `udp_rcvbuf_errors`, `udp_sndbuf_errors` and
`udp_in_errors`. It SHALL report WARN when ΔRcvbufErrors or ΔSndbufErrors is above 0, with a
finding that names the counter. For RcvbufErrors the finding SHALL say "UDP receive buffer
overflows: application too slow or rmem too small". Because the kernel also counts every
receive-buffer overflow in InErrors, it SHALL report the InErrors WARN only when ΔInErrors
exceeds ΔRcvbufErrors, naming the unexplained count. When there is no `Udp:` section, the check
SHALL report no UDP metric or finding and SHALL NOT be SKIPPED because of it.

#### Scenario: UDP receive buffer overflows
- **WHEN** Udp RcvbufErrors and InErrors both grow by 4 during the window
- **THEN** `udp_rcvbuf_errors` and `udp_in_errors` are 4, and there is exactly one WARN, naming RcvbufErrors

#### Scenario: UdpLite counts too
- **WHEN** only UdpLite SndbufErrors grows by 1 during the window
- **THEN** `udp_sndbuf_errors` is 1 and the status is WARN with a finding naming SndbufErrors

#### Scenario: UDP input errors
- **WHEN** Udp InErrors grows by 2 and RcvbufErrors does not change
- **THEN** `udp_in_errors` is 2 and the status is WARN with a finding naming InErrors +2

#### Scenario: No UDP errors
- **WHEN** the UDP error counters do not change
- **THEN** the UDP metrics are 0 and there is no UDP finding

#### Scenario: No Udp section
- **WHEN** `/proc/net/snmp` has only `Tcp:` counters
- **THEN** there is no `udp_rcvbuf_errors` metric and the tcp section is not SKIPPED
