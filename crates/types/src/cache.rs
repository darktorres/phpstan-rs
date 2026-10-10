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
use syntax::cache_blob::{BlobWriter, parse_section};
use syntax::name::{CacheNameEncoder, CacheNameTable};

use crate::{Repr, Type};

/// The types a writer has met, in the order their forms were written: a
/// type's members come before it.
#[derive(Default)]
struct Encoder {
    indexes: FxHashMap<usize, u32>,
    /// Keeps each type met alive, so the address it is filed under stays its.
    kept: Vec<Type>,
    forms: BlobWriter,
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
    let index = table.forms.push(&form)?;
    table.kept.push(ty.clone());
    table.indexes.insert(address, index);
    Ok(index)
}

/// The table section of the file `names` belongs to: the count, the end of
/// each form (little-endian `u32`s), then the forms.
pub fn section(names: &CacheNameEncoder) -> Vec<u8> {
    match names.extension_if::<SharedEncoder>() {
        Some(table) => table.borrow().forms.section(),
        None => BlobWriter::default().section(),
    }
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
    let forms = parse_section(buffer, range)?;
    let types = (0..forms.len()).map(|_| OnceLock::new()).collect();
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
