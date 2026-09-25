# Tasks

## 1. Parsers

- [x] 1.1 `src/procfs/schedstat.rs`: `cpuN` run_delay and timeslices, version ≥ 15; verify `procfs::schedstat::tests` (both fixture trees, old version rejected, garbage rejected)
- [x] 1.2 `src/procfs/softnet.rs`: hex rows, 10- and 15-column formats; verify `procfs::softnet::tests`
- [x] 1.3 `src/procfs/softirqs.rs`: CPU header and named rows; verify `procfs::softirqs::tests`
- [x] 1.4 `processes` field in `src/procfs/stat.rs`; verify `procfs::stat::tests::parses_fixture` and `parses_processes_counter`

## 2. Checks

- [x] 2.1 cpu run-queue latency; verify `checks::cpu::tests::runq_wait_thresholds`, `runq_wait_average_and_worst_cpu`, `runq_wait_needs_timeslices`, `runq_wait_missing_schedstat`
- [x] 2.2 processes fork rate; verify `checks::processes::tests::fork_rate_thresholds`, `fork_rate_missing_counter`, `fork_rate_when_alone`
- [x] 2.3 memory refaults, compaction stalls and NUMA misses; verify `checks::memory::tests::refault_thresholds`, `legacy_refault_counter`, `compaction_stall_note`, `numa_miss_ratio`, `no_vmstat_omits_latency_signals`
- [x] 2.4 net softnet drops/squeezes and NET_RX concentration; verify `checks::net::tests::softnet_drops_warn`, `softnet_squeeze_note`, `softnet_missing_is_fine`, `net_rx_concentration`
- [x] 2.5 tcp UDP and TcpExt socket memory errors; verify `checks::net::tests::udp_errors_warn`, `udplite_counts`, `no_udp_section`, `tcp_socket_memory_drops`

## 3. Verification

- [x] 3.1 Gates: `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked` (includes `tests/fixture_tree.rs` on both trees: nothing SKIPPED, no NaN)
- [x] 3.2 `cargo run --example fixture_report -- linux-legacy` and `-- linux-arm64`
- [x] 3.3 Linux containers: idle, 16 busy loops (run-queue WARN/CRIT) and a fork storm (fork note/WARN) with the aarch64 musl release binary
