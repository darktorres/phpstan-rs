//! The declaration tables of a batch of files, merged shard by shard.
//!
//! An insert of one declaration reads and writes the entries of the name it
//! declares, and reads the built-in stub classes, which no insert writes
//! (`DeclTables::insert`). The names are split into `SHARDS` parts by
//! `shard_of_hash`, each part owned by one job, so no two jobs share a table.
//! Within a shard the declarations are applied in the order of the files,
//! which is the order `insert_built_decl` applies them in, so each name ends
//! as the sequential inserts leave it.

use std::path::{Path, PathBuf};

use syntax::name::Name;
use types::{PhpVersions, Type};

use super::index::{BuiltDecl, DeclTables, for_each_assert_tagged_name};
use super::{
    ChangedNames, ClassReflection, FunctionTable, NameMap, NameSet, NameTable, Registry,
    is_phpstan_namespace,
};
use crate::shard::{SHARDS, merge_by_shard, shard_of_hash};

/// One file's declarations, for `DeclMerge::insert`.
pub(crate) struct DeclaredFile<'a> {
    /// The declarations `build_declarations` built for the file, in order.
    pub(crate) decls: &'a [BuiltDecl],
    /// Whether the file is analysed (see `DeclTables::insert`).
    pub(crate) analyzed: bool,
    /// The path a PHPStan class declared in the file is recorded under.
    pub(crate) shown: &'a Path,
}

/// The declaration tables, split into shards for a merge (see
/// `Registry::merge_declarations`).
pub(crate) struct DeclMerge<'a> {
    shards: Vec<DeclShard>,
    built_in_stub_classes: &'a NameTable<()>,
    php_versions: Option<PhpVersions>,
    recording: bool,
    pool: &'a rayon::ThreadPool,
}

/// The tables of one shard. Each holds the entries of the names that
/// `shard_of_hash` routes to the shard.
struct DeclShard {
    classes: NameTable<ClassReflection>,
    functions: FunctionTable,
    global_constants: NameMap<Type>,
    unconditionally_declared_classes: NameSet,
    unconditionally_declared_functions: NameSet,
    stub_provided_functions: NameSet,
    function_decl_sites: NameMap<Option<u32>>,
    names_with_assert_tags: NameSet,
    phpstan_class_files: NameMap<PathBuf>,
    /// The changed names this shard recorded, when the registry records them.
    changed: ChangedNames,
}

/// The work one chunk of files makes for one shard, in the files' order.
#[derive(Default)]
struct ShardWork<'a> {
    /// Each declaration that takes effect, with its file's analyzed flag.
    decls: Vec<(BuiltDecl, bool)>,
    /// The names to record in `names_with_assert_tags`.
    asserts: Vec<Name>,
    /// The PHPStan class names to record, each with the path it is recorded under.
    phpstan: Vec<(Name, &'a Path)>,
}

impl Registry {
    /// Runs `f` with the declaration tables split into shards, and joins them
    /// back once `f` returns. `f` reaches the tables only through the
    /// `DeclMerge` it is given, so nothing reads them while they are split.
    pub(crate) fn merge_declarations<R>(
        &mut self,
        pool: &rayon::ThreadPool,
        f: impl FnOnce(&mut DeclMerge<'_>) -> R,
    ) -> R {
        let shards = self.split_decl_tables();
        let mut merge = DeclMerge {
            shards,
            built_in_stub_classes: &self.built_in_stub_classes,
            php_versions: self.php_versions,
            recording: self.raw.is_some(),
            pool,
        };
        let result = f(&mut merge);
        let DeclMerge { shards, .. } = merge;
        self.join_decl_tables(shards);
        result
    }

    /// Takes the declaration tables apart into shards, for `join_decl_tables`.
    fn split_decl_tables(&mut self) -> Vec<DeclShard> {
        let mut classes = std::mem::take(&mut self.classes).split_shards().into_iter();
        let mut functions = std::mem::take(&mut self.functions)
            .split_shards()
            .into_iter();
        let mut global_constants = std::mem::take(&mut self.global_constants)
            .split_shards()
            .into_iter();
        let mut unconditionally_declared_classes =
            std::mem::take(&mut self.unconditionally_declared_classes)
                .split_shards()
                .into_iter();
        let mut unconditionally_declared_functions =
            std::mem::take(&mut self.unconditionally_declared_functions)
                .split_shards()
                .into_iter();
        let mut stub_provided_functions = std::mem::take(&mut self.stub_provided_functions)
            .split_shards()
            .into_iter();
        let mut function_decl_sites = std::mem::take(&mut self.function_decl_sites)
            .split_shards()
            .into_iter();
        let mut names_with_assert_tags = std::mem::take(&mut self.names_with_assert_tags)
            .split_shards()
            .into_iter();
        let mut phpstan_class_files = std::mem::take(&mut self.phpstan_class_files)
            .split_shards()
            .into_iter();
        (0..SHARDS)
            .map(|_| DeclShard {
                classes: classes.next().expect("a part for each shard"),
                functions: functions.next().expect("a part for each shard"),
                global_constants: global_constants.next().expect("a part for each shard"),
                unconditionally_declared_classes: unconditionally_declared_classes
                    .next()
                    .expect("a part for each shard"),
                unconditionally_declared_functions: unconditionally_declared_functions
                    .next()
                    .expect("a part for each shard"),
                stub_provided_functions: stub_provided_functions
                    .next()
                    .expect("a part for each shard"),
                function_decl_sites: function_decl_sites.next().expect("a part for each shard"),
                names_with_assert_tags: names_with_assert_tags
                    .next()
                    .expect("a part for each shard"),
                phpstan_class_files: phpstan_class_files.next().expect("a part for each shard"),
                changed: ChangedNames::default(),
            })
            .collect()
    }

    /// Joins the shards of `split_decl_tables` back into the tables, and adds
    /// the names they recorded as changed.
    fn join_decl_tables(&mut self, shards: Vec<DeclShard>) {
        let mut classes = Vec::with_capacity(SHARDS);
        let mut functions = Vec::with_capacity(SHARDS);
        let mut global_constants = Vec::with_capacity(SHARDS);
        let mut unconditionally_declared_classes = Vec::with_capacity(SHARDS);
        let mut unconditionally_declared_functions = Vec::with_capacity(SHARDS);
        let mut stub_provided_functions = Vec::with_capacity(SHARDS);
        let mut function_decl_sites = Vec::with_capacity(SHARDS);
        let mut names_with_assert_tags = Vec::with_capacity(SHARDS);
        let mut phpstan_class_files = Vec::with_capacity(SHARDS);
        for shard in shards {
            classes.push(shard.classes);
            functions.push(shard.functions);
            global_constants.push(shard.global_constants);
            unconditionally_declared_classes.push(shard.unconditionally_declared_classes);
            unconditionally_declared_functions.push(shard.unconditionally_declared_functions);
            stub_provided_functions.push(shard.stub_provided_functions);
            function_decl_sites.push(shard.function_decl_sites);
            names_with_assert_tags.push(shard.names_with_assert_tags);
            phpstan_class_files.push(shard.phpstan_class_files);
            self.changed.merge(shard.changed);
        }
        self.classes = NameTable::join_shards(classes);
        self.functions = NameTable::join_shards(functions);
        self.global_constants = NameMap::join_shards(global_constants);
        self.unconditionally_declared_classes =
            NameSet::join_shards(unconditionally_declared_classes);
        self.unconditionally_declared_functions =
            NameSet::join_shards(unconditionally_declared_functions);
        self.stub_provided_functions = NameSet::join_shards(stub_provided_functions);
        self.function_decl_sites = NameMap::join_shards(function_decl_sites);
        self.names_with_assert_tags = NameSet::join_shards(names_with_assert_tags);
        self.phpstan_class_files = NameMap::join_shards(phpstan_class_files);
    }
}

impl DeclMerge<'_> {
    /// Applies the declarations of `files`, in order, after those of any earlier
    /// call. The files are bucketed by shard in parallel, and then each shard
    /// applies its work in parallel.
    pub(crate) fn insert(&mut self, files: Vec<DeclaredFile<'_>>) {
        let versions = self.php_versions;
        let stubs = self.built_in_stub_classes;
        let recording = self.recording;
        let shards = std::mem::take(&mut self.shards);
        self.shards = merge_by_shard(
            self.pool,
            shards,
            files,
            |chunk| bucket(chunk, versions),
            |shard, work| shard.apply(work, stubs, recording),
        );
    }
}

impl DeclShard {
    /// Applies one chunk's work to the shard's tables.
    fn apply(
        &mut self,
        work: ShardWork<'_>,
        built_in_stub_classes: &NameTable<()>,
        recording: bool,
    ) {
        for name in work.asserts {
            self.names_with_assert_tags.insert(name);
        }
        for (name, path) in work.phpstan {
            self.phpstan_class_files.insert(name, path.to_path_buf());
        }
        let mut tables = DeclTables {
            classes: &mut self.classes,
            functions: &mut self.functions,
            global_constants: &mut self.global_constants,
            built_in_stub_classes,
            unconditionally_declared_classes: &mut self.unconditionally_declared_classes,
            unconditionally_declared_functions: &mut self.unconditionally_declared_functions,
            stub_provided_functions: &mut self.stub_provided_functions,
            function_decl_sites: &mut self.function_decl_sites,
            changed: recording.then_some(&mut self.changed),
        };
        for (decl, analyzed) in work.decls {
            tables.insert(decl, analyzed);
        }
    }
}

/// The work one chunk of files makes for each shard, in the files' order.
fn bucket<'a>(files: Vec<DeclaredFile<'a>>, versions: Option<PhpVersions>) -> Vec<ShardWork<'a>> {
    let mut work: Vec<ShardWork<'a>> = (0..SHARDS).map(|_| ShardWork::default()).collect();
    for file in files {
        for decl in file.decls {
            // A PHPStan class is recorded whether or not its declaration takes
            // effect, as the sequential path records it before the insert.
            if let BuiltDecl::Class { name, .. } = decl
                && is_phpstan_namespace(name)
            {
                work[shard_of(name)]
                    .phpstan
                    .push((name.clone(), file.shown));
            }
            let Some(decl) = decl.clone().in_effect(versions) else {
                continue;
            };
            for_each_assert_tagged_name(&decl, |name| work[shard_of(&name)].asserts.push(name));
            work[shard_of(decl.name())]
                .decls
                .push((decl, file.analyzed));
        }
    }
    work
}

/// The shard a name is kept in.
fn shard_of(name: &Name) -> usize {
    shard_of_hash(name.folded_hash())
}
