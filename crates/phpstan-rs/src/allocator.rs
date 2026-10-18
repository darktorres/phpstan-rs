//! The process's allocator: mimalloc, unless a build measures its
//! allocations (`dhat-heap`, `hotpath-alloc`) or asks for the system's
//! (`system-allocator`).

// mimalloc is reached through its C API.
#[cfg(not(any(
    feature = "dhat-heap",
    feature = "hotpath-alloc",
    feature = "system-allocator"
)))]
#[allow(unsafe_code)]
mod mimalloc;

/// The allocator every project's tables are allocated from and freed to:
/// building and dropping a project is mostly allocation, and this serves
/// many threads' allocations and frees faster than the system's.
#[cfg(not(any(
    feature = "dhat-heap",
    feature = "hotpath-alloc",
    feature = "system-allocator"
)))]
#[global_allocator]
static ALLOCATOR: mimalloc::Allocator = mimalloc::Allocator;

/// Stops the allocator handing memory the process has freed back to the
/// system while the process runs, for a command that exits once its work is
/// done: the memory is the process's own until then, and every purge costs
/// the kernel a flush of the other threads' TLBs. Does nothing under an
/// allocator other than mimalloc.
///
/// # Safety
///
/// No other thread of the process may be running.
#[allow(unsafe_code)]
pub unsafe fn keep_freed_memory() {
    #[cfg(not(any(
        feature = "dhat-heap",
        feature = "hotpath-alloc",
        feature = "system-allocator"
    )))]
    // SAFETY: the caller runs no other thread.
    unsafe {
        mimalloc::keep_freed_memory();
    }
}
