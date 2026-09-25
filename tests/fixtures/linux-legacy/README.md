# linux-legacy fixture (synthetic)

Hand-assembled, not captured: this machine can't boot old kernels. It mimics a CentOS 7 VM
(kernel 3.10, x86_64, 2 vCPU) and mixes formats that newer kernels changed, so the parsers'
fallback paths are exercised end to end:

| File | Old-kernel trait |
|---|---|
| `proc/meminfo` | no `MemAvailable` (added in 3.14) |
| `proc/stat` | 8 cpu columns (no guest/guest_nice) |
| `proc/vmstat` | no `oom_kill` (4.13), legacy `workingset_refault` (split in 5.9), zone-suffixed `pgscan_direct_*` (pre-4.8) |
| `proc/diskstats` | 14 columns (discard/flush columns came in 4.18/5.5) |
| `proc/schedstat` | version 15 |
| `proc/net/softnet_stat` | 10 columns |
| `proc/self/cgroup`, `sys/fs/cgroup/*/` | cgroup v1 hierarchy, `cpu.stat` with `throttled_time` in ns |
| no `proc/pressure/` | PSI came in 4.20: the pressure section must be SKIPPED |
| no `nf_conntrack_*` | conntrack module not loaded |
| `sys/devices/system/edac` | 3 corrected memory errors since boot |
| `sys/devices/system/cpu/cpu*/cpufreq` | `powersave` governor |
| `proc/4211/` (`app`) | 240 open fds against a soft `RLIMIT_NOFILE` of 256 |
| `proc/sys/net/ipv4/*`, `proc/net/sockstat` | 1204 sockets in TIME_WAIT against a 28232-port range |

`statvfs.txt` and `adjtimex.txt` follow the formats in `src/source.rs`.
