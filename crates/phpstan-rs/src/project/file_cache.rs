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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use rustc_hash::FxHashSet;
use syntax::FileFacts;
use syntax::lexer::LexWarning;
use syntax::name::Name;
use syntax::span::{LineIndex, Span};

use super::declared_classes::DeclaredClass;
use super::duplicate_class_sites::DuplicateClassSites;
use super::duplicate_fn_sites::DuplicateFunctionSites;
use super::source::ContentHash;
use super::use_sites::RefTarget;
use super::{Arc, ParsedFile};
use crate::deps::RefKey;
use crate::pest_config::PestFacts;
use crate::registry::{
    BuiltDecl, CacheReader, CacheUnreadable, CacheWriter, FileBodies, GlobalFacts,
};

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
            Some(
                crate::registry::cache_dir()?
                    .join("files")
                    .join(crate::registry::cache_key()),
            )
        })
        .as_deref()
}

/// The directory of this build's extractions of generated autoloaders, when
/// the cache is on.
pub(super) fn vendor_directory() -> Option<PathBuf> {
    directory()?;
    Some(
        crate::registry::cache_dir()?
            .join("vendor_autoload")
            .join(crate::registry::cache_key()),
    )
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
    built_decls: Vec<CachedBuiltDecl>,
    file_decls: Arc<crate::registry::FileDecls>,
    pest_facts: Option<PestFacts>,
    declared_classes: Vec<DeclaredClass>,
    duplicate_fn_sites: DuplicateFunctionSites,
    anonymous_classes: Vec<Name>,
    duplicate_class_sites: DuplicateClassSites,
    bodies: FileBodies,
    global_facts: Arc<GlobalFacts>,
    facts: FileFacts,
    line_index: Arc<LineIndex>,
    class_fn_refs: Option<Vec<(Span, RefTarget)>>,
    /// Runtime-only identity for the cache's single project-file origin.
    #[serde(skip, default)]
    origin_file_id: Option<crate::file_id::FileId>,
}

fn encode_cached_file(cached: &CachedFile) -> Result<Vec<u8>, String> {
    let mut writer = CacheWriter::new();
    let record = writer.encode(cached)?;
    writer.section("file", &record)?;
    Ok(writer.finish())
}

fn decode_cached_file(reader: &CacheReader) -> Result<CachedFile, String> {
    let range = reader.section("file")?;
    let names = reader.names().clone();
    syntax::name::with_cache_name_table(&names, || {
        postcard::from_bytes(&reader.buffer()[range]).map_err(|error| error.to_string())
    })
}

/// The per-declaration data the registry needs alongside the reflection
/// stored once in `file_decls`.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
enum CachedBuiltDecl {
    Class {
        name: Name,
        conditional: bool,
        guard: Option<crate::registry::GuardExpr>,
        namespace: Name,
    },
    Function {
        name: Name,
        conditional: bool,
        guard: Option<crate::registry::GuardExpr>,
        site: u32,
    },
    GlobalConst {
        name: Name,
        guard: Option<crate::registry::GuardExpr>,
        ty: types::Type,
    },
}

impl From<&BuiltDecl> for CachedBuiltDecl {
    fn from(declaration: &BuiltDecl) -> Self {
        match declaration {
            BuiltDecl::Class {
                name,
                conditional,
                guard,
                namespace,
                ..
            } => Self::Class {
                name: name.clone(),
                conditional: *conditional,
                guard: guard.clone(),
                namespace: namespace.clone(),
            },
            BuiltDecl::Function {
                name,
                conditional,
                guard,
                site,
                ..
            } => Self::Function {
                name: name.clone(),
                conditional: *conditional,
                guard: guard.clone(),
                site: *site,
            },
            BuiltDecl::GlobalConst { name, guard, ty } => Self::GlobalConst {
                name: name.clone(),
                guard: guard.clone(),
                ty: ty.clone(),
            },
        }
    }
}

impl CachedFile {
    /// What `parsed`, of a file `len` long, is stored as.
    pub(super) fn of(parsed: &ParsedFile, len: usize) -> CachedFile {
        let mut references: Vec<_> = parsed.references.iter().cloned().collect();
        let mut declaration_references: Vec<_> =
            parsed.declaration_references.iter().cloned().collect();
        syntax::references::sort_ref_keys(&mut references);
        syntax::references::sort_ref_keys(&mut declaration_references);
        CachedFile {
            len,
            lex_warnings: parsed.lex_warnings.clone(),
            functions: parsed.functions.clone(),
            classes: parsed.classes.clone(),
            constants: parsed.constants.clone(),
            references,
            declaration_references,
            built_decls: parsed
                .built_decls
                .iter()
                .map(CachedBuiltDecl::from)
                .collect(),
            file_decls: Arc::clone(&parsed.file_decls),
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
            origin_file_id: parsed.built_decls.iter().find_map(|decl| match decl {
                BuiltDecl::Class { info, .. } => info.provenance().file_id(),
                BuiltDecl::Function { sig, .. } => sig.provenance().file_id(),
                BuiltDecl::GlobalConst { .. } => None,
            }),
        }
    }

    /// The parse of the file at `path` this was stored from, without its
    /// tree and its ignore comments, which only an analysed file reads.
    pub(super) fn into_parsed(self, _path: &Path, content_hash: ContentHash) -> Option<ParsedFile> {
        debug_assert!(syntax::references::ref_keys_are_sorted(&self.references));
        debug_assert!(syntax::references::ref_keys_are_sorted(
            &self.declaration_references
        ));
        let references: FxHashSet<RefKey> = self.references.into_iter().collect();
        let declaration_references: FxHashSet<RefKey> =
            self.declaration_references.into_iter().collect();
        let mut classes = self
            .file_decls
            .classes
            .iter()
            .map(|(_, info)| Arc::clone(info));
        let mut functions = self
            .file_decls
            .functions
            .iter()
            .map(|(_, sig)| Arc::clone(sig));
        let built_decls: Option<Vec<_>> = self
            .built_decls
            .into_iter()
            .map(|declaration| {
                Some(match declaration {
                    CachedBuiltDecl::Class {
                        name,
                        conditional,
                        guard,
                        namespace,
                    } => BuiltDecl::Class {
                        name,
                        conditional,
                        guard,
                        info: classes.next()?,
                        namespace,
                    },
                    CachedBuiltDecl::Function {
                        name,
                        conditional,
                        guard,
                        site,
                    } => BuiltDecl::Function {
                        name,
                        conditional,
                        guard,
                        sig: functions.next()?,
                        site,
                    },
                    CachedBuiltDecl::GlobalConst { name, guard, ty } => {
                        BuiltDecl::GlobalConst { name, guard, ty }
                    }
                })
            })
            .collect();
        let built_decls = built_decls?;
        if classes.next().is_some() || functions.next().is_some() {
            return None;
        }
        drop(classes);
        drop(functions);
        Some(ParsedFile {
            tree: None,
            lex_warnings: self.lex_warnings,
            functions: self.functions,
            classes: self.classes,
            constants: self.constants,
            references,
            declaration_references,
            ignores: Default::default(),
            built_decls,
            file_decls: self.file_decls,
            pest_facts: self.pest_facts,
            declared_classes: self.declared_classes,
            duplicate_fn_sites: self.duplicate_fn_sites,
            anonymous_classes: self.anonymous_classes,
            duplicate_class_sites: self.duplicate_class_sites,
            bodies: self.bodies,
            global_facts: self.global_facts,
            facts: self.facts,
            line_index: self.line_index,
            class_fn_refs: self.class_fn_refs,
            owner: None,
            content_hash,
        })
    }
}

/// Where a file's entry is.
#[derive(Clone)]
pub(super) struct Key {
    file: PathBuf,
}

/// The entry of a file whose bytes hash to `content`, named `path`, parsed
/// as `is_stub` says and with use sites indexed as `index_use_sites` says;
/// `None` when the cache is off.
pub(super) fn key(
    content: ContentHash,
    path: &Path,
    is_stub: bool,
    index_use_sites: bool,
) -> Option<Key> {
    let directory = directory()?;
    let mut hasher = twox_hash::XxHash3_128::new();
    hasher.write(&content.to_le_bytes());
    // What follows is length-prefixed, so the hash and the path can't
    // trade places.
    let path_bytes = path.as_os_str().as_encoded_bytes();
    hasher.write(path_bytes);
    hasher.write(&(path_bytes.len() as u64).to_le_bytes());
    hasher.write(&[u8::from(is_stub), u8::from(index_use_sites)]);
    let digest = hasher.finish_128();
    let hex = format!("{digest:032x}");
    Some(Key {
        file: directory.join(&hex[..2]).join(format!("{hex}.bin")),
    })
}

/// The entry under `key`, when there is one that is whole.
pub(super) fn load(key: &Key, file_id: crate::file_id::FileId) -> Option<CachedFile> {
    let (mut reader, modified) = match CacheReader::open_modified(&key.file) {
        Ok(opened) => opened,
        Err(CacheUnreadable::Damaged) => return None,
        Err(CacheUnreadable::Inconsistent(reason)) => {
            eprintln!("phpstan-rs: the stored file cache is inconsistent ({reason}); reparsing");
            return None;
        }
    };
    reader.set_cache_context(file_id.index() as u64);
    match crate::registry::provenance::with_origin_file_id(file_id, || decode_cached_file(&reader))
    {
        Ok(cached) => {
            if modified
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > REFRESH_AFTER)
            {
                let _ =
                    fs::File::open(&key.file).and_then(|file| file.set_modified(SystemTime::now()));
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

/// Stores `cached` under `key`, through a temporary file renamed into place
/// so a concurrent run never reads a partial entry. A failure leaves the
/// file to be parsed again next time.
pub(super) fn store(key: &Key, cached: &CachedFile) {
    let Ok(container) = encode_cached_file(cached) else {
        return;
    };
    // What is stored reads back as what was.
    #[cfg(debug_assertions)]
    {
        let mut reader = CacheReader::new(container.clone()).expect("what is stored is whole");
        if let Some(file_id) = cached.origin_file_id {
            reader.set_cache_context(file_id.index() as u64);
        }
        let decode = || decode_cached_file(&reader).expect("what is stored decodes");
        let decoded = match cached.origin_file_id {
            Some(file_id) => crate::registry::provenance::with_origin_file_id(file_id, decode),
            None => decode(),
        };
        assert_eq!(
            encode_cached_file(&decoded).ok().as_deref(),
            Some(container.as_slice()),
            "a stored file does not read back as what was stored"
        );
    }
    let Some(directory) = key.file.parent() else {
        return;
    };
    if fs::create_dir_all(directory).is_err() {
        return;
    }
    let temporary = key.file.with_extension(format!(
        "{}.{:?}.tmp",
        std::process::id(),
        std::thread::current().id()
    ));
    let written = fs::File::create(&temporary)
        .and_then(|mut file| file.write_all(&container))
        .is_ok();
    if !written || fs::rename(&temporary, &key.file).is_err() {
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
    use crate::project::parse_file_recording;
    use syntax::name::NameCache;

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
    fn cached_file_round_trip_keeps_declaration_arenas() {
        let source = "<?php class Demo { public int $value; public const int MARK = 1; public function zebra(): void {} public function run(int $n): int {} public function alpha(): void {} } function helper(string $text): bool {} const GLOBAL_MARK = true;";
        let path = Path::new("cache-round-trip.php");
        let mut names = NameCache::default();
        let parsed = parse_file_recording(path, source, false, Some(&mut names), None).unwrap();
        let content_hash = parsed.content_hash;
        let cached = CachedFile::of(&parsed, source.len());
        let bytes = encode_cached_file(&cached).unwrap();
        let reader = CacheReader::new(bytes).unwrap();
        let cached = decode_cached_file(&reader).unwrap();
        let restored = cached.into_parsed(path, content_hash).unwrap();

        assert_eq!(restored.file_decls.classes.len(), 1);
        assert_eq!(restored.file_decls.functions.len(), 1);
        assert_eq!(restored.file_decls.members.len(), 3);
        assert_eq!(restored.file_decls.parameters.len(), 2);
        assert_eq!(restored.file_decls.properties.len(), 1);
        assert_eq!(restored.file_decls.constants.len(), 1);
        let BuiltDecl::Class { info, .. } = &restored.built_decls[0] else {
            panic!("the first declaration should be Demo");
        };
        let cached_class = restored.file_decls.classes.iter().next().unwrap().1;
        assert!(Arc::ptr_eq(info, cached_class));
        let class_layout = restored.file_decls.class_layouts.iter().next().unwrap().0;
        let members = restored.file_decls.members(class_layout);
        assert_eq!(members[0].name, "zebra");
        assert_eq!(
            restored
                .file_decls
                .member(class_layout, "RUN")
                .unwrap()
                .name,
            "run"
        );
        let member = &members[1];
        let cached_method = info.methods().shared_get(&member.name).unwrap();
        assert!(Arc::ptr_eq(&member.declaration, &cached_method));
        assert!(matches!(
            restored.built_decls.last(),
            Some(BuiltDecl::GlobalConst { name, .. }) if name == "GLOBAL_MARK"
        ));

        let mut registry = crate::registry::Registry::default();
        for declaration in &restored.built_decls {
            registry.insert_built_decl(declaration.clone(), true);
        }
        let (class_id, _, registered_class) = registry.class("Demo").unwrap();
        assert!(std::ptr::eq(
            registry.class_by_id(class_id).unwrap(),
            registered_class
        ));
        let (function_id, _, registered_function) = registry.function("helper").unwrap();
        assert!(std::ptr::eq(
            registry.function_declaration_by_id(function_id).unwrap(),
            registered_function
        ));

        let mut mismatched = CachedFile::of(&parsed, source.len());
        mismatched.built_decls.clear();
        assert!(mismatched.into_parsed(path, content_hash).is_none());
    }

    #[test]
    fn cached_project_origins_rebind_to_the_current_file_id() {
        let source = "<?php class Demo { function zebra(): void {} }";
        let path = Path::new("cache-origin-id.php");
        let old_id = crate::file_id::FileId::new(17);
        let new_id = crate::file_id::FileId::new(3);
        let parsed = super::super::parse_file_recording_with_file_id(
            path, source, false, None, None, old_id,
        )
        .unwrap();
        let cached = CachedFile::of(&parsed, source.len());
        let bytes = encode_cached_file(&cached).unwrap();
        let mut reader = CacheReader::new(bytes).unwrap();
        reader.set_cache_context(new_id.index() as u64);
        let cached = crate::registry::provenance::with_origin_file_id(new_id, || {
            decode_cached_file(&reader).unwrap()
        });
        let restored = cached.into_parsed(path, parsed.content_hash).unwrap();
        let BuiltDecl::Class { info, .. } = &restored.built_decls[0] else {
            panic!("the first declaration should be Demo");
        };
        assert_eq!(info.provenance().file_id(), Some(new_id));
        assert_eq!(
            info.methods().get("zebra").unwrap().provenance().file_id(),
            Some(new_id)
        );
    }
}
