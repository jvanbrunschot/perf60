## Purpose
Equivalent of `edac-util`, `cpupower frequency-info` and `chronyc tracking`: hardware and
platform state that silently degrades performance and that the 60-second checklist never looks
at: ECC memory errors, CPU thermal throttling, CPU frequency and governor, kernel taint and
clock synchronization.

## ADDED Requirements

### Requirement: Platform summary, details and metrics
The hardware check SHALL read five sources at every sample:
- EDAC memory controllers `/sys/devices/system/edac/mc/mc*/ce_count` and `ue_count`
- thermal throttle counters
  `/sys/devices/system/cpu/cpu*/thermal_throttle/core_throttle_count` and
  `package_throttle_count`
- cpufreq `/sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq`, `cpuinfo_max_freq` and
  `scaling_governor`
- the kernel taint mask `/proc/sys/kernel/tainted`
- the kernel clock state from `adjtimex(2)`

It SHALL compare the first and the last sample for the EDAC and thermal counters and judge
everything else on the last sample. The summary SHALL join, with `, `, one part per available
source in this order:
- memory: `no memory errors`, `<n> corrected memory errors`, `<n> uncorrectable memory errors`
  or `<n> uncorrectable and <m> corrected memory errors` (singular `error` for 1)
- thermal: `no thermal throttling` or `<n> thermal throttle events`
- cpufreq: `governor <name>` (distinct governors joined with `/`), or `cpufreq <pct>% of max`
  when no governor is readable
- taint: `kernel not tainted` or `kernel tainted <letters>`
- clock: `clock synced` or `clock not synced`

The check SHALL expose the metrics `edac_ce` and `edac_ue` (totals over all controllers),
`thermal_throttle_events` (core plus package events), `cpufreq_pct_of_max`, `tainted` (the raw
mask), `clock_synced` (1 or 0) and `clock_maxerror_ms`, each only when its source is available.

#### Scenario: Summary on a legacy host
- **WHEN** mc0 has ce_count 3 and ue_count 0, two CPUs run the `powersave` governor at 1200000 of 3300000 kHz, there are no thermal throttle counters, tainted is 0 and the clock is synchronized with offset 1200 µs and maxerror 45000 µs
- **THEN** the summary is `3 corrected memory errors, governor powersave, kernel not tainted, clock synced`, the status is OK, and the details include `cpufreq: governor powersave, 1.2/3.3 GHz (36% of max)` and `clock synchronized, offset 1200 µs, maxerror 45 ms`

#### Scenario: Container without hardware counters
- **WHEN** only `/proc/sys/kernel/tainted` (0) and the clock state are available
- **THEN** the summary is `kernel not tainted, clock synced` and the check is not SKIPPED

### Requirement: Uncorrectable memory errors
The check SHALL report CRIT "uncorrectable memory errors: replace DIMM" naming the controllers
when the last sample has a ue_count above 0 on any memory controller.

#### Scenario: No uncorrectable errors
- **WHEN** mc0 has ue_count 0 in every sample
- **THEN** the status is OK and `edac_ue` is 0

#### Scenario: One uncorrectable error
- **WHEN** mc0 has ue_count 1
- **THEN** the status is CRIT and the finding says `replace DIMM` and names mc0

### Requirement: Corrected memory errors
The check SHALL report WARN "corrected memory errors during the window: DIMM degrading" when
the ce_count of any memory controller rises between the first and the last sample. When a
controller's ce_count in the last sample is above 0, it SHALL add a note
`<n> corrected memory errors since boot on <mc>`. Each controller SHALL get a detail line
`<mc>: <ce> corrected, <ue> uncorrectable since boot`. When there is no `mc*` directory, the
details SHALL say `no EDAC memory controllers (VM or driver not loaded)`.

#### Scenario: Corrected errors rise during the window
- **WHEN** mc0 has ce_count 3 in the first sample and 4 in the last
- **THEN** the status is WARN and the finding names mc0 and `+1`

#### Scenario: Corrected errors unchanged during the window
- **WHEN** mc0 has ce_count 3 in every sample and ue_count 0
- **THEN** the status is OK and there is a note `3 corrected memory errors since boot on mc0`

#### Scenario: No EDAC controllers
- **WHEN** `/sys/devices/system/edac/mc` has no `mc*` directories and the taint mask is readable
- **THEN** the details include `no EDAC memory controllers (VM or driver not loaded)`, there is no `edac_ce` metric and the check is not SKIPPED

### Requirement: Thermal throttling
The check SHALL sum the per-CPU `core_throttle_count` values and count each package's
`package_throttle_count` once (CPUs of one package repeat the same value; the package is taken
from `topology/physical_package_id`, all CPUs count as one package when that is missing). It
SHALL report WARN "CPU thermal throttling during the window" naming the CPUs when any CPU's core
or package count rises between the first and the last sample. When the total is above 0 a
detail SHALL say `thermal throttling since boot: <n> core, <m> package events`, otherwise
`no thermal throttling since boot`. Without counters the details SHALL say
`no thermal throttle counters (VM or not x86)`.

#### Scenario: Throttle count rises
- **WHEN** cpu1 has core_throttle_count 10 in the first sample and 12 in the last
- **THEN** the status is WARN and the finding names cpu1

#### Scenario: Throttle count unchanged
- **WHEN** cpu0 has core_throttle_count 10 and package_throttle_count 4 in every sample
- **THEN** the status is OK, `thermal_throttle_events` is 14 and a detail says `thermal throttling since boot: 10 core, 4 package events`

### Requirement: CPU frequency and governor
The check SHALL add a detail `cpufreq: governor <name>, <cur>/<max> GHz (<pct>% of max)` where
cur and max are the averages over the CPUs that report both, in GHz with one decimal, and pct
is their ratio rounded to a whole number, exposed as `cpufreq_pct_of_max`. When the governor of
any CPU is `powersave` and more than 2 CPUs report cpufreq, it SHALL add the note "powersave
governor: expect higher latency; consider performance/schedutil". Without cpufreq the details
SHALL say `no cpufreq (VM or fixed clock)`.

#### Scenario: Powersave on 2 CPUs
- **WHEN** two CPUs report cpufreq with governor `powersave`
- **THEN** there is no powersave note

#### Scenario: Powersave on 4 CPUs
- **WHEN** four CPUs report cpufreq with governor `powersave`
- **THEN** there is a note starting with `powersave governor` and the status is OK

#### Scenario: No cpufreq
- **WHEN** no CPU has a `cpufreq` directory and the taint mask is readable
- **THEN** the details include `no cpufreq (VM or fixed clock)` and there is no `cpufreq_pct_of_max` metric

### Requirement: Kernel taint
The check SHALL decode `/proc/sys/kernel/tainted` into the kernel's taint letters: P(0)
proprietary module, F(1) module force-loaded, S(2) unsafe SMP, R(3) module force-unloaded, M(4)
machine check, B(5) bad page, U(6) user taint, D(7) kernel died (oops), A(8) ACPI table
overridden, W(9) kernel warning, C(10) staging driver, I(11) firmware workaround, O(12)
out-of-tree module, E(13) unsigned module, L(14) soft lockup, K(15) live patched, X(16)
auxiliary taint, T(17) randomized struct layout, N(18) test module. Unknown bits SHALL show as
`?`. It SHALL report:
- CRIT "machine check / bad page: hardware fault" when M or B is set
- WARN "kernel oops / soft lockup since boot: check kernel-log" when D or L is set
- a note `kernel tainted: <letter> <meaning>, …` listing every other set flag

The details SHALL say `kernel not tainted` for 0, and otherwise
`kernel tainted <mask> (<letters>): <letter> <meaning>, …` for all set flags.

#### Scenario: Not tainted
- **WHEN** tainted is 0
- **THEN** the details include `kernel not tainted`, there is no taint finding and `tainted` is 0

#### Scenario: Proprietary module
- **WHEN** tainted is 1
- **THEN** the status is OK and there is a note `kernel tainted: P proprietary module`

#### Scenario: Kernel oops
- **WHEN** tainted is 128
- **THEN** the status is WARN and the finding says `check kernel-log`

#### Scenario: Machine check
- **WHEN** tainted is 16
- **THEN** the status is CRIT and the finding says `hardware fault`

#### Scenario: Kernel warning and out-of-tree module
- **WHEN** tainted is 4608
- **THEN** the status is OK, the summary contains `kernel tainted WO` and there is a note `kernel tainted: W kernel warning, O out-of-tree module`

### Requirement: Clock synchronization
The check SHALL report WARN "system clock not synchronized: no NTP/chrony discipline;
timestamps and TLS may drift" when adjtimex returns `TIME_ERROR` or has `STA_UNSYNC` set. When
the clock is synchronized and its maxerror is above 1000000 µs (1 s), it SHALL add a note that
the clock max error is above 1 s; an unsynchronized clock gets only the WARN, since its maxerror
grows without bound by design. The details SHALL say
`clock synchronized, offset <x> µs, maxerror <y> ms` (or `clock not synchronized, …`), with y
in ms with at most one decimal. When adjtimex is unavailable the details SHALL say
`clock status unavailable`, with no clock finding or metric.

#### Scenario: Clock not synchronized
- **WHEN** adjtimex returns state 5 (TIME_ERROR) with STA_UNSYNC and maxerror 16000000 µs
- **THEN** the status is WARN, `clock_synced` is 0, the summary contains `clock not synced` and there is no maxerror note

#### Scenario: Max error exactly 1 s
- **WHEN** the clock is synchronized and maxerror is 1000000 µs
- **THEN** there is no clock finding and `clock_maxerror_ms` is 1000

#### Scenario: Max error above 1 s
- **WHEN** the clock is synchronized and maxerror is 1000001 µs
- **THEN** the status is OK and there is a note about the clock max error above 1 s

#### Scenario: Clock status unavailable
- **WHEN** adjtimex fails and the taint mask is readable
- **THEN** the details include `clock status unavailable` and there is no `clock_synced` metric

### Requirement: SKIPPED only without any source
The check SHALL be SKIPPED only when none of the five sources is available, with the summary
`no EDAC, thermal throttle, cpufreq, taint or clock data`. A missing source SHALL only drop its
summary part, metrics and findings.

#### Scenario: All sources absent
- **WHEN** there are no EDAC controllers, no thermal throttle counters, no cpufreq, `/proc/sys/kernel/tainted` is missing and adjtimex fails
- **THEN** the status is SKIPPED with the summary `no EDAC, thermal throttle, cpufreq, taint or clock data`
