//! `flatten_phpdoc_inheritance` - backfilling a missing `@param`/
//! `@return` from an ancestor's own docblock, mirroring PHPStan's
//! `ResolvedPhpDocBlock::mergeParamTags`/`mergeReturnTags`.

use super::super::*;
use super::helpers::*;

/// A `list<T>`'s `T`.
fn list_item_type(ty: &Type) -> Type {
    assert!(ty.is_list().yes(), "expected a list<T>, got {ty:?}");
    ty.array_value_type()
        .unwrap_or_else(|| panic!("expected a list<T>, got {ty:?}"))
}

fn is_list_type(ty: &Type) -> bool {
    ty.is_list().yes()
}

fn method<'a>(registry: &'a Registry, class: &str, name: &str) -> &'a FunctionReflection {
    registry
        .lookup_own_method(class, name)
        .expect("method exists")
}

#[test]
fn a_return_type_is_inherited_from_an_interface_when_the_implementor_has_no_own_return_doc() {
    let r = build_registry(
        r#"<?php
            interface HasPageAssets {
                /** @return list<string> */
                public function pageAssets(): array;
            }
            class InstallView implements HasPageAssets {
                public function pageAssets(): array { return []; }
            }
            "#,
    );
    let sig = method(&r, "InstallView", "pageAssets");
    assert_eq!(list_item_type(&sig.return_type), Type::string());
}

#[test]
fn inherited_phpdoc_overlays_win_over_a_deferred_method_snapshot() {
    let r = build_registry(
        r#"<?php
            class DeferredConfig { public const VALUES = ['first', 'second']; }
            interface DeferredFactory {
                /** @return value-of<DeferredConfig::VALUES> */
                public function make(): string;
            }
            class DeferredFactoryImpl implements DeferredFactory {
                /** @param value-of<DeferredConfig::VALUES> $seed */
                public function make(string $seed = 'first'): string { return $seed; }
            }
            "#,
    );

    let sig = method(&r, "DeferredFactoryImpl", "make");
    assert_eq!(
        sig.return_type,
        Type::string_literal("first".into()).union(Type::string_literal("second".into()))
    );
    assert_eq!(
        sig.params[0].ty,
        Type::string_literal("first".into()).union(Type::string_literal("second".into()))
    );
}

#[test]
fn a_param_type_is_inherited_from_an_interface_and_ty_native_is_left_alone() {
    let r = build_registry(
        r#"<?php
            interface Consumer {
                /** @param list<int> $ids */
                public function consume(array $ids): void;
            }
            class RealConsumer implements Consumer {
                public function consume(array $ids): void {}
            }
            "#,
    );
    let sig = method(&r, "RealConsumer", "consume");
    assert_eq!(list_item_type(&sig.params[0].ty), Type::int());
    // The native-only field must never be touched by this pass - it
    // feeds `check_one_method_override`'s Liskov check, which must keep
    // comparing native hints only.
    assert_eq!(
        sig.params[0].ty_native.array_value_type(),
        Some(Type::mixed()),
        "got {:?}",
        sig.params[0].ty_native
    );
}

#[test]
fn inheritance_applies_even_when_the_native_return_hint_is_already_non_mixed() {
    // The corrected-algorithm case: a bare `array` native hint resolves
    // to `array<mixed>`, not `mixed` itself - real
    // PHPStan's own trigger is "no own `@return` tag at all", never
    // gated on the resolved type being `Mixed`.
    let r = build_registry(
        r#"<?php
            interface HasAssets2 {
                /** @return list<string> */
                public function assets(): array;
            }
            class ConcreteView2 implements HasAssets2 {
                public function assets(): array { return []; }
            }
            "#,
    );
    let sig = method(&r, "ConcreteView2", "assets");
    assert!(is_list_type(&sig.return_type));
}

#[test]
fn a_parent_class_wins_over_an_implemented_interface_when_both_declare_the_method() {
    let r = build_registry(
        r#"<?php
            interface IFace3 {
                /** @return string */
                public function make(): mixed;
            }
            class ParentClass3 implements IFace3 {
                /** @return int */
                public function make(): mixed { return 1; }
            }
            class Child3 extends ParentClass3 {
                public function make(): mixed { return 1; }
            }
            "#,
    );
    let sig = method(&r, "Child3", "make");
    assert_eq!(sig.return_type, Type::int());
}

#[test]
fn the_first_declared_interface_wins_never_a_union_of_several() {
    let r = build_registry(
        r#"<?php
            interface IA4 {
                /** @return int */
                public function value(): mixed;
            }
            interface IB4 {
                /** @return string */
                public function value(): mixed;
            }
            class Impl4 implements IA4, IB4 {
                public function value(): mixed { return 1; }
            }
            "#,
    );
    let sig = method(&r, "Impl4", "value");
    assert_eq!(sig.return_type, Type::int());
}

#[test]
fn a_three_level_chain_resolves_through_the_fixed_point_loop() {
    // Grandparent interface has the doc; the middle interface and the
    // concrete class both redeclare the method with no doc of their
    // own - a single non-iterative pass could only see one level.
    let r = build_registry(
        r#"<?php
            interface Grandparent5 {
                /** @return list<int> */
                public function items(): array;
            }
            interface Middle5 extends Grandparent5 {
                public function items(): array;
            }
            class Concrete5 implements Middle5 {
                public function items(): array { return []; }
            }
            "#,
    );
    let sig = method(&r, "Concrete5", "items");
    assert!(is_list_type(&sig.return_type));
}

#[test]
fn a_childs_own_return_doc_is_never_overwritten() {
    let r = build_registry(
        r#"<?php
            interface IFace6 {
                /** @return list<int> */
                public function make(): array;
            }
            class Impl6 implements IFace6 {
                /** @return list<string> */
                public function make(): array { return []; }
            }
            "#,
    );
    let sig = method(&r, "Impl6", "make");
    assert_eq!(list_item_type(&sig.return_type), Type::string());
}

#[test]
fn a_private_ancestor_method_is_never_inherited_from() {
    let r = build_registry(
        r#"<?php
            class Base7 {
                /** @return list<int> */
                private function make(): array { return []; }
            }
            class Sub7 extends Base7 {
                public function make(): array { return []; }
            }
            "#,
    );
    let sig = method(&r, "Sub7", "make");
    assert!(
        !is_list_type(&sig.return_type),
        "a private ancestor method must never be inherited from, got {:?}",
        sig.return_type
    );
}

// PHP never enforces constructor-signature compatibility (arity, order,
// and names can all differ freely between a class and its parent), so
// backfilling ancestor param docs onto a constructor must match by
// name, not position - a positionally-matched ancestor's differently-
// named parameter at the same index would wrongly get stamped with the
// wrong type.
#[test]
fn a_constructor_matches_ancestor_params_by_name_not_position() {
    let r = build_registry(
        r#"<?php
            class DependencyFactory8 {}
            class Command8 {
                /** @param string|null $name */
                public function __construct(?string $name = null) {}
            }
            class DoctrineCommand8 extends Command8 {
                public function __construct(
                    private ?DependencyFactory8 $dependencyFactory = null,
                    ?string $name = null,
                ) {}
            }
            "#,
    );
    let sig = method(&r, "DoctrineCommand8", "__construct");
    // Position 0 ($dependencyFactory) must be untouched - still resolved
    // from its own native `?DependencyFactory8` hint, never `string|null`.
    assert!(
        sig.params[0]
            .ty
            .object_class_names()
            .iter()
            .any(|name| *name == "DependencyFactory8"),
        "got: {:?}",
        sig.params[0].ty
    );
    // Position 1 ($name) genuinely shares the ancestor's own name, so it
    // still legitimately inherits (a harmless no-op here, since the
    // native hint already agrees).
    assert!(sig.params[1].has_own_doc);
}

// An ancestor's literal `@return mixed` tag must never be inherited onto
// a concrete override with a specific native return type, since a
// `mixed` tag can only ever destroy information, never add any.
#[test]
fn an_ancestors_mixed_tag_is_never_inherited() {
    let r = build_registry(
        r#"<?php
            class DbalType9 {
                /** @return mixed */
                public function convertToPHPValue(mixed $value): mixed { return $value; }
            }
            class StringVoType9 extends DbalType9 {
                public function convertToPHPValue(mixed $value): ?string { return null; }
            }
            "#,
    );
    let sig = method(&r, "StringVoType9", "convertToPHPValue");
    assert_ne!(
        sig.return_type,
        Type::mixed(),
        "a Mixed ancestor tag must never overwrite a concrete native return type"
    );
}

// A descendant that implements a generic ancestor without binding its
// template (no `@implements Container10<X>`) doesn't inherit the
// ancestor's template-typed return: `T` means nothing in its scope.
#[test]
fn an_ancestors_template_parameterized_type_is_never_inherited() {
    let r = build_registry(
        r#"<?php
            /** @template T */
            interface Container10 {
                /** @return T */
                public function get(): mixed;
            }
            class ConcreteContainer10 implements Container10 {
                public function get(): mixed { return null; }
            }
            "#,
    );
    let sig = method(&r, "ConcreteContainer10", "get");
    assert_eq!(
        sig.return_type,
        Type::mixed(),
        "a template-parameterized ancestor type must never be inherited verbatim, got {:?}",
        sig.return_type
    );
}

// Corrected sibling of the erased case just above: a class-scoped
// `@template` is safe to backfill after all, provided the descendant
// actually supplies a concrete binding for it via `@implements
// Ancestor<Concrete>` - `Registry::template_param_scopes_all_safe_to_backfill`'s
// own doc comment has the full reasoning for why this is safe (a
// resolvable binding, not the still-unresolvable erased case above).
// Resolved immediately, at backfill time, to the concrete bound type -
// not left as a raw `TemplateParam` for some later call-time pass to
// substitute (`Registry::substitute_class_scoped_ancestor_templates`'s
// own doc comment explains why: this same signature is also read
// directly when checking its own declaring class's method body, a path
// with no "external call site" to resolve anything through at all).
#[test]
fn an_ancestors_template_parameterized_type_is_inherited_when_a_concrete_binding_exists() {
    let r = build_registry(
        r#"<?php
            /** @template T */
            interface Container11 {
                /** @return T */
                public function get(): mixed;
            }
            final class Widget11 {}
            /** @implements Container11<Widget11> */
            class ConcreteContainer11 implements Container11 {
                public function get(): mixed { return new Widget11(); }
            }
            "#,
    );
    let sig = method(&r, "ConcreteContainer11", "get");
    assert_eq!(
        sig.return_type,
        Type::object("Widget11".into(), Vec::new()),
        "expected the ancestor's own scoped TemplateParam to resolve to the concrete bound type immediately, got {:?}",
        sig.return_type
    );
}

// A `@template` declared directly on the ancestor method itself is safe
// to backfill verbatim regardless of any class-level binding.
#[test]
fn a_method_scoped_template_parameterized_type_is_inherited() {
    let r = build_registry(
        r#"<?php
            interface Repo13 {
                /**
                 * @template T of object
                 * @param class-string<T> $entityName
                 * @return T|null
                 */
                public function getReference(string $entityName, mixed $id): object|null;
            }
            class ConcreteRepo13 implements Repo13 {
                public function getReference(string $entityName, mixed $id): object|null { return null; }
            }
            "#,
    );
    let sig = method(&r, "ConcreteRepo13", "getReference");
    assert_eq!(
        sig.params[0].ty.to_string(),
        "class-string<T>",
        "expected the param to backfill to class-string<T>"
    );
    let members = sig.return_type.union_members();
    assert!(
        members.iter().any(|m| m.is_template()),
        "expected a TemplateParam member in the return union, got {:?}",
        sig.return_type
    );
    assert!(
        members.contains(&Type::null()),
        "expected Null to survive in the return union, got {:?}",
        sig.return_type
    );
}

// An ancestor method's own native hint can already resolve (via that
// file's own `use` import) to a concrete class - but a grandparent
// interface documents the same method name against a same-named but
// genuinely different, more generic class (its own `use` resolves the
// identifier differently). Neither the `Mixed` guard nor the
// template-param guard catches this (both sides are concrete,
// non-template types) - only a same-class-name check does.
#[test]
fn an_ancestors_object_type_naming_a_different_class_than_the_native_hint_is_never_inherited() {
    let r = build_registry(
        r#"<?php
            class GenericFactory11 {}
            class SpecificFactory11 {}
            interface Grandparent11 {
                /** @return GenericFactory11 */
                public function getFactory(): mixed;
            }
            interface Middle11 extends Grandparent11 {
                public function getFactory(): SpecificFactory11;
            }
            class Concrete11 implements Middle11 {
                public function getFactory(): SpecificFactory11 { return new SpecificFactory11(); }
            }
            "#,
    );
    let sig = method(&r, "Concrete11", "getFactory");
    assert_eq!(
        sig.return_type,
        Type::object("SpecificFactory11".into(), Vec::new())
    );
}

// Neither `safe_to_backfill_mixed` nor `safe_to_backfill_object_type`
// catches an ancestor's non-`Mixed`, non-`Object` doc type that's simply
// incompatible with the child's own concrete native return type - a
// broad ancestor union must not be blindly cloned wholesale onto the
// child, discarding its own, already-correct, narrower native type for
// something far less precise. The inherited doc type must be
// reconciled against the child's native type via the same
// `reconcile_with_native` an own (non-inherited) docblock already goes
// through (`FunctionReflection::resolve_type`'s own doc comment) - filtering
// the inherited union down to whichever member(s) the native type can
// actually be, mirroring PHPStan's `TypehintHelper::decideType`
// (`$type->isSuperTypeOf($phpDocType)`).
#[test]
fn an_ancestors_doc_return_type_incompatible_with_the_native_return_type_is_reconciled_not_cloned_verbatim()
 {
    let r = build_registry(
        r#"<?php
            interface ConstantScalarType12 {
                /** @return int|float|string|bool|null */
                public function getValue();
            }
            class ConstantStringType12 implements ConstantScalarType12 {
                public function getValue(): string { return "x"; }
            }
            "#,
    );
    let sig = method(&r, "ConstantStringType12", "getValue");
    assert_eq!(
        sig.return_type,
        Type::string(),
        "the child's own concrete native return type must win over an \
         ancestor's incompatible, broader doc type, got {:?}",
        sig.return_type
    );
}

// Same bug, parameter side: `ScalarC12::setValue()`'s own `@param
// int|float|string|bool|null $v` blindly overwrote `StrC12::setValue()`'s
// already-correct, narrower native `string $v` - silently making every
// call passing a real `int`/`float`/`bool`/`null` argument (only ever
// legal for the *interface's* own doc-implied signature, not this
// concrete override's real, narrower one) look valid.
#[test]
fn an_ancestors_doc_param_type_incompatible_with_the_native_param_type_is_reconciled_not_cloned_verbatim()
 {
    let r = build_registry(
        r#"<?php
            interface ScalarC12 {
                /** @param int|float|string|bool|null $v */
                public function setValue($v): void;
            }
            class StrC12 implements ScalarC12 {
                public function setValue(string $v): void {}
            }
            "#,
    );
    let sig = method(&r, "StrC12", "setValue");
    assert_eq!(
        sig.params[0].ty,
        Type::string(),
        "the child's own concrete native param type must win over an \
         ancestor's incompatible, broader doc type, got {:?}",
        sig.params[0].ty
    );
}

#[test]
fn an_inherited_return_keeps_the_implementors_own_template_in_place_of_the_interfaces() {
    let r = build_registry(
        r#"<?php
            interface Meta {}
            /** @template T of Meta */
            interface Factory {
                /** @return list<T> */
                public function all(): array;
            }
            /**
             * @template M of Meta
             * @template-implements Factory<M>
             */
            abstract class BaseFactory implements Factory {
                /** {@inheritDoc} */
                public function all(): array { return []; }
            }
            "#,
    );
    let sig = method(&r, "BaseFactory", "all");
    let item = list_item_type(&sig.return_type);
    assert!(
        item.is_template(),
        "expected BaseFactory's own M, got {item:?}"
    );
    let tp = item.nested_templates()[0];
    assert_eq!(tp.name, "M");
    assert_eq!(tp.scope.as_deref(), Some("BaseFactory"));
}

#[test]
fn a_docblock_reaches_a_descendant_seven_levels_down_whatever_the_declaration_order() {
    // Declared from the last descendant up, so no class comes after its
    // ancestors in the file.
    let r = build_registry(
        r#"<?php
            class L6 extends L5 { public function items(): array { return []; } }
            class L5 extends L4 { public function items(): array { return []; } }
            class L4 extends L3 { public function items(): array { return []; } }
            class L3 extends L2 { public function items(): array { return []; } }
            class L2 extends L1 { public function items(): array { return []; } }
            class L1 extends L0 { public function items(): array { return []; } }
            class L0 {
                /** @return list<string> */
                public function items(): array { return []; }
            }
            "#,
    );
    for class in ["L1", "L2", "L3", "L4", "L5", "L6"] {
        let sig = method(&r, class, "items");
        assert_eq!(list_item_type(&sig.return_type), Type::string(), "{class}");
    }
}

#[test]
fn a_docblock_comes_down_an_interface_and_a_class_chain_that_meet() {
    let r = build_registry(
        r#"<?php
            interface Source {
                /** @param list<int> $ids */
                public function load(array $ids): void;
            }
            interface Cached extends Source {}
            class Base implements Cached {
                public function load(array $ids): void {}
            }
            class Middle extends Base {
                public function load(array $ids): void {}
            }
            class Leaf extends Middle implements Cached {
                public function load(array $ids): void {}
            }
            "#,
    );
    for class in ["Base", "Middle", "Leaf"] {
        let sig = method(&r, class, "load");
        assert_eq!(list_item_type(&sig.params[0].ty), Type::int(), "{class}");
    }
}

#[test]
fn a_class_that_is_its_own_ancestor_does_not_stop_the_others_inheriting() {
    let r = build_registry(
        r#"<?php
            class LoopA extends LoopB { public function f(): array { return []; } }
            class LoopB extends LoopA { public function f(): array { return []; } }
            class Root {
                /** @return list<int> */
                public function f(): array { return []; }
            }
            class Child extends Root { public function f(): array { return []; } }
            "#,
    );
    assert_eq!(
        list_item_type(&method(&r, "Child", "f").return_type),
        Type::int()
    );
    assert!(r.has_class("LoopA") && r.has_class("LoopB"));
}
