//! `textDocument/didChange`'s own coalescing/cancellation bookkeeping -
//! a pure, clock-injected state machine with zero locking/threading of
//! its own. The `Arc<Mutex<...>>`/`Condvar` wrapping that makes this
//! safe to share between the reader thread (which only ever calls
//! `record_edit`) and the debounce worker thread (which owns the timer
//! and calls everything else) lives in `lsp.rs`, around a plain
//! `DebounceQueue` - keeping this module trivially unit-testable with
//! fake `Instant`s and no real sleeping at all.

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lsp_types::Uri;

/// One path's own most-recently-recorded, not-yet-fired edit.
#[derive(Clone)]
pub(super) struct PendingEdit {
    pub(super) uri: Uri,
    pub(super) text: String,
    pub(super) version: i32,
    pub(super) generation: u64,
    deadline: Instant,
}

/// `generations` is deliberately never pruned except by `cancel` - one
/// entry per path this session has ever edited or explicitly canceled,
/// bounded by project size (how many distinct files a real editing
/// session touches), not by edit count.
pub(super) struct DebounceQueue {
    pending: FxHashMap<PathBuf, PendingEdit>,
    generations: FxHashMap<PathBuf, u64>,
    debounce: Duration,
    /// When the whole-program checks are due, set by a save.
    whole_program: Option<Instant>,
}

impl DebounceQueue {
    pub(super) fn new(debounce: Duration) -> Self {
        DebounceQueue {
            pending: FxHashMap::default(),
            generations: FxHashMap::default(),
            debounce,
            whole_program: None,
        }
    }

    /// Coalesces `path`'s own pending edit (if any) into this newer one,
    /// bumping its generation - a not-yet-fired recheck for the OLD text
    /// is superseded before it ever runs, and an ALREADY-running recheck
    /// for the old text observes the bump on its next `should_abort`
    /// check. Generations start at 1, never 0 - `current_generation`
    /// reserves 0 to mean "no entry for this path at all," so a captured
    /// `my_generation >= 1` can never spuriously match an absent/
    /// canceled path.
    pub(super) fn record_edit(
        &mut self,
        path: PathBuf,
        uri: Uri,
        text: String,
        version: i32,
        now: Instant,
    ) -> u64 {
        let generation = self.generations.get(&path).copied().unwrap_or(0) + 1;
        self.generations.insert(path.clone(), generation);
        self.pending.insert(
            path,
            PendingEdit {
                uri,
                text,
                version,
                generation,
                deadline: now + self.debounce,
            },
        );
        generation
    }

    /// The earliest deadline among every still-pending edit - `None`
    /// when nothing is pending at all, letting the worker wait
    /// indefinitely (up to its own idle cap) rather than busy-poll.
    ///
    /// The whole-program checks' deadline counts only while no edit is
    /// pending: they run when the typing has stopped.
    pub(super) fn next_deadline(&self) -> Option<Instant> {
        match self.pending.values().map(|p| p.deadline).min() {
            Some(deadline) => Some(deadline),
            None => self.whole_program,
        }
    }

    /// Asks for the whole-program checks to run once no edit is pending,
    /// a debounce window after the last such request.
    pub(super) fn request_whole_program(&mut self, now: Instant) {
        self.whole_program = Some(now + self.debounce);
    }

    /// Whether the whole-program checks were requested, due or not; clears
    /// the request.
    pub(super) fn take_whole_program_now(&mut self) -> bool {
        self.whole_program.take().is_some()
    }

    /// Whether the whole-program checks are due at `now`, which they are
    /// once requested, with no edit pending and the window elapsed;
    /// clears the request.
    pub(super) fn take_whole_program(&mut self, now: Instant) -> bool {
        match self.whole_program {
            Some(deadline) if deadline <= now && self.pending.is_empty() => {
                self.whole_program = None;
                true
            }
            _ => false,
        }
    }

    /// Drains and returns every entry whose deadline is at or before
    /// `now` - a still-fresh entry (still inside its own debounce
    /// window) is left in place.
    pub(super) fn drain_expired(&mut self, now: Instant) -> Vec<(PathBuf, PendingEdit)> {
        let expired: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, p)| p.deadline <= now)
            .map(|(path, _)| path.clone())
            .collect();
        expired
            .into_iter()
            .filter_map(|path| self.pending.remove(&path).map(|p| (path, p)))
            .collect()
    }

    /// Drains every still-pending entry regardless of its own deadline -
    /// the shutdown-flush's own "don't lose an edit just because the
    /// session ended before its debounce window elapsed" guarantee.
    pub(super) fn drain_all(&mut self) -> Vec<(PathBuf, PendingEdit)> {
        self.pending.drain().collect()
    }

    /// `0` for a path with no entry at all (never recorded, or
    /// explicitly `cancel`ed) - safe to compare against a captured
    /// generation (always `>= 1`) without a separate "does this path
    /// even have one" check.
    pub(super) fn current_generation(&self, path: &Path) -> u64 {
        self.generations.get(path).copied().unwrap_or(0)
    }

    /// Drops `path` from both maps - not just its pending edit, but its
    /// generation entry too. This makes an in-flight recheck for a
    /// since-closed file abort automatically: a `should_abort` closure
    /// capturing `my_generation >= 1` compares against
    /// `current_generation`'s now-`0` answer and never matches again.
    pub(super) fn cancel(&mut self, path: &Path) {
        self.pending.remove(path);
        self.generations.remove(path);
    }
}

#[cfg(test)]
mod tests;
