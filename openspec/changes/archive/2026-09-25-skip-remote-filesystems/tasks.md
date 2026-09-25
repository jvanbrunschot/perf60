# Tasks

## 1. Filesystem selection

- [x] 1.1 Exclude network/FUSE types from statvfs and record them; verify test `network_filesystems_are_never_statted` (MemSource without statvfs entries for them; a statvfs call would fail the test) and `fuseblk_is_checked`
- [x] 1.2 Detail line listing the skipped mounts; verify the same test asserts the detail text
