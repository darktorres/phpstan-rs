//! `flatten_phpdoc_inheritance` - backfilling a missing `@param`/
//! `@return` from an ancestor's own docblock, mirroring PHPStan's
//! `ResolvedPhpDocBlock::mergeParamTags`/`mergeReturnTags`.

use super::super::*;
use super::helpers::*;
use types::AccessoryKind;

/// `list<T>` parses to `Type::Intersection([Array(int, T), Accessory
/// (ArrayList)])` - pulls `T` back out for a plain equality assertion
/// against the expected element type.
fn list_item_type(ty: &Type) -> &Type {
    let Type::Intersection(members) = ty else {
        panic!("expected Type::Intersection (a list<T>), got {ty:?}");
    };
    members
        .iter()
        .find_map(|m| match m {
            Type::Array(_, value) => Some(value.as_ref()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected an Array member inside {ty:?}"))
}

fn is_list_type(ty: &Type) -> bool {
    matches!(ty, Type::Intersection(members) if members.contains(&Type::Accessory(AccessoryKind::ArrayList)))
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
    let sig = &r.classes["InstallView"].methods["pageAssets"];
    assert_eq!(*list_item_type(&sig.return_type), Type::String);
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
    let sig = &r.classes["RealConsumer"].methods["consume"];
    assert_eq!(*list_item_type(&sig.params[0].ty), Type::Int);
    // The native-only field must never be touched by this pass - it
    // feeds `check_one_method_override`'s Liskov check, which must keep
    // comparing native hints only.
    let Type::Array(_, value) = &sig.params[0].ty_native else {
        panic!("expected Type::Array, got {:?}", sig.params[0].ty_native);
    };
    assert_eq!(**value, Type::Mixed(false));
}

#[test]
fn inheritance_applies_even_when_the_native_return_hint_is_already_non_mixed() {
    // The corrected-algorithm case: a bare `array` native hint resolves
    // to `Type::Array(_, Mixed)`, not `Type::Mixed(false)` itself - real
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
    let sig = &r.classes["ConcreteView2"].methods["assets"];
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
    let sig = &r.classes["Child3"].methods["make"];
    assert_eq!(sig.return_type, Type::Int);
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
    let sig = &r.classes["Impl4"].methods["value"];
    assert_eq!(sig.return_type, Type::Int);
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
    let sig = &r.classes["Concrete5"].methods["items"];
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
    let sig = &r.classes["Impl6"].methods["make"];
    assert_eq!(*list_item_type(&sig.return_type), Type::String);
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
    let sig = &r.classes["Sub7"].methods["make"];
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
    let sig = &r.classes["DoctrineCommand8"].methods["__construct"];
    // Position 0 ($dependencyFactory) must be untouched - still resolved
    // from its own native `?DependencyFactory8` hint, never `string|null`.
    assert!(
        matches!(&sig.params[0].ty, Type::Union(members) if members.iter().any(|m| matches!(m, Type::Object(name, _) if name == "DependencyFactory8"))),
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
    let sig = &r.classes["StringVoType9"].methods["convertToPHPValue"];
    assert_ne!(
        sig.return_type,
        Type::Mixed(false),
        "a Mixed ancestor tag must never overwrite a concrete native return type"
    );
}

// This project doesn't model cross-inheritance `@template` binding
// (`@extends Base<Concrete>` fixing a parent's own type parameter - see
// `Type::Object`'s own doc comment) - a template-parameterized ancestor
// type is only meaningful within the ancestor's own `@template` scope,
// so backfilling it verbatim onto an unrelated descendant must never
// happen, or the binding is silently lost, resolving to the far less
// specific interface type.
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
    let sig = &r.classes["ConcreteContainer10"].methods["get"];
    assert_eq!(
        sig.return_type,
        Type::Mixed(false),
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
    let sig = &r.classes["ConcreteContainer11"].methods["get"];
    assert_eq!(
        sig.return_type,
        Type::Object("Widget11".into(), Vec::new()),
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
    let sig = &r.classes["ConcreteRepo13"].methods["getReference"];
    let Type::ClassString(inner) = &sig.params[0].ty else {
        panic!(
            "expected the param to backfill to ClassString(TemplateParam(T)), got {:?}",
            sig.params[0].ty
        );
    };
    assert!(
        matches!(**inner, Type::TemplateParam(_)),
        "expected a TemplateParam inside the ClassString, got {inner:?}"
    );
    let Type::Union(members) = &sig.return_type else {
        panic!(
            "expected the return type to backfill to a Union containing TemplateParam and Null, got {:?}",
            sig.return_type
        );
    };
    assert!(
        members.iter().any(|m| matches!(m, Type::TemplateParam(_))),
        "expected a TemplateParam member in the return union, got {:?}",
        sig.return_type
    );
    assert!(
        members.contains(&Type::Null),
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
    let sig = &r.classes["Concrete11"].methods["getFactory"];
    assert_eq!(
        sig.return_type,
        Type::Object("SpecificFactory11".into(), Vec::new())
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
// through (`FunctionSig::resolve_type`'s own doc comment) - filtering
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
    let sig = &r.classes["ConstantStringType12"].methods["getValue"];
    assert_eq!(
        sig.return_type,
        Type::String,
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
    let sig = &r.classes["StrC12"].methods["setValue"];
    assert_eq!(
        sig.params[0].ty,
        Type::String,
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
    let sig = &r.classes["BaseFactory"].methods["all"];
    match list_item_type(&sig.return_type) {
        Type::TemplateParam(tp) => {
            assert_eq!(tp.name, "M");
            assert_eq!(tp.scope.as_deref(), Some("BaseFactory"));
        }
        other => panic!("expected BaseFactory's own M, got {other:?}"),
    }
}
