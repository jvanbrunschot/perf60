//! cgroup interface files: `/proc/self/cgroup`, `cpu.stat` (v1 and v2), `memory.events` and
//! single-value files such as `memory.current` or `memory.max`.

use super::{Result, err, vmstat};

/// One `/proc/self/cgroup` line: `<hierarchy-id>:<controllers>:<path>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CgroupLine {
    pub id: u32,
    /// Comma-separated controller list as written, e.g. `cpu,cpuacct`; empty for v2.
    pub controllers: String,
    pub path: String,
}

impl CgroupLine {
    pub fn has(&self, controller: &str) -> bool {
        self.controllers.split(',').any(|c| c == controller)
    }
}

pub fn parse_proc_cgroup(input: &str) -> Result<Vec<CgroupLine>> {
    let lines: Vec<CgroupLine> = input
        .lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, ':');
            let id = f.next()?.trim().parse().ok()?;
            let controllers = f.next()?.to_owned();
            let path = f.next()?.trim().to_owned();
            path.starts_with('/').then_some(CgroupLine {
                id,
                controllers,
                path,
            })
        })
        .collect();
    if lines.is_empty() {
        return err("cgroup: no hierarchy lines");
    }
    Ok(lines)
}

/// The own cgroup, as seen by the controllers that matter for CPU and memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnCgroup {
    /// Unified hierarchy path (`0::<path>`).
    V2(String),
    /// v1: the `cpu`/`cpuacct` line (controller list, path) and the `memory` line's path.
    V1 {
        cpu: Option<(String, String)>,
        memory: Option<String>,
    },
}

/// v1 wins when it has a cpu, cpuacct or memory line: in hybrid mode those controllers are bound
/// to v1 and the `0::` line only carries the systemd hierarchy.
pub fn own_cgroup(lines: &[CgroupLine]) -> Option<OwnCgroup> {
    let cpu = lines
        .iter()
        .find(|l| l.has("cpu"))
        .or_else(|| lines.iter().find(|l| l.has("cpuacct")))
        .map(|l| (l.controllers.clone(), l.path.clone()));
    let memory = lines
        .iter()
        .find(|l| l.has("memory"))
        .map(|l| l.path.clone());
    if cpu.is_some() || memory.is_some() {
        return Some(OwnCgroup::V1 { cpu, memory });
    }
    lines
        .iter()
        .find(|l| l.id == 0 && l.controllers.is_empty())
        .map(|l| OwnCgroup::V2(l.path.clone()))
}

/// `cpu.stat`. v2 has `usage_usec` and `throttled_usec`; v1 has no usage (that is in
/// `cpuacct.usage`) and `throttled_time` in ns, normalized here to µs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuStat {
    pub usage_usec: Option<u64>,
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_usec: u64,
}

pub fn parse_cpu_stat(input: &str) -> Result<CpuStat> {
    let Ok(v) = vmstat::parse(input) else {
        return err("cpu.stat: no counters");
    };
    let usage_usec = v.get("usage_usec");
    let periods = v.get("nr_periods");
    if usage_usec.is_none() && periods.is_none() {
        return err("cpu.stat: no usage_usec or nr_periods");
    }
    Ok(CpuStat {
        usage_usec,
        nr_periods: periods.unwrap_or(0),
        nr_throttled: v.get("nr_throttled").unwrap_or(0),
        throttled_usec: v
            .get("throttled_usec")
            .or_else(|| v.get("throttled_time").map(|ns| ns / 1000))
            .unwrap_or(0),
    })
}

/// cgroup v2 `memory.events`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryEvents {
    pub low: u64,
    pub high: u64,
    pub max: u64,
    pub oom: u64,
    pub oom_kill: u64,
}

pub fn parse_memory_events(input: &str) -> Result<MemoryEvents> {
    let Ok(v) = vmstat::parse(input) else {
        return err("memory.events: no counters");
    };
    if ["low", "high", "max", "oom", "oom_kill"]
        .iter()
        .all(|k| v.get(k).is_none())
    {
        return err("memory.events: no known events");
    }
    let g = |k: &str| v.get(k).unwrap_or(0);
    Ok(MemoryEvents {
        low: g("low"),
        high: g("high"),
        max: g("max"),
        oom: g("oom"),
        oom_kill: g("oom_kill"),
    })
}

/// A single integer, e.g. `memory.current`, `memory.failcnt`, `cpuacct.usage`.
pub fn parse_value(input: &str) -> Result<u64> {
    match input.trim().parse() {
        Ok(v) => Ok(v),
        Err(_) => err(format!("expected a number, got {:?}", input.trim())),
    }
}

/// A limit such as `memory.max` or `memory.high`: `max` means no limit (None).
pub fn parse_limit(input: &str) -> Result<Option<u64>> {
    if input.trim() == "max" {
        return Ok(None);
    }
    parse_value(input).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_self_cgroup() {
        let legacy = parse_proc_cgroup(include_str!(
            "../../tests/fixtures/linux-legacy/proc/self/cgroup"
        ))
        .unwrap();
        assert_eq!(legacy.len(), 5);
        assert_eq!(legacy[2].id, 4);
        assert_eq!(legacy[2].controllers, "cpu,cpuacct");
        assert!(legacy[2].has("cpuacct") && !legacy[2].has("cpuset"));
        assert_eq!(legacy[4].controllers, "name=systemd");
        let arm = parse_proc_cgroup(include_str!(
            "../../tests/fixtures/linux-arm64/proc/self/cgroup"
        ))
        .unwrap();
        assert_eq!(
            arm,
            vec![CgroupLine {
                id: 0,
                controllers: String::new(),
                path: "/".into()
            }]
        );
        // Paths may contain colons.
        assert_eq!(parse_proc_cgroup("0::/a:b\n").unwrap()[0].path, "/a:b");
    }

    #[test]
    fn own_cgroup_versions() {
        let own = |s: &str| own_cgroup(&parse_proc_cgroup(s).unwrap());
        assert_eq!(own("0::/app\n"), Some(OwnCgroup::V2("/app".into())));
        assert_eq!(
            own(include_str!(
                "../../tests/fixtures/linux-legacy/proc/self/cgroup"
            )),
            Some(OwnCgroup::V1 {
                cpu: Some(("cpu,cpuacct".into(), "/system.slice/app.service".into())),
                memory: Some("/system.slice/app.service".into()),
            })
        );
        // Hybrid: v1 controllers plus the unified systemd line.
        assert!(matches!(
            own("4:cpu,cpuacct:/app\n1:name=systemd:/app\n0::/app\n"),
            Some(OwnCgroup::V1 { .. })
        ));
        assert_eq!(own("1:name=systemd:/app\n"), None);
    }

    #[test]
    fn parses_v2_cpu_stat_fixture() {
        let c = parse_cpu_stat(include_str!(
            "../../tests/fixtures/linux-arm64/sys/fs/cgroup/cpu.stat"
        ))
        .unwrap();
        assert_eq!(
            c,
            CpuStat {
                usage_usec: Some(42532),
                nr_periods: 0,
                nr_throttled: 0,
                throttled_usec: 0
            }
        );
    }

    #[test]
    fn parses_v1_cpu_stat_fixture() {
        let c = parse_cpu_stat(include_str!(
            "../../tests/fixtures/linux-legacy/sys/fs/cgroup/cpu,cpuacct/system.slice/app.service/cpu.stat"
        ))
        .unwrap();
        assert_eq!(
            c,
            CpuStat {
                usage_usec: None,
                nr_periods: 864000,
                nr_throttled: 43200,
                throttled_usec: 912345678
            }
        );
    }

    #[test]
    fn parses_memory_events() {
        let e = parse_memory_events(include_str!(
            "../../tests/fixtures/linux-arm64/sys/fs/cgroup/memory.events"
        ))
        .unwrap();
        assert_eq!(e, MemoryEvents::default());
        let e = parse_memory_events("low 0\nhigh 3\nmax 2\noom 1\noom_kill 1\n").unwrap();
        assert_eq!(
            e,
            MemoryEvents {
                low: 0,
                high: 3,
                max: 2,
                oom: 1,
                oom_kill: 1
            }
        );
    }

    #[test]
    fn single_values() {
        assert_eq!(parse_value("3440640\n"), Ok(3440640));
        assert_eq!(parse_limit("max\n"), Ok(None));
        assert_eq!(parse_limit("536870912\n"), Ok(Some(536870912)));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_proc_cgroup("").is_err());
        assert!(parse_proc_cgroup("garbage\n").is_err());
        assert!(parse_cpu_stat("").is_err());
        assert!(parse_cpu_stat("nr_bursts 0\n").is_err());
        assert!(parse_memory_events("sock_throttled 0\n").is_err());
        assert!(parse_memory_events("x y\n").is_err());
        assert!(parse_value("max").is_err());
        assert!(parse_limit("lots").is_err());
    }
}
