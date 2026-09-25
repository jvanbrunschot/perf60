# Tasks

## 1. Release workflow

- [x] 1.1 Publish step: upload with `--clobber` to an existing release, else create it; verify with actionlint and by running the step with a stubbed `gh` for: no release, UI-created release, pre-release tag

## 2. README

- [x] 2.1 Quick start: `chmod +x` before `scp`, plus `gh attestation verify`; verify by running the documented steps against the v0.1.0 release and executing the binary in a busybox container
