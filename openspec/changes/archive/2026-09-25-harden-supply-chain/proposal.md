# Proposal

## Why

Worms like Shai-Hulud spread through npm install scripts. They steal whatever tokens the build
environment exposes, then persist by pushing workflows or tags with the stolen credentials.
The CI added in `add-ci-workflows` installs the OpenSpec CLI from npm without a lockfile, while
a GitHub token sits in `.git/config`. It references third-party actions by mutable tags and
branches, and the repository has no protection on `main`, release tags or workflow changes.

## What Changes

- The OpenSpec CLI used in CI is pinned with a committed lockfile (`tools/openspec/`). It is
  installed with `npm ci --ignore-scripts` in an isolated job that has no token permissions,
  no persisted credentials and no cache.
- Every third-party action is pinned to a full commit SHA. Dependabot proposes updates for
  actions, the npm tooling and cargo only after a 7-day cooldown.
- No checkout persists credentials. All cargo builds use `--locked`.
- Release binaries get a signed build provenance attestation (`gh attestation verify`).
- Repository settings (applied outside the code, documented in CLAUDE.md):
  - a `main` ruleset (PR plus green CI required, no force-push or deletion)
  - a `v*` tag ruleset (only admins create, move or delete release tags)
  - an allow-list of actions, with full-SHA pinning required
  - approval required before workflows run for outside contributors

## Capabilities

### New Capabilities

### Modified Capabilities
- `ci-release`: the pull request checks run the spec validation in an isolated, locked,
  script-free job; new requirements for pinned dependencies and release provenance.

## Impact

`.github/workflows/*.yml`, new `.github/dependabot.yml`, new `tools/openspec/` (package.json +
package-lock.json), `scripts/verify.sh` and `scripts/build-release.sh` (`--locked`), CLAUDE.md,
README. GitHub repository settings via the API.
