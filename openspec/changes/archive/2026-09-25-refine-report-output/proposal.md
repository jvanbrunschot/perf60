# Proposal

## Why

Running all eleven checks together in real containers exposed rough edges. The report is not the
"simple report" we want: OK sections print up to seven detail lines each. Some checks also give
misleading results:
- The run-queue check warns on an idle machine, because `procs_running` is a noisy instantaneous
  count.
- The processes check is SKIPPED when perf60 is the only process (PID 1 in a container).
- The kernel-log check gives no hint when `/dev/kmsg` is absent (unprivileged containers).
- The id column is too narrow for `cpu-balance`.

## What Changes

- Text report: detail lines are shown only for sections that are not OK. The new
  `-v/--verbose` flag shows them for every section. Findings (including notes) are always shown.
  The id column width fits the longest section id. JSON output is unchanged and always complete.
- cpu check: the run-queue threshold only applies when the CPUs are actually busy, meaning at
  least half of the effective CPU capacity was in use during the window.
- processes check: when perf60 is the only process it can see, the section is OK with summary
  "no other processes visible".
- kernel-log check: when `/dev/kmsg` does not exist, the SKIPPED reason also says how to get
  access (run as root on the host, or `--privileged` in a container).

## Capabilities

### New Capabilities

### Modified Capabilities
- `core-report`: new `--verbose` flag; text report hides details of OK sections; dynamic id column.
- `cpu-check`: run-queue saturation additionally requires CPU usage ≥ 50% of effective capacity.
- `process-check`: skip rule no longer applies when perf60 is the only visible process.
- `kernel-log-check`: skip reason for a missing `/dev/kmsg` includes an access hint.

## Impact

`src/cli.rs`, `src/main.rs`, `src/report/text.rs`, `src/checks/cpu.rs`, `src/checks/processes.rs`,
`src/checks/kernel_log.rs`. No new dependencies. The JSON schema is unchanged.
