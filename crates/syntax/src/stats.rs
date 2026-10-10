//! Counters for the memos and caches whose worth is a hit rate, printed at the
//! end of a run with `PHPSTAN_RS_MEMO_STATS=1`. They exist only with the
//! `memo-stats` feature: without it every counter call does nothing.
//!
//! A counter is a cell its thread alone writes, so an event costs a load
//! and a store: no lock, no atomic read-modify-write, no allocation. Each
//! thread's cells are registered once, and the report sums them, so a
//! worker that never exits is counted too.

#[cfg(feature = "memo-stats")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "memo-stats")]
use std::sync::{Arc, Mutex};

macro_rules! counters {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        /// One counted event. A name `site.hit` and a name `site.miss` are
        /// reported together as a rate.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Counter {
            $($variant),+
        }

        #[cfg_attr(not(feature = "memo-stats"), allow(dead_code))]
        const NAMES: &[&str] = &[$($name),+];
        #[cfg_attr(not(feature = "memo-stats"), allow(dead_code))]
        const COUNT: usize = NAMES.len();
    };
}

counters! {
    OperationsHit => "operations.hit",
    OperationsMiss => "operations.miss",
    ClassTemplateParamsHit => "class_template_params.hit",
    ClassTemplateParamsMiss => "class_template_params.miss",
    ReturnedHolderHit => "returned_holder.hit",
    ReturnedHolderMiss => "returned_holder.miss",
    ImplicitMethodHit => "implicit_method.hit",
    ImplicitMethodMiss => "implicit_method.miss",
    RegexPatternHit => "regex_pattern.hit",
    RegexPatternMiss => "regex_pattern.miss",
    ParseSqlHit => "parse_sql.hit",
    ParseSqlMiss => "parse_sql.miss",
    RejectedFromHit => "rejected_from.hit",
    RejectedFromMiss => "rejected_from.miss",
    ToStringTypeHit => "to_string_type.hit",
    ToStringTypeMiss => "to_string_type.miss",
    DescendantsHit => "descendants.hit",
    DescendantsMiss => "descendants.miss",
    TemplatePathHit => "template_path.hit",
    TemplatePathMiss => "template_path.miss",
    CalleeNodeShared => "callee_node.hit",
    CalleeNodeWritten => "callee_node.miss",
    VendorAutoloadHit => "vendor_autoload.hit",
    VendorAutoloadMiss => "vendor_autoload.miss",
    VendorWaitNanos => "vendor_autoload.wait_ns",
    DiagnosticsBuilt => "diagnostics.built",
    DiagnosticsBelowLevel => "diagnostics.below_level",
    ResolveRuns => "resolve_run.bodies",
    ResolveRounds => "resolve_run.rounds",
    ResolveLevelsBuilt => "resolve_run.levels_built",
    ResolveLevelsReused => "resolve_run.levels_reused",
}

#[cfg(feature = "memo-stats")]
struct Cells([AtomicU64; COUNT]);

#[cfg(feature = "memo-stats")]
static THREADS: Mutex<Vec<Arc<Cells>>> = Mutex::new(Vec::new());

#[cfg(feature = "memo-stats")]
thread_local! {
    static LOCAL: Arc<Cells> = {
        let cells = Arc::new(Cells([const { AtomicU64::new(0) }; COUNT]));
        THREADS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Arc::clone(&cells));
        cells
    };
}

/// Counts `by` events of `counter` on this thread.
#[cfg(feature = "memo-stats")]
#[inline]
pub fn add(counter: Counter, by: u64) {
    let _ = LOCAL.try_with(|cells| {
        let cell = &cells.0[counter as usize];
        cell.store(cell.load(Ordering::Relaxed) + by, Ordering::Relaxed);
    });
}

/// Counts `by` events of `counter` on this thread: nothing without the
/// `memo-stats` feature.
#[cfg(not(feature = "memo-stats"))]
#[inline]
pub fn add(_counter: Counter, _by: u64) {}

/// Counts one event of `counter`.
#[inline]
pub fn count(counter: Counter) {
    add(counter, 1);
}

/// The sum of every thread's count of `counter`; zero without the
/// `memo-stats` feature.
#[cfg(feature = "memo-stats")]
pub fn total(counter: Counter) -> u64 {
    THREADS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|cells| cells.0[counter as usize].load(Ordering::Relaxed))
        .sum()
}

/// The sum of every thread's count of `counter`; zero without the
/// `memo-stats` feature.
#[cfg(not(feature = "memo-stats"))]
pub fn total(_counter: Counter) -> u64 {
    0
}

/// Every counter that was counted, as `(name, count)` in declaration
/// order, then each `site.hit`/`site.miss` pair as `site.rate` in percent.
/// Empty without the `memo-stats` feature.
#[cfg(not(feature = "memo-stats"))]
pub fn table() -> Vec<(String, u64)> {
    Vec::new()
}

/// Every counter that was counted, as `(name, count)` in declaration
/// order, then each `site.hit`/`site.miss` pair as `site.rate` in percent.
#[cfg(feature = "memo-stats")]
pub fn table() -> Vec<(String, u64)> {
    let mut sums = [0u64; COUNT];
    for cells in THREADS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
    {
        for (sum, cell) in sums.iter_mut().zip(&cells.0) {
            *sum += cell.load(Ordering::Relaxed);
        }
    }
    let mut rows = Vec::new();
    for (name, &count) in NAMES.iter().zip(&sums) {
        if count != 0 {
            rows.push(((*name).to_owned(), count));
        }
    }
    for (index, name) in NAMES.iter().enumerate() {
        let Some(site) = name.strip_suffix(".hit") else {
            continue;
        };
        let hits = sums[index];
        let misses = NAMES
            .iter()
            .position(|other| other.strip_suffix(".miss") == Some(site))
            .map_or(0, |at| sums[at]);
        if let Some(rate) = (hits * 100).checked_div(hits + misses) {
            rows.push((format!("{site}.rate_percent"), rate));
        }
    }
    rows
}

#[cfg(all(test, feature = "memo-stats"))]
mod tests {
    use super::*;

    #[test]
    fn threads_are_summed_and_rates_reported() {
        let before = total(Counter::RejectedFromHit);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..3 {
                        count(Counter::RejectedFromHit);
                    }
                    count(Counter::RejectedFromMiss);
                });
            }
        });
        assert_eq!(total(Counter::RejectedFromHit) - before, 12);
        let rows = table();
        assert!(
            rows.iter()
                .any(|(name, _)| name == "rejected_from.rate_percent")
        );
    }
}
