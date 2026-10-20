//! Recursion that can't run out of stack: a recursive walk calls
//! [`guard`] at each level, and the stack grows onto the heap whenever
//! what is left of it runs low. No input's depth can abort the process.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

/// What must be left of the stack for a walk to go on without growing
/// it: room for the frames between two guarded calls, which are large in
/// an unoptimized build, yet well under the smallest stack a walk runs on
/// (a spawned thread's 2 MiB; the binary runs its command on a thread of its
/// own, not on the main thread, whose stack the system chooses and Windows
/// makes 1 MiB) so that ordinary code never grows at all.
const RED_ZONE: usize = 1024 * 1024;

/// How much a growth adds at a time.
const SEGMENT: usize = 16 * 1024 * 1024;

thread_local! {
    // The lowest address the stack pointer may reach on this thread's
    // current stack segment before a guarded call must check what is left,
    // `usize::MAX` until one has.
    static LIMIT: std::cell::Cell<usize> = const { std::cell::Cell::new(usize::MAX) };
}

/// Runs `f`, first moving to a fresh stack segment when less than
/// `RED_ZONE` is left. A call far from the end of the stack costs a
/// comparison; what is left is looked up only past a remembered address.
#[inline]
pub fn guard<R>(f: impl FnOnce() -> R) -> R {
    // The stack grows downwards on every platform this runs on.
    let marker = 0u8;
    let here = std::ptr::addr_of!(marker) as usize;
    if here > LIMIT.with(std::cell::Cell::get) {
        return f();
    }
    check(here, f)
}

#[cold]
#[inline(never)]
fn check<R>(here: usize, f: impl FnOnce() -> R) -> R {
    match stacker::remaining_stack() {
        Some(remaining) if remaining < RED_ZONE => {
            // Restored on unwind as well as on return: a panic inside the new
            // segment resumes on this stack, and a limit left inside the
            // segment's freed memory would send a later call onto it.
            let _restore = Restore(LIMIT.with(std::cell::Cell::get));
            stacker::grow(SEGMENT, || {
                // A new segment: the first guarded call on it looks again.
                LIMIT.with(|limit| limit.set(usize::MAX));
                f()
            })
        }
        Some(remaining) => {
            LIMIT.with(|limit| limit.set(here - (remaining - RED_ZONE)));
            f()
        }
        None => f(),
    }
}

/// Puts the limit back to the value it had before a segment was entered.
struct Restore(usize);

impl Drop for Restore {
    fn drop(&mut self) {
        LIMIT.with(|limit| limit.set(self.0));
    }
}

thread_local! {
    // Whether a panic on this thread is left off stderr: set while
    // `catch_quietly` runs its closure.
    static QUIET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `catch_unwind`, without the panic's message on stderr, for a caller that
/// turns the panic into a result of its own, which the message would only
/// repeat. The process's panic hook is never replaced, so a panic on any
/// other thread is reported as it would be: a failing test beside this one
/// keeps its message.
pub fn catch_quietly<R>(f: impl FnOnce() -> R) -> std::thread::Result<R> {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !QUIET.with(std::cell::Cell::get) {
                previous(info);
            }
        }));
    });
    let _quiet = Quiet(QUIET.with(|quiet| quiet.replace(true)));
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
}

/// Puts the quiet flag back to the value it had before a catch began.
struct Quiet(bool);

impl Drop for Quiet {
    fn drop(&mut self) {
        QUIET.with(|quiet| quiet.set(self.0));
    }
}

#[cfg(test)]
mod tests {
    use super::{LIMIT, QUIET, catch_quietly, guard};
    use std::panic::{AssertUnwindSafe, catch_unwind};

    fn descend(depth: usize) -> usize {
        guard(|| {
            let padding = [depth as u8; 512];
            if depth == 0 {
                usize::from(padding[0])
            } else {
                descend(depth - 1) + usize::from(padding[depth % 512] == 0)
            }
        })
    }

    /// `descend`, which panics at the bottom of the recursion.
    fn descend_then_panic(depth: usize) -> usize {
        guard(|| {
            let padding = [depth as u8; 512];
            if depth == 0 {
                panic!("the bottom of the recursion");
            } else {
                descend_then_panic(depth - 1) + usize::from(padding[depth % 512] == 0)
            }
        })
    }

    #[test]
    fn a_recursion_far_deeper_than_the_stack_runs_on_grown_segments() {
        let result = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| descend(200_000))
            .expect("spawns")
            .join()
            .expect("does not overflow");
        assert!(result <= 200_001);
    }

    #[test]
    fn a_shallow_call_after_a_deep_one_still_works() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                descend(50_000);
                descend(10);
                descend(50_000);
            })
            .expect("spawns")
            .join()
            .expect("does not overflow");
    }

    #[test]
    fn a_panic_inside_a_grown_segment_restores_the_limit_and_the_walks_after_it_run() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                // Sets the limit on this stack, so the value to restore is a
                // real one and not the initial `usize::MAX`.
                descend(10);
                let before = LIMIT.with(std::cell::Cell::get);
                let caught = catch_unwind(AssertUnwindSafe(|| descend_then_panic(150_000)));
                assert!(caught.is_err(), "the recursion must panic at its bottom");
                assert_eq!(LIMIT.with(std::cell::Cell::get), before);
                // Depths that are multiples of 256 are counted, so the sum is 200_000 / 256.
                assert_eq!(descend(200_000), 200_000 / 256);
            })
            .expect("spawns")
            .join()
            .expect("does not overflow");
    }

    #[test]
    fn a_quiet_catch_returns_the_panic_and_puts_the_flag_back() {
        assert!(catch_quietly(|| panic!("caught quietly")).is_err());
        assert!(!QUIET.with(std::cell::Cell::get));
        assert_eq!(catch_quietly(|| 7).ok(), Some(7));
    }
}
