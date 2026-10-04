//! `cargo xtask store import`: the snapshots kept as plain files before the
//! store, `<artifact>/<commit>.<ext>` and `<artifact>/<commit>/` in the
//! gate's directory, and the front end's text pool, into the store, each
//! commit's snapshot under its ref, as `corpus` would have recorded it.
//!
//! A commit's files go once its snapshot is checked against them: each
//! entry's tree or blob is the one computed from the files, a text read
//! back from the store is the text the files held, and each pooled text is
//! the one its digest names. The pool goes once every snapshot is in.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::rc::Rc;

use dumps::{Entry, Id, Manifest};
use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};

use super::store::{Content, Store, snapshot_ref, text_kind};
use super::{gate_dir, git};

/// The directories of the gate's directory that hold no snapshots.
const KEPT: &[&str] = &[
    "baseline",
    "bin",
    "buildtime",
    "front-end-texts",
    "gatetime",
    "oracle-runs",
    "perf",
    "runtime-oracle",
    "store.git",
    "tests",
];

pub(crate) fn import_snapshots() -> Result<(), String> {
    let gate = gate_dir();
    let mut store = Store::open()?;
    let pool = Rc::new(RefCell::new(Pool::new(gate.join("front-end-texts"))));
    let mut commits = plain_snapshots(&gate)?;
    commits.sort_by_key(|(hash, _)| {
        git(&["show", "-s", "--format=%ct", hash])
            .ok()
            .and_then(|time| time.parse::<u64>().ok())
            .unwrap_or(0)
    });
    for (hash, files) in &commits {
        let mut checks = Vec::new();
        let mut entries = Vec::new();
        for (entry, path) in files {
            entries.push((entry.clone(), content(entry, path, &pool, &mut checks)?));
        }
        store.add(
            &snapshot_ref(hash),
            "import the snapshot kept as plain files",
            entries,
        )?;
        for (entry, digest) in checks {
            let mut read_back = Hashing(Sha256::new());
            let rev = format!("{}:{entry}", snapshot_ref(hash));
            store.write_text(&rev, &mut read_back)?;
            if read_back.0.finalize() != digest.borrow().clone().finalize() {
                return Err(format!("{rev} reads back as another text than its files'"));
            }
        }
        for (_, path) in files {
            remove(path)?;
        }
        eprintln!("{hash}: {} entries imported", files.len());
    }
    let pool = pool.borrow();
    if pool.dir.is_dir() {
        let unused = std::fs::read_dir(&pool.dir)
            .map_err(|e| format!("{}: {e}", pool.dir.display()))?
            .filter(|file| {
                file.as_ref().is_ok_and(|file| {
                    let name = file.file_name().to_string_lossy().into_owned();
                    let digest = name.strip_suffix(".gz").unwrap_or(&name);
                    !pool.known.contains_key(digest)
                })
            })
            .count();
        eprintln!(
            "{} pooled texts imported, {unused} that no snapshot names left out",
            pool.known.len()
        );
        remove(&pool.dir)?;
    }
    Ok(())
}

/// A commit's snapshot kept as plain files: the commit, and each entry's
/// name in the snapshot with its file or directory.
type PlainSnapshot = (String, Vec<(String, PathBuf)>);

/// Each commit with a snapshot kept as plain files.
fn plain_snapshots(gate: &Path) -> Result<Vec<PlainSnapshot>, String> {
    let mut commits: BTreeMap<String, BTreeMap<String, PathBuf>> = BTreeMap::new();
    for artifact in read_dir(gate)? {
        let name = file_name(&artifact);
        if !artifact.is_dir() || name.starts_with('.') || KEPT.contains(&name.as_str()) {
            continue;
        }
        for path in read_dir(&artifact)? {
            let file = file_name(&path);
            let (hash, ext) = file.split_at(file.len().min(40));
            if !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) || hash.len() != 40 {
                return Err(format!("{}: not a commit's snapshot", path.display()));
            }
            let entry = match ext {
                "" if path.is_dir() => name.clone(),
                ".txt" | ".txt.gz" | ".txt.zst" => format!("{name}.txt"),
                ".err" => format!("{name}.err"),
                _ => return Err(format!("{}: not a commit's snapshot", path.display())),
            };
            let entries = commits.entry(hash.to_string()).or_default();
            if let Some(other) = entries.insert(entry.clone(), path.clone()) {
                return Err(format!(
                    "{} and {} are both {entry} of {hash}",
                    other.display(),
                    path.display()
                ));
            }
        }
    }
    Ok(commits
        .into_iter()
        .map(|(hash, entries)| (hash, entries.into_iter().collect()))
        .collect())
}

/// What the store keeps of the snapshot's `entry`, kept at `path`; a text
/// it cuts adds the digest its text will have to `checks`.
fn content(
    entry: &str,
    path: &Path,
    pool: &Rc<RefCell<Pool>>,
    checks: &mut Vec<(String, Rc<RefCell<Sha256>>)>,
) -> Result<Content, String> {
    if path.is_dir() {
        return if entry.starts_with("front-end-") {
            front_end(path, pool)
        } else if entry.starts_with("replay-") {
            let mut entries = Vec::new();
            for file in read_dir(path)? {
                let name = file_name(&file);
                let name = match name.strip_suffix(".gz") {
                    Some(stem) => format!("{stem}.txt"),
                    None if matches!(name.as_str(), "script.txt" | "verdict.txt") => name,
                    None => return Err(format!("{}: not a replay's", file.display())),
                };
                let content = text(&format!("{entry}/{name}"), &file, checks)?;
                entries.push((name, content));
            }
            Ok(Content::Dir(entries))
        } else {
            Err(format!(
                "{}: a directory of an unknown kind",
                path.display()
            ))
        };
    }
    text(entry, path, checks)
}

/// The text kept at `path`, decompressed, for the snapshot's `entry`.
fn text(
    entry: &str,
    path: &Path,
    checks: &mut Vec<(String, Rc<RefCell<Sha256>>)>,
) -> Result<Content, String> {
    let name = entry.rsplit('/').next().unwrap_or(entry);
    let reader = decompressed(path)?;
    Ok(match text_kind(name) {
        Some(kind) => {
            let digest = Rc::new(RefCell::new(Sha256::new()));
            checks.push((entry.to_string(), Rc::clone(&digest)));
            Content::Cut(kind, Box::new(HashingReader { reader, digest }))
        }
        None => {
            let mut text = Vec::new();
            let mut reader = reader;
            reader
                .read_to_end(&mut text)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            Content::Text(text)
        }
    })
}

/// A front-end dump kept as an index of its files' pooled texts by digest,
/// and its unions, as the dump of files a run writes now.
fn front_end(dir: &Path, pool: &Rc<RefCell<Pool>>) -> Result<Content, String> {
    for file in read_dir(dir)? {
        if !matches!(file_name(&file).as_str(), "index.tsv" | "unions.gz") {
            return Err(format!("{}: not a front-end dump's", file.display()));
        }
    }
    let index = dir.join("index.tsv");
    let lines = std::fs::read_to_string(&index).map_err(|e| format!("{}: {e}", index.display()))?;
    let mut sources: FxHashMap<String, Source> = FxHashMap::default();
    let mut entries = Vec::new();
    for line in lines.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [role, file, digest, len] = fields[..] else {
            return Err(format!("{}: {line:?}", index.display()));
        };
        let len: u64 = len
            .parse()
            .map_err(|_| format!("{}: {line:?}", index.display()))?;
        let (components, source): (Vec<&str>, Source) = if digest == "unread" {
            if len != 0 {
                return Err(format!("{}: {line:?}", index.display()));
            }
            (
                ["unread", role]
                    .into_iter()
                    .chain(file.split('/'))
                    .collect(),
                Source::Empty,
            )
        } else {
            let id = pool.borrow_mut().id(digest, len)?;
            (
                std::iter::once(role).chain(file.split('/')).collect(),
                Source::Pooled(digest.to_string(), len, id),
            )
        };
        let path = components
            .into_iter()
            .map(|component| dumps::escape(component.as_bytes()))
            .collect::<Vec<_>>()
            .join("/");
        let (id, len) = (source.id(), source.len());
        match sources.get(&path) {
            Some(known) if known.id() == id => continue,
            Some(_) => {
                return Err(format!(
                    "{}: {path} is listed twice, with two texts",
                    index.display()
                ));
            }
            None => {}
        }
        sources.insert(path.clone(), source);
        entries.push(Entry {
            id,
            len,
            offset: None,
            path,
        });
    }
    let unions = dir.join("unions.gz");
    if unions.exists() {
        let mut text = Vec::new();
        decompressed(&unions)?
            .read_to_end(&mut text)
            .map_err(|e| format!("{}: {e}", unions.display()))?;
        entries.push(Entry {
            id: Id::blob(&text),
            len: text.len() as u64,
            offset: None,
            path: "unions".to_string(),
        });
        sources.insert("unions".to_string(), Source::Text(text));
    }
    let manifest = Manifest::of_files(entries).map_err(|e| format!("{}: {e}", dir.display()))?;
    let sources: Vec<Source> = manifest
        .entries
        .iter()
        .map(|entry| {
            sources
                .remove(&entry.path)
                .expect("each entry has its source")
        })
        .collect();
    let pool = Rc::clone(pool);
    Ok(Content::Files(
        manifest,
        Box::new(move |index| match &sources[index] {
            Source::Empty => Ok(Vec::new()),
            Source::Text(text) => Ok(text.clone()),
            Source::Pooled(digest, len, _) => pool.borrow().text(digest, *len),
        }),
    ))
}

/// Where a front-end dump's file's text is.
enum Source {
    /// A file that could not be read.
    Empty,
    Text(Vec<u8>),
    /// In the pool, by its digest, with its length and id.
    Pooled(String, u64, Id),
}

impl Source {
    fn id(&self) -> Id {
        match self {
            Source::Empty => Id::blob(b""),
            Source::Text(text) => Id::blob(text),
            Source::Pooled(_, _, id) => *id,
        }
    }

    fn len(&self) -> u64 {
        match self {
            Source::Empty => 0,
            Source::Text(text) => text.len() as u64,
            Source::Pooled(_, len, _) => *len,
        }
    }
}

/// The front end's text pool: each text gzipped, named by its BLAKE3
/// digest.
struct Pool {
    dir: PathBuf,
    /// The id of each text read, by its digest.
    known: FxHashMap<String, Id>,
}

impl Pool {
    fn new(dir: PathBuf) -> Pool {
        Pool {
            dir,
            known: FxHashMap::default(),
        }
    }

    /// The id of the text of `digest`, `len` bytes long.
    fn id(&mut self, digest: &str, len: u64) -> Result<Id, String> {
        if let Some(id) = self.known.get(digest) {
            return Ok(*id);
        }
        let id = Id::blob(&self.text(digest, len)?);
        self.known.insert(digest.to_string(), id);
        Ok(id)
    }

    /// The text of `digest`, required to be `len` bytes long and to have
    /// that digest.
    fn text(&self, digest: &str, len: u64) -> Result<Vec<u8>, String> {
        let path = self.dir.join(format!("{digest}.gz"));
        let mut text = Vec::new();
        decompressed(&path)?
            .read_to_end(&mut text)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if text.len() as u64 != len || blake3::hash(&text).to_hex().as_str() != digest {
            return Err(format!(
                "{}: not the text of {len} bytes its name gives",
                path.display()
            ));
        }
        Ok(text)
    }
}

/// The text at `path`, decompressed as its extension says.
fn decompressed(path: &Path) -> Result<Box<dyn Read>, String> {
    let file = || File::open(path).map_err(|e| format!("{}: {e}", path.display()));
    let name = path.to_string_lossy();
    if name.ends_with(".gz") {
        Ok(Box::new(flate2::read::MultiGzDecoder::new(BufReader::new(
            file()?,
        ))))
    } else if name.ends_with(".zst") {
        let mut child = Command::new("zstd")
            .args(["-dc", "-q"])
            .arg(path)
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("zstd {}: {e}", path.display()))?;
        let stdout = child.stdout.take().expect("piped");
        Ok(Box::new(ChildReader { child, stdout }))
    } else {
        Ok(Box::new(BufReader::new(file()?)))
    }
}

/// A child's output, an error at its end unless the child succeeded.
struct ChildReader {
    child: Child,
    stdout: ChildStdout,
}

impl Read for ChildReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.stdout.read(buf)?;
        if read == 0 && !buf.is_empty() {
            let status = self.child.wait()?;
            if !status.success() {
                return Err(io::Error::other(format!("{status}")));
            }
        }
        Ok(read)
    }
}

impl Drop for ChildReader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A reader adding what it reads to a digest.
struct HashingReader {
    reader: Box<dyn Read>,
    digest: Rc<RefCell<Sha256>>,
}

impl Read for HashingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.reader.read(buf)?;
        self.digest.borrow_mut().update(&buf[..read]);
        Ok(read)
    }
}

/// A writer adding what it is given to a digest.
struct Hashing(Sha256);

impl Write for Hashing {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn read_dir(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    paths.sort();
    Ok(paths)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn remove(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
    .map_err(|e| format!("{}: {e}", path.display()))
}
