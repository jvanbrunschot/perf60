# Proposal

## Why

The point of perf60 is to copy one file onto any Linux server, including minimal installs, and
run it. That needs reproducible static binaries for both common server architectures, checksums
to verify the copy, and a README that explains how to read the report.

## What Changes

- `scripts/build-release.sh` builds static musl binaries for x86_64 and aarch64 into `dist/`,
  refuses to publish a binary that isn't statically linked, and writes `SHA256SUMS`.
- `scripts/verify.sh <target>` runs the binary in containers of the matching platform, so the
  x86_64 build is also verified (under emulation on arm64 hosts). It accepts static-pie
  binaries.
- `README.md`: purpose, quick start, how each section maps to the article's commands, exit codes,
  privileges, and building.

## Capabilities

### New Capabilities
- `release-build`: producing and verifying the static release binaries.

### Modified Capabilities

## Impact

New `scripts/build-release.sh`, `README.md`; edits to `scripts/verify.sh` and `.gitignore`
(`/dist`). No code changes.
