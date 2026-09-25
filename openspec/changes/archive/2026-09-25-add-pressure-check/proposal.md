# Proposal

## Why

The last step of Gregg's 60-second checklist is `top`: a final look for anything the earlier
steps missed. Since Linux 4.20 the kernel measures directly how much time tasks lose waiting
for CPU, memory and I/O (Pressure Stall Information, PSI). PSI answers "is anything actually
being slowed down, and by which resource?" in one line, and cgroup v2 exposes the same numbers
per cgroup, which makes it the most useful single signal inside a container.

## What Changes

- New pure parser for `/proc/pressure/{cpu,memory,io}` (and the identical cgroup v2
  `{cpu,memory,io}.pressure` format).
- New `pressure` check ("Pressure stall (PSI)", equivalent `top / PSI`) that measures the
  percentage of wall time with stalled tasks over the sampling window from the cumulative
  `total` counters, and shows the kernel's avg10/avg60/avg300 as context.
- WARN/CRIT thresholds on the window `some` percentage per resource, a WARN on memory/io
  `full`, and the same evaluation for the process's own cgroup when it has pressure files.
- SKIPPED when the kernel has no PSI.

## Capabilities

### New Capabilities
- `pressure-check`: PSI triage for CPU, memory and I/O, system-wide and for the own cgroup.

### Modified Capabilities

## Impact

New files `src/procfs/pressure.rs` and `src/checks/pressure.rs`, one `pub mod` line in
`src/procfs/mod.rs`, and one registry entry in `src/checks/mod.rs` (last in the list). No new
dependencies.
