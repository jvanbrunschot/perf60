#!/bin/sh
# Capture a fixture tree of the kernel interfaces perf60 reads, for use as FsSource root in tests.
# Usage: scripts/capture-fixture.sh <name> [extra docker run args...]
# Runs a privileged alpine container (so /dev/kmsg is readable) and streams the files as a tar
# into tests/fixtures/<name>/. Dev tooling only: the perf60 binary itself never shells out.
#
# Besides plain file copies the tree contains:
#   proc/<pid>/fd/<n>   empty files, so read_dir("/proc/<pid>/fd") counts open fds
#   statvfs.txt         statvfs(3) of every mount (see source::parse_statvfs_table)
#   adjtimex.txt        adjtimex(2) state (see source::parse_clock_status)
set -eu
name=${1:?usage: capture-fixture.sh <name> [docker args]}
shift
out="$(cd "$(dirname "$0")/.." && pwd)/tests/fixtures/$name"
rm -rf "$out"
mkdir -p "$out"
docker run --rm --privileged "$@" alpine sh -c '
  mkdir -p /out
  cp_file() { [ -e "$1" ] || return 0; mkdir -p "/out$(dirname "$1")"; cat "$1" > "/out$1" 2>/dev/null || rm -f "/out$1"; }

  # procfs and sysctls
  for f in /proc/loadavg /proc/uptime /proc/stat /proc/meminfo /proc/vmstat /proc/cpuinfo \
           /proc/diskstats /proc/schedstat /proc/softirqs /proc/interrupts \
           /proc/net/dev /proc/net/snmp /proc/net/netstat /proc/net/softnet_stat \
           /proc/net/sockstat /proc/net/sockstat6 \
           /proc/pressure/cpu /proc/pressure/memory /proc/pressure/io \
           /proc/self/cgroup /proc/self/mounts \
           /proc/sys/kernel/hostname /proc/sys/kernel/osrelease /proc/sys/kernel/pid_max \
           /proc/sys/kernel/threads-max /proc/sys/kernel/tainted /proc/sys/kernel/arch \
           /proc/sys/fs/file-nr /proc/sys/fs/file-max /proc/sys/fs/nr_open \
           /proc/sys/net/netfilter/nf_conntrack_count /proc/sys/net/netfilter/nf_conntrack_max \
           /proc/sys/net/ipv4/tcp_mem /proc/sys/net/ipv4/tcp_max_orphans \
           /proc/sys/net/ipv4/ip_local_port_range \
           /etc/os-release /sys/devices/system/cpu/online \
           /sys/class/dmi/id/product_name /sys/class/dmi/id/sys_vendor; do
    cp_file "$f"
  done

  # cgroup v2 (own cgroup; a container sees its own cgroup as the root)
  for f in cgroup.controllers cpu.max cpu.stat cpu.pressure memory.max memory.high \
           memory.current memory.events memory.stat memory.pressure io.stat io.pressure \
           pids.current pids.max; do
    cp_file "/sys/fs/cgroup/$f"
  done

  # block devices, NICs, CPU frequency / thermal throttling, EDAC memory controllers
  for d in /sys/block/*; do
    cp_file "$d/size"; cp_file "$d/queue/rotational"
  done
  for d in /sys/class/net/*; do
    cp_file "$d/operstate"; cp_file "$d/speed"
  done
  for d in /sys/devices/system/cpu/cpu[0-9]*; do
    for f in cpufreq/scaling_cur_freq cpufreq/cpuinfo_max_freq cpufreq/scaling_governor \
             thermal_throttle/core_throttle_count thermal_throttle/package_throttle_count; do
      cp_file "$d/$f"
    done
  done
  for d in /sys/devices/system/edac/mc/mc*; do
    cp_file "$d/ce_count"; cp_file "$d/ue_count"
  done

  # processes: stat, limits and one empty file per open fd
  sleep 2 & sleep 2 &
  for p in /proc/[0-9]*; do
    cp_file "$p/stat"; cp_file "$p/limits"
    if [ -d "$p/fd" ]; then
      mkdir -p "/out$p/fd"
      for fd in $(ls "$p/fd" 2>/dev/null); do : > "/out$p/fd/$fd"; done
    fi
  done

  # statvfs of every mount: <mountpoint> <frsize> <blocks> <bfree> <bavail> <files> <ffree> <favail> <ro|rw>
  echo "# mountpoint frsize blocks bfree bavail files ffree favail ro" > /out/statvfs.txt
  while read -r dev mp fstype opts rest; do
    case "$mp" in *\\*) continue ;; esac
    case ",$opts," in *,ro,*) ro=ro ;; *) ro=rw ;; esac
    set -- $(stat -f -c "%S %b %f %a %c %d" "$mp" 2>/dev/null) || continue
    [ $# -eq 6 ] && echo "$mp $1 $2 $3 $4 $5 $6 $6 $ro" >> /out/statvfs.txt
  done < /proc/self/mounts

  # adjtimex (read-only query)
  busybox adjtimex 2>/dev/null | awk "
    /offset:/   { print \"offset\", \$3 }
    /maxerror:/ { print \"maxerror\", \$2 }
    /esterror:/ { print \"esterror\", \$2 }
    /status:/   { print \"status\", \$2; print \"state\", (int(\$2) % 128 >= 64) ? 5 : 0 }
  " > /out/adjtimex.txt

  mkdir -p /out/dev /out/run
  timeout 1 cat /dev/kmsg > /out/dev/kmsg 2>/dev/null || true
  [ -e /run/.containerenv ] && : > /out/run/.containerenv
  [ -e /.dockerenv ] && : > /out/.dockerenv
  tar -C /out -cf - .
' | tar -C "$out" -xf -
echo "captured $(find "$out" -type f | wc -l | tr -d " ") files into $out"
