# Changelog

All notable changes to perf60. Versions follow [semantic versioning](https://semver.org/);
every entry maps to one OpenSpec change under `openspec/changes/archive/`.

## 0.2.0 — 2026-09-25

The 2015 checklist is still a sound first minute, but it predates containers, cgroup v2, PSI,
NVMe and Gregg's own eBPF tooling. This release covers what commonly takes modern systems down.

### Added
- **Likely-bottleneck diagnosis.** The report groups warnings by the resource they concern and
  names the top one, with evidence from up to three sections (e.g. `CPU quota throttling
  (cgroup)` or `disk I/O`). The JSON output gets a `diagnosis` object.
- **New sections:**
  - `cgroup`: own-cgroup CPU quota throttling, `memory.events` (OOM kills, `memory.max` hits).
  - `cgroups-top`: systemd-cgtop-style top cgroups by CPU and memory; names throttled
    services and pods.
  - `sockets`: conntrack table fill, TIME_WAIT vs the ephemeral port range, orphans, TCP
    memory.
  - `filesystems`: space and inodes (df-style, bind mounts deduplicated); network and FUSE
    mounts are never stat'ed, since that can hang.
  - `limits`: system fds, tasks vs `pid_max`/`threads-max`, cgroup pids, processes near their
    open-files limit.
  - `hardware`: EDAC memory errors, thermal throttling, cpufreq governor, kernel taint, clock
    sync (`adjtimex`).
- **New signals in existing sections:**
  - `cpu`: run-queue wait from `/proc/schedstat`.
  - `processes`: fork rate (short-lived process storms).
  - `memory`: page-cache thrashing (refaults), compaction and NUMA.
  - `net`: softnet backlog drops, NAPI squeeze, NET_RX on one CPU.
  - `tcp`: UDP buffer errors, TCP backlog drops.
- **`--deep`, eBPF probes** from the BCC checklist: `execsnoop`, `runqlat`, `biolatency`
  (per disk) and `tcpretrans` (by endpoint).
  - They attach to raw tracepoints and read kernel structs via BTF, so they need no tracefs
    and work in `--privileged` containers.
  - They need root, or CAP_BPF + CAP_PERFMON. Without them the probes are SKIPPED with the
    reason.
- `-v/--verbose` shows details for every section. By default, details are only shown for
  problems.
- Each section's `resource` in JSON, and per-finding resources.

### Changed
- The header shows the kernel's architecture and flags emulated binaries.
- Run-queue saturation is only judged when the CPUs are actually busy.
- One finding per problem: no duplicate own-cgroup throttling or UDP overflow warnings.
- Release binaries are about 1.2 MB, up from 0.6 MB (the eBPF loader). Release builds include
  `--deep`.

### Build and security
- CI on every PR:
  - fmt, clippy (also with `--features deep`), tests, spec validation
  - container verification on native x86_64 and arm64 runners, including a privileged `--deep`
    run
- Supply-chain hardening:
  - actions pinned to SHAs
  - npm tooling installed from a lockfile with scripts disabled
  - bpf-linker pinned by version and SHA-256
  - `cargo --locked` everywhere
  - release provenance attestations (`gh attestation verify`)
- Licensing: MIT; the eBPF programs are Dual MIT/GPL-2.0.

## 0.1.0 — 2026-09-25

First release: Gregg's ten 60-second checks (`uptime`, `dmesg`, `vmstat`, `mpstat`, `pidstat`,
`iostat`, `free`, `sar -n DEV`, `sar -n TCP,ETCP`, `top`) plus PSI pressure, as one static musl
binary for x86_64 and aarch64. It reads `/proc`, `/sys` and `/dev/kmsg` directly and never runs
an external command. The report has a system header and OK/WARN/CRIT per section; there is JSON
output, and the exit code reflects the worst status. Thresholds are cgroup-aware.
