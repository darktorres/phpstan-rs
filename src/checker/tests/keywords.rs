//! `break`/`continue`, `goto`/labels, and `declare` directives.

use super::helpers::*;

fn ids(d: &[crate::Diagnostic]) -> Vec<&str> {
    let mut v: Vec<&str> = d
        .iter()
        .map(|x| x.identifier.as_str())
        .filter(|id| {
            id.starts_with("break.")
                || id.starts_with("continue.")
                || id.starts_with("goto.")
                || id.starts_with("label.")
                || id.starts_with("declare")
        })
        .collect();
    v.sort_unstable();
    v
}

#[test]
fn break_and_continue_levels_are_checked() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $xs): void {
                break;
                foreach ($xs as $x) {
                    break 2;
                    switch ($x) {
                        case 1:
                            continue;
                        case 2:
                            continue 2;
                    }
                    break 0;
                    $c = function () { continue; };
                }
            }
        "#,
    );
    assert_eq!(
        ids(&d),
        vec![
            "break.invalidLevel",
            "break.outOfLoop",
            "break.outOfLoop",
            "continue.outOfLoop",
            "continue.switch",
        ],
        "got: {d:?}"
    );
}

#[test]
fn goto_targets_are_checked() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $xs): void {
                goto nowhere;
                goto inside;
                foreach ($xs as $x) {
                    inside:
                    goto out;
                }
                out:
                unused:
                dup:
                dup:
                try {
                    goto fin;
                } finally {
                    fin:
                    goto out;
                }
            }
        "#,
    );
    assert_eq!(
        ids(&d),
        vec![
            "goto.intoFinally",
            "goto.intoLoop",
            "goto.labelUndefined",
            "goto.outOfFinally",
            "label.duplicate",
            "label.unused",
            "label.unused",
        ],
        "got: {d:?}"
    );
}

#[test]
fn declare_directives_are_checked() {
    let d = diagnostics_for(
        r#"<?php
            declare(strict_types=2);
            declare(strict_types=1);
            declare(ticks='x');
            declare(encoding=1);
            declare(nonsense=1);
            declare(strict_types=1) { echo 1; }
        "#,
    );
    assert_eq!(
        ids(&d),
        vec![
            "declare.encodingValue",
            "declare.ticksValue",
            "declare.unsupported",
            "declareStrictTypes.blockMode",
            "declareStrictTypes.notFirst",
            "declareStrictTypes.value",
        ],
        "got: {d:?}"
    );
    let first = diagnostics_for("<?php\ndeclare(strict_types=1);\necho 1;\n");
    assert!(ids(&first).is_empty(), "got: {first:?}");
}
