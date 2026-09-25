# Tasks

## 1. Parsers

- [x] 1.1 `procfs::net_dev` parser for `/proc/net/dev` (two header lines, 16 counters, no space required after `:`); verify `net_dev::tests` against the fixture and inline inputs
- [x] 1.2 `procfs::snmp` parser for the paired header/value format of `/proc/net/snmp` and `/proc/net/netstat`, signed values (MaxConn -1); verify `snmp::tests` against both fixture files

## 2. Net check

- [x] 2.1 `checks::net::NetDev`: window rates, skip `lo`, busiest-interface summary, idle summary, details and metrics; verify tests `summary_names_busiest_interface`, `summary_shows_util`, `all_idle`, `loopback_ignored`
- [x] 2.2 Utilization from `/sys/class/net/<if>/speed` with WARN > 70% and CRIT > 90%; verify tests `util_boundaries` and `unknown_speed_has_no_util`
- [x] 2.3 WARN on errors/drops during the window; verify tests `errors_warn`, `drops_warn`, `old_errors_are_ok`
- [x] 2.4 SKIPPED when `/proc/net/dev` is unavailable; verify test `missing_net_dev_is_skipped`

## 3. TCP check

- [x] 3.1 `checks::net::Tcp`: rates, CurrEstab and summary; verify test `tcp_summary`
- [x] 3.2 Retransmit ratio with the 100-segment minimum, WARN > 1% and CRIT > 5%; verify tests `retrans_not_judged_below_minimum` and `retrans_boundaries`
- [x] 3.3 Since-boot retransmit note; verify test `since_boot_retrans_note`
- [x] 3.4 Listen overflow/drop WARN from `/proc/net/netstat`, optional source; verify tests `listen_overflow_warns`, `listen_drops_warn`, `missing_netstat_is_fine`
- [x] 3.5 SKIPPED when `/proc/net/snmp` is unavailable; verify test `missing_snmp_is_skipped`

## 4. Integration

- [x] 4.1 Register `net` and `tcp` after `load` in `src/checks/mod.rs`; verify `tests/fixture_tree.rs` (not SKIPPED, no NaN)
- [x] 4.2 Run the static musl binary in an alpine container while downloading a file; verify the net and tcp sections show traffic
