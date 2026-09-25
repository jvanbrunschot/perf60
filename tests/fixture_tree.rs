//! End-to-end: run every check against a captured Linux fixture tree.

use perf60::check::Status;
use perf60::source::FsSource;

fn fixture(name: &str) -> FsSource {
    FsSource::new(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

#[test]
fn every_registered_check_renders_against_fixture() {
    let r = perf60::analyze(&fixture("linux-arm64"), 0.0, 1);
    let ids: Vec<&str> = r.sections.iter().map(|s| s.id).collect();
    assert_eq!(ids.len(), perf60::checks::all().len());
    for s in &r.sections {
        assert!(!s.summary.is_empty(), "{} has empty summary", s.id);
        assert_ne!(s.status, Status::Skipped, "{} skipped: {}", s.id, s.summary);
    }
    let text = perf60::report::text::render(&r, false);
    assert!(text.starts_with("perf60 "));
    let json: serde_json::Value = serde_json::from_str(&perf60::report::json::render(&r)).unwrap();
    assert_eq!(json["sections"].as_array().unwrap().len(), ids.len());
}

#[test]
fn empty_root_skips_instead_of_failing() {
    let r = perf60::analyze(&fixture("does-not-exist"), 0.0, 1);
    assert!(r.sections.iter().all(|s| s.status == Status::Skipped));
    assert_eq!(r.overall, Status::Ok);
}
