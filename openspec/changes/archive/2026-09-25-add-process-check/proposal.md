# Proposal

## Why

Step 5 of Gregg's checklist is `pidstat 1`: a rolling per-process CPU breakdown that shows which
process is burning CPU, split into user and system time. Minimal servers usually don't have
sysstat installed, so perf60 has to derive the same view from `/proc/<pid>/stat`. The same scan
also reveals tasks stuck in uninterruptible sleep (D state) and zombie build-up, which explain
high load that isn't CPU.

## What Changes

- New pure parser for `/proc/<pid>/stat` that splits the comm field at the last `)`, so process
  names with spaces and parentheses parse correctly.
- New `processes` check (title "Top processes", equivalent `pidstat 1`) that scans `/proc` on
  every sample and computes per-process %usr, %sys and %cpu over the sampling window.
- Summary with the process count and the top process; details with the top 5 busy processes.
- Findings: one process using more than 90% of total CPU capacity (WARN), more tasks in D state
  than CPUs (WARN) or any D-state task (note), more than 50 zombies (WARN).
- Metrics `processes`, `d_state`, `zombies`, `top_cpu_pct`.
- Registered after the load check.

## Capabilities

### New Capabilities
- `process-check`: per-process CPU usage and process-state triage equivalent to `pidstat 1`.

### Modified Capabilities

## Impact

New files `src/procfs/pid_stat.rs` and `src/checks/processes.rs`, one line each in
`src/procfs/mod.rs` and `src/checks/mod.rs`. Uses `libc::sysconf(_SC_CLK_TCK)` on Linux (libc is
already a dependency). No new dependencies.
