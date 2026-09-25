# disk-check Specification

## Purpose
Equivalent of `iostat -xz 1`: per whole block device IOPS, throughput, average latency (await),
queue size and utilization over the sampling window, flagging saturated or slow disks.

## Requirements

### Requirement: Disk statistics parsing
The disk check SHALL read `/proc/diskstats` and accept lines with 14 fields (major, minor, name
and 11 counters), 18 fields (plus discard counters) and 20 fields (plus flush counters). Lines
with fewer than 14 fields SHALL be ignored.

#### Scenario: Kernel with flush counters
- **WHEN** a diskstats line has 20 fields such as
  `253 0 vda 7085 991 848430 4806 36482 2447 876051 18802 0 6085 25279 0 0 0 0 2248 1670`
- **THEN** it parses as device `vda` with 7085 reads, 848430 sectors read, 4806 ms reading,
  36482 writes, 876051 sectors written, 18802 ms writing, 6085 ms doing I/O and 25279 weighted ms

#### Scenario: Old kernel without discard counters
- **WHEN** a diskstats line has 14 fields
- **THEN** it parses with the same 11 counters

### Requirement: Whole-device selection
The disk check SHALL report whole devices only. When `/sys/block` can be listed, the devices
SHALL be the `/proc/diskstats` entries that also appear in `/sys/block`. When `/sys/block`
cannot be listed, names matching partition patterns (`sdXN`, `vdXN`, `xvdXN`, `hdXN`,
`nvmeXnYpZ`, `mmcblkXpY`) SHALL be excluded. In both cases names starting with `loop`, `ram`,
`zram`, `fd` or `sr` SHALL be excluded; `dm-*` and `md*` devices SHALL be kept.

#### Scenario: Partitions are filtered via /sys/block
- **WHEN** `/proc/diskstats` lists `vda` and `vda1`, both with I/O, and `/sys/block` lists only `vda`
- **THEN** only `vda` is reported

#### Scenario: Missing /sys/block uses the name heuristic
- **WHEN** `/sys/block` cannot be listed and `/proc/diskstats` lists `sda`, `sda1`, `nvme0n1`,
  `nvme0n1p1`, `mmcblk0`, `mmcblk0p1`, `dm-0`, `md0` and `loop0`, all with I/O
- **THEN** exactly `sda`, `nvme0n1`, `mmcblk0`, `dm-0` and `md0` are reported

### Requirement: Per-device statistics
Over the whole window (first to last sample, Δt seconds) the disk check SHALL compute per
device: r/s = Δreads/Δt, w/s = Δwrites/Δt, rkB/s and wkB/s from 512-byte sectors,
r_await = Δms_reading/Δreads, w_await = Δms_writing/Δwrites, await =
(Δms_reading+Δms_writing)/(Δreads+Δwrites) (each 0 when there was no such I/O),
aqu-sz = Δweighted_ms/(Δt×1000) and %util = Δms_doing_io/(Δt×1000)×100 capped at 100. It
SHALL also track the highest %util of any single sampling interval (peak). Devices with no
reads, no writes and no I/O time in the window SHALL be omitted, like `iostat -z`. Metrics
SHALL be `<dev>.util_pct`, `<dev>.await_ms`, `<dev>.r_per_sec`, `<dev>.w_per_sec`,
`<dev>.aqu_sz` for each reported device and `max_util_pct` overall. The summary SHALL describe
the device with the highest %util.

#### Scenario: Busy device summary
- **WHEN** over 1 s `vda` completes 10 reads (10 ms) and 900 writes (16370 ms), with 930 ms
  doing I/O and 3100 weighted ms
- **THEN** the summary is `vda util 93% (peak 93%) r/s 10 w/s 900 await 18.0ms aqu 3.1`, and
  the metrics are `vda.util_pct` 93, `vda.r_per_sec` 10, `vda.w_per_sec` 900, `vda.await_ms` 18
  and `vda.aqu_sz` 3.1

#### Scenario: Peak interval utilization
- **WHEN** `vda` spends 200 ms doing I/O in the first second and 900 ms in the second
- **THEN** `vda.util_pct` is 55 and the summary shows `(peak 90%)`

#### Scenario: Idle device omitted
- **WHEN** `vda` counters are unchanged and `vdb` has I/O
- **THEN** only `vdb` is reported and there is no `vda.util_pct` metric

#### Scenario: All disks idle
- **WHEN** no whole device has I/O in the window and there is 1 whole device
- **THEN** the summary is `all disks idle (1 device)`, `max_util_pct` is 0 and the status is OK

### Requirement: Utilization thresholds
The disk check SHALL report WARN when a device's window %util exceeds 60 and CRIT
("saturated") when it exceeds 90. When a WARN or CRIT %util finding is for a device that is
non-rotational or of unknown type, it SHALL add a note that %util can be misleading for RAID,
NVMe and virtual disks that serve requests in parallel.

#### Scenario: At 60 percent
- **WHEN** a device's %util is exactly 60.0
- **THEN** the status is OK

#### Scenario: Above 60 percent
- **WHEN** a device's %util is 60.1
- **THEN** the status is WARN

#### Scenario: Above 90 percent
- **WHEN** a device's %util is 90.1
- **THEN** the status is CRIT and the finding says `saturated`

#### Scenario: Misleading-util note on non-rotational devices
- **WHEN** a device with `queue/rotational` 0 has %util 60.1
- **THEN** the section includes a note that %util can be misleading, and a rotational device at
  the same %util gets no such note

### Requirement: Latency thresholds
The disk check SHALL compare each device's combined await against thresholds that depend on
`/sys/block/<dev>/queue/rotational`. For non-rotational or unknown devices it SHALL report WARN
above 10 ms and CRIT above 50 ms. For rotational devices (`1`) it SHALL report WARN above
50 ms and CRIT above 200 ms.

#### Scenario: SSD at 10 ms
- **WHEN** a non-rotational device has await 10.0 ms
- **THEN** the status is OK

#### Scenario: SSD above 10 ms
- **WHEN** a non-rotational device has await 10.01 ms
- **THEN** the status is WARN

#### Scenario: SSD above 50 ms
- **WHEN** a non-rotational device has await 50.01 ms
- **THEN** the status is CRIT

#### Scenario: Unknown device type uses SSD thresholds
- **WHEN** a device without a `queue/rotational` file has await 20 ms
- **THEN** the status is WARN

#### Scenario: HDD at 20 ms
- **WHEN** a rotational device has await 20 ms
- **THEN** the status is OK

#### Scenario: HDD above 50 ms
- **WHEN** a rotational device has await 50.1 ms
- **THEN** the status is WARN

#### Scenario: HDD above 200 ms
- **WHEN** a rotational device has await 200.1 ms
- **THEN** the status is CRIT

### Requirement: Queue note
When a device's aqu-sz exceeds 1, the disk check SHALL add the note "requests queueing (can be
normal for devices that serve I/O in parallel)". The note SHALL NOT change the status.

#### Scenario: Queue above 1
- **WHEN** a device has aqu-sz 1.5 with %util and await below their thresholds
- **THEN** the section includes the queueing note and the status is OK

#### Scenario: Queue at 1
- **WHEN** a device has aqu-sz 1.0
- **THEN** there is no queueing note

### Requirement: Disk skip
When `/proc/diskstats` cannot be read, the disk check SHALL be SKIPPED.

#### Scenario: Missing diskstats
- **WHEN** `/proc/diskstats` does not exist
- **THEN** the disk section is SKIPPED and the reason names `/proc/diskstats`
