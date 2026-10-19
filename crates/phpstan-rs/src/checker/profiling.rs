//! hotpath measurements of recursive code. hotpath times each measured call
//! inclusively, so a function measured at every level of a recursion counts
//! the same work once per level, and its allocation stack holds at most 64
//! measured frames. A recursive family is therefore measured at its
//! outermost call only; the breakdown inside it comes from a sampling
//! profiler (README, "Profiling").

/// `$body`, measured by hotpath under `$label` when no call through the same
/// site is already being measured on this thread. Without the `hotpath`
/// feature it is `$body` alone.
macro_rules! measure_outermost {
    ($label:literal, $body:expr) => {{
        #[cfg(feature = "hotpath")]
        {
            thread_local! {
                static ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
            }
            if ACTIVE.get() {
                $body
            } else {
                struct Exit;
                impl Drop for Exit {
                    fn drop(&mut self) {
                        ACTIVE.set(false);
                    }
                }
                ACTIVE.set(true);
                let _exit = Exit;
                // A block is not a literal: a literal label makes hotpath export
                // a symbol to check that the label is unique, which `unsafe_code`
                // denies.
                hotpath::measure_block!({ $label }, $body)
            }
        }
        #[cfg(not(feature = "hotpath"))]
        {
            $body
        }
    }};
}

pub(crate) use measure_outermost;
