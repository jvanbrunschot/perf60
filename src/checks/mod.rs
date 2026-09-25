//! Registry of all checks, in the order of Brendan Gregg's checklist.
//! Adding a check: create `src/checks/<name>.rs` and add one line to `all()`.

use crate::check::Check;

pub mod disk;
pub mod load;
pub mod processes;

pub fn all() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(load::Load::default()),           // uptime
        Box::new(processes::Processes::default()), // pidstat 1
        Box::new(disk::Disk::default()),           // iostat -xz 1
    ]
}
