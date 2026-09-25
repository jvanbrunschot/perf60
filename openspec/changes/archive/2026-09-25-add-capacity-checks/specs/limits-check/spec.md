# Spec Delta

## Purpose

Equivalent of `ulimit -n`, `/proc/sys/fs/file-nr` and `pid_max`: shows how close the system and
its processes are to the kernel limits whose exhaustion makes `open()` fail with ENFILE/EMFILE
and `fork()` fail with EAGAIN.

## ADDED Requirements

### Requirement: Limits summary, details and metrics
The limits check SHALL report system file handles from `/proc/sys/fs/file-nr` (allocated,
unused, max), the total task count (the fourth field of `/proc/loadavg`, which counts threads)
against `/proc/sys/kernel/pid_max` and `/proc/sys/kernel/threads-max`, the own cgroup's pids use,
and every readable process's open fds against its soft open-files limit. The summary SHALL be
`fds <allocated>/<max> (<pct>%), tasks <tasks>/<pid_max>, cgroup pids <current>/<max>, highest
process <comm>(<pid>) <fds>/<limit> fds`, leaving out parts whose source is unavailable and the
cgroup part when the cgroup has no pids limit; when only the cgroup is readable and it has no
limit the summary is `cgroup pids <current> (no limit)`. A file-max of 9223372036854775807 SHALL be shown
as `unlimited`. There SHALL be one detail line per source. The check SHALL expose the metrics
`file_nr_pct`, `tasks_pct`, `threads_pct`, `cgroup_pids_pct` (only when a pids limit applies)
and `max_process_fd_pct`. The values come from the last sample.

#### Scenario: Summary on a legacy host
- **WHEN** file-nr is `3072 0 377212`, loadavg lists 287 tasks, pid_max is 32768, threads-max is 30245, the cgroup has 212 of 4096 pids and process `app` (4211) has 240 fds against a soft limit of 256
- **THEN** the summary is `fds 3072/377212 (0.8%), tasks 287/32768, cgroup pids 212/4096, highest process app(4211) 240/256 fds` and the status is WARN

#### Scenario: Unlimited file-max
- **WHEN** file-nr is `1604 0 9223372036854775807`
- **THEN** the summary starts with `fds 1604/unlimited` and the status is OK

### Requirement: System file handle thresholds
The limits check SHALL report WARN when allocated file handles are above 80% of file-max and
CRIT when they are above 90%.

#### Scenario: File handles exactly 80%
- **WHEN** file-nr is `800 0 1000`
- **THEN** the status is OK

#### Scenario: File handles above 80%
- **WHEN** file-nr is `801 0 1000` (80.1%)
- **THEN** the status is WARN

#### Scenario: File handles exactly 90%
- **WHEN** file-nr is `900 0 1000`
- **THEN** the status is WARN

#### Scenario: File handles above 90%
- **WHEN** file-nr is `901 0 1000` (90.1%)
- **THEN** the status is CRIT

### Requirement: Task thresholds
The limits check SHALL compare the total task count with pid_max and with threads-max
separately, and report WARN when either is above 80% and CRIT when either is above 90%.

#### Scenario: Tasks exactly 80% of pid_max
- **WHEN** loadavg lists 800 tasks and pid_max is 1000
- **THEN** the status is OK

#### Scenario: Tasks above 80% of pid_max
- **WHEN** loadavg lists 801 tasks and pid_max is 1000 (80.1%)
- **THEN** the status is WARN and the finding names pid_max

#### Scenario: Tasks exactly 90% of pid_max
- **WHEN** loadavg lists 900 tasks and pid_max is 1000
- **THEN** the status is WARN

#### Scenario: Tasks above 90% of pid_max
- **WHEN** loadavg lists 901 tasks and pid_max is 1000 (90.1%)
- **THEN** the status is CRIT

#### Scenario: Tasks exactly 80% of threads-max
- **WHEN** loadavg lists 800 tasks, pid_max is 4194304 and threads-max is 1000
- **THEN** the status is OK

#### Scenario: Tasks above 80% of threads-max
- **WHEN** loadavg lists 801 tasks and threads-max is 1000 (80.1%)
- **THEN** the status is WARN and the finding names threads-max

#### Scenario: Tasks exactly 90% of threads-max
- **WHEN** loadavg lists 900 tasks and threads-max is 1000
- **THEN** the status is WARN

#### Scenario: Tasks above 90% of threads-max
- **WHEN** loadavg lists 901 tasks and threads-max is 1000 (90.1%)
- **THEN** the status is CRIT

### Requirement: cgroup pids limit
The limits check SHALL compare `pids.current` with `pids.max` of the own cgroup. With cgroup v1
(a `pids` controller line in `/proc/self/cgroup`) it SHALL look in `/sys/fs/cgroup/pids/<path>`;
with cgroup v2 (the `0::<path>` line) in `/sys/fs/cgroup/<path>`. In both cases it SHALL walk from
the own cgroup up to the mount root (inside a container the listed path may not exist below the
mount) and use the level with the highest current ÷ max. A `pids.max` of `max` means no limit.
It SHALL report WARN when the use is above 80% and CRIT when it is above 90%.

#### Scenario: cgroup pids exactly 80%
- **WHEN** pids.current is 800 and pids.max is 1000
- **THEN** the status is OK

#### Scenario: cgroup pids above 80%
- **WHEN** pids.current is 801 and pids.max is 1000 (80.1%)
- **THEN** the status is WARN

#### Scenario: cgroup pids exactly 90%
- **WHEN** pids.current is 900 and pids.max is 1000
- **THEN** the status is WARN

#### Scenario: cgroup pids above 90%
- **WHEN** pids.current is 901 and pids.max is 1000 (90.1%)
- **THEN** the status is CRIT

#### Scenario: No pids limit
- **WHEN** pids.max is `max`
- **THEN** there is no `cgroup_pids_pct` metric, no cgroup part in the summary, and the detail says `no limit`

#### Scenario: cgroup v1 pids controller
- **WHEN** `/proc/self/cgroup` has the line `3:pids:/system.slice/app.service` and `/sys/fs/cgroup/pids/system.slice/app.service/` has pids.current 212 and pids.max 4096
- **THEN** the summary contains `cgroup pids 212/4096`

#### Scenario: cgroup v2 nested limit
- **WHEN** the own cgroup is `/a/b`, `/a/b` has pids.max `max` and `/a` has 950 of 1000 pids
- **THEN** the cgroup pids use is 95% and the status is CRIT

### Requirement: Per-process open files
For every numeric directory in `/proc`, the limits check SHALL count the entries of
`/proc/<pid>/fd` and compare them with the soft `Max open files` limit in `/proc/<pid>/limits`.
Processes whose fd directory or limits cannot be read (other users' processes without root, or
processes that exited) SHALL be ignored silently, as SHALL an `unlimited` soft limit. A process
above 90% of its soft limit SHALL cause a WARN that names up to 3 processes as
`<comm>(<pid>) <fds>/<limit>` (comm from `/proc/<pid>/stat`), highest first, followed by
`+<n> more` when there are more.

#### Scenario: Process exactly 90% of its limit
- **WHEN** a process has 900 open fds and a soft limit of 1000
- **THEN** the status is OK

#### Scenario: Process above 90% of its limit
- **WHEN** process `app` (4211) has 901 open fds and a soft limit of 1000 (90.1%)
- **THEN** the status is WARN and the finding names `app(4211) 901/1000`

#### Scenario: At most three processes named
- **WHEN** five processes are above 90% of their soft limits
- **THEN** the finding names the three highest and `+2 more`

#### Scenario: Unreadable fd directories
- **WHEN** one process's fd directory cannot be read and another process's can
- **THEN** the unreadable one is ignored without a finding and the readable one is reported

#### Scenario: Legacy fixture
- **WHEN** the check runs on the `linux-legacy` fixture tree, where `app` (4211) has 240 fds against a soft limit of 256
- **THEN** the status is WARN and the finding names `app(4211) 240/256`

### Requirement: Limits SKIPPED behavior
The limits check SHALL be SKIPPED only when none of file-nr, the task count with pid_max or
threads-max, the cgroup pids files and any process's fds and limits can be read. The reason SHALL
name the first source that failed.

#### Scenario: Nothing readable
- **WHEN** none of the sources exist
- **THEN** the section is SKIPPED and the reason names `/proc/sys/fs/file-nr`

#### Scenario: Only file-nr readable
- **WHEN** only `/proc/sys/fs/file-nr` can be read
- **THEN** the section is not SKIPPED and the summary is `fds <allocated>/<max> (<pct>%)`
