# cgroup-check Specification

## Purpose
Shows the saturation of the process's own cgroup, which host-wide tools cannot see: CPU quota
throttling from `cpu.stat` and memory limit events from `memory.events` (v2) or
`memory.failcnt` (v1), measured over the sampling window.

## Requirements

### Requirement: cgroup file parsing
The cgroup parsers SHALL read `/proc/self/cgroup` lines of the form
`<hierarchy-id>:<controllers>:<path>`. They SHALL read `cpu.stat` in the v2 form (`usage_usec`,
`nr_periods`, `nr_throttled`, `throttled_usec`) and in the v1 form (`nr_periods`,
`nr_throttled`, `throttled_time` in nanoseconds, converted to microseconds). They SHALL read
`memory.events` counters (`low`, `high`, `max`, `oom`, `oom_kill`). Single-value files SHALL be
parsed as integers, where `max` means no limit. Input without any of the expected keys SHALL be
rejected.

#### Scenario: v2 cpu.stat
- **WHEN** `cpu.stat` contains `usage_usec 42532`, `nr_periods 0`, `nr_throttled 0` and `throttled_usec 0`
- **THEN** usage is 42532 µs and periods, throttled periods and throttled time are 0

#### Scenario: v1 cpu.stat
- **WHEN** `cpu.stat` contains `nr_periods 864000`, `nr_throttled 43200` and `throttled_time 912345678901`
- **THEN** periods are 864000, throttled periods 43200, throttled time 912345678 µs, and usage is absent

#### Scenario: memory.events
- **WHEN** `memory.events` contains `high 3`, `max 2`, `oom 1` and `oom_kill 1`
- **THEN** each counter has that value

#### Scenario: Malformed files
- **WHEN** `cpu.stat` or `memory.events` contain none of the expected keys, or a single-value file is not a number or `max`
- **THEN** parsing fails

### Requirement: Own cgroup resolution
The cgroup check SHALL resolve its own cgroup from `/proc/self/cgroup` on the first sample.
When the file has a v1 line for the `cpu`, `cpuacct` or `memory` controller, it SHALL use cgroup
v1: CPU files come from `/sys/fs/cgroup/<controllers><path>` (the controller list of the
`cpu` line, then `cpu,cpuacct`, `cpu` and `cpuacct`), and memory files from
`/sys/fs/cgroup/memory<path>`. Otherwise it SHALL use the v2 `0::<path>` line with
`/sys/fs/cgroup<path>`. When the path's directory has no `cpu.stat` but the hierarchy mount root
has one, it SHALL read the mount root, because the container sees its own cgroup there. When
the path is `/` and the system is not a container, the check SHALL report OK with the summary
`host root cgroup (no own limits)` and no findings. When the path is `/` inside a container
(cgroup namespace), the root is the container's cgroup and SHALL be evaluated. The first
detail line SHALL name the cgroup and its version.

#### Scenario: v2 own cgroup
- **WHEN** `/proc/self/cgroup` is `0::/kubepods.slice/pod1/cri-containerd-abc.scope`
- **THEN** the check reads `/sys/fs/cgroup/kubepods.slice/pod1/cri-containerd-abc.scope/cpu.stat` and `memory.events`

#### Scenario: Host root cgroup
- **WHEN** `/proc/self/cgroup` is `0::/` and the system is not a container
- **THEN** the status is OK, the summary is `host root cgroup (no own limits)`, and there are no findings even if `/sys/fs/cgroup/cpu.stat` shows throttling

#### Scenario: Container namespace root
- **WHEN** `/proc/self/cgroup` is `0::/` inside a container and `/sys/fs/cgroup/cpu.stat` shows 40 of 100 periods throttled in the window
- **THEN** the root cgroup is evaluated, the status is CRIT, and the first detail line says it is the container cgroup namespace root

#### Scenario: v1 controller paths
- **WHEN** `/proc/self/cgroup` has `4:cpu,cpuacct:/system.slice/app.service` and `10:memory:/system.slice/app.service`
- **THEN** CPU files come from `/sys/fs/cgroup/cpu,cpuacct/system.slice/app.service` and memory files from `/sys/fs/cgroup/memory/system.slice/app.service`

#### Scenario: Hybrid hierarchy prefers v1 controllers
- **WHEN** `/proc/self/cgroup` has both `4:cpu,cpuacct:/app` and `0::/app`
- **THEN** cgroup v1 is used

#### Scenario: v1 hierarchy mounted at the own cgroup
- **WHEN** `/proc/self/cgroup` has `4:cpu,cpuacct:/docker/abc`, and `cpu.stat` exists at `/sys/fs/cgroup/cpu,cpuacct/cpu.stat` but not under `/docker/abc`
- **THEN** the mount root is evaluated and the first detail line says the hierarchy is mounted at the own cgroup

### Requirement: CPU quota throttling
The cgroup check SHALL compute, between the first and the last sample of `cpu.stat`,
throttled% = Δnr_throttled ÷ Δnr_periods × 100, only when Δnr_periods > 0; otherwise
throttled% is 0 and the detail says `no CPU quota active`. It SHALL report the throttled time
per second as Δthrottled time ÷ Δt in ms/s (v2 `throttled_usec`, v1 `throttled_time` in ns).
The CPU quota SHALL come from v2 `cpu.max` or v1 `cpu.cfs_quota_us` ÷ `cpu.cfs_period_us`.
When the cumulative `nr_periods` is non-zero, a detail line SHALL show the throttled share since
the cgroup was created. Metrics SHALL be `throttled_pct` and `throttled_ms_per_sec`.

#### Scenario: Throttled share and time
- **WHEN** over a 1 s window Δnr_periods is 100, Δnr_throttled 34 and Δthrottled_usec 180000
- **THEN** `throttled_pct` is 34 and `throttled_ms_per_sec` is 180

#### Scenario: v1 throttled time in nanoseconds
- **WHEN** over a 1 s window of the v1 legacy tree Δnr_periods is 10, Δnr_throttled 4 and Δthrottled_time 50,000,000 ns
- **THEN** `throttled_pct` is 40 and `throttled_ms_per_sec` is 50

#### Scenario: No quota periods
- **WHEN** Δnr_periods is 0
- **THEN** `throttled_pct` is 0, a detail says `no CPU quota active`, and there is no throttling finding

#### Scenario: Throttling since creation
- **WHEN** the window is empty but cumulative `cpu.stat` has 864000 periods of which 43200 throttled
- **THEN** a detail line shows 5.0% of periods throttled since creation, and the status is not raised

### Requirement: Throttling thresholds
The cgroup check SHALL report WARN when throttled% exceeds 10 and CRIT when it exceeds 25, with
the finding `CPU quota throttling: … raise cpu.max / limits.cpu or reduce parallelism`.

#### Scenario: At 10 percent
- **WHEN** 10 of 100 periods are throttled in the window
- **THEN** the status is OK

#### Scenario: Above 10 percent
- **WHEN** 101 of 1000 periods are throttled in the window (10.1%)
- **THEN** the status is WARN with the CPU quota throttling finding

#### Scenario: At 25 percent
- **WHEN** 25 of 100 periods are throttled in the window
- **THEN** the status is WARN

#### Scenario: Above 25 percent
- **WHEN** 251 of 1000 periods are throttled in the window (25.1%)
- **THEN** the status is CRIT with the CPU quota throttling finding

### Requirement: Memory limit events
For cgroup v2 the cgroup check SHALL compare `memory.events` between the first and the last
sample: Δ`oom_kill` > 0 SHALL be CRIT, Δ`max` > 0 SHALL be WARN (the cgroup hit `memory.max`,
reclaim or OOM imminent), and Δ`high` > 0 SHALL be a note (throttled at `memory.high`). Events
counted before the window SHALL NOT raise the status. For cgroup v1, Δ`memory.failcnt` > 0 SHALL
be WARN. Metrics SHALL be `memory_max_events` (Δmax, or Δfailcnt for v1) and `oom_kills`
(Δoom_kill, v2 only).

#### Scenario: OOM kill in the window
- **WHEN** `oom_kill` in `memory.events` grows from 0 to 1 during the window
- **THEN** the status is CRIT and `oom_kills` is 1

#### Scenario: memory.max hit in the window
- **WHEN** `max` grows from 5 to 7 and `oom_kill` stays the same
- **THEN** the status is WARN and `memory_max_events` is 2

#### Scenario: memory.high throttling
- **WHEN** only `high` grows during the window
- **THEN** there is a note and the status stays OK

#### Scenario: Old events only
- **WHEN** `memory.events` has `max 9` and `oom_kill 2` that do not change during the window
- **THEN** the status is OK and the summary says `no memory events`

#### Scenario: v1 failcnt
- **WHEN** v1 `memory.failcnt` grows from 3 to 4 during the window
- **THEN** the status is WARN and `memory_max_events` is 1; when it does not grow the status is OK

### Requirement: Own cgroup summary
The summary SHALL combine the CPU part (`cpu quota <n> (throttled <p>% of periods, <ms> ms/s)`,
`cpu quota <n> (not throttled)`, `no cpu quota`, or `throttled <p>% of periods, <ms> ms/s`
without a quota), the memory part (`memory <usage> of <limit>` or `memory <usage> (no limit)`,
with v1 limits at or above physical memory counting as no limit) and the event part
(`no memory events`, or the non-zero window counts of OOM kills, max and high events;
`no memory limit hits` or `<n> memory limit hits` for v1).

#### Scenario: Throttled container
- **WHEN** `cpu.max` is `150000 100000`, over 1 s 34 of 100 periods are throttled with 180000 µs throttled, `memory.current` is 410 MiB, `memory.max` 512 MiB and no memory events change
- **THEN** the summary is `cpu quota 1.5 (throttled 34% of periods, 180 ms/s), memory 410 MiB of 512 MiB, no memory events` and the status is CRIT

### Requirement: Own cgroup SKIPPED
The cgroup check SHALL be SKIPPED only when `/proc/self/cgroup` cannot be read or has no usable
line, or when none of the own cgroup's CPU and memory files can be read.

#### Scenario: No cgroup files
- **WHEN** `/proc/self/cgroup` is missing
- **THEN** the check is SKIPPED with the read error

#### Scenario: Own cgroup files unreadable
- **WHEN** `/proc/self/cgroup` is `0::/app` but no file under `/sys/fs/cgroup/app` can be read
- **THEN** the check is SKIPPED
