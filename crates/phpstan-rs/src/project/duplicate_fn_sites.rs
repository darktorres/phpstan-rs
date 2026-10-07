use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::file_id::FileId;
use syntax::ast::{FunctionDecl, NodeArenas};
use syntax::name::Name;
use syntax::span::Span;

/// One `function` declaration's own site, name-keyed the same way
/// `Registry` is - but every unconditional
/// declaration of a name is kept (not just the winning one), since
/// `check_duplicate_function_declarations` needs to see every occurrence
/// to build its own full location list, exactly mirroring real PHPStan's
/// `DuplicateFunctionDeclarationRule`.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct FnDeclSite {
    pub(crate) path: PathBuf,
    pub(crate) span: Span,
    // The declaration's native shape at index time: each parameter's
    // native hint and the native return hint.
    pub(crate) param_hints: Vec<Option<String>>,
    pub(crate) return_hint: Option<String>,
}

/// Every function's own unconditional declaration site(s), across the
/// whole project - always populated, since `check_duplicate_function_
/// declarations` needs to run by default for `check-project`, matching
/// real PHPStan's own always-on `function.duplicate` rule. A conditional
/// declaration is never tracked (see `record`).
#[derive(Default, serde::Serialize, serde::Deserialize, Clone)]
pub(crate) struct DuplicateFunctionSites {
    sites: FxHashMap<String, Vec<FnDeclSite>>,
}

impl DuplicateFunctionSites {
    /// Records an unconditional function declaration; the caller drops a
    /// conditional one. `registry::built_declaration` still records a
    /// conditional one (with weaker "first unconditional wins"
    /// semantics), but real PHPStan's own rule has no such guard and is a
    /// known, disclosed false-positive source for the common
    /// `symfony/polyfill-*`-style `if (!function_exists('foo')) { function
    /// foo() {} }` idiom (two unrelated packages independently guarding
    /// the identical polyfill name) - a deliberate, stricter departure
    /// from literal real-PHPStan behavior, matching this project's own
    /// "favor false negatives over false positives" precedent.
    pub(crate) fn record(
        &mut self,
        fqn: String,
        path: &Path,
        f: &FunctionDecl,
        arenas: &NodeArenas,
    ) {
        self.sites.entry(fqn).or_default().push(FnDeclSite {
            path: path.to_path_buf(),
            span: f.name.span,
            param_hints: arenas
                .params(f.params.clone())
                .iter()
                .map(|p| p.type_hint.as_ref().map(|hint| hint.to_string_with(arenas)))
                .collect(),
            return_hint: f
                .return_type
                .as_ref()
                .map(|hint| hint.to_string_with(arenas)),
        });
    }

    /// Filters each name's own `Vec` down by `path` rather than a flat
    /// `.remove()` - one function name here can legitimately have sites in several
    /// different files at once, so removing `path`'s own contribution
    /// must leave every other file's site(s) for that same name intact.
    pub(crate) fn remove_declarations(&mut self, path: &Path, removed_functions: &[Name]) {
        for name in removed_functions {
            if let Some(sites) = self.sites.get_mut(name.as_str()) {
                sites.retain(|s| s.path != path);
                if sites.is_empty() {
                    self.sites.remove(name.as_str());
                }
            }
        }
    }

    /// The files declaring `name`.
    pub(crate) fn paths_of(&self, name: &str) -> impl Iterator<Item = &Path> {
        self.sites
            .get(name)
            .into_iter()
            .flatten()
            .map(|site| site.path.as_path())
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &Vec<FnDeclSite>)> {
        self.sites.iter()
    }

    /// Absorbs `other`'s own sites into `self` - correct regardless of
    /// which file's own per-file `DuplicateFunctionSites` `self`/`other`
    /// each hold, since every site already carries its own `path` and
    /// `record`'s own append-only `Vec` per name never depends on
    /// insertion order (unlike `Registry::insert_built_decl`'s FQN-
    /// collision resolution - there is no such collision here, every
    /// unconditional declaration is kept, not just a winner). Lets
    /// `check_project.rs`'s parallel parse stage build one
    /// `DuplicateFunctionSites` per file concurrently and merge them all
    /// sequentially afterward, instead of calling `index` once per file
    /// inside the sequential insert loop.
    pub(crate) fn merge(&mut self, other: DuplicateFunctionSites) {
        for (name, sites) in other.sites {
            self.sites.entry(name).or_default().extend(sites);
        }
    }
}

/// Project-wide duplicate declarations keyed by stable file ids. Parsed
/// cache entries keep paths because they are built before file numbering;
/// the project index stores ids and resolves paths only when reporting.
pub(crate) struct DuplicateFunctionIndex {
    shards: Vec<FxHashMap<String, Vec<IndexedFnDeclSite>>>,
}

const DUPLICATE_INDEX_SHARDS: usize = 32;

fn duplicate_shard(name: &str) -> usize {
    use std::hash::{Hash, Hasher};

    let mut hasher = rustc_hash::FxHasher::default();
    name.hash(&mut hasher);
    (hasher.finish() >> (u64::BITS - 5)) as usize
}

impl Default for DuplicateFunctionIndex {
    fn default() -> Self {
        Self {
            shards: (0..DUPLICATE_INDEX_SHARDS)
                .map(|_| FxHashMap::default())
                .collect(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct IndexedFnDeclSite {
    pub(crate) file: FileId,
    pub(crate) span: Span,
    pub(crate) param_hints: Vec<Option<String>>,
    pub(crate) return_hint: Option<String>,
}

impl DuplicateFunctionIndex {
    pub(crate) fn extend_files(&mut self, files: Vec<(FileId, DuplicateFunctionSites)>) {
        use rayon::prelude::*;

        let per_file: Vec<[Vec<(String, IndexedFnDeclSite)>; DUPLICATE_INDEX_SHARDS]> = files
            .into_par_iter()
            .map(|(file, sites)| {
                let mut buckets = std::array::from_fn(|_| Vec::new());
                for (name, sites) in sites.sites {
                    let bucket = &mut buckets[duplicate_shard(&name)];
                    bucket.extend(sites.into_iter().map(|site| {
                        (
                            name.clone(),
                            IndexedFnDeclSite {
                                file,
                                span: site.span,
                                param_hints: site.param_hints,
                                return_hint: site.return_hint,
                            },
                        )
                    }));
                }
                buckets
            })
            .collect();
        let mut buckets: [Vec<(String, IndexedFnDeclSite)>; DUPLICATE_INDEX_SHARDS] =
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

    pub(crate) fn merge_file(&mut self, file: FileId, other: DuplicateFunctionSites) {
        for (name, sites) in other.sites {
            self.shards[duplicate_shard(&name)]
                .entry(name)
                .or_default()
                .extend(sites.into_iter().map(|site| IndexedFnDeclSite {
                    file,
                    span: site.span,
                    param_hints: site.param_hints,
                    return_hint: site.return_hint,
                }));
        }
    }

    pub(crate) fn remove_file(&mut self, file: FileId, removed_functions: &[Name]) {
        for name in removed_functions {
            let shard = duplicate_shard(name.as_str());
            if let Some(sites) = self.shards[shard].get_mut(name.as_str()) {
                sites.retain(|site| site.file != file);
                if sites.is_empty() {
                    self.shards[shard].remove(name.as_str());
                }
            }
        }
    }

    pub(crate) fn paths_of(&self, name: &str) -> impl Iterator<Item = FileId> + '_ {
        self.shards[duplicate_shard(name)]
            .get(name)
            .into_iter()
            .flatten()
            .map(|site| site.file)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &Vec<IndexedFnDeclSite>)> {
        self.shards.iter().flat_map(|shard| shard.iter())
    }
}

impl syntax::canonical::Canonical for DuplicateFunctionIndex {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let sites: FxHashMap<_, _> = self
            .iter()
            .map(|(name, sites)| (name, syntax::canonical::AsSet(sites.as_slice())))
            .collect();
        p.node("DuplicateFunctionIndex", &[("sites", &sites)]);
    }
}

syntax::canonical_struct!(IndexedFnDeclSite {
    file,
    span,
    param_hints,
    return_hint,
});

syntax::canonical_struct!(FnDeclSite {
    path,
    span,
    param_hints,
    return_hint,
});

/// Each name's sites as a set: they are in the order the files were loaded
/// in.
impl syntax::canonical::Canonical for DuplicateFunctionSites {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let DuplicateFunctionSites { sites } = self;
        p.node(
            "DuplicateFunctionSites",
            &[("sites", &syntax::canonical::MapOfSets(sites))],
        );
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;
    use syntax::span::Span;

    fn sites(path: &str, count: u32) -> DuplicateFunctionSites {
        DuplicateFunctionSites {
            sites: FxHashMap::from_iter([(
                "shared".to_owned(),
                (0..count)
                    .map(|offset| FnDeclSite {
                        path: PathBuf::from(path),
                        span: Span::new(offset, offset + 1),
                        param_hints: vec![],
                        return_hint: None,
                    })
                    .collect(),
            )]),
        }
    }

    #[test]
    fn parallel_index_build_preserves_file_sites_and_incremental_removal() {
        let first = FileId::new(2);
        let second = FileId::new(9);
        let mut index = DuplicateFunctionIndex::default();
        index.extend_files(vec![
            (first, sites("first.php", 2)),
            (second, sites("second.php", 1)),
        ]);
        let mut files: Vec<_> = index.paths_of("shared").collect();
        files.sort_unstable();
        assert_eq!(files, vec![first, first, second]);

        index.remove_file(first, &[Name::from("shared")]);
        assert_eq!(index.paths_of("shared").collect::<Vec<_>>(), vec![second]);
    }
}
