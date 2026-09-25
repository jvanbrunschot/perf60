## ADDED Requirements

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
