## ADDED Requirements

### Requirement: Page cache thrashing
The memory check SHALL read `/proc/vmstat` on every sample; its window runs from the first to
the latest readable sample. It SHALL compute the refault rate as
Δ`workingset_refault_file` per second over that window, falling back to Δ`workingset_refault`
on kernels before 5.9, exposed as metric `refaults_per_sec`. It SHALL report WARN when the
rate exceeds 1000 pages/s, with a finding "page cache thrashing: working set does not fit in
memory". When neither counter is present at both ends of the window, or
`/proc/vmstat` is missing, the check SHALL report no refault metric or finding and SHALL NOT be
SKIPPED because of it.

#### Scenario: Refaults at 1000 per second
- **WHEN** workingset_refault_file grows by 10000 over a 10 s window (1000 pages/s)
- **THEN** `refaults_per_sec` is 1000 and the status is OK

#### Scenario: Refaults above 1000 per second
- **WHEN** workingset_refault_file grows by 10001 over a 10 s window (1000.1 pages/s)
- **THEN** the status is WARN and the finding says page cache thrashing

#### Scenario: Legacy refault counter
- **WHEN** vmstat has only `workingset_refault` and it grows by 20000 over a 10 s window
- **THEN** `refaults_per_sec` is 2000 and the status is WARN

#### Scenario: No vmstat
- **WHEN** `/proc/vmstat` does not exist
- **THEN** the memory section is not SKIPPED and has no `refaults_per_sec`, `compact_stalls` or `numa_miss_pct` metric

### Requirement: Compaction stalls
The memory check SHALL expose Δ`compact_stall` over the window as metric `compact_stalls` and
add a note that allocations stalled on memory compaction (often transparent huge pages) when
it is above 0. Notes SHALL NOT change the status. A missing counter SHALL omit the metric.

#### Scenario: Compaction stalls during the window
- **WHEN** compact_stall grows by 3 during the window
- **THEN** `compact_stalls` is 3, there is a note about compaction stalls, and the status is OK

#### Scenario: No compaction stalls
- **WHEN** compact_stall does not change during the window
- **THEN** `compact_stalls` is 0 and there is no compaction note

### Requirement: NUMA misses
When `/sys/devices/system/node/node1` exists and Δnuma_hit + Δnuma_miss over the window is above
0, the memory check SHALL compute the NUMA miss ratio Δnuma_miss ÷ (Δnuma_hit + Δnuma_miss) × 100,
exposed as metric `numa_miss_pct`, and add a note about remote-node allocations when it exceeds
10%. On a single-node machine, or without NUMA counters, the check SHALL report no NUMA metric
or finding.

#### Scenario: NUMA miss ratio at 10 percent
- **WHEN** node1 exists and numa_hit grows by 90 and numa_miss by 10
- **THEN** `numa_miss_pct` is 10 and there is no NUMA note

#### Scenario: NUMA miss ratio above 10 percent
- **WHEN** node1 exists and numa_hit grows by 89 and numa_miss by 11
- **THEN** there is a NUMA note and the status is OK

#### Scenario: Single node
- **WHEN** node1 does not exist and numa_miss grows
- **THEN** there is no `numa_miss_pct` metric and no NUMA note
