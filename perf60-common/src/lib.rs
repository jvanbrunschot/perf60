//! Types shared by perf60's eBPF programs (`perf60-ebpf`) and user space (`perf60 --deep`).
//! Everything is `#[repr(C)]` and plain data, so it has the same layout on both sides.
#![no_std]

/// Maximum command name length (`TASK_COMM_LEN`).
pub const COMM_LEN: usize = 16;

/// Number of log2 histogram buckets. Bucket `i` counts values in `[2^i, 2^(i+1))`; bucket 0
/// also holds 0. 64 buckets cover every u64.
pub const HIST_BUCKETS: u32 = 64;

/// Map key for per-command counters.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct CommKey {
    pub comm: [u8; COMM_LEN],
}

/// log2 bucket of `v` (floor), as used by every histogram map.
#[inline(always)]
pub fn log2_bucket(v: u64) -> u32 {
    if v == 0 { 0 } else { 63 - v.leading_zeros() }
}

#[cfg(feature = "user")]
mod user {
    // SAFETY: plain `#[repr(C)]` data without padding or pointers.
    unsafe impl aya::Pod for super::CommKey {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets() {
        assert_eq!(log2_bucket(0), 0);
        assert_eq!(log2_bucket(1), 0);
        assert_eq!(log2_bucket(2), 1);
        assert_eq!(log2_bucket(3), 1);
        assert_eq!(log2_bucket(1024), 10);
        assert_eq!(log2_bucket(u64::MAX), 63);
    }
}
