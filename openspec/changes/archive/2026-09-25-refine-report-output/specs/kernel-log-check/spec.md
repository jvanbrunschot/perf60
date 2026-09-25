# Spec Delta

## MODIFIED Requirements

### Requirement: Kernel log skip
When the kernel log cannot be read, the kernel-log check SHALL be SKIPPED. On a permission
error the reason SHALL be `permission denied reading /dev/kmsg (run as root or set
kernel.dmesg_restrict=0)`. When `/dev/kmsg` does not exist, the reason SHALL be
`/dev/kmsg not available (run on the host, or use --privileged in a container)`.

#### Scenario: Permission denied
- **WHEN** reading the kernel log fails with a permission error
- **THEN** the section is SKIPPED with reason `permission denied reading /dev/kmsg (run as root
  or set kernel.dmesg_restrict=0)`

#### Scenario: Missing kernel log
- **WHEN** `/dev/kmsg` does not exist
- **THEN** the section is SKIPPED with reason `/dev/kmsg not available (run on the host, or use
  --privileged in a container)`
