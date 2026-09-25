# Proposal

## Why

Step two of Gregg's checklist is `dmesg | tail`: look at the last kernel messages for errors
that explain a performance problem (OOM kills, hung tasks, I/O errors, SYN floods, dropped
packets). Reading the log by eye is slow and easy to get wrong; perf60 should read the kernel
log directly and flag the events that matter, without shelling out to `dmesg`.

## What Changes

- New `kernel-log` check (title "Kernel log", equivalent `dmesg | tail`), registered directly
  after the load check.
- A pure parser for `/dev/kmsg` records (`prio,seq,usec,flags;message`, continuation lines
  ignored) and a pure converter from `klogctl` syslog lines (`<prio>[ secs.usecs] msg`) to that
  record format.
- On Linux, when `/dev/kmsg` cannot be opened, the live source falls back to the `klogctl`
  syscall (`SYSLOG_ACTION_READ_ALL`).
- Pattern-based classification of messages into CRIT (OOM kill, hung task, I/O error,
  filesystem error, panic, BUG/oops, lockup, hardware error, memory failure) and WARN
  (SYN flood, segfault, conntrack table full, link down, storage reset/timeout, call trace).
  Only events from the last hour escalate the status; older ones become notes.
- Summary with record and error counts, details with the last 5 warning-or-worse records, and
  metrics `records`, `errors`, `recent_critical`, `recent_warning`.

## Capabilities

### New Capabilities
- `kernel-log-check`: kernel log triage equivalent to `dmesg | tail`.

### Modified Capabilities

## Impact

New files `src/procfs/kmsg.rs` and `src/checks/kernel_log.rs`; one line each in
`src/procfs/mod.rs` and `src/checks/mod.rs`; a Linux-only `klogctl` fallback in
`src/source.rs`. No new dependencies (`klogctl` comes from `libc`).
