//! Trait host resolution, aliasing (`as`), and conflict resolution (`insteadof`) between
//! multiple used traits.

use super::helpers::*;

#[test]
fn a_class_using_a_trait_with_an_unimplemented_abstract_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait Greetable9 {
                abstract public function getName(): string;
                public function greet(): string { return "Hello " . $this->getName(); }
            }
            class Person9 { use Greetable9; }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("does not implement abstract method(s): getName")),
        "expected a missing-abstract-method diagnostic, got: {d:?}"
    );
}

#[test]
fn a_class_that_supplies_a_traits_abstract_method_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait Greetable9 {
                abstract public function getName(): string;
                public function greet(): string { return "Hello " . $this->getName(); }
            }
            class Person9 { use Greetable9; public function getName(): string { return "x"; } }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn calling_a_method_from_a_transitively_used_trait_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait Greets { public function greet(): string { return "hi"; } }
            trait Combined { use Greets; public function extra(): int { return 1; } }
            class Foo { use Combined; }
            function test(Foo $f): string { return $f->greet(); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 13 Finding E: a trait method calling $this->hostOnlyMethod()
// is valid PHP (the trait's author expects any using class to
// provide it) - checked standalone (current_class bound to the
// trait's own FQN), this always false-positived as "undefined
// method".
#[test]
fn a_trait_method_calling_a_host_only_method_is_not_flagged_when_the_host_declares_it() {
    let d = diagnostics_for(
        r#"<?php
            trait Greeter {
                public function greet(): string { return $this->name(); }
            }
            class Person {
                use Greeter;
                public function name(): string { return "Alice"; }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_trait_method_calling_a_host_only_method_is_flagged_for_a_sibling_host_missing_it() {
    // Proves per-host re-checking, not "checked once (against
    // whichever host happens to run first) and reused" - the same
    // trait, used by two classes, must be checked once per host.
    let d = diagnostics_for(
        r#"<?php
            trait Greeter {
                public function greet(): string { return $this->name(); }
            }
            class Person {
                use Greeter;
                public function name(): string { return "Alice"; }
            }
            class Robot {
                use Greeter;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("undefined method"), "got: {d:?}");
}

// Real corpus shape: symfony/cache's own `if (version_compare(phpversion(
// 'redis'), '6.3.0', '>=')) { trait Redis63ProxyTrait { ... } }` -
// `check_program`'s own top-level `StmtKind::ClassDecl` handling already
// dispatches a trait through `check_trait_methods` (this same per-host
// re-checking), but a trait declared *inside* a conditional block
// reaches `check_stmt`'s own nested `StmtKind::ClassDecl` arm instead,
// which used to check every trait method with `current_class` set to
// the trait's own name unconditionally, regardless of `ClassKind` -
// `$this->hostOnlyMethod()` (real shape: `initializeLazyObject()`,
// provided by a sibling trait on the real host) was wrongly flagged as
// undefined on the trait itself.
#[test]
fn a_conditionally_declared_trait_still_checks_against_its_real_host() {
    let d = diagnostics_for(
        r#"<?php
            if (true) {
                trait Greeter {
                    public function greet(): string { return $this->name(); }
                }
            }
            class Person {
                use Greeter;
                public function name(): string { return "Alice"; }
            }
        "#,
    );
    // Phase 15 item (2): the literal `if (true)` wrapping here (this
    // test's own scenario shape - a conditionally-declared trait, e.g.
    // real code's own `if (!trait_exists(...)) { trait Foo {...} }`
    // idiom) now legitimately also gets its own `if.alwaysTrue` -
    // unrelated to what this test actually checks (that the trait's own
    // members still resolve against its real host), so it's excluded
    // here rather than changing the fixture's own scenario shape.
    assert!(
        !d.iter().any(|x| x.identifier != "if.alwaysTrue"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_host_independent_trait_bug_is_reported_once_not_once_per_host() {
    let d = diagnostics_for(
        r#"<?php
            trait Broken {
                public function boom(): int { return thisFunctionDoesNotExist(); }
            }
            class A { use Broken; }
            class B { use Broken; }
        "#,
    );
    assert_eq!(d.len(), 1, "expected the duplicate to be deduped: {d:?}");
}

#[test]
fn an_unused_trait_is_still_checked_standalone() {
    let d = diagnostics_for(
        r#"<?php
            trait Orphaned {
                public function boom(): int { return thisFunctionDoesNotExist(); }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "an unused trait must still be checked: {d:?}");
}

#[test]
fn a_trait_used_only_by_another_trait_is_checked_against_the_real_final_host() {
    // `Inner` is used only by `Outer` (itself a trait), never
    // directly by any real class - `Inner`'s own $this->name() must
    // still resolve against Concrete, the real, final host, not
    // against `Outer` (which is itself just as host-unaware).
    let d = diagnostics_for(
        r#"<?php
            trait Inner {
                public function greet(): string { return $this->name(); }
            }
            trait Outer {
                use Inner;
            }
            class Concrete {
                use Outer;
                public function name(): string { return "x"; }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_trait_alias_introduced_via_as_is_callable_under_its_new_name() {
    let d = diagnostics_for(
        r#"<?php
            trait A { public function hello(): string { return "A"; } }
            trait B { public function hello(): string { return "B"; } }
            class C {
                use A, B {
                    A::hello insteadof B;
                    B::hello as helloB;
                }
            }
            function test(C $c): void { echo $c->hello(); echo $c->helloB(); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_unqualified_trait_alias_resolves_against_the_single_used_trait() {
    let d = diagnostics_for(
        r#"<?php
            trait X { public function greet(): string { return "x"; } }
            class Y {
                use X {
                    greet as greetAlias;
                }
            }
            function test(Y $y): void { echo $y->greet(); echo $y->greetAlias(); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn calling_an_undeclared_trait_alias_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait X { public function greet(): string { return "x"; } }
            class Y {
                use X {
                    greet as greetAlias;
                }
            }
            function test(Y $y): void { echo $y->totallyMadeUp(); }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("undefined method")),
        "expected an undefined-method diagnostic, got: {d:?}"
    );
}

#[test]
fn two_traits_declaring_the_same_property_with_incompatible_types_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait T1a { public int $x = 1; }
            trait T1b { public string $x = "a"; }
            class C1 { use T1a, T1b; }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("T1a and T1b define the same property ($x) with incompatible types")),
        "expected a trait-property-conflict diagnostic, got: {d:?}"
    );
}

#[test]
fn two_traits_declaring_the_same_method_with_incompatible_signatures_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait T2a { public function foo(): int { return 1; } }
            trait T2b { public function foo(): string { return "a"; } }
            class C2 { use T2a, T2b; }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("T2a and T2b define the same method (foo) with an incompatible signature")),
        "expected a trait-method-conflict diagnostic, got: {d:?}"
    );
}

#[test]
fn two_traits_declaring_the_same_property_with_identical_types_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait T3a { public int $y = 1; }
            trait T3b { public int $y = 2; }
            class C3 { use T3a, T3b; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_trait_method_conflict_resolved_via_insteadof_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait T4a { public function bar(): int { return 1; } }
            trait T4b { public function bar(): int { return 2; } }
            class C4 {
                use T4a, T4b {
                    T4a::bar insteadof T4b;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
