//! A table printed over its base: what it adds, changes and drops, found
//! from what the two hold.

use syntax::canonical::Printer;

use crate::registry::canonical::CanonicalOver;
use crate::registry::name_map::NameSet;
use crate::registry::name_table::NameTable;

/// `table` printed over `base`.
fn over<T: CanonicalOver>(table: &T, base: &T) -> String {
    let mut p = Printer::default();
    table.canonical_over(base, &mut p);
    p.finish()
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
