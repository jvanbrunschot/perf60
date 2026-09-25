# Spec Delta

## MODIFIED Requirements

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

## ADDED Requirements

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
