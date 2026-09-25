# cpu-check Specification

## Purpose
Equivalent of `vmstat 1`: shows where system-wide CPU time goes over the sampling window
(user, system, idle, I/O wait, steal), the run queue and blocked tasks, and context switch and
interrupt rates, and flags CPU saturation, I/O-bound and hypervisor-steal conditions.

## Requirements

### Requirement: CPU utilization summary
The cpu check SHALL read `/proc/stat` on every sample and compute, from the aggregate `cpu` line
between the first and the last sample, the window percentages us (user + nice), sy (system + irq
+ softirq), id (idle), wa (iowait) and st (steal) of total CPU time. Guest time SHALL NOT be
counted twice, because the kernel already includes it in user and nice. It SHALL report
r = the average of `procs_running` over all samples minus 1 (perf60 itself), floored at 0,
b = the average of `procs_blocked`, and the context switch and interrupt rates per second over
the window. The summary SHALL have the form `us 62% sy 8% id 6% wa 24% st 0%  r=3.0 b=4.0  cs 12k/s`.
A detail line SHALL show the peak busy% (100 − idle − iowait) of any single interval between
consecutive samples, and the interrupt rate. Metrics SHALL be `us`, `sy`, `id`, `wa`, `st`, `r`,
`b`, `cs_per_sec`, `intr_per_sec` and `busy_peak`.

#### Scenario: Summary over one interval
- **WHEN** over a 1 s window the CPU time splits 50% user, 10% nice, 5% system, 2% irq, 1% softirq, 30% idle and 2% iowait, ctxt grows by 12000, and procs_running is 3 and 4 in the two samples
- **THEN** the summary contains `us 60% sy 8% id 30% wa 2% st 0%`, `r=2.5` and `cs 12k/s`, and the status is OK

#### Scenario: Guest time is not double counted
- **WHEN** a CPU line reports guest ticks in addition to user ticks
- **THEN** the percentages are computed from user, nice, system, idle, iowait, irq, softirq and steal only, and still sum to 100%

#### Scenario: Peak interval busy
- **WHEN** three samples are taken and the first interval is 20% busy and the second is 80% busy
- **THEN** the metric `busy_peak` is 80 and the detail shows the peak

#### Scenario: Run queue excludes perf60
- **WHEN** procs_running is 1 in every sample
- **THEN** r is 0.0

#### Scenario: Identical snapshots
- **WHEN** two samples are identical (no ticks elapsed)
- **THEN** the check reports id 100%, all other percentages 0%, rates 0/s, every metric is a finite number, and the status is OK

### Requirement: Run queue saturation
The cpu check SHALL report WARN when r exceeds the effective CPU count and CRIT when r exceeds
twice the effective CPU count, with a finding that says the run queue shows CPU saturation. The
threshold SHALL only apply when the CPUs in use during the window (busy % × online CPUs ÷ 100,
where busy = us + sy + st) are at least 50% of the effective CPU count. A long run queue on
mostly idle CPUs comes from sampling the instantaneous `procs_running` count, not from
saturation.

#### Scenario: Run queue at CPU count
- **WHEN** r is exactly 4.0 on 4 effective CPUs that are 95% busy
- **THEN** the status is OK

#### Scenario: Run queue above CPU count
- **WHEN** r is 5.0 on 4 effective CPUs that are 95% busy
- **THEN** the status is WARN and the finding mentions CPU saturation

#### Scenario: Run queue above twice CPU count
- **WHEN** r is 9.0 on 4 effective CPUs that are 95% busy
- **THEN** the status is CRIT

#### Scenario: Run queue on idle CPUs
- **WHEN** r is 5.0 on 4 effective CPUs that are 19% busy
- **THEN** no run-queue finding is reported

#### Scenario: Run queue under a cgroup quota
- **WHEN** r is 4.0, 4 CPUs are online, the cgroup quota is 1.5 CPUs and the host is 25% busy (1 CPU in use, 67% of the quota)
- **THEN** the status is CRIT

### Requirement: I/O wait
The cpu check SHALL report WARN when wa exceeds 20% and CRIT when wa exceeds 50%, with a finding
that says the system is I/O bound.

#### Scenario: I/O wait at 20%
- **WHEN** wa is exactly 20%
- **THEN** the status is OK

#### Scenario: I/O wait above 20%
- **WHEN** wa is 21%
- **THEN** the status is WARN and the finding mentions I/O

#### Scenario: I/O wait above 50%
- **WHEN** wa is 51%
- **THEN** the status is CRIT

### Requirement: Steal time
The cpu check SHALL report WARN when st exceeds 10% and CRIT when st exceeds 25%, with a finding
that names hypervisor steal (a noisy neighbour).

#### Scenario: Steal at 10%
- **WHEN** st is exactly 10%
- **THEN** the status is OK

#### Scenario: Steal above 10%
- **WHEN** st is 11%
- **THEN** the status is WARN and the finding mentions steal

#### Scenario: Steal above 25%
- **WHEN** st is 26%
- **THEN** the status is CRIT

### Requirement: CPU busy
The cpu check SHALL report WARN when us + sy exceeds 90%.

#### Scenario: Busy at 90%
- **WHEN** us + sy is exactly 90%
- **THEN** the status is OK

#### Scenario: Busy above 90%
- **WHEN** us + sy is 91%
- **THEN** the status is WARN

### Requirement: Informational notes
The cpu check SHALL add a note "high system time, worth investigating" when sy exceeds 20%, and
a note that tasks are blocked on I/O when b is above 0. Notes SHALL NOT change the status.

#### Scenario: System time above 20%
- **WHEN** sy is 21% and us + sy is at most 90%
- **THEN** the section has the note "high system time" and the status is OK

#### Scenario: System time at 20%
- **WHEN** sy is exactly 20%
- **THEN** there is no system time note

#### Scenario: Blocked tasks
- **WHEN** procs_blocked is 0 and 1 in the two samples (b = 0.5)
- **THEN** the section has a note that tasks are blocked on I/O and the status is OK

### Requirement: CPU check skip
The cpu check SHALL be SKIPPED when `/proc/stat` cannot be read or parsed, and SKIPPED with
"not enough samples" when fewer than two samples were taken.

#### Scenario: Missing /proc/stat
- **WHEN** `/proc/stat` does not exist
- **THEN** the cpu section is SKIPPED and the reason mentions `/proc/stat`

#### Scenario: Single sample
- **WHEN** only one sample was taken
- **THEN** the cpu section is SKIPPED with "not enough samples"

### Requirement: Run-queue latency
The cpu check SHALL read `/proc/schedstat` (version 15 or later) on every sample; the window runs
from the first to the latest readable sample. From each `cpuN` line it SHALL take field 8, run_delay (ns spent waiting on a runqueue), and field 9, the
number of timeslices run. Over the window it SHALL compute the average wait per timeslice as
the sum over all CPUs of Δrun_delay ÷ the sum of Δtimeslices, in milliseconds, and the same
average for each CPU that ran at least one timeslice. It SHALL expose metrics `runq_wait_ms`
(all CPUs) and `runq_wait_max_cpu_ms` (the worst CPU), and add a detail line with both values
and the worst CPU's number. The check SHALL report WARN when the average wait exceeds 2 ms and
CRIT when it exceeds 10 ms, with a finding "tasks wait X ms for a CPU on average: CPU
saturation / run queue latency". When Δtimeslices is 0 (including when fewer than two samples were
readable), or `/proc/schedstat` is missing, has an unsupported version or cannot be parsed, the check SHALL
report no run-queue latency finding, detail or metric, and SHALL NOT be SKIPPED because of it.

#### Scenario: Run-queue wait at 2 ms
- **WHEN** during the window run_delay grows by 2,000,000 ns over 1 timeslice
- **THEN** `runq_wait_ms` is 2 and the status is OK

#### Scenario: Run-queue wait above 2 ms
- **WHEN** during the window run_delay grows by 2,010,000 ns over 1 timeslice (2.01 ms)
- **THEN** the status is WARN and the finding says tasks wait for a CPU

#### Scenario: Run-queue wait at 10 ms
- **WHEN** the average wait is exactly 10 ms per timeslice
- **THEN** the status is WARN, not CRIT

#### Scenario: Run-queue wait above 10 ms
- **WHEN** the average wait is 10.01 ms per timeslice
- **THEN** the status is CRIT

#### Scenario: Average over all CPUs and the worst CPU
- **WHEN** cpu0 waits 1,000,000 ns over 100 timeslices and cpu1 waits 9,000,000 ns over 100 timeslices
- **THEN** `runq_wait_ms` is 0.05, `runq_wait_max_cpu_ms` is 0.09, and the detail names cpu1

#### Scenario: No timeslices
- **WHEN** the schedstat counters do not change during the window
- **THEN** there is no `runq_wait_ms` metric and no run-queue latency finding

#### Scenario: Schedstat missing
- **WHEN** `/proc/schedstat` does not exist or has a version below 15
- **THEN** the cpu section is not SKIPPED and has no run-queue latency metric or finding
