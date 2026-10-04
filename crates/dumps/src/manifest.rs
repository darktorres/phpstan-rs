//! A dump's manifest: its entities, each with its id, its length and where
//! the run wrote its text.
//!
//! The manifest is a text beside the dump's: a header, then a line for
//! each entity, in the text's order (a dump of files: by path).
//!
//! ```text
//! dump tree
//! root <id of the dump's tree>
//! reference <the manifest the run was given>
//! <id>\t<length>\t<offset in the text, or ->\t<path>
//! ```
//!
//! An entity the run did not write, since the reference's entity at its
//! path has its id, has `-` for its offset; its text is the reference's.

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::id::Id;
use crate::invalid;
use crate::tree::TreeBuilder;

/// How a dump is cut into entities.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// By a canonical print's sections, fields and entries (`split`).
    Tree,
    /// Into runs of lines alike before the first of this byte.
    ByPrefix(u8),
    /// A file for each entity, given with its path: the text holds them in
    /// the order they were written, the manifest by path.
    Files,
}

/// An entity of a dump.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: Id,
    pub len: u64,
    /// Where its text starts in the dump's text, `None` where the run did
    /// not write it.
    pub offset: Option<u64>,
    /// Its components, joined by `/` (`escape`).
    pub path: String,
}

/// A dump's manifest.
#[derive(Clone, Debug)]
pub struct Manifest {
    pub kind: Kind,
    /// The id of the dump's tree.
    pub root: Id,
    /// The manifest of the run whose entities this run wrote only where
    /// they differ.
    pub reference: Option<PathBuf>,
    pub entries: Vec<Entry>,
}

impl Manifest {
    pub fn read(path: &Path) -> io::Result<Manifest> {
        let context =
            |error: io::Error| io::Error::new(error.kind(), format!("{}: {error}", path.display()));
        let mut lines = BufReader::new(File::open(path).map_err(context)?).lines();
        let mut header = |name: &str| -> io::Result<String> {
            let line = lines.next().transpose()?.unwrap_or_default();
            match line
                .strip_prefix(name)
                .and_then(|rest| rest.strip_prefix(' '))
            {
                Some(value) => Ok(value.to_string()),
                None => Err(invalid(format!("expected `{name}`, found {line:?}"))),
            }
        };
        let kind = match header("dump").map_err(context)?.as_str() {
            "tree" => Kind::Tree,
            "files" => Kind::Files,
            kind => match kind
                .strip_prefix("by-prefix ")
                .map(|byte| u8::from_str_radix(byte, 16))
            {
                Some(Ok(separator)) => Kind::ByPrefix(separator),
                _ => return Err(context(invalid(format!("no such kind of dump: {kind:?}")))),
            },
        };
        let root = header("root").map_err(context)?.parse().map_err(context)?;
        let mut reference = None;
        let mut entries = Vec::new();
        for line in lines {
            let line = line.map_err(context)?;
            if let Some(path) = line.strip_prefix("reference ") {
                reference = Some(PathBuf::from(path));
                continue;
            }
            let mut fields = line.splitn(4, '\t');
            let mut field = || {
                fields
                    .next()
                    .ok_or_else(|| context(invalid(format!("not an entity: {line:?}"))))
            };
            let id = field()?.parse().map_err(context)?;
            let len = field()?
                .parse()
                .map_err(|_| context(invalid(format!("not a length: {line:?}"))))?;
            let offset = match field()? {
                "-" => None,
                offset => Some(
                    offset
                        .parse()
                        .map_err(|_| context(invalid(format!("not an offset: {line:?}"))))?,
                ),
            };
            let path = field()?.to_string();
            entries.push(Entry {
                id,
                len,
                offset,
                path,
            });
        }
        Ok(Manifest {
            kind,
            root,
            reference,
            entries,
        })
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        let mut out = BufWriter::new(File::create(path)?);
        match self.kind {
            Kind::Tree => writeln!(out, "dump tree")?,
            Kind::ByPrefix(separator) => writeln!(out, "dump by-prefix {separator:02x}")?,
            Kind::Files => writeln!(out, "dump files")?,
        }
        writeln!(out, "root {}", self.root)?;
        if let Some(reference) = &self.reference {
            let reference = reference
                .to_str()
                .ok_or_else(|| invalid(format!("not UTF-8: {}", reference.display())))?;
            writeln!(out, "reference {reference}")?;
        }
        for Entry {
            id,
            len,
            offset,
            path,
        } in &self.entries
        {
            match offset {
                Some(offset) => writeln!(out, "{id}\t{len}\t{offset}\t{path}")?,
                None => writeln!(out, "{id}\t{len}\t-\t{path}")?,
            }
        }
        out.flush()
    }

    /// The manifest of a dump of files holding `entries`, which it sorts
    /// by path.
    pub fn of_files(mut entries: Vec<Entry>) -> io::Result<Manifest> {
        entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        let mut tree = TreeBuilder::new(false);
        for entry in &entries {
            tree.add(&entry.path, entry.id)?;
        }
        Ok(Manifest {
            kind: Kind::Files,
            root: tree.finish()?.0,
            reference: None,
            entries,
        })
    }

    /// Each entity's id by its path.
    pub fn ids(&self) -> FxHashMap<&str, Id> {
        self.entries
            .iter()
            .map(|entry| (entry.path.as_str(), entry.id))
            .collect()
    }
}

/// The text of a directory's `.order`: `names`, a line each.
pub fn order_text<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<u8> {
    let mut text = Vec::new();
    for name in names {
        text.extend_from_slice(name.as_bytes());
        text.push(b'\n');
    }
    text
}

/// Where two dumps first differ, in their manifests' order: the index in
/// each of the first entity whose path or id the other's at that index
/// does not have, `None` past either's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Difference {
    pub a: Option<usize>,
    pub b: Option<usize>,
}

/// Where `a` and `b` first differ, `None` when they are alike.
pub fn first_difference(a: &Manifest, b: &Manifest) -> Option<Difference> {
    if a.kind == b.kind && a.root == b.root {
        return None;
    }
    let at = a
        .entries
        .iter()
        .zip(&b.entries)
        .position(|(a, b)| a.id != b.id || a.path != b.path)
        .unwrap_or(a.entries.len().min(b.entries.len()));
    Some(Difference {
        a: (at < a.entries.len()).then_some(at),
        b: (at < b.entries.len()).then_some(at),
    })
}

/// A dump a run wrote, to read its entities' texts from.
pub struct Dump {
    pub manifest: Manifest,
    manifest_path: PathBuf,
    text: Option<File>,
    by_path: Option<FxHashMap<String, usize>>,
    reference: Option<Box<Dump>>,
}

impl Dump {
    /// The dump whose manifest is at `manifest`.
    pub fn open(manifest: &Path) -> io::Result<Dump> {
        Ok(Dump {
            manifest: Manifest::read(manifest)?,
            manifest_path: manifest.to_path_buf(),
            text: None,
            by_path: None,
            reference: None,
        })
    }

    /// Where its text is: its manifest's path without `.manifest`.
    pub fn text_path(&self) -> PathBuf {
        self.manifest_path.with_extension("")
    }

    /// The index of the entity at `path`.
    pub fn find(&mut self, path: &str) -> Option<usize> {
        let entries = &self.manifest.entries;
        let by_path = self.by_path.get_or_insert_with(|| {
            entries
                .iter()
                .enumerate()
                .map(|(index, entry)| (entry.path.clone(), index))
                .collect()
        });
        by_path.get(path).copied()
    }

    /// The text of the entity at `index`: from the dump's text, or from
    /// its reference's where the run did not write it.
    pub fn text(&mut self, index: usize) -> io::Result<Vec<u8>> {
        let Entry {
            id,
            len,
            offset,
            ref path,
        } = self.manifest.entries[index];
        let Some(offset) = offset else {
            let path = path.clone();
            let reference = self.reference()?;
            let found = reference
                .find(&path)
                .filter(|&found| reference.manifest.entries[found].id == id)
                .ok_or_else(|| {
                    invalid(format!("{path} is in neither the dump nor its reference"))
                })?;
            return reference.text(found);
        };
        if self.text.is_none() {
            self.text = Some(File::open(self.text_path())?);
        }
        let file = self.text.as_mut().expect("opened above");
        file.seek(SeekFrom::Start(offset))?;
        let mut text = vec![0; usize::try_from(len).map_err(|_| invalid("an entity past memory"))?];
        file.read_exact(&mut text)?;
        Ok(text)
    }

    /// Writes the dump's text, each entity's in the manifest's order.
    pub fn write_text(&mut self, out: &mut dyn Write) -> io::Result<()> {
        for index in 0..self.manifest.entries.len() {
            out.write_all(&self.text(index)?)?;
        }
        Ok(())
    }

    fn reference(&mut self) -> io::Result<&mut Dump> {
        if self.reference.is_none() {
            let path = self.manifest.reference.clone().ok_or_else(|| {
                invalid(format!(
                    "{} writes no text but has no reference",
                    self.manifest_path.display()
                ))
            })?;
            self.reference = Some(Box::new(Dump::open(&path)?));
        }
        Ok(self.reference.as_mut().expect("opened above"))
    }
}
