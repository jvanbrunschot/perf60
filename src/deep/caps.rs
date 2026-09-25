//! Do we have the capabilities eBPF tracing needs? Checked before loading so the SKIPPED reason
//! is actionable instead of a raw map-creation error.

const CAP_SYS_ADMIN: u32 = 21;
const CAP_PERFMON: u32 = 38;
const CAP_BPF: u32 = 39;

pub const NEED_ROOT: &str =
    "needs root (CAP_BPF and CAP_PERFMON, or CAP_SYS_ADMIN; in a container use --privileged)";

/// Effective capability mask from `/proc/self/status` (`CapEff:` hex).
pub fn cap_eff(status: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("CapEff:"))
        .and_then(|v| u64::from_str_radix(v.trim(), 16).ok())
}

/// CAP_SYS_ADMIN alone, or CAP_BPF together with CAP_PERFMON (kernel 5.8+).
pub fn allows_bpf_tracing(mask: u64) -> bool {
    let has = |c: u32| mask & (1 << c) != 0;
    has(CAP_SYS_ADMIN) || (has(CAP_BPF) && has(CAP_PERFMON))
}

/// `Err(reason)` when this process clearly lacks the capabilities. Unknown → let the load try.
pub fn require_bpf() -> Result<(), String> {
    match std::fs::read_to_string("/proc/self/status")
        .ok()
        .as_deref()
        .and_then(cap_eff)
    {
        Some(mask) if !allows_bpf_tracing(mask) => Err(NEED_ROOT.to_owned()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_masks() {
        // Default docker capabilities (no SYS_ADMIN/BPF/PERFMON).
        let docker = "Name:\tsh\nCapEff:\t00000000a80425fb\n";
        assert_eq!(cap_eff(docker), Some(0xa80425fb));
        assert!(!allows_bpf_tracing(0xa80425fb));
        // root / --privileged: everything.
        assert!(allows_bpf_tracing(0x000001ffffffffff));
        assert!(allows_bpf_tracing(1 << CAP_SYS_ADMIN));
        assert!(allows_bpf_tracing((1 << CAP_BPF) | (1 << CAP_PERFMON)));
        assert!(!allows_bpf_tracing(1 << CAP_BPF));
        assert_eq!(cap_eff("Name:\tx\n"), None);
    }
}
