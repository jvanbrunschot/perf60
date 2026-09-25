//! Small parsers used by the system spec header.

/// `/etc/os-release` → `PRETTY_NAME` (falls back to `NAME VERSION_ID`).
pub fn os_release_pretty_name(input: &str) -> Option<String> {
    let kv: Vec<(&str, String)> = input
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| {
            (
                k.trim(),
                v.trim().trim_matches('"').trim_matches('\'').to_owned(),
            )
        })
        .collect();
    let get = |key: &str| kv.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone());
    get("PRETTY_NAME").filter(|s| !s.is_empty()).or_else(|| {
        match (get("NAME"), get("VERSION_ID")) {
            (Some(n), Some(v)) => Some(format!("{n} {v}")),
            (Some(n), None) => Some(n),
            _ => None,
        }
    })
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CpuInfo {
    pub model: Option<String>,
    pub processors: usize,
    /// x86 `hypervisor` flag.
    pub hypervisor_flag: bool,
}

/// `/proc/cpuinfo`: model name (x86), or a best effort description on ARM.
pub fn cpuinfo(input: &str) -> CpuInfo {
    let kv = super::key_values(input, ':');
    let first = |key: &str| kv.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
    let processors = kv.iter().filter(|(k, _)| *k == "processor").count();
    let hypervisor_flag = kv
        .iter()
        .any(|(k, v)| *k == "flags" && v.split_whitespace().any(|f| f == "hypervisor"));
    let model = first("model name")
        .or_else(|| first("Model"))
        .or_else(|| first("Hardware"))
        .or_else(|| first("cpu model"))
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .or_else(|| {
            let implementer = first("CPU implementer")?;
            let part = first("CPU part").unwrap_or("?");
            let vendor = match implementer {
                "0x41" => "ARM",
                "0x42" => "Broadcom",
                "0x43" => "Cavium",
                "0x46" => "Fujitsu",
                "0x48" => "HiSilicon",
                "0x4e" => "NVIDIA",
                "0x50" => "APM",
                "0x51" => "Qualcomm",
                "0x61" => "Apple",
                "0xc0" => "Ampere",
                other => other,
            };
            let core = match (implementer, part) {
                ("0x41", "0xd0c") => " Neoverse-N1",
                ("0x41", "0xd40") => " Neoverse-V1",
                ("0x41", "0xd49") => " Neoverse-N2",
                ("0x41", "0xd4f") => " Neoverse-V2",
                ("0x41", "0xd08") => " Cortex-A72",
                ("0x41", "0xd0b") => " Cortex-A76",
                _ => "",
            };
            Some(if core.is_empty() {
                format!("{vendor} (part {part})")
            } else {
                format!("{vendor}{core}")
            })
        });
    CpuInfo {
        model,
        processors,
        hypervisor_flag,
    }
}

/// CPU list format used in `/sys/devices/system/cpu/online`: `0-3,5,7-8` → 7.
pub fn cpu_list_count(input: &str) -> Option<usize> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let mut n = 0;
    for part in input.split(',') {
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b): (usize, usize) = (a.parse().ok()?, b.parse().ok()?);
                n += b.checked_sub(a)? + 1;
            }
            None => {
                part.parse::<usize>().ok()?;
                n += 1;
            }
        }
    }
    Some(n)
}

/// `/proc/uptime`: `14937.42 59262.64` → seconds.
pub fn uptime_secs(input: &str) -> Option<f64> {
    input.split_whitespace().next()?.parse().ok()
}

/// cgroup v2 `cpu.max`: `150000 100000` → 1.5 CPUs; `max 100000` → None.
pub fn cgroup2_cpu_max(input: &str) -> Option<f64> {
    let mut f = input.split_whitespace();
    let quota: f64 = f.next()?.parse().ok()?;
    let period: f64 = f.next().unwrap_or("100000").parse().ok()?;
    (quota > 0.0 && period > 0.0).then(|| quota / period)
}

/// cgroup v1 `cpu.cfs_quota_us` / `cpu.cfs_period_us`: quota `-1` means unlimited.
pub fn cgroup1_cpu_quota(quota: &str, period: &str) -> Option<f64> {
    let q: f64 = quota.trim().parse().ok()?;
    let p: f64 = period.trim().parse().ok()?;
    (q > 0.0 && p > 0.0).then(|| q / p)
}

/// cgroup memory limit (`memory.max` or `memory.limit_in_bytes`); `max` means unlimited.
/// v1 reports "unlimited" as a huge page-aligned number, so the caller should ignore limits at
/// or above physical memory.
pub fn cgroup_mem_limit(input: &str) -> Option<u64> {
    input.trim().parse().ok()
}

/// `/proc/self/cgroup` → the cgroup v2 path (the `0::<path>` line).
pub fn cgroup2_path(input: &str) -> Option<String> {
    input
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .map(|p| p.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release() {
        let s = include_str!("../../tests/fixtures/linux-arm64/etc/os-release");
        assert_eq!(
            os_release_pretty_name(s).as_deref(),
            Some("Alpine Linux v3.24")
        );
        assert_eq!(
            os_release_pretty_name("NAME=Foo\nVERSION_ID='1.2'\n").as_deref(),
            Some("Foo 1.2")
        );
        assert_eq!(os_release_pretty_name(""), None);
    }

    #[test]
    fn cpuinfo_x86() {
        let s = "processor\t: 0\nmodel name\t: Intel(R) Xeon(R)   CPU @ 2.20GHz\nflags\t\t: fpu hypervisor sse\n\nprocessor\t: 1\nmodel name\t: Intel(R) Xeon(R) CPU @ 2.20GHz\n";
        let c = cpuinfo(s);
        assert_eq!(c.model.as_deref(), Some("Intel(R) Xeon(R) CPU @ 2.20GHz"));
        assert_eq!(c.processors, 2);
        assert!(c.hypervisor_flag);
    }

    #[test]
    fn cpuinfo_arm_fixture() {
        let c = cpuinfo(include_str!(
            "../../tests/fixtures/linux-arm64/proc/cpuinfo"
        ));
        assert!(c.processors >= 1);
        assert!(c.model.is_some());
        assert!(!c.hypervisor_flag);
        let n1 = cpuinfo("processor\t: 0\nCPU implementer\t: 0x41\nCPU part\t: 0xd0c\n");
        assert_eq!(n1.model.as_deref(), Some("ARM Neoverse-N1"));
    }

    #[test]
    fn cpu_list() {
        assert_eq!(cpu_list_count("0-3\n"), Some(4));
        assert_eq!(cpu_list_count("0-3,5,7-8"), Some(7));
        assert_eq!(cpu_list_count("0"), Some(1));
        assert_eq!(cpu_list_count(""), None);
        assert_eq!(cpu_list_count("3-1"), None);
    }

    #[test]
    fn cgroup_limits() {
        assert_eq!(cgroup2_cpu_max("150000 100000\n"), Some(1.5));
        assert_eq!(cgroup2_cpu_max("max 100000\n"), None);
        assert_eq!(cgroup1_cpu_quota("-1", "100000"), None);
        assert_eq!(cgroup1_cpu_quota("200000\n", "100000\n"), Some(2.0));
        assert_eq!(cgroup_mem_limit("536870912\n"), Some(536870912));
        assert_eq!(cgroup_mem_limit("max\n"), None);
        assert_eq!(
            cgroup2_path("0::/user.slice/x.scope\n").as_deref(),
            Some("/user.slice/x.scope")
        );
        assert_eq!(cgroup2_path("12:cpu:/docker/abc\n"), None);
    }

    #[test]
    fn uptime() {
        assert_eq!(uptime_secs("14937.42 59262.64\n"), Some(14937.42));
        assert_eq!(uptime_secs(""), None);
    }
}
