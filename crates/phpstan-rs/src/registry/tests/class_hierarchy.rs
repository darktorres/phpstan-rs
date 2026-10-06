//! Class/interface/trait hierarchy: inheritance, trait flattening, member and property lookup.

use super::super::*;
use super::helpers::*;
use std::sync::Arc;
use syntax::ast::ClassKind;

#[test]
fn class_info_records_its_own_kind_and_final_abstract_readonly_members() {
    let r = build_registry(
        r#"<?php
            interface Iface {}
            abstract class Base {
                abstract public function required(): void;
                final public function locked(): void {}
                public readonly int $id;
            }
            "#,
    );
    let iface = r.class_by_name("Iface").unwrap();
    assert_eq!(iface.kind, ClassKind::Interface);

    let base = r.class_by_name("Base").unwrap();
    assert_eq!(base.kind, ClassKind::Class);
    assert!(base.is_abstract);
    assert!(!base.is_final);
    assert!(base.is_abstract_method("required"));
    assert!(base.is_final_method("locked"));
    assert!(base.properties.get("id").unwrap().is_readonly);
}

#[test]
fn method_native_exclusion_is_a_result_not_a_declaration_write() {
    let r = build_registry(
        r#"<?php
            class NativeBound {}
            class Excluded {}
            class Demo {
                /** @param NativeBound|Excluded $value */
                public function accept(NativeBound $value): void {}
            }
            "#,
    );
    let declaration = r.class_by_name("Demo").unwrap().methods["accept"].clone();
    let effective = r.lookup_own_method("Demo", "accept").unwrap();

    assert_ne!(declaration.params[0].ty, effective.params[0].ty);
    assert_eq!(
        r.class_by_name("Demo").unwrap().methods["accept"],
        declaration
    );
}

#[test]
fn method_constant_named_types_are_results_not_declaration_writes() {
    let r = build_registry(
        r#"<?php
            const ACCEPTED_MODE = 7;
            class Demo {
                /** @param ACCEPTED_MODE $mode */
                public function accept($mode): void {}
            }
            "#,
    );
    let declaration = r.class_by_name("Demo").unwrap().methods["accept"].clone();
    let effective = r.lookup_own_method("Demo", "accept").unwrap();

    assert_ne!(declaration.params[0].ty, effective.params[0].ty);
    assert_eq!(
        r.class_by_name("Demo").unwrap().methods["accept"],
        declaration
    );
}

#[test]
fn base_switch_retags_id_valued_ancestor_closures_and_keeps_unresolved_edges() {
    let mut registry = build_registry("<?php class Old {} ");
    let replacement =
        build_registry("<?php class Parent {} class Child extends Parent implements Missing {}");

    registry.switch_base(replacement);

    assert!(registry.is_subclass_of("Child", "Parent"));
    assert!(registry.is_subclass_of("Child", "Missing"));
    assert_eq!(
        registry.ancestor_names_of("Child").unwrap().len(),
        2,
        "registered IDs resolve back to names after generation retagging"
    );
}

#[test]
fn a_final_private_method_is_not_recorded_as_final() {
    // A `private` method isn't part of a class's inherited/
    // overridable surface at all in real PHP - `final` on one is
    // inert as far as override-compatibility checking goes.
    let r = build_registry(
        r#"<?php
            class Base { final private function __clone() {} }
            "#,
    );
    let base = r.class_by_name("Base").unwrap();
    assert!(!base.is_final_method("__clone"));
}

#[test]
fn a_trait_using_another_trait_is_flattened_transitively() {
    let r = build_registry(
        r#"<?php
            trait Greets { public function greet(): string { return "hi"; } }
            trait Combined { use Greets; public function extra(): int { return 1; } }
            class Foo { use Combined; }
            "#,
    );
    let foo = r.class_by_name("Foo").unwrap();
    assert!(!foo.methods.contains_key("greet"));
    assert!(!foo.methods.contains_key("extra"));
    assert!(r.lookup_own_method("Foo", "greet").is_some());
    assert!(r.lookup_own_method("Foo", "extra").is_some());
}

#[test]
fn a_three_level_trait_chain_is_flattened_transitively() {
    let r = build_registry(
        r#"<?php
            trait A2 { public function m1(): int { return 1; } }
            trait B2 { use A2; public function m2(): int { return 2; } }
            trait C2 { use B2; public function m3(): int { return 3; } }
            class Deep { use C2; }
            "#,
    );
    let deep = r.class_by_name("Deep").unwrap();
    assert!(!deep.methods.contains_key("m1"));
    assert!(!deep.methods.contains_key("m2"));
    assert!(!deep.methods.contains_key("m3"));
    for method in ["m1", "m2", "m3"] {
        assert!(r.lookup_own_method("Deep", method).is_some());
    }
}

#[test]
fn a_trait_as_alias_registers_the_method_under_its_new_name_too() {
    let r = build_registry(
        r#"<?php
            trait A { public function hello(): string { return "A"; } }
            trait B { public function hello(): string { return "B"; } }
            class C {
                use A, B {
                    A::hello insteadof B;
                    A::hello as helloA;
                    B::hello as helloB;
                }
            }
            "#,
    );
    let c = r.class_by_name("C").unwrap();
    assert!(!c.methods.contains_key("hello"));
    assert!(!c.methods.contains_key("helloA"));
    assert!(!c.methods.contains_key("helloB"));
    let source = r.shared_own_method("C", "hello").unwrap();
    let alias = r.shared_own_method("C", "helloA").unwrap();
    assert!(Arc::ptr_eq(&source, &alias));
}

#[test]
fn an_unqualified_trait_as_alias_resolves_against_the_used_trait() {
    let r = build_registry(
        r#"<?php
            trait X { public function greet(): string { return "x"; } }
            class Y {
                use X {
                    greet as greetAlias;
                }
            }
            "#,
    );
    let y = r.class_by_name("Y").unwrap();
    assert!(!y.methods.contains_key("greet"));
    assert!(!y.methods.contains_key("greetAlias"));
    assert!(r.lookup_own_method("Y", "greet").is_some());
    assert!(r.lookup_own_method("Y", "greetAlias").is_some());
}

#[test]
fn trait_method_flags_are_preserved_on_composed_methods_and_aliases() {
    let r = build_registry(
        r#"<?php
            trait T {
                abstract public function needs(): void;
                public static function run(): void {}
                final public function locked(): void {}
                /** @final */
                public function documentedFinal(): void {}
            }
            abstract class C {
                use T {
                    needs as needsAlias;
                    run as runAlias;
                    locked as lockedAlias;
                    documentedFinal as documentedFinalAlias;
                }
            }
            "#,
    );
    for method in ["needs", "needsAlias"] {
        assert!(r.method_is_abstract("C", method), "{method}");
        assert!(!r.method_is_static("C", method), "{method}");
    }
    for method in ["run", "runAlias"] {
        assert!(!r.method_is_abstract("C", method), "{method}");
        assert!(r.method_is_static("C", method), "{method}");
    }
    for method in ["locked", "lockedAlias"] {
        assert!(r.method_is_final("C", method), "{method}");
    }
    for method in ["documentedFinal", "documentedFinalAlias"] {
        assert!(r.method_is_final_by_phpdoc("C", method), "{method}");
    }
    assert!(r.method_is_static("C", "RUNALIAS"));
}

#[test]
fn trait_mixin_and_phpdoc_members_live_in_host_topology() {
    let r = build_registry(
        r#"<?php
            trait TraitMethods { public function fromTrait(): int { return 1; } }
            class MixinMethods { public function fromMixin(): string { return 'm'; } }
            /**
             * @mixin MixinMethods
             * @method bool fromTag()
             */
            class Host { use TraitMethods { fromTrait as traitAlias; } }
            "#,
    );
    let host = r.class_by_name("Host").unwrap();
    let declaration_methods: Vec<_> = host.methods.keys().map(|name| name.to_string()).collect();
    assert!(declaration_methods.is_empty(), "{declaration_methods:?}");

    for name in ["fromTrait", "traitAlias", "fromMixin", "fromTag"] {
        assert!(r.lookup_own_method("Host", name).is_some(), "{name}");
        assert!(r.member_slot("Host", name).is_some(), "{name}");
    }
    let entries: Vec<_> = r
        .member_entries("Host")
        .map(|entry| entry.name.to_string())
        .collect();
    assert_eq!(entries, ["traitAlias", "fromTrait", "fromTag", "fromMixin"]);
    for (slot, name) in entries.iter().enumerate() {
        assert_eq!(r.member_slot("Host", name), Some(slot as u16));
    }
}

#[test]
fn a_final_trait_property_keeps_its_flag_on_the_using_class() {
    let r = build_registry(
        r#"<?php
            trait T { final public int $id; }
            class Host { use T; }
            class Child extends Host { public int $id; }
            class Grandchild extends Host {}
            "#,
    );
    assert!(
        !r.class_by_name("Host")
            .unwrap()
            .properties
            .contains_key("id")
    );
    assert!(r.lookup_property("Host", "id").unwrap().is_final());
    assert!(r.lookup_property("Grandchild", "id").unwrap().is_final());
    assert_eq!(
        r.find_final_ancestor_property("Child", "id")
            .map(|(name, by_phpdoc)| (name.to_string(), by_phpdoc)),
        Some(("Host".to_owned(), false))
    );
}

#[test]
fn inherited_property_hooks_are_registry_results_not_declaration_writes() {
    let r = build_registry(
        r#"<?php
            class ParentHook {
                public string $name { set(string $value) {} }
            }
            class ChildHook extends ParentHook {
                public string $name { get => 'child'; }
            }
            "#,
    );
    let declared = r
        .classes
        .get("ChildHook")
        .unwrap()
        .properties
        .get("name")
        .unwrap();
    assert!(!declared.has_set_hook());
    let resolved = r.lookup_property("ChildHook", "name").unwrap();
    assert!(resolved.has_get_hook());
    assert!(resolved.has_set_hook());
}

#[test]
fn class_consts_are_registered_and_typed_from_their_literal_value() {
    let r = build_registry(
        r#"<?php
            class Config {
                const NAME = "app";
                const MAX = 10;
                const COPY = self::NAME;
                const COPY_AGAIN = self::COPY;
            }"#,
    );
    assert_eq!(
        r.lookup_const("Config", "NAME").map(|cs| &cs.ty),
        Some(&Type::string_literal("app".into()))
    );
    assert_eq!(
        r.lookup_const("Config", "MAX").map(|cs| &cs.ty),
        Some(&Type::int_literal(10))
    );
    assert_eq!(
        r.lookup_const_with_owner_and_slot("Config", "NAME")
            .map(|(owner, slot, constant)| (owner.to_string(), slot, &constant.ty)),
        Some(("Config".to_owned(), 0, &Type::string_literal("app".into())))
    );
    assert_eq!(
        r.lookup_const_with_owner_and_slot("Config", "MAX")
            .map(|(_, slot, _)| slot),
        Some(1)
    );
    assert_eq!(
        r.lookup_const_type("Config", "COPY"),
        Some(&Type::string_literal("app".into()))
    );
    assert_eq!(
        r.lookup_const_type("Config", "COPY_AGAIN"),
        Some(&Type::string_literal("app".into()))
    );
    assert!(
        r.lookup_const("Config", "COPY")
            .unwrap()
            .ty()
            .contains_deferred(),
        "the declaration keeps its deferred input type"
    );
    assert_eq!(
        r.lookup_const_with_owner_and_slot("Config", "COPY")
            .map(|(_, slot, _)| slot),
        Some(2)
    );
    assert_eq!(
        r.lookup_const_with_owner_and_slot("Config", "COPY_AGAIN")
            .map(|(_, slot, _)| slot),
        Some(3)
    );
    let id = *r.class_ids.get("Config").unwrap();
    assert_eq!(
        r.class_const_types.get(&(id, 2)),
        Some(&Type::string_literal("app".into()))
    );
    assert_eq!(
        r.class_const_types.get(&(id, 3)),
        Some(&Type::string_literal("app".into()))
    );
    assert!(r.lookup_const("Config", "MISSING").is_none());
}

#[test]
fn imported_type_constant_results_leave_the_declaration_unchanged() {
    let r = build_registry(
        r#"<?php
            /** @phpstan-type Imported string */
            class Source {}
            /** @phpstan-import-type Imported from Source */
            class Target {
                /** @var Imported */
                public const VALUE = null;
                /** @var Imported */
                public $property;
            }
            "#,
    );

    assert_eq!(
        r.lookup_const_type("Target", "VALUE"),
        Some(&Type::string())
    );
    assert_ne!(
        r.lookup_const("Target", "VALUE").unwrap().ty(),
        &Type::string(),
        "the declaration keeps its unresolved imported-alias input"
    );
    assert_eq!(
        r.lookup_property("Target", "property").unwrap().ty(),
        &Type::string()
    );
    assert_ne!(
        r.classes
            .get("Target")
            .unwrap()
            .properties
            .get("property")
            .unwrap()
            .ty(),
        &Type::string(),
        "the property declaration keeps its imported-alias input"
    );
    let target = r.class_by_name("Target").unwrap();
    assert!(target.type_aliases().is_empty());
    assert_eq!(target.type_imports().len(), 1);
    assert_eq!(
        r.effective_type_aliases_by_name("Target").get("Imported"),
        Some(&Type::string()),
        "resolved imports live in the Registry result overlay"
    );
}

#[test]
fn class_deferred_generic_types_are_results_not_declaration_writes() {
    let registry = build_registry(
        r#"<?php
            class Config { public const VALUES = ['first', 'second']; }
            /** @template T of value-of<Config::VALUES> */
            class Bounded {}
            /** @template T */
            class ParentGeneric {}
            /** @extends ParentGeneric<value-of<Config::VALUES>> */
            class ChildGeneric extends ParentGeneric {}
            /** @template T */
            interface GenericContract {}
            /** @implements GenericContract<value-of<Config::VALUES>> */
            class ContractUser implements GenericContract {}
            "#,
    );
    let expected =
        Type::string_literal("first".into()).union(Type::string_literal("second".into()));

    assert!(registry.class_by_name("Bounded").unwrap().template_bounds["T"].contains_deferred());
    assert_eq!(
        registry
            .effective_template_bounds_by_name("Bounded")
            .get("T"),
        Some(&expected)
    );

    assert!(registry.class_by_name("ChildGeneric").unwrap().extends_args[0].contains_deferred());
    assert_eq!(
        registry.effective_extends_args_by_name("ChildGeneric"),
        vec![expected.clone()]
    );

    assert!(
        registry
            .class_by_name("ContractUser")
            .unwrap()
            .implements_args["GenericContract"][0]
            .contains_deferred()
    );
    assert_eq!(
        registry
            .effective_implements_args_by_name("ContractUser")
            .get("GenericContract"),
        Some(&vec![expected])
    );
}

#[test]
fn deferred_value_of_property_results_leave_the_declaration_unchanged() {
    let r = build_registry(
        r#"<?php
            class Config { const NAME = 'app'; }
            class UsesConfig {
                /** @var value-of<Config::NAME> */
                public $name;
            }
            "#,
    );
    let expected = Type::value_of(Type::string_literal("app".into()));
    assert_eq!(
        r.lookup_property("UsesConfig", "name").unwrap().ty(),
        &expected
    );
    assert_ne!(
        r.classes
            .get("UsesConfig")
            .unwrap()
            .properties
            .get("name")
            .unwrap()
            .ty(),
        &expected,
        "the declaration keeps its deferred value-of input"
    );
}

#[test]
fn class_consts_are_inherited_through_extends() {
    let r = build_registry(
        r#"<?php
            class Base { const KIND = "base"; }
            class Derived extends Base {}"#,
    );
    assert_eq!(
        r.lookup_const("Derived", "KIND").map(|cs| &cs.ty),
        Some(&Type::string_literal("base".into()))
    );
    assert_eq!(
        r.lookup_const_with_owner_and_slot("Derived", "KIND")
            .map(|(owner, slot, _)| (owner.to_string(), slot)),
        Some(("Base".to_owned(), 0))
    );
}

#[test]
fn transitive_class_inheritance() {
    let r = build_registry(
        r#"<?php
            interface Shape {}
            abstract class BaseShape implements Shape {}
            class Circle extends BaseShape {}
        "#,
    );
    assert!(r.is_subclass_of("Circle", "BaseShape"));
    assert!(r.is_subclass_of("Circle", "Shape"));
    assert!(!r.is_subclass_of("BaseShape", "Circle"));
}

#[test]
fn interface_extending_multiple_interfaces() {
    let r = build_registry(
        r#"<?php
            interface A {}
            interface B {}
            interface C extends A, B {}
            class Impl implements C {}
        "#,
    );
    assert!(r.is_subclass_of("Impl", "A"));
    assert!(r.is_subclass_of("Impl", "B"));
}

#[test]
fn method_lookup_walks_up_extends_chain() {
    let r = build_registry(
        r#"<?php
            class Base { public function greet(): string { return "hi"; } }
            class Sub extends Base {}
        "#,
    );
    let sig = r.lookup_method("Sub", "greet").expect("inherited method");
    assert_eq!(sig.return_type.to_string(), "string");
}

#[test]
fn declared_property_type_is_tracked() {
    let r = build_registry(
        r#"<?php
            class Box { public int $value = 0; }
        "#,
    );
    assert_eq!(r.lookup_property("Box", "value").unwrap().ty, Type::int());
}

#[test]
fn constructor_promotion_registers_a_property() {
    let r = build_registry(
        r#"<?php
            class Point {
                public function __construct(private int $x, public string $label) {}
            }
        "#,
    );
    assert_eq!(r.lookup_property("Point", "x").unwrap().ty, Type::int());
    assert_eq!(
        r.lookup_property("Point", "label").unwrap().ty,
        Type::string()
    );
}

#[test]
fn trait_methods_and_properties_are_flattened_in() {
    let r = build_registry(
        r#"<?php
            trait Greets {
                public string $greeting = "hi";
                public function greet(): string { return "hi"; }
            }
            class Person {
                use Greets;
            }
        "#,
    );
    assert_eq!(
        r.lookup_method("Person", "greet").unwrap().return_type,
        Type::string()
    );
    assert!(
        !r.classes
            .get("Person")
            .unwrap()
            .properties
            .contains_key("greeting")
    );
    assert!(r.property_names("Person").any(|name| name == "greeting"));
    assert_eq!(
        r.lookup_property("Person", "greeting").unwrap().ty,
        Type::string()
    );
}

#[test]
fn trait_constants_are_flattened_in() {
    // Same shape as `trait_methods_and_properties_are_flattened_in`
    // just above, but for PHP 8.2+ trait constants specifically. The
    // result stays outside the host declaration while Registry lookup
    // serves it to the host and its subclasses.
    let r = build_registry(
        r#"<?php
            trait Greets {
                public const string GREETING = "hi";
                public const COPY = self::GREETING;
            }
            class Person {
                use Greets;
            }
            class Child extends Person {}
        "#,
    );
    assert!(
        !r.classes
            .get("Person")
            .unwrap()
            .consts
            .contains_key("GREETING")
    );
    assert!(r.constant_names("Person").any(|name| name == "GREETING"));
    assert_eq!(
        r.lookup_const("Person", "GREETING").unwrap().ty,
        Type::string_literal("hi".into())
    );
    assert_eq!(
        r.lookup_const("Child", "GREETING").unwrap().ty,
        Type::string_literal("hi".into())
    );
    assert_eq!(
        r.lookup_const_type("Person", "COPY"),
        Some(&Type::string_literal("hi".into()))
    );
}

#[test]
fn trait_flattening_works_across_namespaced_classes() {
    // `flatten_traits` iterates `self.classes` (FQN-keyed) and resolves
    // each class's own `uses` list (also FQN, as name resolution resolved
    // it) to look trait method/property tables up by - both
    // sides need to actually agree for this to find anything at all, so
    // this must be tested with namespaced (not bare) PHP.
    let r = build_registry(
        r#"<?php
            namespace App;
            trait Greets {
                public string $greeting = "hi";
                public function greet(): string { return "hi"; }
            }
            class Person {
                use Greets;
            }
        "#,
    );
    assert_eq!(
        r.lookup_method("App\\Person", "greet").unwrap().return_type,
        Type::string()
    );
    assert_eq!(
        r.lookup_property("App\\Person", "greeting").unwrap().ty,
        Type::string()
    );
}

#[test]
fn trait_flattening_also_works_for_enums_not_just_classes() {
    // Enums can use traits in real PHP, and `flatten_traits`/
    // `lookup_method` don't distinguish a class's `ClassReflection` from an
    // enum's - covers `ClassReflection::from_enum_decl`'s `uses` closure
    // (resolving an enum's own `use SomeTrait;`).
    let r = build_registry(
        r#"<?php
            trait Greets {
                public function greet(): string { return "hi"; }
            }
            enum Status {
                use Greets;
                case Active;
            }
        "#,
    );
    assert_eq!(
        r.lookup_method("Status", "greet").unwrap().return_type,
        Type::string()
    );
}

#[test]
fn trait_used_before_its_declaration_still_resolves() {
    // PHP resolves `use` at compile time, not top-to-bottom - a trait
    // declared later in the file must still flatten in correctly.
    let r = build_registry(
        r#"<?php
            class Person {
                use Greets;
            }
            trait Greets {
                public function greet(): string { return "hi"; }
            }
        "#,
    );
    assert!(r.lookup_method("Person", "greet").is_some());
}

#[test]
fn class_method_overrides_trait_method_of_same_name() {
    let r = build_registry(
        r#"<?php
            trait Greets {
                public function greet(): string { return "generic"; }
            }
            class Person {
                use Greets;
                public function greet(): int { return 1; }
            }
        "#,
    );
    assert_eq!(
        r.lookup_method("Person", "greet").unwrap().return_type,
        Type::int()
    );
}

#[test]
fn property_lookup_walks_up_extends_chain() {
    let r = build_registry(
        r#"<?php
            class Base { public string $name = ""; }
            class Sub extends Base {}
        "#,
    );
    assert_eq!(r.lookup_property("Sub", "name").unwrap().ty, Type::string());
}

// `is_intersection_possible` - real PHPStan's own `UnresolvableTypeHelper`
// reference semantics for a native intersection type (`Foo&Bar`).

#[test]
fn two_unrelated_concrete_classes_are_never_possible() {
    let r = build_registry(
        r#"<?php
            class Foo {}
            class Bar {}
        "#,
    );
    assert!(!r.is_intersection_possible(&["Foo".to_string(), "Bar".to_string()]));
}

#[test]
fn a_class_and_its_own_ancestor_are_possible() {
    let r = build_registry(
        r#"<?php
            class Base {}
            class Sub extends Base {}
        "#,
    );
    assert!(r.is_intersection_possible(&["Sub".to_string(), "Base".to_string()]));
}

#[test]
fn two_unrelated_interfaces_are_possible() {
    let r = build_registry(
        r#"<?php
            interface A {}
            interface B {}
        "#,
    );
    assert!(r.is_intersection_possible(&["A".to_string(), "B".to_string()]));
}

#[test]
fn a_non_final_class_and_an_unrelated_interface_are_possible() {
    let r = build_registry(
        r#"<?php
            class Foo {}
            interface Bar {}
        "#,
    );
    assert!(r.is_intersection_possible(&["Foo".to_string(), "Bar".to_string()]));
}

#[test]
fn a_final_class_and_an_interface_it_doesnt_implement_is_impossible() {
    let r = build_registry(
        r#"<?php
            final class Foo {}
            interface Bar {}
        "#,
    );
    assert!(!r.is_intersection_possible(&["Foo".to_string(), "Bar".to_string()]));
}

#[test]
fn a_final_class_and_an_interface_it_does_implement_is_possible() {
    let r = build_registry(
        r#"<?php
            interface Bar {}
            final class Foo implements Bar {}
        "#,
    );
    assert!(r.is_intersection_possible(&["Foo".to_string(), "Bar".to_string()]));
}

#[test]
fn an_unregistered_class_is_never_treated_as_impossible() {
    let r = build_registry(r#"<?php class Foo {}"#);
    assert!(r.is_intersection_possible(&["Foo".to_string(), "SomeUnregisteredClass".to_string()]));
}
