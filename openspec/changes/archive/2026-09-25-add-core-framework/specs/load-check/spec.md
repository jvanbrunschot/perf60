# Spec Delta

## Purpose

Equivalent of `uptime`: shows the 1, 5 and 15 minute load averages against CPU capacity,
and whether load is rising or falling.

## ADDED Requirements

### Requirement: Load averages
The load check SHALL read `/proc/loadavg` and report load1, load5 and load15, runnable/total
tasks, and the effective CPU count. It SHALL expose them as metrics `load1`, `load5`, `load15`
and `cpus`.

#### Scenario: Summary
- **WHEN** loadavg is `1.20 0.90 0.80 2/345 999` on 8 effective CPUs
- **THEN** the summary shows `1.20 0.90 0.80` and `8 cpus`, and the status is OK

### Requirement: Load thresholds
The load check SHALL report WARN when load1 exceeds the effective CPU count and CRIT when load1
exceeds twice the effective CPU count. Linux load includes tasks in uninterruptible (D) state,
so the finding SHALL say that high load can also mean I/O or lock contention.

#### Scenario: Warn above CPU count
- **WHEN** load1 is 9.0 on 8 effective CPUs
- **THEN** the status is WARN

#### Scenario: Crit above twice CPU count
- **WHEN** load1 is 16.5 on 8 effective CPUs
- **THEN** the status is CRIT

#### Scenario: At CPU count
- **WHEN** load1 is exactly 8.0 on 8 effective CPUs
- **THEN** the status is OK

### Requirement: Load trend
The load check SHALL add an informational note "load rising" when load1 exceeds 1.5 × load15
and load1 ≥ 0.5 × effective CPUs. It SHALL add "load falling" when load15 exceeds 1.5 × load1
and load15 ≥ 0.5 × effective CPUs. Notes SHALL NOT change the status.

#### Scenario: Rising
- **WHEN** load averages are `6.0 3.0 2.0` on 8 effective CPUs
- **THEN** the section includes the note "load rising" and the status stays OK

### Requirement: Load skip
When `/proc/loadavg` cannot be read, the load check SHALL be SKIPPED.

#### Scenario: Missing loadavg
- **WHEN** `/proc/loadavg` does not exist
- **THEN** the load section is SKIPPED
