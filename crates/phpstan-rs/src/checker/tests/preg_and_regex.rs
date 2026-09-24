//! preg_match/preg_match_all/preg_replace capture-group and result typing.

use super::helpers::*;

// The `x` (free-spacing) modifier no longer bails the whole parse -
// unescaped whitespace and `#`-comments between tokens are skipped, the
// same way PCRE's own free-spacing mode does.
#[test]
fn preg_match_understands_a_free_spacing_pattern_with_comments() {
    // The pattern has to appear directly as the call argument, not
    // through an intermediate variable - `literal_string_value` only
    // ever looks at the raw `Expr` passed to it, with no access to
    // inferred types/`Locals` (same reasoning as its own `Concat`
    // folding, see `ast::literal_string_value`'s own
    // doc comment).
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/
                    (\d+)   # the leading number
                    \s*
                    ([a-z]+) # the trailing word
                /x', $subject, $matches)) {
                    needsInt($matches[1]);
                    needsInt($matches[2]);
                }
            }
        "#,
    );
    assert_eq!(d.len(), 2, "got: {d:?}");
    assert!(
        d[0].message.contains("needsInt") && d[0].message.ends_with("numeric-string given."),
        "expected group 1 as numeric-string, got: {d:?}"
    );
    assert!(
        d[1].message.contains("needsInt") && d[1].message.ends_with("non-empty-string given."),
        "expected group 2 as non-empty-string, got: {d:?}"
    );
}
