//! Purity and property writes found when asked: a cycle is never pure and
//! writes anything, however the search enters it; and what is found for
//! one declaration does not depend on the declaration asked about first.

use super::super::*;
use super::helpers::parse;

fn registry_and_bodies(src: &str) -> (Registry, FileBodies) {
    let program = parse(src);
    (Registry::build(&program), FileBodies::of(&program))
}

#[test]
fn a_chain_ends_in_a_pure_declaration() {
    let (registry, bodies) = registry_and_bodies(
        "<?php
        function a(): int { return b() + 1; }
        function b(): int { return c() + 1; }
        function c(): int { return 1; }",
    );
    for name in ["a", "b", "c"] {
        assert!(registry.function_is_pure(name, &bodies), "{name}");
    }
}

#[test]
fn a_cycle_is_never_pure_whichever_member_is_asked_first() {
    let src = "<?php
        function a(): int { return b(); }
        function b(): int { return a(); }
        function c(): int { return a(); }
        function d(): int { return 1; }";
    for order in [
        ["a", "b", "c", "d"],
        ["c", "b", "a", "d"],
        ["d", "c", "a", "b"],
    ] {
        let (registry, bodies) = registry_and_bodies(src);
        let answers: Vec<bool> = order
            .iter()
            .map(|name| registry.function_is_pure(name, &bodies))
            .collect();
        for (name, pure) in order.iter().zip(answers) {
            assert_eq!(pure, *name == "d", "{name} asked in {order:?}");
        }
    }
}

#[test]
fn a_declaration_with_an_impure_dependency_is_impure() {
    let (registry, bodies) = registry_and_bodies(
        "<?php
        function a(): int { return b() + c(); }
        function b(): int { return 1; }
        function c(): int { echo 'x'; return 1; }",
    );
    assert!(!registry.function_is_pure("a", &bodies));
    assert!(registry.function_is_pure("b", &bodies));
    assert!(!registry.function_is_pure("c", &bodies));
}

#[test]
fn a_method_calling_its_own_final_method_is_pure() {
    let (registry, bodies) = registry_and_bodies(
        "<?php
        final class Foo {
            public function a(): int { return $this->b(); }
            public function b(): int { return 1; }
            public function x(): int { return $this->y(); }
            public function y(): int { return $this->x(); }
        }",
    );
    assert!(registry.method_is_pure("Foo", "a", &bodies));
    assert!(registry.method_is_pure("foo", "B", &bodies));
    assert!(!registry.method_is_pure("Foo", "x", &bodies));
    assert!(!registry.method_is_pure("Foo", "y", &bodies));
}

#[test]
fn no_body_at_hand_is_impure() {
    let (registry, _) = registry_and_bodies("<?php function a(): int { return 1; }");
    assert!(!registry.function_is_pure("a", &NoBodies));
}

#[test]
fn property_writes_go_down_from_anything_only_through_settled_calls() {
    let (registry, bodies) = registry_and_bodies(
        "<?php
        final class Foo {
            public int $n = 0;
            public function nothing(): int { return 1; }
            public function own(): void { $this->n = 1; }
            public function viaOwn(): void { $this->own(); }
            public function loopA(): void { $this->loopB(); }
            public function loopB(): void { $this->loopA(); }
            public function intoLoop(): void { $this->loopA(); }
        }
        function free(): int { return 1; }
        function other(Foo $f): void { $f->n = 2; }
        function callsOther(Foo $f): void { other($f); }",
    );
    use PropertyWrites::*;
    let writes = |m: &str| registry.method_property_writes("Foo", m, &bodies);
    assert_eq!(writes("nothing"), Nothing);
    assert_eq!(writes("own"), OwnObject);
    assert_eq!(writes("viaOwn"), OwnObject);
    assert_eq!(writes("loopA"), Anything);
    assert_eq!(writes("loopB"), Anything);
    assert_eq!(writes("intoLoop"), Anything);
    assert_eq!(registry.function_property_writes("free", &bodies), Nothing);
    assert_eq!(
        registry.function_property_writes("other", &bodies),
        Anything
    );
    assert_eq!(
        registry.function_property_writes("callsOther", &bodies),
        Anything
    );
}

#[test]
fn a_verdict_does_not_depend_on_where_the_search_began() {
    let src = "<?php
        final class Foo {
            public int $n = 0;
            public function a(): void { $this->b(); }
            public function b(): void { $this->c(); $this->a(); }
            public function c(): void { $this->n = 1; }
        }";
    let methods = ["a", "b", "c"];
    let mut results = Vec::new();
    for order in [[0, 1, 2], [2, 1, 0], [1, 0, 2]] {
        let (registry, bodies) = registry_and_bodies(src);
        let mut found = [PropertyWrites::Nothing; 3];
        for &i in &order {
            found[i] = registry.method_property_writes("Foo", methods[i], &bodies);
        }
        results.push(found);
    }
    assert!(
        results.windows(2).all(|pair| pair[0] == pair[1]),
        "{results:?}"
    );
    assert_eq!(
        results[0],
        [
            PropertyWrites::Anything,
            PropertyWrites::Anything,
            PropertyWrites::OwnObject
        ]
    );
}
