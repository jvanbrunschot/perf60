# Spec Delta

## Purpose

Produces the single-file static binaries that are copied to servers, and verifies they run on
minimal Linux systems of each supported architecture.

## ADDED Requirements

### Requirement: Static release binaries
The release build SHALL produce `dist/perf60-<version>-x86_64-linux-musl` and
`dist/perf60-<version>-aarch64-linux-musl`, where `<version>` is the crate version. Each binary
SHALL be statically linked (static or static-pie) with no dynamic loader dependency. The build
SHALL fail if either binary is dynamically linked. A `dist/SHA256SUMS` file SHALL list the
SHA-256 checksum of every binary.

#### Scenario: Release build
- **WHEN** `scripts/build-release.sh` is run on a host with the Rust toolchain
- **THEN** both binaries and `SHA256SUMS` exist in `dist/` and `shasum -a 256 -c SHA256SUMS` succeeds

#### Scenario: Dynamic binary rejected
- **WHEN** a built binary is dynamically linked
- **THEN** the release build exits non-zero and names the binary

### Requirement: Cross-architecture verification
`scripts/verify.sh <target>` SHALL run the binary for `<target>` in containers of the matching
platform (`linux/amd64` for x86_64, `linux/arm64` for aarch64). It SHALL fail when any run exits
with a code above 2, when the text report has no `OVERALL:` line, or when the JSON output does
not parse.

#### Scenario: Verify x86_64 on an arm64 host
- **WHEN** `scripts/verify.sh x86_64-unknown-linux-musl` runs on an arm64 host with amd64 emulation
- **THEN** the x86_64 binary runs in amd64 alpine, debian-slim and busybox containers and the script prints `verify: PASS`
