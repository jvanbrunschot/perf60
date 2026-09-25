# Proposal

## Why

The first real release (v0.1.0) showed two gaps:
- **UI-created releases fail.** Creating a release in the GitHub UI also creates the tag,
  which triggers the release workflow. The workflow then failed at the very end with
  "a release with the same tag name already exists", after CI, the build and the attestation
  had all succeeded. Re-running the job fails the same way.
- **README quick start is broken.** Release downloads don't keep the executable bit, so the
  documented `curl` + `scp` + run steps end in "permission denied".

## What Changes

- The release workflow attaches the binaries and `SHA256SUMS` to an existing release for the
  tag, keeping its title, notes and pre-release flag. It only creates the release when none
  exists. Re-running a release job replaces the assets instead of failing.
- The README quick start makes the binary executable before copying it, and shows how to verify
  the provenance attestation.

## Capabilities

### New Capabilities

### Modified Capabilities
- `ci-release`: the tag-triggered release also covers releases created in the GitHub UI and
  re-runs.

## Impact

`.github/workflows/release.yml` (publish step), `README.md`.
