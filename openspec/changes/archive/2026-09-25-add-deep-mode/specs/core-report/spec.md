# Spec Delta

## MODIFIED Requirements

### Requirement: Command-line interface
The tool SHALL accept `--interval <seconds>` (default 1, a positive number), `--count <n>`
(default 5, integer ≥ 1), `--json`, `--no-color`, `-v/--verbose`, `--deep`, `-h/--help` and
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

#### Scenario: Deep flag
- **WHEN** perf60 is run with `--deep`
- **THEN** the eBPF probe sections are added to the report
