# Tasks

## 1. Fixes

- [x] 1.1 cgroups-top: resolve the own cgroup path once and exclude it from the throttling finding; verify test `own_cgroup_not_double_reported`
- [x] 1.2 tcp: InErrors WARN only for ΔInErrors > ΔRcvbufErrors; verify updated `udp_errors_warn`
