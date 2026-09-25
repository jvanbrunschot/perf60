# Spec Delta

## MODIFIED Requirements

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
