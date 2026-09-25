---
name: release
description: Cut a perf60 release end to end. Pick the semver bump from the commits since the last tag, open a release PR (version bump in the three Cargo manifests, Cargo.lock, README quick start, CHANGELOG), rebase-merge it once CI is green, push the vX.Y.Z tag, watch the release workflow, and verify the published assets (checksums, provenance attestation, a real run including privileged --deep). Use when the user says "release", "/release", "cut a release", "publish vX.Y.Z" or "tag a new version". Args: optional version (e.g. 0.3.0 or 0.3.0-rc.1) and/or "publish" to skip the final confirmation before tagging.
---

# /release: cut a perf60 release

A release is a `v<version>` tag on `main`. `.github/workflows/release.yml` does the rest:
1. checks that the tag equals `v<Cargo.toml version>`
2. reruns the full CI
3. builds both static musl binaries with `PERF60_FEATURES=deep`
4. attests their provenance
5. publishes a GitHub release with the binaries and `SHA256SUMS`

A tag with a `-` suffix (e.g. `v0.3.0-rc.1`) becomes a pre-release. A release already created
in the GitHub UI gets the assets attached instead.

`main` is protected: rebase-only PRs, green CI required, and only admins may create `v*`
tags. Follow CLAUDE.md ("Pull requests and CI", "Releases"). Never push to `main` directly.

## 0. Preflight (stop and report on any failure)

```sh
git fetch --tags --prune
git switch main && git pull --ff-only
git status --porcelain            # must be empty
last=$(git describe --tags --abbrev=0 --match 'v*' 2>/dev/null)   # e.g. v0.2.0
git log --oneline "$last"..HEAD   # must not be empty
gh run list --branch main --limit 1 --json conclusion --jq '.[0].conclusion'   # "success"
```

## 1. Choose the version

- If the user gave one, use it. It must be semver, greater than `$last`, with no leading `v`.
- Otherwise derive it from the conventional-commit prefixes in `git log "$last"..HEAD`:
  - any `!` or `BREAKING CHANGE` → **major** (while < 1.0.0: minor)
  - any `feat` → **minor**
  - only `fix`/`build`/`docs`/`chore`/`ci` → **patch**
- State the version and the one-line reason, e.g. "0.3.0: 2 feats since v0.2.0".

## 2. Release PR

Branch `chore/release-<version>` from `main`, then:

1. **Versions:** set `version = "<version>"` in `Cargo.toml`, `perf60-common/Cargo.toml` and
   `perf60-ebpf/Cargo.toml`. Keep `publish = false` in all three. Refresh the lockfile with
   `cargo build -q` (without `--locked` this once), then confirm `cargo run -q -- --version`
   prints `perf60 <version>`.
2. **README:** update the quick start's `v=<old>` line and the example header's
   `perf60 <old> ·` to the new version.
3. **CHANGELOG.md:** add `## <version> — <YYYY-MM-DD>` above the previous entry. Group the
   commits since `$last` under **Added** / **Changed** / **Fixed** / **Build and security**.
   Write for users, one line per change, and name the section, flag or behavior. Don't paste
   commit subjects verbatim. The archived OpenSpec proposals
   (`openspec/changes/archive/*/proposal.md` newer than `$last`) are the best source for
   the "why".
4. **Gates:** `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings &&
   cargo test --locked`, and `openspec validate --all --strict`.
5. **Commit:** one commit, `chore(release): <version>` + a body + the Co-Authored-By trailer.
   Push, and open the PR with `gh pr create --title "chore(release): <version>"`. Put the
   CHANGELOG entry and "after merge: push tag v<version>" in the body.
6. **CI:** watch the checks in the background:
   `gh pr checks <n> --watch --interval 30; gh pr view <n> --json mergeStateStatus`. Fix any
   red check on the same branch; never skip or override it.

## 3. Merge and tag

- **Confirm before merging and tagging.** The tag publishes a public, permanent release. Skip
  this only if the invocation said "publish" or the user already approved this release in the
  conversation. Otherwise ask once: "Merge #<n> and push v<version>?"
- `gh pr merge <n> --rebase` (squash is disabled). Then `git switch main && git pull --ff-only`.
- Tag the merged commit, confirming it's the release commit:
  `git log -1 --format=%s` must be `chore(release): <version>`. Then
  `git tag -a v<version> -m "perf60 <version>" && git push origin v<version>`.

## 4. Watch the release workflow

```sh
run=$(gh run list --workflow release.yml --branch v<version> --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch "$run" --interval 20 --exit-status
```

Run it in the background and wait for the notification. On failure, get the failing step with
`gh run view "$run" --log-failed | tail -40`:
- **Tag/version mismatch:** delete the tag (`git push origin :refs/tags/v<version>`,
  `git tag -d v<version>`), fix the version with a new PR, and tag again.
- **Build or CI failure:** fix it via a PR, then delete and re-push the tag on the new release
  commit. If a release object was already created, the workflow attaches to it on the rerun.
- **Publish step only** (for example a release created in the UI, which the workflow handles
  now): `gh run rerun "$run" --failed`.

## 5. Verify the published release

In the session scratchpad:

```sh
gh release view v<version> --json url,isPrerelease,assets --jq '.url, (.assets[] | "\(.name) \(.size)")'
gh release download v<version> --repo jvanbrunschot/perf60
shasum -a 256 -c SHA256SUMS
for a in x86_64 aarch64; do gh attestation verify perf60-<version>-$a-linux-musl --repo jvanbrunschot/perf60 >/dev/null && echo "$a attested"; done
chmod +x perf60-<version>-*-linux-musl
```

Then run the host-architecture binary in containers. Bind mounts don't work here, so use
`docker create` + `docker cp` + `docker start` and read the output from `docker logs`:
- `busybox /perf60 --version` must print `perf60 <version>`.
- `busybox /perf60 -c 1 -i 0.2 --no-color` must exit ≤ 2 with an `OVERALL:` line.
- `--privileged alpine /perf60 --deep -c 1 -i 0.5 --no-color`: none of `execsnoop`,
  `runqlat`, `biolatency` and `tcpretrans` may be `[SKIP]`.

## 6. Report

Give the release URL, the version and why, the assets with sizes, and the verification results
(checksums, both attestations, `--version`, the normal run, the `--deep` run). Name anything
that needed fixing along the way.
