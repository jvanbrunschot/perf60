use std::fmt::Write;

use super::Report;
use crate::check::{Level, Status};
use crate::units;

struct Paint(bool);

impl Paint {
    fn wrap(&self, code: &str, s: &str) -> String {
        if self.0 {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_owned()
        }
    }
    fn status(&self, st: Status) -> String {
        let code = match st {
            Status::Ok => "32",
            Status::Warn => "33;1",
            Status::Crit => "31;1",
            Status::Skipped => "90",
        };
        self.wrap(code, st.label())
    }
    fn bold(&self, s: &str) -> String {
        self.wrap("1", s)
    }
    fn dim(&self, s: &str) -> String {
        self.wrap("90", s)
    }
}

/// Render the text report. Detail lines are shown for non-OK sections, or all when `verbose`.
pub fn render(r: &Report, color: bool, verbose: bool) -> String {
    let p = Paint(color);
    let mut out = String::new();
    let sys = &r.system;

    let mut head = vec![format!("perf60 {}", r.version)];
    head.extend(sys.hostname.clone());
    head.extend(sys.kernel.as_ref().map(|k| format!("Linux {k}")));
    head.extend(sys.distro.clone());
    head.push(sys.arch.clone());
    head.push(format!(
        "{} cpus",
        units::cpus(sys.cpus_online.max(1) as f64)
    ));
    head.extend(
        sys.mem_total_bytes
            .map(|m| format!("{} RAM", units::bytes(m))),
    );
    let mut env = Vec::new();
    env.extend(sys.virtualization.clone());
    if sys.container {
        env.push("container".to_owned());
    }
    if !env.is_empty() {
        head.push(env.join(", "));
    }
    head.extend(
        sys.uptime_secs
            .map(|u| format!("up {}", units::duration(u))),
    );
    head.push(format!(
        "sampled {}×{}s",
        r.sampling.count,
        trim_float(r.sampling.interval)
    ));
    let _ = writeln!(out, "{}", p.bold(&head.join(" · ")));

    if let Some(m) = &sys.cpu_model {
        let _ = writeln!(out, "  {}  {m}", p.dim("cpu   "));
    }
    let mut limits = Vec::new();
    if let Some(c) = sys.cgroup_cpu_limit {
        limits.push(format!("cpu {}", units::cpus(c)));
    }
    if let Some(m) = sys.cgroup_mem_limit_bytes {
        limits.push(format!("memory {}", units::bytes(m)));
    }
    if !limits.is_empty() {
        let _ = writeln!(out, "  {}  cgroup {}", p.dim("limits"), limits.join(", "));
    }
    if let Some(s) = sys.swap_total_bytes {
        let swap = if s == 0 {
            "none".to_owned()
        } else {
            units::bytes(s)
        };
        let _ = writeln!(out, "  {}  {swap}", p.dim("swap  "));
    }
    if !sys.block_devices.is_empty() {
        let disks: Vec<String> = sys
            .block_devices
            .iter()
            .map(|d| {
                let kind = match d.rotational {
                    Some(true) => " hdd",
                    Some(false) => " ssd",
                    None => "",
                };
                format!("{} {}{kind}", d.name, units::bytes(d.size_bytes))
            })
            .collect();
        let _ = writeln!(out, "  {}  {}", p.dim("disks "), disks.join(", "));
    }
    if !sys.interfaces.is_empty() {
        let nics: Vec<String> = sys
            .interfaces
            .iter()
            .map(|n| {
                let mut s = n.name.clone();
                if let Some(st) = &n.state {
                    s.push(' ');
                    s.push_str(st);
                }
                if let Some(sp) = n.speed_mbps {
                    s.push_str(&format!(" {sp}Mb/s"));
                }
                s
            })
            .collect();
        let _ = writeln!(out, "  {}  {}", p.dim("nics  "), nics.join(", "));
    }

    let mut counts = Vec::new();
    for (st, name) in [
        (Status::Crit, "critical"),
        (Status::Warn, "warning"),
        (Status::Skipped, "skipped"),
    ] {
        let n = r.count(st);
        if n > 0 {
            counts.push(format!("{n} {name}"));
        }
    }
    let counts = if counts.is_empty() {
        String::new()
    } else {
        format!("  ({})", counts.join(", "))
    };
    let _ = writeln!(
        out,
        "\n{} {}{counts}\n",
        p.bold("OVERALL:"),
        p.status(r.overall)
    );

    let width = r.sections.iter().map(|s| s.id.len()).max().unwrap_or(0) + 1;
    let indent = 7 + width;
    for s in &r.sections {
        let tag = format!("[{:^4}]", s.status.label());
        let tag = tag.replace(s.status.label(), &p.status(s.status));
        let _ = writeln!(out, "{tag} {:<width$}{}", s.id, s.summary);
        if verbose || !matches!(s.status, Status::Ok) {
            for d in &s.details {
                let _ = writeln!(out, "{:indent$}{d}", "");
            }
        }
        for f in &s.findings {
            let mark = match f.level {
                Level::Note => p.dim("·"),
                Level::Warn => p.wrap("33;1", "!"),
                Level::Crit => p.wrap("31;1", "!!"),
            };
            let _ = writeln!(out, "{:indent$}{mark} {}", "", f.message);
        }
    }
    out
}

fn trim_float(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::Section;
    use crate::report::Sampling;
    use crate::sysinfo::SysInfo;

    fn report() -> Report {
        let mut warn = Section::new("disk", "Disk I/O", "iostat -xz 1");
        warn.summary("vda util 93%");
        warn.detail("vda r/s 10 w/s 900");
        warn.warn("vda saturated");
        warn.note("fyi");
        let sys = SysInfo {
            hostname: Some("host01".into()),
            kernel: Some("6.8.0".into()),
            distro: Some("Ubuntu 24.04".into()),
            arch: "x86_64".into(),
            cpus_online: 8,
            mem_total_bytes: Some(16 << 30),
            cgroup_cpu_limit: Some(1.5),
            ..Default::default()
        };
        Report::new(
            sys,
            Sampling {
                interval: 0.5,
                count: 4,
            },
            vec![Section::new("load", "Load", "uptime"), warn],
        )
    }

    #[test]
    fn plain_text_layout() {
        let t = render(&report(), false, false);
        let first = t.lines().next().unwrap();
        assert!(
            first.contains("host01 · Linux 6.8.0 · Ubuntu 24.04 · x86_64 · 8 cpus · 16 GiB RAM"),
            "{first}"
        );
        assert!(first.ends_with("sampled 4×0.5s"), "{first}");
        assert!(t.contains("cgroup cpu 1.5"));
        assert!(t.contains("OVERALL: WARN  (1 warning)"));
        assert!(t.contains("[ OK ] load"));
        assert!(t.contains("[WARN] disk vda util 93%"), "{t}");
        assert!(t.contains("! vda saturated"));
        assert!(!t.contains('\x1b'));
    }

    fn section(id: &'static str, warn: bool) -> Section {
        let mut s = Section::new(id, "T", "t");
        s.summary("sum");
        s.detail("detail line");
        s.note("a note");
        if warn {
            s.warn("problem");
        }
        s
    }

    fn render_sections(sections: Vec<Section>, verbose: bool) -> String {
        let r = Report::new(
            SysInfo::default(),
            Sampling {
                interval: 1.0,
                count: 1,
            },
            sections,
        );
        render(&r, false, verbose)
    }

    #[test]
    fn ok_details_hidden() {
        let t = render_sections(vec![section("load", false)], false);
        assert!(!t.contains("detail line"), "{t}");
        assert!(t.contains("· a note"));
    }

    #[test]
    fn problem_details_shown() {
        let t = render_sections(vec![section("disk", true)], false);
        assert!(t.contains("detail line"));
        assert!(t.contains("! problem"));
    }

    #[test]
    fn verbose_shows_all() {
        let t = render_sections(vec![section("load", false)], true);
        assert!(t.contains("detail line"));
    }

    #[test]
    fn long_ids_aligned() {
        let t = render_sections(
            vec![section("load", false), section("cpu-balance", false)],
            false,
        );
        let cols: Vec<usize> = t
            .lines()
            .filter(|l| l.starts_with('['))
            .map(|l| l.find("sum").unwrap())
            .collect();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0], cols[1]);
        assert!(t.contains("cpu-balance sum"));
    }

    #[test]
    fn color_uses_ansi() {
        assert!(render(&report(), true, false).contains("\x1b[33;1mWARN\x1b[0m"));
    }
}
