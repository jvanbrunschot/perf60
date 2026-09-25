## Purpose
The eBPF equivalent of BCC `runqlat`: how long runnable tasks wait on a CPU run queue before
they run, as a distribution over the sampling window.

## ADDED Requirements

### Requirement: Run queue latency measurement
The runqlat probe SHALL attach to the raw tracepoints `sched_wakeup`, `sched_wakeup_new` and
`sched_switch`. A task's wait SHALL start when it is woken, when it is new, or when it is
switched out while still runnable (state 0, `TASK_RUNNING`), and SHALL end when it is switched
in. Each wait SHALL be added in microseconds to a log2 histogram (`perf60_common::log2_bucket`).
The idle task (pid 0) SHALL be ignored.

#### Scenario: Preempted task
- **WHEN** a task is switched out while its state is `TASK_RUNNING` and is switched in again 3 ms later
- **THEN** one wait is added to the 2048–4095 µs bucket

### Requirement: Kernel data access
The probe SHALL read task fields at offsets from the kernel's BTF: `task_struct.pid`, and
`task_struct.__state`, or `task_struct.state` when `__state` does not exist (kernels before
5.14). Start times SHALL be kept in an LRU hash of 10240 entries, so a full map evicts old
entries instead of stopping the measurement.

#### Scenario: Offsets on an older kernel
- **WHEN** the kernel's BTF has `task_struct.state` but no `task_struct.__state`
- **THEN** the probe uses the offset of `task_struct.state`

#### Scenario: No BTF
- **WHEN** `/sys/kernel/btf/vmlinux` is missing
- **THEN** the runqlat section is SKIPPED with a reason naming `/sys/kernel/btf/vmlinux` and `CONFIG_DEBUG_INFO_BTF`

#### Scenario: No privileges
- **WHEN** perf60 runs with `--deep` in a container with default capabilities
- **THEN** the runqlat section is SKIPPED with the needs-root reason

### Requirement: Runqlat section
The section SHALL have id `runqlat`, title `Run queue latency (eBPF)`, equivalent
`runqlat (BCC)` and resource cpu. Percentiles SHALL be the upper bound of the log2 bucket that
holds them (`deep::hist`). The summary SHALL be `p50 <p50> p99 <p99> max <max> (<N> wakeups)`,
and the details SHALL be the BCC-style distribution lines of the non-empty bucket range. It
SHALL expose metrics `runq_p50_us`, `runq_p99_us`, `runq_max_us` and `runq_events`.

#### Scenario: Distribution summary
- **WHEN** the window has 90 waits in 8–15 µs, 9 in 1024–2047 µs and 1 in 8192–16383 µs
- **THEN** the summary is `p50 16µs p99 2ms max 16.4ms (100 wakeups)`, the metrics are `runq_p50_us` 16, `runq_p99_us` 2048, `runq_max_us` 16384 and `runq_events` 100, and there are 11 distribution lines

#### Scenario: No wakeups
- **WHEN** no wait is recorded during the window
- **THEN** the summary is `no wakeups`, the status is OK, there are no details and `runq_events` is 0

### Requirement: Run queue latency thresholds
The runqlat section SHALL report WARN when the p99 bucket upper bound exceeds 10 000 µs and
CRIT when it exceeds 50 000 µs, with the finding `tasks wait up to <p99> for a CPU (p99): CPU
saturation or a runaway high-priority task`. Because upper bounds are powers of two, a p99 in
the 4096–8191 µs bucket (upper bound 8192) is OK, 8192–16383 µs (16384) and 16384–32767 µs
(32768) WARN, and 32768–65535 µs (65536) or higher CRIT.

#### Scenario: p99 below the warning threshold
- **WHEN** the p99 falls in the 4096–8191 µs bucket (upper bound 8192 µs)
- **THEN** the status is OK

#### Scenario: p99 above the warning threshold
- **WHEN** the p99 falls in the 8192–16383 µs bucket (upper bound 16384 µs)
- **THEN** the status is WARN and the finding starts with `tasks wait up to 16.4ms for a CPU (p99)`

#### Scenario: p99 at the top of the warning range
- **WHEN** the p99 falls in the 16384–32767 µs bucket (upper bound 32768 µs)
- **THEN** the status is WARN

#### Scenario: p99 above the critical threshold
- **WHEN** the p99 falls in the 32768–65535 µs bucket (upper bound 65536 µs)
- **THEN** the status is CRIT
