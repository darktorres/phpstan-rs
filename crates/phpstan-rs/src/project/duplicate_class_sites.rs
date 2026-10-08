use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::file_id::FileId;
use syntax::ast::ClassKind;
use syntax::name::Name;
use syntax::span::Span;

/// One class-like (`class`/`interface`/`trait`/`enum`) declaration's own
/// site, name-keyed the same way `Registry` is - mirrors `project::duplicate_fn_sites::FnDeclSite`
/// exactly, with one new field this side needs and the function side
/// doesn't: `is_stub`, whether this site's own file falls under a
/// configured `stub_paths` entry (see `project::duplicate_classes`'s own
/// doc comment for why that distinction exists at all - a designated
/// override for a real corpus-vendored stub file, e.g. `Imagick.stub.
/// php`, must never itself be flagged, and must never count toward
/// flagging the real declaration it's overriding).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ClassDeclSite {
    pub(crate) path: PathBuf,
    pub(crate) span: Span,
    pub(crate) kind: ClassKind,
    pub(crate) is_stub: bool,
}

/// Every class-like's own unconditional declaration site(s),
/// across the whole project - a straight port of `DuplicateFunctionSites`
/// to class-likes, plus the `is_stub` field above. Gated behind
/// `RulesSettings::check_duplicate_class_declarations` (opt-in, unlike
/// `DuplicateFunctionSites`, which has no such gate - see that field's own
/// doc comment for why this one needs it and the function-side precedent
/// didn't).
#[derive(Default, serde::Serialize, serde::Deserialize, Clone)]
pub(crate) struct DuplicateClassSites {
    #[serde(serialize_with = "syntax::canonical::serialize_in_key_order")]
    sites: FxHashMap<String, Vec<ClassDeclSite>>,
}

impl DuplicateClassSites {
    /// Records an unconditional class-like declaration; the caller drops
    /// a conditional one, as `DuplicateFunctionSites::record` does, for
    /// the same false-negative-favoring reason (a real, if rarer, analogue
    /// of the `function_exists`-guarded polyfill idiom: `if (!class_exists(
    /// Foo::class)) { class Foo {} }`). `is_stub` is the file's
    /// provenance.
    pub(crate) fn record(
        &mut self,
        fqn: String,
        path: &Path,
        span: Span,
        kind: ClassKind,
        is_stub: bool,
    ) {
        self.sites.entry(fqn).or_default().push(ClassDeclSite {
            path: path.to_path_buf(),
            span,
            kind,
            is_stub,
        });
    }
}

/// Project-wide class declaration sites addressed by FileId. The cached,
/// parse-local `DuplicateClassSites` above retains paths until numbering.
pub(crate) struct DuplicateClassIndex {
    shards: Vec<FxHashMap<String, Vec<IndexedClassDeclSite>>>,
}

const DUPLICATE_CLASS_INDEX_SHARDS: usize = 32;

fn duplicate_class_shard(name: &str) -> usize {
    use std::hash::{Hash, Hasher};

    let mut hasher = rustc_hash::FxHasher::default();
    name.hash(&mut hasher);
    (hasher.finish() >> (u64::BITS - 5)) as usize
}

impl Default for DuplicateClassIndex {
    fn default() -> Self {
        Self {
            shards: (0..DUPLICATE_CLASS_INDEX_SHARDS)
                .map(|_| FxHashMap::default())
                .collect(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct IndexedClassDeclSite {
    pub(crate) file: FileId,
    pub(crate) span: Span,
    pub(crate) kind: ClassKind,
    pub(crate) is_stub: bool,
}

impl DuplicateClassIndex {
    pub(crate) fn extend_files(&mut self, files: Vec<(FileId, DuplicateClassSites)>) {
        use rayon::prelude::*;

        let per_file: Vec<[Vec<(String, IndexedClassDeclSite)>; DUPLICATE_CLASS_INDEX_SHARDS]> =
            files
                .into_par_iter()
                .map(|(file, sites)| {
                    let mut buckets = std::array::from_fn(|_| Vec::new());
                    for (name, sites) in sites.sites {
                        let bucket = &mut buckets[duplicate_class_shard(&name)];
                        bucket.extend(sites.into_iter().map(|site| {
                            (
                                name.clone(),
                                IndexedClassDeclSite {
                                    file,
                                    span: site.span,
                                    kind: site.kind,
                                    is_stub: site.is_stub,
                                },
                            )
                        }));
                    }
                    buckets
                })
                .collect();
        let mut buckets: [Vec<(String, IndexedClassDeclSite)>; DUPLICATE_CLASS_INDEX_SHARDS] =
            std::array::from_fn(|_| Vec::new());
        for mut file_buckets in per_file {
            for (bucket, file_bucket) in buckets.iter_mut().zip(&mut file_buckets) {
                bucket.append(file_bucket);
            }
        }
        self.shards
            .par_iter_mut()
            .zip(buckets.into_par_iter())
            .for_each(|(shard, entries)| {
                shard.reserve(entries.len());
                for (name, site) in entries {
                    shard.entry(name).or_insert_with(Vec::new).push(site);
                }
            })
    }

    pub(crate) fn merge_file(&mut self, file: FileId, other: DuplicateClassSites) {
        for (name, sites) in other.sites {
            self.shards[duplicate_class_shard(&name)]
                .entry(name)
                .or_default()
                .extend(sites.into_iter().map(|site| IndexedClassDeclSite {
                    file,
                    span: site.span,
                    kind: site.kind,
                    is_stub: site.is_stub,
                }));
        }
    }

    pub(crate) fn remove_file(&mut self, file: FileId, removed_classes: &[Name]) {
        for name in removed_classes {
            let shard = duplicate_class_shard(name.as_str());
            if let Some(sites) = self.shards[shard].get_mut(name.as_str()) {
                sites.retain(|site| site.file != file);
                if sites.is_empty() {
                    self.shards[shard].remove(name.as_str());
                }
            }
        }
    }

    pub(crate) fn paths_of(&self, name: &str) -> impl Iterator<Item = FileId> + '_ {
        self.shards[duplicate_class_shard(name)]
            .get(name)
            .into_iter()
            .flatten()
            .map(|site| site.file)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &Vec<IndexedClassDeclSite>)> {
        self.shards.iter().flat_map(|shard| shard.iter())
    }
}

impl syntax::canonical::Canonical for DuplicateClassIndex {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let sites: FxHashMap<_, _> = self
            .iter()
            .map(|(name, sites)| (name, syntax::canonical::AsSet(sites.as_slice())))
            .collect();
        p.node("DuplicateClassIndex", &[("sites", &sites)]);
    }
}

syntax::canonical_struct!(IndexedClassDeclSite {
    file,
    span,
    kind,
    is_stub,
});

syntax::canonical_struct!(ClassDeclSite {
    path,
    span,
    kind,
    is_stub,
});

/// Each name's sites as a set: they are in the order the files were loaded
/// in.
impl syntax::canonical::Canonical for DuplicateClassSites {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let DuplicateClassSites { sites } = self;
        p.node(
            "DuplicateClassSites",
            &[("sites", &syntax::canonical::MapOfSets(sites))],
        );
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;
    use syntax::span::Span;

    fn sites(path: &str) -> DuplicateClassSites {
        DuplicateClassSites {
            sites: FxHashMap::from_iter([(
                "Shared".to_owned(),
                vec![ClassDeclSite {
                    path: PathBuf::from(path),
                    span: Span::new(1, 2),
                    kind: ClassKind::Class,
                    is_stub: false,
                }],
            )]),
        }
    }

    #[test]
    fn parallel_index_build_preserves_sites_and_incremental_removal() {
        let first = FileId::new(3);
        let second = FileId::new(7);
        let mut index = DuplicateClassIndex::default();
        index.extend_files(vec![
            (first, sites("first.php")),
            (second, sites("second.php")),
        ]);
        let mut files: Vec<_> = index.paths_of("Shared").collect();
        files.sort_unstable();
        assert_eq!(files, vec![first, second]);

        index.remove_file(first, &[Name::from("Shared")]);
        assert_eq!(index.paths_of("Shared").collect::<Vec<_>>(), vec![second]);
    }
}
