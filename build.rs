//! With the `deep` feature, build the eBPF programs in `perf60-ebpf/` (one object per
//! `src/bin/<probe>.rs`) with a pinned nightly and embed them from `$OUT_DIR`.
//! Without it this does nothing, so plain builds need no eBPF toolchain.

/// Pinned nightly for the eBPF crate (needs the `rust-src` component). Keep in sync with
/// scripts/build-deep.sh and .github/workflows/*.yml.
#[cfg(feature = "deep")]
const EBPF_TOOLCHAIN: &str = "nightly-2026-09-20";

fn main() {
    #[cfg(feature = "deep")]
    {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/perf60-ebpf");
        aya_build::build_ebpf(
            [aya_build::Package {
                name: "perf60-ebpf",
                root_dir: root,
                ..Default::default()
            }],
            aya_build::Toolchain::Custom(EBPF_TOOLCHAIN),
        )
        .expect("building eBPF programs (see scripts/build-deep.sh for the toolchain)");
    }
}
