//! A `NameTable` finds what a map by the lowercased text finds, by text in
//! any case and by a `Name`, whether the entries are its own, in a base
//! built in memory or in a stored base read in place, hidden or replaced.

use std::collections::BTreeMap;

use syntax::name::Interner;

use crate::registry::NameTable;
use crate::registry::container::{Reader, Writer};

fn texts(count: usize, seed: u64) -> Vec<String> {
    let mut state = seed;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let alphabet: Vec<char> = "aAbBzZ_\\0\u{e9}\u{c9}".chars().collect();
    (0..count)
        .map(|_| {
            let len = (next() % 14) as usize + 1;
            (0..len)
                .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
                .collect()
        })
        .collect()
}

type Model = BTreeMap<String, (String, u32)>;

fn agree(table: &NameTable<u32>, model: &Model, probes: &[String], what: &str) {
    for text in probes {
        let want = model.get(&text.to_ascii_lowercase());
        let by_text = table.get_with_name(text.as_str());
        let by_name = table.get_with_name(&Interner::global().name(text));
        let show = |found: Option<(&syntax::name::Name, &u32)>| {
            found.map(|(name, value)| (name.to_string(), *value))
        };
        assert_eq!(show(by_text), want.cloned(), "{what}: {text} by text");
        assert_eq!(show(by_name), want.cloned(), "{what}: {text} by name");
        assert_eq!(
            table.get(text.as_str()),
            want.map(|(_, v)| v),
            "{what}: {text} get"
        );
        assert_eq!(
            table.contains_key(text.as_str()),
            want.is_some(),
            "{what}: {text} contains"
        );
    }
    assert_eq!(table.len(), model.len(), "{what}: length");
}

#[test]
fn a_table_finds_a_name_in_any_case_over_every_kind_of_base() {
    let all = texts(1500, 0x9e37_79b9);
    let (first, second) = all.split_at(800);
    let mut table: NameTable<u32> = NameTable::default();
    let mut model: Model = BTreeMap::new();
    for (i, text) in first.iter().enumerate() {
        table.insert(Interner::global().name(text), i as u32);
        model.insert(text.to_ascii_lowercase(), (text.clone(), i as u32));
    }
    agree(&table, &model, &all, "own entries");

    // A base built in memory, then changes over it: new names, replaced
    // ones, removed ones.
    let mutate = |table: &mut NameTable<u32>, model: &mut Model, offset: u32| {
        for (i, text) in second.iter().enumerate() {
            let value = offset + i as u32;
            match i % 4 {
                0 | 1 => {
                    table.insert(Interner::global().name(text), value);
                    model.insert(text.to_ascii_lowercase(), (text.clone(), value));
                }
                2 => {
                    table.remove(text.as_str());
                    model.remove(&text.to_ascii_lowercase());
                }
                _ => {
                    let old = table.take(text.as_str());
                    assert_eq!(
                        old,
                        model.remove(&text.to_ascii_lowercase()).map(|(_, v)| v)
                    );
                }
            }
        }
    };
    table.freeze();
    agree(&table, &model, &all, "frozen");
    mutate(&mut table, &mut model, 10_000);
    agree(&table, &model, &all, "frozen, then changed");

    // A stored base read in place, then the same changes over it.
    let mut stored: NameTable<u32> = NameTable::default();
    let mut stored_model: Model = BTreeMap::new();
    for (i, text) in first.iter().enumerate() {
        stored.insert(Interner::global().name(text), i as u32);
        stored_model.insert(text.to_ascii_lowercase(), (text.clone(), i as u32));
    }
    let mut writer = Writer::new();
    stored.store(&mut writer, "names").expect("stores");
    let Ok(reader) = Reader::new(writer.finish()) else {
        panic!("a stored table is a whole file");
    };
    let mut loaded: NameTable<u32> = NameTable::load(&reader, "names").expect("reads back");
    agree(&loaded, &stored_model, &all, "stored");
    mutate(&mut loaded, &mut stored_model, 20_000);
    agree(&loaded, &stored_model, &all, "stored, then changed");
}
