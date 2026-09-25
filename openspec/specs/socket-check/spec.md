# socket-check Specification

## Purpose
Equivalent of `ss -s` and `conntrack -S`: socket counts per protocol from sockstat, and how close
the connection-tracking table, the ephemeral port range (TIME_WAIT), orphaned sockets and TCP
socket memory are to their kernel limits.

## Requirements

### Requirement: Socket summary
The sockets check SHALL read `/proc/net/sockstat` and, when present, `/proc/net/sockstat6` at
every sample. It SHALL judge the last sample and keep the first one for the TIME_WAIT trend.
TCP sockets in use SHALL be `TCP: inuse` plus `TCP6: inuse`. TIME_WAIT sockets SHALL be
`TCP: tw` plus `TCP6: tw` where sockstat6 has it. Orphans SHALL be `TCP: orphan`, and TCP
memory SHALL be `TCP: mem` (pages). The check SHALL add one detail line per sockstat and
sockstat6 protocol line (for example `TCP: inuse 38 orphan 0 tw 1204 alloc 45 mem 12`). It SHALL
expose metrics `tcp_inuse` and `tcp_tw`. The summary SHALL have the form
`tcp 38 in use, 1204 time-wait (4% of ports), 0 orphans, conntrack 1203/262144 (0.5%)`:
- the port percentage is present only when the ephemeral port range is known
- the conntrack part is present only when conntrack is in use
- percentages above 0 and below 0.1 show as `<0.1%`, other percentages below 1 have one
  decimal, and the rest have none

#### Scenario: Summary
- **WHEN** sockstat has `TCP: inuse 38 orphan 0 tw 1204 alloc 45 mem 12`, the port range is 32768–60999, and conntrack count is 1203 of max 262144
- **THEN** the summary is `tcp 38 in use, 1204 time-wait (4% of ports), 0 orphans, conntrack 1203/262144 (0.5%)`, the status is OK, and there is a detail line per sockstat protocol line

#### Scenario: IPv6 sockets counted
- **WHEN** sockstat has `TCP: inuse 10 … tw 300` and sockstat6 has `TCP6: inuse 5 tw 201`
- **THEN** `tcp_inuse` is 15 and `tcp_tw` is 501

### Requirement: Connection tracking capacity
When `/proc/sys/net/netfilter/nf_conntrack_count` and `nf_conntrack_max` are both readable and
the max is greater than 0, the check SHALL compute count ÷ max × 100, expose it as metric
`conntrack_pct`, and report WARN above 80% and CRIT above 90% with "conntrack table nearly
full: new connections will be dropped; raise nf_conntrack_max or shorten timeouts". When either
file is missing, a detail SHALL say `conntrack not in use`, there SHALL be no conntrack finding
or `conntrack_pct` metric, and the check SHALL NOT be SKIPPED.

#### Scenario: Conntrack at 80 percent
- **WHEN** nf_conntrack_count is 800 and nf_conntrack_max is 1000
- **THEN** `conntrack_pct` is 80 and the status is OK

#### Scenario: Conntrack warn above 80 percent
- **WHEN** nf_conntrack_count is 801 and nf_conntrack_max is 1000 (80.1%)
- **THEN** the status is WARN and the finding mentions `conntrack table nearly full`

#### Scenario: Conntrack at 90 percent
- **WHEN** nf_conntrack_count is 900 and nf_conntrack_max is 1000
- **THEN** the status is WARN, not CRIT

#### Scenario: Conntrack crit above 90 percent
- **WHEN** nf_conntrack_count is 901 and nf_conntrack_max is 1000 (90.1%)
- **THEN** the status is CRIT

#### Scenario: Conntrack not in use
- **WHEN** the nf_conntrack files do not exist and sockstat is readable
- **THEN** a detail says `conntrack not in use`, there is no `conntrack_pct` metric, the summary has no conntrack part, and the section is not SKIPPED

### Requirement: Ephemeral port exhaustion
When `/proc/sys/net/ipv4/ip_local_port_range` holds `lo hi` with lo ≤ hi, the check SHALL
compute TIME_WAIT sockets ÷ (hi − lo + 1) × 100, expose it as metric `tw_port_pct`, and report
WARN above 50% with "many TIME_WAIT sockets vs the ephemeral port range: outbound connection
failures likely; reuse connections / tcp_tw_reuse". When the range is unavailable this finding,
the metric and the summary percentage SHALL be omitted.

#### Scenario: TIME_WAIT at 50 percent
- **WHEN** the port range is 1000–1999 (1000 ports) and `TCP: tw` is 500
- **THEN** `tw_port_pct` is 50 and the status is OK

#### Scenario: TIME_WAIT warn above 50 percent
- **WHEN** the port range is 1000–1999 and `TCP: tw` is 501 (50.1%)
- **THEN** the status is WARN and the finding mentions `ephemeral port range`

#### Scenario: IPv6 TIME_WAIT counted
- **WHEN** the port range is 1000–1999, `TCP: tw` is 300 and sockstat6 has `TCP6: tw 201`
- **THEN** the status is WARN

### Requirement: TIME_WAIT trend
When the ephemeral port range is known and TIME_WAIT rose between the first and the last sample
by more than 5% of the range, the check SHALL add an informational note
`time-wait rising: <first> → <last> during the window`. The note SHALL NOT change the status.

#### Scenario: Rising TIME_WAIT
- **WHEN** the port range is 1000–1999 and `TCP: tw` goes from 100 to 151
- **THEN** there is a note mentioning `time-wait rising` and the status is OK

#### Scenario: Stable TIME_WAIT
- **WHEN** the port range is 1000–1999 and `TCP: tw` goes from 100 to 150
- **THEN** there is no note

### Requirement: Orphaned sockets
When `/proc/sys/net/ipv4/tcp_max_orphans` is readable and greater than 0, the check SHALL
compute `TCP: orphan` ÷ tcp_max_orphans × 100, expose it as metric `orphan_pct`, and report
WARN above 50%. When the sysctl is unavailable this finding and the metric SHALL be omitted.

#### Scenario: Orphans at 50 percent
- **WHEN** tcp_max_orphans is 1000 and `TCP: orphan` is 500
- **THEN** `orphan_pct` is 50 and the status is OK

#### Scenario: Orphans warn above 50 percent
- **WHEN** tcp_max_orphans is 1000 and `TCP: orphan` is 501 (50.1%)
- **THEN** the status is WARN and the finding mentions `orphan`

### Requirement: TCP memory
When `/proc/sys/net/ipv4/tcp_mem` has three values and the third is greater than 0, the check
SHALL compute `TCP: mem` ÷ the third value × 100 (both in pages), expose it as metric
`tcp_mem_pct`, and report WARN above 80% with "TCP socket memory near tcp_mem limit: the kernel
will start dropping/collapsing". When the sysctl is unavailable this finding and the metric
SHALL be omitted.

#### Scenario: TCP memory at 80 percent
- **WHEN** tcp_mem is `100 200 1000` and `TCP: mem` is 800
- **THEN** `tcp_mem_pct` is 80 and the status is OK

#### Scenario: TCP memory warn above 80 percent
- **WHEN** tcp_mem is `100 200 1000` and `TCP: mem` is 801 (80.1%)
- **THEN** the status is WARN and the finding mentions `tcp_mem limit`

### Requirement: Missing sysctls
A missing or unparsable `ip_local_port_range`, `tcp_max_orphans` or `tcp_mem` SHALL omit only
the finding and metric that need it. The other findings SHALL still be judged and the check
SHALL NOT be SKIPPED.

#### Scenario: Sysctls missing
- **WHEN** sockstat has `TCP: inuse 1 orphan 900 tw 900 alloc 1 mem 900` and none of the three sysctls exists, while conntrack is at 95%
- **THEN** the status is CRIT from conntrack only, and there are no `tw_port_pct`, `orphan_pct` or `tcp_mem_pct` metrics

### Requirement: Sockets skip
When `/proc/net/sockstat` cannot be read, cannot be parsed, or has no `TCP:` line, the sockets
check SHALL be SKIPPED with a summary naming `/proc/net/sockstat`. A missing or unparsable
sockstat6 SHALL NOT make it SKIPPED.

#### Scenario: Missing sockstat
- **WHEN** `/proc/net/sockstat` does not exist
- **THEN** the sockets section is SKIPPED and its summary names `/proc/net/sockstat`

#### Scenario: No sockstat6
- **WHEN** `/proc/net/sockstat` is readable and `/proc/net/sockstat6` does not exist
- **THEN** the section is not SKIPPED and counts only IPv4 sockets
