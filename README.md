# perf60

[![CI](https://github.com/jvanbrunschot/perf60/actions/workflows/ci.yml/badge.svg)](https://github.com/jvanbrunschot/perf60/actions/workflows/ci.yml)

Brendan Gregg's [Linux Performance Analysis in 60,000 Milliseconds](https://netflixtechblog.com/linux-performance-analysis-in-60-000-milliseconds-accc10403c55)
as one static binary.

perf60 runs the ten checks from the article and prints a short OK/WARN/CRIT report with a system
spec header. It reads `/proc`, `/sys` and `/dev/kmsg` directly and never runs an external
command. That means it works on minimal servers and containers without procps or sysstat.

```
perf60 0.1.0 · web01 · Linux 6.8.0 · Ubuntu 24.04 · x86_64 · 8 cpus · 16 GiB RAM · KVM · up 12d 3h04m · sampled 5×1s
  cpu     Intel(R) Xeon(R) Platinum 8375C CPU @ 2.90GHz
  swap    none
  disks   nvme0n1 100 GiB ssd
  nics    ens5 up 25000Mb/s

OVERALL: WARN  (2 warnings)

[ OK ] load        3.10 2.80 2.50 (1/5/15m) on 8 cpus, 4/612 tasks runnable
[ OK ] kernel-log  48213 records, no errors
[WARN] cpu         us 41% sy 6% id 29% wa 24% st 0%  r=3.2 b=4.0  cs 18k/s
                   peak interval busy 58%, interrupts 21k/s
                   ! iowait 24%: I/O bound, CPUs sit idle waiting on I/O
                   · b=4.0: tasks blocked on I/O (uninterruptible sleep)
[ OK ] cpu-balance 8 cpus, avg 47%, max cpu3 71%
[ OK ] processes   612 processes, top: java 310% (pid 2231)
[WARN] disk        nvme0n1 util 84% (peak 97%) r/s 3200 w/s 850 await 7.9ms aqu 31.2
                   nvme0n1  r/s 3200.0  w/s 850.0  rkB/s 409600.0  wkB/s 54400.0  r_await 7.10  w_await 11.00  aqu-sz 31.20  %util 84.0 (peak 97.0)
                   ! nvme0n1 busy: %util 84.0% (peak 97%)
                   · nvme0n1: %util can be misleading for RAID, NVMe and virtual disks that serve requests in parallel; judge by await and aqu-sz
...
```
(Illustrative output, abridged.)

## Quick start

Download the binary for your architecture from the
[latest release](https://github.com/jvanbrunschot/perf60/releases/latest), copy it to the server
and run it:

```sh
v=0.1.0 arch=x86_64   # or aarch64
base=https://github.com/jvanbrunschot/perf60/releases/download/v$v
curl -LO $base/perf60-$v-$arch-linux-musl -LO $base/SHA256SUMS
grep "perf60-$v-$arch-" SHA256SUMS | shasum -a 256 -c -
gh attestation verify perf60-$v-$arch-linux-musl --repo jvanbrunschot/perf60   # optional: built by CI from this repo
chmod +x perf60-$v-$arch-linux-musl   # downloads are not executable
scp -p perf60-$v-$arch-linux-musl server:/tmp/perf60
ssh server 'sudo /tmp/perf60'
```

Root isn't required. Without it, only the kernel-log section may be SKIPPED, if
`kernel.dmesg_restrict=1` is set.

```
perf60 [OPTIONS]
  -i, --interval <SECONDS>  Seconds between samples [default: 1]
  -c, --count <N>           Number of sampling intervals [default: 5]
  -j, --json                Print a JSON report
      --no-color            Disable ANSI colors (also honors NO_COLOR)
  -v, --verbose             Show detail lines for every section, not only problems
```

Under the overall status, perf60 names the **likely bottleneck**. It groups every warning by
the resource it concerns and quotes the strongest evidence from different sections, e.g.:

```
OVERALL: CRIT  (3 critical, 1 skipped)
Likely bottleneck: disk I/O
  · cpu: iowait 67%
  · disk: vda saturated: %util 90.8% (peak 91%)
  · pressure: io some pressure 90.5% of the window
```

The text report shows one line per check. Detail lines only appear for sections that need
attention, unless you pass `-v`. `--json` always includes every metric, detail and finding.

Exit status: `0` OK, `1` WARN, `2` CRIT, `3` usage error or unsupported OS. This makes it usable
from monitoring and scripts, e.g. `perf60 -c 3 --json > /var/tmp/perf60.json || alert`.

## What each section replaces

| Section | Article command | Source | Flags |
|---|---|---|---|
| `load` | `uptime` | `/proc/loadavg` | load1 > CPUs (WARN), > 2× CPUs (CRIT); rising/falling trend |
| `kernel-log` | `dmesg \| tail` | `/dev/kmsg`, falls back to `klogctl(2)` | OOM kills, hung tasks, I/O and filesystem errors, panics, lockups, MCE (CRIT); SYN floods, segfaults, conntrack full, link down, storage resets (WARN). Only events in the last hour escalate. |
| `cpu` | `vmstat 1` | `/proc/stat`, `/proc/schedstat` | run queue > CPUs when busy; average run-queue wait > 2/10 ms; iowait > 20/50%; steal > 10/25%; busy > 90% |
| `cpu-balance` | `mpstat -P ALL 1` | `/proc/stat` | one CPU > 90% while the mean is < 50% (single-thread or IRQ bottleneck) |
| `processes` | `pidstat 1` | `/proc/<pid>/stat`, `/proc/stat` | top 5 by CPU; one process > 90% of capacity; D-state tasks > CPUs; > 50 zombies; fork rate > 100/s (note) or > 1000/s (short-lived process storms) |
| `disk` | `iostat -xz 1` | `/proc/diskstats` | %util > 60/90; await > 10/50 ms (SSD) or 50/200 ms (HDD) |
| `memory` | `free -m` | `/proc/meminfo`, cgroup files, `/proc/vmstat` | available < 10/5%; cgroup working set > 90/95% of its limit; OOM kills in the window; page-cache refaults > 1000/s (thrashing); compaction stalls, NUMA misses (notes) |
| `swap` | `vmstat 1` si/so | `/proc/vmstat` | any swapping (WARN), > 256 pages/s (CRIT) |
| `net` | `sar -n DEV 1` | `/proc/net/dev`, `/sys/class/net`, `/proc/net/softnet_stat`, `/proc/softirqs` | utilization > 70/90% of link speed; errors or drops; backlog drops (softnet); NAPI squeeze and NET_RX on one CPU (notes) |
| `tcp` | `sar -n TCP,ETCP 1` | `/proc/net/snmp`, `/proc/net/netstat` | retransmits > 1/5% of segments sent; listen queue overflows; UDP buffer/input errors; TCP backlog drops and aborts on memory |
| `pressure` | `top` (PSI) | `/proc/pressure/*`, cgroup `*.pressure` | CPU, memory or I/O stall > 10/25% of the window; memory or I/O full stall > 5% |

### Beyond the 2015 checklist

The article predates containers, cgroup v2, PSI and modern NVMe, and it doesn't look at
capacity limits or hardware health. These sections cover what commonly takes systems down
today:

| Section | Replaces | Source | Flags |
|---|---|---|---|
| `cgroup` | `cat cpu.stat memory.events` | own cgroup (v2, or v1 `cpu,cpuacct`/`memory`) | CPU quota throttling > 10/25% of periods; `oom_kill` (CRIT), `memory.max` hits (WARN), `memory.high` (note) |
| `cgroups-top` | `systemd-cgtop` | `/sys/fs/cgroup` tree | top 5 leaf cgroups by CPU and memory; any cgroup throttled > 25% (e.g. Kubernetes pods on a node) |
| `sockets` | `ss -s`, `conntrack -S` | `/proc/net/sockstat{,6}`, `nf_conntrack_*`, TCP sysctls | conntrack table > 80/90%; TIME_WAIT > 50% of the ephemeral port range; orphans > 50%; TCP memory > 80% of `tcp_mem` |
| `filesystems` | `df -h`, `df -i` | `/proc/self/mounts`, `statvfs(3)` | space or inodes > 85/95%; unexpected read-only mounts. Network and FUSE filesystems are never stat'ed, since that can hang |
| `limits` | `ulimit -n`, `file-nr` | `/proc/sys/fs/file-nr`, `pid_max`, `threads-max`, cgroup `pids.*`, `/proc/<pid>/limits` | fds, tasks and cgroup pids > 80/90%; a process above 90% of its open-files limit |
| `hardware` | `edac-util`, `cpupower`, `chronyc tracking` | EDAC, `thermal_throttle`, cpufreq, `/proc/sys/kernel/tainted`, `adjtimex(2)` | uncorrectable memory errors, machine checks (CRIT); new corrected errors, thermal throttling, oops/soft lockup taint, unsynchronized clock (WARN); powersave governor (note) |

CPU thresholds use *effective* CPUs, meaning online CPUs lowered to the cgroup CPU quota.
Memory checks also take the cgroup memory limit into account, so the report means the same
thing inside a container. The full behaviour of each check, including every threshold, is
specified in [`openspec/specs/`](openspec/specs/).

## Deep mode (eBPF)

`perf60 --deep` adds probes from the BCC checklist in Gregg's *BPF Performance Tools* (2019).
The kernel aggregates events during the sampling window and perf60 reads the totals at the end:

| Section | BCC tool | What it shows |
|---|---|---|
| `execsnoop` | `execsnoop` | new processes by command, including the short-lived ones `pidstat` never sees; > 100 execs/s warns |
| `runqlat` | `runqlat` | run queue latency distribution: how long runnable tasks wait for a CPU (p50/p99/max); p99 > 10 ms warns, > 50 ms is critical |
| `biolatency` | `biolatency -D` | block I/O latency histogram per disk, issue to completion; p99 > 20 ms warns and > 100 ms is critical (rotational: 100 ms / 500 ms) |
| `tcpretrans` | `tcpretrans` | TCP retransmits by remote endpoint (`10.0.0.5:443`, `[2001:db8::1]:443`); a note when one endpoint has > 50% of ≥ 20 retransmits, > 100 retransmits/s warns |

`--deep` needs root, or `CAP_BPF` + `CAP_PERFMON` (kernel 5.8+); in a container, use
`--privileged`. It uses raw tracepoints, so it works without tracefs. Probes that read kernel
structs need the kernel's BTF (`/sys/kernel/btf/vmlinux`, standard on current distributions).
Without the privileges, or on a build without eBPF support, the probe sections are SKIPPED
with the reason and the rest of the report is unaffected.

## Building

You need a Rust toolchain. No cross compiler or container is needed: the musl targets link with
Rust's bundled `rust-lld` (see `.cargo/config.toml`).

```sh
cargo test                     # unit + fixture tests, runs on macOS too
scripts/build-deep.sh          # --features deep (eBPF probes), built in a rust container
scripts/build-release.sh       # dist/perf60-<version>-{x86_64,aarch64}-linux-musl + SHA256SUMS
scripts/verify.sh              # run in alpine, debian-slim and busybox containers
scripts/verify.sh x86_64-unknown-linux-musl   # the other architecture, under emulation
```

## Releasing

Bump `version` in `Cargo.toml` through a PR. Once it is merged, push a matching tag:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

The [release workflow](.github/workflows/release.yml) reruns all CI checks, builds both static
binaries, attests their build provenance, and publishes them with `SHA256SUMS` as a GitHub
release. The tag must equal
`v<Cargo.toml version>`. A tag with a suffix such as `v0.2.0-rc.1` becomes a pre-release.

## License

MIT. The eBPF programs in `perf60-ebpf/` are dual-licensed MIT OR GPL-2.0, because the kernel
only lets GPL-compatible programs use the tracing helpers they need.

## Development

The project is spec-driven with [OpenSpec](https://github.com/Fission-AI/OpenSpec), and every
feature lands as one commit. See [`CLAUDE.md`](CLAUDE.md) for the rules and workflow.
