use super::Report;

pub fn render(report: &Report) -> String {
    let mut s = serde_json::to_string_pretty(report).expect("report is always serializable");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Resource, Section};
    use crate::report::Sampling;
    use crate::sysinfo::SysInfo;

    #[test]
    fn json_is_parseable_and_has_all_sections() {
        let mut warn = Section::new("disk", "Disk I/O", "iostat -xz 1", Resource::Disk);
        warn.warn("busy");
        warn.metric("util_pct", 93.0);
        let r = Report::new(
            SysInfo::default(),
            Sampling {
                interval: 1.0,
                count: 5,
            },
            vec![
                Section::new("load", "Load", "uptime", Resource::Cpu),
                warn,
                Section::new("x", "X", "x", Resource::Kernel).skipped("gone"),
            ],
        );
        let v: serde_json::Value = serde_json::from_str(&render(&r)).unwrap();
        assert_eq!(v["overall"], "WARN");
        assert_eq!(v["diagnosis"]["bottleneck"], "disk I/O");
        assert_eq!(v["diagnosis"]["resource"], "disk");
        assert_eq!(v["diagnosis"]["evidence"][0], "disk: busy");
        assert_eq!(v["sampling"]["count"], 5);
        let sections = v["sections"].as_array().unwrap();
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[1]["equivalent"], "iostat -xz 1");
        assert_eq!(sections[1]["resource"], "disk");
        assert_eq!(sections[0]["resource"], "cpu");
        assert_eq!(sections[1]["metrics"]["util_pct"], 93.0);
        assert_eq!(sections[1]["findings"][0]["level"], "warn");
        assert_eq!(sections[2]["status"], "SKIPPED");
        assert!(v["system"].is_object());
    }

    #[test]
    fn diagnosis_is_null_when_all_ok() {
        let r = Report::new(
            SysInfo::default(),
            Sampling {
                interval: 1.0,
                count: 1,
            },
            vec![Section::new("load", "Load", "uptime", Resource::Cpu)],
        );
        let v: serde_json::Value = serde_json::from_str(&render(&r)).unwrap();
        assert!(v["diagnosis"].is_null());
    }
}
