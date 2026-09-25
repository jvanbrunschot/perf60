# Proposal

## Why

The first combined Phase A runs showed two cases where one problem gives two findings. That
inflates the WARN count and will skew the upcoming diagnosis:
- **cgroups-top repeats the cgroup section.** In a container, `cgroups-top` sees only the
  container's own cgroup and warns about the same CPU-quota throttling the `cgroup` section
  already reports.
- **One UDP overflow counts twice.** The kernel bumps both `RcvbufErrors` and `InErrors`, so it
  produces two UDP WARNs.

## What Changes

- `cgroups-top` does not raise its throttling finding for the own cgroup, which the `cgroup`
  section covers. The own cgroup is still listed in the top tables.
- The UDP `InErrors` finding only fires for input errors not explained by receive-buffer
  overflows: ΔInErrors > ΔRcvbufErrors. The metrics stay raw.

## Capabilities

### New Capabilities

### Modified Capabilities
- `cgroups-top-check`: "Throttled cgroups" excludes the own cgroup.
- `tcp-check`: "UDP buffer errors" no longer double-reports receive-buffer overflows.

## Impact

`src/checks/cgroup.rs`, `src/checks/net.rs`.
