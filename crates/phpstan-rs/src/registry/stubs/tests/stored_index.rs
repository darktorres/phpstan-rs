//! The stub index the cache stores reads back whole, from the file's own
//! buffer.

use super::super::{Packed, Reader, Records, StubIndex, Writer, stub_index};

fn read_back() -> StubIndex {
    let mut writer = Writer::new();
    stub_index().store(&mut writer).expect("stores");
    let reader = match Reader::new(writer.finish()) {
        Ok(reader) => reader,
        Err(_) => panic!("a stored index is a whole file"),
    };
    let index = StubIndex::load(&reader).expect("reads back");
    for (section, packed) in [
        ("dependents", &index.dependents),
        ("function_extensions", &index.function_extensions),
        ("class_extensions", &index.class_extensions),
        ("class_names", &index.class_names),
        ("function_names", &index.function_names),
        ("constant_names", &index.constant_names),
    ] {
        packed.check_order(section).expect("its keys are in order");
    }
    for (section, records) in [
        ("declared_by_file", &index.declared_by_file),
        ("phpstan_functions", &index.phpstan_functions),
        ("phpstan_classes", &index.phpstan_classes),
        ("phpstan_constants", &index.phpstan_constants),
    ] {
        records
            .check_order(section)
            .expect("its offsets are in order");
    }
    index
}

fn same_records(name: &str, a: &Records, b: &Records) {
    assert_eq!(a.len(), b.len(), "{name}");
    assert!(
        a.len() > 0 || name.starts_with("phpstan"),
        "{name} is empty"
    );
    for position in 0..a.len() {
        assert_eq!(a.get(position), b.get(position), "{name} {position}");
    }
}

#[test]
fn the_stub_index_survives_a_round_trip() {
    let before = stub_index();
    let after = read_back();
    same_records(
        "declared_by_file",
        &before.declared_by_file,
        &after.declared_by_file,
    );
    same_records(
        "phpstan_functions",
        &before.phpstan_functions,
        &after.phpstan_functions,
    );
    same_records(
        "phpstan_classes",
        &before.phpstan_classes,
        &after.phpstan_classes,
    );
    same_records(
        "phpstan_constants",
        &before.phpstan_constants,
        &after.phpstan_constants,
    );
    assert_eq!(before.extension_names, after.extension_names);
    assert_eq!(before.extensions, after.extensions);
}

#[test]
fn the_stored_name_maps_answer_as_the_built_ones_do() {
    let before = stub_index();
    let after = read_back();
    let pairs: [(&str, &Packed, &Packed); 6] = [
        ("dependents", &before.dependents, &after.dependents),
        (
            "function_extensions",
            &before.function_extensions,
            &after.function_extensions,
        ),
        (
            "class_extensions",
            &before.class_extensions,
            &after.class_extensions,
        ),
        ("class_names", &before.class_names, &after.class_names),
        (
            "function_names",
            &before.function_names,
            &after.function_names,
        ),
        (
            "constant_names",
            &before.constant_names,
            &after.constant_names,
        ),
    ];
    for (name, built, stored) in pairs {
        // Every key of the built map, found by its own bytes, then keys
        // that are not there.
        for position in 0..built.records_len() {
            let key = built.key_text(position);
            assert_eq!(built.get(key), stored.get(key), "{name} {key}");
            assert!(stored.contains(key), "{name} {key}");
        }
        assert!(!stored.contains("no such name\u{0}"), "{name}");
    }
    assert_eq!(
        before.extension(&before.function_extensions, "strlen"),
        after.extension(&after.function_extensions, "strlen")
    );
}
