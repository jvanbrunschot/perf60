//! execsnoop: count exec() calls by the new command name, and fork() calls in total.
//! Raw tracepoints `sched_process_exec` and `sched_process_fork` (no tracefs needed); no
//! arguments are read because the command comes from `bpf_get_current_comm`.
#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::bpf_get_current_comm,
    macros::{map, raw_tracepoint},
    maps::{HashMap, PerCpuArray},
    programs::RawTracePointContext,
};
use perf60_common::CommKey;

perf60_ebpf::license!();
perf60_ebpf::panic_handler!();

/// exec count per command name (after exec).
#[map]
static EXECS: HashMap<CommKey, u64> = HashMap::with_max_entries(4096, 0);

/// [0] = total execs, [1] = total forks (per CPU; user space sums).
#[map]
static TOTALS: PerCpuArray<u64> = PerCpuArray::with_max_entries(2, 0);

#[raw_tracepoint(tracepoint = "sched_process_exec")]
pub fn execsnoop_exec(_ctx: RawTracePointContext) -> i32 {
    bump(0);
    if let Ok(comm) = bpf_get_current_comm() {
        let key = CommKey { comm };
        match EXECS.get_ptr_mut(&key) {
            // SAFETY: a valid map value pointer; a racing increment can at worst lose a count.
            Some(v) => unsafe { *v += 1 },
            None => {
                let _ = EXECS.insert(&key, &1, 0);
            }
        }
    }
    0
}

#[raw_tracepoint(tracepoint = "sched_process_fork")]
pub fn execsnoop_fork(_ctx: RawTracePointContext) -> i32 {
    bump(1);
    0
}

#[inline(always)]
fn bump(i: u32) {
    if let Some(v) = TOTALS.get_ptr_mut(i) {
        // SAFETY: per-CPU slot, no concurrent writer on this CPU.
        unsafe { *v += 1 };
    }
}
