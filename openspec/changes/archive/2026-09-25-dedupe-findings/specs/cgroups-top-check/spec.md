# Spec Delta

## MODIFIED Requirements

### Requirement: Throttled cgroups
The cgroups-top check SHALL report WARN when any leaf cgroup other than perf60's own cgroup is
throttled on more than 25% of its periods in the window. The own cgroup (from
`/proc/self/cgroup`) is judged by the `cgroup` section, so it SHALL NOT raise a cgroups-top
finding, although it is still listed. The finding SHALL name at most 3 such cgroups, the most
throttled first, followed by `+N more` when there are more.

#### Scenario: At 25 percent
- **WHEN** a leaf has 25 of 100 periods throttled
- **THEN** the status is OK and `throttled_cgroups` is 0

#### Scenario: Above 25 percent
- **WHEN** a leaf has 251 of 1000 periods throttled (25.1%)
- **THEN** the status is WARN and the finding names it

#### Scenario: Many throttled cgroups
- **WHEN** 5 leaves are throttled above 25%
- **THEN** the finding names 3 of them followed by `+2 more` and `throttled_cgroups` is 5

#### Scenario: Own cgroup is left to the cgroup section
- **WHEN** perf60 runs in a container whose only visible cgroup `/` is throttled on 100% of its periods
- **THEN** cgroups-top is OK, lists `/` with its CPU usage, and `throttled_cgroups` is 0
