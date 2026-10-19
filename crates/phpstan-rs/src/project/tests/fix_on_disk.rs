//! A fix is written only to the bytes its analysis read: a file changed since
//! then keeps its bytes, and the fixes of the files that did not change still
//! apply.

use super::super::session::apply_fixes_to_disk;
use super::helpers::{TempDir, resolved, session};
use crate::project::source::content_hash;
use diagnostics::{Diagnostic, DiagnosticBuilder};
use rustc_hash::FxHashMap;
use std::path::Path;
use syntax::span::{LineCol, Range, Span};

const ORIGINAL: &str = "<?php\nfoo();\n";
/// `ORIGINAL` with a line added above the call, so that the bytes of
/// `ORIGINAL`'s fix land elsewhere in it.
const CHANGED: &str = "<?php\n\nfoo();\n";

/// A diagnostic whose fix renames the `foo` of `ORIGINAL`.
fn rename_foo() -> Diagnostic {
    DiagnosticBuilder::new("call to foo()".to_string(), Span::default(), 0, "test.fix")
        .fix(Span { start: 6, end: 9 }, "bar", "rename foo")
        .build_at(Range {
            start: LineCol { line: 2, col: 1 },
            end: LineCol { line: 2, col: 4 },
        })
}

#[test]
fn a_file_changed_since_it_was_analysed_keeps_its_bytes() {
    let dir = TempDir::new("fix_changed");
    let path = resolved(&dir).join("proj/a.php");
    dir.write("proj/a.php", ORIGINAL);
    let digests = FxHashMap::from_iter([(path.clone(), content_hash(ORIGINAL.as_bytes()))]);
    dir.write("proj/a.php", CHANGED);
    let results = FxHashMap::from_iter([(path.clone(), vec![rename_foo()])]);

    let outcome = apply_fixes_to_disk(&results, &digests);

    assert_eq!(std::fs::read_to_string(&path).unwrap(), CHANGED);
    assert_eq!(outcome.changed_since_analysis, vec![path]);
    assert_eq!(outcome.fixed_files, 0);
    assert_eq!(outcome.applied, 0);
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
}

#[test]
fn a_file_that_did_not_change_is_fixed_beside_one_that_did() {
    let dir = TempDir::new("fix_mixed");
    let stable = resolved(&dir).join("proj/stable.php");
    let edited = resolved(&dir).join("proj/edited.php");
    dir.write("proj/stable.php", ORIGINAL);
    dir.write("proj/edited.php", ORIGINAL);
    let digests = FxHashMap::from_iter([
        (stable.clone(), content_hash(ORIGINAL.as_bytes())),
        (edited.clone(), content_hash(ORIGINAL.as_bytes())),
    ]);
    dir.write("proj/edited.php", CHANGED);
    let results = FxHashMap::from_iter([
        (stable.clone(), vec![rename_foo()]),
        (edited.clone(), vec![rename_foo()]),
    ]);

    let outcome = apply_fixes_to_disk(&results, &digests);

    assert_eq!(std::fs::read_to_string(&stable).unwrap(), "<?php\nbar();\n");
    assert_eq!(std::fs::read_to_string(&edited).unwrap(), CHANGED);
    assert_eq!(outcome.changed_since_analysis, vec![edited]);
    assert_eq!(outcome.fixed_files, 1);
    assert_eq!(outcome.applied, 1);
}

#[test]
fn a_session_keeps_the_digest_of_each_analysed_file_only_when_asked() {
    let dir = TempDir::new("read_digests");
    dir.write("proj/a.php", ORIGINAL);
    let root = resolved(&dir).join("proj");
    let mut session = session(&root, Vec::new());
    assert!(session.run(false).unwrap().read_digests().is_empty());

    session.keep_read_digests();
    let digests = session.run(false).unwrap().read_digests();

    assert_eq!(
        digests.get(Path::new("a.php")),
        Some(&content_hash(ORIGINAL.as_bytes()))
    );
}
