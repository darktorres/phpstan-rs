//! A restricted symbol's `allow_in` zone names the checked file by the case
//! rule of the root's directory: any case of its letters where the directory
//! folds case, its own spelling elsewhere.

use super::helpers::all_diagnostics_for_with;
use crate::restricted_symbols::{RestrictedSymbolEntry, RestrictedSymbols};
use diagnostics::CaseRule;

/// How many times `var_dump` is reported in `test.php` when the zone
/// `allow_in` allows it, under `case`.
fn reports_with_zone(allow_in: &str, case: CaseRule) -> usize {
    let entry: RestrictedSymbolEntry = toml::from_str(&format!(
        "kind = \"function\"\nname = \"var_dump\"\nmessage = \"no\"\nallow_in = [\"{allow_in}\"]\n"
    ))
    .unwrap();
    all_diagnostics_for_with("<?php\nvar_dump(1);\n", |settings| {
        settings.restricted_symbols = RestrictedSymbols::compile(&[entry]);
        settings.path_case = case;
    })
    .iter()
    .filter(|d| d.message.contains("no"))
    .count()
}

#[test]
fn a_zone_in_another_case_allows_the_file_under_a_folded_rule() {
    assert_eq!(reports_with_zone("TEST.php", CaseRule::Folded), 0);
}

#[test]
fn a_zone_in_another_case_does_not_allow_the_file_under_an_exact_rule() {
    assert_eq!(reports_with_zone("TEST.php", CaseRule::Exact), 1);
}

#[test]
fn a_zone_in_the_files_own_case_allows_it_under_either_rule() {
    for case in [CaseRule::Exact, CaseRule::Folded] {
        assert_eq!(reports_with_zone("test.php", case), 0, "{case:?}");
    }
}
