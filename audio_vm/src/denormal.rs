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
        use core::arch::x86_64::{
            _MM_DENORMALS_ZERO_ON, _MM_FLUSH_ZERO_ON, _MM_SET_DENORMALS_ZERO_MODE,
            _MM_SET_FLUSH_ZERO_MODE,
        };

        _MM_SET_FLUSH_ZERO_MODE(_MM_FLUSH_ZERO_ON);
        _MM_SET_DENORMALS_ZERO_MODE(_MM_DENORMALS_ZERO_ON);
    }
}
