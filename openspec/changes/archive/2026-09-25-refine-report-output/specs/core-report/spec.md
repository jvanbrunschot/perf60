# Spec Delta

## MODIFIED Requirements

### Requirement: Command-line interface
The tool SHALL accept `--interval <seconds>` (default 1, a positive number), `--count <n>`
(default 5, integer ≥ 1), `--json`, `--no-color`, `-v/--verbose`, `-h/--help` and
`-V/--version`. Invalid or unknown arguments SHALL print an error with usage to stderr and exit
with code 3.

#### Scenario: Defaults
- **WHEN** perf60 is run without arguments
- **THEN** it samples for 5 intervals of 1 second and prints the text report

#### Scenario: Invalid argument
- **WHEN** perf60 is run with `--count 0` or `--bogus`
- **THEN** it prints an error and usage to stderr and exits with code 3

#### Scenario: Verbose flag
- **WHEN** perf60 is run with `-v`
- **THEN** the text report includes detail lines for every section

### Requirement: Text report
The text report SHALL start with a one-line system summary, then the overall status with WARN
and CRIT counts. After that comes one line per section: a status tag, the section id padded to
the longest section id, and the summary. Detail lines SHALL follow only for sections whose status
is not OK, or for every section when `--verbose` is given. Finding lines (notes, warnings,
criticals) SHALL always follow. ANSI colors SHALL be used only when stdout is a terminal,
`--no-color` is absent and the `NO_COLOR` environment variable is unset.

#### Scenario: Piped output has no colors
- **WHEN** stdout is not a terminal
- **THEN** the report contains no ANSI escape sequences

#### Scenario: OK section details hidden
- **WHEN** a section is OK and has detail lines, and `--verbose` is not given
- **THEN** only its summary line and findings are printed

#### Scenario: Problem section details shown
- **WHEN** a section is WARN or CRIT
- **THEN** its detail lines are printed below the summary

#### Scenario: Long section ids stay aligned
- **WHEN** the report contains the section id `cpu-balance`
- **THEN** every summary starts in the same column, at least one space after the longest id
