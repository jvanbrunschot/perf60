# memory-check Specification

## Purpose
Equivalent of `free -m`: shows how much memory is available against the total and against the
cgroup memory limit, and whether the OOM killer has fired.

## Requirements

### Requirement: Memory summary
The memory check SHALL read `/proc/meminfo` at the last sample and report total, available,
buffers, cached, shared (Shmem) memory and swap used/total. Available memory SHALL be
`MemAvailable`; when `MemAvailable` is absent (kernels before 3.14) it SHALL be estimated as
`MemFree + Buffers + Cached`. It SHALL expose the metrics `total_bytes`, `available_bytes`,
`available_pct`, `cached_bytes` and `swap_used_bytes`.

#### Scenario: Summary
- **WHEN** meminfo reports MemTotal 1986320 kB, MemAvailable 1470524 kB, Buffers 1336 kB, Cached 450432 kB and SwapTotal 0 kB
- **THEN** the summary is `available 1.4 GiB of 1.9 GiB (74%), buffers 1.3 MiB, cached 440 MiB, swap 0 B/0 B` and the status is OK

#### Scenario: Old kernel without MemAvailable
- **WHEN** meminfo has MemTotal 1000000 kB, MemFree 50000 kB, Buffers 10000 kB, Cached 40000 kB and no MemAvailable
- **THEN** available is 100000 kB (10%) and the status is OK

### Requirement: Low available memory
The memory check SHALL report WARN when available memory is below 10% of MemTotal and CRIT when
it is below 5% of MemTotal.

#### Scenario: Exactly 10% available
- **WHEN** available memory is 10.0% of MemTotal
- **THEN** the status is OK

#### Scenario: Warn below 10%
- **WHEN** available memory is 9.9% of MemTotal
- **THEN** the status is WARN

#### Scenario: Exactly 5% available
- **WHEN** available memory is 5.0% of MemTotal
- **THEN** the status is WARN

#### Scenario: Crit below 5%
- **WHEN** available memory is 4.9% of MemTotal
- **THEN** the status is CRIT

### Requirement: cgroup memory limit
When a cgroup memory limit applies, the memory check SHALL compute the working set as memory
usage minus inactive file cache and compare it with the limit. For cgroup v2 it SHALL walk from
the own cgroup (`/proc/self/cgroup`) up to the root, take the tightest `memory.max`, and use that
cgroup's `memory.current` and `inactive_file` from `memory.stat`. When no v2 limit is found it
SHALL fall back to cgroup v1: `memory.usage_in_bytes`, `memory.limit_in_bytes` and
`total_inactive_file` from `memory.stat` under `/sys/fs/cgroup/memory`. A limit of `max` or a
limit at or above MemTotal SHALL be treated as no limit. The check SHALL add the detail
`cgroup: <working set> of <limit> (<pct>%)`, expose the metric `cgroup_used_pct`, and report
WARN when the working set exceeds 90% of the limit and CRIT when it exceeds 95%.

#### Scenario: Working set within limit
- **WHEN** memory.max is 536870912 (512 MiB), memory.current is 450 MiB and inactive_file is 40 MiB
- **THEN** the detail is `cgroup: 410 MiB of 512 MiB (80%)` and the status is OK

#### Scenario: Working set exactly 90%
- **WHEN** the cgroup working set is exactly 90% of a 1000 MiB limit
- **THEN** the status is OK

#### Scenario: Warn above 90%
- **WHEN** the cgroup working set is 91% of the limit
- **THEN** the status is WARN

#### Scenario: Exactly 95%
- **WHEN** the cgroup working set is exactly 95% of the limit
- **THEN** the status is WARN

#### Scenario: Crit above 95%
- **WHEN** the cgroup working set is 96% of the limit
- **THEN** the status is CRIT

#### Scenario: Nested cgroups use the tightest limit
- **WHEN** the own cgroup is `/a/b`, `/a` has memory.max 512 MiB and `/a/b` has memory.max `max`
- **THEN** the working set of `/a` is compared with 512 MiB

#### Scenario: No limit
- **WHEN** memory.max is `max`
- **THEN** no cgroup detail or `cgroup_used_pct` metric is reported

#### Scenario: cgroup v1 unlimited
- **WHEN** the v1 memory.limit_in_bytes is 9223372036854771712 and MemTotal is smaller
- **THEN** no cgroup detail is reported

#### Scenario: cgroup v1 limit
- **WHEN** the v1 limit is 1000 MiB, usage is 980 MiB and total_inactive_file is 0
- **THEN** the working set is 98% of the limit and the status is CRIT

### Requirement: OOM kills
The memory check SHALL read `oom_kill` from `/proc/vmstat` (kernel 4.13 and later) at every
sample and expose the since-boot total as the metric `oom_kills`. An increase during the
sampling window SHALL be CRIT. A non-zero since-boot total without an increase SHALL add the
note `<N> OOM kills since boot` and SHALL NOT change the status. A missing counter or an
unreadable `/proc/vmstat` SHALL be ignored.

#### Scenario: OOM kill during the window
- **WHEN** oom_kill is 3 at the first sample and 4 at the last sample
- **THEN** the status is CRIT

#### Scenario: OOM kills since boot
- **WHEN** oom_kill is 2 at every sample
- **THEN** the section includes the note `2 OOM kills since boot` and the status is OK

#### Scenario: No oom_kill counter
- **WHEN** `/proc/vmstat` has no oom_kill line
- **THEN** no OOM finding or `oom_kills` metric is reported

### Requirement: Memory skip
When `/proc/meminfo` cannot be read, the memory check SHALL be SKIPPED.

#### Scenario: Missing meminfo
- **WHEN** `/proc/meminfo` does not exist
- **THEN** the memory section is SKIPPED and the summary names `/proc/meminfo`

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
