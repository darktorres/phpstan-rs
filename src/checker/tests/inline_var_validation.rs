//! Statement-level `@var` tags checked against the values they describe
//! (`checker::var_tag_types`): `varTag.nativeType`, `varTag.type`,
//! `varTag.trait` and `class.notFound`.
use super::helpers::*;

fn var_tag_messages(src: &str) -> Vec<(String, String)> {
    diagnostics_for(src)
        .into_iter()
        .filter(|x| {
            matches!(
                x.identifier.as_str(),
                "varTag.nativeType" | "varTag.type" | "varTag.trait" | "class.notFound"
            )
        })
        .map(|x| (x.identifier, x.message))
        .collect()
}

#[test]
fn a_var_tag_contradicted_by_an_obviously_wrong_literal_is_flagged() {
    let m = var_tag_messages(
        r#"<?php
            function f(): void {
                /** @var int $x */
                $x = "hello";
            }
        "#,
    );
    assert_eq!(
        m,
        [(
            "varTag.nativeType".to_string(),
            "PHPDoc tag @var with type int is not subtype of native type 'hello'.".to_string()
        )]
    );
}

#[test]
fn a_bare_var_tag_with_no_name_is_also_checked() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                /** @var int */
                $x = "hello";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "varTag.nativeType"),
        "got: {d:?}"
    );
}

#[test]
fn a_var_tag_referencing_an_unknown_class_is_flagged_class_not_found() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                /** @var NoSuchClass $x */
                $x = null;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn a_var_tag_referencing_a_real_class_is_never_flagged_class_not_found() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function f(): void {
                /** @var Foo $x */
                $x = new Foo();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn a_var_tag_matching_the_assigned_value_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                /** @var int $x */
                $x = 5;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "varTag.type"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_var_tag_widening_the_assigned_value_is_never_flagged() {
    // int is a real subtype of int|string - a real, common, legitimate
    // widening annotation, not a mistake.
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                /** @var int|string $x */
                $x = 5;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "varTag.type"),
        "unexpected diagnostics: {d:?}"
    );
}

// Piwigo's own `Piwigo\Category\CategoryRepository::findAvailableList`:
// `/** @var list<array<string, mixed>> $rows */
// $rows = $qb->getQuery()->getArrayResult();` - Doctrine's own
// `getArrayResult()` returns a bare, imprecise `array`, and the whole
// point of writing an inline `@var` here is to *refine* that into a
// more specific shape. A `list<array<string, mixed>>` is unconditionally
// still an array - not a mismatch, even though the refined type isn't
// itself a "supertype" of the bare one in the naive one-directional
// sense.
#[test]
fn a_var_tag_refining_a_wider_native_return_type_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                /** @var array{a: int} $x */
                $x = returnsArray();
            }
            function returnsArray(): array { return ['a' => 1]; }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "varTag.type"),
        "unexpected diagnostics: {d:?}"
    );
}

// A `foreach` header's own inline `@var` annotation (Phase 161) - the
// named form overrides the loop variable's own ordinarily-inferred
// element type for the whole body, not just validated-and-discarded
// the way `check_inline_var_annotation_type`'s own assign-only scope
// leaves it for this statement kind.
#[test]
fn a_named_var_tag_on_a_foreach_header_overrides_the_loop_variable_s_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(array $items): void {
                /** @var string $x */
                foreach ($items as $x) {
                    assertType('string', $x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_bare_var_tag_on_a_foreach_header_resolves_to_the_loop_variable() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(array $items): void {
                /** @var string */
                foreach ($items as $x) {
                    assertType('string', $x);
                }
            }
        "#,
    );
    // Real PHPStan also counts the iterated variable as a candidate, so
    // an unnamed tag here is `varTag.noVariable` - not this test's subject.
    let d: Vec<_> = d
        .into_iter()
        .filter(|x| x.identifier != "varTag.noVariable")
        .collect();
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_foreach_value_tag_is_checked_against_the_element_type() {
    let m = var_tag_messages(
        r#"<?php
            /** @param list<int> $xs */
            function f(array $xs): void {
                /** @var string $x */
                foreach ($xs as $x) {}
                /** @var int $y */
                foreach ([1, 2] as $y) {}
            }
        "#,
    );
    assert_eq!(
        m,
        [(
            "varTag.type".to_string(),
            "PHPDoc tag @var with type string is incompatible with type int.".to_string()
        )]
    );
}

#[test]
fn a_var_tag_is_checked_against_native_types_of_calls_parameters_and_properties() {
    let m = var_tag_messages(
        r#"<?php
            class Foo {}
            class Bar {}
            class C {
                public Foo $foo;
                public function make(): Foo { return new Foo(); }
                public function f(int $n): void {
                    /** @var string $a */
                    $a = $n;
                    /** @var Bar $b */
                    $b = $this->make();
                    /** @var Bar $c */
                    $c = $this->foo;
                    /** @var list<int> $d */
                    $d = g();
                    /** @var string $e */
                    $e = g();
                }
            }
            function g(): array { return []; }
        "#,
    );
    assert_eq!(
        m,
        [
            (
                "varTag.nativeType".to_string(),
                "PHPDoc tag @var with type string is not subtype of native type int.".to_string()
            ),
            (
                "varTag.nativeType".to_string(),
                "PHPDoc tag @var with type Bar is not subtype of native type Foo.".to_string()
            ),
            (
                "varTag.nativeType".to_string(),
                "PHPDoc tag @var with type Bar is not subtype of native type Foo.".to_string()
            ),
            (
                "varTag.nativeType".to_string(),
                "PHPDoc tag @var with type string is not subtype of native type array.".to_string()
            ),
        ]
    );
}

#[test]
fn a_var_tag_retyping_an_existing_variable_is_checked_against_its_current_type() {
    let m = var_tag_messages(
        r#"<?php
            /** @return int */
            function g() { return 1; }
            function f(int $n): void {
                $m = g();
                $k = g();
                /** @var string $n */
                echo $n;
                /** @var string $m */
                echo $m;
                /** @var int<0, max> $k */
                echo $k;
            }
        "#,
    );
    assert_eq!(
        m,
        [
            (
                "varTag.nativeType".to_string(),
                "PHPDoc tag @var with type string is not subtype of native type int.".to_string()
            ),
            (
                "varTag.type".to_string(),
                "PHPDoc tag @var with type string is incompatible with type int.".to_string()
            ),
        ]
    );
}

#[test]
fn a_by_reference_argument_forgets_the_variables_native_type() {
    let m = var_tag_messages(
        r#"<?php
            function fill(&$out): void { $out = 'x'; }
            function f(int $n): void {
                fill($n);
                /** @var string $s */
                $s = $n;
            }
        "#,
    );
    assert!(m.is_empty(), "unexpected: {m:?}");
}

#[test]
fn a_var_tag_naming_a_trait_or_unknown_class_is_reported_on_any_statement() {
    let m = var_tag_messages(
        r#"<?php
            trait T {}
            function f($x): void {
                /** @var T $x */
                echo $x;
                /** @var Missing $x */
                return;
            }
        "#,
    );
    assert_eq!(
        m,
        [
            (
                "varTag.trait".to_string(),
                "PHPDoc tag @var for variable $x has invalid type T.".to_string()
            ),
            (
                "class.notFound".to_string(),
                "PHPDoc tag @var for variable $x contains unknown class Missing.".to_string()
            ),
        ]
    );
}

#[test]
fn every_named_var_tag_on_a_statement_is_applied() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f($a, $b): void {
                /**
                 * @var int $a
                 * @var string $b
                 */
                echo 1;
                assertType('int', $a);
                assertType('string', $b);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn psalm_and_phan_var_tags_are_applied_and_a_foreach_key_tag_types_the_key() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, int> $xs */
            function f(string $a, string $b, array $xs): void {
                /** @psalm-var non-empty-string $a */
                $a = str_replace('_', '-', $a);
                assertType('non-empty-string', $a);
                /** @phan-var non-empty-string $b */
                $b = str_replace('_', '-', $b);
                assertType('non-empty-string', $b);
                /** @var non-empty-string $k */
                foreach ($xs as $k => $v) {
                    assertType('non-empty-string', $k);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_tag_overlapping_the_inferred_type_is_never_incompatible() {
    let m = var_tag_messages(
        r#"<?php
            function f(string $c): void {
                /** @var non-empty-string $c */
                $c = strtolower($c);
            }
        "#,
    );
    assert!(m.is_empty(), "unexpected: {m:?}");
}
