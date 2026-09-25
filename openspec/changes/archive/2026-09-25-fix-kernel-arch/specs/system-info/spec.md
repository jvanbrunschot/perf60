# Spec Delta

## ADDED Requirements

### Requirement: Kernel architecture
The header SHALL show the kernel's architecture. It SHALL be read from `/proc/sys/kernel/arch`.
When that file is missing, it SHALL be the known architecture suffix of the kernel release
(`x86_64`, `aarch64`, `ppc64le`, `s390x`, `i686`, `armv7l`, `riscv64`). Only when neither is
available SHALL the binary's build architecture be used. When the build architecture differs
from the kernel architecture, the header SHALL add `(binary <arch>, emulated)`.

#### Scenario: Kernel arch file
- **WHEN** `/proc/sys/kernel/arch` contains `aarch64`
- **THEN** the header shows `aarch64`

#### Scenario: Release suffix fallback
- **WHEN** `/proc/sys/kernel/arch` is missing and the kernel release is `3.10.0-1160.el7.x86_64`
- **THEN** the header shows `x86_64`

#### Scenario: Emulated binary
- **WHEN** the kernel architecture is `aarch64` and the binary was built for `x86_64`
- **THEN** the header shows `aarch64 (binary x86_64, emulated)`
