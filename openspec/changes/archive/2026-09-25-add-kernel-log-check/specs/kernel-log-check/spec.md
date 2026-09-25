# Spec Delta

## Purpose

Equivalent of `dmesg | tail`: reads the kernel log and flags recent events that explain
performance problems (OOM kills, hung tasks, I/O and filesystem errors, lockups, hardware
errors, SYN floods, segfaults, link and storage resets).

## ADDED Requirements

### Requirement: Kernel log source
The kernel-log check SHALL read the kernel log once per run, in the `/dev/kmsg` record format
`prio,seq,usec,flags;message`. The log level of a record SHALL be `prio & 7` (0 emerg to
7 debug). Continuation lines (lines starting with a space, e.g. ` SUBSYSTEM=...`) SHALL be
ignored. The age of a record SHALL be the uptime from `/proc/uptime` minus `usec / 1e6`
seconds; when `/proc/uptime` is unavailable the newest record's timestamp SHALL be used as the
reference.

#### Scenario: Continuation lines ignored
- **WHEN** the log holds 2 records, one followed by the lines ` SUBSYSTEM=pci` and
  ` DEVICE=+pci:0000:00:01.0`
- **THEN** the `records` metric is 2

#### Scenario: Real fixture parses
- **WHEN** the check reads the captured `tests/fixtures/linux-arm64/dev/kmsg`
- **THEN** every non-continuation line parses as a record and the `records` metric is greater
  than 0

### Requirement: Live klogctl fallback
On Linux, when `/dev/kmsg` cannot be opened, the live source SHALL read the log with the
`syslog(2)` syscall (`SYSLOG_ACTION_READ_ALL`, buffer sized by `SYSLOG_ACTION_SIZE_BUFFER`)
and convert each `<prio>[ seconds.micros] message` line into the `/dev/kmsg` record format.
When both fail, the source SHALL return the original `/dev/kmsg` error.

#### Scenario: Syslog line conversion
- **WHEN** the syslog buffer holds `<6>[   12.345678] eth0: link is down`
- **THEN** it converts to a record with prio 6, usec 12345678 and message `eth0: link is down`

#### Scenario: Line without timestamp
- **WHEN** the syslog buffer holds `<3>disk failure` (printk timestamps disabled)
- **THEN** it converts to a record with prio 3, usec 0 and message `disk failure`

### Requirement: Event classification
The kernel-log check SHALL classify each message by case-insensitive patterns that start at a
word boundary.
CRIT patterns: `out of memory`, `oom-kill`, `killed process` (OOM kill); `blocked for more
than`, `hung_task` (hung task); `i/o error` (I/O error, including `buffer i/o error`);
`ext4-fs error`, `xfs` together with `error`, `btrfs` together with `error` (filesystem error);
`kernel panic`; `bug:` and `oops` (kernel BUG/oops); `soft lockup`, `hard lockup` (CPU lockup);
`machine check`, `mce:` (hardware error); `memory failure`.
WARN patterns: `syn flooding` (SYN flood); `segfault`; `nf_conntrack: table full`;
`link is down`; `task abort`, or `nvme`/`scsi` together with `reset` or `timeout` (storage
reset/timeout); `call trace`.
A matching event SHALL escalate the status only when its age is at most 3600 seconds. Matching
events older than 3600 seconds SHALL produce a note per category with the count of matching
messages and the age of the most recent one, and SHALL NOT change the status. Counts are of
log messages, not incidents: one OOM kill typically logs several matching lines.

#### Scenario: Recent OOM kill
- **WHEN** uptime is 10000 s and the log holds `Out of memory: Killed process 4242 (java)`
  at 9867 s (133 s ago)
- **THEN** the status is CRIT, a finding mentions `OOM kill` and `2m13s ago`, and
  `recent_critical` is 1

#### Scenario: Old OOM kill
- **WHEN** uptime is 20000 s and the log holds `Out of memory: Killed process 4242 (java)`
  at 12620 s (7380 s ago)
- **THEN** the status is OK and a note says `OOM kill: 1 message older than 1h, most recent 2h03m ago`

#### Scenario: One-hour boundary
- **WHEN** an OOM kill is exactly 3600 s old
- **THEN** the status is CRIT
- **WHEN** an OOM kill is 3601 s old
- **THEN** the status is OK with a note

#### Scenario: Recent SYN flood
- **WHEN** the log holds `TCP: request_sock_TCP: Possible SYN flooding on port 80. Sending
  cookies.` 60 s ago
- **THEN** the status is WARN and `recent_warning` is 1

#### Scenario: Word boundaries
- **WHEN** the log holds `usb: debug: loops done` and `systemd[1]: Listening on
  systemd-factory-reset.socket`
- **THEN** no event is classified and the status is OK

### Requirement: Kernel log summary
The kernel-log check SHALL summarize as `<N> records, <E> errors (prio ≤ 3), last error <age>
ago` (`1 error` when there is exactly one), or `<N> records, no errors` when no record has
level ≤ 3. Details SHALL list the last 5
records with level ≤ 4 as `[-<age>] <message>`, each message truncated to 120 characters. It
SHALL expose metrics `records`, `errors`, `recent_critical` and `recent_warning`.

#### Scenario: Clean log
- **WHEN** the log holds only level 6 records such as `Booting Linux on physical CPU 0x0`
- **THEN** the status is OK, the summary ends in `no errors`, there are no findings and
  `errors` is 0

#### Scenario: Errors in summary
- **WHEN** uptime is 20000 s and the log holds a level 3 record at 12620 s
- **THEN** the summary contains `1 error (prio ≤ 3), last error 2h03m ago`

#### Scenario: Details show last warnings
- **WHEN** the log holds 7 records of level ≤ 4 and one 300-character warning message
- **THEN** there are 5 detail lines, each starting with `[-` and none longer than the
  truncated message plus its age prefix

### Requirement: Kernel log skip
When the kernel log cannot be read, the kernel-log check SHALL be SKIPPED. On a permission
error the reason SHALL be `permission denied reading /dev/kmsg (run as root or set
kernel.dmesg_restrict=0)`.

#### Scenario: Permission denied
- **WHEN** reading the kernel log fails with a permission error
- **THEN** the section is SKIPPED with reason `permission denied reading /dev/kmsg (run as root
  or set kernel.dmesg_restrict=0)`

#### Scenario: Missing kernel log
- **WHEN** `/dev/kmsg` does not exist
- **THEN** the section is SKIPPED with a reason naming `/dev/kmsg`
