# net-interface-check Specification

## Purpose
Equivalent of `sar -n DEV 1`: per-interface receive/transmit throughput and packet rates over
the sampling window, utilization against the link speed, and interface errors and drops.

## Requirements

### Requirement: Interface rates
The net check SHALL read `/proc/net/dev` at every sample and compute, for every interface
except `lo` that is present in the first and last sample, the average over the sampling window
of received and transmitted bytes/s and packets/s, errors/s (receive + transmit errors) and
drops/s (receive + transmit drops). It SHALL expose per-interface metrics
`<if>.rx_bytes_per_sec`, `<if>.tx_bytes_per_sec`, `<if>.errors` and `<if>.drops` (the latter two
are counts during the window). An interface is active when any of its counters changed during
the window; the check SHALL add one detail line per active interface. The summary SHALL name
the busiest active interface (highest rx + tx bytes/s).

#### Scenario: Busiest interface in summary
- **WHEN** in a 1 s window eth0 receives 1,258,291 bytes and transmits 348,160 bytes, eth1 receives 1,000 bytes, and no link speed is known
- **THEN** the summary is `eth0 rx 1.2 MiB/s tx 340 KiB/s`, there are two detail lines, and the status is OK

#### Scenario: Utilization in summary
- **WHEN** eth0 has a speed of 1000 Mb/s and receives 12,500,000 bytes in a 1 s window
- **THEN** the summary ends with `(util 10%)`

#### Scenario: All idle
- **WHEN** no counter of eth0 or eth1 changes during the window
- **THEN** the summary is `all interfaces idle (2)` and the status is OK

#### Scenario: Loopback ignored
- **WHEN** `lo` transfers 1 GiB during a 1 s window and eth0 is idle
- **THEN** `lo` produces no metrics, details or findings and the summary is `all interfaces idle (1)`

### Requirement: Interface utilization
The net check SHALL read the link speed in Mb/s from `/sys/class/net/<if>/speed`. When the speed
is greater than 0, utilization SHALL be max(rx, tx) bits/s ÷ (speed × 1,000,000) × 100, shown in
the summary and details and exposed as metric `<if>.util_pct`. The check SHALL report WARN when
utilization exceeds 70% and CRIT when it exceeds 90%. When the speed file is missing, unreadable
or not greater than 0 (virtual interfaces report -1), utilization is unknown: no `util_pct`
metric and no utilization threshold.

#### Scenario: At 70 percent
- **WHEN** eth0 has a speed of 1000 Mb/s and receives 87,500,000 bytes in a 1 s window (70%)
- **THEN** the status is OK and `eth0.util_pct` is 70

#### Scenario: Warn above 70 percent
- **WHEN** eth0 has a speed of 1000 Mb/s and transmits 88,750,000 bytes in a 1 s window (71%)
- **THEN** the status is WARN

#### Scenario: At 90 percent
- **WHEN** eth0 has a speed of 1000 Mb/s and receives 112,500,000 bytes in a 1 s window (90%)
- **THEN** the status is WARN, not CRIT

#### Scenario: Crit above 90 percent
- **WHEN** eth0 has a speed of 1000 Mb/s and receives 113,750,000 bytes in a 1 s window (91%)
- **THEN** the status is CRIT

#### Scenario: Unknown speed
- **WHEN** eth0 has speed -1 (or no speed file) and receives 1,000,000,000 bytes in a 1 s window
- **THEN** there is no `eth0.util_pct` metric and the status is OK

### Requirement: Interface errors and drops
The net check SHALL report WARN, naming the interface and the error and drop counts, when an
interface's receive + transmit errors or receive + transmit drops increased by more than 0
during the window.

#### Scenario: Errors during the window
- **WHEN** eth0's rx errs goes from 0 to 3 during the window
- **THEN** the status is WARN, a finding names eth0 with 3 errors, and `eth0.errors` is 3

#### Scenario: Drops during the window
- **WHEN** eth0's tx drop goes from 10 to 15 during the window
- **THEN** the status is WARN, a finding names eth0 with 5 drops, and `eth0.drops` is 5

#### Scenario: Old errors only
- **WHEN** eth0 has 100 rx errs since boot that do not change during the window
- **THEN** the status is OK

### Requirement: Net skip
When `/proc/net/dev` cannot be read or parsed, the net check SHALL be SKIPPED.

#### Scenario: Missing net/dev
- **WHEN** `/proc/net/dev` does not exist
- **THEN** the net section is SKIPPED and its summary names `/proc/net/dev`

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
