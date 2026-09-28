//! Function and method signature resolution, including phpdoc typing and override merging.

use super::super::*;
use super::helpers::*;

#[test]
fn a_native_mixed_parameter_hint_resolves_to_explicit_mixed() {
    // Phase 84's own `Type::Mixed(bool)` - the literal `mixed` keyword,
    // written natively here, is real PHPStan's own definition of
    // *explicit* mixed (`vendor/phpstan-src/src/Type/
    // ParserNodeTypeToPHPStanType.php`).
    let r = build_registry(r#"<?php function g(mixed $m): void {}"#);
    let sig = r.functions.get("g").unwrap();
    assert!(sig.params[0].ty.is_explicit_mixed());
}

#[test]
fn an_untyped_parameter_resolves_to_implicit_mixed() {
    let r = build_registry(r#"<?php function g($m): void {}"#);
    let sig = r.functions.get("g").unwrap();
    assert!(sig.params[0].ty.is_implicit_mixed());
}

#[test]
fn the_object_type_hint_is_not_namespace_qualified() {
    // `object` is a PHP-reserved word - no real class can ever be
    // named that, so the native `object` type hint must never be
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
    assert_eq!(sig.return_type, Type::object_without_class());
}

#[test]
fn a_leading_backslash_in_a_native_type_hint_is_not_re_namespaced() {
    // A leading `\` (root-anchored, "absolute, from the global
    // namespace") must survive into `resolve_names_in_type` as the
    // "already fully qualified, don't touch" signal - `types::
    // parse_type_atom` must not strip it while computing a bare name for
    // keyword matching, or an absolute reference gets silently
    // mis-qualified as namespace-relative.
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
        Type::object(
            ("Doctrine\\ORM\\QueryBuilder".to_string()).into(),
            Vec::new()
        )
    );
}

#[test]
fn a_root_anchored_native_closure_type_hint_is_not_namespace_qualified() {
    // A root-anchored `\Closure` must keep its leading `\` through
    // `types::parse_type_keyword_or_class`'s `"closure"` keyword arm, not
    // get hardcoded to the bare unqualified name - `NameContext::resolve`
    // needs to see it to know not to re-qualify it.
    let r = build_registry(
        r#"<?php
            namespace App;
            class Foo {
                public function bar(\Closure $callback): void {}
            }"#,
    );
    let sig = r.lookup_method("App\\Foo", "bar").unwrap();
    assert_eq!(sig.params[0].ty, Type::object("Closure".into(), Vec::new()));
}

#[test]
fn a_bare_unqualified_closure_type_hint_defers_to_a_real_use_import() {
    // Deliberately NOT given the same treatment as the root-anchored
    // case above: a bare, unqualified `Closure` is genuinely ambiguous -
    // a file may `use Some\Other\Namespace\Closure;` an entirely
    // different class and type-hint bare `Closure` meaning that one, not
    // the native global class. Must resolve through the same
    // `use`-import lookup any other class reference gets.
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
        Type::object(
            ("Some\\Other\\Namespace_\\Closure".to_string()).into(),
            Vec::new()
        )
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
    assert_eq!(sig.params[0].ty, Type::int());
    assert_eq!(sig.return_type, Type::int());
}

// phpstorm-stubs represents "this parameter changed across PHP versions"
// as the same conceptual parameter declared twice, each gated by its own
// `#[PhpStormStubsElementAvailable]` attribute - PHP can't have two
// same-named parameters in one function, so a duplicate name reaching
// `FunctionReflection::from_decl` is unconditionally this versioning artifact.
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
    assert_eq!(sig.params[0].name, "connection".into());
    assert_eq!(
        sig.params[0].ty,
        Type::object(("PgSql\\Connection".to_string()).into(), Vec::new()),
        "the surviving type must be the modern (second) occurrence's, not the legacy (first, untyped) one"
    );
    assert_eq!(sig.params[1].name, "query".into());
    assert_eq!(sig.params[1].ty, Type::string());
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
    assert_eq!(sig.params[1].name, "flags".into());
    assert_eq!(sig.params[1].ty, Type::int());
    assert_eq!(sig.params[2].name, "encryptionKey".into());
    assert_eq!(sig.params[2].ty, Type::string());
}

// `fsockopen`'s duplicated `$port` - the duplicate breaks argument
// binding since it isn't the last parameter.
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
            .map(|p| p.name.as_ref())
            .collect::<Vec<_>>(),
        vec!["hostname", "port", "error_code", "error_message", "timeout"]
    );
}

#[test]
fn a_function_with_no_duplicate_parameters_is_unaffected() {
    let r = build_registry(r#"<?php function f(int $a, string $b) {}"#);
    let sig = &r.functions["f"];
    assert_eq!(sig.params.len(), 2);
    assert_eq!(sig.params[0].name, "a".into());
    assert_eq!(sig.params[1].name, "b".into());
}

// A `LanguageLevelTypeAware` parameter override must win over a
// competing legacy `@param` docblock tag, even though the override is
// only ever injected as a native type hint and `resolve_type` normally
// treats a matching `@param` tag as authoritative over the native hint.
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
        Type::union_of(vec![
            Type::string(),
            Type::int(),
            Type::float(),
            Type::bool(),
            Type::null(),
        ]),
        "the modern override must win over the stale '@param string' tag"
    );
}

// A stale doc type of `resource` must not defeat the override either -
// `resolve_type`'s `resource`-stripping must not fire on a
// never-overridden stale doc type in place of the real override.
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
        Type::object("finfo".into(), Vec::new())
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
        Type::resource(false),
        "earlier's own docblock must be untouched by later's override"
    );
    assert_eq!(
        r.functions["later"].params[0].ty,
        Type::object("finfo".into(), Vec::new()),
        "later's override must still win via the native-hint-only path"
    );
}

// `resolve_type`'s docblock-vs-native reconciliation applies one general
// rule via `is_super_type_of`: a present, non-`object` native hint is
// authoritative about what's possible, so any docblock member
// incompatible with it can never actually happen and is dropped.
#[test]
fn a_stale_false_member_is_dropped_when_the_native_hint_no_longer_allows_it() {
    let r = build_registry(
        r#"<?php
            /** @return int|false */
            function imagesx(): int {}
        "#,
    );
    assert_eq!(r.functions["imagesx"].return_type, Type::int());
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
        Type::union_of(vec![Type::int(), Type::bool_literal(false)])
    );
}

// `member_possible_given_native`'s `Object => true`/`Callable => true`
// leniency (deliberately permissive since it usually can't disprove a
// class-hierarchy claim) must not extend to a native `void` return,
// which admits nothing at all, not even a class or callable.
#[test]
fn a_stale_object_member_is_dropped_when_the_native_hint_is_void() {
    let r = build_registry(
        r#"<?php
            class Foo14 {}
            /** @return Foo14 */
            function make14(): void {}
        "#,
    );
    assert_eq!(r.functions["make14"].return_type, Type::void());
}

// `Type::Never` must be exempted from the native-`void` exclusion above -
// an explicit `@return never` means the function always diverges
// (throws/exits/loops forever), compatible with any native return hint
// including `void` (mirrors PHPStan's own `TypehintHelper::decideType`
// special case, checked before its general `isSuperTypeOf` rule).
// Collapsing this to plain `void` would lose the one piece of
// information control-flow-termination analysis needs to recognize the
// call as always-diverging.
#[test]
fn an_explicit_never_doc_survives_reconciliation_against_a_native_void_hint() {
    let r = build_registry(
        r#"<?php
            /** @return never */
            function alwaysThrows15(): void {
                throw new \RuntimeException();
            }
        "#,
    );
    assert_eq!(r.functions["alwaysThrows15"].return_type, Type::never());
}

// A PHPStan/Psalm callable-shape doc member (`Closure(Tv): bool`) parses
// into a real, shaped `Type::Callable(Some(sig))`, not a bare
// signature-less marker - `refine_callable_member`'s guard
// (`registry/function_sig/type_resolution.rs`) must not touch an
// already-shaped member, since it's already a genuine refinement in its
// own right, strictly more precise than degrading it to a signature-less
// `Closure` object type would be. `is_super_type_of`'s `(Callable(Some(_)),
// Callable(None))` leniency (`types/subtype.rs`) keeps this compatible
// with the native `Closure` hint.
#[test]
fn a_callable_shape_doc_member_keeps_its_own_real_signature_alongside_the_closure_native_hint() {
    let r = build_registry(
        r#"<?php
            class Closure {}
            /** @param (Closure(mixed): bool)|null $predicate */
            function f(null|Closure $predicate = null) {}
        "#,
    );
    let Type::Union(members) = &r.functions["f"].params[0].ty else {
        panic!(
            "expected a top-level union, got {:?}",
            r.functions["f"].params[0].ty
        );
    };
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].to_string(), "Closure(mixed): bool");
    assert_eq!(members[1], Type::null());
}

// A doc union that only narrows *one* member of a native union must keep
// the native's *other*, untouched alternatives alongside the refined one
// - `reconcile_with_native` previously only ever filtered the doc type's
// own member list down, never added a native member back. Real shape:
// `Pest\Browser\Api\Concerns\InteractsWithElements::keys(string $selector,
// array|string $keys)`, `@param array<int, string> $keys` - confirmed via
// `dumpType()` against the real method that real PHPStan's own resolved
// type keeps both `array<int, string>` (the refined alternative) and the
// untouched `string` alternative.
#[test]
fn a_doc_union_refining_one_native_union_member_keeps_the_others_untouched() {
    let r = build_registry(
        r#"<?php
            /** @param array<int, string> $keys */
            function f(array|string $keys) {}
        "#,
    );
    let Type::Union(members) = &r.functions["f"].params[0].ty else {
        panic!(
            "expected a top-level union, got {:?}",
            r.functions["f"].params[0].ty
        );
    };
    assert_eq!(members.len(), 2, "got {members:?}");
    assert!(
        members.contains(&Type::String),
        "the untouched native `string` alternative must survive: {members:?}"
    );
    assert!(
        matches!(&members[0], Type::Array(_, v) if **v == Type::String)
            || matches!(&members[1], Type::Array(_, v) if **v == Type::String),
        "the refined `array<int, string>` alternative must also survive: {members:?}"
    );
}

// An `array<K, V>`-shaped doc member's *key* must not be checked against
// the native `array` hint the way any other member is - a `@template`-
// bound key placeholder (`array<TKey, TValue>`) is still a bare,
// unresolved `Object("TKey", [])` when `reconcile_with_native` runs
// (before `mark_template_params` substitutes it), so `is_super_type_of`
// against it is correctly `false` for an arbitrary class and would
// discard the entire array member, including a `TValue` that would have
// resolved fine. A native `array` hint carries no key information to
// contradict in the first place, so the key is always trusted as-is;
// only the value is still checked against the native hint.
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
        Type::array(Type::object("Bar".into(), Vec::new()), Type::string())
    );
}

// A `mixed`-native case must still trust the doc fully, now via the
// general `subtype(_, Type::Mixed(false))` rule falling out for free
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
        Type::union_of(vec![Type::resource(false), Type::bool_literal(false)])
    );
}

// `is_super_type_of`'s `Object` handling is name-equality only (no class
// hierarchy), so a doc type naming a real subclass of a more generic
// native return hint must not be treated as "incompatible" and
// discarded in favor of the generic native type - any class name doc
// member must always be kept (the reconciliation above only ever
// discards the one class-name case it can prove reliably, a literal
// `resource` doc member).
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
        Type::object("ActivityRepository".into(), Vec::new())
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
        Type::array(Type::benevolent_array_key(), Type::int())
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
    assert_eq!(r.lookup_property("Box", "value").unwrap().ty, Type::int());
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
        Type::array(Type::benevolent_array_key(), Type::int())
    );
}

// A promoted property's own inline `/** @var T */` doubles as that
// property's own docblock, alongside an `@param $name` tag on the
// enclosing `__construct()`'s own docblock.
#[test]
fn phpdoc_types_a_promoted_constructor_property_via_its_own_inline_var_tag() {
    let r = build_registry(
        r#"<?php
            class Point93 {
                public function __construct(
                    /**
                     * @var array<int, int>
                     */
                    public $coords,
                ) {}
            }
        "#,
    );
    assert_eq!(
        r.lookup_property("Point93", "coords").unwrap().ty,
        Type::array(Type::int(), Type::int())
    );
}

// The same, with an attribute sitting between the inline doc comment and
// the parameter itself - confirms the doc comment attaches to the
// parameter, not silently discarded once an attribute comes between the
// two.
#[test]
fn an_inline_var_tag_survives_an_attribute_between_it_and_the_parameter() {
    let r = build_registry(
        r#"<?php
            #[Attribute]
            class Column93 {}
            class Entity93 {
                public function __construct(
                    /**
                     * @var array<string, mixed>|null
                     */
                    #[Column93]
                    public ?array $details,
                ) {}
            }
        "#,
    );
    let ty = &r.lookup_property("Entity93", "details").unwrap().ty;
    assert!(
        matches!(ty, Type::Union(members) if members.iter().any(|m| matches!(m, Type::Array(_, v) if v.is_mixed()))),
        "got: {ty:?}"
    );
}

#[test]
fn missing_doc_comment_falls_back_to_native_hint() {
    let r = build_registry(r#"<?php function f(string $s): string { return $s; }"#);
    assert_eq!(r.functions["f"].params[0].ty, Type::string());
    assert_eq!(r.functions["f"].return_type, Type::string());
}

// `@phpstan-assert`-family tags - PHPStan's custom-assertion-function
// convention.

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
    assert_eq!(info.ty, Type::null());
    assert_eq!(
        info.target,
        AssertTarget::ThisProp("bar".to_string().into())
    );
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
        AssertTarget::Param("x".to_string().into())
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
    assert_eq!(sig.assert[0].ty, Type::string());
    assert_eq!(
        sig.assert[0].target,
        AssertTarget::Param("actual".to_string().into())
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
fn a_target_naming_a_nonexistent_parameter_classifies_as_not_found() {
    // `AssertTarget::NotFound` carries the offending name through so the
    // checker can report `parameter.notFound`.
    let r = build_registry(
        r#"<?php
            class Foo {
                /** @phpstan-assert-if-true !null $notAnActualParam */
                public function f(): bool { return false; }
            }
        "#,
    );
    let sig = &r.classes["Foo"].methods["f"];
    assert_eq!(sig.assert_if_true.len(), 1);
    assert_eq!(
        sig.assert_if_true[0].target,
        AssertTarget::NotFound("notAnActualParam".to_string().into())
    );
}

#[test]
fn a_method_call_target_classifies_as_unknown_not_misclassified() {
    // `AssertTarget::Unknown` carries the raw target text through so the
    // checker can report `assert.unknownExpr`.
    let r = build_registry(
        r#"<?php
            class Foo {
                /** @phpstan-assert-if-false !null $this->parseError() */
                public function isValid(): bool { return true; }
            }
        "#,
    );
    let sig = &r.classes["Foo"].methods["isValid"];
    assert_eq!(sig.assert_if_false.len(), 1);
    assert_eq!(
        sig.assert_if_false[0].target,
        AssertTarget::Unknown("$this->parseError()".to_string().into())
    );
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
