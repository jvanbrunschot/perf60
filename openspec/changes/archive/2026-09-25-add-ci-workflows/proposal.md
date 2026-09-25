# Proposal

## Why

The project now has a GitHub remote. Until now the quality gates (fmt, clippy, tests, spec
validation, container verification) only ran by hand on a developer machine, and release
binaries were built locally. Pull requests should prove that they keep every gate green on
real Linux hosts of both architectures. Tagging a version should publish the static binaries
without manual steps.

## What Changes

- `.github/workflows/ci.yml`: on every pull request (and callable from other workflows) it runs
  fmt, clippy, cargo test and `openspec validate --all --strict`. It also runs
  `scripts/verify.sh` natively on an x86_64 runner and an arm64 runner.
- `.github/workflows/release.yml`: on a pushed `v*` tag it checks the tag matches the crate
  version, runs the full CI workflow, builds both static binaries with
  `scripts/build-release.sh`, and publishes a GitHub release with the binaries and
  `SHA256SUMS`. Tags with a pre-release suffix (e.g. `v0.2.0-rc.1`) publish a pre-release.
- CLAUDE.md work method: features go through a pull request on `feat/<change-id>` and are
  squash-merged, so `main` still gets exactly one commit per feature.

## Capabilities

### New Capabilities
- `ci-release`: continuous integration on pull requests and tag-triggered releases.

### Modified Capabilities

## Impact

New `.github/workflows/`, CLAUDE.md and README updates. No code changes. Uses GitHub-hosted
`ubuntu-latest` and `ubuntu-24.04-arm` runners (free for public repositories).
