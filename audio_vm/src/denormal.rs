/// Enables CPU modes that avoid denormal/subnormal floating point slow paths on
/// realtime audio threads.
///
/// On x86_64 this sets both FTZ (flush-to-zero) and DAZ (denormals-are-zero).
/// ARM/aarch64 targets commonly run audio with denormals flushed already; keep
/// this a no-op there rather than poking platform-specific FPCR state.
#[inline]
pub fn enable_flush_to_zero() {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use core::arch::asm;

        // MXCSR bit 15 is FTZ; bit 6 is DAZ. Keep all other thread-local
        // floating-point control bits unchanged. The DAZ helper intrinsics
        // are not available in core::arch on all stable toolchains.
        let mut mxcsr = 0u32;
        asm!("stmxcsr [{}]", in(reg) &mut mxcsr, options(nostack, preserves_flags));
        mxcsr |= (1 << 15) | (1 << 6);
        asm!("ldmxcsr [{}]", in(reg) &mxcsr, options(nostack, preserves_flags));
    }
}
