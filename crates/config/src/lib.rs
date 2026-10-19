//! The TOML config file (`phpstan-rs.toml`): global keys, and one table per
//! owner (`[rules]`, `[ext.<name>]`, ...). Each owner declares its part as
//! a typed [`Section`]; the composition root registers every section in a
//! [`Schema`], which rejects any key no section owns, reporting its full
//! dotted path, and moves keys from older layouts to their new place with
//! a warning.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

use std::fmt;
use std::path::{Path, PathBuf};

use serde::de::{self, DeserializeOwned, Deserializer, Visitor};

/// One owner's part of the config file.
pub trait Section: DeserializeOwned + Default {
    /// Where it sits: `&[]` for keys at the top level, `&["ext", "extras"]`
    /// for the `[ext.extras]` table.
    const PATH: &'static [&'static str];
}

/// A section whose owner is switched on as a whole, as PHPStan's
/// extension installer loads an installed extension: by the section's own
/// `enabled` key, or, unset, by one of its packages being installed.
pub trait Extension: Section {
    /// The packages whose installation activates it.
    const PACKAGES: &'static [&'static str];

    /// `enabled`, when the section sets it.
    fn enabled(&self) -> Option<bool>;

    /// Whether it's active, `installed` telling whether a package is
    /// installed.
    fn is_active(&self, installed: &dyn Fn(&str) -> bool) -> bool {
        self.enabled()
            .unwrap_or_else(|| Self::PACKAGES.iter().any(|p| installed(p)))
    }
}

/// Every registered section, and the keys moved from older layouts.
#[derive(Default)]
pub struct Schema {
    sections: Vec<RegisteredSection>,
    moves: Vec<Move>,
}

struct RegisteredSection {
    path: &'static [&'static str],
    /// The section's keys, when it's a table.
    fields: &'static [&'static str],
    check: fn(&toml::Value) -> Result<(), String>,
}

struct Move {
    old: &'static str,
    new: &'static [&'static str],
}

impl Schema {
    pub fn new() -> Schema {
        Schema::default()
    }

    /// Registers `S`.
    pub fn section<S: Section>(mut self) -> Schema {
        self.sections.push(RegisteredSection {
            path: S::PATH,
            fields: fields_of::<S>(),
            check: |value| {
                value
                    .clone()
                    .try_into::<S>()
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
        });
        self
    }

    /// Registers top-level key `old` as moved to `new`.
    pub fn moved(mut self, old: &'static str, new: &'static [&'static str]) -> Schema {
        self.moves.push(Move { old, new });
        self
    }

    /// Moves every old top-level key in `table` to its new place.
    fn apply_moves(&self, table: &mut toml::Table) -> Result<Vec<String>, String> {
        let mut warnings = Vec::new();
        for m in &self.moves {
            let Some(value) = table.remove(m.old) else {
                continue;
            };
            if lookup(table, m.new).is_some() {
                return Err(format!(
                    "config sets both `{}` and `{}`",
                    m.old,
                    dotted(m.new)
                ));
            }
            insert(table, m.new, value)?;
            warnings.push(deprecation(m));
        }
        Ok(warnings)
    }

    /// The path `key` (dotted) names, mapped through the moves.
    fn resolve_key(&self, key: &str, warnings: &mut Vec<String>) -> Vec<String> {
        match self.moves.iter().find(|m| m.old == key) {
            Some(m) => {
                warnings.push(deprecation(m));
                m.new.iter().map(|s| s.to_string()).collect()
            }
            None => key.split('.').map(str::to_string).collect(),
        }
    }

    fn validate(&self, table: &toml::Table) -> Result<(), String> {
        let mut errors = Vec::new();
        self.check_table(&[], table, &mut errors);
        let whole = toml::Value::Table(table.clone());
        for section in &self.sections {
            let value = if section.path.is_empty() {
                Some(&whole)
            } else {
                lookup(table, section.path)
            };
            if let Some(value) = value
                && let Err(e) = (section.check)(value)
            {
                let at = if section.path.is_empty() {
                    "the top level".to_string()
                } else {
                    format!("`{}`", dotted(section.path))
                };
                errors.push(format!("config at {at}: {e}"));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    /// Reports every key in `table`, at `path`, that no section owns.
    fn check_table(&self, path: &[&str], table: &toml::Table, errors: &mut Vec<String>) {
        stack::guard(|| {
            for (key, value) in table {
                let mut child: Vec<&str> = path.to_vec();
                child.push(key);
                let exact = self.sections.iter().any(|s| s.path == child.as_slice());
                let deeper = self.sections.iter().any(|s| {
                    s.path.len() > child.len() && s.path[..child.len()] == *child.as_slice()
                });
                if exact || deeper {
                    match value {
                        toml::Value::Table(inner) => self.check_table(&child, inner, errors),
                        _ if exact => {}
                        _ => {
                            errors.push(format!("config key `{}` must be a table", dotted(&child)))
                        }
                    }
                    continue;
                }
                let owned = self
                    .sections
                    .iter()
                    .any(|s| s.path == path && s.fields.contains(&key.as_str()));
                if !owned {
                    errors.push(format!("unknown config key `{}`", dotted(&child)));
                }
            }
        })
    }
}

fn deprecation(m: &Move) -> String {
    format!(
        "config key `{}` is deprecated; use `{}`",
        m.old,
        dotted(m.new)
    )
}

/// A loaded config file, with the old keys moved and the command line's
/// `--set` assignments applied.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigFile {
    table: toml::Table,
    path: Option<PathBuf>,
    warnings: Vec<String>,
}

impl ConfigFile {
    /// No config file: every section at its default.
    pub fn empty() -> ConfigFile {
        ConfigFile::default()
    }

    pub fn load(path: &Path, schema: &Schema) -> Result<ConfigFile, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let mut file =
            ConfigFile::parse(&text, schema).map_err(|e| format!("{}: {e}", path.display()))?;
        file.path = Some(path.to_path_buf());
        Ok(file)
    }

    pub fn parse(text: &str, schema: &Schema) -> Result<ConfigFile, String> {
        let mut table: toml::Table =
            toml::from_str(text).map_err(|e| format!("failed to parse: {e}"))?;
        let warnings = schema.apply_moves(&mut table)?;
        Ok(ConfigFile {
            table,
            path: None,
            warnings,
        })
    }

    /// Applies one `--set key=value`: `key` is a dotted path (or an old
    /// key, moved), and `value` is read as a TOML value, or as a string
    /// when it isn't one.
    pub fn set(&mut self, schema: &Schema, assignment: &str) -> Result<(), String> {
        let (key, value) = assignment
            .split_once('=')
            .ok_or_else(|| format!("--set needs `key=value`, got `{assignment}`"))?;
        let path = schema.resolve_key(key.trim(), &mut self.warnings);
        let segments: Vec<&str> = path.iter().map(String::as_str).collect();
        insert(&mut self.table, &segments, parse_value(value.trim()))
    }

    /// Removes the top-level `key`, one the schema doesn't know, and returns
    /// its value.
    pub fn remove(&mut self, key: &str) -> Option<toml::Value> {
        self.table.remove(key)
    }

    /// Checks every key against `schema`.
    pub fn validate(&self, schema: &Schema) -> Result<(), String> {
        schema.validate(&self.table)
    }

    /// `S`, or its default when the file doesn't set it.
    pub fn section<S: Section>(&self) -> Result<S, String> {
        let value = if S::PATH.is_empty() {
            Some(toml::Value::Table(self.table.clone()))
        } else {
            lookup(&self.table, S::PATH).cloned()
        };
        match value {
            Some(value) => value.try_into().map_err(|e| {
                let at = if S::PATH.is_empty() {
                    "the top level".to_string()
                } else {
                    format!("`{}`", dotted(S::PATH))
                };
                format!("config at {at}: {e}")
            }),
            None => Ok(S::default()),
        }
    }

    /// The file it was loaded from.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The directory the config's relative paths resolve against.
    pub fn dir(&self) -> Option<&Path> {
        self.path.as_deref().and_then(Path::parent)
    }

    /// One line per old key used.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

fn dotted<S: AsRef<str>>(path: &[S]) -> String {
    path.iter()
        .map(AsRef::as_ref)
        .collect::<Vec<&str>>()
        .join(".")
}

fn lookup<'t>(table: &'t toml::Table, path: &[&str]) -> Option<&'t toml::Value> {
    let (last, parents) = path.split_last()?;
    let mut current = table;
    for segment in parents {
        current = current.get(*segment)?.as_table()?;
    }
    current.get(*last)
}

fn insert(table: &mut toml::Table, path: &[&str], value: toml::Value) -> Result<(), String> {
    let Some((last, parents)) = path.split_last() else {
        return Err("an empty config key".to_string());
    };
    let mut current = table;
    for (i, segment) in parents.iter().enumerate() {
        current = current
            .entry(segment.to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .ok_or_else(|| format!("config key `{}` isn't a table", dotted(&path[..=i])))?;
    }
    current.insert(last.to_string(), value);
    Ok(())
}

/// `value` as a TOML value (`true`, `5`, `["a"]`, `"x"`), else as a string.
fn parse_value(value: &str) -> toml::Value {
    toml::from_str::<toml::Table>(&format!("v = {value}"))
        .ok()
        .and_then(|mut table| table.remove("v"))
        .unwrap_or_else(|| toml::Value::String(value.to_string()))
}

/// The keys serde's derive gives `S` (none for a section that isn't a
/// struct): a deserializer that records the field list a derived struct
/// asks for, then stops.
fn fields_of<S: DeserializeOwned>() -> &'static [&'static str] {
    let mut fields: &'static [&'static str] = &[];
    let _ = S::deserialize(FieldProbe(&mut fields));
    fields
}

#[derive(Debug)]
struct Probed;

impl fmt::Display for Probed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("field probe")
    }
}

impl std::error::Error for Probed {}

impl de::Error for Probed {
    fn custom<T: fmt::Display>(_: T) -> Self {
        Probed
    }
}

struct FieldProbe<'a>(&'a mut &'static [&'static str]);

impl<'de> Deserializer<'de> for FieldProbe<'_> {
    type Error = Probed;

    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Probed> {
        Err(Probed)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: &'static [&'static str],
        _: V,
    ) -> Result<V::Value, Probed> {
        *self.0 = fields;
        Err(Probed)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map enum identifier ignored_any
    }
}

#[cfg(test)]
mod tests;
