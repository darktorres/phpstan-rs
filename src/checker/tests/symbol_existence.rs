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
