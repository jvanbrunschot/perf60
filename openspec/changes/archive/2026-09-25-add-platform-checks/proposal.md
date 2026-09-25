# Proposal

## Why

Hardware and platform faults degrade performance without showing up in the 2015 checklist. A
failing DIMM logs corrected ECC errors before it logs uncorrectable ones. A CPU that throttles
on heat, or runs under the `powersave` governor, is simply slower. A tainted kernel (oops, soft
lockup, machine check) is a known-bad state. A clock that no NTP daemon disciplines skews
timestamps, TLS and distributed timeouts. `edac-util`, `cpupower` and `chronyc tracking` are the
usual first look, and none of them is installed on a minimal server.

## What Changes

- New `hardware` check ("Hardware and platform", equivalent
  `edac-util / cpupower / chronyc tracking`, resource hardware) with five sources:
  - EDAC memory controllers (`/sys/devices/system/edac/mc/mc*/{ce_count,ue_count}`), first and
    last sample: any uncorrectable error is CRIT, corrected errors during the window are WARN,
    corrected errors since boot are a note
  - x86 thermal throttle counters
    (`/sys/devices/system/cpu/cpu*/thermal_throttle/{core,package}_throttle_count`): an increase
    during the window is WARN
  - cpufreq (`scaling_cur_freq`, `cpuinfo_max_freq`, `scaling_governor`): a detail line with the
    governor and the average frequency against the maximum, and a note for `powersave` on more
    than 2 CPUs
  - kernel taint (`/proc/sys/kernel/tainted`), decoded to the kernel's letters: M or B is CRIT,
    D or L is WARN, any other flag is a note
  - clock discipline (`adjtimex(2)` through `Source::clock_status`): not synchronized is WARN,
    a max error above 1 s is a note
- A missing source only drops its part. The check is SKIPPED only when all five are missing.
- New pure parser module `src/procfs/platform.rs` (sysfs counters, governor, cpu/mc directory
  names, taint decoding).

## Capabilities

### New Capabilities
- `hardware-check`: EDAC memory errors, thermal throttling, CPU frequency and governor, kernel
  taint and clock synchronization.

### Modified Capabilities

## Impact

- New files `src/checks/hardware.rs` and `src/procfs/platform.rs`. One registry line in
  `src/checks/mod.rs` (last, after `capacity::Limits`) and one `pub mod` line in
  `src/procfs/mod.rs`.
- The report gets one more section. It renders on both fixture trees: linux-legacy has EDAC,
  cpufreq, taint and clock; linux-arm64 has taint and clock.
- No new dependencies.
