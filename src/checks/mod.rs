//! Registry of all checks, in the order of Brendan Gregg's checklist.
//! Adding a check: create `src/checks/<name>.rs` and add one line to `all()`.

use crate::check::Check;

pub mod cpu;
pub mod disk;
pub mod kernel_log;
pub mod load;
pub mod memory;
pub mod net;
pub mod pressure;
pub mod processes;

pub fn all() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(load::Load::default()),            // uptime
        Box::new(kernel_log::KernelLog::default()), // dmesg | tail
        Box::new(cpu::Cpu::default()),              // vmstat 1
        Box::new(cpu::CpuBalance::default()),       // mpstat -P ALL 1
        Box::new(processes::Processes::default()),  // pidstat 1
        Box::new(disk::Disk::default()),            // iostat -xz 1
        Box::new(memory::Memory::default()),        // free -m
        Box::new(memory::Swap::default()),          // vmstat si/so
        Box::new(net::NetDev::default()),           // sar -n DEV 1
        Box::new(net::Tcp::default()),              // sar -n TCP,ETCP 1
        Box::new(pressure::Pressure::default()),    // top / PSI
    ]
}
