# Spec Delta

## Purpose

Keeps every change to perf60 verified on real Linux hosts of both supported architectures, and
turns a version tag into published static release binaries without manual steps.

## ADDED Requirements

### Requirement: Pull request checks
Every pull request SHALL run these checks, and each SHALL pass for the pull request to be green:
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`,
`openspec validate --all --strict`, and `scripts/verify.sh <target>` for both
`x86_64-unknown-linux-musl` (on an x86_64 runner) and `aarch64-unknown-linux-musl` (on an arm64
runner). A new push to the same pull request SHALL cancel the run in progress.

#### Scenario: Failing test
- **WHEN** a pull request introduces a failing unit test
- **THEN** the CI workflow fails and the pull request check is red

#### Scenario: Both architectures verified
- **WHEN** a pull request is opened
- **THEN** the static binary is built and run in minimal containers on both an x86_64 and an arm64 runner

### Requirement: Tag-triggered release
Pushing a tag matching `v*` SHALL build and publish a GitHub release named after the tag. It
SHALL contain `perf60-<version>-x86_64-linux-musl`, `perf60-<version>-aarch64-linux-musl` and
`SHA256SUMS`, with generated release notes. The release SHALL only be published after all
pull request checks pass for the tagged commit.

#### Scenario: Release on tag
- **WHEN** tag `v0.2.0` is pushed and `Cargo.toml` has version `0.2.0`
- **THEN** a GitHub release `v0.2.0` is published with both binaries and `SHA256SUMS`

#### Scenario: Pre-release tag
- **WHEN** tag `v0.2.0-rc.1` is pushed and `Cargo.toml` has version `0.2.0-rc.1`
- **THEN** the GitHub release is marked as a pre-release

#### Scenario: Failing checks block the release
- **WHEN** a tag is pushed on a commit whose tests fail
- **THEN** no release is published

### Requirement: Tag matches crate version
The release workflow SHALL fail before building when the tag is not `v` followed by the
`Cargo.toml` package version, so binary names and `perf60 --version` always match the release.

#### Scenario: Mismatched tag
- **WHEN** tag `v0.3.0` is pushed while `Cargo.toml` has version `0.2.0`
- **THEN** the release workflow fails with an error naming both versions, and nothing is published
