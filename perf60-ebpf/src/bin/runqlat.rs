//! runqlat: run queue latency, the time from a task becoming runnable (woken, new, or preempted
//! while still runnable) until it is switched in, as a log2 histogram in microseconds.
//! Raw tracepoints `sched_wakeup`, `sched_wakeup_new` and `sched_switch` (no tracefs needed).
//! `task_struct` fields are read at BTF offsets that user space passes as globals.
#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::{bpf_ktime_get_ns, bpf_probe_read_kernel},
    macros::{map, raw_tracepoint},
    maps::{LruHashMap, PerCpuArray},
    programs::RawTracePointContext,
};
use perf60_common::{HIST_BUCKETS, log2_bucket};

perf60_ebpf::license!();
perf60_ebpf::panic_handler!();

/// Byte offset of `task_struct.pid` (set by user space from BTF).
#[unsafe(no_mangle)]
static PID_OFF: u64 = 0;

/// Byte offset of `task_struct.__state` (5.14+) or `task_struct.state` (set from BTF).
#[unsafe(no_mangle)]
static STATE_OFF: u64 = 0;

/// `TASK_RUNNING`: the task is runnable (on a run queue or on a CPU).
const TASK_RUNNING: u32 = 0;

/// Enqueue time (ns) per pid (thread id). LRU, so a full map evicts instead of refusing inserts.
#[map]
static START: LruHashMap<u32, u64> = LruHashMap::with_max_entries(10240, 0);

/// log2 histogram of run queue latency in µs (per CPU; user space sums).
#[map]
static HIST: PerCpuArray<u64> = PerCpuArray::with_max_entries(HIST_BUCKETS, 0);

/// Read a `T` at `task + *off`. The offset global is read volatile so the compiler cannot
/// const-fold its placeholder 0.
#[inline(always)]
fn field<T>(task: *const u8, off: &'static u64) -> Option<T> {
    // SAFETY: `off` points at a valid static.
    let off = unsafe { core::ptr::read_volatile(off) };
    // SAFETY: bpf_probe_read_kernel validates the address and fails instead of faulting.
    unsafe { bpf_probe_read_kernel(task.wrapping_add(off as usize).cast::<T>()) }.ok()
}

#[inline(always)]
fn pid(task: *const u8) -> u32 {
    field::<i32>(task, &PID_OFF).map_or(0, |p| p as u32)
}

/// Record that `task` became runnable now. The idle task (pid 0) is ignored.
#[inline(always)]
fn enqueue(task: *const u8) {
    let pid = pid(task);
    if pid != 0 {
        // SAFETY: no preconditions.
        let now = unsafe { bpf_ktime_get_ns() };
        let _ = START.insert(&pid, &now, 0);
    }
}

/// `sched_wakeup(struct task_struct *p)`
#[raw_tracepoint(tracepoint = "sched_wakeup")]
pub fn runqlat_wakeup(ctx: RawTracePointContext) -> i32 {
    enqueue(ctx.arg(0));
    0
}

/// `sched_wakeup_new(struct task_struct *p)`
#[raw_tracepoint(tracepoint = "sched_wakeup_new")]
pub fn runqlat_wakeup_new(ctx: RawTracePointContext) -> i32 {
    enqueue(ctx.arg(0));
    0
}

/// `sched_switch(bool preempt, struct task_struct *prev, struct task_struct *next, …)`
#[raw_tracepoint(tracepoint = "sched_switch")]
pub fn runqlat_switch(ctx: RawTracePointContext) -> i32 {
    let prev: *const u8 = ctx.arg(1);
    let next: *const u8 = ctx.arg(2);
    // Preempted while still runnable: it waits on the run queue from now on. The low 32 bits
    // of `state` (a long before 5.14) are enough to test for 0 on little-endian kernels.
    if field::<u32>(prev, &STATE_OFF) == Some(TASK_RUNNING) {
        enqueue(prev);
    }
    let pid = pid(next);
    if pid == 0 {
        return 0;
    }
    // SAFETY: the value is copied out immediately.
    let Some(start) = (unsafe { START.get(&pid) }).copied() else {
        return 0;
    };
    // SAFETY: no preconditions.
    let now = unsafe { bpf_ktime_get_ns() };
    let bucket = log2_bucket(now.saturating_sub(start) / 1000);
    if let Some(slot) = HIST.get_ptr_mut(bucket) {
        // SAFETY: per-CPU slot, no concurrent writer on this CPU.
        unsafe { *slot += 1 };
    }
    let _ = START.remove(&pid);
    0
}
