//! The program the build links: every unit's functions, globals and
//! records, and the functions it calls without defining them.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use crate::ir::{FnName, Function, Global, GlobalName, Ty};
use crate::layout::{Layout, Layouts};
use crate::store::{Hash, Store};

/// A function's definition: its lowered code is in the store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Definition {
    pub(crate) hash: Hash,
    pub(crate) file: Arc<str>,
    pub(crate) line: u32,
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Program {
    /// Each function's definitions: an external function that several of
    /// the build's binaries define (each SAPI its own `main`) has each of
    /// theirs, and a call by its name reaches any of them.
    pub(crate) functions: FxHashMap<FnName, Vec<Definition>>,
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
    /// Each global declared `extern` and not defined, with the files
    /// declaring it and its type.
    pub(crate) declared_globals: FxHashMap<GlobalName, (Vec<Arc<str>>, Ty)>,
    /// The globals some unit defines without an initializer.
    pub(crate) tentative: FxHashSet<GlobalName>,
    /// Each typedef's type; a name some units give different types has
    /// the first.
    pub(crate) typedefs: FxHashMap<Box<str>, Ty>,
    /// The typedefs some units give different types, with each type.
    pub(crate) typedef_conflicts: FxHashMap<Box<str>, Vec<Ty>>,
    /// Each unnamed record's name by its typedef, for the catalog's name
    /// for it.
    pub(crate) aliases: FxHashMap<Arc<str>, Arc<str>>,
    /// The file defining each record.
    pub(crate) record_files: FxHashMap<Arc<str>, Arc<str>>,
    /// Each enumeration's size in bytes, by its tag or the typedef naming
    /// an unnamed one.
    pub(crate) enums: FxHashMap<Arc<str>, u64>,
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
    /// The globals the unit declares `extern`, each with the file
    /// declaring it and its type.
    pub(crate) declared_globals: Vec<(GlobalName, Arc<str>, Ty)>,
    /// The globals the unit defines without an initializer.
    pub(crate) tentative: Vec<GlobalName>,
    pub(crate) typedefs: FxHashMap<Box<str>, Ty>,
    pub(crate) aliases: FxHashMap<Arc<str>, Arc<str>>,
    pub(crate) record_files: FxHashMap<Arc<str>, Arc<str>>,
    pub(crate) enums: FxHashMap<Arc<str>, u64>,
}

impl Program {
    /// Adds a unit's part, its functions' code to the store.
    pub(crate) fn add(&mut self, part: UnitPart, store: &Store) -> Result<(), String> {
        for function in part.functions {
            let definition = Definition {
                hash: store.put(&function)?,
                file: function.file.clone(),
                line: function.line,
            };
            self.functions
                .entry(function.name)
                .or_default()
                .push(definition);
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
        for (alias, name) in part.aliases {
            self.aliases.entry(alias).or_insert(name);
        }
        for (record, file) in part.record_files {
            self.record_files.entry(record).or_insert(file);
        }
        for (name, size) in part.enums {
            self.enums.entry(name).or_insert(size);
        }
        for (name, file) in part.declared {
            let files = self.declared.entry(name).or_default();
            if !files.contains(&file) {
                files.push(file);
            }
        }
        for (name, file, ty) in part.declared_globals {
            let (files, _) = self
                .declared_globals
                .entry(name)
                .or_insert_with(|| (Vec::new(), ty));
            if !files.contains(&file) {
                files.push(file);
            }
        }
        self.tentative.extend(part.tentative);
        Ok(())
    }

    /// Runs `visit` on each function's definitions, by name, each loaded
    /// from the store in turn.
    pub(crate) fn each_function(
        &self,
        store: &Store,
        mut visit: impl FnMut(&Function),
    ) -> Result<(), String> {
        let mut names: Vec<&FnName> = self.functions.keys().collect();
        names.sort_unstable();
        for name in names {
            for definition in &self.functions[name] {
                let function: Function = store.get(definition.hash)?;
                visit(&function);
            }
        }
        Ok(())
    }

    /// The definitions of a function, loaded from the store.
    pub(crate) fn load(&self, store: &Store, name: &str) -> Result<Vec<Function>, String> {
        self.functions
            .get(name)
            .into_iter()
            .flatten()
            .map(|definition| store.get(definition.hash))
            .collect()
    }

    /// Drops the declarations of the functions and globals some unit
    /// defines.
    pub(crate) fn finish(&mut self) {
        let functions = &self.functions;
        self.declared
            .retain(|name, _| !functions.contains_key(name));
        let (globals, tentative) = (&self.globals, &self.tentative);
        self.declared_globals
            .retain(|name, _| !globals.contains_key(name) && !tentative.contains(name));
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
