# Spec Delta

## Purpose

Equivalent of `systemd-cgtop`: which services and containers use the machine's CPU and memory,
per leaf cgroup across the whole visible hierarchy, and which of them are throttled on their CPU
quota.

## ADDED Requirements

### Requirement: Hierarchy walk
The cgroups-top check SHALL walk the cgroup hierarchy on every sample. It SHALL use cgroup v2
at `/sys/fs/cgroup` when `/sys/fs/cgroup/cgroup.controllers` or `/sys/fs/cgroup/cpu.stat`
exists. Otherwise it SHALL use cgroup v1 at `/sys/fs/cgroup/cpu,cpuacct`, falling back to
`/sys/fs/cgroup/cpu`. The walk SHALL go at most 6 levels below the hierarchy root and stop after
2000 cgroup directories. A directory SHALL count as a cgroup when it has `cpu.stat` (v2) or
`cpuacct.usage` or `cpu.stat` (v1). Directories without them SHALL still be walked. Interface
files (`<controller>.<name>`, `tasks`, `notify_on_release`, `release_agent`) SHALL NOT be
treated as directories.

#### Scenario: Depth limit
- **WHEN** the hierarchy has a chain `/a/b/c/d/e/f/g`
- **THEN** `/a/b/c/d/e/f` (depth 6) is walked and listed as a leaf, and `/a/b/c/d/e/f/g` is not walked

#### Scenario: Cap at 2000 cgroups
- **WHEN** the v2 root has 2001 child cgroups
- **THEN** the walk stops at 2000 cgroup directories (the root and 1999 children), `cgroups` is 2000, and a detail says the walk was capped

#### Scenario: v1 paths
- **WHEN** only `/sys/fs/cgroup/cpu,cpuacct/system.slice/app.service/{cpuacct.usage,cpu.stat}` and `/sys/fs/cgroup/memory/system.slice/app.service/memory.usage_in_bytes` exist
- **THEN** the check lists `/system.slice/app.service` with its CPU from `cpuacct.usage` and its memory from `memory.usage_in_bytes`

### Requirement: Per-cgroup usage
For each cgroup the cgroups-top check SHALL compute CPU% (100% = one CPU) as Δusage ÷ Δt
between its first and last sample, from v2 `cpu.stat` `usage_usec` or v1 `cpuacct.usage` (ns).
Memory SHALL be v2 `memory.current`, or v1 `memory.usage_in_bytes` at the same path under
`/sys/fs/cgroup/memory`, at the last sample. Throttled% SHALL be Δnr_throttled ÷ Δnr_periods ×
100 when Δnr_periods > 0, otherwise 0. The check SHALL report the cgroups present in the last
walk.

#### Scenario: CPU percent over the window
- **WHEN** over a 1 s window a cgroup's `usage_usec` grows by 1,800,000
- **THEN** its CPU is 180%

### Requirement: Leaf-only listing
The cgroups-top check SHALL list only leaf cgroups (cgroups without walked child directories),
because parents include their children. The details SHALL show the top 5 leaves by CPU% as
`<cpu>% cpu <memory>  <path>`, e.g. `  12.3% cpu  410 MiB  /kubepods.slice/…/cri-containerd-abc.scope`,
with paths longer than 60 characters shortened in the middle with `…`. Then, when the top 5 by
memory are a different set, it SHALL show them in the same format.

#### Scenario: Parents are not listed
- **WHEN** `/system.slice` has children `/system.slice/a.service` (180%) and `/system.slice/b.service` (10%)
- **THEN** the details list `a.service` before `b.service` and never list `/system.slice` or the root

#### Scenario: Top memory differs
- **WHEN** the largest memory user is a leaf outside the top 5 by CPU
- **THEN** a second list of the top 5 by memory is shown, and when the sets are equal it is not

#### Scenario: Long paths
- **WHEN** a leaf path is longer than 60 characters
- **THEN** it is shown as 60 characters with its start and end kept and `…` in the middle

### Requirement: Throttled cgroups
The cgroups-top check SHALL report WARN when any leaf cgroup is throttled on more than 25% of its
periods in the window. The finding SHALL name at most 3 such cgroups, the most throttled first,
followed by `+N more` when there are more.

#### Scenario: At 25 percent
- **WHEN** a leaf has 25 of 100 periods throttled
- **THEN** the status is OK and `throttled_cgroups` is 0

#### Scenario: Above 25 percent
- **WHEN** a leaf has 251 of 1000 periods throttled (25.1%)
- **THEN** the status is WARN and the finding names it

#### Scenario: Many throttled cgroups
- **WHEN** 5 leaves are throttled above 25%
- **THEN** the finding names 3 of them followed by `+2 more` and `throttled_cgroups` is 5

### Requirement: Cgroups summary and metrics
The summary SHALL be `<n> cgroups, top cpu: <path> <pct>%, top memory: <path> <size>`. The
memory part is left out when no memory is known. When the only visible cgroup is the root inside
a container, the summary SHALL be `1 cgroup visible (container cgroup namespace)`. Metrics SHALL
be `cgroups` (all cgroups found), `top_cpu_pct` and `throttled_cgroups`.

#### Scenario: Host summary
- **WHEN** 4 cgroups are found and the top leaf by CPU is `/system.slice/a.service` at 180% while `/user.slice` uses the most memory (2.1 GiB)
- **THEN** the summary is `4 cgroups, top cpu: /system.slice/a.service 180%, top memory: /user.slice 2.1 GiB`

#### Scenario: Container namespace
- **WHEN** inside a container only the root cgroup is visible
- **THEN** the summary is `1 cgroup visible (container cgroup namespace)`

### Requirement: Cgroups-top SKIPPED
The cgroups-top check SHALL be SKIPPED when no cgroup hierarchy exists at `/sys/fs/cgroup`, or
when the walk finds no cgroup.

#### Scenario: No hierarchy
- **WHEN** `/sys/fs/cgroup` has neither v2 files nor a v1 cpu hierarchy
- **THEN** the check is SKIPPED
