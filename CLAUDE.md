# perf60

A single static Rust binary that runs the checks from Brendan Gregg's
"Linux Performance Analysis in 60,000 Milliseconds" (uptime, dmesg, vmstat, mpstat, pidstat,
iostat, free, sar -n DEV, sar -n TCP,ETCP, top) and prints a short system spec
plus an OK/WARN/CRIT report.

## Hard rules

- **Never shell out.** No `std::process::Command`. Everything comes from `/proc`, `/sys`,
  `/etc/os-release` and `/dev/kmsg`, or from read-only syscalls (`klogctl`, `statvfs`,
  `adjtimex`). The binary must work on a minimal server with no procps/sysstat installed.
- **All kernel access goes through `Source`** (`src/source.rs`): files, directories, kmsg,
  `statvfs(path)` and `clock_status()`. That makes every check testable with `MemSource`
  (in-memory, with `set_statvfs`/`set_clock`) and with fixture trees (`FsSource::new(root)`,
  where syscalls come from `statvfs.txt` and `adjtimex.txt`).
- **Parsers are pure.** `fn parse_x(input: &str) -> Result<T>` lives in `src/procfs/`, has no I/O,
  and is unit-tested against fixtures in `tests/fixtures/`. Only the readers touch the filesystem
  and are `#[cfg(target_os = "linux")]`, so `cargo test` runs on macOS as well.
- **Degrade, don't die.** A missing file or a permission error marks the check `SKIPPED (reason)`.
  It never aborts the run.
- **Few dependencies.** Allowed: `libc`, `serde`, `serde_json`, and only behind the `deep`
  feature `aya`, `aya-build` (build) and `aya-ebpf` (in `perf60-ebpf`). Anything else needs a
  design.md decision in its OpenSpec change.
- **`--deep` eBPF probes** (`src/deep/`, `perf60-ebpf/`):
  - One probe is one eBPF program file, `perf60-ebpf/src/bin/<probe>.rs`, which Cargo
    auto-discovers and `build.rs` embeds as `$OUT_DIR/<probe>`. Its user-space check lives in
    `src/deep/<probe>.rs`, and it is registered in `deep::checks()`.
  - Attach to **raw tracepoints**, never classic tracepoints: they need no tracefs, which
    containers and minimal hosts lack. Read kernel structs with `bpf_probe_read_kernel` at
    offsets from `deep::btf::kernel_offsets`, which uses the running kernel's BTF. Pass the
    offsets as globals with `Probe::attach_with`.
  - Maps only (hash, array, per-CPU array, log2 histograms via `perf60_common::log2_bucket`,
    and in user space `deep::hist::Hist` for percentiles and BCC-style distribution lines).
    No ring buffers or perf buffers: the kernel aggregates and user space reads once at the end.
  - The evaluation (map contents → `Section`) is a pure, always-compiled function with unit
    tests. Only loading and attaching is `#[cfg(feature = "deep")]`.
  - A probe failure is SKIPPED with an actionable reason (`deep::caps` checks privileges first).
  - `perf60-ebpf` is Dual MIT/GPL-2.0, because the kernel only exposes the helpers it uses
    to GPL programs. Everything else is MIT.
- **Every threshold is spec'd.** Each WARN/CRIT threshold has a `#### Scenario` in the capability's
  spec and a unit test.
- **Every section has a resource.** `Section::new(id, title, equivalent, Resource::…)`. The
  resource (cpu, memory, disk, network, capacity, hardware, kernel, pressure) drives the
  "Likely bottleneck" diagnosis (`src/report/diagnosis.rs`).
- **Findings about another resource say so.** Use `warn_on`/`crit_on`/`note_on`/`threshold_on`
  with the resource they concern, e.g. iowait in the cpu section is `Resource::Disk`. Keep the
  number early in the message (`what 42%: why`), since the diagnosis quotes that leading
  part as evidence.
- **Fixture trees:**
  - `tests/fixtures/linux-arm64` is captured with `scripts/capture-fixture.sh linux-arm64`.
  - `tests/fixtures/linux-legacy` is synthetic, with old-kernel formats (see its README).
  - `tests/fixture_tree.rs` runs every check against both and lists each tree's expected
    SKIPPED sections. A new check must render on both trees or be added to the expected-skip
    list with a reason.
  - Recapturing `linux-arm64` changes values, so rebaseline the parser tests that assert exact
    fixture values in the same change.
  - `cargo run --example fixture_report -- linux-legacy` prints the report for a tree.

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
- Merge with rebase (`gh pr merge --rebase`). The repo only allows rebase merges: squash and
  merge commits are disabled in the settings and in the `main` ruleset. Each feature commit
  lands on `main` as it is, so a one-feature PR is one commit and a phase PR keeps one commit
  per feature. Merged branches are deleted automatically.
- Don't push or merge without the user's go-ahead.

### Supply-chain rules (Shai-Hulud-style worms)
- **Actions:** pin every third-party action to a full 40-hex commit SHA with the version in a
  comment. The repo enforces SHA pinning and only allows GitHub-owned actions plus
  `dtolnay/rust-toolchain` and `Swatinem/rust-cache`. A new third-party action must be added to
  that allow-list (Settings → Actions) in the same PR.
- **Checkouts and builds:** every checkout sets `persist-credentials: false`, and every cargo
  command in CI and the scripts uses `--locked`.
- **npm:** only `tools/openspec/` with its committed lockfile, installed with
  `npm ci --ignore-scripts`, in the `specs` job that has `permissions: {}`. Never add npm steps
  to jobs that hold write permissions.
- **Release job:** the only job with write, `id-token` and `attestations` permissions. It uses no
  cache and runs no npm.
- **Dependency updates:** Dependabot proposes updates to actions, npm tooling and cargo only after a
  7-day cooldown. Review the diff (SHA → tag) before merging.
- **Repo settings (applied via `gh api`, not in git):**
  - ruleset `main: PR + green CI`: PR required, the four CI checks must pass and come from GitHub
    Actions, rebase merges only, no force-push or deletion, no bypass
  - ruleset `release tags: admins only`: create, move and delete of `v*` tags
  - the actions allow-list with SHA pinning required
  - approval required before workflows run for all external contributors
  - workflow token read-only by default
  Changing CI job names means updating the required checks in the `main` ruleset too.

### Releases
- Use the **`/release` skill** (`.claude/skills/release/SKILL.md`). It covers the version choice,
  the release PR, merge and tag, watching the workflow and verifying the published assets. The
  bullets below are the underlying rules.
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
- Larger efforts (phases) collect their features on an integration branch, e.g.
  `phase/a-counters`, which becomes one PR, rebase-merged so each feature stays one commit.
- To integrate a finished worktree branch, run `scripts/integrate-worktree.sh <worktree-path>`
  from the main checkout, with the integration branch checked out. It:
  - requires exactly one commit on the branch
  - rebases it
  - resolves registry conflicts with `scripts/resolve-registry.py`
  - runs every gate
  - fast-forwards the integration branch and removes the worktree and branch

  A new section id must also be added to `ORDER` in `scripts/resolve-registry.py`.
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
tests/fixtures/      fixture trees: linux-arm64 (captured), linux-legacy (synthetic)
examples/fixture_report.rs  render the report for a fixture tree
scripts/verify.sh    builds a static Linux binary and runs it in minimal containers
scripts/capture-fixture.sh  captures a /proc+/sys fixture tree from a container
scripts/resolve-registry.py resolves registry conflicts between parallel feature branches
scripts/integrate-worktree.sh  rebase + gate + fast-forward one worktree branch
scripts/build-release.sh    static release binaries + checksums into dist/ (PERF60_FEATURES=deep in CI)
scripts/build-deep.sh       --features deep build inside a rust container
scripts/install-bpf-linker.sh  pinned, checksum-verified bpf-linker
src/deep/            --deep probes: loader, BTF reader, capability check, one module per probe
perf60-common/       no_std map types shared with the eBPF programs
perf60-ebpf/         eBPF programs, one per src/bin/<probe>.rs (Dual MIT/GPL-2.0)
.github/workflows/   ci.yml (PRs), release.yml (v* tags)
```

## Building and verifying

- Unit tests (macOS or Linux): `cargo test` (without `deep`; the eBPF crate is not a default
  member of the workspace)
- With the eBPF probes: `scripts/build-deep.sh [--clippy] [triple…]` builds `--features deep`
  (and with `--clippy` also lints it, as CI does) inside a rust
  container, so macOS needs no nightly, bpf-linker or LLVM. On Linux with the toolchain (as in
  CI): `cargo build --release --features deep --target <triple>`.
  - The nightly is pinned in `build.rs` (`EBPF_TOOLCHAIN`), and CI reads it from there.
  - bpf-linker is pinned by version and per-arch SHA-256 in `scripts/install-bpf-linker.sh`.
  - Dependabot updates neither pin, so bump both by hand in a reviewed PR.
- Static Linux binary: `cargo build --release --target aarch64-unknown-linux-musl`
  (or `x86_64-unknown-linux-musl`). This links with Rust's bundled `rust-lld` via
  `.cargo/config.toml`, so no cross toolchain is needed, even on macOS.
- Release binaries for both architectures plus `SHA256SUMS`: `scripts/build-release.sh` (output in `dist/`)
- End-to-end in minimal containers (alpine, debian-slim, busybox):
  `scripts/verify.sh [--deep] [target]`. `--deep` adds a privileged run whose probes must not be
  SKIPPED.
  `x86_64-unknown-linux-musl` runs under amd64 emulation on arm64 hosts.
