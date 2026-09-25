# Spec Delta

## Purpose

Equivalent of `sar -n TCP,ETCP 1`: TCP active and passive connection rates, retransmits
relative to segments sent, established connections, and listen (accept) queue overflows.

## ADDED Requirements

### Requirement: TCP rates
The tcp check SHALL read the `Tcp:` counters from `/proc/net/snmp` at every sample and compute,
over the sampling window, active opens/s (ActiveOpens), passive opens/s (PassiveOpens),
retransmitted segments/s (RetransSegs) and output segments/s (OutSegs), plus the CurrEstab gauge
from the last sample. It SHALL expose metrics `active_per_sec`, `passive_per_sec`,
`retrans_per_sec` and `curr_estab`. The summary SHALL have the form
`active 3.0/s passive 12.0/s retrans 0.2/s (0.05%) estab 42`, where the percentage is present
only when the retransmit ratio is judged.

#### Scenario: Summary
- **WHEN** during a 1 s window ActiveOpens rises by 3, PassiveOpens by 12, OutSegs by 4000 and RetransSegs by 2, and CurrEstab is 42
- **THEN** the summary is `active 3.0/s passive 12.0/s retrans 2.0/s (0.05%) estab 42` and the status is OK

### Requirement: Retransmit ratio
The retransmit ratio SHALL be ΔRetransSegs ÷ ΔOutSegs × 100 over the window, exposed as metric
`retrans_pct`. It SHALL only be judged when ΔOutSegs is at least 100; otherwise the check SHALL
state that there is too little traffic to judge and SHALL NOT report `retrans_pct`. The check
SHALL report WARN when the ratio exceeds 1% and CRIT when it exceeds 5%, saying this points to
network or remote-host problems.

#### Scenario: Too little traffic
- **WHEN** during the window OutSegs rises by 99 and RetransSegs by 50
- **THEN** the status is OK, a detail says there is too little traffic to judge, and there is no `retrans_pct` metric

#### Scenario: Minimum traffic is judged
- **WHEN** during the window OutSegs rises by 100 and RetransSegs by 2 (2%)
- **THEN** the status is WARN

#### Scenario: At 1 percent
- **WHEN** during the window OutSegs rises by 1000 and RetransSegs by 10 (1%)
- **THEN** the status is OK and `retrans_pct` is 1

#### Scenario: Warn above 1 percent
- **WHEN** during the window OutSegs rises by 1000 and RetransSegs by 11 (1.1%)
- **THEN** the status is WARN

#### Scenario: At 5 percent
- **WHEN** during the window OutSegs rises by 1000 and RetransSegs by 50 (5%)
- **THEN** the status is WARN, not CRIT

#### Scenario: Crit above 5 percent
- **WHEN** during the window OutSegs rises by 1000 and RetransSegs by 51 (5.1%)
- **THEN** the status is CRIT

### Requirement: Since-boot retransmits
When the since-boot OutSegs counter (last sample) is at least 100 and RetransSegs ÷ OutSegs × 100
exceeds 1%, the tcp check SHALL add an informational note with the since-boot ratio. The note
SHALL NOT change the status.

#### Scenario: High since-boot ratio
- **WHEN** the last sample has OutSegs 11000 and RetransSegs 500 (4.5%), and the window itself has no retransmits over 1000 segments
- **THEN** the section has a note mentioning `since boot` and the status is OK

#### Scenario: Low since-boot ratio
- **WHEN** the last sample has OutSegs 11000 and RetransSegs 110 (1%)
- **THEN** the section has no since-boot note

### Requirement: Listen queue overflows
The tcp check SHALL read the `TcpExt:` counters from `/proc/net/netstat`. When ListenOverflows
or ListenDrops increased by more than 0 during the window it SHALL report WARN "accept queue
overflow: application not accepting fast enough (check somaxconn/backlog)" with the counts, and
expose metric `listen_overflows` (ΔListenOverflows). When `/proc/net/netstat` is unavailable
these findings and the metric are omitted and the check is not SKIPPED.

#### Scenario: Listen overflow
- **WHEN** ListenOverflows rises from 5 to 8 during the window
- **THEN** the status is WARN, the finding mentions `accept queue overflow`, and `listen_overflows` is 3

#### Scenario: Listen drops only
- **WHEN** ListenDrops rises by 1 and ListenOverflows is unchanged
- **THEN** the status is WARN

#### Scenario: No netstat
- **WHEN** `/proc/net/netstat` does not exist and `/proc/net/snmp` is readable
- **THEN** the tcp section is not SKIPPED and has no `listen_overflows` metric

### Requirement: TCP skip
When `/proc/net/snmp` cannot be read, cannot be parsed, or has no `Tcp:` counters, the tcp check
SHALL be SKIPPED.

#### Scenario: Missing snmp
- **WHEN** `/proc/net/snmp` does not exist
- **THEN** the tcp section is SKIPPED and its summary names `/proc/net/snmp`
