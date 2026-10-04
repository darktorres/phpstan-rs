//! A table printed over its base: what it adds, changes and drops, found
//! from what the two hold.

use syntax::canonical::Printer;

use crate::registry::canonical::{CanonicalOver, CompareAt};
use crate::registry::name_map::NameSet;
use crate::registry::name_table::NameTable;

/// `table` printed over `base`, which comparing every name prints the same.
fn over<T: CanonicalOver>(table: &T, base: &T) -> String {
    let print = |compare| {
        let mut p = Printer::default();
        table.canonical_over(base, compare, &mut p);
        p.finish()
    };
    let layers = print(CompareAt::Layers);
    assert_eq!(layers, print(CompareAt::Whole), "both ways print the same");
    layers
}

#[test]
fn a_table_prints_the_entries_it_adds_changes_or_respells_and_the_names_it_drops() {
    let mut base: NameTable<u32> = NameTable::default();
    for (name, value) in [
        ("Kept", 1),
        ("Changed", 2),
        ("Dropped", 3),
        ("Respelled", 4),
    ] {
        base.insert(name, value);
    }
    base.freeze();
    let mut table = base.clone();
    table.insert("Added", 5);
    table.replace("Changed", 20);
    assert!(table.remove("Dropped"));
    table.take("Respelled");
    table.insert("RESPELLED", 4);
    assert_eq!(
        over(&table, &base),
        "Over {\n  changed: [\n    Added => 5\n    Changed => 20\n    RESPELLED => 4\n  ]\n  \
         dropped: [\n    Dropped\n  ]\n}"
    );
}

#[test]
fn a_table_that_reads_its_base_unchanged_prints_nothing_over_it() {
    let mut base: NameTable<u32> = NameTable::default();
    base.insert("One", 1);
    base.freeze();
    let table = base.clone();
    assert!(table.shares_base(&base));
    assert_eq!(
        over(&table, &base),
        "Over {\n  changed: []\n  dropped: []\n}"
    );
}

#[test]
fn a_set_prints_the_names_it_adds_and_drops() {
    let mut base = NameSet::default();
    base.insert("kept");
    base.insert("dropped");
    base.freeze();
    let mut set = base.clone();
    set.insert("added");
    set.remove("dropped");
    assert_eq!(
        over(&set, &base),
        "Over {\n  added: [\n    added\n  ]\n  dropped: [\n    dropped\n  ]\n}"
    );
}

#[test]
fn a_table_over_another_base_compares_every_name() {
    let mut base: NameTable<u32> = NameTable::default();
    base.insert("One", 1);
    base.freeze();
    let mut table: NameTable<u32> = NameTable::default();
    table.insert("One", 1);
    table.insert("Two", 2);
    table.freeze();
    assert!(!table.shares_base(&base));
    assert_eq!(
        over(&table, &base),
        "Over {\n  changed: [\n    Two => 2\n  ]\n  dropped: []\n}"
    );
}

#[test]
fn a_table_whose_base_has_entries_of_its_own_prints_over_both_layers() {
    let mut shared: NameTable<u32> = NameTable::default();
    for (name, value) in [("Kept", 1), ("Hidden", 2), ("Theirs", 3)] {
        shared.insert(name, value);
    }
    shared.freeze();
    let mut base = shared.clone();
    base.insert("Added", 4);
    assert!(base.remove("Theirs"));
    let mut table = shared.clone();
    assert!(table.remove("Hidden"));
    table.replace("Kept", 10);
    assert!(table.shares_base(&base));
    assert_eq!(
        over(&table, &base),
        "Over {\n  changed: [\n    Kept => 10\n    Theirs => 3\n  ]\n  \
         dropped: [\n    Added\n    Hidden\n  ]\n}"
    );
}
