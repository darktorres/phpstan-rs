//! What the stub cache stores must read back: the resolved registry, with
//! every class and function entry decoding.

use crate::project::Project;
use crate::registry::Registry;

#[test]
fn the_resolved_stub_registry_survives_a_round_trip() {
    let project = Project::new();
    let registry = project.registry();
    let mut stored = Vec::new();
    ciborium::ser::into_writer(&registry.to_stored().expect("diffs"), &mut stored).expect("stores");
    let back = Registry::from_stored(
        ciborium::de::from_reader_with_recursion_limit(
            &stored[..],
            crate::registry::stub_cache::RECURSION_LIMIT,
        )
        .expect("reads back"),
    )
    .expect("rebuilds");

    assert_eq!(back.classes.len(), registry.classes.len());
    assert_eq!(back.functions.len(), registry.functions.len());
    for (name, class) in registry.classes.iter() {
        assert_eq!(back.classes.get(name), Some(class), "class {name}");
    }
    for (name, function) in registry.functions.iter() {
        assert_eq!(back.functions.get(name), Some(function), "function {name}");
    }
}
