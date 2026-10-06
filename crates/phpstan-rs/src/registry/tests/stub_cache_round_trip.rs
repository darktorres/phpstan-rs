//! What the stub cache stores must read back: the resolved registry and
//! the declarations as built, with every class and function entry decoding.

use super::helpers::build_registry;
use crate::project::Project;
use crate::registry::Registry;
use crate::registry::container::{Reader, Writer};
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

    assert_eq!(back.classes.len(), registry.classes().len());
    assert_eq!(back.functions.len(), registry.functions().len());
    for (name, class) in registry.classes().iter() {
        assert_eq!(back.classes.get(name), Some(class), "class {name}");
    }
    for (name, function) in registry.functions().iter() {
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
            "#,
    );
    let declared = registry.functions["reads_answer"].clone();
    let effective = registry.lookup_function("reads_answer").unwrap();
    assert_eq!(effective.params[0].ty, Type::int_literal(42));
    assert_eq!(registry.functions["reads_answer"], declared);

    let back = round_trip(&registry);
    assert_eq!(
        back.lookup_function("reads_answer").unwrap().params[0].ty,
        Type::int_literal(42)
    );
    assert_eq!(back.functions["reads_answer"], declared);
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
        assert!(
            registry
                .classes
                .get("MethodHost")
                .unwrap()
                .methods
                .get(method)
                .is_none()
        );
        assert!(registry.lookup_own_method("MethodHost", method).is_some());
        let signature = back.lookup_own_method("MethodHost", method).unwrap();
        assert_eq!(signature.params()[0].name().as_ref(), "name");
        assert_eq!(
            signature.return_type(),
            &Type::string_literal("hello".into())
        );
    }
    let raw_own = registry
        .classes
        .get("MethodHost")
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
fn the_declarations_as_built_survive_a_round_trip() {
    let project = Project::new();
    let registry = project.registry();
    assert!(
        registry.has_resolved_declarations(),
        "the stub registry keeps its declarations as built"
    );
    let back = read_back();
    let (before, after) = (
        registry.raw.as_ref().expect("kept"),
        back.raw.as_ref().expect("stored with the registry"),
    );
    assert_eq!(after.classes.len(), before.classes.len());
    assert_eq!(after.functions.len(), before.functions.len());
    for (name, class) in before.classes.iter() {
        assert_eq!(after.classes.get(name), Some(class), "class {name}");
    }
    for (name, function) in before.functions.iter() {
        assert_eq!(after.functions.get(name), Some(function), "function {name}");
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
        registry.raw = None;
        syntax::canonical::print(&registry)
    };
    // Built here, since `Project::new` may itself have read its base back.
    for with_phpstan in [false, true] {
        let mut built = Project::built_stubs(with_phpstan);
        let back = round_trip(&built);
        built.freeze();
        let ancestry_names = |registry: &Registry| {
            registry
                .classes()
                .iter()
                .filter(|(name, _)| registry.class_has_phpstan_ancestry(name))
                .map(|(name, _)| name.lower().to_string())
                .collect::<rustc_hash::FxHashSet<_>>()
        };
        assert_eq!(
            ancestry_names(&back),
            ancestry_names(&built),
            "the ID-addressed PHPStan-ancestry verdicts rebuild after a cache read"
        );
        for (name, _) in built.classes().iter() {
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
        names_with_assert_tags,
        builtin_methods_running_code
    );
    // A loaded registry reads its classes lazily: what is made from them
    // comes stored, not from the classes it holds itself.
    assert!(registry.builtin_methods_running_code.contains("getmessage"));
}

#[test]
fn a_conditional_type_survives_the_codec() {
    use crate::registry::stub_cache::{decode, encode};
    // Declared after the one variant that is not stored: the codec writes
    // the variant's number.
    let project = Project::new();
    let ty = &project.registry().functions()["iterator_to_array"].return_type;
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
