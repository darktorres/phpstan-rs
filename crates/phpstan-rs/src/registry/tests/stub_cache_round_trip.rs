//! What the stub cache stores must read back: the resolved registry and
//! the declarations as built, with every class and function entry decoding.

use crate::project::Project;
use crate::registry::Registry;
use crate::registry::container::{Reader, Writer};

/// The stub registry of `Project::new` stored and read back, once for all
/// the tests here: storing encodes every entry and reading decodes the ones
/// a test asks for, which is most of what these tests cost.
fn read_back() -> &'static Registry {
    static BACK: std::sync::LazyLock<Registry> = std::sync::LazyLock::new(|| {
        let project = Project::new();
        let mut writer = Writer::new();
        project.registry().store(&mut writer).expect("stores");
        let reader = match Reader::new(writer.finish()) {
            Ok(reader) => reader,
            Err(_) => panic!("a stored registry is a whole file"),
        };
        let back = Registry::load(&reader).expect("reads back");
        back.check_stored().expect("every stored table is in order");
        back
    });
    &BACK
}

#[test]
fn the_resolved_stub_registry_survives_a_round_trip() {
    let project = Project::new();
    let registry = project.registry();
    let back = read_back();

    assert_eq!(back.classes.len(), registry.classes.len());
    assert_eq!(back.functions.len(), registry.functions.len());
    for (name, class) in registry.classes.iter() {
        assert_eq!(back.classes.get(name), Some(class), "class {name}");
    }
    for (name, function) in registry.functions.iter() {
        assert_eq!(back.functions.get(name), Some(function), "function {name}");
    }
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
        ancestor_names,
        subtype_children,
        trait_users,
        recorded_parents,
        method_owner_by_class,
        ancestor_method_owner_by_class,
        property_owner_by_class,
        ancestor_property_owner_by_class,
        const_owner_by_class
    );
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

    assert!(!registry.global_constants.is_empty());
    assert_eq!(back.global_constants.len(), registry.global_constants.len());
    for (name, ty) in registry.global_constants.iter() {
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
}

#[test]
fn a_conditional_type_survives_the_codec() {
    use crate::registry::stub_cache::{decode, encode};
    // Declared after the one variant that is not stored: the codec writes
    // the variant's number.
    let project = Project::new();
    let ty = &project.registry().functions["iterator_to_array"].return_type;
    assert!(ty.to_string().contains(" is true ? "), "got {ty}");
    let back: types::Type = decode(&encode(ty).unwrap()).unwrap();
    assert_eq!(&back, ty);
}
