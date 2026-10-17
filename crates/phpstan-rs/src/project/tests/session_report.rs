//! A report is the same whether its analysis is finalized by moving the
//! results or by copying them, and a copy leaves the analysis to be finalized
//! again. The gate runner takes its several views of one analysis this way.

use super::super::session::{DiagnosticOrder, Finalize};
use super::helpers::{TempDir, resolved, session};

#[test]
fn a_copied_report_is_the_moved_one_and_the_analysis_stays_whole() {
    let dir = TempDir::new("finalize_copy");
    dir.write("proj/a.php", "<?php function g() { return $undefinedVar; }");
    let root = resolved(&dir).join("proj");
    let session = session(&root, Vec::new());
    let sorted = session.finalize_options();
    let emission = Finalize {
        order: DiagnosticOrder::Emission,
        ..session.finalize_options()
    };

    let analysis = session.run(false).unwrap();
    let copied = analysis.finalize_copy(&sorted);
    let copied_again = analysis.finalize_copy(&sorted);
    let copied_emission = analysis.finalize_copy(&emission);
    let moved = analysis.finalize(&sorted);
    let moved_emission = session.run(false).unwrap().finalize(&emission);

    assert!(
        moved
            .results
            .values()
            .flatten()
            .any(|d| d.message.contains("undefined variable")),
        "expected the undefined variable to be reported, got: {:?}",
        moved.results
    );
    assert_eq!(copied.results, moved.results);
    assert_eq!(copied_again.results, moved.results);
    assert_eq!(copied_emission.results, moved_emission.results);
}
