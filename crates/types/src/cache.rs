//! A cache file's table of types. Inside a cache codec a `Type` is a `u32`
//! index into the file's table, and the table holds each distinct type's
//! form once, so a type met a thousand times in a file is decoded once. The
//! table lives beside the file's name table (`syntax::name`), which every
//! codec scope already carries.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use rustc_hash::FxHashMap;
use syntax::name::{CacheNameEncoder, CacheNameTable};

use crate::{Repr, Type};

/// The types a writer has met, in the order their forms were written: a
/// type's members come before it.
#[derive(Default)]
struct Encoder {
    indexes: FxHashMap<usize, u32>,
    /// Keeps each type met alive, so the address it is filed under stays its.
    kept: Vec<Type>,
    ends: Vec<u32>,
    blob: Vec<u8>,
}

type SharedEncoder = RefCell<Encoder>;

fn encoder() -> Option<Rc<SharedEncoder>> {
    let names = syntax::name::current_cache_name_encoder()?;
    Some(names.extension(SharedEncoder::default))
}

/// The index of `ty` in the table of the file being written.
pub(crate) fn index_of(ty: &Type) -> Option<Result<u32, String>> {
    let table = encoder()?;
    Some(index_in(&table, ty))
}

fn index_in(table: &SharedEncoder, ty: &Type) -> Result<u32, String> {
    let address = ty.node_address();
    if let Some(index) = table.borrow().indexes.get(&address) {
        return Ok(*index);
    }
    // The members register while the form is written, so they come first.
    let form = syntax::stack::guard(|| postcard::to_allocvec(ty.repr()))
        .map_err(|error| error.to_string())?;
    let mut table = table.borrow_mut();
    let index = u32::try_from(table.ends.len())
        .map_err(|_| "a cache file has too many distinct types".to_string())?;
    table.blob.extend_from_slice(&form);
    let end = u32::try_from(table.blob.len())
        .map_err(|_| "a cache file's types outgrew its address space".to_string())?;
    table.ends.push(end);
    table.kept.push(ty.clone());
    table.indexes.insert(address, index);
    Ok(index)
}

/// The table section of the file `names` belongs to: the count, the end of
/// each form (little-endian `u32`s), then the forms.
pub fn section(names: &CacheNameEncoder) -> Vec<u8> {
    let table = names.extension_if::<SharedEncoder>();
    let table = table.as_ref().map(|table| table.borrow());
    let (ends, blob): (&[u32], &[u8]) = match &table {
        Some(table) => (&table.ends, &table.blob),
        None => (&[], &[]),
    };
    let mut out = Vec::with_capacity(4 + 4 * ends.len() + blob.len());
    out.extend_from_slice(&(ends.len() as u32).to_le_bytes());
    for end in ends {
        out.extend_from_slice(&end.to_le_bytes());
    }
    out.extend_from_slice(blob);
    out
}

/// A file's types as read: each form's place in the file's buffer and the
/// type it decoded to, once it has.
pub(crate) struct Table {
    buffer: Arc<Vec<u8>>,
    forms: Vec<Range<usize>>,
    types: Vec<OnceLock<Type>>,
}

impl std::fmt::Debug for Table {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Table({} types)", self.forms.len())
    }
}

/// Keeps the types table found at `range` of `buffer` beside `names`.
pub fn attach(
    names: &mut CacheNameTable,
    buffer: &Arc<Vec<u8>>,
    range: Range<usize>,
) -> Result<(), String> {
    let section = buffer
        .get(range.clone())
        .ok_or("its type table lies outside the file")?;
    let count = section
        .get(..4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")) as usize)
        .ok_or("its type table has no count")?;
    let blob_start = count
        .checked_mul(4)
        .and_then(|ends| ends.checked_add(4))
        .filter(|start| *start <= section.len())
        .ok_or("its type table is shorter than its count")?;
    let blob_len = section.len() - blob_start;
    let mut forms = Vec::with_capacity(count);
    let mut start = 0usize;
    for at in 0..count {
        let bytes = &section[4 + 4 * at..8 + 4 * at];
        let end = u32::from_le_bytes(bytes.try_into().expect("four bytes")) as usize;
        if end < start || end > blob_len {
            return Err("its type table has a form out of order".to_string());
        }
        forms.push(range.start + blob_start + start..range.start + blob_start + end);
        start = end;
    }
    let types = (0..count).map(|_| OnceLock::new()).collect();
    names.set_extension(Arc::new(Table {
        buffer: Arc::clone(buffer),
        forms,
        types,
    }));
    Ok(())
}

/// The type at `index` of the table beside the name table being decoded
/// with, when there is one.
pub(crate) fn type_at<E: serde::de::Error>(index: u32) -> Option<Result<Type, E>> {
    let table = syntax::name::current_cache_extension::<Table>()?;
    Some(table.get(index))
}

impl Table {
    fn get<E: serde::de::Error>(&self, index: u32) -> Result<Type, E> {
        let cell = self
            .types
            .get(index as usize)
            .ok_or_else(|| E::custom(format!("cache type index {index} is out of bounds")))?;
        if let Some(ty) = cell.get() {
            return Ok(ty.clone());
        }
        let form = &self.buffer[self.forms[index as usize].clone()];
        let repr = syntax::stack::guard(|| postcard::from_bytes::<Repr>(form))
            .map_err(|error| E::custom(format!("cache type {index} does not decode: {error}")))?;
        Ok(cell.get_or_init(|| Type::new(repr)).clone())
    }
}
