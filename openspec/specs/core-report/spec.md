# core-report Specification

## Purpose
Defines how perf60 is invoked, how it samples the system, how individual checks report
their status, and how the overall report and exit code are produced.

## Requirements

### Requirement: Command-line interface
The tool SHALL accept `--interval <seconds>` (default 1, a positive number), `--count <n>`
(default 5, integer ≥ 1), `--json`, `--no-color`, `-h/--help` and `-V/--version`. Invalid or
unknown arguments SHALL print an error with usage to stderr and exit with code 3.

#### Scenario: Defaults
- **WHEN** perf60 is run without arguments
- **THEN** it samples for 5 intervals of 1 second and prints the text report

#### Scenario: Invalid argument
- **WHEN** perf60 is run with `--count 0` or `--bogus`
- **THEN** it prints an error and usage to stderr and exits with code 3

### Requirement: No external commands
The tool SHALL NOT execute any external program. All data SHALL come from files under
`/proc`, `/sys`, `/etc` and `/dev/kmsg`, or from direct system calls.

#### Scenario: Minimal container
- **WHEN** perf60 runs in a container with no procps, sysstat or shell utilities
- **THEN** it produces a complete report

### Requirement: Sampling
The tool SHALL take `count + 1` snapshots spaced `interval` seconds apart. Rate metrics
SHALL use the measured elapsed time between snapshots. Rate-based checks SHALL report the
average over the whole window and SHALL report the peak interval where the check spec says so.

#### Scenario: Window length
- **WHEN** run with `--interval 0.5 --count 4`
- **THEN** the report header states the sampling window as 4×0.5s and the run takes about 2 seconds

### Requirement: Check status
Each check SHALL produce a section with status OK, WARN, CRIT or SKIPPED, a one-line summary,
optional detail lines, optional findings, and machine-readable metrics. A section's status SHALL be
the most severe of its findings, or OK when there are none.

#### Scenario: Finding escalates section
- **WHEN** a check records a WARN finding and no CRIT finding
- **THEN** the section status is WARN

### Requirement: Graceful degradation
When a check's data source is missing or unreadable, the check SHALL be reported as SKIPPED
with the reason, and the remaining checks SHALL still run.

#### Scenario: Missing source
- **WHEN** a source file of one check does not exist
- **THEN** that section is SKIPPED with a reason naming the file, and the other sections are reported

### Requirement: Overall status and exit code
The overall status SHALL be the most severe status among non-SKIPPED sections, or OK when all
are OK or SKIPPED. The exit code SHALL be 0 for OK, 1 for WARN and 2 for CRIT.

#### Scenario: Warning exit code
- **WHEN** the most severe section status is WARN
- **THEN** the overall status is WARN and the exit code is 1

#### Scenario: Skipped does not fail
- **WHEN** one section is SKIPPED and the others are OK
- **THEN** the overall status is OK and the exit code is 0

### Requirement: Text report
The text report SHALL start with a one-line system summary, then the overall status with WARN
and CRIT counts. After that comes one line per section: a status tag, the section id and the
summary, followed by indented detail and finding lines. ANSI colors SHALL be used only when
stdout is a terminal, `--no-color` is absent and the `NO_COLOR` environment variable is unset.

#### Scenario: Piped output has no colors
- **WHEN** stdout is not a terminal
- **THEN** the report contains no ANSI escape sequences

### Requirement: JSON report
With `--json`, the tool SHALL print one JSON object with `version`, `sampling`
(`interval`, `count`), `system`, `overall` and a `sections` array. Each section object has
`id`, `title`, `equivalent` (the article command), `status`, `summary`, `details`,
`findings` and `metrics`.

#### Scenario: JSON is parseable
- **WHEN** run with `--json`
- **THEN** stdout is a single valid JSON document containing every section

### Requirement: Non-Linux hosts
On operating systems other than Linux, the tool SHALL print that it supports Linux only and exit
with code 3.

#### Scenario: macOS
- **WHEN** perf60 runs on macOS
- **THEN** it prints "perf60 supports Linux only" and exits with code 3
