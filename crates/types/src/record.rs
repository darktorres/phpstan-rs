//! The unions a run builds, recorded while asked: each one's operands, in
//! order, whether the class hierarchy joined them, and its result, as
//! canonical prints. A union is recorded where it is asked for, above the
//! memo that may answer it, so the set does not depend on which thread
//! built it first; it is one set for the run, since a docblock's or a
//! type's memo builds it in whichever file needs it first.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use rustc_hash::FxHashSet;

use crate::Type;

static RECORDING: AtomicBool = AtomicBool::new(false);

static RECORDED: Mutex<Option<FxHashSet<String>>> = Mutex::new(None);

/// Starts recording the unions built, forgetting any recorded before.
pub fn record_unions() {
    let mut recorded = RECORDED.lock().unwrap_or_else(|e| e.into_inner());
    *recorded = Some(FxHashSet::default());
    RECORDING.store(true, Ordering::Release);
}

/// Stops recording, and gives the unions recorded, one line each, sorted:
/// `[A, B] => R`, with `hierarchy ` before it when the class hierarchy
/// joined them.
pub fn recorded_unions() -> Vec<String> {
    RECORDING.store(false, Ordering::Release);
    let mut recorded = RECORDED.lock().unwrap_or_else(|e| e.into_inner());
    let mut lines: Vec<String> = recorded.take().unwrap_or_default().into_iter().collect();
    lines.sort_unstable();
    lines
}

/// Whether unions are being recorded.
#[inline]
pub(crate) fn recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

/// Records that the union of `operands` is `result`, when recording.
#[inline]
pub(crate) fn note_union(operands: &[&Type], hierarchy: bool, result: &Type) {
    if recording() {
        note(operands, hierarchy, result);
    }
}

#[cold]
fn note(operands: &[&Type], hierarchy: bool, result: &Type) {
    let mut line = String::new();
    if hierarchy {
        line.push_str("hierarchy ");
    }
    line.push('[');
    for (i, operand) in operands.iter().enumerate() {
        if i > 0 {
            line.push_str(", ");
        }
        crate::canonical::write_type(operand, &mut line);
    }
    line.push_str("] => ");
    crate::canonical::write_type(result, &mut line);
    let mut recorded = RECORDED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(recorded) = recorded.as_mut() {
        recorded.insert(line);
    }
}
