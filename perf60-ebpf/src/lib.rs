//! Helpers shared by perf60's eBPF programs. Each program lives in `src/bin/<probe>.rs`.
#![no_std]

/// Every program's `LICENSE` section. The kernel refuses GPL-only helpers (e.g. reading kernel
/// memory) for non-GPL programs; this crate is dual-licensed MIT OR GPL-2.0.
#[macro_export]
macro_rules! license {
    () => {
        #[unsafe(link_section = "license")]
        #[unsafe(no_mangle)]
        static LICENSE: [u8; 13] = *b"Dual MIT/GPL\0";
    };
}

/// eBPF programs cannot unwind; the verifier guarantees this is unreachable.
#[macro_export]
macro_rules! panic_handler {
    () => {
        #[cfg(not(test))]
        #[panic_handler]
        fn panic(_info: &core::panic::PanicInfo) -> ! {
            loop {}
        }
    };
}
