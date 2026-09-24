//! Built-in string function return type inference (str_replace, sprintf, explode, etc).

use super::helpers::*;

#[test]
fn str_replace_falls_back_to_the_generic_path_for_a_non_string_subject() {
    // Scoped to "subject is exactly Type::String" - a Mixed subject
    // (or a real array one) must keep today's existing, unaffected
    // generic-path behavior rather than the new special case firing.
    let d = diagnostics_for(
        r#"<?php
            function f($subject) {
                $result = str_replace('a', 'b', $subject);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
