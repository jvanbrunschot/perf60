# perf60

A single static Rust binary that runs the checks from Brendan Gregg's
"Linux Performance Analysis in 60,000 Milliseconds" (uptime, dmesg, vmstat, mpstat, pidstat,
iostat, free, sar -n DEV, sar -n TCP,ETCP, top) and prints a short system spec
plus an OK/WARN/CRIT report.

## Hard rules

- **Never shell out.** No `std::process::Command`. Everything comes from `/proc`, `/sys`,
  `/etc/os-release` and `/dev/kmsg` (or the `klogctl` syscall). The binary must work on a minimal
  server with no procps/sysstat installed.
- **Parsers are pure.** `fn parse_x(input: &str) -> Result<T>` lives in `src/procfs/`, has no I/O,
  and is unit-tested against fixtures in `tests/fixtures/`. Only the readers touch the filesystem
  and are `#[cfg(target_os = "linux")]`, so `cargo test` runs on macOS as well.
- **Degrade, don't die.** A missing file or a permission error marks the check `SKIPPED (reason)`.
  It never aborts the run.
- **Few dependencies.** Allowed: `libc`, `serde`, `serde_json`. Anything else needs a design.md
  decision in its OpenSpec change.
- **Every threshold is spec'd.** Each WARN/CRIT threshold has a `#### Scenario` in the capability's
  spec and a unit test.

## Work method

### Spec-driven with OpenSpec
Every feature starts as an OpenSpec change (`openspec/changes/<change-id>/`). The CLI is
`openspec` (npm `@fission-ai/openspec`). Claude integration lives in `.claude/commands/opsx/` and
`.claude/skills/openspec-*`.

1. **Propose:** `/opsx:propose <idea>` (or `openspec new change <id>`), then write `proposal.md`,
   `specs/<capability>/spec.md` (delta), `design.md` (only when needed) and `tasks.md`.
2. **Validate:** `openspec validate <id> --strict`.
3. **Apply:** `/opsx:apply <id>`. Implement the tasks and tick the checkboxes in `tasks.md`.
4. **Archive:** `openspec archive <id> -y` merges the delta into `openspec/specs/<capability>/spec.md`
   and moves the change to `openspec/changes/archive/`.

Project context and rules for artifacts are in `openspec/config.yaml`.

### One feature = one commit
- Each OpenSpec change lands on `main` as **exactly one** commit with a conventional message, e.g.
  `feat(disk): add iostat-equivalent disk check`. The commit contains the archived change,
  the updated main spec, the code and the tests.
- Non-feature housekeeping uses `chore:`, `docs:`, `build:` or `fix:`.
- Before every commit, run:
  `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && openspec validate --all --strict`

### Pull requests and CI (GitHub: `jvanbrunschot/perf60`)
- `main` is only changed through pull requests. Work on `feat/<change-id>` (or `fix/…`,
  `chore/…`), push the branch, and open a PR with `gh pr create`.
- CI (`.github/workflows/ci.yml`) runs on every PR:
  - fmt, clippy, cargo test and `openspec validate --all --strict`
  - `scripts/verify.sh` on an x86_64 runner and an arm64 runner
  All jobs must be green before merging.
- Squash-merge the PR (`gh pr merge --squash`). The squash commit message is the conventional
  feature message, so `main` keeps one commit per feature.
- Don't push or merge without the user's go-ahead.

### Releases
- Bump `version` in `Cargo.toml` in a PR (e.g. `chore(release): 0.2.0`). After it is merged, tag
  `main` with a matching tag and push it: `git tag v0.2.0 && git push origin v0.2.0`.
- `.github/workflows/release.yml` checks that the tag equals `v<Cargo.toml version>` and reruns
  the full CI. Then it builds both static binaries with `scripts/build-release.sh` and publishes
  a GitHub release with them and `SHA256SUMS`. A tag with a suffix (`v0.2.0-rc.1`) becomes a
  pre-release.

### Worktrees for parallel work
- Serial foundation work happens on `main`.
- Independent features (typically one check each) are built in parallel in git worktrees on branch
  `feat/<change-id>`, e.g. `git worktree add ../perf60-wt/<change-id> -b feat/<change-id>`, or an
  Agent with `isolation: "worktree"`.
- To integrate: rebase the branch onto `main`, squash it to one commit, push it and open a PR
  (see above). Remove the worktree after the merge (`git worktree remove`).
- Keep shared touchpoints minimal. A new check should only need its own files plus one line in
  `src/checks/mod.rs` (the registry) and, if needed, a `pub mod` line in `src/procfs/mod.rs`.

## Layout

```
src/main.rs          CLI, orchestration, exit code (0 OK, 1 WARN, 2 CRIT)
src/sample.rs        periodic snapshots of raw /proc sources
src/check.rs         Check trait, Section/Status types
src/checks/          one module per check + registry (mod.rs)
src/procfs/          pure parsers
src/sysinfo.rs       system spec header
src/report/          text and JSON renderers
tests/fixtures/      captured /proc and /sys samples
scripts/verify.sh    builds a static Linux binary and runs it in minimal containers
scripts/capture-fixture.sh  captures a /proc+/sys fixture tree from a container
scripts/build-release.sh    static release binaries + checksums into dist/
.github/workflows/   ci.yml (PRs), release.yml (v* tags)
```

## Building and verifying

- Unit tests (macOS or Linux): `cargo test`
- Static Linux binary: `cargo build --release --target aarch64-unknown-linux-musl`
  (or `x86_64-unknown-linux-musl`). This links with Rust's bundled `rust-lld` via
  `.cargo/config.toml`, so no cross toolchain is needed, even on macOS.
- Release binaries for both architectures plus `SHA256SUMS`: `scripts/build-release.sh` (output in `dist/`)
- End-to-end in minimal containers (alpine, debian-slim, busybox): `scripts/verify.sh [target]`.
  `x86_64-unknown-linux-musl` runs under amd64 emulation on arm64 hosts.
