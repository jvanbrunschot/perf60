//! End-to-end: run every check against each fixture tree.
//!
//! `linux-arm64` is captured from a real container (`scripts/capture-fixture.sh`);
//! `linux-legacy` is a synthetic 3.x/4.x-era tree (see its README). Each tree lists the
//! sections that must be SKIPPED because the data does not exist there; every other section
//! must render with a summary. A new check needing data a tree lacks either adds it to the
//! tree or is added to that tree's expected skips with a reason.

use perf60::check::Status;
use perf60::source::FsSource;

const TREES: &[(&str, &[&str])] = &[
    ("linux-arm64", &[]),
    // PSI arrived in 4.20.
    ("linux-legacy", &["pressure"]),
];

fn fixture(name: &str) -> FsSource {
    FsSource::new(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

#[test]
fn every_registered_check_renders_against_each_fixture() {
    for (tree, expected_skips) in TREES {
        let r = perf60::analyze(&fixture(tree), 0.0, 1);
        let ids: Vec<&str> = r.sections.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), perf60::checks::all().len(), "{tree}");
        for s in &r.sections {
            assert!(!s.summary.is_empty(), "{tree}: {} has empty summary", s.id);
            let skipped = s.status == Status::Skipped;
            let expected = expected_skips.contains(&s.id);
            assert_eq!(
                skipped, expected,
                "{tree}: {} skipped={skipped}, expected={expected}: {}",
                s.id, s.summary
            );
            for (k, v) in &s.metrics {
                assert!(v.is_finite(), "{tree}: {}.{k} = {v}", s.id);
            }
        }
        let text = perf60::report::text::render(&r, false, true);
        assert!(text.starts_with("perf60 "), "{tree}");
        let json: serde_json::Value =
            serde_json::from_str(&perf60::report::json::render(&r)).unwrap();
        assert_eq!(
            json["sections"].as_array().unwrap().len(),
            ids.len(),
            "{tree}"
        );
    }
}

#[test]
fn legacy_tree_exercises_old_formats() {
    let r = perf60::analyze(&fixture("linux-legacy"), 0.0, 1);
    assert_eq!(r.system.distro.as_deref(), Some("CentOS Linux 7 (Core)"));
    assert_eq!(r.system.cpus_online, 2);
    let memory = r.sections.iter().find(|s| s.id == "memory").unwrap();
    // No MemAvailable: MemFree + Buffers + Cached.
    assert!(
        memory.summary.starts_with("available 2.4 GiB of 3.7 GiB"),
        "{}",
        memory.summary
    );
    let disk = r.sections.iter().find(|s| s.id == "disk").unwrap();
    assert_ne!(disk.status, Status::Skipped, "{}", disk.summary);
}

#[test]
fn source_syscalls_read_fixture_files() {
    use perf60::source::Source;
    let src = fixture("linux-legacy");
    let root = src.statvfs("/").unwrap();
    assert_eq!(root.blocks, 13100800);
    assert!(src.statvfs("/nope").is_err());
    assert!(src.clock_status().unwrap().synchronized());
    let arm = fixture("linux-arm64");
    assert!(arm.statvfs("/").unwrap().blocks > 0);
    assert!(arm.clock_status().is_ok());
}

#[test]
fn empty_root_skips_instead_of_failing() {
    let r = perf60::analyze(&fixture("does-not-exist"), 0.0, 1);
    assert!(r.sections.iter().all(|s| s.status == Status::Skipped));
    assert_eq!(r.overall, Status::Ok);
}
