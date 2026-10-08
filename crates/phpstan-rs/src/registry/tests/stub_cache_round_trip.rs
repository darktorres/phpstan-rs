//! What the stub cache stores must read back: the resolved registry and
//! the declarations as built, with every class and function entry decoding.

use super::helpers::build_registry;
use crate::project::Project;
use crate::registry::container::{Reader, Writer};
use crate::registry::{MethodRef, Registry, Storage, effects::EffectId};
use syntax::name::Name;
use types::Type;

/// `registry` stored and read back, and frozen, as a run loads it from the
/// stub cache.
fn round_trip(registry: &Registry) -> Registry {
    let mut writer = Writer::new();
    registry.store(&mut writer).expect("stores");
    let reader = match Reader::new(writer.finish()) {
        Ok(reader) => reader,
        Err(_) => panic!("a stored registry is a whole file"),
    };
    let mut back = Registry::load(&reader).expect("reads back");
    back.check_stored().expect("every stored table is in order");
    back.freeze();
    back
}

/// The stub registry of `Project::new` stored and read back, once for all
/// the tests here: storing encodes every entry and reading decodes the ones
/// a test asks for, which is most of what these tests cost.
fn read_back() -> &'static Registry {
    static BACK: std::sync::LazyLock<Registry> =
        std::sync::LazyLock::new(|| round_trip(Project::new().registry()));
    &BACK
}

#[test]
fn the_resolved_stub_registry_survives_a_round_trip() {
    let project = Project::new();
    let registry = project.registry();
    let back = read_back();

    assert_eq!(back.classes.len(), registry.iter_classes().count());
    assert_eq!(back.functions.len(), registry.iter_functions().count());
    for (name, class) in registry.iter_classes() {
        assert_eq!(back.classes.get(name), Some(class), "class {name}");
    }
    for (name, function) in registry.iter_functions() {
        assert_eq!(back.functions.get(name), Some(function), "function {name}");
    }
}

#[test]
fn deferred_class_constant_results_survive_freeze_and_cache_round_trip() {
    let registry = build_registry(
        r#"<?php
            class Config {
                const NAME = 'app';
                const COPY = self::NAME;
                const COPY_AGAIN = self::COPY;
            }"#,
    );
    let back = round_trip(&registry);
    let expected = Type::string_literal("app".into());

    assert_eq!(
        registry.lookup_const_type("Config", "COPY"),
        Some(&expected)
    );
    assert_eq!(
        registry.lookup_const_type("Config", "COPY_AGAIN"),
        Some(&expected)
    );
    assert_eq!(back.lookup_const_type("Config", "COPY"), Some(&expected));
    assert_eq!(
        back.lookup_const_type("Config", "COPY_AGAIN"),
        Some(&expected)
    );
    assert!(
        back.lookup_const("Config", "COPY")
            .unwrap()
            .ty()
            .contains_deferred(),
        "the serialized declaration remains the unresolved source"
    );
}

#[test]
fn function_signature_results_survive_freeze_and_cache_round_trip() {
    let registry = build_registry(
        r#"<?php
            const ANSWER = 42;
            /** @param ANSWER $value */
            function reads_answer($value): void {}
            class Config { public const VALUES = ['first', 'second']; }
            /** @param value-of<Config::VALUES> $value */
            function accepts_value(string $value): void {}
            "#,
    );
    let declared = registry
        .function_declaration_by_name("reads_answer")
        .unwrap()
        .clone();
    let value_of_declaration = registry
        .function_declaration_by_name("accepts_value")
        .unwrap()
        .clone();
    let effective = registry.lookup_function("reads_answer").unwrap();
    assert_eq!(effective.params[0].ty, Type::int_literal(42));
    assert_eq!(
        registry
            .function_declaration_by_name("reads_answer")
            .unwrap(),
        &declared
    );
    assert!(value_of_declaration.params[0].ty.contains_deferred());
    assert_eq!(
        registry.lookup_function("accepts_value").unwrap().params[0].ty,
        registry.relation().union(
            Type::string_literal("first".into()),
            Type::string_literal("second".into())
        )
    );
    assert_eq!(
        registry
            .function_declaration_by_name("accepts_value")
            .unwrap(),
        &value_of_declaration
    );

    let back = round_trip(&registry);
    assert_eq!(
        back.lookup_function("reads_answer").unwrap().params[0].ty,
        Type::int_literal(42)
    );
    assert_eq!(
        back.function_declaration_by_name("reads_answer").unwrap(),
        &declared
    );
    assert_eq!(
        back.function_declaration_by_name("accepts_value").unwrap(),
        &value_of_declaration
    );
    assert_eq!(
        back.lookup_function("accepts_value").unwrap().params[0].ty,
        back.relation().union(
            Type::string_literal("first".into()),
            Type::string_literal("second".into())
        )
    );
}

#[test]
fn inherited_class_property_results_survive_cache_round_trip() {
    let registry = build_registry(
        r#"<?php
            class ParentHook {
                public string $name { set(string $value) {} }
            }
            class ChildHook extends ParentHook {
                public string $name { get => 'child'; }
            }
            /** @phpstan-type Imported string */
            class Source {}
            /** @phpstan-import-type Imported from Source */
            class Target {
                /** @var Imported */
                public $property;
            }
            trait HasIdentifier { final public int $id; }
            class TraitHost { use HasIdentifier; }
            trait HasGreeting { public const string GREETING = "hi"; }
            class TraitConstHost { use HasGreeting; }
            "#,
    );
    let back = round_trip(&registry);
    let child_id = back.class_ids["ChildHook"];
    let child_property = back.property_ref_by_name(child_id, "name").unwrap();
    assert!(back.class_property_results.contains_key(&child_property));
    assert_eq!(back.property_name_by_ref(child_property).unwrap(), "name");
    let resolved = back.lookup_property("ChildHook", "name").unwrap();
    assert!(resolved.has_get_hook());
    assert!(resolved.has_set_hook());
    assert!(
        !back
            .classes
            .get("ChildHook")
            .unwrap()
            .properties
            .get("name")
            .unwrap()
            .has_set_hook(),
        "the cached declaration remains the as-built property"
    );
    assert_eq!(
        back.lookup_property("Target", "property").unwrap().ty(),
        &Type::string()
    );
    assert_ne!(
        back.classes
            .get("Target")
            .unwrap()
            .properties
            .get("property")
            .unwrap()
            .ty(),
        &Type::string(),
        "the cached declaration keeps its imported-alias input"
    );
    assert!(
        !back
            .classes
            .get("TraitHost")
            .unwrap()
            .properties
            .contains_key("id"),
        "trait properties stay out of the host declaration"
    );
    assert!(back.lookup_property("TraitHost", "id").unwrap().is_final());
    let host_id = back.class_ids["TraitHost"];
    let trait_property = back.property_ref_by_name(host_id, "id").unwrap();
    assert!(back.class_property_results.contains_key(&trait_property));
    assert!(
        !back
            .classes
            .get("TraitConstHost")
            .unwrap()
            .consts
            .contains_key("GREETING"),
        "trait constants stay out of the host declaration"
    );
    assert_eq!(
        back.lookup_const("TraitConstHost", "GREETING")
            .unwrap()
            .ty(),
        &Type::string_literal("hi".into())
    );
}

#[test]
fn imported_type_alias_results_survive_freeze_and_cache_round_trip() {
    let registry = build_registry(
        r#"<?php
            /** @phpstan-type Imported string */
            class AliasSource {}
            /** @phpstan-import-type Imported from AliasSource */
            class AliasTarget {
                /** @var Imported */
                public $value;
            }
            "#,
    );
    let target = registry.class_by_name("AliasTarget").unwrap();
    assert!(target.type_aliases().is_empty());
    assert_eq!(target.type_imports().len(), 1);
    assert_eq!(
        registry
            .effective_type_aliases_by_name("AliasTarget")
            .get("Imported"),
        Some(&Type::string())
    );

    let back = round_trip(&registry);
    let target = back.class_by_name("AliasTarget").unwrap();
    assert!(target.type_aliases().is_empty());
    assert_eq!(target.type_imports().len(), 1);
    assert_eq!(
        back.effective_type_aliases_by_name("AliasTarget")
            .get("Imported"),
        Some(&Type::string())
    );
    assert_eq!(
        back.lookup_property("AliasTarget", "value").unwrap().ty(),
        &Type::string()
    );
}

#[test]
fn canonical_class_references_are_results_and_survive_cache_round_trip() {
    let registry = build_registry(
        r#"<?php
            namespace RefCase;
            class ParentTarget {}
            interface ContractTarget {}
            trait TraitTarget { public function original(): void {} }
            /** @phpstan-type Imported string */
            class TypeSource {}
            /** @phpstan-import-type Imported from typesource */
            class Consumer extends parenttarget implements contracttarget {
                use traittarget { traittarget::original as renamed; }
            }
            "#,
    );
    let id = registry
        .class_ids
        .get("RefCase\\Consumer")
        .copied()
        .unwrap();
    let declared = registry.class_by_id(id).unwrap();
    assert_eq!(declared.extends, vec![Name::from("RefCase\\parenttarget")]);
    assert_eq!(
        declared.implements,
        vec![Name::from("RefCase\\contracttarget")]
    );
    assert_eq!(declared.uses, vec![Name::from("RefCase\\traittarget")]);
    assert_eq!(
        declared.type_imports[0].source_class_fqn,
        Name::from("RefCase\\typesource")
    );

    let refs = registry.class_references(id).unwrap();
    assert_eq!(refs.extends, &[Name::from("RefCase\\ParentTarget")]);
    assert_eq!(refs.implements, &[Name::from("RefCase\\ContractTarget")]);
    assert_eq!(refs.uses, &[Name::from("RefCase\\TraitTarget")]);
    assert_eq!(
        refs.trait_aliases[0].trait_name,
        Some(Name::from("RefCase\\TraitTarget"))
    );
    assert_eq!(
        refs.type_imports[0].source_class_fqn,
        Name::from("RefCase\\TypeSource")
    );
    assert!(registry.is_subclass_of("RefCase\\Consumer", "RefCase\\ParentTarget"));
    assert!(registry.is_subclass_of("RefCase\\Consumer", "RefCase\\ContractTarget"));
    assert!(
        registry
            .all_ancestor_names("RefCase\\Consumer")
            .contains("RefCase\\TraitTarget")
    );
    assert!(
        registry
            .lookup_own_method("RefCase\\Consumer", "renamed")
            .is_some()
    );

    let back = round_trip(&registry);
    let back_id = back.class_ids.get("RefCase\\Consumer").copied().unwrap();
    let back_declared = back.class_by_id(back_id).unwrap();
    assert_eq!(back_declared.extends, declared.extends);
    assert_eq!(back_declared.implements, declared.implements);
    assert_eq!(back_declared.uses, declared.uses);
    assert_eq!(back_declared.trait_aliases, declared.trait_aliases);
    assert_eq!(back_declared.type_imports, declared.type_imports);
    let back_refs = back.class_references(back_id).unwrap();
    assert_eq!(back_refs.extends, refs.extends);
    assert_eq!(back_refs.implements, refs.implements);
    assert_eq!(back_refs.uses, refs.uses);
    assert_eq!(back_refs.trait_aliases, refs.trait_aliases);
    assert_eq!(back_refs.type_imports, refs.type_imports);
    assert!(back.is_subclass_of("RefCase\\Consumer", "RefCase\\ParentTarget"));
    assert!(back.is_subclass_of("RefCase\\Consumer", "RefCase\\ContractTarget"));
    assert!(
        back.all_ancestor_names("RefCase\\Consumer")
            .contains("RefCase\\TraitTarget")
    );
    assert!(
        back.lookup_own_method("RefCase\\Consumer", "renamed")
            .is_some()
    );
}

#[test]
fn class_deferred_generic_type_results_survive_cache_round_trip() {
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
    let expected = registry.relation().union(
        Type::string_literal("first".into()),
        Type::string_literal("second".into()),
    );
    let back = round_trip(&registry);

    assert!(back.class_by_name("Bounded").unwrap().template_bounds["T"].contains_deferred());
    assert_eq!(
        back.effective_template_bounds_by_name("Bounded").get("T"),
        Some(&expected)
    );
    assert!(back.class_by_name("ChildGeneric").unwrap().extends_args[0].contains_deferred());
    assert_eq!(
        back.effective_extends_args_by_name("ChildGeneric"),
        vec![expected.clone()]
    );
    assert!(
        back.class_by_name("ContractUser").unwrap().implements_args["GenericContract"][0]
            .contains_deferred()
    );
    assert_eq!(
        back.effective_implements_args_by_name("ContractUser")
            .get("GenericContract"),
        Some(&vec![expected])
    );
}

#[test]
fn trait_method_results_survive_freeze_and_cache_round_trip() {
    let registry = build_registry(
        r#"<?php
            trait SourceMethods {
                public const VALUES = ['hello'];
                /** @return value-of<self::VALUES> */
                public function greet(string $name): string { return $name; }
            }
            trait NestedMethods { use SourceMethods; }
            class MethodHost {
                use NestedMethods { greet as welcome; }
                public const HOST_VALUES = ['host'];
                /** @return value-of<self::HOST_VALUES> */
                public function own(): string { return 'host'; }
            }
        "#,
    );
    let back = round_trip(&registry);

    for method in ["greet", "welcome"] {
        let original_slot = registry.member_slot("MethodHost", method).unwrap();
        let original_ref =
            crate::registry::MethodRef::new(registry.class_ids["MethodHost"], original_slot);
        assert!(registry.class_method_results.contains_key(&original_ref));
        assert!(
            registry
                .class_by_name("MethodHost")
                .unwrap()
                .methods
                .get(method)
                .is_none()
        );
        assert!(registry.lookup_own_method("MethodHost", method).is_some());
        let signature = back.lookup_own_method("MethodHost", method).unwrap();
        assert_eq!(back.member_slot("MethodHost", method), Some(original_slot));
        assert!(
            back.class_method_results
                .contains_key(&crate::registry::MethodRef::new(
                    back.class_ids["MethodHost"],
                    original_slot,
                ))
        );
        assert_eq!(
            back.member_entry_by_ref(crate::registry::MethodRef::new(
                back.class_ids["MethodHost"],
                original_slot,
            ))
            .map(|entry| entry.name.as_ref()),
            Some(method)
        );
        assert_eq!(signature.params()[0].name().as_ref(), "name");
        assert_eq!(
            signature.return_type(),
            &Type::string_literal("hello".into())
        );
    }
    let raw_own = registry
        .class_by_name("MethodHost")
        .unwrap()
        .methods
        .get("own")
        .unwrap();
    assert!(raw_own.return_type().contains_deferred());
    assert_eq!(
        registry
            .lookup_own_method("MethodHost", "own")
            .unwrap()
            .return_type(),
        &Type::string_literal("host".into())
    );
}

#[test]
fn mixin_and_phpdoc_method_topology_survives_freeze_and_cache_round_trip() {
    let registry = build_registry(
        r#"<?php
            class MixinSource { public function fromMixin(): string { return 'm'; } }
            /**
             * @mixin MixinSource
             * @method bool fromTag()
             */
            class MethodHost {}
        "#,
    );
    let back = round_trip(&registry);
    let original_names: Vec<_> = registry
        .member_entries("MethodHost")
        .map(|entry| entry.name.to_string())
        .collect();
    assert_eq!(original_names, ["fromTag", "fromMixin"]);
    assert!(
        registry
            .class_by_name("MethodHost")
            .unwrap()
            .methods
            .is_empty()
    );
    assert!(back.class_by_name("MethodHost").unwrap().methods.is_empty());
    for name in &original_names {
        assert_eq!(
            registry.member_slot("MethodHost", name),
            back.member_slot("MethodHost", name)
        );
        assert!(
            back.lookup_own_method("MethodHost", name).is_some(),
            "{name}"
        );
    }
    assert_eq!(
        back.member_entries("MethodHost")
            .map(|entry| entry.name.to_string())
            .collect::<Vec<_>>(),
        original_names
    );
}

#[test]
fn trait_method_effects_use_host_method_results_and_survive_cache_round_trip() {
    let registry = build_registry(
        r#"<?php
            trait WritesOwnProperty {
                public function writeValue() { $this->value = 1; }
            }
            class EffectHost { use WritesOwnProperty; }
        "#,
    );
    let declaration =
        registry.class_by_name("WritesOwnProperty").unwrap().methods["writeValue"].clone();
    assert!(
        registry
            .class_by_name("EffectHost")
            .unwrap()
            .methods
            .get("writeValue")
            .is_none()
    );
    assert!(
        registry
            .lookup_own_method("EffectHost", "writeValue")
            .is_some()
    );
    let expected = Storage::Own(Some(Name::from("value")));
    let effect = registry.method_effect_by_name("writeValue");
    assert!(
        effect
            .storages()
            .is_some_and(|storages| storages.contains(&expected))
    );
    assert_eq!(
        registry.class_by_name("WritesOwnProperty").unwrap().methods["writeValue"],
        declaration,
        "effect settlement leaves the trait declaration unchanged"
    );

    let back = round_trip(&registry);
    let effect_after_cache = back.method_effect_by_name("writeValue");
    assert!(
        effect_after_cache
            .storages()
            .is_some_and(|storages| storages.contains(&expected))
    );
    assert_eq!(
        back.class_by_name("WritesOwnProperty").unwrap().methods["writeValue"],
        declaration
    );
}

#[test]
fn stub_effect_memos_live_on_the_registry_and_use_function_ids() {
    let registry = Project::built_stubs(false);
    let (id, _, signature) = registry.function("array_walk").expect("stub function");
    assert!(signature.is_stub);
    assert_eq!(
        registry.stub_effect_results.get(&EffectId::Function(id)),
        Some(&true),
        "array_walk runs the callable argument and the memo belongs to its function ID"
    );
    let date_time = *registry.class_ids.get("DateTime").expect("stub class");
    let format_slot = registry
        .member_slot("DateTime", "format")
        .expect("stub method");
    assert_eq!(
        registry
            .stub_effect_results
            .get(&EffectId::Method(MethodRef::new(date_time, format_slot))),
        Some(&false),
        "DateTime::format has a method-ID entry and runs no passed code"
    );

    let back = round_trip(&registry);
    assert!(
        back.stub_effect_results.is_empty(),
        "the runtime memo is rebuilt for the loaded registry rather than serialized"
    );
}

#[test]
fn the_declarations_as_built_survive_a_round_trip() {
    let project = Project::new();
    let registry = project.registry();
    assert!(
        registry.has_resolved_declarations(),
        "the stub registry keeps its declarations as built"
    );
    let back = read_back();
    let classes = registry.classes_for_cache();
    let functions = registry.functions_for_cache();
    assert_eq!(back.classes.len(), classes.len());
    assert_eq!(back.functions.len(), functions.len());
    for (name, class) in classes.iter() {
        assert_eq!(back.classes.get(name), Some(class), "class {name}");
    }
    for (name, function) in functions.iter() {
        assert_eq!(back.functions.get(name), Some(function), "function {name}");
    }
}

#[test]
fn every_stored_table_reads_back_with_its_names_and_values() {
    let project = Project::new();
    let registry = project.registry();
    let back = read_back();

    macro_rules! same_table {
        ($($field:ident),*) => {$(
            assert_eq!(
                back.$field.len(),
                registry.$field.len(),
                concat!(stringify!($field), " has as many entries")
            );
            for (name, value) in registry.$field.iter() {
                let (declared, read) = back
                    .$field
                    .get_with_name(name)
                    .unwrap_or_else(|| panic!(concat!(stringify!($field), " lacks {}"), name));
                assert_eq!(declared.as_str(), name.as_str());
                assert_eq!(read, value, concat!(stringify!($field), " {}"), name);
            }
        )*};
    }
    same_table!(
        built_in_stub_classes,
        subtype_children,
        trait_users,
        recorded_parents
    );
}

#[test]
fn a_built_stub_registry_reads_back_as_it_was_stored() {
    // The declarations are compared one by one above; printed, they would
    // decode every stored member.
    let printed = |registry: &Registry| {
        let mut registry = registry.clone();
        registry.classes = Default::default();
        registry.functions = Default::default();
        syntax::canonical::print(&registry)
    };
    // Built here, since `Project::new` may itself have read its base back.
    for with_phpstan in [false, true] {
        let mut built = Project::built_stubs(with_phpstan);
        let back = round_trip(&built);
        built.freeze();
        let ancestry_names = |registry: &Registry| {
            registry
                .iter_classes()
                .filter(|(name, _)| registry.class_has_phpstan_ancestry(name))
                .map(|(name, _)| name.lower().to_string())
                .collect::<rustc_hash::FxHashSet<_>>()
        };
        assert_eq!(
            ancestry_names(&back),
            ancestry_names(&built),
            "the ID-addressed PHPStan-ancestry verdicts rebuild after a cache read"
        );
        for (name, _) in built.iter_classes() {
            assert_eq!(
                back.ancestor_names_of(name),
                built.ancestor_names_of(name),
                "the ID-addressed ancestor closure for {name} survives a cache read"
            );
            for ancestor in back.ancestor_names_of(name).into_iter().flatten() {
                assert!(
                    back.is_subclass_of(name, ancestor.as_str()),
                    "the resolved ID closure for {name} still reaches {ancestor} after a cache read"
                );
            }
            assert_eq!(
                back.method_owners(name),
                built.method_owners(name),
                "the ID-addressed method owners for {name} survive a cache read"
            );
            assert_eq!(
                back.property_owners(name),
                built.property_owners(name),
                "the ID-addressed property owners for {name} survive a cache read"
            );
            assert_eq!(
                back.const_owners(name),
                built.const_owners(name),
                "the ID-addressed constant owners for {name} survive a cache read"
            );
        }
        let (stored, back) = (printed(&built), printed(&back));
        if let Some(line) = stored
            .lines()
            .zip(back.lines())
            .position(|(a, b)| a != b)
            .or((stored.lines().count() != back.lines().count())
                .then(|| stored.lines().count().min(back.lines().count())))
        {
            let from = |text: &str| {
                text.lines()
                    .skip(line)
                    .take(8)
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            panic!(
                "with PHPStan's source {with_phpstan}, the print read back differs from line {}:\n{}\n---\n{}",
                line + 1,
                from(&stored),
                from(&back),
            );
        }
    }
}

#[test]
fn a_read_back_registry_iterates_its_names_in_lowercased_order() {
    let back = read_back();
    let keys: Vec<String> = back
        .classes
        .keys()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn the_stored_maps_and_sets_read_back_whole() {
    let project = Project::new();
    let registry = project.registry();
    let back = read_back();

    assert!(!registry.global_constants().is_empty());
    assert_eq!(
        back.global_constants.len(),
        registry.global_constants().len()
    );
    for (name, ty) in registry.global_constants().iter() {
        assert_eq!(back.global_constants.get(name), Some(ty), "constant {name}");
    }
    assert_eq!(
        back.function_decl_sites.len(),
        registry.function_decl_sites.len()
    );
    for (name, site) in registry.function_decl_sites.iter() {
        assert_eq!(back.function_decl_sites.get(name), Some(site), "{name}");
    }
    assert_eq!(
        back.phpstan_class_files.len(),
        registry.phpstan_class_files.len()
    );

    assert!(!registry.unconditionally_declared_classes.is_empty());
    assert!(!registry.unconditionally_declared_functions.is_empty());

    macro_rules! same_set {
        ($($field:ident),*) => {$(
            assert_eq!(back.$field.len(), registry.$field.len(), stringify!($field));
            for name in registry.$field.iter() {
                assert!(back.$field.contains(name), concat!(stringify!($field), " {}"), name);
            }
        )*};
    }
    same_set!(
        unconditionally_declared_classes,
        unconditionally_declared_functions,
        stub_provided_functions,
        names_with_assert_tags
    );
    assert_eq!(
        back.builtin_methods_running_code.len(),
        registry.builtin_methods_running_code.len()
    );
    for (name, owners) in registry.builtin_methods_running_code.iter() {
        assert_eq!(
            back.builtin_methods_running_code.get(name),
            Some(owners),
            "{name}"
        );
    }
    // A loaded registry reads its classes lazily: what is made from them
    // comes stored, not from the classes it holds itself.
    assert!(
        registry
            .builtin_methods_running_code
            .contains_key("getmessage")
    );
}

#[test]
fn a_conditional_type_survives_the_codec() {
    use crate::registry::stub_cache::{decode, encode};
    // Declared after the one variant that is not stored: the codec writes
    // the variant's number.
    let project = Project::new();
    let ty = &project
        .registry()
        .function_by_name("iterator_to_array")
        .unwrap()
        .return_type;
    assert!(ty.to_string().contains(" is true ? "), "got {ty}");
    let back: types::Type = decode(&encode(ty).unwrap()).unwrap();
    assert_eq!(&back, ty);
}

#[test]
fn a_stored_classs_methods_are_decoded_when_read() {
    use crate::registry::Members;
    use crate::registry::function_sig::FunctionReflection;
    use crate::registry::stub_cache::{decode, encode};

    let project = Project::new();
    let (_, exception) = project
        .registry()
        .lookup_class("Exception")
        .expect("a stub class");
    let count = exception.methods().len();
    assert!(count > 3);

    let back: Members<FunctionReflection> =
        decode(&encode(exception.methods()).expect("encodes")).expect("decodes");
    assert_eq!(back.len(), count);
    assert_eq!(back.encoded_count(), count, "nothing is read yet");

    let message = back.get("getmessage").expect("a method");
    assert_eq!(Some(message), exception.methods().get("getMessage"));
    assert_eq!(back.encoded_count(), count - 1, "one method was read");

    // Storing what was not read again keeps it as it was.
    let again: Members<FunctionReflection> =
        decode(&encode(&back).expect("encodes")).expect("decodes");
    assert_eq!(again.encoded_count(), count);
    assert_eq!(again, *exception.methods());
}
