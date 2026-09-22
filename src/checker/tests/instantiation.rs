//! `new` target legality: pseudo-class names outside a class, `new
//! parent` without a parent, the instantiated symbol's kind, a
//! constructor-less class given arguments, inaccessible constructors,
//! and dynamic class-name expressions that can't name a class.

use super::helpers::*;

fn with_identifier<'d>(d: &'d [crate::Diagnostic], id: &str) -> Vec<&'d str> {
    d.iter()
        .filter(|x| x.identifier == id)
        .map(|x| x.message.as_str())
        .collect()
}

#[test]
fn new_static_in_an_abstract_class_is_not_flagged_but_new_self_is() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Shape {
                public static function make(): static { return new static(); }
                public static function bad(): self { return new self(); }
            }
        "#,
    );
    assert_eq!(
        with_identifier(&d, "new.abstract"),
        vec!["cannot instantiate abstract class Shape"],
        "got: {d:?}"
    );
}

#[test]
fn instantiating_a_trait_or_enum_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait NewT {}
            enum NewE {}
            function f(): void {
                $a = new NewT();
                $b = new NewE();
            }
        "#,
    );
    assert_eq!(
        with_identifier(&d, "new.trait"),
        vec!["cannot instantiate trait NewT"]
    );
    assert_eq!(
        with_identifier(&d, "new.enum"),
        vec!["cannot instantiate enum NewE"]
    );
}

#[test]
fn arguments_to_a_constructor_less_class_are_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Bare {}
            function f(): void {
                $a = new Bare(1);
                $b = new Bare();
            }
        "#,
    );
    assert_eq!(
        with_identifier(&d, "new.noConstructor"),
        vec![
            "class Bare does not have a constructor and must be instantiated without any parameters"
        ],
        "got: {d:?}"
    );
}

#[test]
fn inaccessible_constructors_are_flagged_only_from_outside() {
    let d = diagnostics_for(
        r#"<?php
            class Single {
                private function __construct() {}
                public static function make(): self { return new self(); }
            }
            class Guarded {
                protected function __construct() {}
            }
            class GuardedChild extends Guarded {
                public static function make(): self { return new self(); }
            }
            function f(): void {
                $a = new Single();
                $b = new Guarded();
                $c = new GuardedChild();
            }
        "#,
    );
    assert_eq!(
        with_identifier(&d, "new.privateConstructor"),
        vec!["cannot instantiate class Single via private constructor Single::__construct()"],
        "got: {d:?}"
    );
    assert_eq!(
        with_identifier(&d, "new.protectedConstructor"),
        vec![
            "cannot instantiate class Guarded via protected constructor Guarded::__construct()",
            "cannot instantiate class GuardedChild via protected constructor Guarded::__construct()",
        ],
        "got: {d:?}"
    );
}

#[test]
fn pseudo_class_names_outside_a_class_are_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                $a = new self();
                $b = new static();
                $c = new parent();
            }
        "#,
    );
    assert_eq!(
        with_identifier(&d, "outOfClass.self"),
        vec!["using self outside of class scope"]
    );
    assert_eq!(
        with_identifier(&d, "outOfClass.static"),
        vec!["using static outside of class scope"]
    );
    assert_eq!(
        with_identifier(&d, "outOfClass.parent"),
        vec!["using parent outside of class scope"]
    );
}

#[test]
fn new_parent_without_a_parent_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Orphan {
                public function make(): object { return new parent(); }
            }
            class Base2 {}
            class Child2 extends Base2 {
                public function make(): object { return new parent(); }
            }
        "#,
    );
    assert_eq!(
        with_identifier(&d, "class.noParent"),
        vec!["Orphan::make() calls new parent but Orphan does not extend any class"],
        "got: {d:?}"
    );
}

#[test]
fn dynamic_new_on_a_non_class_name_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $i, int|string $m, string $s, object $o): void {
                $a = new $i();
                $b = new $m();
                $c = new $s();
                $d = new $o();
            }
        "#,
    );
    let found: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "new.nonObject")
        .map(|x| (x.message.as_str(), x.level))
        .collect();
    assert_eq!(
        found,
        vec![
            ("cannot instantiate class using int", 0),
            ("cannot instantiate class using int|string", 7),
        ],
        "got: {d:?}"
    );
}

#[test]
fn new_static_is_flagged_only_without_a_consistent_constructor_guarantee() {
    let d = diagnostics_for(
        r#"<?php
            class Open { public static function make(): static { return new static(); } }
            final class Closed { public static function make(): static { return new static(); } }
            /** @final */
            class SoftClosed { public static function make(): static { return new static(); } }
            /** @phpstan-consistent-constructor */
            class Consistent { public static function make(): static { return new static(); } }
            class ConsistentChild extends Consistent {
                public function m(): static { return new static(); }
            }
            class FinalCtor {
                final public function __construct() {}
                public function m(): static { return new static(); }
            }
            interface HasCtor { public function __construct(); }
            class ViaIface implements HasCtor {
                public function __construct() {}
                public function m(): static { return new static(); }
            }
            abstract class AbsProto { abstract public function __construct(); }
            class ProtoImpl extends AbsProto {
                public function __construct() {}
                public function m(): static { return new static(); }
            }
            trait NeedsCtor {
                abstract public function __construct();
                public function m(): static { return new static(); }
            }
            class UsesNeedsCtor {
                use NeedsCtor;
                public function __construct() {}
            }
        "#,
    );
    let found: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "new.static")
        .map(|x| x.location.as_str())
        .collect();
    assert_eq!(found, vec!["Open::make"], "got: {d:?}");
}

#[test]
fn new_static_in_a_static_method_of_an_abstract_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            abstract class AbsFactory {
                final public function __construct() {}
                public static function make(): static { return new static(); }
                public function copy(): static { return new static(); }
            }
        "#,
    );
    assert_eq!(
        with_identifier(&d, "new.staticInAbstractClassStaticMethod"),
        vec!["unsafe usage of new static() in abstract class AbsFactory in static method make()"],
        "got: {d:?}"
    );
    assert!(with_identifier(&d, "new.static").is_empty(), "got: {d:?}");
}

#[test]
fn first_class_callable_new_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Made { public function __construct(int $x) {} }
            function f(): void {
                $factory = new Made(...);
            }
        "#,
    );
    assert_eq!(
        with_identifier(&d, "callable.notSupported"),
        vec!["Cannot create callable from the new operator."],
        "got: {d:?}"
    );
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("arguments.")),
        "got: {d:?}"
    );
}
