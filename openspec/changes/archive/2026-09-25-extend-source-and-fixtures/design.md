# Design

## Context

See proposal.md. Five feature changes will be built in parallel worktrees on top of this one,
so everything shared must be settled here.

## Decisions

- **`Section::new(id, title, equivalent, resource)`.** Resource is a required constructor
  argument rather than a builder method, so no check can forget it. A single enum covers the
  diagnosis needs. `pressure` is its own resource because PSI spans cpu, memory and io; the
  diagnosis reads its findings per resource.
- **System calls behind `Source`.**
  - `statvfs(path) -> FsStat`: frsize, blocks, bfree, bavail, files, ffree, favail, read-only
    flag.
  - `clock_status() -> ClockStatus`: adjtimex `state`, `status` bits, offset, maxerror,
    esterror.

  How each source provides them:
  - Live: `libc::statvfs` and `libc::adjtimex` with `modes = 0`, which is read-only and needs no
    privilege.
  - Fixture trees: two plain-text files at the tree root, `statvfs.txt` (one line per mount
    point) and `adjtimex.txt` (`key value` lines). They are parsed by pure functions in
    `source.rs`.
  - `MemSource`: `set_statvfs` and `set_clock`.

  Alternative considered: `statvfs` directly in the check. Rejected because it can't be tested
  off-Linux or against fixtures.
- **Fixture fd directories.** For per-process fd counts the capture creates empty files named
  after each fd under `proc/<pid>/fd/`. `read_dir` then behaves the same on fixtures and live.
- **Expected skips per fixture.** `tests/fixture_tree.rs` lists, per tree, the sections that
  must be SKIPPED (e.g. `pressure` on linux-legacy). Every other section must render and not
  be skipped. A new check that needs data the fixture doesn't have must either add it to the
  fixture or add itself to the expected-skip list, with a comment.
- **linux-legacy is synthetic.** It is hand-assembled from documented old formats, not captured,
  because this machine can't boot old kernels. Its README lists what each file mimics.

## Risks / Trade-offs

- [Recapturing linux-arm64 changes values that existing tests assert on] → Tests assert on
  shape, not exact values. Any that break get fixed in this change.
- [statvfs.txt drift from real `statvfs` semantics] → The capture writes it from busybox
  `stat -f`, which calls statvfs, so it is real data.
