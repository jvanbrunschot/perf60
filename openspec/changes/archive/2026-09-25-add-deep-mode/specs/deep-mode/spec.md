# Spec Delta

## Purpose

Defines perf60's optional eBPF mode: how `--deep` adds kernel-aggregated probe sections, how
they behave without privileges or eBPF support, and the constraints on how they are built.

## ADDED Requirements

### Requirement: Deep option
With `--deep`, the tool SHALL append the eBPF probe sections to the report, after the counter
sections. Without `--deep`, no probe is loaded.

#### Scenario: Deep sections appear
- **WHEN** perf60 runs with `--deep` as root on Linux with a build that includes eBPF support
- **THEN** the report contains an `execsnoop` section that is not SKIPPED

#### Scenario: Default run loads nothing
- **WHEN** perf60 runs without `--deep`
- **THEN** no eBPF program is loaded and no probe section appears

### Requirement: Probe lifecycle
A probe SHALL attach at the first sample, aggregate in kernel maps for the whole sampling
window, be read once after the last sample, and detach when perf60 exits. Rates SHALL use the
measured window length.

#### Scenario: Window aggregation
- **WHEN** perf60 runs with `--deep --interval 1 --count 2` while a loop execs `true` repeatedly
- **THEN** the execsnoop section reports the exec count and rate over the 2-second window

### Requirement: No tracefs dependency
Probes SHALL attach to raw tracepoints by name. Probes that read kernel structs SHALL obtain
member offsets from the running kernel's BTF (`/sys/kernel/btf/vmlinux`). The tool SHALL NOT
mount any filesystem.

#### Scenario: Container without tracefs
- **WHEN** perf60 runs with `--deep` in a `--privileged` container where tracefs is not mounted
- **THEN** the execsnoop section is not SKIPPED

### Requirement: Degraded deep mode
Every probe failure SHALL make only that probe's section SKIPPED, with an actionable reason. The
rest of the report SHALL be unaffected. Without CAP_SYS_ADMIN, or without CAP_BPF together with
CAP_PERFMON, the reason SHALL be `needs root (CAP_BPF and CAP_PERFMON, or CAP_SYS_ADMIN; in a
container use --privileged)`. A build without eBPF support SHALL report a single SKIPPED `deep`
section saying so.

#### Scenario: Unprivileged container
- **WHEN** perf60 runs with `--deep` in a container with default capabilities
- **THEN** the execsnoop section is SKIPPED with the needs-root reason and the exit code reflects only the other sections

#### Scenario: Build without eBPF support
- **WHEN** a binary built without the `deep` feature runs with `--deep`
- **THEN** there is one SKIPPED section `deep` whose reason names the missing `deep` feature

### Requirement: Reproducible, licensed eBPF build
The eBPF programs SHALL be built with a nightly toolchain pinned in one place (`build.rs`) and
a bpf-linker release pinned by version and SHA-256. A download whose checksum doesn't match
SHALL fail the build. The eBPF programs SHALL declare the license `Dual MIT/GPL`. Release
binaries SHALL include the probes.

#### Scenario: Tampered bpf-linker download
- **WHEN** the downloaded bpf-linker archive does not match the pinned SHA-256
- **THEN** `scripts/install-bpf-linker.sh` exits non-zero without installing it
