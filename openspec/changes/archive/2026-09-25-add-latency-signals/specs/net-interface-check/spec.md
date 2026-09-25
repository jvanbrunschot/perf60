## ADDED Requirements

### Requirement: Softnet backlog drops and time squeezes
The net check SHALL read `/proc/net/softnet_stat` on every sample. It has one row of
hexadecimal columns per CPU: column 0 is packets processed, column 1 packets dropped because
the per-CPU backlog was full, and column 2 the times the NAPI poll ran out of budget
(time_squeeze). Over the window the check SHALL expose the Δdropped and Δtime_squeeze summed
over all CPUs as metrics `softnet_dropped` and `softnet_squeezed`. It SHALL report WARN when
Δdropped is above 0 ("packets dropped at the per-CPU backlog: raise net.core.netdev_max_backlog
/ check RPS") and add a note when Δtime_squeeze is above 0 ("NAPI budget exhausted:
net.core.netdev_budget"). Rows with fewer columns (older kernels) SHALL be accepted. When the
file is missing or cannot be parsed, the check SHALL report no softnet metric or finding and
SHALL NOT be SKIPPED because of it.

#### Scenario: Backlog drops
- **WHEN** the dropped column of one CPU grows by 1 during the window
- **THEN** `softnet_dropped` is 1 and the status is WARN with a finding about the per-CPU backlog

#### Scenario: No backlog drops
- **WHEN** the dropped and time_squeeze columns do not change
- **THEN** `softnet_dropped` and `softnet_squeezed` are 0 and the status is OK

#### Scenario: Time squeeze
- **WHEN** the time_squeeze column grows by 5 during the window and nothing is dropped
- **THEN** `softnet_squeezed` is 5, there is a note about the NAPI budget, and the status is OK

#### Scenario: Softnet missing
- **WHEN** `/proc/net/softnet_stat` does not exist
- **THEN** the net section is not SKIPPED and has no `softnet_dropped` metric

### Requirement: NET_RX concentration
The net check SHALL read the `NET_RX` row of `/proc/softirqs` on every sample. When there are
at least 2 CPUs and the NET_RX softirqs of all CPUs grew by more than 1000 during the window,
it SHALL expose the largest single-CPU share of that growth as metric
`net_rx_max_cpu_share_pct`, and add a note "NET_RX concentrated on cpuN: check IRQ affinity /
RSS" when that share exceeds 80%. Notes SHALL NOT change the status. With one CPU, 1000 or
fewer NET_RX softirqs, or a missing or unparsable file, the check SHALL report no NET_RX metric
or finding.

#### Scenario: NET_RX share at 80 percent
- **WHEN** on 2 CPUs NET_RX grows by 1600 on cpu0 and 400 on cpu1
- **THEN** `net_rx_max_cpu_share_pct` is 80 and there is no NET_RX note

#### Scenario: NET_RX share above 80 percent
- **WHEN** on 2 CPUs NET_RX grows by 1601 on cpu0 and 399 on cpu1
- **THEN** there is a note naming cpu0 and the status is OK

#### Scenario: Too few NET_RX softirqs
- **WHEN** on 2 CPUs NET_RX grows by 1000 in total, all on cpu1
- **THEN** there is no NET_RX metric or note

#### Scenario: Single CPU
- **WHEN** there is one CPU and its NET_RX grows by 5000
- **THEN** there is no NET_RX metric or note
