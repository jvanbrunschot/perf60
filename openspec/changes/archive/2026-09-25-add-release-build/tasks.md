# Tasks

## 1. Build and verify scripts

- [x] 1.1 Add `scripts/build-release.sh` (both musl targets, static check, `SHA256SUMS`) and ignore `/dist`; verify by running it and `shasum -a 256 -c SHA256SUMS` in `dist/`
- [x] 1.2 Make `scripts/verify.sh` pick the container platform from the target and accept static-pie; verify `scripts/verify.sh x86_64-unknown-linux-musl` and `scripts/verify.sh aarch64-unknown-linux-musl` both print `verify: PASS`

## 2. Documentation

- [x] 2.1 Write `README.md` (quick start, section-to-command mapping, exit codes, privileges, building); verify every command in it runs as written
