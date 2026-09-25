# Tasks

## 1. Parser

- [x] 1.1 `src/procfs/sockstat.rs`: `parse` for sockstat/sockstat6 lines (`PROTO: key value …`) and `parse_numbers` for whitespace-separated sysctl values; verified by `procfs::sockstat::tests` (`parses_sockstat_fixtures`, `parses_legacy_sockstat`, `rejects_bad_input`, `parses_sysctl_numbers`)
- [x] 1.2 Register `pub mod sockstat;` in `src/procfs/mod.rs`; verified by `cargo build --locked`

## 2. Check

- [x] 2.1 `src/checks/sockets.rs`: `Sockets` check with summary, per-protocol details and metrics; verified by `checks::sockets::tests::summary`, `percent_format` and `ipv6_sockets_counted`
- [x] 2.2 Conntrack capacity (80/80.1/90/90.1) and "not in use"; verified by `conntrack_boundaries` and `conntrack_not_in_use`
- [x] 2.3 Ephemeral ports (50/50.1, IPv6 counted); verified by `tw_port_boundaries` and `ipv6_time_wait_counted`
- [x] 2.4 TIME_WAIT trend note; verified by `time_wait_trend_note`
- [x] 2.5 Orphans (50/50.1) and TCP memory (80/80.1); verified by `orphan_boundaries` and `tcp_mem_boundaries`
- [x] 2.6 Missing sysctls skip only their finding; SKIPPED without sockstat; verified by `missing_sysctls_skip_only_their_finding`, `missing_sockstat_is_skipped` and `no_sockstat6_is_fine`
- [x] 2.7 Register `Box::new(sockets::Sockets::default())` in `src/checks/mod.rs`; verified by `tests/fixture_tree.rs` (not SKIPPED on linux-arm64 and linux-legacy) and `cargo run --example fixture_report -- linux-legacy`

## 3. Verification

- [x] 3.1 Gates: `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked`
- [x] 3.2 Static aarch64 musl binary in an alpine container, idle and with TIME_WAIT buildup (non-zero `time-wait` in the sockets section)
