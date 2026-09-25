//! tcpretrans: count TCP retransmits by remote endpoint (family, address, port), and in total.
//! Raw tracepoint `tcp_retransmit_skb(const struct sock *sk, const struct sk_buff *skb, …)`
//! (no tracefs needed). The `sock_common` fields are read at offsets that user space resolves
//! from the kernel's BTF and passes in as globals.
#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::bpf_probe_read_kernel,
    macros::{map, raw_tracepoint},
    maps::{HashMap, PerCpuArray},
    programs::RawTracePointContext,
};

perf60_ebpf::license!();
perf60_ebpf::panic_handler!();

const AF_INET: u16 = 2;
const AF_INET6: u16 = 10;

/// Remote endpoint; the same `#[repr(C)]` layout as `perf60::deep::tcpretrans::Endpoint`.
/// `dport` is in network byte order; IPv4 addresses use the first 4 bytes of `daddr`.
#[repr(C)]
#[derive(Clone, Copy)]
struct Endpoint {
    family: u16,
    dport: u16,
    daddr: [u8; 16],
}

const _: () = assert!(core::mem::size_of::<Endpoint>() == 20);

// Byte offsets from the kernel's BTF, set by user space at load time.
/// `sock.__sk_common` (0 on every kernel so far).
#[unsafe(no_mangle)]
static SK_COMMON_OFF: u64 = 0;
#[unsafe(no_mangle)]
static SKC_FAMILY_OFF: u64 = 0;
#[unsafe(no_mangle)]
static SKC_DPORT_OFF: u64 = 0;
#[unsafe(no_mangle)]
static SKC_DADDR_OFF: u64 = 0;
#[unsafe(no_mangle)]
static SKC_V6_DADDR_OFF: u64 = 0;

/// Retransmits per remote endpoint.
#[map]
static RETRANS: HashMap<Endpoint, u64> = HashMap::with_max_entries(4096, 0);

/// [0] = total retransmits (per CPU; user space sums).
#[map]
static TOTAL: PerCpuArray<u64> = PerCpuArray::with_max_entries(1, 0);

#[raw_tracepoint(tracepoint = "tcp_retransmit_skb")]
pub fn tcpretrans(ctx: RawTracePointContext) -> i32 {
    if let Some(v) = TOTAL.get_ptr_mut(0) {
        // SAFETY: per-CPU slot, no concurrent writer on this CPU.
        unsafe { *v += 1 };
    }
    let sk: *const u8 = ctx.arg(0);
    if let Some(key) = endpoint(sk) {
        match RETRANS.get_ptr_mut(&key) {
            // SAFETY: a valid map value pointer; a racing increment can at worst lose a count.
            Some(v) => unsafe { *v += 1 },
            None => {
                let _ = RETRANS.insert(&key, &1, 0);
            }
        }
    }
    0
}

#[inline(always)]
fn offset(global: &u64) -> usize {
    // SAFETY: reading our own global; volatile so the loader-set value isn't constant-folded.
    unsafe { core::ptr::read_volatile(global) as usize }
}

/// The remote end of `sk`, or `None` for a non-IP family or a failed read.
#[inline(always)]
fn endpoint(sk: *const u8) -> Option<Endpoint> {
    let common = sk.wrapping_add(offset(&SK_COMMON_OFF));
    let at = |global: &u64| common.wrapping_add(offset(global));
    // SAFETY: bpf_probe_read_kernel checks the address and fails instead of faulting.
    let family = unsafe { bpf_probe_read_kernel(at(&SKC_FAMILY_OFF).cast::<u16>()) }.ok()?;
    let dport = unsafe { bpf_probe_read_kernel(at(&SKC_DPORT_OFF).cast::<u16>()) }.ok()?;
    let mut key = Endpoint {
        family,
        dport,
        daddr: [0; 16],
    };
    match family {
        AF_INET => {
            let a = unsafe { bpf_probe_read_kernel(at(&SKC_DADDR_OFF).cast::<[u8; 4]>()) }.ok()?;
            key.daddr[..4].copy_from_slice(&a);
        }
        AF_INET6 => {
            key.daddr =
                unsafe { bpf_probe_read_kernel(at(&SKC_V6_DADDR_OFF).cast::<[u8; 16]>()) }.ok()?;
        }
        _ => return None,
    }
    Some(key)
}
