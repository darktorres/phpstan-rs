//! A cache file's table of calls. Inside a cache codec a call an `Origin`
//! holds is a `u32` index into the file's table, which holds each distinct
//! call once, so a call met a thousand times in a file is decoded once and
//! shared. The table lives beside the file's name table, as the table of
//! types does.

use std::cell::RefCell;
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use rustc_hash::FxHashMap;
use serde::ser::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use syntax::cache_blob::{BlobWriter, parse_section};
use syntax::name::{CacheNameEncoder, CacheNameTable};

use super::summaries::Callee;

/// The calls a writer has met, in the order their forms were written: a
/// call's inner calls come before it.
#[derive(Default)]
struct Encoder {
    by_content: FxHashMap<Arc<Callee>, u32>,
    by_address: FxHashMap<usize, u32>,
    /// Keeps each call met alive, so the address it is filed under stays its.
    kept: Vec<Arc<Callee>>,
    forms: BlobWriter,
}

type SharedEncoder = RefCell<Encoder>;

fn index_in(table: &SharedEncoder, callee: &Arc<Callee>) -> Result<u32, String> {
    let address = Arc::as_ptr(callee) as usize;
    let found = {
        let table = table.borrow();
        if let Some(index) = table.by_address.get(&address) {
            return Ok(*index);
        }
        table.by_content.get(callee).copied()
    };
    if let Some(index) = found {
        let mut table = table.borrow_mut();
        table.kept.push(Arc::clone(callee));
        table.by_address.insert(address, index);
        return Ok(index);
    }
    // The inner calls register while the form is written, so they come first.
    let form = syntax::stack::guard(|| postcard::to_allocvec(callee.as_ref()))
        .map_err(|error| error.to_string())?;
    let mut table = table.borrow_mut();
    let index = table.forms.push(&form)?;
    table.kept.push(Arc::clone(callee));
    table.by_address.insert(address, index);
    table.by_content.insert(Arc::clone(callee), index);
    Ok(index)
}

/// The table section of the file `names` belongs to.
pub(super) fn section(names: &CacheNameEncoder) -> Vec<u8> {
    match names.extension_if::<SharedEncoder>() {
        Some(table) => table.borrow().forms.section(),
        None => BlobWriter::default().section(),
    }
}

/// A file's calls as read: each form's place in the file's buffer and the
/// call it decoded to, once it has.
struct Table {
    buffer: Arc<Vec<u8>>,
    forms: Vec<Range<usize>>,
    calls: Vec<OnceLock<Arc<Callee>>>,
}

impl std::fmt::Debug for Table {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Table({} calls)", self.forms.len())
    }
}

/// Keeps the calls table found at `range` of `buffer` beside `names`.
pub(super) fn attach(
    names: &mut CacheNameTable,
    buffer: &Arc<Vec<u8>>,
    range: Range<usize>,
) -> Result<(), String> {
    let forms = parse_section(buffer, range)?;
    let calls = (0..forms.len()).map(|_| OnceLock::new()).collect();
    names.set_extension(Arc::new(Table {
        buffer: Arc::clone(buffer),
        forms,
        calls,
    }));
    Ok(())
}

impl Table {
    fn get<E: serde::de::Error>(&self, index: u32) -> Result<Arc<Callee>, E> {
        let cell = self
            .calls
            .get(index as usize)
            .ok_or_else(|| E::custom(format!("cache call index {index} is out of bounds")))?;
        if let Some(callee) = cell.get() {
            return Ok(Arc::clone(callee));
        }
        let form = &self.buffer[self.forms[index as usize].clone()];
        let callee = syntax::stack::guard(|| postcard::from_bytes::<Callee>(form))
            .map_err(|error| E::custom(format!("cache call {index} does not decode: {error}")))?;
        Ok(Arc::clone(cell.get_or_init(|| Arc::new(callee))))
    }
}

/// `serialize_with` for an `Arc<Callee>`: the index in the file's table
/// inside a cache codec, the call itself outside one.
pub(super) fn serialize<S: Serializer>(
    callee: &Arc<Callee>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match syntax::name::current_cache_name_encoder() {
        Some(names) => {
            let table = names.extension(SharedEncoder::default);
            let index = index_in(&table, callee).map_err(S::Error::custom)?;
            serializer.serialize_u32(index)
        }
        None => callee.as_ref().serialize(serializer),
    }
}

/// `deserialize_with` for an `Arc<Callee>`.
pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<Callee>, D::Error> {
    match syntax::name::current_cache_extension::<Table>() {
        Some(table) => table.get(u32::deserialize(deserializer)?),
        None => Callee::deserialize(deserializer).map(Arc::new),
    }
}
