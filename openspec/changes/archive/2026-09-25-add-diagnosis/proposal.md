# Proposal

## Why

One root cause often surfaces in several sections. A CPU quota shows up in `cpu` (run queue),
`pressure` (cpu stall) and `cgroup` (throttling). A saturated disk shows up in `cpu` (iowait),
`disk` (%util) and `pressure` (io stall). A reader has to connect these by hand, which is the
expertise the 60-second checklist assumes. The report should name the likely bottleneck
directly.

## What Changes

- A finding can name the resource it is about when that differs from its section:
  - PSI io pressure → disk, and cpu iowait → disk
  - a cgroup's memory events → memory
  - kernel-log OOM kills → memory; I/O, filesystem and storage errors → disk; machine checks →
    hardware; SYN floods, conntrack and link-down → network
- A diagnosis groups every WARN/CRIT finding by resource, ranks the resources and names the top
  one with up to 3 pieces of evidence from different sections, plus other affected resources.
- The text report shows `Likely bottleneck: …` under OVERALL. The JSON report gets a
  `diagnosis` object, or `null` when nothing is wrong.
- Findings in JSON carry `resource` when set.
- Fix: the OVERALL count says "2 warnings", not "2 warning".

## Capabilities

### New Capabilities

### Modified Capabilities
- `core-report`: new "Diagnosis" requirement; the text and JSON report requirements include it.

## Impact

`src/check.rs` (`Finding.resource`, `*_on` helpers), `src/report/{diagnosis,mod,text,json}.rs`,
finding sites in `pressure`, `cpu`, `cgroup` and `kernel_log`, README, CLAUDE.md.
