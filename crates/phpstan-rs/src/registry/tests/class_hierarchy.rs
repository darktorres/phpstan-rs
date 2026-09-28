//! Class/interface/trait hierarchy: inheritance, trait flattening, member and property lookup.

use super::super::*;
use super::helpers::*;
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
    let iface = r.classes.get("Iface").unwrap();
    assert_eq!(iface.kind, ClassKind::Interface);

    let base = r.classes.get("Base").unwrap();
    assert_eq!(base.kind, ClassKind::Class);
    assert!(base.is_abstract);
    assert!(!base.is_final);
    assert!(base.abstract_methods.contains("required"));
    assert!(base.final_methods.contains("locked"));
    assert!(base.properties.get("id").unwrap().is_readonly);
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
    let base = r.classes.get("Base").unwrap();
    assert!(!base.final_methods.contains("__clone"));
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
    let foo = r.classes.get("Foo").unwrap();
    assert!(foo.methods.contains_key("greet"));
    assert!(foo.methods.contains_key("extra"));
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
    let deep = r.classes.get("Deep").unwrap();
    assert!(deep.methods.contains_key("m1"));
    assert!(deep.methods.contains_key("m2"));
    assert!(deep.methods.contains_key("m3"));
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
                    B::hello as helloB;
                }
            }
            "#,
    );
    let c = r.classes.get("C").unwrap();
    assert!(c.methods.contains_key("hello"));
    assert!(c.methods.contains_key("helloB"));
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
    let y = r.classes.get("Y").unwrap();
    assert!(y.methods.contains_key("greet"));
    assert!(y.methods.contains_key("greetAlias"));
}

#[test]
fn class_consts_are_registered_and_typed_from_their_literal_value() {
    let r = build_registry(
        r#"<?php
            class Config {
                const NAME = "app";
                const MAX = 10;
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
    assert!(r.lookup_const("Config", "MISSING").is_none());
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
    assert_eq!(
        r.lookup_property("Person", "greeting").unwrap().ty,
        Type::string()
    );
}

#[test]
fn trait_constants_are_flattened_in() {
    // Same shape as `trait_methods_and_properties_are_flattened_in`
    // just above, but for PHP 8.2+ trait constants specifically - these
    // were never merged into a using class's own `consts` map at all
    // (only methods/properties were), so `lookup_const` against the
    // using class silently found nothing.
    let r = build_registry(
        r#"<?php
            trait Greets {
                public const string GREETING = "hi";
            }
            class Person {
                use Greets;
            }
        "#,
    );
    assert_eq!(
        r.lookup_const("Person", "GREETING").unwrap().ty,
        Type::string_literal("hi".into())
    );
}

#[test]
fn trait_flattening_works_across_namespaced_classes() {
    // `flatten_traits` iterates `self.classes` (FQN-keyed) and resolves
    // each class's own `uses` list (also FQN, via `ClassReflection::from_decl`'s
    // `ctx.resolve`) to look trait method/property tables up by - both
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
