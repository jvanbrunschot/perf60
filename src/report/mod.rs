//! Report model plus text and JSON renderers.

pub mod diagnosis;
pub mod json;
pub mod text;

use serde::Serialize;

use crate::check::{Section, Status, overall};
use crate::sysinfo::SysInfo;

#[derive(Debug, Clone, Serialize)]
pub struct Sampling {
    pub interval: f64,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub version: &'static str,
    pub sampling: Sampling,
    pub system: SysInfo,
    pub overall: Status,
    /// Likely bottleneck across sections; `null` when nothing is WARN or CRIT.
    pub diagnosis: Option<diagnosis::Diagnosis>,
    pub sections: Vec<Section>,
}

impl Report {
    pub fn new(system: SysInfo, sampling: Sampling, sections: Vec<Section>) -> Self {
        Report {
            version: env!("CARGO_PKG_VERSION"),
            overall: overall(&sections),
            diagnosis: diagnosis::diagnose(&sections),
            sampling,
            system,
            sections,
        }
    }

    pub fn count(&self, status: Status) -> usize {
        self.sections.iter().filter(|s| s.status == status).count()
    }
}
