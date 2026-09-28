//! Recursion that can't run out of stack: a recursive walk calls
//! [`guard`] at each level, and the stack grows onto the heap whenever
//! what is left of it runs low. No input's depth can abort the process.

/// What must be left of the stack for a walk to go on without growing
/// it: room for the frames between two guarded calls, which are large in
/// an unoptimized build, yet well under the smallest thread stack (2 MiB)
/// so that ordinary code never grows at all.
const RED_ZONE: usize = 1024 * 1024;

/// How much a growth adds at a time.
const SEGMENT: usize = 16 * 1024 * 1024;

/// Runs `f`, first moving to a fresh stack segment when less than
/// `RED_ZONE` is left.
#[inline]
pub fn guard<R>(f: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(RED_ZONE, SEGMENT, f)
}
