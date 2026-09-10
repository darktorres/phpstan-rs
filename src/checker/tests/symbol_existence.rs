//! Phase 48: the "does this name actually resolve" existence-validation
//! pipeline (`Checker::class_name_exists`/`collect_unknown_class_names`/
//! `check_type_class_names_exist`, `checker/resolve.rs`), wired into every
//! site that used to trust a class/interface/trait/enum/function name
//! blindly - `use function`, `extends`/`implements`/trait-use targets,
//! `instanceof`, `catch`, and parameter/return/property type hints
//! (including on closures and arrow functions). Every positive case here
//! is paired with at least one negative case exercising the same syntactic
//! shape with a real, existing name, to guard against a false positive.

use super::helpers::*;

#[test]
fn instantiating_a_typod_class_is_still_flagged_with_class_not_found() {
    // Pre-existing behavior (`ExprKind::New`'s own check, predating this
    // phase) - included here as the baseline every other site below is
    // measured against.
    let d = diagnostics_for("<?php function f() { return new TotallyMadeUp(); }");
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn extending_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Sub extends TotallyMadeUpBase {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.notFound" && x.message.contains("TotallyMadeUpBase")),
        "got: {d:?}"
    );
}

#[test]
fn extending_a_real_class_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class Sub extends Base {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_interface_extending_multiple_unknown_interfaces_is_flagged_for_each() {
    // Real, valid PHP an ordinary class can never do (`extends` is
    // single-parent for a class) - an interface's own `extends A, B;`
    // can list more than one, and `check_class_references_exist` walks
    // every one of `info.extends`, not just the first.
    let d = diagnostics_for(
        r#"<?php
            interface Combined extends MadeUpA, MadeUpB {}
        "#,
    );
    let found: Vec<&str> = d
        .iter()
        .filter(|x| x.identifier == "class.notFound")
        .map(|x| x.message.as_str())
        .collect();
    assert!(found.iter().any(|m| m.contains("MadeUpA")), "got: {d:?}");
    assert!(found.iter().any(|m| m.contains("MadeUpB")), "got: {d:?}");
}

#[test]
fn implementing_an_unknown_interface_is_flagged_with_interface_not_found() {
    let d = diagnostics_for(
        r#"<?php
            class C implements TotallyMadeUpInterface {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "interface.notFound"
            && x.message.contains("TotallyMadeUpInterface")),
        "got: {d:?}"
    );
}

#[test]
fn implementing_a_real_interface_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Countable2 {}
            class C implements Countable2 {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_enum_implementing_an_unknown_interface_is_flagged() {
    // `check_class_references_exist` is called for `EnumDecl` too, not
    // just `ClassDecl` - an enum's own `implements` is a real, common
    // shape (backed-enum-implements-an-interface).
    let d = diagnostics_for(
        r#"<?php
            enum Status implements TotallyMadeUpInterface {
                case Active;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "interface.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn using_an_unknown_trait_is_flagged_with_trait_not_found() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                use TotallyMadeUpTrait;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "trait.notFound" && x.message.contains("TotallyMadeUpTrait")),
        "got: {d:?}"
    );
}

#[test]
fn using_a_real_trait_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait Greetable {
                public function greet(): string { return "hi"; }
            }
            class C {
                use Greetable;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_against_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f($x) {
                if ($x instanceof TotallyMadeUpType) {
                    return true;
                }
                return false;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.notFound" && x.message.contains("TotallyMadeUpType")),
        "got: {d:?}"
    );
}

#[test]
fn instanceof_against_a_real_class_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class RealType {}
            function f($x) {
                if ($x instanceof RealType) {
                    return true;
                }
                return false;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_self_and_static_inside_a_real_class_body_are_never_flagged() {
    // `resolve_name_expr` resolves `self`/`static`/`parent` against
    // `current_class` before this check ever sees an `Object` - a real,
    // ordinary self-check must never be misread as an unresolvable name.
    let d = diagnostics_for(
        r#"<?php
            class Node {
                public function isSameKind($other): bool {
                    return $other instanceof self && $other instanceof static;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_with_a_dynamic_target_is_never_flagged() {
    // `$x instanceof $className` never names a concrete class at all -
    // `InstanceofTarget::Expr`, not `::Name` - so there's nothing for
    // this check to even look at.
    let d = diagnostics_for(
        r#"<?php
            function f($x, $className) {
                return $x instanceof $className;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn catching_an_unknown_exception_class_is_flagged_with_class_not_found() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                try {
                    doSomething();
                } catch (TotallyMadeUpException $e) {
                }
            }
            function doSomething() {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.notFound"
                && x.message.contains("TotallyMadeUpException")),
        "got: {d:?}"
    );
    // Real bug fixed alongside the existence check: a typo'd/missing
    // exception class must not ALSO get the misleading "does not
    // implement Throwable" diagnostic - `class_name_exists` gates that
    // check now (an `else if`, not a second independent `if`).
    assert!(
        !d.iter().any(|x| x.identifier == "catch.notThrowable"),
        "got: {d:?}"
    );
}

#[test]
fn catching_a_real_non_throwable_class_still_gets_the_not_throwable_diagnostic() {
    // Negative case for the above: a real, existing class that simply
    // isn't `Throwable` must still be caught by the pre-existing check,
    // unaffected by the new existence gate.
    let d = diagnostics_for(
        r#"<?php
            class NotAnException {}
            function f() {
                try {
                    doSomething();
                } catch (NotAnException $e) {
                }
            }
            function doSomething() {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "catch.notThrowable"),
        "got: {d:?}"
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn catching_a_real_throwable_subclass_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                try {
                    doSomething();
                } catch (InvalidArgumentException $e) {
                }
            }
            function doSomething() {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_parameter_typed_with_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(TotallyMadeUpParam $x): void {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.notFound" && x.message.contains("TotallyMadeUpParam")),
        "got: {d:?}"
    );
}

#[test]
fn a_return_type_of_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): TotallyMadeUpReturn {
                return null;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.notFound" && x.message.contains("TotallyMadeUpReturn")),
        "got: {d:?}"
    );
}

#[test]
fn a_method_parameter_and_return_type_that_both_exist_are_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class A {}
            class B {
                public function convert(A $a): A {
                    return $a;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_nullable_and_union_typed_parameter_still_catches_a_typo_inside_it() {
    // Exercises the recursive walk through `Union`/nullable - a typo
    // buried inside `Foo|Bar|null`, not just a bare top-level reference.
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function f(Foo|TotallyMadeUpUnionMember|null $x): void {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.notFound"
                && x.message.contains("TotallyMadeUpUnionMember")),
        "got: {d:?}"
    );
}

#[test]
fn a_property_typed_with_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Holder {
                public TotallyMadeUpPropertyType $value;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.notFound"
                && x.message.contains("TotallyMadeUpPropertyType")),
        "got: {d:?}"
    );
}

#[test]
fn a_property_typed_with_a_real_class_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Value {}
            class Holder {
                public Value $value;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_closure_parameter_typed_with_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                $cb = function (TotallyMadeUpClosureParam $x): void {};
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.notFound"
                && x.message.contains("TotallyMadeUpClosureParam")),
        "got: {d:?}"
    );
}

#[test]
fn a_closure_return_type_of_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                $cb = function (): TotallyMadeUpClosureReturn {
                    return null;
                };
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"
            && x.message.contains("TotallyMadeUpClosureReturn")),
        "got: {d:?}"
    );
}

#[test]
fn a_closure_with_only_real_types_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Payload {}
            function f() {
                $cb = function (Payload $x): Payload { return $x; };
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_arrow_function_parameter_typed_with_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                $cb = fn (TotallyMadeUpArrowParam $x) => $x;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.notFound"
                && x.message.contains("TotallyMadeUpArrowParam")),
        "got: {d:?}"
    );
}

#[test]
fn an_arrow_function_return_type_of_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Payload {}
            function f(Payload $p) {
                $cb = fn (): TotallyMadeUpArrowReturn => $p;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.notFound"
                && x.message.contains("TotallyMadeUpArrowReturn")),
        "got: {d:?}"
    );
}

#[test]
fn an_arrow_function_with_only_real_types_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Payload {}
            function f() {
                $cb = fn (Payload $x): Payload => $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_typod_use_function_import_is_flagged_with_function_not_found() {
    let d = diagnostics_for(
        r#"<?php
            use function totallyMadeUpFunctionName;
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.notFound"
            && x.message.contains("totallyMadeUpFunctionName")),
        "got: {d:?}"
    );
}

#[test]
fn a_real_use_function_import_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            use function strlen;
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 71 corpus find: real PHP function calls are case-insensitive -
// `Registry::lookup_function` used to be a plain exact-case `HashMap`
// lookup, wrongly flagging any differently-cased call (a builtin or a
// user-declared function alike) as undefined.

// Phase 71 Item 4: `function_exists()`-guard suppression for the
// undefined-function check - a disclosed, project-specific adaptation
// (this project's own registry can legitimately lack a symbol real PHP
// itself resolves fine at runtime, unlike real PHPStan's own live
// reflection).

#[test]
fn function_exists_guard_suppresses_undefined_function_call() {
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                if (function_exists('totallyMadeUpFunctionName')) {
                    totallyMadeUpFunctionName();
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn negated_function_exists_guard_with_early_return_suppresses_call_after_if() {
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                if (!function_exists('totallyMadeUpFunctionName')) {
                    return;
                }
                totallyMadeUpFunctionName();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn function_exists_guard_does_not_leak_past_non_terminating_else() {
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                if (function_exists('totallyMadeUpFunctionName')) {
                    echo "found";
                } else {
                    echo "not found";
                }
                totallyMadeUpFunctionName();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn function_exists_guard_with_dynamic_name_stays_unsuppressed() {
    let d = diagnostics_for(
        r#"<?php
            function test(string $name): void {
                if (function_exists($name)) {
                    totallyMadeUpFunctionName();
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn function_exists_guard_first_class_callable_syntax_suppressed() {
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                if (function_exists('totallyMadeUpFunctionName')) {
                    $f = totallyMadeUpFunctionName(...);
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn function_exists_guard_scoped_correctly_across_closures() {
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                if (function_exists('totallyMadeUpFunctionName')) {
                    $cb = function () {
                        totallyMadeUpFunctionName();
                    };
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.notFound"),
        "got: {d:?}"
    );
}

// Phase 48.1: real PHP resolves class/interface/trait/enum names case-
// insensitively too, the same bug class already fixed for functions
// (Phase 71).

#[test]
fn a_differently_cased_new_of_a_user_declared_class_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(): void {
                new foo();
                new FOO();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn a_differently_cased_extends_target_resolves_correctly() {
    let d = diagnostics_for(
        r#"<?php
            abstract class bar { abstract public function f(): void; }
            class Child extends Bar {}
        "#,
    );
    // Not just "no class.notFound" - the completeness check must also
    // see the real ancestor's abstract method through the differently-
    // cased `extends` reference, or `Child` would wrongly look complete.
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.abstractMethodNotImplemented"),
        "expected the completeness check to still see the real ancestor, got: {d:?}"
    );
}

#[test]
fn a_differently_cased_use_imported_alias_reference_resolves_to_the_real_class() {
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            class Foo {}

            namespace Other;
            use App\Foo as Bar;
            function test(): void {
                new bar();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn a_genuinely_undefined_class_is_still_flagged_regardless_of_case() {
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                new totallyMadeUpClassName();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"),
        "got: {d:?}"
    );
}

// Phase 48.2: `class.nameCase`/`interface.nameCase`/`trait.nameCase`/
// `enum.nameCase`, scoped to a class/enum declaration's own `extends`/
// `implements`/trait-`use` references.

#[test]
fn a_differently_cased_extends_reference_to_a_user_class_is_flagged_name_case() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class Child extends base {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.nameCase"),
        "got: {d:?}"
    );
}

#[test]
fn a_differently_cased_implements_reference_to_a_user_interface_is_flagged_name_case() {
    let d = diagnostics_for(
        r#"<?php
            interface Iface {}
            class C implements iface {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "interface.nameCase"),
        "got: {d:?}"
    );
}

#[test]
fn a_differently_cased_trait_use_reference_is_flagged_name_case() {
    let d = diagnostics_for(
        r#"<?php
            trait T {}
            class C { use t; }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "trait.nameCase"),
        "got: {d:?}"
    );
}

#[test]
fn a_use_imported_reference_is_never_flagged_name_case_regardless_of_case() {
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            class Base {}

            namespace Other;
            use App\Base;
            class Child extends base {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.ends_with(".nameCase")),
        "got: {d:?}"
    );
}

#[test]
fn a_differently_cased_reference_to_a_builtin_class_is_never_flagged_name_case() {
    let d = diagnostics_for(
        r#"<?php
            class C extends exception {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.ends_with(".nameCase")),
        "got: {d:?}"
    );
}

#[test]
fn an_exact_case_reference_is_never_flagged_name_case() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class Child extends Base {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.ends_with(".nameCase")),
        "got: {d:?}"
    );
}

#[test]
fn a_differently_cased_call_to_a_builtin_function_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                STRTOLOWER("x");
                StrToLower("x");
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_differently_cased_call_to_a_user_declared_function_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function myFunc(): void {}
            function g(): void {
                MYFUNC();
                myfunc();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_genuinely_undefined_function_is_still_flagged_regardless_of_case() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                TOTALLYMADEUPFUNCTION();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.notFound"),
        "got: {d:?}"
    );
}

// `namespace\foo()` is a real, distinct PHP construct - "call `foo`
// relative to the current namespace only, no use-import lookup, no
// global-namespace fallback" - used as a deliberate house style by
// `php-standard-library`/Psl (663 real call sites in the corpus) to
// force resolution against a sibling function of the same short name
// that also exists globally. Without recognizing the `namespace\`
// prefix, `take_name_segment` accepting the keyword as an ordinary name
// segment left the literal word "namespace" in the resolved string,
// which then got re-qualified against the current namespace a *second*
// time (`Psl\Str\namespace\chunk` instead of the real `Psl\Str\chunk`).
#[test]
fn a_namespace_relative_call_resolves_against_the_current_namespace() {
    let d = diagnostics_for(
        r#"<?php
            namespace Psl\Str;
            function chunk(string $s): array { return [$s]; }
            function f(string $s): array {
                return namespace\chunk($s);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_namespace_relative_call_never_falls_back_to_the_global_namespace() {
    // The real function only exists *globally* here, not in the
    // current namespace - `namespace\` must not fall back to it the
    // way a bare unqualified call otherwise would.
    let d = diagnostics_for(
        r#"<?php
            function chunk(string $s): array { return [$s]; }
            namespace Psl\Str {
                function f(string $s): array {
                    return namespace\chunk($s);
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.notFound"),
        "expected namespace\\chunk() to be reported as not found (no \
         global fallback), got: {d:?}"
    );
}

#[test]
fn a_namespace_relative_call_is_case_insensitive() {
    let d = diagnostics_for(
        r#"<?php
            namespace Psl\Str;
            function chunk(string $s): array { return [$s]; }
            function f(string $s): array {
                return NAMESPACE\chunk($s);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_group_use_function_import_is_checked_per_member() {
    // Group-use expands to individually-kinded `UseImport`s, each
    // carrying its own `function`/`const` marker independent of its
    // siblings in the same group.
    let d = diagnostics_for(
        r#"<?php
            use function Foo\{strlen as l, totallyMadeUpGroupedFunction};
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.notFound"
            && x.message.contains("totallyMadeUpGroupedFunction")),
        "got: {d:?}"
    );
}

#[test]
fn a_plain_use_class_import_is_not_existence_checked() {
    // Matches real PHPStan's own `ExistingNamesInUseRule`, confirmed
    // directly against its source: a plain, class-shaped `use Foo;` is
    // never existence-checked at the `use` statement itself (only
    // case-sensitivity/forbidden-name checking runs there) - a class
    // reference is only ever existence-checked where it's actually used.
    // So a `use`-imported class that's never referenced again produces
    // no diagnostic here, even though the name doesn't exist anywhere in
    // this snippet.
    let d = diagnostics_for(
        r#"<?php
            use Some\Totally\MadeUp\ClassName;
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_use_const_import_is_not_existence_checked() {
    // Deliberate scope decision (see `UseImport::kind`'s own doc
    // comment): this project has no project-wide global-constant
    // registry to check a `use const` against at all.
    let d = diagnostics_for(
        r#"<?php
            use const Some\Totally\MadeUp\CONST_NAME;
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_generic_type_argument_referencing_an_unregistered_class_is_not_flagged() {
    // Deliberately mirrors `a_bound_argument_that_s_an_unregistered_class_
    // is_never_flagged` (`generics_and_templates.rs`) - the same real
    // corpus finding (a PHAR-bundled dependency with no loose source
    // files this project's registry can index) applies equally to
    // existence-checking, not just bound-checking: a class referenced
    // only as a `@template` type ARGUMENT (not the bare typehint itself)
    // is deliberately never existence-checked, since this project's own
    // registry completeness can't be relied on for it.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class Collection {}
            /** @param Collection<UnregisteredArg> $c */
            function f($c) {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_anonymous_class_self_and_static_typed_return_is_never_flagged() {
    // Mirrors `anonymous_class_method_resolves_a_static_return_type`
    // (`oop_overrides_and_inheritance.rs`): an anonymous class's own
    // synthetic `"class@anonymous"` name is deliberately never indexed
    // into the registry at all, so `self`/`static`-typed members inside
    // its own body must never be misread as referencing a missing class.
    let d = diagnostics_for(
        r#"<?php
            function make() {
                return new class {
                    public function self(): static {
                        return $this;
                    }
                };
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real corpus shape (jetbrains/phpstorm-stubs' own `relay/Cluster.php`):
// a file can have more than one semicolon-style `namespace Foo;`
// statement, each switching the active namespace for everything that
// follows - `Checker::name_ctx` used to be built once, whole-file (last
// `namespace` seen wins), which wrongly resolved a self-referencing
// return type declared under an *earlier* namespace as if it were under
// the file's *last* one.
#[test]
fn a_self_referencing_type_before_a_later_namespace_switch_in_the_same_file_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Relay;
            class Cluster {
                public function append(): Cluster {}
            }
            namespace Relay\Cluster;
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 48.3-48.6: existence-checking for @property/@method/@mixin/
// @phpstan-type docblock tags - previously parsed/resolved and
// semantically consumed, but never existence-checked.

#[test]
fn a_property_tag_referencing_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @property NoSuchClass $x
             */
            class Foo {}
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "class.notFound"));
}

#[test]
fn a_property_tag_referencing_a_known_class_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Bar {}
            /**
             * @property Bar $x
             */
            class Foo {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_method_tag_return_type_referencing_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @method NoSuchClass bar()
             */
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn a_method_tag_parameter_type_referencing_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @method void bar(NoSuchClass $x)
             */
            class Foo {}
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "class.notFound"));
}

#[test]
fn a_mixin_tag_referencing_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @mixin NoSuchClass
             */
            class Foo {}
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "class.notFound"));
}

#[test]
fn a_mixin_tag_referencing_a_known_class_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Bar {}
            /**
             * @mixin Bar
             */
            class Foo {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_phpstan_type_alias_referencing_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @phpstan-type MyAlias NoSuchClass
             */
            class Foo {}
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "class.notFound"));
}

#[test]
fn a_phpstan_import_type_from_an_unknown_class_is_not_flagged_by_default() {
    // Deliberate leniency (Phase 48.6): the `from Class` target is very
    // commonly only reachable via Composer autoloading this project's
    // own registry may legitimately have no data for.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @phpstan-import-type MyAlias from SomeVendorClassNotIndexedHere
             */
            class Foo {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 48.7: `StaticMethodCallCheck`/`AccessStaticPropertiesCheck`/
// `ClassConstantRule`'s own "the class part itself doesn't exist" half -
// a real, previously-silent gap (confirmed via direct code reading, not
// just the design doc's own flagged-for-verification note): a `::`
// reference through a genuinely undefined class produced zero
// diagnostics before this.

#[test]
fn calling_a_static_method_on_a_genuinely_undefined_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                \NoSuchClassAtAll::doSomething();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"
            && x.message
                .contains("static method call target class NoSuchClassAtAll not found")),
        "got: {d:?}"
    );
}

#[test]
fn accessing_a_static_property_on_a_genuinely_undefined_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                echo \NoSuchClassAtAll::$prop;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"
            && x.message
                .contains("static property access target class NoSuchClassAtAll not found")),
        "got: {d:?}"
    );
}

#[test]
fn accessing_a_class_constant_on_a_genuinely_undefined_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                echo \NoSuchClassAtAll::SOME_CONST;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"
            && x.message
                .contains("class constant access target class NoSuchClassAtAll not found")),
        "got: {d:?}"
    );
}

#[test]
fn a_static_method_call_on_a_known_class_is_still_only_flagged_as_a_missing_method() {
    // Regression guard: the new "class not found" branch must not also
    // fire (or double-report) once the class itself is real.
    let d = diagnostics_for(
        r#"<?php
            class RealClass {}
            function f(): void {
                RealClass::doesNotExist();
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "staticMethod.notFound");
}
