# ci-release Specification

## Purpose
Keeps every change to perf60 verified on real Linux hosts of both supported architectures, and
turns a version tag into published static release binaries without manual steps.

## Requirements

### Requirement: Pull request checks
Every pull request SHALL run these checks, and each SHALL pass for the pull request to be green:
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`,
`openspec validate --all --strict`, and `scripts/verify.sh <target>` for both
`x86_64-unknown-linux-musl` (on an x86_64 runner) and `aarch64-unknown-linux-musl` (on an arm64
runner). A new push to the same pull request SHALL cancel the run in progress. The spec
validation SHALL run in its own job with no token permissions, no persisted git credentials and
no cache. It SHALL use the OpenSpec CLI from the committed lockfile in `tools/openspec/`,
installed without running package lifecycle scripts.

#### Scenario: Failing test
- **WHEN** a pull request introduces a failing unit test
- **THEN** the CI workflow fails and the pull request check is red

#### Scenario: Both architectures verified
- **WHEN** a pull request is opened
- **THEN** the static binary is built and run in minimal containers on both an x86_64 and an arm64 runner

#### Scenario: npm install scripts do not run
- **WHEN** a package in the OpenSpec dependency tree declares a `preinstall` or `postinstall` script
- **THEN** CI installs the tree without executing it, in a job whose token has no permissions

### Requirement: Tag-triggered release
Pushing a tag matching `v*` SHALL build and publish a GitHub release named after the tag. It
SHALL contain `perf60-<version>-x86_64-linux-musl`, `perf60-<version>-aarch64-linux-musl` and
`SHA256SUMS`, with generated release notes. The release SHALL only be published after all
pull request checks pass for the tagged commit. When a release for the tag already exists (for
example created in the GitHub UI, which also creates the tag), the workflow SHALL attach the
files to that release and keep its title, notes and pre-release flag. Existing files with the
same names SHALL be replaced, so re-running the release job succeeds.

#### Scenario: Release on tag
- **WHEN** tag `v0.2.0` is pushed and `Cargo.toml` has version `0.2.0`
- **THEN** a GitHub release `v0.2.0` is published with both binaries and `SHA256SUMS`

#### Scenario: Pre-release tag
- **WHEN** tag `v0.2.0-rc.1` is pushed and `Cargo.toml` has version `0.2.0-rc.1`
- **THEN** the GitHub release is marked as a pre-release

#### Scenario: Failing checks block the release
- **WHEN** a tag is pushed on a commit whose tests fail
- **THEN** no release is published

#### Scenario: Release created in the GitHub UI
- **WHEN** a release `v0.2.0` with hand-written notes is created in the GitHub UI, creating the tag
- **THEN** the workflow attaches both binaries and `SHA256SUMS` to it, and its notes and title are unchanged

#### Scenario: Re-run release job
- **WHEN** the release job is re-run after a release with the files already exists
- **THEN** the files are replaced and the job succeeds

### Requirement: Tag matches crate version
The release workflow SHALL fail before building when the tag is not `v` followed by the
`Cargo.toml` package version, so binary names and `perf60 --version` always match the release.

#### Scenario: Mismatched tag
- **WHEN** tag `v0.3.0` is pushed while `Cargo.toml` has version `0.2.0`
- **THEN** the release workflow fails with an error naming both versions, and nothing is published

### Requirement: Pinned build dependencies
Every third-party GitHub Action SHALL be referenced by a full 40-character commit SHA. No
checkout SHALL persist git credentials. Every cargo build and test in CI and in the release and
verify scripts SHALL use `--locked`. Automated dependency update proposals (actions, npm tooling,
cargo) SHALL wait at least 7 days after an upstream release before being opened.

#### Scenario: Action pinned by SHA
- **WHEN** a workflow references an action outside this repository
- **THEN** the reference is `owner/repo@<40-hex-sha>` with the version in a comment

#### Scenario: Lockfile drift
- **WHEN** `Cargo.lock` does not match `Cargo.toml`
- **THEN** the CI build fails instead of resolving new dependency versions

### Requirement: Release provenance
Each published release binary SHALL have a signed build provenance attestation linking it to
the release workflow run and commit, verifiable with
`gh attestation verify <file> --repo jvanbrunschot/perf60`.

#### Scenario: Verify a downloaded binary
- **WHEN** a user runs `gh attestation verify perf60-0.2.0-x86_64-linux-musl --repo jvanbrunschot/perf60`
- **THEN** verification succeeds and names the release workflow as the builder
