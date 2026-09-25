# Proposal

## Why

The header shows the architecture perf60 was compiled for, not the machine's. That is wrong
in two cases: the x86_64 build running under emulation on an aarch64 kernel (seen in the
cross-architecture verify runs), and the x86_64 legacy fixture rendered by an aarch64 test host.

## What Changes

- The architecture comes from the kernel, via `/proc/sys/kernel/arch`. The fallback is the
  architecture suffix of the kernel release (e.g. `3.10.0-1160.el7.x86_64`), and the last
  resort is the build architecture.
- When the binary's architecture differs from the kernel's, the header says so (e.g.
  `aarch64 (binary x86_64, emulated)`).

## Capabilities

### New Capabilities

### Modified Capabilities
- `system-info`: new requirement for how the architecture is determined.

## Impact

`src/sysinfo.rs`, `src/procfs/system.rs`, `src/report/text.rs`, fixture file
`tests/fixtures/linux-arm64/proc/sys/kernel/arch`.
