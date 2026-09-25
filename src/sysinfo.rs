//! System spec header: what machine are we looking at?

use serde::Serialize;

use crate::procfs::{meminfo, system};
use crate::source::Source;

#[derive(Debug, Clone, Default, Serialize)]
pub struct BlockDevice {
    pub name: String,
    pub size_bytes: u64,
    pub rotational: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct NetInterface {
    pub name: String,
    pub state: Option<String>,
    pub speed_mbps: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SysInfo {
    pub hostname: Option<String>,
    pub kernel: Option<String>,
    pub distro: Option<String>,
    /// Kernel architecture (see `kernel_arch`).
    pub arch: String,
    /// Architecture perf60 was built for; differs from `arch` under emulation.
    pub binary_arch: String,
    pub cpu_model: Option<String>,
    pub cpus_online: usize,
    pub cgroup_cpu_limit: Option<f64>,
    pub mem_total_bytes: Option<u64>,
    pub swap_total_bytes: Option<u64>,
    pub cgroup_mem_limit_bytes: Option<u64>,
    pub uptime_secs: Option<f64>,
    pub virtualization: Option<String>,
    pub container: bool,
    pub block_devices: Vec<BlockDevice>,
    pub interfaces: Vec<NetInterface>,
}

impl SysInfo {
    /// Online CPUs, lowered to the cgroup CPU quota when one is set. Never below 1 CPU's worth
    /// of a tiny quota (keeps thresholds meaningful), never zero.
    pub fn effective_cpus(&self) -> f64 {
        let online = self.cpus_online.max(1) as f64;
        match self.cgroup_cpu_limit {
            Some(q) if q < online => q.max(0.01),
            _ => online,
        }
    }

    /// Memory capacity to compare against: physical memory, lowered to the cgroup limit.
    pub fn effective_mem_bytes(&self) -> Option<u64> {
        match (self.mem_total_bytes, self.cgroup_mem_limit_bytes) {
            (Some(t), Some(l)) => Some(t.min(l)),
            (t, l) => t.or(l),
        }
    }
}

fn trimmed(src: &dyn Source, path: &str) -> Option<String> {
    src.read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

pub fn collect(src: &dyn Source) -> SysInfo {
    let cpu = src
        .read_to_string("/proc/cpuinfo")
        .map(|s| system::cpuinfo(&s))
        .unwrap_or_default();
    let cpus_online = src
        .read_to_string("/sys/devices/system/cpu/online")
        .ok()
        .and_then(|s| system::cpu_list_count(&s))
        .unwrap_or(cpu.processors);
    let mem = src
        .read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| meminfo::parse(&s).ok());
    let mem_total_bytes = mem.as_ref().and_then(|m| m.get("MemTotal"));

    let (cgroup_cpu_limit, cgroup_mem_limit_bytes) = cgroup_limits(src, mem_total_bytes);

    SysInfo {
        hostname: trimmed(src, "/proc/sys/kernel/hostname"),
        kernel: trimmed(src, "/proc/sys/kernel/osrelease"),
        distro: src
            .read_to_string("/etc/os-release")
            .ok()
            .and_then(|s| system::os_release_pretty_name(&s)),
        arch: kernel_arch(src),
        binary_arch: std::env::consts::ARCH.to_owned(),
        cpu_model: cpu.model,
        cpus_online,
        cgroup_cpu_limit,
        mem_total_bytes,
        swap_total_bytes: mem.as_ref().and_then(|m| m.get("SwapTotal")),
        cgroup_mem_limit_bytes,
        uptime_secs: src
            .read_to_string("/proc/uptime")
            .ok()
            .and_then(|s| system::uptime_secs(&s)),
        virtualization: virtualization(src, cpu.hypervisor_flag),
        container: container(src),
        block_devices: block_devices(src),
        interfaces: interfaces(src),
    }
}

/// `/proc/sys/kernel/arch`, else the kernel release suffix, else the build architecture.
fn kernel_arch(src: &dyn Source) -> String {
    trimmed(src, "/proc/sys/kernel/arch")
        .or_else(|| {
            trimmed(src, "/proc/sys/kernel/osrelease")
                .and_then(|r| system::release_arch(&r).map(str::to_owned))
        })
        .unwrap_or_else(|| std::env::consts::ARCH.to_owned())
}

/// Walk from our own cgroup up to the root and take the tightest limits (v2), else try v1.
fn cgroup_limits(src: &dyn Source, mem_total: Option<u64>) -> (Option<f64>, Option<u64>) {
    let mut cpu: Option<f64> = None;
    let mut mem: Option<u64> = None;
    let min_f = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let min_u = |a: Option<u64>, b: Option<u64>| match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };

    if src.exists("/sys/fs/cgroup/cgroup.controllers") || src.exists("/sys/fs/cgroup/cpu.max") {
        let own = src
            .read_to_string("/proc/self/cgroup")
            .ok()
            .and_then(|s| system::cgroup2_path(&s))
            .unwrap_or_else(|| "/".to_owned());
        let mut path = own.trim_end_matches('/').to_owned();
        loop {
            let dir = format!("/sys/fs/cgroup{path}");
            cpu = min_f(
                cpu,
                src.read_to_string(&format!("{dir}/cpu.max"))
                    .ok()
                    .and_then(|s| system::cgroup2_cpu_max(&s)),
            );
            mem = min_u(
                mem,
                src.read_to_string(&format!("{dir}/memory.max"))
                    .ok()
                    .and_then(|s| system::cgroup_mem_limit(&s)),
            );
            match path.rfind('/') {
                Some(i) => path.truncate(i),
                None => break,
            }
        }
    } else {
        for base in ["/sys/fs/cgroup/cpu,cpuacct", "/sys/fs/cgroup/cpu"] {
            if let (Ok(q), Ok(p)) = (
                src.read_to_string(&format!("{base}/cpu.cfs_quota_us")),
                src.read_to_string(&format!("{base}/cpu.cfs_period_us")),
            ) {
                cpu = system::cgroup1_cpu_quota(&q, &p);
                break;
            }
        }
        mem = src
            .read_to_string("/sys/fs/cgroup/memory/memory.limit_in_bytes")
            .ok()
            .and_then(|s| system::cgroup_mem_limit(&s));
    }

    // v1 "unlimited" is a huge number; any limit at or above RAM is no limit at all.
    if let (Some(l), Some(t)) = (mem, mem_total)
        && l >= t
    {
        mem = None;
    }
    (cpu, mem)
}

fn virtualization(src: &dyn Source, hypervisor_flag: bool) -> Option<String> {
    let product = trimmed(src, "/sys/class/dmi/id/product_name").unwrap_or_default();
    let vendor = trimmed(src, "/sys/class/dmi/id/sys_vendor").unwrap_or_default();
    let both = format!("{vendor} {product}").to_lowercase();
    let known = [
        ("amazon ec2", "Amazon EC2"),
        ("google", "Google Compute Engine"),
        ("kvm", "KVM"),
        ("qemu", "QEMU"),
        ("vmware", "VMware"),
        ("virtualbox", "VirtualBox"),
        ("xen", "Xen"),
        ("microsoft corporation virtual", "Hyper-V"),
        ("openstack", "OpenStack"),
        ("libkrun", "libkrun"),
        ("apple virtualization", "Apple Virtualization"),
        ("parallels", "Parallels"),
    ];
    if let Some((_, name)) = known.iter().find(|(needle, _)| both.contains(needle)) {
        return Some((*name).to_owned());
    }
    if src.exists("/proc/xen") {
        return Some("Xen".to_owned());
    }
    hypervisor_flag.then(|| "virtual machine".to_owned())
}

fn container(src: &dyn Source) -> bool {
    if src.exists("/.dockerenv") || src.exists("/run/.containerenv") {
        return true;
    }
    src.read_to_string("/proc/1/cgroup")
        .map(|s| {
            ["docker", "kubepods", "containerd", "lxc", "libpod"]
                .iter()
                .any(|m| s.contains(m))
        })
        .unwrap_or(false)
}

fn block_devices(src: &dyn Source) -> Vec<BlockDevice> {
    let Ok(names) = src.read_dir("/sys/block") else {
        return Vec::new();
    };
    names
        .into_iter()
        .filter(|n| {
            !["loop", "ram", "fd", "sr", "zram"]
                .iter()
                .any(|p| n.starts_with(p))
        })
        .filter_map(|name| {
            let sectors: u64 = trimmed(src, &format!("/sys/block/{name}/size"))?
                .parse()
                .ok()?;
            (sectors > 0).then(|| BlockDevice {
                rotational: trimmed(src, &format!("/sys/block/{name}/queue/rotational"))
                    .map(|r| r == "1"),
                size_bytes: sectors * 512,
                name,
            })
        })
        .collect()
}

fn interfaces(src: &dyn Source) -> Vec<NetInterface> {
    let Ok(names) = src.read_dir("/sys/class/net") else {
        return Vec::new();
    };
    names
        .into_iter()
        .filter(|n| n != "lo" && !n.starts_with("bonding_masters"))
        .map(|name| NetInterface {
            state: trimmed(src, &format!("/sys/class/net/{name}/operstate")),
            // Virtual NICs return -1 or fail with EINVAL; either way it's unknown.
            speed_mbps: trimmed(src, &format!("/sys/class/net/{name}/speed"))
                .and_then(|s| s.parse::<i64>().ok())
                .filter(|s| *s > 0)
                .map(|s| s as u64),
            name,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{FsSource, MemSource};

    fn fixture() -> FsSource {
        FsSource::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/linux-arm64"
        ))
    }

    #[test]
    fn collects_fixture_tree() {
        let s = collect(&fixture());
        assert!(s.hostname.is_some());
        assert!(s.kernel.as_deref().unwrap().contains("aarch64"));
        assert_eq!(s.distro.as_deref(), Some("Alpine Linux v3.24"));
        assert_eq!(s.cpus_online, 4);
        assert!(s.mem_total_bytes.unwrap() > 0);
        assert!(s.uptime_secs.unwrap() > 0.0);
        assert_eq!(s.virtualization.as_deref(), Some("libkrun"));
        assert!(s.container);
        assert_eq!(s.block_devices[0].name, "vda");
        assert!(
            s.interfaces
                .iter()
                .any(|i| i.name == "eth0" && i.speed_mbps == Some(10000))
        );
        assert!(!s.interfaces.iter().any(|i| i.name == "lo"));
        assert_eq!(s.effective_cpus(), 4.0);
    }

    #[test]
    fn cgroup_v2_quota_lowers_effective_cpus() {
        let src = MemSource::new()
            .with("/sys/devices/system/cpu/online", "0-3\n")
            .with("/proc/meminfo", "MemTotal: 2000000 kB\n")
            .with("/proc/self/cgroup", "0::/\n")
            .with("/sys/fs/cgroup/cpu.max", "150000 100000\n")
            .with("/sys/fs/cgroup/memory.max", "536870912\n");
        let s = collect(&src);
        assert_eq!(s.cgroup_cpu_limit, Some(1.5));
        assert_eq!(s.effective_cpus(), 1.5);
        assert_eq!(s.cgroup_mem_limit_bytes, Some(536870912));
        assert_eq!(s.effective_mem_bytes(), Some(536870912));
    }

    #[test]
    fn cgroup_v2_nested_takes_tightest_limit() {
        let src = MemSource::new()
            .with("/sys/devices/system/cpu/online", "0-7\n")
            .with("/proc/self/cgroup", "0::/a/b\n")
            .with("/sys/fs/cgroup/cgroup.controllers", "cpu memory\n")
            .with("/sys/fs/cgroup/a/cpu.max", "200000 100000\n")
            .with("/sys/fs/cgroup/a/b/cpu.max", "max 100000\n");
        assert_eq!(collect(&src).effective_cpus(), 2.0);
    }

    #[test]
    fn no_quota_uses_online_cpus() {
        let src = MemSource::new()
            .with("/sys/devices/system/cpu/online", "0-3\n")
            .with("/sys/fs/cgroup/cpu.max", "max 100000\n");
        assert_eq!(collect(&src).effective_cpus(), 4.0);
    }

    #[test]
    fn cgroup_v1_unlimited_memory_is_ignored() {
        let src = MemSource::new()
            .with("/proc/meminfo", "MemTotal: 1000 kB\n")
            .with("/sys/fs/cgroup/cpu/cpu.cfs_quota_us", "-1\n")
            .with("/sys/fs/cgroup/cpu/cpu.cfs_period_us", "100000\n")
            .with(
                "/sys/fs/cgroup/memory/memory.limit_in_bytes",
                "9223372036854771712\n",
            );
        let s = collect(&src);
        assert_eq!(s.cgroup_cpu_limit, None);
        assert_eq!(s.cgroup_mem_limit_bytes, None);
    }

    #[test]
    fn arch_lookup_order() {
        let from_file = MemSource::new()
            .with("/proc/sys/kernel/arch", "aarch64\n")
            .with("/proc/sys/kernel/osrelease", "3.10.0-1160.el7.x86_64\n");
        assert_eq!(collect(&from_file).arch, "aarch64");
        let from_release =
            MemSource::new().with("/proc/sys/kernel/osrelease", "3.10.0-1160.el7.x86_64\n");
        assert_eq!(collect(&from_release).arch, "x86_64");
        let s = collect(&MemSource::new());
        assert_eq!(s.arch, std::env::consts::ARCH);
        assert_eq!(s.binary_arch, std::env::consts::ARCH);
    }

    #[test]
    fn missing_items_are_omitted() {
        let s = collect(&MemSource::new());
        assert!(s.distro.is_none() && s.hostname.is_none());
        assert_eq!(s.effective_cpus(), 1.0);
        assert!(!s.container);
    }

    #[test]
    fn container_markers() {
        assert!(collect(&MemSource::new().with("/.dockerenv", "")).container);
        assert!(collect(&MemSource::new().with("/proc/1/cgroup", "0::/kubepods/x")).container);
    }
}
