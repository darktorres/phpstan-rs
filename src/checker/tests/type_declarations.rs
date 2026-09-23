//! Native type declarations PHP rejects at compile time.

use super::helpers::*;

fn found(src: &str) -> Vec<String> {
    let mut v: Vec<String> = diagnostics_for(src)
        .into_iter()
        .filter(|d| {
            let id = d.identifier.as_str();
            id.starts_with("unionType.")
                || id.starts_with("trueType.")
                || id.starts_with("falseType.")
                || id.starts_with("nullType.")
                || id.starts_with("nullableType.")
                || id.starts_with("intersectionType.")
                || id.ends_with("Type")
                    && (id.starts_with("property.") || id.starts_with("classConstant."))
                || matches!(
                    id,
                    "parameter.void" | "parameter.never" | "parameter.static"
                )
        })
        .map(|d| format!("{}: {}", d.identifier, d.message))
        .collect();
    v.sort();
    v
}

#[test]
fn standalone_only_types_in_a_union_or_nullable() {
    assert_eq!(
        found(
            "<?php
            function a(int|mixed $x): int|void {}
            function b(?mixed $x): ?never {}
            function c(): ?VOID {}
            "
        ),
        vec![
            "nullableType.mixed: Type mixed cannot be part of a nullable type declaration.",
            "nullableType.never: Type never cannot be part of a nullable type declaration.",
            "nullableType.void: Type VOID cannot be part of a nullable type declaration.",
            "unionType.mixed: Type mixed cannot be part of a union type declaration.",
            "unionType.void: Type void cannot be part of a union type declaration.",
        ]
    );
}

#[test]
fn duplicate_and_redundant_union_members() {
    assert_eq!(
        found(
            "<?php
            namespace App;
            use Other\\Thing;
            class Foo {}
            function a(int|INT $x, Foo|\\App\\Foo $y, Thing|\\Other\\Thing $z): void {}
            function b(bool|false $x, true|false $y, object|Foo $z, iterable|array $w): void {}
            function c(): object|static {}
            function d(): object|self {}
            "
        ),
        vec![
            "unionType.duplicate: Duplicate type INT is redundant.",
            "unionType.duplicate: Duplicate type \\App\\Foo is redundant.",
            "unionType.duplicate: Duplicate type \\Other\\Thing is redundant.",
            "unionType.duplicate: Duplicate type false is redundant.",
            "unionType.iterableAndArray: Type iterable|array contains both iterable and array, which is redundant.",
            "unionType.objectAndClass: Type object|Foo contains both object and a class type, which is redundant.",
            "unionType.objectAndClass: Type object|self contains both object and a class type, which is redundant.",
            "unionType.objectAndClass: Type object|static contains both object and a class type, which is redundant.",
            "unionType.trueAndFalse: Type contains both true and false, bool must be used instead.",
        ]
    );
}

#[test]
fn nullable_null_and_intersection_members() {
    assert_eq!(
        found(
            "<?php
            interface A {} interface B {}
            function a(?null $x, A&int $y, A&B&A $z): (A&B)|(A&string) { return $z; }
            "
        ),
        vec![
            "intersectionType.duplicate: Duplicate type A is redundant.",
            "intersectionType.int: Type int cannot be part of an intersection type.",
            "intersectionType.string: Type string cannot be part of an intersection type.",
            "nullableType.null: Type null cannot be marked as nullable.",
        ]
    );
}

#[test]
fn positions_that_reject_void_never_and_static() {
    assert_eq!(
        found(
            "<?php
            class Foo {
                const void A = 1;
                const callable B = 'strlen';
                public void $a;
                public never $b;
                public function m(void $x, never $y, static $z): static { return $this; }
            }
            "
        ),
        vec![
            "classConstant.callableType: Class constant Foo::B cannot have type callable.",
            "classConstant.voidType: Class constant Foo::A cannot have type void.",
            "parameter.never: never cannot be used as a parameter type.",
            "parameter.static: static cannot be used as a parameter type.",
            "parameter.void: void cannot be used as a parameter type.",
            "property.neverType: Property Foo::$b cannot have type never.",
            "property.voidType: Property Foo::$a cannot have type void.",
        ]
    );
}

#[test]
fn valid_declarations_are_accepted() {
    assert!(found(
        "<?php
        interface A {} interface B {}
        class Foo {
            const int|string C = 1;
            public ?int $a = null;
            public int|string|null $b = null;
            public function m(?Foo $x, int|false $y, (A&B)|null $z, A&B $w, mixed $v): static|null { return null; }
            public function n(): void {}
            public function o(): never { throw new \\Exception(); }
            public function p(): ?self { return null; }
        }
        $f = fn(int|string $x): int|string => $x;
        $g = function (?int $x): ?int { return $x; };
        "
    )
    .is_empty());
}

#[test]
fn closures_and_arrow_functions_are_checked() {
    assert_eq!(
        found(
            "<?php
            $f = fn(int|int $x): ?mixed => $x;
            $g = function (void $x): void {};
            "
        ),
        vec![
            "nullableType.mixed: Type mixed cannot be part of a nullable type declaration.",
            "parameter.void: void cannot be used as a parameter type.",
            "unionType.duplicate: Duplicate type int is redundant.",
        ]
    );
}

#[test]
fn static_cannot_be_part_of_an_intersection() {
    assert_eq!(
        found(
            "<?php interface A {} class C implements A { public function m(): A&static { return $this; } public function n(): A&self { return $this; } }"
        ),
        vec!["intersectionType.static: Type static cannot be part of an intersection type."]
    );
}

fn found_on(src: &str, version: (u8, u8)) -> Vec<String> {
    let mut v: Vec<String> = diagnostics_for_with(src, |r| r.php_version = Some(version))
        .into_iter()
        .filter(|d| {
            d.identifier.ends_with("NotSupported") || d.identifier.ends_with(".notSupported")
        })
        .filter(|d| d.identifier.contains("Type"))
        .map(|d| format!("{}: {}", d.identifier, d.message))
        .collect();
    v.sort();
    v
}

#[test]
fn standalone_null_false_true_and_dnf_need_php_8_2() {
    let src = "<?php
        interface A {} interface B {}
        function a(null $x, ?false $y, null|false $z, int|false $w): true { return true; }
        function b((A&B)|null $x): void {}
        ";
    assert_eq!(
        found_on(src, (8, 1)),
        vec![
            "falseType.standaloneNotSupported: Type ?false uses false as a standalone type, which is supported only on PHP 8.2 and later.",
            "falseType.standaloneNotSupported: Type null|false uses false as a standalone type, which is supported only on PHP 8.2 and later.",
            "nullType.standaloneNotSupported: Type null uses null as a standalone type, which is supported only on PHP 8.2 and later.",
            "trueType.notSupported: Type true is supported only on PHP 8.2 and later.",
            "unionType.dnfNotSupported: Disjunctive normal form types are supported only on PHP 8.2 and later.",
        ]
    );
    assert!(found_on(src, (8, 2)).is_empty());
}
