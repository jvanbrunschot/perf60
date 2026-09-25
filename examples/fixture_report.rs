//! Render the report for a fixture tree: `cargo run --example fixture_report -- linux-legacy`.
fn main() {
    let tree = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "linux-arm64".into());
    let root = format!("{}/tests/fixtures/{tree}", env!("CARGO_MANIFEST_DIR"));
    let r = perf60::analyze(&perf60::source::FsSource::new(root), 0.0, 1);
    print!("{}", perf60::report::text::render(&r, false, true));
}
