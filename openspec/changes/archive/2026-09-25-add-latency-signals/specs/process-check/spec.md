## ADDED Requirements

### Requirement: Fork rate
The process check SHALL read the `processes` counter (forks since boot) from `/proc/stat` on
every sample and compute forks/s = Δprocesses ÷ Δt between the first and the latest sample
that had the counter, exposed as metric `forks_per_sec`. It SHALL add a note "short-lived processes: N forks/s; top-5 misses them, try
--deep execsnoop" when the rate exceeds 100/s, and report WARN with the same message when it
exceeds 1000/s. When `/proc/stat` is missing, cannot be parsed or has no `processes` line in
at least two samples, the check SHALL report no fork rate metric or finding and SHALL
NOT be SKIPPED because of it. The fork rate SHALL also be reported when no other process is
visible.

#### Scenario: Fork rate at 100 per second
- **WHEN** processes grows by 1000 over a 10 s window (100/s)
- **THEN** `forks_per_sec` is 100 and there is no fork rate finding

#### Scenario: Fork rate above 100 per second
- **WHEN** processes grows by 1001 over a 10 s window (100.1/s)
- **THEN** there is a note about short-lived processes and the status is OK

#### Scenario: Fork rate at 1000 per second
- **WHEN** processes grows by 10000 over a 10 s window (1000/s)
- **THEN** the finding is a note and the status is OK

#### Scenario: Fork rate above 1000 per second
- **WHEN** processes grows by 10001 over a 10 s window (1000.1/s)
- **THEN** the status is WARN and the finding mentions short-lived processes

#### Scenario: No fork counter
- **WHEN** `/proc/stat` is missing or has no `processes` line
- **THEN** the process section is not SKIPPED and has no `forks_per_sec` metric
