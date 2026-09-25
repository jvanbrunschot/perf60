# Spec Delta

## Purpose

Describes the host that perf60 is analysing, so the report can be read in context and
thresholds can scale to the resources that are actually available.

## ADDED Requirements

### Requirement: System spec collection
The tool SHALL collect the following, where available: hostname, kernel release, distribution
pretty name (`/etc/os-release`), CPU model, online CPU count, total memory, total swap, uptime,
virtualization hint, cgroup CPU and memory limits, block devices (name, size, rotational) and
network interfaces (name, operational state, speed). An item that cannot be read SHALL be omitted.
It SHALL never cause a failure.

#### Scenario: Header line
- **WHEN** the report is printed
- **THEN** the first line shows hostname, kernel, distro, CPU count, memory and the sampling window

#### Scenario: Unreadable item
- **WHEN** `/etc/os-release` is missing
- **THEN** the distro is omitted and the rest of the header is shown

### Requirement: Effective CPU capacity
The tool SHALL compute effective CPUs as the online CPU count, lowered to the cgroup CPU quota
(quota ÷ period, cgroup v2 `cpu.max` or v1 `cpu.cfs_quota_us`/`cpu.cfs_period_us`) when a
quota is set. Checks that compare against CPU count SHALL use effective CPUs.

#### Scenario: Container with CPU quota
- **WHEN** 4 CPUs are online and `cpu.max` is `150000 100000`
- **THEN** effective CPUs is 1.5 and the header shows the cgroup limit

#### Scenario: No quota
- **WHEN** `cpu.max` is `max 100000`
- **THEN** effective CPUs equals the online CPU count

### Requirement: Virtualization hint
The tool SHALL report a virtualization hint from the DMI product/vendor names (KVM, QEMU, VMware,
VirtualBox, Xen, Hyper-V, Amazon EC2, Google), the `hypervisor` cpuinfo flag, or container
markers (`/.dockerenv`, `/run/.containerenv`).

#### Scenario: Container detected
- **WHEN** `/run/.containerenv` or `/.dockerenv` exists
- **THEN** the header marks the system as a container
