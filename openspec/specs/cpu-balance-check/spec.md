# cpu-balance-check Specification

## Purpose
Equivalent of `mpstat -P ALL 1`: shows per-CPU busy time over the sampling window and flags a
single hot CPU while the others are mostly idle, which points at a single-threaded bottleneck or
an interrupt (IRQ) imbalance.

## Requirements

### Requirement: Per-CPU busy summary
The cpu-balance check SHALL read `/proc/stat` on every sample and compute, for every `cpuN` line
present in both the first and the last sample, busy% = 100 − idle% − iowait% over the window.
The summary SHALL have the form `4 cpus, avg 23%, max cpu2 97%`. Detail lines SHALL show the
busiest CPUs, at most 4, busiest first, each with usr (user + nice), sys (system), irq+soft
(irq + softirq) and iowait percentages. Metrics SHALL be `avg_busy`, `max_busy` and `max_cpu`
(the CPU index).

#### Scenario: Summary
- **WHEN** 4 CPUs are 10%, 20%, 97% and 5% busy over the window
- **THEN** the summary is `4 cpus, avg 33%, max cpu2 97%`, `max_cpu` is 2 and `max_busy` is 97

#### Scenario: At most 4 detail lines
- **WHEN** there are 6 CPUs
- **THEN** there are 4 detail lines, the first one for the busiest CPU

#### Scenario: Single CPU
- **WHEN** there is only one CPU and it is 99% busy
- **THEN** the summary starts with `1 cpu` and the status is OK

#### Scenario: Identical snapshots
- **WHEN** two samples are identical (no ticks elapsed)
- **THEN** every CPU is 0% busy, every metric is a finite number, and the status is OK

### Requirement: Hot CPU imbalance
With at least 2 CPUs, the cpu-balance check SHALL report WARN when some CPU is more than 90%
busy while the mean busy% across CPUs is below 50%, with a finding that names the CPU and says
it may be a single-threaded bottleneck or IRQ imbalance. When more than 50% of that CPU's busy
time is irq + softirq, the finding SHALL say "interrupt imbalance (check IRQ affinity / RPS)".

#### Scenario: Hot CPU at 90%
- **WHEN** 4 CPUs are 90%, 5%, 5% and 5% busy
- **THEN** the status is OK

#### Scenario: Hot CPU above 90%
- **WHEN** 4 CPUs are 5%, 5%, 91% and 5% busy
- **THEN** the status is WARN and the finding names cpu2

#### Scenario: Mean at 50%
- **WHEN** 2 CPUs are 95% and 5% busy (mean 50%)
- **THEN** the status is OK

#### Scenario: Mean below 50%
- **WHEN** 2 CPUs are 95% and 4% busy (mean 49.5%)
- **THEN** the status is WARN

#### Scenario: Interrupt imbalance
- **WHEN** the hot CPU is 95% busy and 60 of those 95 points are irq + softirq
- **THEN** the status is WARN and the finding says "interrupt imbalance (check IRQ affinity / RPS)"

#### Scenario: Interrupts at half of busy time
- **WHEN** the hot CPU is 96% busy and exactly 48 of those points are irq + softirq
- **THEN** the status is WARN and the finding does not say "interrupt imbalance"

### Requirement: CPU balance skip
The cpu-balance check SHALL be SKIPPED when `/proc/stat` cannot be read or parsed or has no
per-CPU lines, and SKIPPED with "not enough samples" when fewer than two samples were taken.

#### Scenario: Missing /proc/stat
- **WHEN** `/proc/stat` does not exist
- **THEN** the cpu-balance section is SKIPPED and the reason mentions `/proc/stat`

#### Scenario: Single sample
- **WHEN** only one sample was taken
- **THEN** the cpu-balance section is SKIPPED with "not enough samples"
