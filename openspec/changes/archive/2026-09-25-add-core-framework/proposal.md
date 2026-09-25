# Proposal

## Why

Gregg's 60-second checklist needs ten commands from procps and sysstat. Minimal servers and
containers often don't have them. We need one static binary that does the same triage from
kernel interfaces directly. Before any individual check can be built, the tool needs a shared
foundation: CLI, sampling loop, check abstraction, system spec header, report rendering and exit
codes.

## What Changes

- New `perf60` binary with CLI options `--interval`, `--count`, `--json`, `--no-color`,
  `--help` and `--version`.
- A sampling engine that takes `count + 1` snapshots spaced `interval` seconds apart and gives
  each check the measured elapsed time between snapshots.
- A check abstraction with statuses OK / WARN / CRIT / SKIPPED, plus a registry that later
  changes extend with one line each.
- A system spec header: host, kernel, distro, CPU, memory, virtualization, cgroup limits,
  uptime, block devices and NICs.
- Text and JSON reports, and an exit code that reflects the worst status.
- The load-average check (`uptime` equivalent) as the reference check.
- `scripts/verify.sh` to build a static Linux binary and run it in minimal containers.

## Capabilities

### New Capabilities
- `core-report`: CLI, sampling, check lifecycle, SKIPPED semantics, text/JSON output, exit codes.
- `system-info`: the system spec header gathered from /proc, /sys and /etc/os-release.
- `load-check`: load-average triage equivalent to `uptime`.

### Modified Capabilities

## Impact

New crate. Dependencies: `libc`, `serde`, `serde_json`. Linux-only at runtime; it compiles and
unit-tests on macOS.
