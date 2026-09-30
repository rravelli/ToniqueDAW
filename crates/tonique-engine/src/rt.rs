//! Real-time safety utilities: denormal flushing, thread priority, and an
//! allocation checker that turns "no allocation in process()" from a
//! convention into something tests can enforce.

use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Set flush-to-zero / denormals-are-zero on the calling thread. IIR filters
/// and decaying tails otherwise hit denormals and spike CPU silently.
pub fn enable_flush_denormals() {
    #[cfg(target_arch = "x86_64")]
    {
        #[allow(deprecated)]
        unsafe {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            // FTZ (bit 15) | DAZ (bit 6)
            _mm_setcsr(_mm_getcsr() | 0x8040);
        }
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let mut fpcr: u64;
        std::arch::asm!("mrs {}, fpcr", out(reg) fpcr);
        fpcr |= 1 << 24; // FZ
        std::arch::asm!("msr fpcr, {}", in(reg) fpcr);
    }
}

/// Whether FTZ is active on the calling thread (where detectable).
pub fn flush_denormals_enabled() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        #[allow(deprecated)]
        unsafe {
            std::arch::x86_64::_mm_getcsr() & 0x8000 != 0
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Request real-time (SCHED_FIFO) scheduling for the calling thread. Meant
/// for the audio callback thread only, not helper workers. Usually requires
/// rtkit / an rtprio limit; failure is not fatal.
pub fn set_realtime_priority(priority: i32) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let param = libc::sched_param {
            sched_priority: priority,
        };
        let rc =
            unsafe { libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_FIFO, &param) };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(rc))
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = priority;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "not implemented on this platform",
        ))
    }
}

thread_local! {
    static NO_ALLOC_DEPTH: Cell<u32> = const { Cell::new(0) };
}

static VIOLATIONS: AtomicUsize = AtomicUsize::new(0);

/// Run `f` in a region where allocating is a bug. The check is only
/// enforced when [`CheckingAllocator`] is the global allocator; otherwise
/// this costs a thread-local increment.
#[inline]
pub fn no_alloc<R>(f: impl FnOnce() -> R) -> R {
    NO_ALLOC_DEPTH.with(|d| d.set(d.get() + 1));
    let r = f();
    NO_ALLOC_DEPTH.with(|d| d.set(d.get() - 1));
    r
}

/// Number of allocations/frees seen inside `no_alloc` regions so far.
pub fn alloc_violations() -> usize {
    VIOLATIONS.load(Ordering::SeqCst)
}

/// Global allocator wrapper that records any allocation, reallocation or
/// deallocation made inside a [`no_alloc`] region. Install it in test or
/// debug binaries:
///
/// ```ignore
/// #[global_allocator]
/// static A: tonique_engine::rt::CheckingAllocator<std::alloc::System> =
///     tonique_engine::rt::CheckingAllocator(std::alloc::System);
/// ```
pub struct CheckingAllocator<A>(pub A);

impl<A> CheckingAllocator<A> {
    #[inline]
    fn check(&self) {
        // `try_with`: TLS may be gone during thread teardown.
        if NO_ALLOC_DEPTH.try_with(|d| d.get() > 0).unwrap_or(false) {
            VIOLATIONS.fetch_add(1, Ordering::SeqCst);
        }
    }
}

unsafe impl<A: GlobalAlloc> GlobalAlloc for CheckingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.check();
        unsafe { self.0.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.check();
        unsafe { self.0.dealloc(ptr, layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        self.check();
        unsafe { self.0.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        self.check();
        unsafe { self.0.realloc(ptr, layout, new_size) }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn denormals_flag_sets() {
        super::enable_flush_denormals();
        #[cfg(target_arch = "x86_64")]
        assert!(super::flush_denormals_enabled());
    }
}
