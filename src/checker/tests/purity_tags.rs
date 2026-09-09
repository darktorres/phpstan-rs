//! Tests for `@phpstan-impure` tag validation
//! (`checker::noop::check_declared_purity_tag`).
use super::helpers::*;

#[test]
fn a_function_declared_impure_but_provably_pure_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @phpstan-impure */
            function addOne(int $x): int {
                return $x + 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "impureFunction.pure"),
        "got: {d:?}"
    );
}

#[test]
fn a_method_declared_impure_but_provably_pure_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            final class Calc {
                /** @phpstan-impure */
                public function addOne(int $x): int {
                    return $x + 1;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "impureMethod.pure"),
        "got: {d:?}"
    );
}

#[test]
fn a_function_declared_impure_that_really_is_impure_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @phpstan-impure */
            function logIt(string $msg): void {
                echo $msg;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "impureFunction.pure"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_provably_pure_function_with_no_purity_tag_at_all_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            function addOne(int $x): int {
                return $x + 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "impureFunction.pure"),
        "unexpected diagnostics: {d:?}"
    );
}
