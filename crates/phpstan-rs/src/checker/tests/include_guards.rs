//! A path a file tests with `file_exists()` and friends guards the includes of
//! that file by the case rule of the root's directory: any case of its
//! letters where the directory folds case, its own spelling elsewhere.

use super::helpers::all_diagnostics_for_with;
use diagnostics::CaseRule;

/// The includes of `source` that are reported as naming no file, under `case`.
fn missing_includes(source: &str, case: CaseRule) -> usize {
    all_diagnostics_for_with(source, |settings| settings.path_case = case)
        .iter()
        .filter(|d| d.message.contains("is not a file or it does not exist"))
        .count()
}

const GUARDED_IN_ANOTHER_CASE: &str =
    "<?php\nif (file_exists('/no/such/Config.php')) {\n    require '/no/such/config.php';\n}\n";

const GUARDED_IN_THE_SAME_CASE: &str =
    "<?php\nif (file_exists('/no/such/config.php')) {\n    require '/no/such/config.php';\n}\n";

#[test]
fn a_guard_in_another_case_guards_the_include_under_a_folded_rule() {
    assert_eq!(
        missing_includes(GUARDED_IN_ANOTHER_CASE, CaseRule::Folded),
        0
    );
}

#[test]
fn a_guard_in_another_case_does_not_guard_the_include_under_an_exact_rule() {
    assert_eq!(
        missing_includes(GUARDED_IN_ANOTHER_CASE, CaseRule::Exact),
        1
    );
}

#[test]
fn a_guard_in_the_same_case_guards_the_include_under_either_rule() {
    for case in [CaseRule::Exact, CaseRule::Folded] {
        assert_eq!(
            missing_includes(GUARDED_IN_THE_SAME_CASE, case),
            0,
            "{case:?}"
        );
    }
}

#[test]
fn an_include_nothing_guards_is_reported_under_either_rule() {
    let source = "<?php\nrequire '/no/such/config.php';\n";
    for case in [CaseRule::Exact, CaseRule::Folded] {
        assert_eq!(missing_includes(source, case), 1, "{case:?}");
    }
}
