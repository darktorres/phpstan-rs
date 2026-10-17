//! The per-file cache: what parsing a context file gave, stored on disk for
//! the next run to read back instead of lexing, parsing and building the
//! file again.
//!
//! What is kept is all a context file contributes to a run: its
//! declarations as built, its names and references, the summaries of its
//! bodies, and its lines. Its tree is not kept, as a batch run drops it at
//! once. An analysed file is always parsed, since checking needs its tree.
//!
//! An entry is found by a hash of everything that decides it: the file's
//! bytes, the path its declarations name it by, how it was parsed
//! (`is_stub`, and whether use sites are indexed with it), and the build's
//! source hash, which a change to the code or the stubs changes. An entry
//! under another build's hash is never read. A truncated or damaged entry
//! fails its checksum and the file is parsed as if it had none. The size of
//! the cache is bounded: after a run that stored entries, the oldest go
//! first. `PHPSTAN_RS_NO_FILE_CACHE` turns it off; so does the stub cache's
//! switch.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use rustc_hash::FxHashSet;
use syntax::FileFacts;
use syntax::lexer::LexWarning;
use syntax::name::Name;
use syntax::span::{LineIndex, Span};

use super::declared_classes::DeclaredClass;
use super::duplicate_class_sites::ClassDeclSite;
use super::duplicate_fn_sites::FnDeclSite;
use super::use_sites::RefTarget;
use super::{Arc, ParsedFile};
use crate::deps::RefKey;
use crate::pest_config::PestFacts;
use crate::registry::{BuiltDecl, FileBodies, GlobalFacts};

/// The build's source hash: an entry carries it in its path.
const BUILD: &str = env!("PHPSTAN_RS_SOURCE_HASH");

const MAGIC: [u8; 8] = *b"PRSFILE1";

/// Magic, the payload's length, its checksum.
const HEADER_LEN: usize = 8 + 8 + 4;

/// How large the cache may grow, in megabytes, unless
/// `PHPSTAN_RS_FILE_CACHE_MB` says otherwise.
const DEFAULT_CAP_MB: u64 = 2048;

/// What an entry's last use is refreshed to, once it is this old: the
/// order eviction goes in.
const REFRESH_AFTER: Duration = Duration::from_secs(12 * 3600);

/// The entries stored by this process.
static STORED: AtomicUsize = AtomicUsize::new(0);

/// The directory of this build's entries, when the cache is on.
fn directory() -> Option<&'static Path> {
    static DIRECTORY: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIRECTORY
        .get_or_init(|| {
            if std::env::var_os("PHPSTAN_RS_NO_FILE_CACHE").is_some() {
                return None;
            }
            Some(crate::registry::cache_dir()?.join("files").join(BUILD))
        })
        .as_deref()
}

/// Whether entries can be stored: checked once, at the first store, by
/// doing what a store does first - creating the build's directory and a
/// file in it - and the 256 shard directories entries go in. A run that
/// stores nothing never looks, and one that can't store builds none of
/// the entries. Says so, once, when it can't; the cache is read all the
/// same.
pub(super) fn can_store() -> bool {
    static WRITABLE: OnceLock<bool> = OnceLock::new();
    *WRITABLE.get_or_init(|| {
        let Some(directory) = directory() else {
            return false;
        };
        let prepared = crate::registry::probe_writable(directory).and_then(|()| {
            (0..=255u8).try_for_each(|shard| fs::create_dir_all(directory.join(format!("{shard:02x}"))))
        });
        match prepared {
            Ok(()) => true,
            Err(error) => {
                eprintln!(
                    "phpstan-rs: the per-file cache in {} can't be written ({error}); it is read, not stored to",
                    directory.display()
                );
                false
            }
        }
    })
}

/// What a file of a context parse is stored as.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct CachedFile {
    /// How long the file is as checking weighs it.
    pub(super) len: usize,
    lex_warnings: Vec<LexWarning>,
    functions: Vec<Name>,
    classes: Vec<Name>,
    constants: Vec<Name>,
    references: Vec<RefKey>,
    declaration_references: Vec<RefKey>,
    built_decls: Vec<BuiltDecl>,
    pest_facts: Option<PestFacts>,
    declared_classes: Vec<DeclaredClass>,
    duplicate_fn_sites: Vec<(String, FnDeclSite)>,
    anonymous_classes: Vec<Name>,
    duplicate_class_sites: Vec<(String, ClassDeclSite)>,
    bodies: FileBodies,
    global_facts: Arc<GlobalFacts>,
    facts: FileFacts,
    line_index: Arc<LineIndex>,
    class_fn_refs: Option<Vec<(Span, RefTarget)>>,
}

impl CachedFile {
    /// What `parsed`, of a file `len` long, is stored as.
    pub(super) fn of(parsed: &ParsedFile, len: usize) -> CachedFile {
        CachedFile {
            len,
            lex_warnings: parsed.lex_warnings.clone(),
            functions: parsed.functions.clone(),
            classes: parsed.classes.clone(),
            constants: parsed.constants.clone(),
            references: parsed.references.iter().cloned().collect(),
            declaration_references: parsed.declaration_references.iter().cloned().collect(),
            built_decls: parsed.built_decls.clone(),
            pest_facts: parsed.pest_facts.clone(),
            declared_classes: parsed.declared_classes.clone(),
            duplicate_fn_sites: parsed.duplicate_fn_sites.clone(),
            anonymous_classes: parsed.anonymous_classes.clone(),
            duplicate_class_sites: parsed.duplicate_class_sites.clone(),
            bodies: parsed.bodies.clone(),
            global_facts: Arc::clone(&parsed.global_facts),
            facts: parsed.facts,
            line_index: Arc::clone(&parsed.line_index),
            class_fn_refs: parsed.class_fn_refs.clone(),
        }
    }

    /// The parse of the file at `path` this was stored from, `source` its
    /// bytes, without its tree and its ignore comments, which only an
    /// analysed file reads.
    pub(super) fn into_parsed(self, path: &Path, source: &[u8]) -> ParsedFile {
        let references: FxHashSet<RefKey> = self.references.into_iter().collect();
        let declaration_references: FxHashSet<RefKey> =
            self.declaration_references.into_iter().collect();
        ParsedFile {
            ast: None,
            lex_warnings: self.lex_warnings,
            functions: self.functions,
            classes: self.classes,
            constants: self.constants,
            references,
            declaration_references,
            ignores: Default::default(),
            built_decls: self.built_decls,
            pest_facts: self.pest_facts,
            declared_classes: self.declared_classes,
            duplicate_fn_sites: self.duplicate_fn_sites,
            anonymous_classes: self.anonymous_classes,
            duplicate_class_sites: self.duplicate_class_sites,
            bodies: self.bodies.with_source(source),
            global_facts: self.global_facts,
            origin_path: Arc::from(path),
            facts: self.facts,
            line_index: self.line_index,
            class_fn_refs: self.class_fn_refs,
            owner: None,
        }
    }
}

/// Asserts that `skipped`, the parse of a file that skipped bodies, and
/// `full`, the parse of the whole of it, are stored alike but for what the
/// bodies give: the file's bodies, and its references, which `skipped`
/// has the bodies' of in what `FileBodies::text_references` finds.
#[cfg(debug_assertions)]
pub(super) fn assert_same_but_bodies(skipped: &ParsedFile, full: &ParsedFile) {
    let stored = |parsed: &ParsedFile| {
        let mut stored = CachedFile::of(parsed, 0);
        stored.references.clear();
        stored.declaration_references.clear();
        stored.bodies = FileBodies::default();
        stored.facts.has_skimmed_bodies = false;
        postcard::to_allocvec(&stored).expect("a parse is stored")
    };
    assert!(
        stored(skipped) == stored(full),
        "a file whose parse skipped its bodies is stored as the whole parse is, but for its bodies"
    );
    let mut references = skipped.references.clone();
    references.extend(skipped.bodies.text_references());
    assert!(
        references == full.references,
        "the references of a file's skipped bodies and of the rest are those of the whole parse"
    );
    assert!(
        skipped.declaration_references == full.declaration_references,
        "a skipped body refers to nothing a declaration is read from"
    );
    assert_eq!(skipped.bodies.spans(), full.bodies.spans());
    for span in full.bodies.spans() {
        assert_eq!(
            format!("{:?}", skipped.bodies.summary(span)),
            format!("{:?}", full.bodies.summary(span)),
            "a skipped body is read as the whole parse has it"
        );
        assert_eq!(
            format!("{:?}", skipped.bodies.init_flow(span)),
            format!("{:?}", full.bodies.init_flow(span)),
            "a skipped body's flow is as the whole parse has it"
        );
    }
}

/// Where a file's entry is.
#[derive(Clone)]
pub(super) struct Key {
    file: PathBuf,
}

/// The entry of a file with these `bytes`, named `path` and spelled as
/// `shown_path(root, path)` says (its anonymous classes carry that spelling),
/// parsed as `is_stub` says and with use sites indexed as `index_use_sites`
/// says; `None` when the cache is off.
pub(super) fn key(
    bytes: &[u8],
    path: &Path,
    root: Option<&Path>,
    is_stub: bool,
    index_use_sites: bool,
) -> Option<Key> {
    let directory = directory()?;
    let shown = super::shown_path(root, path);
    let mut hasher = twox_hash::XxHash3_128::new();
    hasher.write(bytes);
    // What follows is length-prefixed, so the bytes and the path can't
    // trade places.
    hasher.write(&(bytes.len() as u64).to_le_bytes());
    let path_bytes = path.as_os_str().as_encoded_bytes();
    hasher.write(path_bytes);
    hasher.write(&(path_bytes.len() as u64).to_le_bytes());
    let shown_bytes = shown.as_os_str().as_encoded_bytes();
    hasher.write(shown_bytes);
    hasher.write(&(shown_bytes.len() as u64).to_le_bytes());
    hasher.write(&[u8::from(is_stub), u8::from(index_use_sites)]);
    let digest = hasher.finish_128();
    let hex = format!("{digest:032x}");
    Some(Key {
        file: directory.join(&hex[..2]).join(format!("{hex}.bin")),
    })
}

/// The entry under `key`, when there is one that is whole.
pub(super) fn load(key: &Key) -> Option<CachedFile> {
    let mut file = fs::File::open(&key.file).ok()?;
    let modified = file.metadata().ok().and_then(|m| m.modified().ok());
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut bytes).ok()?;
    let payload = payload(&bytes)?;
    match postcard::from_bytes::<CachedFile>(payload) {
        Ok(cached) => {
            if modified
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > REFRESH_AFTER)
            {
                let _ = file.set_modified(SystemTime::now());
            }
            Some(cached)
        }
        Err(error) => {
            // The checksum held, so this is not a damaged entry: what was
            // stored can't be read back, which only a bug causes.
            debug_assert!(false, "a stored file no longer decodes: {error}");
            None
        }
    }
}

/// The payload of an entry that is whole.
fn payload(bytes: &[u8]) -> Option<&[u8]> {
    let header = bytes.get(..HEADER_LEN)?;
    if header[..8] != MAGIC {
        return None;
    }
    let len = u64::from_le_bytes(header[8..16].try_into().ok()?) as usize;
    let checksum = u32::from_le_bytes(header[16..20].try_into().ok()?);
    let payload = bytes.get(HEADER_LEN..)?;
    (payload.len() == len && crc32fast::hash(payload) == checksum).then_some(payload)
}

/// Stores `cached` under `key`, through a temporary file renamed into place
/// so a concurrent run never reads a partial entry. A failure leaves the
/// file to be parsed again next time. The caller has found that entries
/// can be stored (`can_store`), which made the shard directories.
pub(super) fn store(key: &Key, cached: &CachedFile) {
    let Ok(payload) = postcard::to_allocvec(cached) else {
        return;
    };
    // What is stored reads back as what was.
    #[cfg(debug_assertions)]
    {
        let decoded: CachedFile = postcard::from_bytes(&payload).expect("what is stored decodes");
        assert_eq!(
            postcard::to_allocvec(&decoded).ok().as_deref(),
            Some(payload.as_slice()),
            "a stored file does not read back as what was stored"
        );
    }
    static PROCESS: OnceLock<u32> = OnceLock::new();
    let temporary = key.file.with_extension(format!(
        "{}.{:?}.tmp",
        PROCESS.get_or_init(std::process::id),
        std::thread::current().id()
    ));
    let mut entry = Vec::with_capacity(HEADER_LEN + payload.len());
    entry.extend_from_slice(&MAGIC);
    entry.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    entry.extend_from_slice(&crc32fast::hash(&payload).to_le_bytes());
    entry.extend_from_slice(&payload);
    let written = fs::File::create(&temporary)
        .and_then(|mut file| file.write_all(&entry))
        .and_then(|()| fs::rename(&temporary, &key.file));
    if let Err(error) = written {
        static REPORTED: AtomicBool = AtomicBool::new(false);
        if !REPORTED.swap(true, Ordering::Relaxed) {
            eprintln!(
                "phpstan-rs: an entry of the per-file cache can't be written ({error}); the files it fails for are parsed again next run"
            );
        }
        let _ = fs::remove_file(&temporary);
        return;
    }
    STORED.fetch_add(1, Ordering::Relaxed);
}

/// How many builds' entries stay: a branch switch back to a recent build
/// finds its entries.
const KEPT_BUILDS: usize = 2;

/// Keeps the cache within its size: other builds' entries but the latest
/// are removed, then this build's oldest until it is under the cap. Called
/// once at the end of a load, and does nothing when the run stored no
/// entry.
pub(super) fn trim() {
    if STORED.swap(0, Ordering::Relaxed) == 0 {
        return;
    }
    let Some(directory) = directory() else {
        return;
    };
    if let Some(builds) = directory.parent() {
        trim_builds(builds, directory);
    }
    let cap = std::env::var("PHPSTAN_RS_FILE_CACHE_MB")
        .ok()
        .and_then(|text| text.parse::<u64>().ok())
        .unwrap_or(DEFAULT_CAP_MB)
        * 1024
        * 1024;
    trim_to(directory, cap);
}

/// Removes the oldest entries of `directory` until what is left is nine
/// tenths of `cap` bytes, when it is over `cap`; nine tenths, so the next
/// run does not trim again at once.
fn trim_to(directory: &Path, cap: u64) {
    let mut entries: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    for shard in fs::read_dir(directory).into_iter().flatten().flatten() {
        for entry in fs::read_dir(shard.path()).into_iter().flatten().flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            entries.push((modified, metadata.len(), entry.path()));
        }
    }
    let mut total: u64 = entries.iter().map(|entry| entry.1).sum();
    if total <= cap {
        return;
    }
    let target = cap / 10 * 9;
    entries.sort_by_key(|entry| entry.0);
    for (_, size, path) in entries {
        if total <= target {
            break;
        }
        if fs::remove_file(path).is_ok() {
            total -= size;
        }
    }
}

/// Removes all but the `KEPT_BUILDS` most recently written builds'
/// directories of `builds`, `current` among them.
fn trim_builds(builds: &Path, current: &Path) {
    let mut others: Vec<(SystemTime, PathBuf)> = fs::read_dir(builds)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path() != current)
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    others.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, path) in others.into_iter().skip(KEPT_BUILDS - 1) {
        let _ = fs::remove_dir_all(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "phpstan_rs_file_cache_{label}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_oldest_entries_go_first_and_only_when_over_the_cap() {
        let dir = scratch("trim");
        let shard = dir.join("ab");
        fs::create_dir_all(&shard).unwrap();
        let start = SystemTime::now() - Duration::from_secs(1000);
        for i in 0..10u64 {
            let path = shard.join(format!("{i}.bin"));
            fs::write(&path, [0u8; 100]).unwrap();
            let file = fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(start + Duration::from_secs(i * 10))
                .unwrap();
        }
        // Under the cap: nothing goes.
        trim_to(&dir, 1000);
        assert_eq!(fs::read_dir(&shard).unwrap().count(), 10);
        // Over it: down to nine tenths of the cap, the newest staying.
        trim_to(&dir, 500);
        let mut left: Vec<String> = fs::read_dir(&shard)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["6.bin", "7.bin", "8.bin", "9.bin"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_entry_is_whole_only_with_its_magic_length_and_checksum() {
        let payload = b"the payload";
        let mut entry = Vec::new();
        entry.extend_from_slice(&MAGIC);
        entry.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        entry.extend_from_slice(&crc32fast::hash(payload).to_le_bytes());
        entry.extend_from_slice(payload);
        assert_eq!(payload_of(&entry), Some(&payload[..]));
        assert_eq!(payload_of(&entry[..entry.len() - 1]), None);
        let mut damaged = entry.clone();
        *damaged.last_mut().unwrap() ^= 1;
        assert_eq!(payload_of(&damaged), None);
        let mut other = entry.clone();
        other[0] = b'X';
        assert_eq!(payload_of(&other), None);
        assert_eq!(payload_of(&[]), None);
    }

    fn payload_of(bytes: &[u8]) -> Option<&[u8]> {
        payload(bytes)
    }
}
