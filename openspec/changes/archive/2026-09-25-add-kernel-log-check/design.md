# Design

## Context

`/dev/kmsg` is the structured kernel log. Reading it requires `CAP_SYSLOG` when
`kernel.dmesg_restrict=1`, and container runtimes often don't expose the device at all. The
older `syslog(2)` interface (`klogctl`) reads the same ring buffer in a different text format.

## Decisions

### One record format
The live source converts `klogctl` output (`<prio>[ secs.usecs] msg`) into the `/dev/kmsg`
record format (`prio,seq,usec,-;msg`, with `seq` the line index), so the check has one parser.
The `<prio>` prefix of `SYSLOG_ACTION_READ_ALL` carries the same facility/level value as kmsg.
The converter is a pure function in `src/procfs/kmsg.rs`; only the syscall lives in
`src/source.rs` behind `#[cfg(target_os = "linux")]`. When both interfaces fail, the original
`/dev/kmsg` error is returned so the SKIPPED reason names `/dev/kmsg`.

### Record age
A record's age is `uptime - usec / 1e6`, using `/proc/uptime` read in the same sample. If
`/proc/uptime` is unavailable the newest record's timestamp is the reference. kmsg timestamps
don't advance during suspend, so after a suspend ages are overestimated; this only makes events
look older, never newer.

### Matching
Patterns are matched case-insensitively and must start at a word boundary (the preceding
character is not alphanumeric), so `bug:` does not match `debug:` and `oops` does not match
`loops`. Storage resets and timeouts only count when the message also mentions `nvme` or
`scsi`, so unrelated words such as `systemd-factory-reset.socket` don't match. Classification
uses the message text only, not the log level: several relevant events (segfault, SYN
flooding) are logged at `info`.

### Read once
The kernel log is not a rate, so the check reads it on the first sample only.
