//! Function and method signature resolution, including phpdoc typing and override merging.

use super::super::*;
use super::helpers::*;

#[test]
fn the_object_type_hint_is_not_namespace_qualified() {
    // `object` is a PHP-reserved word - no real class can ever be
    // named that, so `Type::Object("object")` (parse_type_string's
    // own sentinel for the native `object` type hint) must never be
    // resolved as if it were a genuine class reference.
    let r = build_registry(
        r#"<?php
            namespace App\Some\Deep\Namespace_;
            class Foo {
                public function bar(): object { return new \stdClass(); }
            }"#,
    );
    let sig = r
        .lookup_method("App\\Some\\Deep\\Namespace_\\Foo", "bar")
        .unwrap();
    assert_eq!(
        sig.return_type,
        Type::Object("object".to_string(), Vec::new())
    );
}

#[test]
fn a_leading_backslash_in_a_native_type_hint_is_not_re_namespaced() {
    // Real, previously-live bug found via real code in this
    // project's own verification corpus (a `Doctrine\ORM\QueryBuilder`
    // union member written as `\Doctrine\ORM\QueryBuilder`, i.e.
    // "absolute, from the global namespace" - real PHP syntax): the
    // leading `\` used to be stripped before `resolve_names_in_type`
    // ever saw it (inside `types::parse_type_atom`, purely as part of
    // computing a bare name for keyword matching), losing the "already
    // fully qualified, don't touch" signal - every absolute reference
    // containing a `\` then got silently mis-qualified as if it were
    // namespace-relative (`App\Doctrine\ORM\QueryBuilder` instead of
    // the real `Doctrine\ORM\QueryBuilder`).
    let r = build_registry(
        r#"<?php
            namespace App;
            class Foo {
                public function bar(\Doctrine\ORM\QueryBuilder $qb): void {}
            }"#,
    );
    let sig = r.lookup_method("App\\Foo", "bar").unwrap();
    assert_eq!(
        sig.params[0].ty,
        Type::Object("Doctrine\\ORM\\QueryBuilder".to_string(), Vec::new())
    );
}

#[test]
fn a_root_anchored_native_closure_type_hint_is_not_namespace_qualified() {
    // Real corpus shape (amphp/amp): a native `\Closure $callback` type
    // hint (root-anchored, unambiguous) in a namespaced file used to
    // become a bogus `App\Closure` - `types::parse_type_keyword_or_class`'s
    // own `"closure"` keyword arm used to hardcode the bare, unqualified
    // name regardless of the original text, losing the leading `\` that
    // `NameContext::resolve` needs to see to know not to re-qualify it
    // (the same reason `a_leading_backslash_in_a_native_type_hint_is_
    // not_re_namespaced` above exists for an ordinary class reference).
    let r = build_registry(
        r#"<?php
            namespace App;
            class Foo {
                public function bar(\Closure $callback): void {}
            }"#,
    );
    let sig = r.lookup_method("App\\Foo", "bar").unwrap();
    assert_eq!(
        sig.params[0].ty,
        Type::Object("Closure".to_string(), Vec::new())
    );
}

#[test]
fn a_bare_unqualified_closure_type_hint_defers_to_a_real_use_import() {
    // Deliberately NOT given the same treatment as the root-anchored
    // case above: a bare, unqualified `Closure` is genuinely ambiguous -
    // real corpus shape, `rector/rector`'s own consumers of `nikic/
    // php-parser` routinely `use PhpParser\Node\Expr\Closure;` (an
    // entirely different real class) and then type-hint bare `Closure`
    // meaning *that* one, not the native global class. Must resolve
    // through the same `use`-import lookup any other class reference
    // gets.
    let r = build_registry(
        r#"<?php
            namespace App;
            use Some\Other\Namespace_\Closure;
            class Foo {
                public function bar(Closure $callback): void {}
            }"#,
    );
    let sig = r.lookup_method("App\\Foo", "bar").unwrap();
    assert_eq!(
        sig.params[0].ty,
        Type::Object("Some\\Other\\Namespace_\\Closure".to_string(), Vec::new())
    );
}

#[test]
fn phpdoc_types_an_otherwise_untyped_function() {
    let r = build_registry(
        r#"<?php
            /**
             * @param int $x
             * @return int
             */
            function double($x) { return $x * 2; }
        "#,
    );
    let sig = &r.functions["double"];
    assert_eq!(sig.params[0].ty, Type::Int);
    assert_eq!(sig.return_type, Type::Int);
}

// Finding A (Phase 32): phpstorm-stubs represents "this parameter
// changed across PHP versions" as the same conceptual parameter
// declared twice, each gated by its own `#[PhpStormStubsElementAvailable]`
// attribute - real PHP can't have two same-named parameters in one
// function, so a duplicate name reaching `FunctionSig::from_decl` is
// unconditionally this versioning artifact. Real shape: `pg_query`'s
// `$connection`.
#[test]
fn a_duplicate_named_parameter_collapses_to_its_last_occurrence_s_type() {
    let r = build_registry(
        r#"<?php
            function pg_query(
                #[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $connection = null,
                #[PhpStormStubsElementAvailable(from: '8.0')] \PgSql\Connection $connection,
                string $query
            ) {}
        "#,
    );
    let sig = &r.functions["pg_query"];
    assert_eq!(sig.params.len(), 2, "the legacy duplicate must be dropped");
    assert_eq!(sig.params[0].name, "connection");
    assert_eq!(
        sig.params[0].ty,
        Type::Object("PgSql\\Connection".to_string(), Vec::new()),
        "the surviving type must be the modern (second) occurrence's, not the legacy (first, untyped) one"
    );
    assert_eq!(sig.params[1].name, "query");
    assert_eq!(sig.params[1].ty, Type::String);
}

// Real shape: `SQLite3::open`'s `$flags`/`$encryptionKey`, two
// separate duplicated pairs in the same parameter list.
#[test]
fn two_separate_duplicated_parameter_pairs_both_collapse() {
    let r = build_registry(
        r#"<?php
            function sqlite3_open(
                string $filename,
                #[PhpStormStubsElementAvailable(from: '5.3', to: '5.6')] $flags = 6,
                #[PhpStormStubsElementAvailable(from: '5.3', to: '5.6')] $encryptionKey = '',
                #[PhpStormStubsElementAvailable(from: '7.0')] int $flags = 0,
                #[PhpStormStubsElementAvailable(from: '7.0')] string $encryptionKey = ''
            ) {}
        "#,
    );
    let sig = &r.functions["sqlite3_open"];
    assert_eq!(sig.params.len(), 3);
    assert_eq!(sig.params[1].name, "flags");
    assert_eq!(sig.params[1].ty, Type::Int);
    assert_eq!(sig.params[2].name, "encryptionKey");
    assert_eq!(sig.params[2].ty, Type::String);
}

// Real shape: `fsockopen`'s duplicated `$port` - the duplicate that
// actually broke argument binding (Finding A's own motivating case),
// since it isn't the last parameter.
#[test]
fn a_duplicated_parameter_in_the_middle_of_the_list_doesn_t_shift_later_ones() {
    let r = build_registry(
        r#"<?php
            function fsockopen(
                string $hostname,
                #[PhpStormStubsElementAvailable(from: '5.3', to: '7.0')] int $port = -1,
                #[PhpStormStubsElementAvailable(from: '7.1')] int $port = -1,
                &$error_code = null,
                &$error_message = null,
                ?float $timeout = null
            ) {}
        "#,
    );
    let sig = &r.functions["fsockopen"];
    assert_eq!(sig.params.len(), 5);
    assert_eq!(
        sig.params
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        vec!["hostname", "port", "error_code", "error_message", "timeout"]
    );
}

#[test]
fn a_function_with_no_duplicate_parameters_is_unaffected() {
    let r = build_registry(r#"<?php function f(int $a, string $b) {}"#);
    let sig = &r.functions["f"];
    assert_eq!(sig.params.len(), 2);
    assert_eq!(sig.params[0].name, "a");
    assert_eq!(sig.params[1].name, "b");
}

// Finding B (Phase 32): a `LanguageLevelTypeAware` parameter override
// loses to a competing legacy `@param` docblock tag, since the
// override was only ever injected as a native type hint - `resolve_type`
// treats a matching `@param` tag as authoritative over the native hint.
// Real shape: `ini_set`'s `@param string $value` next to a native
// `#[LanguageLevelTypeAware(['8.1' => 'string|int|float|bool|null'],
// default: 'string')] $value`.
#[test]
fn a_language_level_type_aware_override_wins_over_a_matching_stale_param_tag() {
    let r = build_registry(
        r#"<?php
            /**
             * @param string $option
             * @param string $value
             * @return string|false
             */
            function ini_set($option, #[LanguageLevelTypeAware(['8.1' => 'string|int|float|bool|null'], default: 'string')] $value) {}
        "#,
    );
    assert_eq!(
        r.functions["ini_set"].params[1].ty,
        Type::Union(vec![
            Type::String,
            Type::Int,
            Type::Float,
            Type::Bool,
            Type::Null,
        ]),
        "the modern override must win over the stale '@param string' tag"
    );
}

// Real shape: `finfo_file`'s doc type is `resource`; before Finding B,
// this collapsed to `Type::Mixed` (a *worse* failure than `ini_set`'s,
// since `resolve_type`'s own `resource`-stripping fires on the
// never-overridden stale doc type) rather than resolving to the
// override at all.
#[test]
fn a_language_level_type_aware_override_wins_even_when_the_stale_doc_type_is_resource() {
    let r = build_registry(
        r#"<?php
            class finfo {}
            /**
             * @param resource $finfo
             * @return string|false
             */
            function finfo_file(#[LanguageLevelTypeAware(['8.1' => 'finfo'], default: 'resource')] $finfo, string $filename) {}
        "#,
    );
    assert_eq!(
        r.functions["finfo_file"].params[0].ty,
        Type::Object("finfo".to_string(), Vec::new())
    );
}

// The boundary-crossing guard's own regression test: `later`'s
// override must not be spliced onto `earlier`'s unrelated docblock
// just because it's the nearest one found by an unbounded backward
// scan - `later` has no doc comment of its own, so the override must
// fall back to the native-hint-only path instead.
#[test]
fn an_override_is_not_spliced_onto_an_unrelated_earlier_declaration_s_docblock() {
    let r = build_registry(
        r#"<?php
            /** @param resource $x */
            function earlier($x) {}
            function later(#[LanguageLevelTypeAware(['8.1' => 'finfo'], default: 'resource')] $finfo) {}
        "#,
    );
    assert_eq!(
        r.functions["earlier"].params[0].ty,
        Type::Object("resource".to_string(), Vec::new()),
        "earlier's own docblock must be untouched by later's override"
    );
    assert_eq!(
        r.functions["later"].params[0].ty,
        Type::Object("finfo".to_string(), Vec::new()),
        "later's override must still win via the native-hint-only path"
    );
}

// Finding I (Phase 32): `resolve_type`'s docblock-vs-native
// reconciliation generalizes Phase 23's `resource`-specific staleness
// check into one rule via `is_subtype` - a real, present, non-`object`
// native hint is authoritative about what's possible; any docblock
// member incompatible with it can never actually happen. Real shape:
// `imagesx`'s native `int` vs. a stale `@return int|false` doc (the
// GD-object migration dropped `false` from the native hint without
// the doc ever being updated).
#[test]
fn a_stale_false_member_is_dropped_when_the_native_hint_no_longer_allows_it() {
    let r = build_registry(
        r#"<?php
            /** @return int|false */
            function imagesx(): int {}
        "#,
    );
    assert_eq!(r.functions["imagesx"].return_type, Type::Int);
}

// A native hint that itself keeps `false` must *not* have it stripped
// from the doc - proof this isn't now unconditionally dropping `false`,
// only when the native hint disallows it.
#[test]
fn a_false_member_the_native_hint_still_allows_is_kept() {
    let r = build_registry(
        r#"<?php
            /** @return int|false */
            function f(): int|false {}
        "#,
    );
    assert_eq!(
        r.functions["f"].return_type,
        Type::Union(vec![Type::Int, Type::False])
    );
}

// Phase 60 corpus find: a PHPStan/Psalm callable-shape doc member
// (`Closure(Tv): bool`) degrades to plain `Type::Callable` at parse
// time (no per-parameter/return signature is tracked) - `is_subtype
// (Callable, Closure)` is correctly `false` in general (not every
// `callable` is a `Closure` instance), but that used to make
// `reconcile_with_native` wrongly discard the doc member even when the
// native hint names `Closure` exactly, collapsing `null|Closure` down
// to bare `null` (`Psl\Dict\filter()`'s real shape). The now-kept
// `Callable` member is further refined back to the native hint's own
// `Closure` - strictly more precise, and (a second real corpus find)
// avoids a *different* false positive: a same-signature override
// looking like an illegal narrowing purely because the ancestor's own
// resolved type had degraded to `Callable` while the child's never did.
#[test]
fn a_callable_shape_doc_member_is_refined_to_the_closure_native_hint() {
    let r = build_registry(
        r#"<?php
            class Closure {}
            /** @param (Closure(mixed): bool)|null $predicate */
            function f(null|Closure $predicate = null) {}
        "#,
    );
    assert_eq!(
        r.functions["f"].params[0].ty,
        Type::Union(vec![
            Type::Object("Closure".to_string(), Vec::new()),
            Type::Null
        ])
    );
}

// Phase 74 corpus find: an `array<K, V>`-shaped doc member's own *key*
// used to be checked against the native `array` hint via the same full
// `is_subtype` as any other member - correct for a concrete key
// (`array<string, int>`'s own `string` really is a subtype of
// `int|string`), but wrong once the key is a `@template`-bound
// placeholder (`array<TKey, TValue>`, `TKey` still a bare, unresolved
// `Object("TKey", [])` at this point - `reconcile_with_native` runs
// *before* `mark_template_params` ever gets a chance to substitute it):
// `is_subtype(Object("TKey", []), int|string)` is correctly `false` for
// an arbitrary class, so the *entire* array member (including `TValue`,
// which would have resolved just fine) was discarded in favor of the
// bare native `array`. A native `array` hint carries no key information
// to contradict in the first place, so the key is now always trusted
// as-is; only the value is still checked against native's own.
#[test]
fn an_array_doc_member_with_a_template_key_is_not_discarded() {
    let r = build_registry(
        r#"<?php
            /**
             * @template TKey
             * @template TValue
             */
            class Foo {
                /** @param array<TKey, TValue> $x */
                public function f(array $x): void {}
            }
        "#,
    );
    let sig = &r.classes["Foo"].methods["f"];
    let Type::Array(key, value) = &sig.params[0].ty else {
        panic!("expected Type::Array, got {:?}", sig.params[0].ty);
    };
    assert!(
        matches!(key.as_ref(), Type::TemplateParam(p) if p.name == "TKey"),
        "got key: {key:?}"
    );
    assert!(
        matches!(value.as_ref(), Type::TemplateParam(p) if p.name == "TValue"),
        "got value: {value:?}"
    );
}

// The same trust extends to a plain (non-template) class name in the
// key position too - `member_possible_given_native` never actually
// checks an array member's own key against the native hint at all (a
// bare `array` carries no key information to contradict in the first
// place), regardless of what the key turns out to be. Real PHP would
// reject `Bar` as an actual array key at runtime, but that's a usage-
// site problem for whatever later reads/writes through it, not a
// reason to sanitize the declared type itself here - the same "trust
// the docblock, let real usage-site checks catch real problems"
// leniency this project applies everywhere else.
#[test]
fn an_array_doc_member_with_a_plain_class_key_is_also_kept() {
    let r = build_registry(
        r#"<?php
            class Bar {}
            class Foo {
                /** @param array<Bar, string> $x */
                public function f(array $x): void {}
            }
        "#,
    );
    let sig = &r.classes["Foo"].methods["f"];
    assert_eq!(
        sig.params[0].ty,
        Type::Array(
            Box::new(Type::Object("Bar".to_string(), Vec::new())),
            Box::new(Type::String)
        )
    );
}

// A `mixed`-native case must still trust the doc fully, now via the
// general `is_subtype(_, Type::Mixed)` rule falling out for free
// rather than a dedicated `mixed`/`object` branch.
#[test]
fn a_mixed_native_hint_still_trusts_the_doc_type_fully() {
    let r = build_registry(
        r#"<?php
            /** @return resource|false */
            function f(): mixed {}
        "#,
    );
    assert_eq!(
        r.functions["f"].return_type,
        Type::Union(vec![
            Type::Object("resource".to_string(), Vec::new()),
            Type::False
        ])
    );
}

// Found via a full corpus re-check after first shipping Finding I as
// a bare `is_subtype(member, native_ty)` filter: `is_subtype`'s own
// `Object` handling is name-equality only (no class hierarchy), so a
// doc type naming a real subclass of a more generic native return
// hint - the actual real corpus shape, a Doctrine-repository-typed
// service declaring a native `EntityRepository` return with a doc
// type naming the real concrete repository subclass - was wrongly
// treated as "incompatible" and discarded in favor of the generic
// native type, a real regression across dozens of real call sites.
// Scoped back to exactly the one class-name case this project can
// prove reliably (a literal `resource` doc member) - any other class
// name must always be kept.
#[test]
fn a_doc_type_naming_a_real_subclass_of_the_native_hint_is_not_discarded() {
    let r = build_registry(
        r#"<?php
            class EntityRepository {}
            class ActivityRepository extends EntityRepository {}
            class ActivityService {
                /** @return ActivityRepository */
                private function getRepository(): EntityRepository {
                    return new ActivityRepository();
                }
            }
        "#,
    );
    assert_eq!(
        r.classes["ActivityService"].methods["getRepository"].return_type,
        Type::Object("ActivityRepository".to_string(), Vec::new())
    );
}

#[test]
fn phpdoc_wins_over_a_conflicting_native_hint() {
    let r = build_registry(
        r#"<?php
            /**
             * @param int[] $items
             */
            function f(array $items) {}
        "#,
    );
    assert_eq!(
        r.functions["f"].params[0].ty,
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int))
    );
}

#[test]
fn phpdoc_types_an_otherwise_untyped_property() {
    let r = build_registry(
        r#"<?php
            class Box {
                /** @var int */
                public $value;
            }
        "#,
    );
    assert_eq!(r.lookup_property("Box", "value").unwrap().ty, Type::Int);
}

#[test]
fn phpdoc_types_a_promoted_constructor_property_via_param_tag() {
    let r = build_registry(
        r#"<?php
            class Point {
                /**
                 * @param int[] $coords
                 */
                public function __construct(public $coords) {}
            }
        "#,
    );
    assert_eq!(
        r.lookup_property("Point", "coords").unwrap().ty,
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int))
    );
}

#[test]
fn missing_doc_comment_falls_back_to_native_hint() {
    let r = build_registry(r#"<?php function f(string $s): string { return $s; }"#);
    assert_eq!(r.functions["f"].params[0].ty, Type::String);
    assert_eq!(r.functions["f"].return_type, Type::String);
}

// `@phpstan-assert`-family tags - real PHPStan's custom-assertion-
// function convention (real corpus shape: phpunit's own accessor pair
// `hasTestFilesFile(): bool` / `testFilesFile(): string`).

#[test]
fn an_assert_if_true_tag_naming_a_this_property_resolves() {
    let r = build_registry(
        r#"<?php
            class Foo {
                /** @phpstan-assert-if-true !null $this->bar */
                public function hasBar(): bool {
                    return $this->bar !== null;
                }
                private ?string $bar = null;
            }
        "#,
    );
    let sig = &r.classes["Foo"].methods["hasBar"];
    assert_eq!(sig.assert_if_true.len(), 1);
    let info = &sig.assert_if_true[0];
    assert!(info.negated);
    assert_eq!(info.ty, Type::Null);
    assert_eq!(info.target, AssertTarget::ThisProp("bar".to_string()));
}

#[test]
fn an_assert_if_false_tag_resolves() {
    let r = build_registry(
        r#"<?php
            class Foo {
                /** @phpstan-assert-if-false !null $x */
                public function isNull($x): bool {
                    return $x === null;
                }
            }
        "#,
    );
    let sig = &r.classes["Foo"].methods["isNull"];
    assert_eq!(sig.assert_if_false.len(), 1);
    assert_eq!(
        sig.assert_if_false[0].target,
        AssertTarget::Param("x".to_string())
    );
}

#[test]
fn an_unconditional_assert_tag_on_the_function_s_own_docblock_resolves() {
    let r = build_registry(
        r#"<?php
            /** @phpstan-assert string $actual */
            function assertIsString($actual): void {}
        "#,
    );
    let sig = &r.functions["assertIsString"];
    assert_eq!(sig.assert.len(), 1);
    assert!(!sig.assert[0].negated);
    assert_eq!(sig.assert[0].ty, Type::String);
    assert_eq!(
        sig.assert[0].target,
        AssertTarget::Param("actual".to_string())
    );
}

#[test]
fn a_bare_this_target_resolves() {
    let r = build_registry(
        r#"<?php
            interface After {}
            class Foo {
                /** @phpstan-assert-if-true After $this */
                public function isAfter(): bool { return false; }
            }
        "#,
    );
    let sig = &r.classes["Foo"].methods["isAfter"];
    assert_eq!(sig.assert_if_true[0].target, AssertTarget::ThisSelf);
}

#[test]
fn a_target_naming_a_nonexistent_parameter_is_dropped() {
    let r = build_registry(
        r#"<?php
            class Foo {
                /** @phpstan-assert-if-true !null $notAnActualParam */
                public function f(): bool { return false; }
            }
        "#,
    );
    let sig = &r.classes["Foo"].methods["f"];
    assert!(sig.assert_if_true.is_empty());
}

#[test]
fn a_method_call_target_is_dropped_not_misclassified() {
    let r = build_registry(
        r#"<?php
            class Foo {
                /** @phpstan-assert-if-false !null $this->parseError() */
                public function isValid(): bool { return true; }
            }
        "#,
    );
    let sig = &r.classes["Foo"].methods["isValid"];
    assert!(sig.assert_if_false.is_empty());
}

#[test]
fn a_template_equality_form_is_never_resolved() {
    let r = build_registry(
        r#"<?php
            class Foo {
                /** @phpstan-assert =ExpectedType $actual */
                public function f($actual): void {}
            }
        "#,
    );
    let sig = &r.classes["Foo"].methods["f"];
    assert!(sig.assert.is_empty());
}
