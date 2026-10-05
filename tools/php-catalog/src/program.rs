//! The program the build links: every unit's functions, globals and
//! records, and the functions it calls without defining them.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::ir::{FnName, Function, Global, GlobalName, Ty};
use crate::layout::{Layout, Layouts};

#[derive(Default)]
pub(crate) struct Program {
    /// Each function's definitions: an external function that several of
    /// the build's binaries define (each SAPI its own `main`) has each of
    /// theirs, and a call by its name reaches any of them.
    pub(crate) functions: FxHashMap<FnName, Vec<Function>>,
    pub(crate) globals: FxHashMap<GlobalName, Vec<Global>>,
    /// Each record's layout, by the catalog's name for it.
    pub(crate) records: Layouts,
    /// The records whose name some units give different layouts, each
    /// layout told by its size and its first member that differs from the
    /// first layout's.
    pub(crate) conflicts: FxHashMap<Arc<str>, Vec<String>>,
    /// Each function declared and not defined, with the files declaring
    /// it.
    pub(crate) declared: FxHashMap<FnName, Vec<Arc<str>>>,
    /// Each typedef's type; a name some units give different types has
    /// the first.
    pub(crate) typedefs: FxHashMap<Box<str>, Ty>,
    /// The typedefs some units give different types, with each type.
    pub(crate) typedef_conflicts: FxHashMap<Box<str>, Vec<Ty>>,
}

/// What a unit gives the program.
#[derive(Default)]
pub(crate) struct UnitPart {
    pub(crate) functions: Vec<Function>,
    pub(crate) globals: Vec<Global>,
    pub(crate) layouts: Layouts,
    /// The external functions the unit declares without defining, each with
    /// the file declaring it.
    pub(crate) declared: Vec<(FnName, Arc<str>)>,
    pub(crate) typedefs: FxHashMap<Box<str>, Ty>,
}

impl Program {
    pub(crate) fn add(&mut self, part: UnitPart) {
        for function in part.functions {
            self.functions
                .entry(function.name.clone())
                .or_default()
                .push(function);
        }
        for global in part.globals {
            self.globals
                .entry(global.name.clone())
                .or_default()
                .push(global);
        }
        for (name, layout) in part.layouts {
            match self.records.get(&name) {
                None => {
                    self.records.insert(name, layout);
                }
                Some(known) => {
                    if let Some(difference) = layout_difference(known, &layout) {
                        let differences = self
                            .conflicts
                            .entry(name)
                            .or_insert_with(|| vec![format!("{} bytes", known.size)]);
                        if !differences.contains(&difference) {
                            differences.push(difference);
                        }
                    }
                }
            }
        }
        for (name, ty) in part.typedefs {
            match self.typedefs.get(&name) {
                None => {
                    self.typedefs.insert(name, ty);
                }
                Some(known) if *known == ty => {}
                Some(known) => {
                    let types = self
                        .typedef_conflicts
                        .entry(name)
                        .or_insert_with(|| vec![known.clone()]);
                    if !types.contains(&ty) {
                        types.push(ty);
                    }
                }
            }
        }
        for (name, file) in part.declared {
            let files = self.declared.entry(name).or_default();
            if !files.contains(&file) {
                files.push(file);
            }
        }
    }

    /// Drops the declarations of the functions some unit defines.
    pub(crate) fn finish(&mut self) {
        let functions = &self.functions;
        self.declared
            .retain(|name, _| !functions.contains_key(name));
    }
}

/// How a layout differs from the one known: its size and its first member
/// placed otherwise; none where they agree on every member's name and
/// offset.
fn layout_difference(known: &Layout, other: &Layout) -> Option<String> {
    let mismatch = known
        .fields
        .iter()
        .zip(&other.fields)
        .find(|(a, b)| a.name != b.name || a.offset != b.offset)
        .map(|(_, b)| format!("{} at {}", b.name, b.offset));
    let mismatch = mismatch.or_else(|| {
        (known.fields.len() != other.fields.len()).then(|| {
            let extra = other.fields.get(known.fields.len()).or(known.fields.last());
            extra.map_or_else(String::new, |f| format!("{} at {}", f.name, f.offset))
        })
    });
    if mismatch.is_none() && known.size == other.size && known.is_union == other.is_union {
        return None;
    }
    Some(format!(
        "{} bytes{}",
        other.size,
        mismatch.map_or_else(String::new, |m| format!(", {m}"))
    ))
}
