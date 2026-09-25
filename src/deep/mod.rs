//! `--deep`: eBPF probes from Brendan Gregg's BCC checklist (BPF Performance Tools, ch. 3).
//! Each probe is a [`Check`]: it attaches on the first sample, the kernel aggregates into maps
//! during the window, and `evaluate` reads the maps. Programs detach when the check is dropped.
//!
//! The evaluation logic of every probe is plain Rust and always compiled (and tested on any
//! OS). Loading and attaching needs the `deep` cargo feature, which embeds the eBPF objects
//! built from `perf60-ebpf/`.

use crate::check::{Check, Context, Resource, Section};
use crate::source::Source;

pub mod biolatency;
pub mod btf;
pub mod caps;
pub mod execsnoop;
pub mod hist;
#[cfg(feature = "deep")]
pub mod probe;
pub mod runqlat;
pub mod tcpretrans;

/// The `--deep` checks of this build.
pub fn checks() -> Vec<Box<dyn Check>> {
    #[cfg(feature = "deep")]
    {
        vec![
            Box::new(execsnoop::Execsnoop::default()),
            Box::new(runqlat::Runqlat::default()),
            Box::new(biolatency::Biolatency::default()),
            Box::new(tcpretrans::Tcpretrans::default()),
        ]
    }
    #[cfg(not(feature = "deep"))]
    {
        vec![Box::new(Unavailable)]
    }
}

/// Stand-in section when perf60 was built without eBPF support.
pub struct Unavailable;

impl Check for Unavailable {
    fn id(&self) -> &'static str {
        "deep"
    }

    fn sample(&mut self, _src: &dyn Source, _t: f64) {}

    fn evaluate(&self, _ctx: &Context) -> Section {
        Section::new("deep", "eBPF probes", "BCC tools", Resource::Kernel).skipped(
            "this perf60 build has no eBPF support (built without the `deep` feature, see scripts/build-deep.sh)",
        )
    }
}

/// Seconds between the first and last sample, for per-second rates.
#[derive(Default, Debug, Clone, Copy)]
pub struct Window {
    first: Option<f64>,
    last: f64,
}

impl Window {
    pub fn tick(&mut self, t: f64) {
        self.first.get_or_insert(t);
        self.last = t;
    }

    pub fn secs(&self) -> f64 {
        (self.last - self.first.unwrap_or(self.last)).max(0.0)
    }
}

/// NUL-terminated `comm` bytes to a string.
pub fn comm_str(comm: &[u8]) -> String {
    let end = comm.iter().position(|&b| b == 0).unwrap_or(comm.len());
    String::from_utf8_lossy(&comm[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_and_comm() {
        let mut w = Window::default();
        assert_eq!(w.secs(), 0.0);
        w.tick(1.0);
        w.tick(3.5);
        assert_eq!(w.secs(), 2.5);
        assert_eq!(comm_str(b"sh\0\0\0"), "sh");
        assert_eq!(comm_str(b"sixteen-chars-xx"), "sixteen-chars-xx");
    }

    #[cfg(not(feature = "deep"))]
    #[test]
    fn unavailable_without_feature() {
        let ctx = Context {
            sys: Default::default(),
            interval: 1.0,
            count: 1,
        };
        let s = checks()[0].evaluate(&ctx);
        assert_eq!(s.status, crate::check::Status::Skipped);
        assert!(s.summary.contains("`deep` feature"));
    }
}
