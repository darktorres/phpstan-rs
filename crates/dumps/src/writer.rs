//! Writing a dump: its text, cut into entities as it comes, and its
//! manifest.

use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::id::Id;
use crate::manifest::{Entry, Kind, Manifest};
use crate::name::escape;
use crate::split::{Emit, Splitter};
use crate::tree::TreeBuilder;
use crate::{invalid, manifest_path};

/// Writes a dump's text and, when finished, its manifest beside it. A dump
/// cut from a text is written through `io::Write`; a dump of files, a file
/// at a time (`file`).
pub struct DumpWriter {
    splitter: Option<Splitter>,
    sink: Sink,
}

struct Sink {
    kind: Kind,
    text_path: PathBuf,
    text: BufWriter<File>,
    written: u64,
    reference: Option<(PathBuf, FxHashMap<String, Id>)>,
    entries: Vec<Entry>,
    /// A dump cut from a text: its tree, built as its entities come.
    tree: Option<TreeBuilder>,
}

impl DumpWriter {
    /// Starts the dump of `kind` whose text is written to `text`. Given
    /// `reference`, the manifest of another run of the same dump, an entity
    /// whose id is the reference's at its path is not written.
    pub fn create(text: &Path, kind: Kind, reference: Option<&Path>) -> io::Result<DumpWriter> {
        let reference = match reference {
            Some(path) => {
                let manifest = Manifest::read(path)?;
                if manifest.kind != kind {
                    return Err(invalid(format!(
                        "{} is a dump of another kind: {:?}, not {kind:?}",
                        path.display(),
                        manifest.kind
                    )));
                }
                let ids = manifest
                    .entries
                    .into_iter()
                    .map(|entry| (entry.path, entry.id))
                    .collect();
                Some((std::path::absolute(path)?, ids))
            }
            None => None,
        };
        Ok(DumpWriter {
            splitter: Splitter::new(kind),
            sink: Sink {
                kind,
                text_path: text.to_path_buf(),
                text: BufWriter::with_capacity(1 << 20, File::create(text)?),
                written: 0,
                reference,
                entries: Vec::new(),
                tree: (kind != Kind::Files).then(|| TreeBuilder::new(true)),
            },
        })
    }

    /// Adds the file `file` to a dump of files.
    pub fn file(&mut self, file: FileEntity<'_>) -> io::Result<()> {
        if self.sink.kind != Kind::Files {
            return Err(invalid("a file given to a dump cut from a text"));
        }
        let FileEntity { path, text, id } = file;
        self.sink.add(path, text, id)
    }

    /// Finishes the text and writes the manifest; the id of the dump's
    /// tree.
    pub fn finish(mut self) -> io::Result<Id> {
        if let Some(splitter) = self.splitter.take() {
            splitter.finish(&mut self.sink)?;
        }
        let Sink {
            kind,
            text_path,
            mut text,
            written: _,
            reference,
            entries,
            tree,
        } = self.sink;
        text.flush()?;
        let reference = reference.map(|(path, _)| path);
        let manifest = match tree {
            Some(tree) => Manifest {
                kind,
                root: tree.finish()?.0,
                reference,
                entries,
            },
            None => Manifest {
                reference,
                ..Manifest::of_files(entries)?
            },
        };
        manifest.write(&manifest_path(&text_path))?;
        Ok(manifest.root)
    }
}

/// What a cut gives each entity to: its path, its id and its text.
type Each<'a> = dyn FnMut(&str, Id, &[u8]) -> io::Result<()> + 'a;

/// Cuts `text` into the entities of a dump of `kind`, one cut from a text,
/// and gives each to `each` with its path and id, in the text's order.
/// Gives the dump's manifest, each offset the entity's place in `text`.
pub fn cut(kind: Kind, text: &mut dyn Read, each: &mut Each<'_>) -> io::Result<Manifest> {
    let mut splitter =
        Splitter::new(kind).ok_or_else(|| invalid("a dump of files is not cut from a text"))?;
    let mut cutting = Cutting {
        each,
        offset: 0,
        entries: Vec::new(),
        tree: TreeBuilder::new(true),
    };
    let mut buffer = vec![0; 1 << 20];
    loop {
        let read = match text.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        splitter.push(&buffer[..read], &mut cutting)?;
    }
    splitter.finish(&mut cutting)?;
    Ok(Manifest {
        kind,
        root: cutting.tree.finish()?.0,
        reference: None,
        entries: cutting.entries,
    })
}

struct Cutting<'a, 'b> {
    each: &'a mut Each<'b>,
    offset: u64,
    entries: Vec<Entry>,
    tree: TreeBuilder,
}

impl Emit for Cutting<'_, '_> {
    fn entity(&mut self, path: &str, text: &[u8]) -> io::Result<()> {
        let id = Id::blob(text);
        (self.each)(path, id, text)?;
        self.tree.add(path, id)?;
        let len = text.len() as u64;
        self.entries.push(Entry {
            id,
            len,
            offset: Some(self.offset),
            path: path.to_string(),
        });
        self.offset += len;
        Ok(())
    }
}

impl Write for DumpWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let splitter = self
            .splitter
            .as_mut()
            .ok_or_else(|| invalid("a text written to a dump of files"))?;
        splitter.push(bytes, &mut self.sink)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.sink.text.flush()
    }
}

impl Sink {
    fn add(&mut self, path: String, text: &[u8], id: Id) -> io::Result<()> {
        let unchanged = self
            .reference
            .as_ref()
            .is_some_and(|(_, ids)| ids.get(&path) == Some(&id));
        let len = text.len() as u64;
        let offset = if unchanged {
            None
        } else {
            self.text.write_all(text)?;
            let offset = self.written;
            self.written += len;
            Some(offset)
        };
        if let Some(tree) = &mut self.tree {
            tree.add(&path, id)?;
        }
        self.entries.push(Entry {
            id,
            len,
            offset,
            path,
        });
        Ok(())
    }
}

impl Emit for Sink {
    fn entity(&mut self, path: &str, text: &[u8]) -> io::Result<()> {
        self.add(path.to_string(), text, Id::blob(text))
    }
}

/// A file of a dump of files, its id computed where it is made, so that
/// threads making files hash at once and hand the writer only the text.
pub struct FileEntity<'a> {
    path: String,
    text: &'a [u8],
    id: Id,
}

impl<'a> FileEntity<'a> {
    /// The file at `components`, each a key of its own (`escape`), holding
    /// `text`.
    pub fn new<'c>(
        components: impl IntoIterator<Item = &'c [u8]>,
        text: &'a [u8],
    ) -> FileEntity<'a> {
        let path = components
            .into_iter()
            .map(escape)
            .collect::<Vec<_>>()
            .join("/");
        FileEntity {
            path,
            text,
            id: Id::blob(text),
        }
    }
}
