//! The records' layouts clang computes for a unit (`-fdump-record-layouts-
//! complete`), which it writes before the unit's dump: each record's size,
//! alignment and members' offsets, for `sizeof`, `_Alignof` and `offsetof`.

use std::io::BufRead;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::lower::{anonymous_record, clean_type, numbered};

#[derive(Debug, Clone)]
pub(crate) struct Layout {
    pub(crate) size: u64,
    pub(crate) align: u64,
    pub(crate) is_union: bool,
    /// The record's own members, an anonymous one named as the catalog
    /// names its record: by the place clang names it by, which the unit's
    /// index numbers where records share it.
    pub(crate) fields: Vec<LayoutField>,
}

#[derive(Debug, Clone)]
pub(crate) struct LayoutField {
    pub(crate) name: Arc<str>,
    pub(crate) offset: u64,
    /// The member's type, as the catalog writes types.
    pub(crate) ty: Arc<str>,
}

/// The layouts, by the catalog's name for each record.
pub(crate) type Layouts = FxHashMap<Arc<str>, Layout>;

/// Reads the layouts, leaving the reader at the dump that follows them.
pub(crate) fn read_layouts(reader: &mut impl BufRead, root: &str) -> std::io::Result<Layouts> {
    let mut layouts = Layouts::default();
    let mut line = String::new();
    let mut current: Option<(Arc<str>, Layout)> = None;
    // How many records of each name were laid out, to tell the records of
    // one name apart as the unit's index does (`numbered`).
    let mut counts: FxHashMap<Arc<str>, u32> = FxHashMap::default();
    loop {
        match reader.fill_buf()?.first() {
            None | Some(b'{') => return Ok(layouts),
            Some(_) => {}
        }
        line.clear();
        reader.read_line(&mut line)?;
        if line.starts_with("*** Dumping AST Record Layout") {
            current = None;
            continue;
        }
        let Some((left, right)) = line.trim_end_matches('\n').split_once('|') else {
            continue;
        };
        let right = right.strip_prefix(' ').unwrap_or(right);
        if let Some(sizes) = right.trim().strip_prefix("[sizeof=") {
            if let Some((name, mut layout)) = current.take() {
                let mut parts = sizes.trim_end_matches(']').split(", ");
                layout.size = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                for part in parts {
                    if let Some(align) = part.strip_prefix("align=") {
                        layout.align = align.parse().unwrap_or(0);
                    }
                }
                let count = counts.entry(name.clone()).or_insert(0);
                *count += 1;
                layouts.insert(numbered(name, *count), layout);
            }
            continue;
        }
        let depth = right.len() - right.trim_start_matches(' ').len();
        let text = right.trim_start_matches(' ');
        match &mut current {
            None if depth == 0 => {
                let text = clean_type(root, text.trim_end());
                let name = record_name(&text);
                current = Some((
                    name,
                    Layout {
                        size: 0,
                        align: 0,
                        is_union: text.starts_with("union "),
                        fields: Vec::new(),
                    },
                ));
            }
            Some((_, layout)) if depth == 2 => {
                let offset = left.trim().split(':').next().unwrap_or("");
                let Ok(offset) = offset.parse() else {
                    continue;
                };
                let (ty, name) = split_member(text);
                let ty: Arc<str> = Arc::from(clean_type(root, ty));
                let name = if name.is_empty() {
                    match anonymous_record(&ty) {
                        Some(record) => record,
                        None => continue,
                    }
                } else {
                    Arc::from(name)
                };
                layout.fields.push(LayoutField { name, offset, ty });
            }
            _ => {}
        }
    }
}

/// A member's line split into its type and its name, which is empty for
/// an anonymous member or an unnamed bit-field.
fn split_member(text: &str) -> (&str, &str) {
    if text.ends_with(' ') || text.ends_with(')') {
        return (text.trim_end(), "");
    }
    match text.rsplit_once(' ') {
        Some((ty, name)) => (ty, name),
        None => (text, ""),
    }
}

/// The catalog's name for a record a layout's first line gives (`struct
/// _zend_string`, `union _zval_struct::(unnamed at …)`).
fn record_name(text: &str) -> Arc<str> {
    if let Some(name) = anonymous_record(text) {
        return name;
    }
    for keyword in ["struct ", "union ", "class "] {
        if let Some(tag) = text.strip_prefix(keyword) {
            return Arc::from(tag.trim());
        }
    }
    Arc::from(text.trim())
}
