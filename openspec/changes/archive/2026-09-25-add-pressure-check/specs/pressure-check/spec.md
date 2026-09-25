# Spec Delta

## Purpose

Modern complement to the final `top` step of the checklist: Pressure Stall Information (PSI)
shows how much wall time tasks lost waiting for CPU, memory and I/O, system-wide and for the
process's own cgroup.

## ADDED Requirements

### Requirement: PSI parsing
The pressure check SHALL read `/proc/pressure/cpu`, `/proc/pressure/memory` and
`/proc/pressure/io`, each with a `some` line and an optional `full` line of the form
`avg10=<pct> avg60=<pct> avg300=<pct> total=<usec>`, where `total` is the cumulative stall time
in microseconds. A missing `full` line (CPU on older kernels) SHALL be accepted. A file without
a `some` line SHALL be rejected as malformed.

#### Scenario: Some and full lines
- **WHEN** a pressure file contains `some avg10=1.50 avg60=0.75 avg300=0.25 total=12345` and `full avg10=0.10 avg60=0.05 avg300=0.01 total=678`
- **THEN** some has avg10 1.50, avg60 0.75, avg300 0.25 and total 12345, and full has total 678

#### Scenario: CPU without full line
- **WHEN** `/proc/pressure/cpu` contains only a `some` line
- **THEN** it parses with no full values

#### Scenario: Malformed file
- **WHEN** a pressure file has no `some` line
- **THEN** parsing fails

### Requirement: Window pressure
For each resource the pressure check SHALL compute the stall percentage over the sampling
window as Δtotal (µs) ÷ (Δt s × 1,000,000) × 100 between the first and last sample, for `some`
and, when present, `full`, clamped to 0–100%. A window of zero length SHALL yield 0%. The
summary SHALL list the window `some` percentage per available resource followed by
`(some, sampled)`, e.g. `cpu 3.2% memory 0.0% io 12.5% (some, sampled)`. There SHALL be one
detail line per resource with the window some/full percentages and the last sample's
avg10/avg60/avg300. Metrics SHALL be `cpu_some_pct`, `memory_some_pct`, `io_some_pct`,
`memory_full_pct` and `io_full_pct`.

#### Scenario: Summary from sampled totals
- **WHEN** over a 1 s window the cpu some total grows by 32,000 µs, memory by 0 and io by 125,000 µs
- **THEN** the summary is `cpu 3.2% memory 0.0% io 12.5% (some, sampled)`, `cpu_some_pct` is 3.2, `io_some_pct` is 12.5, and the status is WARN only because io exceeds 10%

#### Scenario: Zero-length window
- **WHEN** both samples are taken at the same instant
- **THEN** every window percentage is 0.0 and no metric is NaN

#### Scenario: Only some resources available
- **WHEN** `/proc/pressure/cpu` exists but `/proc/pressure/memory` and `/proc/pressure/io` do not
- **THEN** the check reports cpu only and is not SKIPPED

### Requirement: Some thresholds
Per resource, the pressure check SHALL report WARN when the window `some` percentage exceeds
10% and CRIT when it exceeds 25%. The finding SHALL name the resource and interpret it: cpu
"runnable tasks waiting for CPU", memory "tasks stalled on memory reclaim/swap", io "tasks
stalled on I/O".

#### Scenario: At 10 percent
- **WHEN** the window cpu some percentage is exactly 10.0%
- **THEN** the status is OK

#### Scenario: Above 10 percent
- **WHEN** the window io some percentage is 10.1%
- **THEN** the status is WARN and the finding mentions "tasks stalled on I/O"

#### Scenario: At 25 percent
- **WHEN** the window memory some percentage is exactly 25.0%
- **THEN** the status is WARN and the finding mentions "tasks stalled on memory reclaim/swap"

#### Scenario: Above 25 percent
- **WHEN** the window cpu some percentage is 25.1%
- **THEN** the status is CRIT and the finding mentions "runnable tasks waiting for CPU"

### Requirement: Full thresholds
The pressure check SHALL report WARN when the window `full` percentage of memory or io exceeds
5%, with a finding that all non-idle tasks were stalled at once. The system-wide CPU `full`
line SHALL be ignored (not meaningful at system level before kernel 5.13): it is neither shown
nor evaluated.

#### Scenario: Memory full above 5 percent
- **WHEN** the window memory full percentage is 5.1% and memory some is 8.0%
- **THEN** the status is WARN and the finding mentions "all non-idle tasks stalled at once"

#### Scenario: IO full at 5 percent
- **WHEN** the window io full percentage is exactly 5.0% and io some is 5.0%
- **THEN** the status is OK

#### Scenario: CPU full ignored
- **WHEN** the system cpu full total grows by 900,000 µs over 1 s while cpu some grows by 1,000 µs
- **THEN** the status is OK and no cpu full value is reported

### Requirement: Cgroup pressure
When `/proc/self/cgroup` names a cgroup v2 path (`0::<path>`) and
`/sys/fs/cgroup<path>/{cpu,memory,io}.pressure` exist, the pressure check SHALL add detail
lines for that cgroup in the same format and evaluate them with the same thresholds, with
findings prefixed by `cgroup`. This SHALL apply when the path is not `/`, and also when the
path is `/` inside a container (a cgroup namespace root is the container's own cgroup). At
`/` outside a container the cgroup files duplicate the system figures and SHALL NOT be
reported. The cgroup metrics SHALL be prefixed `cgroup_` (e.g. `cgroup_cpu_some_pct`).

#### Scenario: Cgroup CPU pressure reported
- **WHEN** the own cgroup is `/app` and its cpu some total grows by 400,000 µs over 1 s while system pressure is idle
- **THEN** there is a cgroup cpu detail line, `cgroup_cpu_some_pct` is 40.0, the status is CRIT and the finding starts with `cgroup cpu`

#### Scenario: Host root cgroup not duplicated
- **WHEN** the own cgroup is `/` outside a container
- **THEN** no cgroup detail lines are reported

#### Scenario: Container cgroup namespace root
- **WHEN** the own cgroup is `/` inside a container and `/sys/fs/cgroup/cpu.pressure` exists
- **THEN** the cgroup pressure is reported

### Requirement: Pressure skip
When `/proc/pressure/cpu` does not exist, the pressure check SHALL be SKIPPED with the reason
`PSI not available (kernel < 4.20, or booted with psi=0)`. When it exists but cannot be read or
parsed, it SHALL be SKIPPED with the read error.

#### Scenario: No PSI
- **WHEN** `/proc/pressure/cpu` does not exist
- **THEN** the pressure section is SKIPPED with reason `PSI not available (kernel < 4.20, or booted with psi=0)`
