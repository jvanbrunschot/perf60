# Tasks

## 1. Workflows

- [x] 1.1 Add `.github/workflows/ci.yml` (pull_request + workflow_call; lint/test/spec job, verify matrix on ubuntu-latest and ubuntu-24.04-arm; concurrency cancels superseded PR runs); verify with actionlint
- [x] 1.2 Add `.github/workflows/release.yml` (v* tags; version check → reusable CI → build-release.sh → `gh release create`, pre-release for suffixed tags); verify with actionlint and by checking the version-check step locally against a matching and a mismatched tag

## 2. Documentation

- [x] 2.1 Update CLAUDE.md work method (feature branch → PR → CI green → squash-merge) and README (CI badge, releasing); verify the documented release commands are correct

## 3. Integration

- [x] 3.1 Verify the gates pass in a Linux container (as on the runners): fmt, clippy, cargo test, both musl builds
