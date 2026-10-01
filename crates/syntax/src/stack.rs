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
            let outer = LIMIT.with(std::cell::Cell::get);
            let out = stacker::grow(SEGMENT, || {
                // A new segment: the first guarded call on it looks again.
                LIMIT.with(|limit| limit.set(usize::MAX));
                f()
            });
            LIMIT.with(|limit| limit.set(outer));
            out
        }
        Some(remaining) => {
            LIMIT.with(|limit| limit.set(here - (remaining - RED_ZONE)));
            f()
        }
        None => f(),
    }
}

#[cfg(test)]
mod tests {
    use super::guard;

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
}
