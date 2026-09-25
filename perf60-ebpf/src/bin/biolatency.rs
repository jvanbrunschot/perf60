//! biolatency: block I/O latency (issue to completion) as a log2 histogram in µs per disk.
//! Raw tracepoints `block_rq_issue` and `block_rq_complete` (no tracefs needed). The disk name
//! is read from the kernel's `struct request` at BTF offsets that user space passes as globals.
#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::{bpf_ktime_get_ns, bpf_probe_read_kernel, bpf_probe_read_kernel_str_bytes},
    macros::{map, raw_tracepoint},
    maps::{HashMap, LruHashMap},
    programs::RawTracePointContext,
};
use perf60_common::log2_bucket;

perf60_ebpf::license!();
perf60_ebpf::panic_handler!();

/// `DISK_NAME_LEN` in the kernel.
const DISK_NAME_LEN: usize = 32;

/// Histogram key: disk name and log2 bucket. Same `#[repr(C)]` layout as
/// `perf60::deep::biolatency::DiskBucket` (36 bytes, no padding).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DiskBucket {
    pub name: [u8; DISK_NAME_LEN],
    pub bucket: u32,
}

const _: () = assert!(core::mem::size_of::<DiskBucket>() == 36);

/// Argument index of `struct request *` in `block_rq_issue`: 0 on kernel >= 5.11, 1 before
/// (where the first argument is the `struct request_queue *`).
#[unsafe(no_mangle)]
static RQ_ARG: u64 = 0;
/// 1: disk = `rq->q->disk` (current kernels); 0: disk = `rq->rq_disk` (older kernels).
#[unsafe(no_mangle)]
static DISK_VIA_QUEUE: u64 = 0;
/// Offset of `request.q` (via queue) or `request.rq_disk`.
#[unsafe(no_mangle)]
static RQ_OFF: u64 = 0;
/// Offset of `request_queue.disk` (via queue only).
#[unsafe(no_mangle)]
static QUEUE_DISK_OFF: u64 = 0;
/// Offset of `gendisk.disk_name`.
#[unsafe(no_mangle)]
static DISK_NAME_OFF: u64 = 0;

/// Issue timestamp (ns) per in-flight request. LRU, so requests whose completion we never see
/// (e.g. issued before a hot-unplug) are evicted instead of filling the map.
#[map]
static START: LruHashMap<u64, u64> = LruHashMap::with_max_entries(10240, 0);

/// I/O count per (disk, log2 µs bucket). Entries grow with disks × non-empty buckets: latencies
/// from 1 µs to 16 s span about 25 buckets, so 4096 entries cover ~160 busy disks (64 even if a
/// disk filled all 64 buckets). When full, new (disk, bucket) pairs are dropped.
#[map]
static HIST: HashMap<DiskBucket, u64> = HashMap::with_max_entries(4096, 0);

#[inline(always)]
fn global(g: &'static u64) -> u64 {
    // SAFETY: a valid static; volatile so the loader-set value isn't constant-folded to 0.
    unsafe { core::ptr::read_volatile(g) }
}

#[raw_tracepoint(tracepoint = "block_rq_issue")]
pub fn biolatency_issue(ctx: RawTracePointContext) -> i32 {
    let rq: u64 = if global(&RQ_ARG) == 1 {
        ctx.arg(1)
    } else {
        ctx.arg(0)
    };
    // SAFETY: helper without preconditions.
    let now = unsafe { bpf_ktime_get_ns() };
    let _ = START.insert(&rq, &now, 0);
    0
}

#[raw_tracepoint(tracepoint = "block_rq_complete")]
pub fn biolatency_complete(ctx: RawTracePointContext) -> i32 {
    let rq: u64 = ctx.arg(0);
    let Some(start) = START.get_ptr(&rq) else {
        return 0;
    };
    // SAFETY: a valid map value pointer.
    let start = unsafe { *start };
    let _ = START.remove(&rq);
    // SAFETY: helper without preconditions.
    let us = unsafe { bpf_ktime_get_ns() }.saturating_sub(start) / 1000;
    let mut key = DiskBucket {
        name: [0; DISK_NAME_LEN],
        bucket: log2_bucket(us),
    };
    // SAFETY: reads kernel memory through the checked helper; failures leave the name empty.
    if unsafe { !read_disk_name(rq, &mut key.name) } {
        return 0;
    }
    if let Some(v) = HIST.get_ptr_mut(&key) {
        // SAFETY: a valid map value pointer. Completions on several CPUs can race and lose an
        // increment (the bpfel target has no 64-bit atomic add in `core`); the error is
        // negligible for a distribution.
        unsafe { *v += 1 };
    } else {
        let _ = HIST.insert(&key, &1, 0);
    }
    0
}

/// Copy the gendisk name of request `rq` into `name`; false when it can't be resolved.
#[inline(always)]
unsafe fn read_disk_name(rq: u64, name: &mut [u8; DISK_NAME_LEN]) -> bool {
    let read = |addr: u64| unsafe { bpf_probe_read_kernel(addr as *const u64).unwrap_or(0) };
    let mut disk = read(rq + global(&RQ_OFF));
    if global(&DISK_VIA_QUEUE) == 1 && disk != 0 {
        disk = read(disk + global(&QUEUE_DISK_OFF));
    }
    if disk == 0 {
        return false;
    }
    let src = (disk + global(&DISK_NAME_OFF)) as *const u8;
    matches!(unsafe { bpf_probe_read_kernel_str_bytes(src, name) }, Ok(s) if !s.is_empty())
}
