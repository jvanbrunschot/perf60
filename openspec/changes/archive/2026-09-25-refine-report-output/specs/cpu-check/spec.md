# Spec Delta

## MODIFIED Requirements

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
