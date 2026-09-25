# Tasks

## 1. Report output

- [x] 1.1 Add `-v/--verbose` to `cli.rs` and USAGE; verify the cli unit tests (defaults false, `-v` sets it)
- [x] 1.2 `report/text.rs`: hide details of OK sections unless verbose, size the id column to the longest id; verify tests `ok_details_hidden`, `problem_details_shown`, `verbose_shows_all`, `long_ids_aligned`
- [x] 1.3 Pass verbose from `main.rs` to the renderer; verify `cargo build` and a container run

## 2. Check fixes

- [x] 2.1 cpu: gate the run-queue threshold on CPUs in use ≥ 50% of effective capacity; verify the updated run-queue tests plus `run_queue_on_idle_cpus` and `run_queue_under_cgroup_quota`
- [x] 2.2 processes: OK "no other processes visible" when only perf60 is listed; verify tests `perf60_alone_is_ok` and `no_readable_process_is_skipped`
- [x] 2.3 kernel-log: access hint on a missing `/dev/kmsg`; verify test `missing_kmsg_has_hint`

## 3. Integration

- [x] 3.1 Run `scripts/verify.sh` and check that an idle alpine run prints one line per section without details
