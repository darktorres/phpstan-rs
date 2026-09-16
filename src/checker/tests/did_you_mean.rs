//! `method.notFound`'s "did you mean?" tip (`checker::suggest`, PHASE_BACKLOG
//! D8) - a close-typo call site gets a name suggestion, an unrelated one
//! doesn't.

use super::helpers::*;

#[test]
fn a_close_typo_method_call_gets_a_suggestion_tip() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public function getX(): int { return 0; }
            }
            function test(Point $p): int {
                return $p->getx_();
            }
        "#,
    );
    let hit = d
        .iter()
        .find(|x| x.identifier == "method.notFound")
        .unwrap_or_else(|| panic!("got: {d:?}"));
    assert_eq!(hit.tip.as_deref(), Some("Did you mean getX()?"));
}

#[test]
fn an_unrelated_method_name_gets_no_suggestion_tip() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public function getX(): int { return 0; }
            }
            function test(Point $p): int {
                return $p->totallyUnrelatedThing();
            }
        "#,
    );
    let hit = d
        .iter()
        .find(|x| x.identifier == "method.notFound")
        .unwrap_or_else(|| panic!("got: {d:?}"));
    assert_eq!(hit.tip, None);
}
