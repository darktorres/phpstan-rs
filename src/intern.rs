//! Process-wide string interning for identifier-like token text (Fix 10
//! Stage 1 of the memory-optimization plan - see the plan file's own
//! entry for the full design/rationale). Deduplicates repeated
//! identifier/variable/doc-comment/property-name text across a whole
//! corpus so N occurrences of the same text share one heap allocation
//! instead of each getting its own, *provided* the shared `Arc<str>`
//! representation actually survives into the AST and beyond rather than
//! being re-materialized into a fresh `String` at the very next consumer
//! (a real, previously-confirmed wash/regression otherwise).
//!
//! Backed by a single global `Mutex`-guarded `HashSet`: lexing/parsing
//! genuinely runs in parallel across files via a real `rayon` thread pool
//! (`src/main/check_project.rs`), so this must be thread-safe (`Arc`, not
//! `Rc`), and correct under concurrent first-sight races (see `intern`'s
//! own doc comment). A sharded/thread-local/lock-free design is
//! deliberately NOT attempted here - that's Stage 4's own job, gated on
//! profiling Stages 1-3 first once they've actually landed, not assumed
//! upfront.

use rustc_hash::FxHashSet;
use std::sync::{Arc, Mutex, OnceLock};

fn table() -> &'static Mutex<FxHashSet<Arc<str>>> {
    static TABLE: OnceLock<Mutex<FxHashSet<Arc<str>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(FxHashSet::default()))
}

/// Returns a shared `Arc<str>` for `s`'s own text. The first call for a
/// given piece of text allocates it once and stores it in the shared
/// table; every later call for identical text (from any thread) gets a
/// cheap refcount-bumped clone of that same allocation instead of a new
/// one of its own.
///
/// The lookup, allocation (on a first sighting) and insertion all happen
/// under one single lock acquisition - not a separate "check, then
/// insert" pair of lock acquisitions - specifically so two threads
/// racing to intern the same brand-new text can't both decide "not
/// present yet" and each insert their own separate allocation; the
/// second one to actually acquire the lock always sees the first one's
/// entry and reuses it instead. Still brief: a hash lookup plus, only on
/// a first sighting, one `Arc::from` allocation - no blocking work of any
/// kind happens while the lock is held.
pub(crate) fn intern(s: &str) -> Arc<str> {
    let mut table = table().lock().unwrap();
    if let Some(existing) = table.get(s) {
        return Arc::clone(existing);
    }
    let arc: Arc<str> = Arc::from(s);
    table.insert(Arc::clone(&arc));
    arc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interns_identical_text_to_the_same_allocation() {
        let a = intern("hello");
        let b = intern("hello");
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn distinct_text_gets_distinct_allocations() {
        let a = intern("hello");
        let b = intern("world");
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(&*a, "hello");
        assert_eq!(&*b, "world");
    }

    #[test]
    fn concurrent_interning_of_the_same_new_text_converges_on_one_allocation() {
        use std::thread;

        // A text guaranteed not to have been interned by any earlier test
        // in this process (tests share the one global table).
        let text = "concurrent-intern-test-marker-xyz";
        let handles: Vec<_> = (0..8)
            .map(|_| thread::spawn(move || intern(text)))
            .collect();
        let results: Vec<Arc<str>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let first = &results[0];
        for r in &results[1..] {
            assert!(Arc::ptr_eq(first, r));
        }
    }
}
