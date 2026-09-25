# biolatency-probe Specification

## Purpose
The eBPF equivalent of BCC `biolatency -D`: the distribution of block I/O latency per disk,
from issue to the device to completion, over the sampling window. It shows the outliers that
the average await of `iostat` hides.

## Requirements

### Requirement: Per-disk latency histogram
The biolatency probe SHALL record the time of each `block_rq_issue` per request and, at the
matching `block_rq_complete`, add the elapsed time in microseconds to a log2 histogram keyed by
the disk name (`gendisk.disk_name`). Requests issued before the probe attached, and requests
whose disk can't be resolved, SHALL NOT be counted. Percentiles SHALL be reported as the upper
bound of their log2 bucket. The section SHALL expose metrics `<disk>.p50_us`,
`<disk>.p99_us`, `<disk>.max_us` and `<disk>.ios` for every disk with I/O, and `max_p99_us`
(0 when there was no I/O).

#### Scenario: Worst disk summary
- **WHEN** during the window `vda` completes 5000 I/Os in the 256–511 µs bucket, 300 in the 4096–8191 µs bucket and 21 in the 16384–32767 µs bucket, and `vdb` completes 100 I/Os in the 64–127 µs bucket
- **THEN** the summary is `vda p50 512µs p99 8.2ms max 32.8ms (5321 I/Os), 2 disks`, `vda.p99_us` is 8192, `max_p99_us` is 8192 and the status is OK

#### Scenario: Worst disk is chosen by p99
- **WHEN** `sda` has p99 in the 1024–2047 µs bucket with 10000 I/Os and `sdb` has p99 in the 4096–8191 µs bucket with 50 I/Os
- **THEN** the summary starts with `sdb`

#### Scenario: No block I/O
- **WHEN** no block I/O completes during the window
- **THEN** the summary is `no block I/O`, the status is OK and `max_p99_us` is 0

### Requirement: Latency outlier thresholds
The biolatency section SHALL compare each disk's p99 (bucket upper bound, in µs) with limits
that depend on `/sys/block/<disk>/queue/rotational`. Non-rotational disks, and disks whose type
is unknown, SHALL be WARN when p99 exceeds 20000 µs and CRIT when it exceeds 100000 µs.
Rotational disks SHALL be WARN when p99 exceeds 100000 µs and CRIT when it exceeds 500000 µs.
The finding SHALL start with `I/O latency outliers on <disk>, p99 <value>` and name the limit
and the disk type.

#### Scenario: SSD below the warning limit
- **WHEN** a non-rotational disk has p99 in the 8192–16383 µs bucket (p99 16384 µs)
- **THEN** the status is OK

#### Scenario: SSD above the warning limit
- **WHEN** a non-rotational disk has p99 in the 16384–32767 µs bucket (p99 32768 µs > 20000 µs)
- **THEN** the status is WARN and the finding starts with `I/O latency outliers on <disk>, p99 32.8ms`

#### Scenario: SSD above the critical limit
- **WHEN** a non-rotational disk has p99 in the 65536–131071 µs bucket (p99 131072 µs > 100000 µs)
- **THEN** the status is CRIT

#### Scenario: Unknown disk type
- **WHEN** a disk has no readable `queue/rotational` and p99 32768 µs
- **THEN** the status is WARN, as for a non-rotational disk

#### Scenario: HDD below the warning limit
- **WHEN** a rotational disk has p99 65536 µs
- **THEN** the status is OK

#### Scenario: HDD above the warning limit
- **WHEN** a rotational disk has p99 131072 µs (> 100000 µs) or 262144 µs
- **THEN** the status is WARN

#### Scenario: HDD above the critical limit
- **WHEN** a rotational disk has p99 524288 µs (> 500000 µs)
- **THEN** the status is CRIT

### Requirement: Details
The biolatency section SHALL list one detail line per disk, worst p99 first, with its p50, p99,
max, I/O count, I/O rate over the window and disk type, for at most 8 disks, followed by
`<N> more disks` when there are more. Directly after the worst disk's line it SHALL show that
disk's BCC-style distribution (30 characters wide), and no distribution for the other disks.

#### Scenario: Distribution for the worst disk only
- **WHEN** `vda` and `vdb` have I/O and `vda` has the higher p99
- **THEN** the first detail line is about `vda`, followed by its distribution lines, and the `vdb` line has no distribution after it

#### Scenario: Many disks
- **WHEN** 10 disks have I/O
- **THEN** 8 disks have a detail line, followed by `2 more disks`

### Requirement: Kernel-dependent attachment
The probe SHALL attach to the raw tracepoints `block_rq_issue` and `block_rq_complete`. The
`struct request *` of `block_rq_issue` SHALL be read from argument 0 when the kernel release
(`/proc/sys/kernel/osrelease`, major.minor) is 5.11 or newer, or unparseable, and from argument
1 before 5.11 (whose `TP_PROTO` starts with `struct request_queue *`). `block_rq_complete` SHALL
use argument 0. The disk SHALL be found through the kernel's BTF: `request.q` then
`request_queue.disk` when both members exist, otherwise `request.rq_disk`; the name at
`gendisk.disk_name`.

#### Scenario: Kernel before 5.11
- **WHEN** the kernel release is `5.10.0-28-amd64` or `4.19.0`
- **THEN** the request is argument 1 of `block_rq_issue`

#### Scenario: Kernel 5.11 or newer
- **WHEN** the kernel release is `5.11.0`, `6.10.14-linuxkit` or unparseable
- **THEN** the request is argument 0 of `block_rq_issue`

#### Scenario: Current kernel BTF
- **WHEN** the BTF has `request.q`, `request_queue.disk` and `gendisk.disk_name` (and possibly `request.rq_disk`)
- **THEN** the probe follows `request.q` → `request_queue.disk`

#### Scenario: Older kernel BTF
- **WHEN** the BTF has no `request_queue.disk` but has `request.rq_disk` and `gendisk.disk_name`
- **THEN** the probe follows `request.rq_disk`

#### Scenario: No usable disk path
- **WHEN** the BTF has neither path, or no `gendisk.disk_name`
- **THEN** the section is SKIPPED with a reason naming the missing members

### Requirement: Degraded biolatency
The biolatency section SHALL be SKIPPED, with the deep-mode reasons, when privileges, kernel
BTF or eBPF loading are missing. The needs-root check SHALL run before the BTF lookup.

#### Scenario: Unprivileged container
- **WHEN** perf60 runs with `--deep` in a container with default capabilities
- **THEN** the biolatency section is SKIPPED with the needs-root reason

#### Scenario: No kernel BTF
- **WHEN** `/sys/kernel/btf/vmlinux` is missing and perf60 has the privileges
- **THEN** the biolatency section is SKIPPED with a reason naming `/sys/kernel/btf/vmlinux` and `CONFIG_DEBUG_INFO_BTF`
