# Tasks

## 1. Locked, isolated tooling

- [x] 1.1 Add `tools/openspec/package.json` + `package-lock.json` (70 packages, all registry.npmjs.org with integrity, none with install scripts) and ignore its node_modules; verify `npm ci --ignore-scripts --prefix tools/openspec` then `tools/openspec/node_modules/.bin/openspec validate --all --strict` passes
- [x] 1.2 CI: move spec validation into a `specs` job with `permissions: {}`, `persist-credentials: false` and no cache; verify with actionlint
- [x] 1.3 Add `--locked` to cargo in CI, `scripts/verify.sh` and `scripts/build-release.sh`; verify both scripts still pass locally

## 2. Pinning and updates

- [x] 2.1 Pin all actions to full commit SHAs with version comments and set `persist-credentials: false` on every checkout; verify `grep -E 'uses: [^.].*@' .github/workflows/*.yml` shows only 40-hex refs
- [x] 2.2 Add `.github/dependabot.yml` (github-actions, npm in tools/openspec, cargo; 7-day cooldown); verify it parses

## 3. Release provenance

- [x] 3.1 Attest the release binaries with actions/attest-build-provenance (id-token + attestations write, release job only); verify with actionlint

## 4. Repository settings and docs

- [x] 4.1 Apply via `gh api`: main ruleset, v* tag ruleset, actions allow-list with SHA pinning required, fork PR approval for all external contributors; verify by reading each setting back
- [x] 4.2 Document the protections and the release verification in CLAUDE.md and README; verify the documented commands
