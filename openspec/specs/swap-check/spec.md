# swap-check Specification

## Purpose
Equivalent of the si/so columns of `vmstat 1`: shows whether the system is swapping during the
sampling window, plus major page faults and direct reclaim as signs of memory pressure.

## Requirements

### Requirement: Swap rates
The swap check SHALL compute swap-in (si) and swap-out (so) rates in pages per second from the
`pswpin` and `pswpout` counters in `/proc/vmstat` between the first and last sample, and also
show them as byte rates (e.g. `4 KiB/s`) using the system page size. The summary SHALL be
`si <n> so <n> pages/s` followed by the byte rates; when SwapTotal in `/proc/meminfo` is 0 and nothing was swapped it SHALL be
`no swap configured, no swapping`. It SHALL expose the metrics `si_pages_per_sec`,
`so_pages_per_sec`, `majflt_per_sec` and `pgscan_direct_per_sec`.

#### Scenario: Summary
- **WHEN** swap is configured and pswpin and pswpout do not change over a 1 second window
- **THEN** the summary starts with `si 0 so 0 pages/s` and the status is OK

#### Scenario: No swap configured
- **WHEN** SwapTotal is 0 kB and pswpin and pswpout do not change
- **THEN** the summary is `no swap configured, no swapping` and the status is OK

### Requirement: Swapping thresholds
The swap check SHALL report WARN "system is swapping: memory pressure" when si + so exceeds 0
pages/s, and CRIT when si + so exceeds 256 pages/s (about 1 MiB/s with 4 KiB pages).

#### Scenario: No swapping
- **WHEN** si + so is 0 pages/s
- **THEN** the status is OK

#### Scenario: Warn on any swapping
- **WHEN** pswpin increases by 1 page over a 1 second window
- **THEN** the status is WARN

#### Scenario: Exactly 256 pages/s
- **WHEN** pswpin increases by 128 and pswpout by 128 over a 1 second window
- **THEN** the status is WARN

#### Scenario: Crit above 256 pages/s
- **WHEN** pswpout increases by 257 over a 1 second window
- **THEN** the status is CRIT

### Requirement: Major faults and direct reclaim
The swap check SHALL add an informational note when the `pgmajfault` rate is above 0 during the
window, and when the direct reclaim scan rate is above 0. The direct reclaim counter SHALL be
`pgscan_direct`, or on kernels without it the sum of the per-zone `pgscan_direct_*` counters
(excluding `pgscan_direct_throttle`). Notes SHALL NOT change the status.

#### Scenario: Major faults
- **WHEN** pgmajfault increases by 50 over a 1 second window and nothing is swapped
- **THEN** the section includes a note about 50 major faults/s and the status is OK

#### Scenario: Direct reclaim on per-zone kernels
- **WHEN** pgscan_direct_normal increases by 30 and pgscan_direct_dma32 by 10 over a 1 second window
- **THEN** `pgscan_direct_per_sec` is 40, a direct reclaim note is added and the status is OK

### Requirement: Swap skip
When `/proc/vmstat` cannot be read, the swap check SHALL be SKIPPED.

#### Scenario: Missing vmstat
- **WHEN** `/proc/vmstat` does not exist
- **THEN** the swap section is SKIPPED and the summary names `/proc/vmstat`
