# Spec Delta

## MODIFIED Requirements

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
