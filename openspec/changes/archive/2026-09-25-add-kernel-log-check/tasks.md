# Tasks

## 1. Parsers

- [x] 1.1 `procfs::kmsg::parse_record` for `prio,seq,usec,flags;message` (level = prio & 7, continuation lines → none); verify unit tests `parses_record`, `continuation_lines_are_ignored`, `rejects_garbage` and `parses_fixture` (every non-continuation line of `tests/fixtures/linux-arm64/dev/kmsg` parses)
- [x] 1.2 `procfs::kmsg::from_syslog` converting `<prio>[ secs.usecs] msg` lines to kmsg records; verify unit tests `converts_syslog_lines` and `syslog_line_without_timestamp`

## 2. Live source

- [x] 2.1 Linux-only `klogctl` fallback in `src/source.rs` when opening `/dev/kmsg` fails, returning the original error when both fail; verify `cargo clippy --all-targets --target aarch64-unknown-linux-musl -- -D warnings` and the unprivileged/privileged container runs in 4.2

## 3. Check

- [x] 3.1 `checks::kernel_log` classification with word-boundary patterns; verify unit tests `recent_oom_kill_is_crit`, `old_oom_kill_is_note`, `one_hour_boundary`, `recent_syn_flood_is_warn`, `word_boundaries_avoid_false_matches`
- [x] 3.2 Summary, details and metrics; verify unit tests `clean_log_is_ok`, `errors_in_summary`, `details_show_last_five_warnings`, `continuation_lines_not_counted`
- [x] 3.3 SKIPPED handling with the permission hint; verify unit tests `permission_denied_is_skipped_with_hint` and `missing_kmsg_is_skipped`
- [x] 3.4 Register the check after `load` in `src/checks/mod.rs`; verify `tests/fixture_tree.rs` (`every_registered_check_renders_against_fixture`) and unit test `fixture_parses`

## 4. Integration

- [x] 4.1 Gates: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` pass
- [x] 4.2 Build `aarch64-unknown-linux-musl` and run in an alpine container unprivileged (SKIPPED with reason or klogctl records) and with `--privileged` (real records)
