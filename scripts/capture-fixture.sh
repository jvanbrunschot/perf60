#!/bin/sh
# Capture a fixture tree of the kernel interfaces perf60 reads, for use as FsSource root in tests.
# Usage: scripts/capture-fixture.sh <name> [extra docker run args...]
# Runs a privileged alpine container (so /dev/kmsg is readable) and streams the files as a tar
# into tests/fixtures/<name>/. Dev tooling only: the perf60 binary itself never shells out.
set -eu
name=${1:?usage: capture-fixture.sh <name> [docker args]}
shift
out="$(cd "$(dirname "$0")/.." && pwd)/tests/fixtures/$name"
rm -rf "$out"
mkdir -p "$out"
docker run --rm --privileged "$@" alpine sh -c '
  mkdir -p /out
  cp_file() { mkdir -p "/out$(dirname "$1")"; cat "$1" > "/out$1" 2>/dev/null || rm -f "/out$1"; }
  for f in /proc/loadavg /proc/uptime /proc/stat /proc/meminfo /proc/vmstat /proc/cpuinfo \
           /proc/diskstats /proc/net/dev /proc/net/snmp /proc/net/netstat \
           /proc/pressure/cpu /proc/pressure/memory /proc/pressure/io /proc/self/cgroup \
           /proc/sys/kernel/hostname /proc/sys/kernel/osrelease /etc/os-release \
           /sys/devices/system/cpu/online /sys/fs/cgroup/cpu.max /sys/fs/cgroup/memory.max \
           /sys/class/dmi/id/product_name /sys/class/dmi/id/sys_vendor; do
    [ -e "$f" ] && cp_file "$f"
  done
  for d in /sys/block/*; do
    cp_file "$d/size"; cp_file "$d/queue/rotational"
  done
  for d in /sys/class/net/*; do
    cp_file "$d/operstate"; cp_file "$d/speed"
  done
  sleep 2 & sleep 2 &
  for p in /proc/[0-9]*; do cp_file "$p/stat"; done
  mkdir -p /out/dev /out/run
  timeout 1 cat /dev/kmsg > /out/dev/kmsg 2>/dev/null || true
  [ -e /run/.containerenv ] && : > /out/run/.containerenv
  [ -e /.dockerenv ] && : > /out/.dockerenv
  tar -C /out -cf - .
' | tar -C "$out" -xf -
echo "captured $(find "$out" -type f | wc -l | tr -d " ") files into $out"
