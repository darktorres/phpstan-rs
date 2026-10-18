//! The project's own tables a load fills from its files: the use sites, the
//! duplicate declaration sites, and the reverse indexes of references
//! (`dependents`, `signature_dependents`). `Project::insert_file_tables` merges
//! the entries of every file of a load in one pass. The tables are split into
//! shards by key (`shard::shard_of_key`), and each shard applies the entries
//! that route to it in the order of the files, so each key ends as the serial
//! inserts of the files leave it. A use site routes twice, by its target for
//! the target map and by its file's number for the file map, so each map of
//! each shard holds the keys that route to that shard.

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::span::Span;

use super::duplicate_class_sites::{ClassDeclSite, DuplicateClassSites};
use super::duplicate_fn_sites::{DuplicateFunctionSites, FnDeclSite};
use super::use_sites::{RefTarget, UseSiteIndex};
use super::{ParsedFile, Project, register_reference};
use crate::deps::RefKey;
use crate::file_id::FileId;
use crate::shard::{SHARDS, join_map, merge_by_shard, shard_of_key, split_map};

/// The entries a parsed file takes out for the merge (`ParsedFile::take_entries`).
struct FileEntries {
    class_fn_refs: Vec<(Span, RefTarget)>,
    duplicate_fn_sites: Vec<(String, FnDeclSite)>,
    duplicate_class_sites: Vec<(String, ClassDeclSite)>,
}

/// A file's entries, and the reference keys of its parse, for one merge.
struct FileTables<'a> {
    id: FileId,
    entries: FileEntries,
    references: &'a FxHashSet<RefKey>,
    declaration_references: &'a FxHashSet<RefKey>,
}

/// The project's own tables, split into shards for a merge.
struct ProjectShard {
    use_sites: UseSiteIndex,
    duplicate_fn_sites: DuplicateFunctionSites,
    duplicate_class_sites: DuplicateClassSites,
    dependents: FxHashMap<RefKey, FxHashSet<FileId>>,
    signature_dependents: FxHashMap<RefKey, FxHashSet<FileId>>,
}

/// The work one chunk of files makes for one shard, in the files' order.
#[derive(Default)]
struct ChunkWork<'a> {
    /// The use sites whose target the shard holds, each with its file.
    target_sites: Vec<(RefTarget, FileId, Span)>,
    /// The use sites whose file the shard holds, each with its target.
    file_sites: Vec<(FileId, RefTarget, Span)>,
    duplicate_fn_sites: Vec<(String, FileId, FnDeclSite)>,
    duplicate_class_sites: Vec<(String, FileId, ClassDeclSite)>,
    dependents: Vec<(&'a RefKey, FileId)>,
    signature_dependents: Vec<(&'a RefKey, FileId)>,
}

impl ParsedFile {
    /// Takes out the entries the file adds to the project's tables. The
    /// references stay in the parse, which `insert_numbered_file` keeps with
    /// the file.
    fn take_entries(&mut self) -> FileEntries {
        debug_assert!(
            self.class_fn_refs.is_some() || self.ast.is_none(),
            "a parse with a tree has its use sites indexed before its insert"
        );
        FileEntries {
            class_fn_refs: self.class_fn_refs.take().unwrap_or_default(),
            duplicate_fn_sites: std::mem::take(&mut self.duplicate_fn_sites),
            duplicate_class_sites: std::mem::take(&mut self.duplicate_class_sites),
        }
    }
}

impl Project {
    /// Inserts the entries of `files`, each with its id and parse, in the
    /// order given. Each table ends as the serial inserts of the files in
    /// turn leave it. A file's entries are taken out of its parse, and its
    /// references are borrowed from it, so `insert_numbered_file` takes the
    /// parse after this. An empty batch changes nothing.
    pub(crate) fn insert_file_tables<'a>(
        &mut self,
        pool: &rayon::ThreadPool,
        files: impl IntoIterator<Item = (FileId, &'a mut ParsedFile)>,
    ) {
        let tables: Vec<FileTables<'a>> = files
            .into_iter()
            .map(|(id, parsed)| {
                let entries = parsed.take_entries();
                let parsed: &'a ParsedFile = parsed;
                FileTables {
                    id,
                    entries,
                    references: &parsed.references,
                    declaration_references: &parsed.declaration_references,
                }
            })
            .collect();
        if tables.is_empty() {
            return;
        }
        let shards = self.split_project_tables();
        let shards = merge_by_shard(pool, shards, tables, bucket, ProjectShard::apply);
        self.join_project_tables(shards);
    }

    /// Takes the project's own tables apart into shards, for
    /// `join_project_tables`.
    fn split_project_tables(&mut self) -> Vec<ProjectShard> {
        let mut use_sites = std::mem::take(&mut self.use_sites)
            .split_shards()
            .into_iter();
        let mut duplicate_fn_sites = std::mem::take(&mut self.duplicate_fn_sites)
            .split_shards()
            .into_iter();
        let mut duplicate_class_sites = std::mem::take(&mut self.duplicate_class_sites)
            .split_shards()
            .into_iter();
        let mut dependents = split_map(std::mem::take(&mut self.dependents)).into_iter();
        let mut signature_dependents =
            split_map(std::mem::take(&mut self.signature_dependents)).into_iter();
        (0..SHARDS)
            .map(|_| ProjectShard {
                use_sites: use_sites.next().expect("a part for each shard"),
                duplicate_fn_sites: duplicate_fn_sites.next().expect("a part for each shard"),
                duplicate_class_sites: duplicate_class_sites.next().expect("a part for each shard"),
                dependents: dependents.next().expect("a part for each shard"),
                signature_dependents: signature_dependents.next().expect("a part for each shard"),
            })
            .collect()
    }

    /// Joins the shards of `split_project_tables` back into the tables.
    fn join_project_tables(&mut self, shards: Vec<ProjectShard>) {
        let mut use_sites = Vec::with_capacity(SHARDS);
        let mut duplicate_fn_sites = Vec::with_capacity(SHARDS);
        let mut duplicate_class_sites = Vec::with_capacity(SHARDS);
        let mut dependents = Vec::with_capacity(SHARDS);
        let mut signature_dependents = Vec::with_capacity(SHARDS);
        for shard in shards {
            use_sites.push(shard.use_sites);
            duplicate_fn_sites.push(shard.duplicate_fn_sites);
            duplicate_class_sites.push(shard.duplicate_class_sites);
            dependents.push(shard.dependents);
            signature_dependents.push(shard.signature_dependents);
        }
        self.use_sites = UseSiteIndex::join_shards(use_sites);
        self.duplicate_fn_sites = DuplicateFunctionSites::join_shards(duplicate_fn_sites);
        self.duplicate_class_sites = DuplicateClassSites::join_shards(duplicate_class_sites);
        self.dependents = join_map(dependents);
        self.signature_dependents = join_map(signature_dependents);
    }
}

impl ProjectShard {
    /// Applies one chunk's work to the shard's tables.
    fn apply(&mut self, work: ChunkWork<'_>) {
        for (target, file, span) in work.target_sites {
            self.use_sites.add_target_site(target, file, span);
        }
        for (file, target, span) in work.file_sites {
            self.use_sites.add_file_site(file, target, span);
        }
        for (name, file, site) in work.duplicate_fn_sites {
            self.duplicate_fn_sites.add(name, file, site);
        }
        for (name, file, site) in work.duplicate_class_sites {
            self.duplicate_class_sites.add(name, file, site);
        }
        for (name, file) in work.dependents {
            register_reference(&mut self.dependents, file, name);
        }
        for (name, file) in work.signature_dependents {
            register_reference(&mut self.signature_dependents, file, name);
        }
    }
}

/// The work one chunk of files makes for each shard, in the files' order.
fn bucket<'a>(files: Vec<FileTables<'a>>) -> Vec<ChunkWork<'a>> {
    let mut work: Vec<ChunkWork<'a>> = (0..SHARDS).map(|_| ChunkWork::default()).collect();
    for file in files {
        let FileTables {
            id,
            entries,
            references,
            declaration_references,
        } = file;
        let FileEntries {
            class_fn_refs,
            duplicate_fn_sites,
            duplicate_class_sites,
        } = entries;
        let file_shard = shard_of_key(&id);
        for (span, target) in class_fn_refs {
            let target_shard = shard_of_key(&target);
            work[target_shard]
                .target_sites
                .push((target.clone(), id, span));
            work[file_shard].file_sites.push((id, target, span));
        }
        for (name, site) in duplicate_fn_sites {
            work[shard_of_key(&name)]
                .duplicate_fn_sites
                .push((name, id, site));
        }
        for (name, site) in duplicate_class_sites {
            work[shard_of_key(&name)]
                .duplicate_class_sites
                .push((name, id, site));
        }
        for name in references {
            work[shard_of_key(name)].dependents.push((name, id));
        }
        for name in declaration_references {
            work[shard_of_key(name)]
                .signature_dependents
                .push((name, id));
        }
    }
    work
}
