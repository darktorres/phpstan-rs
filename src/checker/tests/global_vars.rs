//! `Registry::global_variables` - a `global $x;`-declared variable now
//! resolves to its own inferred type instead of unconditionally falling
//! back to `Mixed`, so checks that already exist (`method.notFound`,
//! `isset.offset`/`shapeAccess.notFound`) start firing correctly on it.
use super::helpers::*;

#[test]
fn a_call_to_a_real_method_on_an_inferred_global_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Template {
                function assign($name, $value) {}
            }
            $template = new Template();
            function render() {
                global $template;
                $template->assign('x', 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_call_to_a_nonexistent_method_on_an_inferred_global_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Template {
                function assign($name, $value) {}
            }
            $template = new Template();
            function render() {
                global $template;
                $template->assing('x', 1);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "method.notFound");
}

#[test]
fn a_shape_inferred_global_flags_a_nonexistent_offset() {
    let d = diagnostics_for(
        r#"<?php
            $conf['language'] = 'en';
            function f() {
                global $conf;
                if (isset($conf['langauge'])) {
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "isset.offset");
}

#[test]
fn a_shape_inferred_globals_real_offset_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            $conf['language'] = 'en';
            function f() {
                global $conf;
                if (isset($conf['language'])) {
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
