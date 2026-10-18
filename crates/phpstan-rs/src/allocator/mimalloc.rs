//! mimalloc, asked for the alignment a request needs only when it needs
//! more than a block has by its size.
//!
//! The `mimalloc` crate's `GlobalAlloc` calls the `_aligned` entry points
//! for every request, and their alignment handling is a large part of an
//! allocation's cost (most requests are for 8 bytes of alignment, which a
//! block of any size has, or for 16 on a size that is a multiple of it).

use std::alloc::{GlobalAlloc, Layout};
use std::ffi::c_void;

pub(crate) struct Allocator;

/// mimalloc's `mi_option_purge_delay`, which the binding has no constant
/// for: index 15 of `mimalloc.h`'s `mi_option_e`, between the binding's
/// `mi_option_eager_commit_delay` (14) and `mi_option_use_numa_nodes` (16).
const PURGE_DELAY: libmimalloc_sys::mi_option_t = 15;

/// Stops the allocator purging: memory the process frees stays its own, for
/// its later allocations, until it exits.
///
/// # Safety
///
/// No other thread may be running: `mi_option_set` writes the option with
/// no synchronization, and the allocator reads it each time it schedules or
/// makes a purge.
pub(crate) unsafe fn keep_freed_memory() {
    // SAFETY: the caller runs no other thread, so nothing reads the option
    // while it is written.
    unsafe { libmimalloc_sys::mi_option_set(PURGE_DELAY, -1) }
}

/// Whether a block `mi_malloc` serves for `size` bytes is aligned to
/// `align` without being asked: blocks are multiples of 8 bytes in pages
/// aligned to far more, and a block whose size is a multiple of 16 starts
/// at a multiple of 16.
fn is_natural(size: usize, align: usize) -> bool {
    align <= 8 || (align <= 16 && size.is_multiple_of(16))
}

// SAFETY: every call is to the allocator the matching call of the same
// library frees, and a request that needs more alignment than a block has
// by its size goes to the `_aligned` entry points, as the `mimalloc` crate
// sends them all.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: `mi_malloc` and `mi_malloc_aligned` accept any size and
        // a power-of-two alignment, which a `Layout` always holds.
        unsafe {
            if is_natural(layout.size(), layout.align()) {
                libmimalloc_sys::mi_malloc(layout.size()) as *mut u8
            } else {
                libmimalloc_sys::mi_malloc_aligned(layout.size(), layout.align()) as *mut u8
            }
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as `alloc`.
        unsafe {
            if is_natural(layout.size(), layout.align()) {
                libmimalloc_sys::mi_zalloc(layout.size()) as *mut u8
            } else {
                libmimalloc_sys::mi_zalloc_aligned(layout.size(), layout.align()) as *mut u8
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // SAFETY: `ptr` came from this allocator; `mi_free` frees a block
        // of either kind.
        unsafe { libmimalloc_sys::mi_free(ptr as *mut c_void) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `ptr` came from this allocator with `layout`. A block
        // that is naturally aligned for the new size stays so through
        // `mi_realloc`; any other keeps its alignment through the
        // `_aligned` entry point.
        unsafe {
            if is_natural(new_size, layout.align()) {
                libmimalloc_sys::mi_realloc(ptr as *mut c_void, new_size) as *mut u8
            } else {
                libmimalloc_sys::mi_realloc_aligned(ptr as *mut c_void, new_size, layout.align())
                    as *mut u8
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_purge_delay_option_is_the_one_whose_default_is_a_second() {
        // An environment that sets the option changes what it reads.
        if ["MIMALLOC_PURGE_DELAY", "MIMALLOC_RESET_DELAY"]
            .iter()
            .any(|name| std::env::var_os(name).is_some())
        {
            return;
        }
        // SAFETY: every option was read at load, so `mi_option_get` only
        // reads; nothing in this test binary sets one.
        let delay = unsafe { libmimalloc_sys::mi_option_get(super::PURGE_DELAY) };
        assert_eq!(delay, 1000);
    }
}
