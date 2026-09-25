//! "Likely bottleneck": correlate WARN/CRIT findings across sections by the resource they are
//! about, so one root cause that shows up in several sections (a CPU quota in cpu, pressure,
//! cgroup and cgroups-top) is named once.

use serde::Serialize;

use crate::check::{Level, Resource, Section, Status};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagnosis {
    /// Human label, e.g. `CPU quota throttling (cgroup)`.
    pub bottleneck: String,
    pub resource: Resource,
    pub status: Status,
    /// Up to 3 strongest findings for that resource, as `<section>: <short message>`.
    pub evidence: Vec<String>,
    /// Other resources with problems, as labels (most severe first, at most 3).
    pub also: Vec<String>,
}

/// One WARN/CRIT finding, attributed to a resource.
struct Item<'a> {
    resource: Resource,
    level: Level,
    section: &'static str,
    message: &'a str,
}

const MAX_EVIDENCE: usize = 3;
const MAX_ALSO: usize = 3;
const EVIDENCE_CHARS: usize = 72;

pub fn diagnose(sections: &[Section]) -> Option<Diagnosis> {
    let items: Vec<Item> = sections
        .iter()
        .filter(|s| s.status != Status::Skipped)
        .flat_map(|s| {
            s.findings
                .iter()
                .filter(|f| f.level != Level::Note)
                .map(move |f| Item {
                    resource: s.resource_of(f),
                    level: f.level,
                    section: s.id,
                    message: &f.message,
                })
        })
        .collect();
    if items.is_empty() {
        return None;
    }

    let mut groups: Vec<(Resource, Vec<&Item>)> = Vec::new();
    for it in &items {
        match groups.iter_mut().find(|(r, _)| *r == it.resource) {
            Some((_, v)) => v.push(it),
            None => groups.push((it.resource, vec![it])),
        }
    }
    groups.sort_by_key(|(r, v)| std::cmp::Reverse(score(*r, v)));

    let (resource, top) = &groups[0];
    let mut ranked: Vec<&&Item> = top.iter().collect();
    // CRIT first, then the order sections appear in the report.
    ranked.sort_by_key(|it| it.level != Level::Crit);
    let mut evidence = Vec::new();
    let mut used = Vec::new();
    for it in &ranked {
        if evidence.len() == MAX_EVIDENCE {
            break;
        }
        if !used.contains(&it.section) {
            used.push(it.section);
            evidence.push(format!("{}: {}", it.section, short(it.message)));
        }
    }
    Some(Diagnosis {
        bottleneck: label(*resource, top),
        resource: *resource,
        status: if top.iter().any(|i| i.level == Level::Crit) {
            Status::Crit
        } else {
            Status::Warn
        },
        evidence,
        also: groups[1..]
            .iter()
            .take(MAX_ALSO)
            .map(|(r, v)| label(*r, v))
            .collect(),
    })
}

/// Rank: any CRIT, number of sections agreeing, CRIT count, WARN count. Kernel log events
/// corroborate other resources, so a pure-kernel group loses ties.
fn score(r: Resource, v: &[&Item]) -> (bool, usize, usize, usize, bool) {
    let crit = v.iter().filter(|i| i.level == Level::Crit).count();
    let mut sections: Vec<&str> = v.iter().map(|i| i.section).collect();
    sections.dedup();
    (
        crit > 0,
        sections.len(),
        crit,
        v.len() - crit,
        r != Resource::Kernel,
    )
}

fn from(v: &[&Item], section: &str) -> bool {
    v.iter().any(|i| i.section == section)
}

fn label(r: Resource, v: &[&Item]) -> String {
    match r {
        Resource::Cpu if from(v, "cgroup") || from(v, "cgroups-top") => {
            "CPU quota throttling (cgroup)".into()
        }
        Resource::Cpu if v.iter().all(|i| i.section == "cpu-balance") => {
            "single hot CPU (single-threaded or IRQ bottleneck)".into()
        }
        Resource::Cpu => "CPU saturation".into(),
        Resource::Memory if from(v, "cgroup") => "memory limit (cgroup)".into(),
        Resource::Memory
            if from(v, "swap") || v.iter().any(|i| i.message.contains("thrashing")) =>
        {
            "memory pressure (swapping or thrashing)".into()
        }
        Resource::Memory => "memory".into(),
        Resource::Disk => "disk I/O".into(),
        Resource::Network if from(v, "sockets") => "network capacity (sockets/conntrack)".into(),
        Resource::Network => "network".into(),
        Resource::Capacity => {
            let mut s: Vec<&str> = v.iter().map(|i| i.section).collect();
            s.dedup();
            format!("capacity limits ({})", s.join(", "))
        }
        Resource::Hardware => "hardware fault".into(),
        Resource::Kernel => "kernel errors (see kernel-log)".into(),
        Resource::Pressure => "resource pressure".into(),
    }
}

/// A finding reads `what: why`, sometimes `what: how much: why`. Keep the shortest leading
/// part that contains a number (the evidence), then cap the length.
fn short(message: &str) -> String {
    let mut end = message.len();
    let mut pos = 0;
    for part in message.split(": ") {
        pos += part.len();
        if message[..pos].chars().any(|c| c.is_ascii_digit()) {
            end = pos;
            break;
        }
        pos += 2;
    }
    let head = &message[..end.min(message.len())];
    if head.chars().count() <= EVIDENCE_CHARS {
        head.to_owned()
    } else {
        let cut: String = head.chars().take(EVIDENCE_CHARS - 1).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sec(id: &'static str, r: Resource) -> Section {
        Section::new(id, "T", "t", r)
    }

    #[test]
    fn nothing_wrong_means_no_diagnosis() {
        let mut s = sec("cpu", Resource::Cpu);
        s.note("fyi");
        assert_eq!(diagnose(&[s, sec("disk", Resource::Disk)]), None);
    }

    #[test]
    fn cgroup_throttling_beats_host_cpu() {
        let mut cpu = sec("cpu", Resource::Cpu);
        cpu.crit("run queue r=6.0 exceeds 2 cpus: CPU saturation, runnable tasks wait");
        let mut pressure = sec("pressure", Resource::Pressure);
        pressure.crit_on(
            Resource::Cpu,
            "cpu some pressure 76.7% of the window: runnable tasks waiting for CPU",
        );
        let mut cgroup = sec("cgroup", Resource::Cpu);
        cgroup.crit("CPU quota throttling: 100.0% of periods throttled (1609 ms/s): raise cpu.max");
        let mut fs = sec("filesystems", Resource::Capacity);
        fs.warn("/fill is 95% full (1 MiB free): writes fail with ENOSPC");
        let d = diagnose(&[cpu, pressure, cgroup, fs]).unwrap();
        assert_eq!(d.bottleneck, "CPU quota throttling (cgroup)");
        assert_eq!(d.resource, Resource::Cpu);
        assert_eq!(d.status, Status::Crit);
        assert_eq!(
            d.evidence,
            [
                "cpu: run queue r=6.0 exceeds 2 cpus",
                "pressure: cpu some pressure 76.7% of the window",
                "cgroup: CPU quota throttling: 100.0% of periods throttled (1609 ms/s)",
            ]
        );
        assert_eq!(d.also, ["capacity limits (filesystems)"]);
    }

    #[test]
    fn iowait_and_io_pressure_point_at_disk() {
        let mut cpu = sec("cpu", Resource::Cpu);
        cpu.warn_on(
            Resource::Disk,
            "iowait 34%: I/O bound, CPUs sit idle waiting on I/O",
        );
        let mut disk = sec("disk", Resource::Disk);
        disk.crit("vda saturated: %util 97.0% (peak 100%)");
        let mut pressure = sec("pressure", Resource::Pressure);
        pressure.warn_on(
            Resource::Disk,
            "io some pressure 18.0% of the window: tasks stalled",
        );
        let d = diagnose(&[cpu, disk, pressure]).unwrap();
        assert_eq!(d.bottleneck, "disk I/O");
        assert_eq!(
            d.evidence[0],
            "disk: vda saturated: %util 97.0% (peak 100%)"
        );
        assert!(d.also.is_empty());
    }

    #[test]
    fn more_agreeing_sections_win_over_a_lone_warning() {
        let mut net = sec("net", Resource::Network);
        net.warn("eth0 errors/drops during the window: 12 drops");
        let mut mem = sec("memory", Resource::Memory);
        mem.warn("page cache thrashing: working set does not fit in memory");
        let mut swap = sec("swap", Resource::Memory);
        swap.warn("system is swapping: memory pressure");
        let d = diagnose(&[net, mem, swap]).unwrap();
        assert_eq!(d.bottleneck, "memory pressure (swapping or thrashing)");
        assert_eq!(d.also, ["network"]);
    }

    #[test]
    fn kernel_only_group_loses_ties() {
        let mut k = sec("kernel-log", Resource::Kernel);
        k.warn("segfault: 1 message in the last hour");
        let mut hw = sec("hardware", Resource::Hardware);
        hw.warn("CPU thermal throttling on cpu0");
        let d = diagnose(&[k, hw]).unwrap();
        assert_eq!(d.bottleneck, "hardware fault");
        assert_eq!(d.also, ["kernel errors (see kernel-log)"]);
    }

    #[test]
    fn oom_in_kernel_log_counts_as_memory() {
        let mut k = sec("kernel-log", Resource::Kernel);
        k.crit_on(
            Resource::Memory,
            "OOM kill: 6 messages in the last hour, last 1m ago",
        );
        let d = diagnose(&[k]).unwrap();
        assert_eq!(d.resource, Resource::Memory);
        assert_eq!(d.bottleneck, "memory");
    }

    #[test]
    fn skipped_sections_are_ignored() {
        let mut s = sec("disk", Resource::Disk).skipped("gone");
        s.findings.push(crate::check::Finding {
            level: Level::Crit,
            message: "x".into(),
            resource: None,
        });
        assert_eq!(diagnose(&[s]), None);
    }

    #[test]
    fn short_keeps_the_what_and_caps_length() {
        assert_eq!(short("iowait 34%: I/O bound, CPUs idle"), "iowait 34%");
        assert_eq!(
            short("CPU quota throttling: 100% of periods: raise cpu.max"),
            "CPU quota throttling: 100% of periods"
        );
        assert_eq!(
            short("vda saturated: %util 97% (peak 100%)"),
            "vda saturated: %util 97% (peak 100%)"
        );
        assert_eq!(short("no numbers here: at all"), "no numbers here: at all");
        assert!(short(&"x".repeat(200)).ends_with('…'));
        assert_eq!(short(&"x".repeat(200)).chars().count(), EVIDENCE_CHARS);
    }
}
