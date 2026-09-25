# Tasks

## 1. Framework

- [x] 1.1 `Resource` enum and `Section::new(.., resource)`; update every check and test; verify `cargo test` and a JSON test asserting `resource`
- [x] 1.2 `Source::statvfs` and `Source::clock_status` (live, fixture files, MemSource setters) with pure parsers; verify unit tests for parsers, MemSource and the FsSource fixture mode

## 2. Fixtures

- [x] 2.1 Extend `scripts/capture-fixture.sh` (new files, fd dirs, limits, statvfs.txt, adjtimex.txt) and recapture `tests/fixtures/linux-arm64`; verify all existing tests still pass
- [x] 2.2 Add synthetic `tests/fixtures/linux-legacy` with README; run the fixture test over both trees with expected skips; verify `cargo test --test fixture_tree`

## 3. Tooling and docs

- [x] 3.1 `scripts/resolve-registry.py` (order incl. the new ids) and `scripts/integrate-worktree.sh <branch> <base>`; verify the resolver on a synthetic conflict
- [x] 3.2 CLAUDE.md: new Source methods, fixture trees and expected skips, integration scripts; verify the commands shown run
