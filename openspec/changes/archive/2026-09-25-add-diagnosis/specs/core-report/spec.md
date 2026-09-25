# Spec Delta

## MODIFIED Requirements

### Requirement: Text report
The text report SHALL start with a one-line system summary, then the overall status with WARN
and CRIT counts. When there is a diagnosis, a `Likely bottleneck: <label>` line follows,
with one `· <evidence>` line per piece of evidence and an `also: <labels>` line when other
resources have problems. After that comes one line per section: a status tag, the section id padded to
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

#### Scenario: Diagnosis under the overall line
- **WHEN** the disk section is WARN with finding "vda saturated"
- **THEN** the lines after OVERALL are `Likely bottleneck: disk I/O` and `  · disk: vda saturated`

### Requirement: JSON report
With `--json`, the tool SHALL print one JSON object with `version`, `sampling`
(`interval`, `count`), `system`, `overall`, `diagnosis` (an object with `bottleneck`,
`resource`, `status`, `evidence` and `also`, or `null` when no section is WARN or CRIT) and a
`sections` array. Each section object has
`id`, `title`, `equivalent` (the article command), `resource`, `status`, `summary`, `details`,
`findings` and `metrics`. Findings carry `level` and `message`, and `resource` when the finding
concerns another resource than its section. `resource` SHALL be one of `cpu`, `memory`, `disk`, `network`,
`capacity`, `hardware`, `kernel` or `pressure`.

#### Scenario: JSON is parseable
- **WHEN** run with `--json`
- **THEN** stdout is a single valid JSON document containing every section

#### Scenario: Resource per section
- **WHEN** run with `--json`
- **THEN** the `disk` section has `"resource": "disk"` and the `cpu` section has `"resource": "cpu"`

#### Scenario: Diagnosis is null when all is well
- **WHEN** every section is OK or SKIPPED
- **THEN** `diagnosis` is `null`

## ADDED Requirements

### Requirement: Diagnosis
The report SHALL derive a likely bottleneck from the WARN and CRIT findings of all non-SKIPPED
sections. Each finding counts for its own resource when it names one, else for its section's
resource. Resources SHALL be ranked by: any CRIT finding, then the number of distinct sections
with findings for that resource, then the number of CRIT findings, then WARN findings. A
resource whose findings all come from the kernel log (resource `kernel`) SHALL lose ties. The
top resource SHALL be labelled:
- cpu: `CPU quota throttling (cgroup)` when a finding comes from `cgroup` or `cgroups-top`,
  `single hot CPU (single-threaded or IRQ bottleneck)` when all findings come from
  `cpu-balance`, else `CPU saturation`
- memory: `memory limit (cgroup)` when a finding comes from `cgroup`, `memory pressure
  (swapping or thrashing)` when one comes from `swap` or mentions thrashing, else `memory`
- disk: `disk I/O`
- network: `network capacity (sockets/conntrack)` when a finding comes from `sockets`, else
  `network`
- capacity: `capacity limits (<sections>)`
- hardware: `hardware fault`
- kernel: `kernel errors (see kernel-log)`

Evidence SHALL be at most 3 findings of the top resource from distinct sections, CRIT first,
each as `<section>: <what>`. `<what>` is the shortest leading `: `-separated part of the message
that contains a number (the whole message when none does), capped at 72 characters. Up to 3
other resources SHALL be listed as `also`, in rank order.

#### Scenario: CPU quota wins over host CPU and capacity
- **WHEN** cpu has a CRIT run-queue finding, pressure a CRIT cpu-pressure finding, cgroup a CRIT throttling finding and filesystems a WARN
- **THEN** the bottleneck is `CPU quota throttling (cgroup)` with evidence from cpu, pressure and cgroup, and `also` is `capacity limits (filesystems)`

#### Scenario: I/O-bound system
- **WHEN** cpu reports iowait (resource disk), disk reports saturation and pressure reports io pressure (resource disk)
- **THEN** the bottleneck is `disk I/O`

#### Scenario: Agreement beats a lone warning
- **WHEN** net has one WARN, and memory and swap each have one WARN
- **THEN** the bottleneck is `memory pressure (swapping or thrashing)` and `also` is `network`

#### Scenario: Kernel log corroborates
- **WHEN** kernel-log has a WARN segfault and hardware a WARN thermal throttling finding
- **THEN** the bottleneck is `hardware fault`

#### Scenario: OOM kill in the kernel log
- **WHEN** the only finding is a CRIT kernel-log OOM kill (resource memory)
- **THEN** the bottleneck is `memory` with status CRIT

#### Scenario: Nothing wrong
- **WHEN** no section has a WARN or CRIT finding
- **THEN** there is no diagnosis
