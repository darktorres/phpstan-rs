//! Conditional declarations and stub merging for classes and functions.

use super::super::*;
use super::helpers::*;
use crate::lexer::Lexer;
use crate::parser::Parser;

#[test]
fn function_declared_inside_an_if_guard_is_still_indexed() {
    let r = build_registry(
        r#"<?php
            if (!function_exists('polyfilled')) {
                function polyfilled() {}
            }"#,
    );
    assert!(r.lookup_function("polyfilled").is_some());
}

#[test]
fn unconditional_class_wins_over_a_later_conditional_stub() {
    // Real shape: `vendor/rector/rector/stubs-rector/PHPUnit/Framework/
    // TestCase.php` guards a minimal fallback `TestCase` (no `extends
    // Assert`) behind `if (!class_exists(...))`, declared *after* the
    // real `vendor/phpunit/phpunit/.../TestCase.php` in file-sort
    // order - the real class must still win.
    let r = build_registry(
        r#"<?php
            class X { public function real() {} }
            if (!class_exists('X')) {
                class X { public function stub() {} }
            }"#,
    );
    assert!(r.classes["X"].methods.contains_key("real"));
    assert!(!r.classes["X"].methods.contains_key("stub"));
}

#[test]
fn unconditional_class_wins_over_an_earlier_conditional_stub() {
    // Same collision, opposite file-processing order - the fix must
    // be order-independent, not just "unconditional-after-conditional"
    // happens to work by accident of insertion order.
    let r = build_registry(
        r#"<?php
            if (!class_exists('X')) {
                class X { public function stub() {} }
            }
            class X { public function real() {} }"#,
    );
    assert!(r.classes["X"].methods.contains_key("real"));
    assert!(!r.classes["X"].methods.contains_key("stub"));
}

// Phase 25, real shape: Piwigo's own `phpstan-stubs/Imagick.stub.php`,
// wired into `phpstan.neon` as a PHPStan `stubFiles` entry - a bare
// `class Imagick { public function convolveImage(...) {} }` meant to
// override *one* method of the real, already-known `Imagick` class
// from `jetbrains/phpstorm-stubs`. Both declarations are unconditional
// - the fix must merge them, not let the second one wholesale-replace
// the first and silently drop every other real method/constant.
#[test]
fn two_unconditional_class_declarations_merge_instead_of_replacing() {
    let r = build_registry(
        r#"<?php
            class Imagick {
                const FILTER_LANCZOS = 1;
                public function getImageWidth() {}
                public function cropImage($width, $height, $x, $y) {}
                public function convolveImage($kernel) {}
            }
            class Imagick {
                public function convolveImage($kernel, $channel = 1) {}
            }"#,
    );
    assert!(r.classes["Imagick"].methods.contains_key("getImageWidth"));
    assert!(r.classes["Imagick"].methods.contains_key("cropImage"));
    assert!(r.classes["Imagick"].consts.contains_key("FILTER_LANCZOS"));
    // The override's own signature wins for the one method both share.
    assert_eq!(
        r.classes["Imagick"].methods["convolveImage"].params.len(),
        2,
        "the second declaration's own convolveImage() (2 params) should win over the first's (1 param)"
    );
}

#[test]
fn two_unconditional_class_declarations_with_disjoint_methods_union_both() {
    // Order-independent, and not specific to an override-shaped
    // (single-method) second declaration - two genuinely different,
    // non-overlapping method sets under the same name still end up
    // with the union of both, either direction.
    let r = build_registry(
        r#"<?php
            class X { public function fromFirst() {} }
            class X { public function fromSecond() {} }"#,
    );
    assert!(r.classes["X"].methods.contains_key("fromFirst"));
    assert!(r.classes["X"].methods.contains_key("fromSecond"));
}

#[test]
fn a_merged_class_keeps_its_extends_when_the_second_declaration_has_none() {
    // The real Imagick shape: the override redeclares the class bare
    // (no `extends`/`implements` of its own) - it must not blank out
    // real inheritance information the first declaration carried.
    let r = build_registry(
        r#"<?php
            class Base {}
            class X extends Base { public function fromFirst() {} }
            class X { public function fromSecond() {} }"#,
    );
    assert_eq!(r.classes["X"].extends, vec!["Base".to_string()]);
    assert!(r.classes["X"].methods.contains_key("fromFirst"));
    assert!(r.classes["X"].methods.contains_key("fromSecond"));
}

#[test]
fn re_indexing_an_unconditional_class_after_removal_does_not_panic() {
    // Found while implementing the merge fix above:
    // `remove_declarations` (incremental re-checking's own removal
    // step, `Project::update_file`) cleared `classes` but left a
    // stale "already unconditionally declared" marker behind, so
    // re-indexing the same file after a change hit the merge branch
    // against an entry `remove_declarations` had already deleted -
    // panicking instead of cleanly re-inserting.
    let src = "<?php class X { public function original() {} }";
    let tokens = Lexer::new(src).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    let mut r = Registry::build(&program);
    assert!(r.classes["X"].methods.contains_key("original"));

    r.remove_declarations(&[], &["X".to_string()]);
    assert!(!r.classes.contains_key("X"));

    r.index(&program);
    assert!(r.classes["X"].methods.contains_key("original"));
}

// Finding C (Phase 32): two unconditional declarations of the same
// function name overwrite instead of merging - the function-side
// sibling of Phase 25's class-merge fix. Real shape: `strtr`'s own
// real stub declares a 3-string-argument overload and a 2-argument,
// `array $replace_pairs` overload as two separate top-level
// declarations sharing one name.
#[test]
fn two_unconditional_function_declarations_with_different_arities_merge() {
    let r = build_registry(
        r#"<?php
            function strtr(string $string, string $from, string $to): string {}
            function strtr(string $str, array $replace_pairs): string {}
        "#,
    );
    let sig = &r.functions["strtr"];
    assert_eq!(sig.params.len(), 3);
    assert_eq!(sig.params[0].name, "str", "later declaration's name wins");
    assert_eq!(sig.params[0].ty, Type::String);
    assert!(!sig.params[0].has_default);
    assert_eq!(sig.params[1].name, "replace_pairs");
    assert_eq!(
        sig.params[1].ty,
        Type::Union(vec![
            Type::String,
            Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed))
        ])
    );
    assert!(!sig.params[1].has_default);
    assert_eq!(sig.params[2].name, "to");
    assert_eq!(sig.params[2].ty, Type::String);
    assert!(
        sig.params[2].has_default,
        "a position only the first overload has can't be required in the merged callable"
    );
}

#[test]
fn two_unconditional_function_declarations_with_a_disjoint_type_at_one_position_union_it() {
    let r = build_registry(
        r#"<?php
            function f(int $x): int {}
            function f(string $x): string {}
        "#,
    );
    let sig = &r.functions["f"];
    assert_eq!(sig.params.len(), 1);
    assert_eq!(sig.params[0].ty, Type::Union(vec![Type::Int, Type::String]));
    assert_eq!(sig.return_type, Type::Union(vec![Type::Int, Type::String]));
}

#[test]
fn re_indexing_an_unconditional_function_after_removal_does_not_panic() {
    let src = "<?php function f(int $x): int {}";
    let tokens = Lexer::new(src).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    let mut r = Registry::build(&program);
    assert_eq!(r.functions["f"].params[0].ty, Type::Int);

    r.remove_declarations(&["f".to_string()], &[]);
    assert!(!r.functions.contains_key("f"));

    r.index(&program);
    assert_eq!(r.functions["f"].params[0].ty, Type::Int);
}

#[test]
fn class_declared_inside_an_if_else_guard_is_still_indexed() {
    let r = build_registry(
        r#"<?php
            if (PHP_VERSION_ID >= 80100) {
                class Widget {}
            } else {
                class OldWidget {}
            }"#,
    );
    assert!(r.classes.contains_key("Widget"));
    assert!(r.classes.contains_key("OldWidget"));
}

#[test]
fn merge_stubs_does_not_override_user_functions() {
    let mut r = build_registry(
        r#"<?php
            function strlen(string $s): int { return 0; }
        "#,
    );
    r.merge_stubs(vec![(
        "strlen",
        FunctionSig {
            params: vec![],
            return_type: Type::Bool,
            return_type_native: Type::Bool,
            no_discard: false,
            visibility: crate::ast::Visibility::Public,
        },
    )]);
    assert_eq!(r.functions["strlen"].return_type, Type::Int);
}

// Phase 25, real shape: `Project::new` calls `merge_stub_classes`
// *before* any project file is indexed - a built-in class like
// `Imagick` is already present by the time a project's own
// `phpstan-stubs/Imagick.stub.php`-style override (a bare
// `class Imagick { public function convolveImage(...) {} }` meaning
// to patch one method) gets indexed later. Without also recording
// the merged-in name in `unconditionally_declared_classes`,
// `index_with_context`'s own merge fix would see "no unconditional
// declaration recorded yet" and wholesale-replace the entire
// built-in class with the override's bare one-method version.
#[test]
fn a_stub_file_override_merges_onto_a_built_in_class_from_merge_stub_classes() {
    // The built-in class's own `ClassInfo` is built the same way
    // `php_stubs::generated_stub_classes()` builds a real one - via
    // `Registry::build` over its own source - rather than
    // constructing a `ClassInfo` literal by hand.
    let built_in = build_registry(
            "<?php class Imagick { public function getImageWidth() {} public function getImageHeight() {} public function convolveImage($kernel) {} }",
        )
        .classes
        .remove("Imagick")
        .unwrap();

    let mut r = Registry::default();
    r.merge_stub_classes(vec![("Imagick".to_string(), built_in)]);
    assert!(r.classes["Imagick"].methods.contains_key("getImageWidth"));
    assert!(r.classes["Imagick"].methods.contains_key("getImageHeight"));

    let tokens = Lexer::new(
        "<?php class Imagick { public function convolveImage($kernel, $channel = 1) {} }",
    )
    .tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program);

    assert!(
        r.classes["Imagick"].methods.contains_key("getImageWidth"),
        "the override must not discard the built-in class's other real methods"
    );
    assert!(r.classes["Imagick"].methods.contains_key("getImageHeight"));
    assert_eq!(
        r.classes["Imagick"].methods["convolveImage"].params.len(),
        2,
        "the override's own convolveImage() (2 params) should win over the built-in's (1 param)"
    );
}
