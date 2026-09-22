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

// A stub file meant to override one method of an already-known class
// (e.g. a `stubFiles`-provided bare `class Imagick { public function
// convolveImage(...) {} }`) must merge with the real class, not let the
// second unconditional declaration wholesale-replace the first and
// silently drop every other method/constant.
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
    assert_eq!(r.classes["X"].extends, vec!["Base".to_string().into()]);
    assert!(r.classes["X"].methods.contains_key("fromFirst"));
    assert!(r.classes["X"].methods.contains_key("fromSecond"));
}

#[test]
fn re_indexing_an_unconditional_class_after_removal_does_not_panic() {
    // `remove_declarations` (incremental re-checking's removal step,
    // `Project::update_file`) must clear the "already unconditionally
    // declared" marker along with `classes`, or re-indexing the same
    // file after a change hits the merge branch against an entry
    // already deleted - panicking instead of cleanly re-inserting.
    let src = "<?php class X { public function original() {} }";
    let tokens = Lexer::new(src).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    let mut r = Registry::build(&program);
    assert!(r.classes["X"].methods.contains_key("original"));

    r.remove_declarations(&[], &["X".to_string()]);
    assert!(!r.classes.contains_key("X"));

    r.index(&program, false);
    assert!(r.classes["X"].methods.contains_key("original"));
}

// Two unconditional declarations of the same function name must merge,
// not overwrite - the function-side sibling of the class-merge fix
// above. (`strtr`'s stub declares a 3-string-argument overload and a
// 2-argument, `array $replace_pairs` overload as two separate top-level
// declarations sharing one name.)
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
    assert_eq!(
        sig.params[0].name,
        "str".into(),
        "later declaration's name wins"
    );
    assert_eq!(sig.params[0].ty, Type::String);
    assert!(!sig.params[0].has_default);
    assert_eq!(sig.params[1].name, "replace_pairs".into());
    assert_eq!(
        sig.params[1].ty,
        Type::Union(vec![
            Type::String,
            Type::Array(
                Box::new(Type::unknown_array_key()),
                Box::new(Type::Mixed(false))
            )
        ])
    );
    assert!(!sig.params[1].has_default);
    assert_eq!(sig.params[2].name, "to".into());
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

    r.index(&program, false);
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
            return_has_own_doc: true,
            return_case_refs: Vec::new(),
            return_explicit_mixed_hint: false,
            no_discard: false,
            visibility: crate::ast::Visibility::Public,
            is_internal: false,
            is_api: false,
            has_override_attr: false,
            deprecated: None,
            throws: None,
            assert_if_true: Vec::new(),
            assert_if_false: Vec::new(),
            assert: Vec::new(),
            body: None,
            purity_dependencies: None,
            is_transitively_pure: false,
            is_stub: false,
            is_magic: false,
        },
    )]);
    assert_eq!(r.functions["strlen"].return_type, Type::Int);
}

// `Project::new` merges stubs before any project file is indexed (the
// same ordering the class-side test just below documents), so a
// conditionally-declared polyfill of a built-in name (`if
// (!function_exists('str_contains')) { function str_contains(?string
// $haystack, ?string $needle): bool {...} }`) must still be able to win
// over the stub's own signature - `entry(...).or_insert_with(...)` is a
// no-op once the stub already occupies the slot, which would silently
// discard the polyfill's more-precise (here, nullable) native parameter
// types.
#[test]
fn a_conditional_polyfill_declaration_wins_over_a_previously_merged_stub() {
    let stub_sig =
        build_registry("<?php function str_contains(string $haystack, string $needle): bool {}")
            .functions
            .remove("str_contains")
            .unwrap();

    let mut r = Registry::default();
    r.merge_stubs(vec![("str_contains", stub_sig)]);
    assert_eq!(r.functions["str_contains"].params[0].ty, Type::String);

    let tokens = Lexer::new(
        r#"<?php
            if (!function_exists('str_contains')) {
                function str_contains(?string $haystack, ?string $needle): bool { return true; }
            }
        "#,
    )
    .tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program, false);

    assert_eq!(
        r.functions["str_contains"].params[0].ty,
        Type::Union(vec![Type::String, Type::Null]),
        "the polyfill's own nullable native param type must win over the stub's non-nullable one"
    );
}

// Two unrelated conditional declarations of the same built-in name - a
// bare `func_get_args()`-forwarding shim with no declared params at all,
// and the real, correctly-typed one - compete for the one stub-provided
// slot the fix above lets a conditional declaration claim. If the
// zero-param shim won first (whichever file happens to be indexed
// first), the other file's own body-check would zip its one real
// parameter against the winning zero-param registry signature, quietly
// losing the parameter and reading it as undefined inside its own body.
// The arity gate keeps the door open for whichever declaration actually
// matches the stub's shape, regardless of which one is seen first.
#[test]
fn a_stub_slot_is_not_claimed_by_an_arity_mismatched_conditional_declaration() {
    let stub_sig = build_registry("<?php function grapheme_strrev(string $string): string {}")
        .functions
        .remove("grapheme_strrev")
        .unwrap();

    let mut r = Registry::default();
    r.merge_stubs(vec![("grapheme_strrev", stub_sig)]);

    let tokens = Lexer::new(
        r#"<?php
            if (!function_exists('grapheme_strrev')) {
                function grapheme_strrev() { return \Prefix\grapheme_strrev(...func_get_args()); }
            }
        "#,
    )
    .tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program, false);
    assert_eq!(
        r.functions["grapheme_strrev"].params.len(),
        1,
        "the zero-param shim must not have displaced the (arity-matching) stub"
    );

    let tokens = Lexer::new(
        r#"<?php
            if (!function_exists('grapheme_strrev')) {
                function grapheme_strrev(string $string) { return $string; }
            }
        "#,
    )
    .tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program, false);
    assert_eq!(
        r.functions["grapheme_strrev"].params.len(),
        1,
        "the real, arity-matching declaration must still be able to win afterward"
    );
    assert_eq!(
        r.functions["grapheme_strrev"].params[0].name,
        "string".into()
    );
}

// The identical arity-mismatch shape one level down from the two tests
// just above: neither of these two conditional declarations ever
// occupies a stub-provided slot at all (no `merge_stubs` call anywhere
// here) - an alphabetically-earlier `func_get_args()`-forwarding shim
// and the real, correctly-typed declaration, both ordinary conditional
// declarations of the same global function name, compete directly with
// each other. `conditional_declaration_may_replace_stub`'s comparison
// must still apply here, not a plain `or_insert_with` (first one indexed
// wins, arity or not), or whichever file happens to be indexed first
// wins regardless of quality.
#[test]
fn two_ordinary_conditional_declarations_are_tiebroken_by_arity_too() {
    let mut r = Registry::default();

    let tokens = Lexer::new(
        r#"<?php
            if (!function_exists('trigger_deprecation')) {
                function trigger_deprecation() { return \Prefix\trigger_deprecation(...func_get_args()); }
            }
        "#,
    )
    .tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program, false);
    assert_eq!(r.functions["trigger_deprecation"].params.len(), 0);

    let tokens = Lexer::new(
        r#"<?php
            if (!function_exists('trigger_deprecation')) {
                function trigger_deprecation(string $package, string $version, string $message, mixed ...$args): void {}
            }
        "#,
    )
    .tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program, false);
    assert_eq!(
        r.functions["trigger_deprecation"].params.len(),
        4,
        "the real, correctly-typed declaration must win over the earlier 0-param shim"
    );

    // Order-independence: a later, *worse* (0-param shim) conditional
    // declaration must not un-seat the real one that already won.
    let tokens = Lexer::new(
        r#"<?php
            if (!function_exists('trigger_deprecation')) {
                function trigger_deprecation() { return \Prefix\trigger_deprecation(...func_get_args()); }
            }
        "#,
    )
    .tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program, false);
    assert_eq!(
        r.functions["trigger_deprecation"].params.len(),
        4,
        "the real declaration must not be displaced by a later, worse shim"
    );
}

// Even at matching arity, a polyfill package can bundle several
// PHP-version-gated variants of the same function behind separate
// `function_exists()` guards - an untyped variant (`function
// ctype_punct($text) {...}`) alongside a fully-typed one (`function
// ctype_punct(mixed $text): bool {...}`) for the exact same name. The
// untyped one must not win (whichever file happens to be indexed
// first), or it would silently discard type information the stub
// itself already carried, and every other file's call sites resolving
// through this one shared registry entry would then report the
// parameter/return type as missing.
#[test]
fn a_stub_slot_is_not_claimed_by_a_less_precisely_typed_conditional_declaration() {
    let stub_sig = build_registry("<?php function ctype_punct(mixed $text): bool {}")
        .functions
        .remove("ctype_punct")
        .unwrap();

    let mut r = Registry::default();
    r.merge_stubs(vec![("ctype_punct", stub_sig)]);

    // The untyped legacy variant must not win, arity match or not.
    let tokens = Lexer::new(
        r#"<?php
            if (!function_exists('ctype_punct')) {
                function ctype_punct($text) { return true; }
            }
        "#,
    )
    .tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program, false);
    assert_eq!(
        r.functions["ctype_punct"].return_type,
        Type::Bool,
        "the untyped variant must not have displaced the stub's own bool return type"
    );

    // The fully-typed variant must still be able to win afterward.
    let tokens = Lexer::new(
        r#"<?php
            if (!function_exists('ctype_punct')) {
                function ctype_punct(mixed $text): bool { return true; }
            }
        "#,
    )
    .tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program, false);
    assert_eq!(r.functions["ctype_punct"].return_type, Type::Bool);
    assert_eq!(r.functions["ctype_punct"].params[0].ty, Type::Mixed(false));
}

// A vendored, unconditional, bare (guard-free) duplicate of a stub
// function gets indexed as if it were ordinary project source, taking
// the "second unconditional declaration merges onto the first" path
// against the identical signature `merge_stubs` already put there.
// Clearing `stub_provided_functions` there would tell a later,
// better-typed conditional polyfill declaration that the slot is no
// longer stub-equivalent and off limits - silently discarding its
// nullable parameter types via an extra hop.
#[test]
fn a_stub_slot_survives_an_unconditional_duplicate_of_the_stub_itself() {
    let stub_sig =
        build_registry("<?php function str_contains(string $haystack, string $needle): bool {}")
            .functions
            .remove("str_contains")
            .unwrap();

    let mut r = Registry::default();
    r.merge_stubs(vec![("str_contains", stub_sig)]);

    // The vendored phpstorm-stubs copy: unconditional, identical shape.
    let tokens =
        Lexer::new("<?php function str_contains(string $haystack, string $needle): bool {}")
            .tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program, false);

    // The real polyfill: conditional, nullable native params.
    let tokens = Lexer::new(
        r#"<?php
            if (!function_exists('str_contains')) {
                function str_contains(?string $haystack, ?string $needle): bool { return true; }
            }
        "#,
    )
    .tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program, false);

    assert_eq!(
        r.functions["str_contains"].params[0].ty,
        Type::Union(vec![Type::String, Type::Null]),
        "the real polyfill's own nullable params must still win despite the intervening unconditional duplicate"
    );
}

// `Project::new` calls `merge_stub_classes` before any project file is
// indexed - a built-in class like `Imagick` is already present by the
// time a project's own override (a bare `class Imagick { public
// function convolveImage(...) {} }` meaning to patch one method) gets
// indexed later. The merged-in name must also be recorded in
// `unconditionally_declared_classes`, or `insert_built_decl`'s merge fix
// would see "no unconditional declaration recorded yet" and
// wholesale-replace the entire built-in class with the override's bare
// one-method version.
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
    r.index(&program, false);

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

// A polyfill's conditional declaration (`if (PHP_VERSION_ID < 80400) {
// final class ReflectionConstant { private $value; ... } }`) can collide
// with a native-class stub for a newer PHP version's built-in of the
// same name (only `$name`, no `$value`) - merged in via
// `merge_stub_classes` before this conditional declaration is ever
// indexed. Unlike the `class_exists`-guarded defensive-fallback shape
// above (`unconditional_class_wins_over_a_later_conditional_stub`), the
// built-in stub's own members must gain the conditional declaration's
// real properties/methods, not discard them - the stub is only a
// type-checking fiction, and the polyfill's own method bodies are
// checked against whatever wins this name.
#[test]
fn a_conditional_declaration_merges_onto_a_built_in_stub_class() {
    let built_in = build_registry("<?php class ReflectionConstant { public string $name; }")
        .classes
        .remove("ReflectionConstant")
        .unwrap();

    let mut r = Registry::default();
    r.merge_stub_classes(vec![("ReflectionConstant".to_string(), built_in)]);

    let tokens = Lexer::new(
        r#"<?php
            if (\PHP_VERSION_ID < 80400) {
                final class ReflectionConstant {
                    private $value;

                    public function getValue() {
                        return $this->value;
                    }
                }
            }
        "#,
    )
    .tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program, false);

    assert!(
        r.classes["ReflectionConstant"]
            .properties
            .contains_key("value"),
        "the polyfill's own $value property must become visible"
    );
    assert!(
        r.classes["ReflectionConstant"]
            .properties
            .contains_key("name"),
        "the built-in stub's own $name property must survive the merge"
    );
}

// The sibling regression this fix must not reintroduce: a *real*,
// already-known vendor class colliding with a `class_exists`-guarded
// defensive fallback must still behave exactly like
// `unconditional_class_wins_over_a_later_conditional_stub` - the
// fallback's own members must not leak in just because the winning
// declaration happens to have been indexed via ordinary file processing
// rather than `merge_stub_classes`.
#[test]
fn a_conditional_declaration_does_not_merge_onto_a_real_non_stub_class() {
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

// A conditional class declaration's `extends` (e.g. `class PhpToken
// extends Symfony\Polyfill\Php80\PhpToken {}`, guarding PHP <8.0)
// colliding with a built-in stub must never be adopted onto the stub,
// even while its properties/methods do merge in - otherwise a wholly
// unrelated third file inheriting from the built-in transitively picks
// up the never-taken `extends`, wrongly flagging its real `__construct()`
// as overriding a `final` method it has no actual relation to.
#[test]
fn a_conditional_declaration_s_extends_never_leaks_onto_a_built_in_stub() {
    let built_in = build_registry("<?php class PhpToken implements Stringable {}")
        .classes
        .remove("PhpToken")
        .unwrap();

    let mut r = Registry::default();
    r.merge_stub_classes(vec![("PhpToken".to_string(), built_in)]);

    let tokens = Lexer::new(
        r#"<?php
            if (\PHP_VERSION_ID < 80000) {
                class PhpToken extends Symfony\Polyfill\Php80\PhpToken {
                }
            }
        "#,
    )
    .tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program, false);

    assert!(
        r.classes["PhpToken"].extends.is_empty(),
        "the built-in stub declared no extends of its own - the conditional \
         declaration's real-but-never-taken extends must not be adopted"
    );
    assert_eq!(
        r.classes["PhpToken"].implements,
        vec!["Stringable".to_string().into()],
        "the built-in stub's own implements must survive unchanged"
    );
}

// A class/enum declaring the same method name twice in one body (a
// documented phpstorm-stubs idiom for a version-split or genuine
// overload, e.g. `DatePeriod::__construct`'s four declarations) must
// have its method table built by merging every declaration, not a bare
// `.collect()` that silently keeps only the textually-last one and
// drops every earlier alternate parameter list with zero trace.
#[test]
fn two_method_declarations_with_different_arities_in_one_class_body_merge() {
    // Mirrors real `DatePeriod`'s own three distinct constructor shapes
    // (vendor/phpstorm-stubs/date/date_c.php:1529,1538,1548 - the fourth,
    // identical-signature declaration there is just a PHP-8.3-gated copy
    // of the third and doesn't change the merged result). `FunctionSig::
    // merge`'s own positional-union rule (registry/function_sig.rs:606)
    // means only the position every real overload declares as required
    // stays required in the merged signature - position 0 here, since
    // `$start`/`$isostr` are both required, but `$interval`/`$end` only
    // appear in two of the three overloads (position 3, `$options`, is
    // optional everywhere already).
    let r = build_registry(
        r#"<?php
            class DatePeriod {
                public function __construct(DateTimeInterface $start, DateInterval $interval, DateTimeInterface $end, $options = 0) {}
                public function __construct(DateTimeInterface $start, DateInterval $interval, $recurrences, $options = 0) {}
                public function __construct($isostr, $options = 0) {}
            }
        "#,
    );
    let sig = &r.classes["DatePeriod"].methods["__construct"];
    assert_eq!(
        sig.params.len(),
        4,
        "the merged signature must be as wide as the widest real overload"
    );
    assert!(
        !sig.params[0].has_default,
        "start/isostr is required in every real overload"
    );
    assert!(
        sig.params[1].has_default,
        "interval is required in only two of the three real overloads \
         (absent from the isostr-only one), so a caller matching that \
         third overload can't be forced to supply it"
    );
    assert!(
        sig.params[2].has_default,
        "end/recurrences is required in only two of the three real \
         overloads, same reasoning as position 1"
    );
}

#[test]
fn two_method_declarations_with_different_arities_in_one_enum_body_merge() {
    let r = build_registry(
        r#"<?php
            enum E {
                case A;
                public function m(int $a) {}
                public function m(int $a, int $b) {}
            }
        "#,
    );
    let sig = &r.classes["E"].methods["m"];
    assert_eq!(
        sig.params.len(),
        2,
        "from_enum_decl's own .collect() site must merge too, not just from_decl's"
    );
    assert!(!sig.params[0].has_default);
    assert!(
        sig.params[1].has_default,
        "b is only required in the second overload"
    );
}
