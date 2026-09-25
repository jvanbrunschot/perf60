#!/bin/sh
# Integrate one parallel feature branch (usually from an agent worktree) onto the current branch.
# Usage: scripts/integrate-worktree.sh <worktree-path>
#
# Run from the main checkout with the integration branch checked out (e.g. phase/a-counters).
# 1. the feature branch must hold exactly one commit on top of the integration branch's history
# 2. rebase it onto the integration branch; conflicts are only allowed in the registries and are
#    resolved by scripts/resolve-registry.py
# 3. run every gate in the worktree, then fast-forward the integration branch and remove the
#    worktree and its branch
set -eu
export OPENSPEC_TELEMETRY=0
repo=$(git rev-parse --show-toplevel)
base=$(git -C "$repo" symbolic-ref --short HEAD)
wt=$(cd "${1:?usage: integrate-worktree.sh <worktree-path>}" && pwd)
br=$(git -C "$wt" symbolic-ref --short HEAD)
openspec_bin="$repo/tools/openspec/node_modules/.bin/openspec"
[ -x "$openspec_bin" ] || openspec_bin=openspec

cd "$wt"
count=$(git rev-list --count "$(git merge-base "$base" "$br")..$br")
[ "$count" = 1 ] || { echo "error: $br must have exactly 1 commit, has $count"; git log --oneline "$base..$br"; exit 1; }

if ! git rebase "$base" >/dev/null 2>&1; then
  conflicted=$(git diff --name-only --diff-filter=U)
  for f in $conflicted; do
    case "$f" in
      src/checks/mod.rs|src/procfs/mod.rs) ;;
      *) echo "error: unexpected conflict in $f (resolve by hand, then rerun)"; exit 1 ;;
    esac
  done
  # shellcheck disable=SC2086
  python3 "$repo/scripts/resolve-registry.py" $conflicted
  cargo fmt
  # shellcheck disable=SC2086
  git add $conflicted
  GIT_EDITOR=true git rebase --continue >/dev/null
fi

cargo fmt --check
cargo clippy -q --locked --all-targets -- -D warnings
cargo test -q --locked 2>&1 | grep -E "test result|FAILED|panicked"
"$openspec_bin" validate --all --strict 2>&1 | tail -1

cd "$repo"
git merge --ff-only -q "$br"
git worktree remove "$wt"
git branch -D "$br" >/dev/null
git log --oneline -1
