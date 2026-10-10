//! Inside a cache codec a type is an index into the file's table of types:
//! the table holds each distinct form once, and reading gives back the node
//! that was written.
use super::super::*;
use std::sync::Arc;
use syntax::name::{CacheNameEncoder, CacheNameTable};

fn sample() -> Vec<Type> {
    let object = Type::object("Foo".into(), vec![Type::int(), Type::string()]);
    let list = Type::array(Type::int(), object.clone());
    vec![
        object.clone(),
        list.clone(),
        Type::union(list, Type::null()),
        Type::string_literal("a".into()),
        Type::int(),
        object,
    ]
}

/// Writes `values` in a codec scope and returns the bytes, the name
/// section's texts and the type section.
fn write(values: &[Type]) -> (Vec<u8>, Vec<String>, Vec<u8>) {
    let names = CacheNameEncoder::default();
    let bytes = syntax::name::with_cache_name_encoder(&names, || {
        postcard::to_allocvec(&values.to_vec()).unwrap()
    });
    (bytes, names.names(), cache::section(&names))
}

fn read(bytes: &[u8], texts: Vec<String>, section: &[u8]) -> Vec<Type> {
    let buffer = Arc::new(section.to_vec());
    let mut names = CacheNameTable::new(texts);
    cache::attach(&mut names, &buffer, 0..section.len()).unwrap();
    syntax::name::with_cache_name_table(&names, || {
        postcard::from_bytes::<Vec<Type>>(bytes).unwrap()
    })
}

#[test]
fn types_read_back_are_the_nodes_that_were_written() {
    let values = sample();
    let (bytes, texts, section) = write(&values);
    let back = read(&bytes, texts, &section);
    assert_eq!(back.len(), values.len());
    for (back, written) in back.iter().zip(&values) {
        assert!(Arc::ptr_eq(&back.0, &written.0));
    }
}

#[test]
fn a_type_met_twice_is_written_once_and_its_members_come_first() {
    let values = sample();
    let (_, _, section) = write(&values);
    let count = u32::from_le_bytes(section[..4].try_into().unwrap()) as usize;
    // int, string, Foo<int, string>, the array, null, the union and "a": the
    // repeated object adds nothing.
    assert_eq!(count, 7);
    let again = write(&[values[0].clone(), values[0].clone()]);
    let twice = u32::from_le_bytes(again.2[..4].try_into().unwrap()) as usize;
    assert_eq!(twice, 3);
}

#[test]
fn outside_a_codec_a_type_is_its_form() {
    let values = sample();
    let bytes = postcard::to_allocvec(&values).unwrap();
    let back: Vec<Type> = postcard::from_bytes(&bytes).unwrap();
    for (back, written) in back.iter().zip(&values) {
        assert!(Arc::ptr_eq(&back.0, &written.0));
    }
}

#[test]
fn a_damaged_table_is_refused() {
    let mut names = CacheNameTable::new(Vec::new());
    let buffer = Arc::new(vec![5, 0, 0, 0]);
    assert!(cache::attach(&mut names, &buffer, 0..4).is_err());
    let buffer = Arc::new(vec![1, 0, 0, 0, 9, 0, 0, 0, 1]);
    assert!(cache::attach(&mut names, &buffer, 0..9).is_err());
}
