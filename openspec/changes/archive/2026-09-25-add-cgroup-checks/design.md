# Design

## Context

`Source` only offers `read_dir` (names, no file types), `exists` and `read_to_string`. The
`cgroups-top` walk runs on every sample over up to 2000 cgroups, each holding 30 to 60
interface files, so we must not probe every file name to find out whether it is a directory.

## Decisions

### Finding sub-cgroups without file types
Every cgroup interface file is named `<controller>.<name>` (`cgroup.procs`, `cpu.stat`,
`memory.current`, `io.pressure`, …) or is one of the v1 core files `tasks`,
`notify_on_release` and `release_agent`. The walk skips those names and tries `read_dir` on the
remaining entries. A failing `read_dir` means the entry is not a directory. This way each
sample costs one `read_dir` per cgroup, plus the reads of its `cpu.stat` and memory file.
Directories without usage files (possible in synthetic trees) are still descended into.
They are not counted as cgroups, but they do make their parent a non-leaf.

### Leaves, depth and cap
Parent cgroups include their children's usage, so the listing shows only leaves. The walk stops
at depth 6 below the hierarchy root, and a cgroup at depth 6 counts as a leaf. Its counters
cover its whole subtree, so the leaves still split the usage without double counting. The walk
is breadth-first and stops after 2000 directories. That way a huge hierarchy loses its deepest
levels first, and the report says the walk was capped.

### Per-path state
The walk visits every cgroup on every sample. For each path it keeps only the first and the
last sample (time, CPU usage, periods, throttled periods, memory). The evaluation uses the
cgroups present in the final walk.

### Hierarchy mounted at the own cgroup
Under cgroup v1 without a cgroup namespace (and in some v2 setups), a container sees its own
cgroup's files at the hierarchy mount root, while `/proc/self/cgroup` shows the host path. When
`<mount><path>/cpu.stat` does not exist but `<mount>/cpu.stat` does, the `cgroup` check reads
the mount root and says so. On a host the own cgroup directory always exists, so the fallback
never applies there.

### v1 or v2 for the own cgroup
When `/proc/self/cgroup` has a v1 line for `cpu`, `cpuacct` or `memory`, the check uses v1,
because in hybrid mode those controllers are bound to v1. Otherwise it uses the `0::` line (v2).

### Throttling since creation
With a zero-length window (for example the fixture reports), the window has no periods to
compare. When cumulative `nr_periods` is non-zero, the check adds a detail line with the
throttled share since the cgroup was created. This line is context only: thresholds use the
window.

## Risks

A directory whose name looks like an interface file (for example `cpu.slice`) would be missed
by the walk. systemd, Docker, containerd and Kubernetes don't name cgroups like that.
