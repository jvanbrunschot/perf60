# process-check Specification

## Purpose
Equivalent of `pidstat 1`: shows which processes use CPU during the sampling window, split into
user and system time, and flags processes stuck in uninterruptible sleep (D state) and zombies.

## Requirements

### Requirement: Process stat parsing
The process check SHALL read `/proc/<pid>/stat` for every numeric entry in `/proc` and extract
pid, comm, state, utime, stime, num_threads and starttime. The comm field SHALL be taken as the
text between the first `(` and the LAST `)`, so names containing spaces or parentheses parse
correctly.

#### Scenario: Comm with spaces and parentheses
- **WHEN** a stat line is `42 (my (weird) proc) R 1 ...` with utime 7 and stime 3
- **THEN** the comm is `my (weird) proc`, the state is `R`, utime is 7 and stime is 3

#### Scenario: Captured fixture
- **WHEN** `/proc/1/stat` from the linux-arm64 fixture is parsed
- **THEN** the pid is 1, the comm is `sh`, the state is `S` and num_threads is 1

### Requirement: Per-process CPU usage
The process check SHALL compute, per pid, %usr from Δutime, %sys from Δstime and %cpu from
Δ(utime + stime): clock ticks divided by the clock tick rate and by the elapsed time between the
first and the last sample in which that pid was seen, times 100. Like `pidstat`, %cpu MAY exceed
100 for multi-threaded processes. A pid seen in only one sample, or whose elapsed time is zero,
SHALL have 0% CPU. A pid whose starttime changes between samples SHALL be treated as a new
process (pid reuse). The check SHALL exclude its own pid.

#### Scenario: CPU percentage over the window
- **WHEN** a process's utime goes from 100 to 230 and stime from 50 to 62 between samples at
  t=0 and t=1 with 100 clock ticks per second
- **THEN** it reports %usr 130.0, %sys 12.0 and %cpu 142.0

#### Scenario: Process disappears mid-window
- **WHEN** a process is present at t=0 but its stat file is gone at t=1
- **THEN** the missing file is not an error, the process is not counted, and the section is not
  SKIPPED

#### Scenario: Pid reuse
- **WHEN** a pid's starttime changes between t=0 and t=1
- **THEN** that pid has 0% CPU instead of a delta across two different processes

### Requirement: Summary, details and metrics
The summary SHALL show the number of processes present in the last sample and the top process
by %cpu as `top: <comm> <cpu>% (pid <pid>)`, or `all idle` when no process used CPU. It SHALL
append `<n> in D state` and `<n> zombies` when those counts are non-zero. The details SHALL list
up to 5 processes with %cpu > 0, ordered by %cpu descending (ties by pid ascending), each as the
pid, the padded comm, %usr, %sys and %cpu. The check SHALL expose metrics `processes`,
`d_state`, `zombies` and `top_cpu_pct`.

#### Scenario: Top-N ordering
- **WHEN** 7 processes use 10%, 70%, 30%, 0%, 50%, 20% and 60% CPU on 8 CPUs
- **THEN** the details list exactly 5 processes in the order 70, 60, 50, 30, 20, the summary
  shows the 70% process as top, `top_cpu_pct` is 70 and the status is OK

#### Scenario: Idle system
- **WHEN** no process's utime or stime changes during the window
- **THEN** the summary ends in `all idle`, there are no details and `top_cpu_pct` is 0

### Requirement: Single process saturating the machine
The process check SHALL report WARN, naming the process, when one process's %cpu exceeds 90 ×
the effective CPU count (more than 90% of total capacity).

#### Scenario: Above 90% of capacity
- **WHEN** one process uses 185% CPU on 2 effective CPUs (limit 180%)
- **THEN** the status is WARN and the finding names that process

#### Scenario: At 90% of capacity
- **WHEN** one process uses exactly 180% CPU on 2 effective CPUs
- **THEN** the status is OK

### Requirement: Uninterruptible (D state) tasks
The process check SHALL count processes in state `D` at the last sample and list up to 5 as
`comm(pid)`. When the count exceeds the effective CPU count it SHALL report WARN (many tasks
stuck in uninterruptible sleep, usually I/O); otherwise, when at least 1 process is in D state,
it SHALL add a note that does not change the status.

#### Scenario: A few D-state tasks
- **WHEN** 2 processes are in D state on 4 effective CPUs
- **THEN** the section has a note listing both as `comm(pid)` and the status is OK

#### Scenario: D-state count equal to CPUs
- **WHEN** 4 processes are in D state on 4 effective CPUs
- **THEN** the status is OK with a note

#### Scenario: More D-state tasks than CPUs
- **WHEN** 5 processes are in D state on 4 effective CPUs
- **THEN** the status is WARN

### Requirement: Zombies
The process check SHALL count processes in state `Z` at the last sample and report WARN when
more than 50 zombies exist.

#### Scenario: Zombies at the limit
- **WHEN** 50 processes are in state Z
- **THEN** the status is OK and the summary shows `50 zombies`

#### Scenario: Many zombies
- **WHEN** 51 processes are in state Z
- **THEN** the status is WARN and `zombies` is 51

### Requirement: Process check skip
When `/proc` cannot be listed, or other processes are listed but none of their stat files could
be read in the last successful scan, the process check SHALL be SKIPPED. When the last scan
lists no process other than perf60 itself (e.g. perf60 is PID 1 in a container), the check
SHALL be OK with the summary `no other processes visible`.

#### Scenario: No /proc
- **WHEN** `/proc` does not exist
- **THEN** the processes section is SKIPPED with a reason naming `/proc`

#### Scenario: No readable process
- **WHEN** `/proc` lists process 42 but `/proc/42/stat` cannot be read
- **THEN** the processes section is SKIPPED

#### Scenario: perf60 alone
- **WHEN** `/proc` lists only perf60's own pid and non-numeric entries
- **THEN** the processes section is OK with summary `no other processes visible`
