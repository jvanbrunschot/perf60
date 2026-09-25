# Tasks

## 1. eBPF program

- [x] 1.1 `perf60-ebpf/src/bin/tcpretrans.rs` on raw tracepoint `tcp_retransmit_skb`, reading `sock_common` fields at BTF offsets passed as globals; verify `scripts/build-deep.sh aarch64-unknown-linux-musl` builds without warnings

## 2. User space

- [x] 2.1 `deep::tcpretrans` key type and endpoint formatting; verify `tcpretrans::tests::key_layout`, `format_v4`, `format_v6`, `format_v4_mapped`
- [x] 2.2 Pure `evaluate` (summary, top-5 details, metrics); verify `tcpretrans::tests::quiet_window`, `summary_and_metrics`, `top_n_ordering`
- [x] 2.3 Concentration note and rate WARN; verify `tcpretrans::tests::concentration_count_boundary`, `concentration_share_boundary`, `rate_boundary`
- [x] 2.4 `Tcpretrans` check (BTF offsets, attach, map reads) registered in `deep::checks()`; verify `cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked`

## 3. Live verification and docs

- [x] 3.1 Privileged container runs: idle (`--deep -c 2 -i 1 -v`) and forced retransmits with `tc netem loss` on `lo` (expect `127.0.0.1:9000`); unprivileged run SKIPPED with the needs-root reason
- [x] 3.2 README deep-mode table row; verify `openspec validate --all --strict`
