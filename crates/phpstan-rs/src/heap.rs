//! The heap measurements of the `dhat-heap` build, which no profile of one
//! run gives alone: dhat keeps one global maximum, which falls in the
//! parse, and its `HeapStats` cannot be reset. So the check's own are
//! taken here:
//! - the live heap at the start of the check, and its maximum over the
//!   check, read after each file's check;
//! - the resident maximum over the check: `5` written to
//!   `/proc/self/clear_refs` at its start, `VmHWM` read at its end.
//!
//! They are printed to stderr at the end of the check. The profiler itself
//! starts with the run, or, with `PHPSTAN_RS_DHAT_FROM_CHECK` set, at the
//! start of the check, so that its maximum and holders are the check's;
//! with `PHPSTAN_RS_DHAT_UNTRIMMED` set it keeps whole backtraces, so that
//! the allocations whose kept frames all lie inside the allocator are
//! placed too. Outside the `dhat-heap` build every function is empty.

#[cfg(feature = "dhat-heap")]
mod measured {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    static PROFILER: Mutex<Option<dhat::Profiler>> = Mutex::new(None);
    static CHECK_START: AtomicU64 = AtomicU64::new(0);
    static CHECK_MAXIMUM: AtomicU64 = AtomicU64::new(0);

    fn start_profiler() {
        let mut builder = dhat::Profiler::builder();
        if std::env::var_os("PHPSTAN_RS_DHAT_UNTRIMMED").is_some() {
            builder = builder.trim_backtraces(None);
        }
        let mut profiler = PROFILER.lock().unwrap_or_else(|e| e.into_inner());
        if profiler.is_none() {
            *profiler = Some(builder.build());
        }
    }

    fn from_check() -> bool {
        std::env::var_os("PHPSTAN_RS_DHAT_FROM_CHECK").is_some()
    }

    fn live() -> u64 {
        dhat::HeapStats::get().curr_bytes as u64
    }

    pub fn start_of_run() {
        if !from_check() {
            start_profiler();
        }
    }

    pub fn end_of_run() {
        // Its `Drop` writes `dhat-heap.json`.
        drop(PROFILER.lock().unwrap_or_else(|e| e.into_inner()).take());
    }

    pub(crate) fn check_started() {
        if from_check() {
            start_profiler();
        }
        let now = live();
        CHECK_START.store(now, Ordering::Relaxed);
        CHECK_MAXIMUM.store(now, Ordering::Relaxed);
        if let Err(e) = std::fs::write("/proc/self/clear_refs", "5") {
            eprintln!("heap: /proc/self/clear_refs: {e}");
        }
    }

    pub(crate) fn file_checked() {
        CHECK_MAXIMUM.fetch_max(live(), Ordering::Relaxed);
    }

    pub(crate) fn check_finished() {
        file_checked();
        let resident = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find_map(|line| line.strip_prefix("VmHWM:"))
                    .map(|value| value.trim().to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());
        eprintln!(
            "heap: live at the check's start {} B, the check's live maximum {} B, its resident maximum {resident}",
            CHECK_START.load(Ordering::Relaxed),
            CHECK_MAXIMUM.load(Ordering::Relaxed),
        );
    }
}

#[cfg(not(feature = "dhat-heap"))]
mod measured {
    pub fn start_of_run() {}
    pub fn end_of_run() {}
    pub(crate) fn check_started() {}
    pub(crate) fn file_checked() {}
    pub(crate) fn check_finished() {}
}

/// Starts the profiler, unless it starts with the check.
pub use measured::start_of_run;

/// Stops the profiler, which writes its profile.
pub use measured::end_of_run;

/// Records the heap at the start of the check, and starts the profiler
/// when it starts there.
pub(crate) use measured::check_started;

/// Records the heap after one file's check.
pub(crate) use measured::file_checked;

/// Prints the check's measurements.
pub(crate) use measured::check_finished;
