use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use syntax::name::{NameMap, NameSet};

use crate::checker::constant_resolver::ConstantResolver;
use crate::checker::include_lookup::{IncludeEnv, IncludeSettings, LookupChanges};
use crate::checker::rule_level_helper::RuleLevelHelper;
use crate::checker::settings::CheckSettings;
use crate::checker::{self, Checker, PropertyAssignFacts};
use crate::deps;
use crate::file_id::{FileId, FileIdSet};
use crate::ignore_comments;
use crate::pest_config::{PestConfig, PestFacts};
use crate::registry::scope::Scope;
use crate::registry::stub_set::Stubs;
use crate::registry::stubs::Corpus;
use crate::registry::{
    BodySource, BuiltDecl, DeclKind, DeclaredFile, FileBodies, GlobalFacts, Origin, Provenance,
    Registry,
};
#[cfg(debug_assertions)]
use crate::registry::{Keep, build_declarations_where};
use declared_classes::DeclaredClass;
use diagnostics::{self, Diagnostic, DiagnosticBuilder};
use syntax::FileFacts;
use syntax::ast::{Stmt, StmtKind};
use syntax::declarations::Declared;
use syntax::lexer::LexWarning;
use syntax::parser::ParseError;
use syntax::span::{LineIndex, Span};
use types::Name;
use whole_program::renamed_named_arguments::NamedArgumentCallFactsWithFile;

mod architecture;
mod banned;
mod class_shape;
mod composer_hygiene;
mod configured_path;
pub(crate) mod declared_classes;
pub(crate) mod duplicate_class_sites;
mod duplicate_classes;
pub(crate) mod duplicate_fn_sites;
mod duplicate_functions;
mod file_cache;
mod file_declarations;
mod file_table;
mod file_tables;
mod mock_hygiene;
mod path_spelling;
pub mod session;
pub mod source;
pub(crate) mod use_sites;
pub(crate) mod walk;
mod whole_program;

use duplicate_class_sites::{ClassDeclSite, DuplicateClassSites};
use duplicate_fn_sites::{DuplicateFunctionSites, FnDeclSite};
use use_sites::UseSiteIndex;

/// The files `Project::walk_affected` has reached, with what it has walked,
/// so that it can go on from more changed keys without starting again.
#[derive(Default)]
struct AffectedWalk {
    affected: FxHashSet<FileId>,
    seen: FxHashSet<deps::RefKey>,
    stub_files: StubFileSet,
}

/// A set of stub files, by number, which is added to a file's closure at a
/// time.
#[derive(Default)]
struct StubFileSet {
    words: Vec<u64>,
}

impl StubFileSet {
    /// Adds every stub file `file` leads to (`Corpus::stub_file_closure`),
    /// calling `visit` with each that was not in the set.
    fn add_closure(&mut self, corpus: &Corpus, file: u32, mut visit: impl FnMut(u32)) {
        let closure = corpus.stub_file_closure(file);
        if self.words.len() < closure.len() {
            self.words.resize(closure.len(), 0);
        }
        for (at, (have, add)) in self.words.iter_mut().zip(closure).enumerate() {
            let mut new = add & !*have;
            *have |= new;
            while new != 0 {
                let bit = new.trailing_zeros();
                new &= new - 1;
                visit((at * 64) as u32 + bit);
            }
        }
    }
}

/// A loaded file's text as its load hands it over: how long it is, which
/// checking weighs it by, the text itself when the file keeps one, and the
/// digest of its bytes when the project retains source.
pub(crate) struct FileText {
    pub(crate) len: usize,
    pub(crate) source: Option<String>,
    pub(crate) content_hash: Option<u128>,
}

/// One file's parsed state, kept around so `Project::update_file` can undo
/// exactly what that file previously contributed - both to the shared
/// registry and to `dependents` below - before re-indexing its new parse.
/// That undo/redo pair is the difference between an incremental update and
/// a full reload.
pub(crate) struct FileState {
    // Its tree. An analysed file's is kept, as is any file's of a project
    // that is not built once; a context file's of a batch run is not: what
    // the passes and the checks read of it was taken when it was parsed.
    kept_ast: Option<Vec<Stmt>>,
    // The source's length in bytes: what checking the file costs, roughly.
    source_len: usize,
    // The digest of the bytes the file was read from (`source::content_hash`),
    // when the project retains source.
    content_hash: Option<u128>,
    // Where the file came in the order files were loaded, which a rebuild of
    // a name declared in several of them follows; an edited file keeps it.
    load_order: u64,
    functions: Vec<Name>,
    classes: Vec<Name>,
    // The global constants it declares (`const`, `define()`).
    constants: Vec<Name>,
    // Whether it declares or references a `PHPStan\` name.
    mentions_phpstan: bool,
    references: FxHashSet<deps::RefKey>,
    // The names its declarations themselves mention
    // (`deps::declaration_referenced_names`).
    declaration_references: FxHashSet<deps::RefKey>,
    // Inline `@phpstan-ignore*` comments, read while the file was lexed
    // (the AST carries no comment text).
    ignores: ignore_comments::IgnoreComments,
    // The registry names of the file's anonymous classes.
    anonymous_classes: Vec<Name>,
    // This file's own raw source text, kept only when `Project::keeps_source`
    // says so: the project retains source and the file keeps its tree. `None`
    // for every ordinary `check-project` batch run, which never needs it again
    // once this file's own diagnostics are computed.
    source: Option<String>,
    // Same meaning as the `analyzed` parameter of `insert_numbered_file` -
    // `true` for a real, first-party file being checked, `false` for
    // context (vendor/scan/stub, never analyzed or reported). Recorded
    // per-file so `Project::is_analyzed`/`analyzed_paths` can answer
    // without threading extra state through every caller.
    analyzed: bool,
    // The composer package this file belongs to (`Project::packages`).
    owner: Option<composer::PackageOwner>,
    // Its function and method bodies, which the declarations' provenance
    // finds.
    bodies: FileBodies,
    // What it assigns to globals, before the registry says what the values
    // are.
    global_facts: Arc<GlobalFacts>,
    // Its declarations as built, which the registry's are rebuilt from when
    // a name they declare changes.
    built_decls: Vec<BuiltDecl>,
    // What it binds, when it is a `Pest.php`.
    pest_facts: Option<PestFacts>,
    // The classes it declares unconditionally, as the renamed-argument
    // check reads them.
    declared_classes: Vec<DeclaredClass>,
    // What is true of the file as a whole (`syntax::FileFacts`).
    facts: FileFacts,
    // Its lines, which the lines and columns of its spans are found by.
    line_index: Arc<LineIndex>,
}

/// A file's tree: the one it kept, or one parsed again.
pub(crate) enum AstRef<'a> {
    Kept(&'a [Stmt]),
    Parsed(Arc<Vec<Stmt>>),
}

impl std::ops::Deref for AstRef<'_> {
    type Target = [Stmt];

    fn deref(&self) -> &[Stmt] {
        match self {
            AstRef::Kept(ast) => ast,
            AstRef::Parsed(ast) => ast,
        }
    }
}

/// The trees of the context files most recently parsed again, which a pass
/// that reads a few of them (the files that declare a class writing a
/// private property) asks for more than once.
#[derive(Default)]
struct AstCache(std::collections::VecDeque<(PathBuf, Arc<Vec<Stmt>>)>);

impl AstCache {
    const CAPACITY: usize = 32;

    fn get(&mut self, path: &Path) -> Option<Arc<Vec<Stmt>>> {
        let at = self.0.iter().position(|(cached, _)| cached == path)?;
        let entry = self.0.remove(at)?;
        let ast = Arc::clone(&entry.1);
        self.0.push_back(entry);
        Some(ast)
    }

    fn insert(&mut self, path: PathBuf, ast: Arc<Vec<Stmt>>) {
        if self.0.len() == Self::CAPACITY {
            self.0.pop_front();
        }
        self.0.push_back((path, ast));
    }

    fn forget(&mut self, path: &Path) {
        self.0.retain(|(cached, _)| cached != path);
    }
}

impl FileState {
    /// The file's tree, for a reader of a file whose tree is kept: an
    /// analysed file's. A test build asserts that no reader asks for the
    /// tree of a file that has none.
    fn ast(&self) -> &[Stmt] {
        debug_assert!(
            self.kept_ast.is_some(),
            "a reader asked for the tree of a context file that dropped it"
        );
        self.kept_ast.as_deref().unwrap_or(&[])
    }

    /// The file's tree, when it is kept.
    fn tree(&self) -> Option<&[Stmt]> {
        self.kept_ast.as_deref()
    }
}

/// The loaded files' function and method bodies, found by provenance.
impl BodySource for file_table::FileTable<FileState> {
    #[cfg(debug_assertions)]
    fn body(&self, provenance: &Provenance) -> Option<&Arc<Vec<Stmt>>> {
        self.get_by_id(provenance.file()?)?
            .bodies
            .get(provenance.declaration_span())
    }

    fn summary(&self, provenance: &Provenance) -> Option<&Arc<crate::registry::BodySummary>> {
        self.get_by_id(provenance.file()?)?
            .bodies
            .summary(provenance.declaration_span())
    }

    fn init_flow(&self, provenance: &Provenance) -> Option<&Arc<crate::registry::InitFlow>> {
        self.get_by_id(provenance.file()?)?
            .bodies
            .init_flow(provenance.declaration_span())
    }
}

/// A multi-file PHP project: every loaded file's own parse tree, one
/// registry merged across all of them (so a class in one file can extend a
/// class declared in another), and a reverse index - `dependents[name]` is
/// every file that *references* `name` - used to find which other files
/// need re-checking when a file's declarations change.
///
/// `Session::load` is for the initial, full load of a codebase; `update_file`
/// is the incremental path: re-parsing and re-checking only the file that
/// changed, plus the files reading a declaration whose resolved form the
/// change altered (see `affected_files`) - every other file's parse tree
/// and registry entries are untouched.
pub struct Project {
    files: file_table::FileTable<FileState>,
    dependents: FxHashMap<deps::RefKey, FxHashSet<FileId>>,
    // `dependents` for the names files' declarations themselves mention
    // (`deps::declaration_referenced_names`).
    signature_dependents: FxHashMap<deps::RefKey, FxHashSet<FileId>>,
    registry: Registry,
    sql: sql::context::SqlContext,
    // Directory-scoped Pest `$this`-binding config, rebuilt from every
    // currently-loaded `Pest.php` file whenever the loaded file set could
    // have changed (`finish_loading`/`update_file`) - see `pest_config`'s
    // own module doc comment for the full resolution algorithm.
    pest_config: PestConfig,
    // The bytes the loads and the edits read a file's text from, in place
    // of the disk's where it has an entry (see `source`).
    source: source::FileSource,
    // The root as it resolves on disk, links followed. Every file's key is
    // under it, and relative paths, include paths included, resolve against
    // it. `None` for a project whose files are given in memory.
    absolute_root: Option<PathBuf>,
    // The composer packages and which directories they own; each file's
    // owner is kept in its `FileState`.
    packages: composer::PackageIndex,
    // What the checks are configured with (`apply_settings`).
    settings: CheckSettings,
    // What the configuration and the manifests say of where includes are
    // found (`apply_settings`).
    include_settings: IncludeSettings,
    // What the include rule knows of the project: the settings, with what
    // the code adds to them. Built on first use, and built again after the
    // files that feed it change (`refresh_include_env`).
    include_env: OnceLock<IncludeEnv>,
    // Whether `Session::load`/`update_file` keep each file's own raw source
    // text around in its `FileState` (see that field's own doc comment) -
    // off by default. Only `lsp.rs`'s `Server` turns this on:
    // hover/definition's own text-split technique (`lsp::text_split`)
    // needs a known span's exact source text, including whatever an
    // editor's `didChange` most recently sent - not necessarily what's on
    // disk.
    retain_source: bool,
    // `ProjectSettings::built_once`.
    built_once: bool,
    // Whether the finishing passes have run.
    finished: bool,
    // Whether PHPStan's source stays merged in whatever the loaded files
    // mention: for the project building the registry that has it.
    phpstan_pinned: bool,
    // The `FileState::load_order` the next new file gets.
    next_load_order: u64,
    // The files loaded or edited since the finishing passes last walked
    // them for global variables.
    globals_stale: FxHashSet<FileId>,
    // Whether a file's parse skipped function bodies, whose references
    // `dependents` has not, until `complete_skipped_references`.
    skipped_bodies: bool,
    // The files a cancelled recheck never reached, whose last diagnostics
    // are therefore stale (`take_unchecked`).
    unchecked: Vec<FileId>,
    // Every top-level function's own unconditional declaration site(s) -
    // always populated (no `retain_source` gate), the same indexing-side/
    // check-side split
    // `duplicate_class_sites` below uses: its own gate is at the check
    // side, `RulesSettings::check_duplicate_function_declarations`, not the
    // indexing side. See `DuplicateFunctionSites`'s own doc comment.
    duplicate_fn_sites: DuplicateFunctionSites,
    // The class-like counterpart to `duplicate_fn_sites` above - always
    // populated too (its own gate is at the check side,
    // `RulesSettings::check_duplicate_class_declarations`, not the indexing
    // side, so an opt-out project pays only the cost of an unused table,
    // not a missing one it might later want turned on incrementally).
    // See `DuplicateClassSites`'s own doc comment.
    duplicate_class_sites: DuplicateClassSites,
    // `textDocument/references`/`textDocument/rename`'s own whole-project
    // use-site index - see `use_sites`'s own module doc comment for the
    // two-tier split. Tier 1 (class/function references) is indexed for
    // each loaded file while `indexes_class_fn_sites` holds, with real,
    // narrow per-identifier spans when `retain_source` is on and AST-node
    // spans otherwise (see `use_sites::index_class_fn_refs_without_source`'s
    // own doc comment). NOT unconditionally: measured on a real corpus,
    // this walk is too expensive to run for every loaded file when nothing
    // consumes it. Tier 2 (member references) is NOT populated by ordinary
    // indexing at all - see `member_index_built`.
    use_sites: UseSiteIndex,
    // Whether Tier 2 (member references) of `use_sites` has ever been
    // built for this project - `false` until the first
    // `ensure_member_index_built` call. Deliberately NOT built as part
    // of `finish_loading`/`Session::load`: Tier 2 needs one real `Checker`
    // pass per file, which would make every workspace load pay a cost
    // only `textDocument/references`/`textDocument/rename` actually
    // need - see `ensure_member_index_built`'s own doc comment.
    member_index_built: bool,
    // The trees of context files re-parsed on demand, the latest few.
    ast_cache: Mutex<AstCache>,
    // Every real `Enum` class's own canonically-declared FQN that
    // `Checker::escaped_enums` (see its own doc comment) recorded for
    // some file - populated/queried alongside `use_sites`/`member_
    // index_built` above, by the identical two paths (`ensure_member_
    // index_built`'s own loop, or `seed_member_refs` fed from `check-
    // project`'s primary pass). Consulted by `whole_program::
    // dead_enum_cases::find_dead_cases`.
    escaped_enum_cases: FxHashSet<String>,
    // `Checker::escaped_member_classes`'s own project-wide merge -
    // exactly the same shape/population paths as `escaped_enum_cases`
    // above, for `whole_program::dead_members::find_dead_members`
    // instead of dead enum cases.
    escaped_member_classes: FxHashSet<String>,
}

/// The stack each checking thread starts with. Deep walks grow their stack
/// on demand (`syntax::stack`), so this only spares ordinary code those
/// growths.
pub const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;

/// The process's pool of one `WORKER_STACK_SIZE` thread per core (or
/// `PHPSTAN_RS_THREADS`, for a harness running many projects at once,
/// where the cores are already busy), which runs every parallel parse,
/// check and registry pass - one pool, so concurrent sessions and
/// projects share the cores rather than each starting threads of its own.
pub fn worker_pool() -> Arc<rayon::ThreadPool> {
    static POOL: std::sync::LazyLock<Arc<rayon::ThreadPool>> = std::sync::LazyLock::new(|| {
        let threads = std::env::var("PHPSTAN_RS_THREADS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .stack_size(WORKER_STACK_SIZE)
                .build()
                .expect("failed to build the worker pool"),
        )
    });
    Arc::clone(&POOL)
}

/// The result of `Project::check_paths_isolating_panics`: successful
/// per-file diagnostics, plus every path whose own check panicked
/// (message, not a diagnostic list) instead of completing.
pub type CheckPathsIsolatingPanics = (FxHashMap<FileId, Vec<Diagnostic>>, Vec<(PathBuf, String)>);

/// Everything one main-pass check of a file produces.
pub(crate) struct FileCheck {
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Member-reference sites; empty unless member references were
    /// collected.
    pub(crate) member_refs: Vec<(Span, String, String)>,
    /// The `Enum` classes the file let escape (`Checker::escaped_enums`).
    pub(crate) escaped_enums: FxHashSet<String>,
    /// The classes the file let escape through a dynamic member access
    /// (`Checker::escaped_member_classes`).
    pub(crate) escaped_member_classes: FxHashSet<String>,
    /// Named-argument call sites against overridable methods.
    pub(crate) named_argument_calls: checker::NamedArgumentCallFacts,
    /// The `--dump-types` lines (`line:col [host] vars`, one per
    /// statement); empty unless requested.
    pub(crate) type_dump: Vec<String>,
    /// The file's writes to private properties.
    property_assign_facts: PropertyAssignFacts,
    /// The file's private property types, until judged against the
    /// writes of every file (`Project::judge_property_types`).
    property_type_sites: Vec<checker::PropertyTypeSite>,
}

impl FileCheck {
    fn empty() -> FileCheck {
        FileCheck {
            diagnostics: Vec::new(),
            member_refs: Vec::new(),
            escaped_enums: FxHashSet::default(),
            escaped_member_classes: FxHashSet::default(),
            named_argument_calls: FxHashMap::default(),
            type_dump: Vec::new(),
            property_assign_facts: NameMap::default(),
            property_type_sites: Vec::new(),
        }
    }
}

/// What checking a file came to: its check, or the message it panicked
/// with.
pub(crate) type CheckOutcome = Result<FileCheck, String>;

/// A panic payload's message.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown panic".to_string())
}

/// `f` over `items`, in parallel on `pool` when there is one, the results
/// in `items`' order.
fn map_on_pool<T: Sync, R: Send>(
    pool: Option<&rayon::ThreadPool>,
    items: &[T],
    f: impl Fn(&T) -> R + Sync + Send,
) -> Vec<R> {
    match pool {
        Some(pool) => pool.install(|| items.par_iter().map(&f).collect()),
        None => items.iter().map(f).collect(),
    }
}

/// `map_on_pool`, with the costliest items started first: a file as large
/// as the rest of a run's combined, started last, would run alone after the
/// others are done. The results are still in `items`' order.
fn map_on_pool_largest_first<T: Sync, R: Send>(
    pool: Option<&rayon::ThreadPool>,
    items: &[T],
    cost: impl Fn(&T) -> usize,
    f: impl Fn(&T) -> R + Sync + Send,
) -> Vec<R> {
    let Some(pool) = pool else {
        return items.iter().map(f).collect();
    };
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(cost(&items[i])));
    let done: Vec<(usize, R)> = pool.install(|| {
        order
            .par_iter()
            .with_max_len(1)
            .map(|&i| (i, f(&items[i])))
            .collect()
    });
    let mut results: Vec<Option<R>> = items.iter().map(|_| None).collect();
    for (i, result) in done {
        results[i] = Some(result);
    }
    results
        .into_iter()
        .map(|result| result.expect("every item was mapped"))
        .collect()
}

/// Everything a project is configured with, applied together
/// (`Project::apply_settings`).
#[derive(Default, Clone, PartialEq)]
pub struct ProjectSettings {
    /// The root as it resolves on disk, links followed. Every file's key is
    /// under it; `None` for a project whose files are given in memory.
    pub absolute_root: Option<PathBuf>,
    /// How configured path text compares with the letters of a path: as the
    /// root's directory compares names (`Session::resolve` probes it).
    pub path_case: diagnostics::CaseRule,
    /// The composer packages, and which directories each owns.
    pub packages: composer::PackageIndex,
    /// Where includes are found: the include path, the directories a run
    /// starts in and the trees the project reads.
    pub include: IncludeSettings,
    pub php_versions: Option<types::PhpVersions>,
    /// The level types are accepted at; `None` when it's unset.
    pub configured_level: Option<u8>,
    /// Whether the project consumes PHPStan as a library, which the
    /// PHPStan-API rules apply to.
    pub phpstan_api_check_enabled: bool,
    /// Keeps each file's source text (the language server's hover and
    /// definition need it).
    pub retain_source: bool,
    /// Whether the project is built once and never changed afterwards (a
    /// batch run), so it keeps no copy of its declarations as built for
    /// recomputing them after a change.
    pub built_once: bool,
    pub rules: crate::rules_settings::RulesSettings,
    /// Which extensions are active; each extension's checks default to it.
    pub extensions: crate::ext::ActiveExtensions,
    pub extras: crate::ext::extras::ExtrasSettings,
    pub strict_rules: crate::ext::strict_rules::StrictRulesSettings,
    pub shipmonk: crate::ext::shipmonk::ShipmonkSettings,
    pub voku: crate::ext::voku::VokuSettings,
    pub slam: crate::ext::slam::SlamSettings,
    pub psr3: crate::ext::psr3::Psr3Settings,
    pub sidz: crate::ext::sidz::SidzSettings,
    pub phpunit: crate::ext::phpunit::PhpunitSettings,
    /// Every restricted symbol, the presets' included (`Session::resolve`).
    pub restricted_symbols: Vec<crate::restricted_symbols::RestrictedSymbolEntry>,
    pub doctrine: crate::ext::doctrine::DoctrineSettings,
    /// `EngineSettings::dynamic_constant_entries`.
    pub dynamic_constant_names: Vec<(String, Option<String>)>,
    pub overlays: crate::assertion_tables::OverlaySettings,
    pub sql_reflection: Option<sql::config::SqlReflectionConfig>,
    pub sql_schema: Option<sql::schema::SchemaCatalog>,
}

impl Project {
    /// A project holding the built-in stubs of `stubs`, resolved: a clone
    /// of a registry resolved once per `Stubs`, sharing its tables. Waits
    /// for that registry, so a caller that has other work to do first
    /// makes the project with `Project::loading`.
    pub fn from_stubs(stubs: &Stubs) -> Self {
        Project::with_registry(stubs.base(false))
    }

    /// A project that holds no stubs yet, for a caller that parses the
    /// files before it inserts them: `expect_files` gives it the stubs
    /// the files call for, else `finish_loading` the plain ones. Until
    /// then it can't be checked.
    pub fn loading() -> Self {
        Project::with_registry(Registry::default())
    }

    /// A project holding the built-in stubs of the process's shared test
    /// value (`Stubs::shared`).
    #[cfg(test)]
    pub fn new() -> Self {
        Project::from_stubs(Stubs::shared())
    }

    /// The registry of the built-in stubs, with PHPStan's own source when
    /// `with_phpstan`, resolved: loaded from the stub cache when it holds
    /// it, else built and stored there. Frozen, to share. Run on the
    /// thread `Stubs` builds it on.
    pub(crate) fn resolved_stubs(stubs: &Stubs, with_phpstan: bool) -> Registry {
        let cache_name = if with_phpstan {
            "stub-base-phpstan"
        } else {
            "stub-base"
        };
        if let Some(stored) = Registry::load_cached(stubs.cache_dir(), cache_name) {
            match stored {
                Ok(mut registry) => {
                    registry.corpus = Some(stubs.corpus());
                    registry.freeze();
                    return registry;
                }
                Err(error) => {
                    debug_assert!(false, "the stored {cache_name} is inconsistent: {error}");
                    eprintln!(
                        "phpstan-rs: the stored {cache_name} is inconsistent ({error}); building it again"
                    );
                }
            }
        }
        let corpus = stubs.corpus();
        let mut registry = Registry::default();
        registry.corpus = Some(Arc::clone(&corpus));
        if with_phpstan {
            registry.phpstan_stubs_loaded = true;
            registry.merge_stubs(corpus.generated_stubs());
            registry.merge_stub_classes(corpus.generated_stub_classes());
            registry.merge_stub_constants(corpus.generated_stub_constants());
        } else {
            registry.merge_stubs(corpus.builtin_stubs());
            registry.merge_stub_classes(corpus.builtin_stub_classes());
            registry.merge_stub_constants(corpus.builtin_stub_constants());
        }
        let mut base = Project::with_registry(registry);
        base.phpstan_pinned = with_phpstan;
        base.finish_declarations(None);
        let mut registry = base.registry;
        if let Err(error) = registry.store_cached(stubs.cache_dir(), cache_name) {
            eprintln!("phpstan-rs: the {cache_name} can't be stored ({error})");
        }
        registry.freeze();
        registry
    }

    fn with_registry(registry: Registry) -> Self {
        Project {
            files: file_table::FileTable::default(),
            dependents: FxHashMap::default(),
            signature_dependents: FxHashMap::default(),
            registry,
            sql: sql::context::SqlContext::default(),
            pest_config: PestConfig::default(),
            source: source::FileSource::default(),
            absolute_root: None,
            packages: composer::PackageIndex::default(),
            settings: CheckSettings::default(),
            include_settings: IncludeSettings::default(),
            include_env: OnceLock::new(),
            retain_source: false,
            built_once: false,
            finished: false,
            unchecked: Vec::new(),
            phpstan_pinned: false,
            next_load_order: 0,
            globals_stale: FxHashSet::default(),
            skipped_bodies: false,
            duplicate_fn_sites: DuplicateFunctionSites::default(),
            duplicate_class_sites: DuplicateClassSites::default(),
            use_sites: UseSiteIndex::default(),
            member_index_built: false,
            ast_cache: Mutex::default(),
            escaped_enum_cases: FxHashSet::default(),
            escaped_member_classes: FxHashSet::default(),
        }
    }

    /// Configures the project with `settings`, replacing what it had.
    /// Each loaded file's package owner is recomputed. The loaded files' class
    /// and function use sites are re-derived when whether they are indexed
    /// changes.
    pub fn apply_settings(&mut self, settings: ProjectSettings) {
        let ProjectSettings {
            absolute_root,
            path_case,
            packages,
            include,
            php_versions,
            configured_level,
            phpstan_api_check_enabled,
            retain_source,
            built_once,
            rules,
            extensions: active,
            extras,
            strict_rules,
            shipmonk,
            voku,
            slam,
            psr3,
            sidz,
            phpunit,
            restricted_symbols,
            doctrine,
            dynamic_constant_names,
            overlays,
            sql_reflection,
            sql_schema,
        } = settings;
        let class_fn_sites_before = self.indexes_class_fn_sites();
        self.absolute_root = absolute_root;
        self.include_settings = include;
        self.include_env = OnceLock::new();
        self.retain_source = retain_source;
        self.built_once = built_once;

        self.registry.php_versions = php_versions;
        self.registry
            .set_assertion_tables(overlays.assertions, overlays.class_argument_returns);

        self.settings = CheckSettings {
            rule_levels: RuleLevelHelper {
                configured_level,
                strict_mixed_arrays: rules.strict_mixed_arrays,
                check_benevolent_union_types: rules.check_benevolent_union_types,
            },
            constants: ConstantResolver::new(&dynamic_constant_names),
            rules,
            phpstan_api: phpstan_api_check_enabled,
            extras: extras.resolve(active.extras),
            strict_rules: strict_rules.resolve(active.strict_rules),
            shipmonk: shipmonk.resolve(active.shipmonk),
            voku: voku.resolve(active.voku),
            slam: slam.resolve(active.slam),
            psr3_context_key_pattern: psr3.context_key_pattern().ok().flatten(),
            psr3: psr3.resolve(active.psr3),
            sidz: sidz.resolve(active.sidz),
            phpunit: phpunit.resolve(active.phpunit),
            deprecation_rules: active.deprecation_rules,
            restricted_symbols: crate::restricted_symbols::RestrictedSymbols::compile(
                &restricted_symbols,
            ),
            path_case,
            doctrine: active.doctrine,
            doctrine_custom_functions: doctrine
                .custom_functions
                .iter()
                .map(|e| (e.name.to_ascii_lowercase(), e.kind))
                .collect(),
            language_extensions: active.language_extensions,
            pest: active.pest,
        };

        self.sql.set_config(sql_reflection);
        self.sql.set_schema(sql_schema);

        self.packages = packages;
        let owners: Vec<(PathBuf, Option<composer::PackageOwner>)> = self
            .files
            .keys()
            .map(|path| (path.clone(), self.owner_of_file(path)))
            .collect();
        for (path, owner) in owners {
            if let Some(file) = self.files.get_mut(&path) {
                file.owner = owner;
            }
        }
        if !self.member_index_built && class_fn_sites_before != self.indexes_class_fn_sites() {
            self.reindex_class_fn_sites();
        }
    }

    pub(crate) fn use_sites(&self) -> &UseSiteIndex {
        &self.use_sites
    }

    /// Rebuilds `pest_config` from every currently-loaded `Pest.php` file
    /// (`PestConfig::build` itself filters for that filename) - cheap
    /// regardless of corpus size, since it's proportional only to the
    /// small number of `Pest.php` files a real project has, never the
    /// whole loaded file set. Called unconditionally from both
    /// `finish_loading` and `update_file`, rather than only when the
    /// specific edited file is itself a `Pest.php`, to keep the LSP
    /// incremental-edit path correct if a `Pest.php` config itself is
    /// what just changed.
    fn rebuild_pest_config(&mut self) {
        self.pest_config = PestConfig::build(
            self.files
                .iter()
                .filter_map(|(_, state)| state.pest_facts.as_ref()),
        );
    }

    /// The project's SQL reflection service.
    pub fn sql(&self) -> &sql::context::SqlContext {
        &self.sql
    }

    /// What the checks are configured with.
    pub fn check_settings(&self) -> &CheckSettings {
        &self.settings
    }

    /// The loaded files' function and method bodies.
    pub(crate) fn bodies(&self) -> &dyn BodySource {
        &self.files
    }

    /// The lines of the loaded file `path`.
    pub(crate) fn line_index(&self, path: &Path) -> Option<&Arc<LineIndex>> {
        self.files.get(path).map(|file| &file.line_index)
    }

    /// The 1-based line of the byte offset `offset` of the loaded file
    /// numbered `file`: the first line for a file that is not loaded.
    pub(crate) fn line_in(&self, file: FileId, offset: u32) -> u32 {
        self.files
            .get_by_id(file)
            .map_or(1, |file| file.line_index.line(offset))
    }

    /// Applies every analyzed file's inline ignore comments to `results`
    /// and, with `report_unmatched`, reports each ignore that suppressed
    /// nothing (`ignore.unmatchedLine`/`ignore.unmatchedIdentifier`),
    /// except inside a trait, whose diagnostics depend on its users.
    pub fn apply_inline_ignores(
        &self,
        results: &mut FxHashMap<FileId, Vec<Diagnostic>>,
        report_unmatched: bool,
    ) {
        for (id, _, file) in self.files.iter_with_ids() {
            if !file.analyzed {
                continue;
            }
            let ignores = &file.ignores;
            let mut matched_lines: FxHashSet<u32> = FxHashSet::default();
            let mut matched_ids: FxHashSet<(u32, &str)> = FxHashSet::default();
            if let Some(diagnostics) = results.get_mut(&id) {
                diagnostics.retain(|d| {
                    // Reserved for diagnostics about an ignore comment itself,
                    // which no ignore comment may suppress, as in PHPStan.
                    if d.non_ignorable {
                        return true;
                    }
                    let line = d.span.start.line;
                    if ignores.lines.contains(&line) {
                        matched_lines.insert(line);
                        return false;
                    }
                    if let Some(ids) = ignores.identifiers.get(&line)
                        && let Some(id) = ids.iter().find(|id| ***id == *d.identifier)
                    {
                        matched_ids.insert((line, id.as_str()));
                        return false;
                    }
                    true
                });
            }
            if !report_unmatched {
                continue;
            }
            let trait_lines = trait_line_ranges(file.ast(), &file.line_index);
            let in_trait = |line: u32| trait_lines.iter().any(|(a, b)| (*a..=*b).contains(&line));
            let mut unmatched: Vec<Diagnostic> = Vec::new();
            let mut unmatched_line = |line: u32, message: String, identifier: &'static str| {
                unmatched.push(
                    diagnostics::DiagnosticBuilder::new(message, Span::default(), 0, identifier)
                        .non_ignorable()
                        .build_at(line_start_range(line)),
                );
            };
            for &line in &ignores.lines {
                if !matched_lines.contains(&line) && !in_trait(line) {
                    unmatched_line(
                        line,
                        format!("No error to ignore is reported on line {line}."),
                        "ignore.unmatchedLine",
                    );
                }
            }
            for (&line, ids) in &ignores.identifiers {
                if ignores.lines.contains(&line) || in_trait(line) {
                    continue;
                }
                for id in ids {
                    if !matched_ids.contains(&(line, id.as_str())) {
                        unmatched_line(
                            line,
                            format!("No error with identifier {id} is reported on line {line}."),
                            "ignore.unmatchedIdentifier",
                        );
                    }
                }
            }
            if !unmatched.is_empty() {
                let entry = results.entry(id).or_default();
                entry.extend(unmatched);
                entry.sort_by(|a, b| {
                    (a.span.start.line, a.span.start.col, &a.message).cmp(&(
                        b.span.start.line,
                        b.span.start.col,
                        &b.message,
                    ))
                });
            }
        }
    }

    /// `path` as an absolute path: a relative path joined to the root, then
    /// `.` and `..` resolved lexically. Without a root, only the latter.
    pub(crate) fn absolute_path(&self, path: &Path) -> PathBuf {
        let joined = match &self.absolute_root {
            Some(root) => root.join(path),
            None => path.to_path_buf(),
        };
        composer::manifest::lexically_normalized(&joined)
    }

    /// The composer package the file `path` belongs to: the one owning the
    /// directory it is in.
    fn owner_of_file(&self, path: &Path) -> Option<composer::PackageOwner> {
        self.packages
            .owner_of_dir(self.absolute_path(path).parent()?)
    }

    /// The root every file's key is under, as it resolves on disk.
    pub fn absolute_root(&self) -> Option<&Path> {
        self.absolute_root.as_deref()
    }

    /// A loaded file's own retained raw source text, when the file keeps its
    /// source (`Project::keeps_source`) and this exact path is currently
    /// loaded - `None` otherwise.
    pub fn source(&self, path: &Path) -> Option<&str> {
        self.files.get(path)?.source.as_deref()
    }

    /// The digest of the bytes the file at `path` was loaded or last edited
    /// with, which an edit compares against to find a document unchanged. A
    /// project that does not retain source keeps no digest.
    pub fn content_hash(&self, path: &Path) -> Option<u128> {
        self.files.get(path).and_then(|file| file.content_hash)
    }

    /// Gives the file at `path` the bytes `bytes` for every load and edit
    /// that reads it, in place of the disk's (the editor's open documents).
    pub fn set_document(&mut self, path: &Path, bytes: &[u8]) {
        let full = self.full_path(path);
        self.source.set(&full, bytes);
        self.forget_parse_of(path);
    }

    /// Replaces every document the loads and edits read, with `source`.
    pub(crate) fn set_source(&mut self, source: source::FileSource) {
        self.source = source;
        self.ast_cache = Mutex::default();
    }

    /// Gives the file at `path` back to the disk for the reads after this.
    pub fn unset_document(&mut self, path: &Path) {
        let full = self.full_path(path);
        self.source.unset(&full);
        self.forget_parse_of(path);
    }

    /// Forgets the tree `ast_of` parsed for `path`, once the file's bytes or
    /// state change, so the next parse reads them again.
    fn forget_parse_of(&mut self, path: &Path) {
        self.ast_cache
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .forget(path);
    }

    /// The bytes of the file at `path`: the document's, else the disk's.
    pub(crate) fn read_bytes(&self, path: &Path) -> std::io::Result<std::borrow::Cow<'_, [u8]>> {
        self.source.read(&self.full_path(path))
    }

    /// Reads the bytes of the file at `path` into `buf`, which is cleared
    /// first, so a worker reads each of its files into one buffer.
    pub(crate) fn read_into(&self, path: &Path, buf: &mut Vec<u8>) -> std::io::Result<()> {
        self.source.read_into(&self.full_path(path), buf)
    }

    /// Whether a loaded file keeps its tree: an analysed file's does, and a
    /// context file's does unless the project is built once, which drops
    /// each context tree on the thread that parsed it.
    pub(crate) fn keeps_tree(&self, analyzed: bool) -> bool {
        analyzed || !self.built_once
    }

    /// Whether a loaded file keeps its source text: the project retains
    /// source, and the file keeps its tree. Any other file's text is read and
    /// parsed on its worker, and dropped there.
    pub(crate) fn keeps_source(&self, analyzed: bool) -> bool {
        self.retain_source && self.keeps_tree(analyzed)
    }

    /// `path` as the disk names it: under the root, when there is one and the
    /// path is relative. An absolute path, as a listed one is, is its own.
    fn full_path<'p>(&self, path: &'p Path) -> Cow<'p, Path> {
        match &self.absolute_root {
            Some(root) if !path.is_absolute() => Cow::Owned(root.join(path)),
            _ => Cow::Borrowed(path),
        }
    }

    /// The tree of the loaded file numbered `id`: the one it kept, or, for a
    /// context file of a batch run, one parsed from the file again.
    pub(crate) fn ast_of(&self, id: FileId) -> Option<AstRef<'_>> {
        let file = self.files.get_by_id(id)?;
        let path = self.files.path(id);
        if let Some(ast) = file.tree() {
            return Some(AstRef::Kept(ast));
        }
        if let Some(ast) = self.ast_cache.lock().ok()?.get(path) {
            return Some(AstRef::Parsed(ast));
        }
        let bytes = self.read_bytes(path).ok()?;
        let parsed = match source::utf8_text(&bytes) {
            Some(text) => parse_with_ignores(None, path, text, None, false, false),
            None => parse_with_ignores(None, path, &bytes[..], None, false, false),
        }
        .ok()?;
        let ast = Arc::new(parsed.ast);
        self.ast_cache
            .lock()
            .ok()?
            .insert(path.to_path_buf(), Arc::clone(&ast));
        Some(AstRef::Parsed(ast))
    }

    /// The AST of a currently-loaded file, if any - the position-walker's
    /// (`lsp::navigation`) own entry point.
    pub(crate) fn ast(&self, path: &Path) -> Option<&[Stmt]> {
        self.files.get(path)?.tree()
    }

    /// Numbers `paths` before any is inserted, which a caller can do while
    /// it parses them. The number of each place in the list, for
    /// `insert_numbered_file`.
    pub(crate) fn number_files(
        &mut self,
        numbering: (file_table::FileTable<FileState>, Vec<FileId>),
    ) -> Vec<FileId> {
        let (table, numbers) = numbering;
        if self.files.has_numbers() {
            return numbers
                .iter()
                .map(|&id| self.files.id_for_numbered(&table, id))
                .collect();
        }
        self.files = table;
        numbers
    }

    /// Numbers the paths of the composer manifests, which the composer
    /// checks report in, though no file is held for them. Their numbers,
    /// one for each manifest.
    pub(crate) fn number_manifests(&mut self) -> Vec<FileId> {
        let Project {
            files, packages, ..
        } = self;
        packages
            .manifests()
            .iter()
            .map(|manifest| files.id_for(&manifest.path))
            .collect()
    }

    /// The path `file` was numbered by. The path is kept whether or not the
    /// file is held.
    pub fn path_of(&self, file: FileId) -> &Path {
        self.files.path(file)
    }

    /// The number of `path`, if the project has numbered it.
    pub(crate) fn file_id(&self, path: &Path) -> Option<FileId> {
        self.files.id_of(path)
    }

    /// The number of `path`, which the project has numbered.
    #[cfg(test)]
    pub(crate) fn number_of(&self, path: impl AsRef<Path>) -> FileId {
        self.files
            .id_of(path.as_ref())
            .expect("the path is numbered")
    }

    /// The place of `path` among the numbered paths, in path order, which is
    /// the order a sort of paths gives (`FileTable::rank_of`). A path with no
    /// number holds no file, and a check of it records nothing, so its place
    /// does not change a merge: it sorts last.
    #[cfg(test)]
    pub(crate) fn path_rank(&self, path: &Path) -> u32 {
        self.files
            .id_of(path)
            .map_or(u32::MAX, |id| self.files.rank_of(id))
    }

    /// Sets the places of the numbered paths from `ids`, which hold each of
    /// them once in path order (`FileTable::set_path_order`).
    pub(crate) fn set_path_order(&mut self, ids: &[FileId]) {
        self.files.set_path_order(ids);
    }

    /// Inserts the declarations of `files`, each with its path, whether it is
    /// analysed, and its declarations, in the order given: a name that more
    /// than one of them declares resolves as the sequential inserts of those
    /// declarations, in that order, leave it (`Registry::merge_declarations`).
    /// `insert_numbered_file` does not insert declarations, so a caller
    /// inserts a file's declarations here before it hands the file over.
    /// An empty batch changes nothing.
    pub(crate) fn insert_declarations<'a>(
        &mut self,
        pool: &rayon::ThreadPool,
        files: impl IntoIterator<Item = (&'a Path, bool, &'a [BuiltDecl])>,
    ) {
        let declared: Vec<DeclaredFile<'a>> = files
            .into_iter()
            .map(|(path, analyzed, decls)| {
                debug_assert!(
                    decls.iter().all(BuiltDecl::is_bound),
                    "a file's declarations are inserted once it is numbered"
                );
                DeclaredFile {
                    decls,
                    analyzed,
                    path,
                }
            })
            .collect();
        if declared.is_empty() {
            return;
        }
        self.begin_declaration_changes();
        let root = self.absolute_root.as_deref();
        self.registry
            .merge_declarations(pool, |merge| merge.insert(declared, root));
    }

    /// Inserts `parsed` as the file `path`, which `id` numbers, without its
    /// declarations or its table entries: `insert_declarations` inserts the
    /// declarations, `insert_file_tables` the entries, and a caller runs both
    /// first. The reader of the file indexed its use sites from its text
    /// (`index_use_sites`) before the insert: only the reader holds the text.
    pub(crate) fn insert_numbered_file(
        &mut self,
        id: FileId,
        path: &Path,
        parsed: ParsedFile,
        text: FileText,
        analyzed: bool,
    ) {
        let ParsedFile {
            ast,
            lex_warnings: _,
            functions,
            classes,
            constants,
            references,
            declaration_references,
            ignores,
            built_decls,
            anonymous_classes,
            duplicate_fn_sites: _,
            duplicate_class_sites: _,
            bodies,
            global_facts,
            pest_facts,
            declared_classes,
            facts,
            line_index,
            class_fn_refs: _,
            owner,
        } = parsed;
        let FileText {
            len,
            source,
            content_hash,
        } = text;
        debug_assert_eq!(
            ast.is_some(),
            self.keeps_tree(analyzed),
            "a file arrives with its tree when the project keeps it, and without it when not"
        );
        self.skipped_bodies |= facts.has_skimmed_bodies;
        let owner = owner.unwrap_or_else(|| self.owner_of_file(path));
        self.globals_stale.insert(id);
        let load_order = self.next_load_order;
        self.next_load_order += 1;
        self.forget_parse_of(path);
        self.files.insert_at(
            id,
            FileState {
                kept_ast: ast,
                source_len: len,
                content_hash,
                load_order,
                mentions_phpstan: mentions_phpstan(&functions, &classes, &references),
                functions,
                classes,
                constants,
                references,
                declaration_references,
                ignores,
                anonymous_classes,
                source,
                analyzed,
                owner,
                bodies,
                global_facts,
                built_decls,
                pest_facts,
                declared_classes,
                facts,
                line_index,
            },
        );
    }

    /// The class and function use sites of a file being loaded.
    ///
    /// Real source text (`retain_source`) buys narrower per-identifier
    /// spans (`use_sites::index_class_fn_refs`'s own doc comment) - a
    /// batch run with no retained source can still get real, just less
    /// precisely-scoped, entries via `index_class_fn_refs_without_
    /// source`, which `whole_program::dead_code` (existence-
    /// only, never reads a use-site's span) can't tell apart from the
    /// precise version at all. But this
    /// walk is NOT cheap enough to run unconditionally in batch mode -
    /// measured on a real corpus (Piwigo, ~14k loaded files including
    /// vendor context): running it for every loaded file regardless of
    /// whether anything consumes it roughly doubled `check-project`'s
    /// own wall time. Gated on `indexes_class_fn_sites` for exactly that
    /// reason - with the check left off, the default, a run pays nothing
    /// extra here until an editor session builds Tier 2.
    fn class_fn_refs_of(&self, ast: &[Stmt], source: &str) -> Vec<(Span, use_sites::RefTarget)> {
        if !self.indexes_class_fn_sites() {
            Vec::new()
        } else if self.retain_source {
            use_sites::index_class_fn_refs(ast, source)
        } else {
            use_sites::index_class_fn_refs_without_source(ast)
        }
    }

    /// Whether the class and function use sites of the loaded files are in
    /// the index. They are while the dead-code check is on, which reads them,
    /// and once an editor session has built the member index, whose per-file
    /// re-index drops every entry a file has, so these are re-derived too.
    pub(crate) fn indexes_class_fn_sites(&self) -> bool {
        self.settings.rules.check_dead_code || (self.retain_source && self.member_index_built)
    }

    /// Indexes `parsed`'s use sites ahead of its insertion, so a
    /// caller parsing many files in parallel indexes them there too.
    pub(crate) fn index_use_sites(&self, parsed: &mut ParsedFile, source: &str) {
        if let Some(ast) = &parsed.ast {
            parsed.class_fn_refs = Some(self.class_fn_refs_of(ast, source));
        }
    }

    /// Merges PHPStan's own source in with the built-in stubs while some
    /// loaded file mentions a `PHPStan\` name, and takes it out again once
    /// none does: most projects never name it, and it is over half of the
    /// stub classes. The names it declares are rebuilt, from the stubs
    /// first and then every file declaring them, as `Project::new`'s merge
    /// and the loads after it would have built them.
    fn sync_phpstan_stubs(&mut self, stubs: Option<&Stubs>) {
        let wanted = self.phpstan_pinned || self.files.values().any(|f| f.mentions_phpstan);
        if self.registry.corpus.is_none() {
            self.switch_to_base(Self::stubs_of(stubs), wanted);
            return;
        }
        if wanted == self.registry.phpstan_stubs_loaded {
            return;
        }
        if wanted && !self.finished {
            self.switch_to_base(Self::stubs_of(stubs), true);
            return;
        }
        self.registry.phpstan_stubs_loaded = wanted;
        let names = self.registry.corpus().phpstan_stub_names();
        self.rebuild_declarations(
            &DeclaredNames {
                functions: names.functions.into_iter().map(Name::from).collect(),
                classes: names.classes.into_iter().map(Name::from).collect(),
                constants: names.constants.into_iter().map(Name::from).collect(),
            },
            None,
        );
    }

    /// The `Stubs` a project that is not finished takes its stubs from, if
    /// the caller gave none: only a test can be in that case.
    fn stubs_of(stubs: Option<&Stubs>) -> &Stubs {
        match stubs {
            Some(stubs) => stubs,
            #[cfg(test)]
            None => Stubs::shared(),
            #[cfg(not(test))]
            None => panic!("a project that is not finished needs the stubs to take its base from"),
        }
    }

    /// Before the files `parsed` are inserted into a project that has none
    /// yet: gives it the registry with PHPStan's source when one of them
    /// mentions it, else the plain one, so that no declaration is
    /// inserted twice.
    pub(crate) fn expect_files<'a>(
        &mut self,
        parsed: impl IntoIterator<Item = &'a ParsedFile>,
        stubs: &Stubs,
    ) {
        if self.finished || !self.files.is_empty() {
            return;
        }
        let with_phpstan = self.registry.phpstan_stubs_loaded
            || parsed
                .into_iter()
                .any(|p| mentions_phpstan(&p.functions, &p.classes, &p.references));
        if self.registry.corpus.is_none() || (with_phpstan && !self.registry.phpstan_stubs_loaded) {
            self.registry.switch_base(stubs.base(with_phpstan));
        }
    }

    /// Starts again from the registry of `stubs`, with PHPStan's own
    /// source in it when `with_phpstan`, instead of resolving that source
    /// for this project alone: the loaded files' declarations go into it
    /// as they went into the registry without it, from the built-in stubs
    /// first, context files before analyzed ones and each group in load order.
    /// Only for a project that hasn't finished yet, whose resolved
    /// declarations are all the shared base's.
    fn switch_to_base(&mut self, stubs: &Stubs, with_phpstan: bool) {
        self.registry.switch_base(stubs.base(with_phpstan));
        self.begin_declaration_changes();
        let mut sources: Vec<(bool, u64, &Path, &[BuiltDecl])> = self
            .files
            .iter()
            .map(|(path, state)| {
                (
                    state.analyzed,
                    state.load_order,
                    path.as_path(),
                    state.built_decls.as_slice(),
                )
            })
            .collect();
        sources.sort_by_key(|source| (source.0, source.1));
        // What the files' trees build, which their kept declarations are.
        #[cfg(debug_assertions)]
        for (_, _, path, decls) in &sources {
            if let Some(state) = self.files.get(path)
                && let Some(ast) = state.tree()
            {
                let file = self.files.id_of(path).expect("a file held has a number");
                let built = crate::registry::build_declarations(ast, &Origin::File(file));
                assert!(
                    built.iter().eq(decls.iter()),
                    "the declarations kept of {} are not those its tree builds",
                    path.display()
                );
            }
        }
        for (analyzed, _, path, decls) in sources {
            for decl in decls {
                if let BuiltDecl::Class { name, .. } = decl {
                    self.registry.record_phpstan_class_file(name, || {
                        shown_path(self.absolute_root.as_deref(), path).to_path_buf()
                    });
                }
                self.registry.insert_built_decl(decl.clone(), analyzed);
            }
        }
    }

    /// Makes the registry's classes and functions the declarations as
    /// built, for declarations to change on; the finishing passes then
    /// resolve again whatever the changes can have affected.
    fn begin_declaration_changes(&mut self) {
        debug_assert!(
            !(self.built_once && self.finished),
            "a project built once can't change after its finishing passes ran"
        );
        self.registry.begin_declaration_changes();
    }

    /// Registers what the function bodies a file's parse skipped refer to,
    /// which are read for it: a load that skipped them is never asked what
    /// depends on a name, and a project asked it first completes what
    /// `dependents` holds.
    fn complete_skipped_references(&mut self) {
        if !std::mem::take(&mut self.skipped_bodies) {
            return;
        }
        let ids: Vec<FileId> = self
            .files
            .iter_with_ids()
            .filter(|(_, _, file)| file.facts.has_skimmed_bodies)
            .map(|(id, _, _)| id)
            .collect();
        let found: Vec<(FileId, FxHashSet<deps::RefKey>)> = worker_pool().install(|| {
            ids.par_iter()
                .filter_map(|&id| {
                    let file = self.files.get_by_id(id)?;
                    Some((id, file.bodies.text_references()))
                })
                .collect()
        });
        for (id, names) in found {
            register_references(&mut self.dependents, id, &names);
            if let Some(file) = self.files.get_by_id_mut(id) {
                file.references.extend(names);
            }
        }
    }

    /// Debug check that completing `dependents` with what the skipped
    /// bodies refer to leaves both scopes as they are.
    #[cfg(debug_assertions)]
    fn assert_skipped_references_add_nothing(
        &mut self,
        changed: &crate::registry::ChangedNames,
        scope: &Scope,
        body_scope: &Scope,
    ) {
        if !self.skipped_bodies {
            return;
        }
        self.complete_skipped_references();
        let (full, full_body) = self.finishing_scopes(changed);
        assert_eq!(scope.class_keys(), full.class_keys());
        assert_eq!(scope.function_keys(), full.function_keys());
        assert_eq!(body_scope.class_keys(), full_body.class_keys());
        assert_eq!(body_scope.function_keys(), full_body.function_keys());
    }

    /// The classes and functions the finishing passes must run over after
    /// `changed`, as two scopes: what resolution reads from a
    /// declaration (its signature, hierarchy, PHPDoc and constant values)
    /// and what purity and property writes read from a body too, a
    /// superset. Each is the changed names plus every class and function
    /// declared where a name in it is referenced - in declarations only,
    /// or anywhere - transitively, in a loaded file or a built-in stub
    /// file.
    fn finishing_scopes(&self, changed: &crate::registry::ChangedNames) -> (Scope, Scope) {
        // The two walks read the project alike and share nothing.
        rayon::join(
            || self.closure_scope(changed, &self.signature_dependents),
            || self.closure_scope(changed, &self.dependents),
        )
    }

    fn closure_scope(
        &self,
        changed: &crate::registry::ChangedNames,
        index: &FxHashMap<deps::RefKey, FxHashSet<FileId>>,
    ) -> Scope {
        let corpus = self.registry.corpus();
        let mut classes: FxHashSet<String> = changed.classes.clone();
        let mut functions: FxHashSet<String> = changed.functions.clone();
        let mut frontier: Vec<deps::RefKey> = classes
            .iter()
            .map(|c| deps::RefKey::class_text(c))
            .chain(functions.iter().map(|f| deps::RefKey::function_text(f)))
            .chain(
                changed
                    .constants
                    .keys()
                    .map(|c| deps::RefKey::constant_text(c)),
            )
            .collect();
        let mut seen: FxHashSet<deps::RefKey> = FxHashSet::default();
        // Each file is expanded once: what it contributes to the scope is
        // its declared names, whichever name led to it.
        let mut expanded = FileIdSet::default();
        let mut stub_files = StubFileSet::default();
        while let Some(key) = frontier.pop() {
            if seen.contains(&key) {
                continue;
            }
            // A project file's declared name is a key of its kind, walked
            // when something references it.
            for file in index.get(&key).into_iter().flatten() {
                if !expanded.insert(*file) {
                    continue;
                }
                let Some(state) = self.files.get_by_id(*file) else {
                    continue;
                };
                for name in state.classes.iter().chain(&state.anonymous_classes) {
                    let text = name.lower().trim_start_matches('\\');
                    if !classes.contains(text) {
                        classes.insert(text.to_string());
                        frontier.push(deps::RefKey::class(name));
                    }
                }
                for name in &state.functions {
                    let text = name.lower().trim_start_matches('\\');
                    if !functions.contains(text) {
                        functions.insert(text.to_string());
                        frontier.push(deps::RefKey::function(name));
                    }
                }
            }
            // A stub file is reached through every key it references, and
            // with it every stub file that references what it declares;
            // the names of each are walked the first time. A stub's name is
            // a key already, and is walked only if a project file
            // references it.
            if let Some(stub_key) = key.stub_key() {
                for file in corpus.stub_dependent_files(stub_key) {
                    stub_files.add_closure(corpus, file, |file| {
                        for (is_class, name) in corpus.stub_file_names(file) {
                            let set = if is_class {
                                &mut classes
                            } else {
                                &mut functions
                            };
                            if set.contains(name) {
                                continue;
                            }
                            set.insert(name.to_string());
                            let key = if is_class {
                                deps::RefKey::class_text(name)
                            } else {
                                deps::RefKey::function_text(name)
                            };
                            if index.contains_key(&key) {
                                frontier.push(key);
                            }
                        }
                    });
                }
            }
            seen.insert(key);
        }
        Scope::from_keys(classes, functions)
    }

    /// Finalizes an initial load (trait flattening).
    #[hotpath::measure]
    pub fn finish_loading_with(&mut self, stubs: &Stubs) {
        self.finish_declarations(Some(stubs));
        self.include_env = OnceLock::new();
    }

    /// `finish_loading_with` the process's shared test stubs.
    #[cfg(test)]
    pub fn finish_loading(&mut self) {
        self.finish_loading_with(Stubs::shared());
    }

    /// Settles which stubs the project has, then runs the finishing passes
    /// on the worker pool. The stubs are settled on the calling thread: a
    /// registry may have to be waited for, which a pool thread must not do.
    /// `stubs` is only needed by a project that is not finished and holds
    /// none yet or gains a file mentioning PHPStan.
    fn finish_declarations(&mut self, stubs: Option<&Stubs>) -> FxHashSet<FileId> {
        self.begin_declaration_changes();
        self.sync_phpstan_stubs(stubs);
        let pool = match stubs {
            Some(stubs) => Arc::clone(stubs.pool()),
            None => worker_pool(),
        };
        pool.install(|| self.run_finishing_passes())
    }

    /// Runs the finishing passes, returning the files that read a class,
    /// function or constant whose resolved form they changed, or a global
    /// variable whose type they changed (`walk_affected`).
    fn run_finishing_passes(&mut self) -> FxHashSet<FileId> {
        self.begin_declaration_changes();
        // What changed is looked up in `dependents`, which holds what every
        // body refers to from here on. A project built once is looked up
        // in only here, and every name its files declare is among those
        // that changed, so what a skipped body refers to adds no declared
        // name to the scopes.
        if self.registry.has_resolved_declarations() && !self.built_once {
            self.complete_skipped_references();
        }
        let (scope, body_scope, changed) = if self.registry.has_resolved_declarations() {
            let changed = self.registry.take_changed();
            let (scope, body_scope) = self.finishing_scopes(&changed);
            #[cfg(debug_assertions)]
            if self.built_once {
                self.assert_skipped_references_add_nothing(&changed, &scope, &body_scope);
            }
            (scope, body_scope, Some(changed))
        } else {
            (Scope::all(), Scope::all(), None)
        };
        if !body_scope.is_all() {
            self.registry.warm(&body_scope);
        }
        let before = self.registry.end_declaration_changes(
            &scope,
            changed.as_ref().map(|_| &body_scope),
            !self.built_once,
        );
        self.registry.set_scope(scope.clone());
        self.finished = true;
        self.registry.reset_lazy_tables();
        self.registry.canonicalize_class_name_case();
        self.registry.flatten_traits();
        self.registry.resolve_method_tags();
        // `flatten_phpdoc_inheritance` consumes `lookup_ancestor_method_
        // with_owner`/`lookup_method_with_owner` internally - see
        // `Registry::index_and_flatten`'s own identical two-precompute-
        // calls comment for why a single call at the very end (like
        // `precompute_ancestor_names`) isn't enough here.
        self.registry.precompute_member_owners();
        // Aliases and deferred value types resolve first, so inherited
        // PHPDoc copies an ancestor's finished types.
        self.registry.resolve_deferred_type_imports();
        self.registry.resolve_deferred_value_of();
        self.registry.flatten_phpdoc_inheritance();
        self.registry.inherit_property_hooks();
        self.registry.apply_assertion_table();
        // Adds more method/property names (mixin members), which the
        // member-owner maps take in here, before the passes below look
        // methods up. None of those adds or removes a
        // method/property/const name anywhere, so the maps stay correct
        // through them and for the checker's own later use - no third
        // rebuild needed.
        self.registry.resolve_deferred_mixins();
        self.registry.precompute_member_owners_after_mixins();
        // Purity and property writes are found when asked; what was found
        // is kept to find again once the declarations have settled.
        let asked = self.registry.reset_demand(&self.files);
        self.registry.precompute_ancestor_names();
        self.registry.resolve_constant_named_types();
        self.registry.drop_doc_members_excluded_by_native();
        let changed_keys = match (&before, &changed) {
            (Some(before), Some(changed)) => {
                self.registry
                    .changed_since(before, changed, &asked, &self.files)
            }
            _ => FxHashSet::default(),
        };
        // What a file contributes to a global's type reads what it
        // references, so a file is walked again when one of those changed.
        let mut stale = std::mem::take(&mut self.globals_stale);
        let mut walk = AffectedWalk::default();
        self.walk_affected(&mut walk, changed_keys.iter().cloned());
        stale.extend(walk.affected.iter().cloned());
        let files: Vec<(FileId, &Path, &GlobalFacts)> = self
            .files
            .iter_with_ids()
            .map(|(id, p, f)| (id, p.as_path(), &*f.global_facts))
            .collect();
        self.registry.infer_global_variables_of(&files, &stale);
        self.registry.set_scope(Scope::all());
        self.rebuild_pest_config();
        // The walk goes on from where it stopped with the globals that
        // changed too, which `stale` has no use for.
        if let Some(before) = &before {
            self.walk_affected(&mut walk, self.registry.changed_global_variables(before));
        }
        walk.affected
    }

    /// Re-parses one already-loaded file, patches just its own registry
    /// contributions and `dependents` entries, and re-checks it plus every
    /// other file that references a symbol it declared (before or after
    /// the change - a symbol the new version dropped still needs its old
    /// dependents re-checked, since they'll now see it as undefined).
    /// Returns every re-checked file's fresh diagnostics, keyed by its
    /// number - the changed file is always included, even with zero
    /// diagnostics.
    /// Unwinds `path`'s prior contributions to the shared registry/
    /// `dependents` - the same undo both `update_file` (before
    /// re-indexing a new parse) and `remove_files` (permanently) need.
    /// Returns what the removed file was, or `None` when `path` wasn't
    /// loaded (a no-op - both callers treat that as "nothing to undo").
    fn unindex_file(&mut self, path: &Path) -> Option<UnindexedFile> {
        self.begin_declaration_changes();
        self.complete_skipped_references();
        let id = self.files.id_of(path)?;
        self.forget_parse_of(path);
        let old = self.files.remove(path)?;
        let classes: Vec<Name> = old
            .classes
            .iter()
            .chain(&old.anonymous_classes)
            .cloned()
            .collect();
        self.registry.remove_declarations(&old.functions, &classes);
        self.use_sites.remove_file(id);
        self.duplicate_fn_sites
            .remove_declarations(id, &old.functions);
        self.duplicate_class_sites
            .remove_declarations(id, &old.classes);
        unregister_references(&mut self.dependents, id, &old.references);
        unregister_references(
            &mut self.signature_dependents,
            id,
            &old.declaration_references,
        );
        Some(UnindexedFile {
            load_order: old.load_order,
            functions: old.functions,
            classes: old.classes,
            constants: old.constants,
        })
    }

    /// Rebuilds the registry entries of `functions` and `classes` from
    /// every declaration of them still loaded: the built-in stub's first,
    /// as `Project::new` merges it, then each file's, context files before
    /// analyzed ones and each group in load order - the order a project loads
    /// them in. `pending` is a file whose parse isn't in `self.files` yet;
    /// all of its declarations are inserted, at its place in that order.
    fn rebuild_declarations(
        &mut self,
        names: &DeclaredNames,
        pending: Option<(&Path, &[BuiltDecl], bool, u64)>,
    ) {
        self.begin_declaration_changes();
        self.registry
            .remove_declarations(&names.functions, &names.classes);
        let function_keys: FxHashSet<String> = names
            .functions
            .iter()
            .map(|n| n.to_ascii_lowercase())
            .collect();
        let class_keys: FxHashSet<String> = names
            .classes
            .iter()
            .map(|n| n.to_ascii_lowercase())
            .collect();
        let constant_keys: FxHashSet<&str> = names.constants.iter().map(Name::as_str).collect();
        let with_phpstan = self.registry.phpstan_stubs_loaded;
        let corpus = self.registry.shared_corpus();
        for name in &constant_keys {
            self.registry.remove_global_constant(name);
        }
        for name in &function_keys {
            self.registry
                .merge_stubs(corpus.generated_stubs_named(name, with_phpstan));
        }
        for name in &class_keys {
            self.registry
                .merge_stub_classes(corpus.generated_stub_classes_named(name, with_phpstan));
        }
        for name in &constant_keys {
            self.registry
                .merge_stub_constants(corpus.generated_stub_constants_named(name, with_phpstan));
        }
        let declares_one = |state: &FileState| {
            state
                .functions
                .iter()
                .any(|n| function_keys.contains(&n.to_ascii_lowercase()))
                || state
                    .classes
                    .iter()
                    .any(|n| class_keys.contains(&n.to_ascii_lowercase()))
                || state
                    .constants
                    .iter()
                    .any(|n| constant_keys.contains(n.as_str()))
        };
        let mut sources: Vec<(bool, u64, &Path, &[BuiltDecl], bool)> = self
            .files
            .iter()
            .filter(|(_, state)| declares_one(state))
            .map(|(path, state)| {
                (
                    state.analyzed,
                    state.load_order,
                    path.as_path(),
                    state.built_decls.as_slice(),
                    false,
                )
            })
            .collect();
        if let Some((path, decls, analyzed, load_order)) = pending {
            sources.push((analyzed, load_order, path, decls, true));
        }
        sources.sort_by_key(|source| (source.0, source.1));
        for (analyzed, _, path, decls, is_pending) in sources {
            // Of a file already loaded, only the declarations being
            // rebuilt are inserted again; the pending file's all are.
            let rebuilt = |kind: DeclKind, name: &str| match kind {
                DeclKind::Class => class_keys.contains(&name.to_ascii_lowercase()),
                DeclKind::Function => function_keys.contains(&name.to_ascii_lowercase()),
                DeclKind::Constant => constant_keys.contains(name),
            };
            // What the file's tree builds, which its kept declarations are.
            #[cfg(debug_assertions)]
            if !is_pending
                && let Some(state) = self.files.get(path)
                && let Some(ast) = state.tree()
            {
                let file = self.files.id_of(path).expect("a file held has a number");
                let origin = Origin::File(file);
                let keep: Keep<'_> = &rebuilt;
                let built = build_declarations_where(ast, &origin, Some(keep));
                let kept: Vec<&BuiltDecl> = decls
                    .iter()
                    .filter(|decl| {
                        let (kind, name) = decl.kind_and_name();
                        rebuilt(kind, name)
                    })
                    .collect();
                assert!(
                    built.iter().eq(kept.iter().copied()),
                    "the declarations kept of {} are not those its tree builds",
                    path.display()
                );
            }
            for decl in decls {
                let (kind, name) = decl.kind_and_name();
                if !is_pending && !rebuilt(kind, name) {
                    continue;
                }
                if let BuiltDecl::Class { name, .. } = decl {
                    self.registry.record_phpstan_class_file(name, || {
                        shown_path(self.absolute_root.as_deref(), path).to_path_buf()
                    });
                }
                self.registry.insert_built_decl(decl.clone(), analyzed);
            }
        }
    }

    /// Recomputes the own Tier-1 (class/function) use-sites of the file
    /// numbered `id` from its currently-loaded `FileState`, replacing
    /// whatever was previously indexed for it - a no-op past the initial
    /// `use_sites.remove_file` when `retain_source` is off, while
    /// `indexes_class_fn_sites` is off, or when the file isn't currently
    /// loaded at all. Called for EVERY
    /// file `update_file`'s own affected-file loop re-checks, not just
    /// the one that was directly edited: a dependent file's own bare-
    /// `Name`/`ClassConstFetch`/`StaticPropertyFetch` resolution can change (e.g. registry-wide
    /// class-name-case canonicalization) even though its own source text
    /// didn't, so re-deriving Tier 1 only for the edited file itself
    /// would let a dependent's index silently go stale.
    /// `member_refs`: `None` before `ensure_member_index_built` has ever
    /// been called (nothing to reindex for Tier 2 yet, since no path has
    /// any Member entry to begin with), `Some(refs)` (`refs` freshly
    /// recomputed by the caller's own `check_file_and_member_refs` call)
    /// once it has. A single `use_sites.remove_file` covers both tiers at
    /// once - Tier 1 and Tier 2 entries share the same underlying
    /// `by_file`/`by_target` maps, so clearing and rebuilding them
    /// separately (as two independent remove-then-insert calls) would
    /// have each tier's own rebuild silently wipe out the other's
    /// freshly-inserted entries for the same path.
    fn reindex_use_sites_for_file(
        &mut self,
        id: FileId,
        member_refs: Option<Vec<(Span, String, String)>>,
    ) {
        self.use_sites.remove_file(id);
        if self.retain_source
            && self.indexes_class_fn_sites()
            && let Some(file) = self.files.get_by_id(id)
            && let Some(source) = &file.source
        {
            for (span, target) in use_sites::index_class_fn_refs(file.ast(), source) {
                self.use_sites.insert(id, target, span);
            }
        }
        if let Some(member_refs) = member_refs {
            for (span, owner, member) in member_refs {
                self.use_sites
                    .insert(id, use_sites::RefTarget::Member(owner, member), span);
            }
        }
    }

    /// Re-indexes every loaded file's class and function use sites under the
    /// current settings, for a change to whether they are indexed. Valid only
    /// while the member index is unbuilt: the per-file re-index replaces every
    /// entry a file has, Tier 2 included.
    fn reindex_class_fn_sites(&mut self) {
        let files: Vec<FileId> = self.files.iter_with_ids().map(|(id, _, _)| id).collect();
        for id in files {
            self.reindex_use_sites_for_file(id, None);
        }
    }

    /// Builds Tier 2 (member-reference) of `use_sites` for every
    /// currently-loaded file, exactly once - idempotent, a no-op on every
    /// call after the first. Deliberately NOT run as part of
    /// `finish_loading` or a load: unlike Tier 1 (purely
    /// syntactic, no `Checker` needed), Tier 2 needs one real `Checker`
    /// pass per file, which would make every workspace open pay a cost
    /// only `textDocument/references`/`textDocument/rename` actually
    /// need. `Server::references`/`Server::rename` call this first; every
    /// other request path (hover, definition, diagnostics, ...) never
    /// triggers it at all. The FIRST call after a workspace loads is
    /// therefore the one place this cost is paid, synchronously, on
    /// whatever thread calls it - see this item's own open-risks note.
    pub(crate) fn ensure_member_index_built(&mut self) {
        if self.member_index_built {
            return;
        }
        if self.retain_source && !self.indexes_class_fn_sites() {
            // The files were loaded without their class and function sites
            // (`class_fn_refs_of`), so those are indexed here.
            let indexed: Vec<(FileId, Vec<(Span, use_sites::RefTarget)>)> =
                worker_pool().install(|| {
                    self.files
                        .par_iter_with_ids()
                        .filter_map(|(id, _, file)| {
                            let source = file.source.as_ref()?;
                            Some((id, use_sites::index_class_fn_refs(file.ast(), source)))
                        })
                        .collect()
                });
            for (id, refs) in indexed {
                for (span, target) in refs {
                    self.use_sites.insert(id, target, span);
                }
            }
        }
        let files: Vec<FileId> = self.files.iter_with_ids().map(|(id, _, _)| id).collect();
        for id in files {
            let check = self.run_checker(id, true, false);
            self.seed_member_refs(
                id,
                check.member_refs,
                check.escaped_enums,
                check.escaped_member_classes,
            );
        }
        self.mark_member_index_built();
    }

    /// Adopts already-computed Tier-2 member-ref/escaped-enum/escaped-
    /// member-class data for the file numbered `id` into `use_sites`/
    /// `escaped_enum_cases`/`escaped_member_classes` - the per-file work
    /// `ensure_member_index_built`'s own loop body does above, factored out
    /// so `check-project`'s own primary pass can feed it data it already
    /// computed inline (via `check_files`) instead of
    /// `ensure_member_index_built`
    /// re-deriving it via a second, redundant pass. Caller must call
    /// `mark_member_index_built` once every file has been seeded.
    pub fn seed_member_refs(
        &mut self,
        id: FileId,
        member_refs: Vec<(Span, String, String)>,
        escaped_enums: FxHashSet<String>,
        escaped_member_classes: FxHashSet<String>,
    ) {
        for (span, owner, member) in member_refs {
            self.use_sites
                .insert(id, use_sites::RefTarget::Member(owner, member), span);
        }
        self.escaped_enum_cases.extend(escaped_enums);
        self.escaped_member_classes.extend(escaped_member_classes);
    }

    /// See `seed_member_refs`'s own doc comment - marks Tier 2 as built
    /// without `ensure_member_index_built`'s own per-file loop ever
    /// running, once every file has been seeded some other way.
    pub fn mark_member_index_built(&mut self) {
        self.member_index_built = true;
    }

    /// The always-run-to-completion convenience wrapper every existing
    /// caller uses - `update_file_cancellable` with a `should_abort`
    /// that never fires.
    pub fn update_file(
        &mut self,
        path: &Path,
        source: impl AsRef<[u8]>,
    ) -> Result<FxHashMap<FileId, Vec<Diagnostic>>, ParseError> {
        self.update_file_cancellable(path, source, || false)
    }

    /// The debounce worker's own entry point (VSCode plan item 2.2) -
    /// identical to `update_file` except its final affected-files loop
    /// checks `should_abort()` once per file, breaking (not erroring)
    /// before the next `check_file` the moment it returns `true`, so a
    /// generation bump made by a concurrently-running reader thread
    /// (recorded via `lsp::debounce::DebounceQueue::record_edit`) can
    /// stop an in-flight cascade partway through rather than only ever
    /// being observable *between* separate `update_file` calls.
    pub fn update_file_cancellable(
        &mut self,
        path: &Path,
        source: impl AsRef<[u8]>,
        should_abort: impl FnMut() -> bool,
    ) -> Result<FxHashMap<FileId, Vec<Diagnostic>>, ParseError> {
        let analysed = !path_under_excluded_dir(
            shown_path(self.absolute_root(), path),
            walk::Excluded::new(DEFAULT_SCAN_EXCLUDED_DIR_NAMES, self.settings.path_case),
        );
        self.update_file_as(path, source, analysed, false, should_abort)
    }

    /// `update_file_cancellable` with the role the configuration gives the
    /// file: `analysed` is its role when the project has not seen it before,
    /// and `stub` says whether it is parsed as a stub.
    pub fn update_file_as(
        &mut self,
        path: &Path,
        source: impl AsRef<[u8]>,
        analysed: bool,
        stub: bool,
        should_abort: impl FnMut() -> bool,
    ) -> Result<FxHashMap<FileId, Vec<Diagnostic>>, ParseError> {
        let bytes = source.as_ref();
        self.update_from(
            path,
            bytes,
            || source::content_hash(bytes),
            analysed,
            stub,
            should_abort,
        )
    }

    /// `update_file_as` with a document whose digest was taken already, as
    /// the language server takes it once for every project that holds the
    /// file.
    pub fn update_digested_as(
        &mut self,
        path: &Path,
        document: source::Digested<'_>,
        analysed: bool,
        stub: bool,
        should_abort: impl FnMut() -> bool,
    ) -> Result<FxHashMap<FileId, Vec<Diagnostic>>, ParseError> {
        self.update_from(
            path,
            document.bytes(),
            || document.digest(),
            analysed,
            stub,
            should_abort,
        )
    }

    /// `update_file_as` for `bytes`, whose digest `digest` gives when the
    /// project retains source.
    fn update_from(
        &mut self,
        path: &Path,
        bytes: &[u8],
        digest: impl FnOnce() -> u128,
        analysed: bool,
        stub: bool,
        mut should_abort: impl FnMut() -> bool,
    ) -> Result<FxHashMap<FileId, Vec<Diagnostic>>, ParseError> {
        // Parse BEFORE unindexing anything - on a parse failure (the
        // normal, common state while a file is mid-edit, not an edge
        // case), this must leave every existing registry/
        // `self.files` entry for `path` completely untouched, not
        // half-torn-down. `unindex_file` used to run first unconditionally,
        // which meant any edit that failed to parse permanently discarded
        // the file's own last-good `FileState` (AST + retained source)
        // the moment this method returned `Err` - `Project::ast`/
        // `Project::source` read straight from `self.files`, with no
        // other place a "last known good" version could still be found.
        // That silently broke every last-good-AST-dependent LSP feature
        // (hover/definition/documentSymbol, and `lsp::completion`'s own
        // member-completion probe, which specifically needs the AST from
        // *before* the very edit that introduced its own trigger) for as
        // long as the file kept failing to parse, not just for the one
        // request that happened to race a genuinely-invalid save.
        let root = self.absolute_root();
        // A file the project has seen keeps its analysed flag: an edit does
        // not change what kind of file it is. The flag is read before the
        // parse, which keeps ignore comments for an analysed file only.
        let analyzed = self.files.get(path).map_or(analysed, |file| file.analyzed);
        // A file that is not UTF-8 is parsed from its bytes; a lossy reading of
        // them is its text.
        let text = source::decode_text(bytes);
        let mut parsed = match &text {
            Cow::Borrowed(text) => {
                parse_file_with_names(path, root, *text, stub, BodyText::Parsed, None, analyzed)
            }
            Cow::Owned(_) => {
                parse_file_with_names(path, root, bytes, stub, BodyText::Parsed, None, analyzed)
            }
        }?;
        parsed.bind_file(self.files.id_for(path));
        // Whether the file moved where includes are looked up before; `None`
        // for a file the project did not hold.
        let moves_lookup_before = self
            .files
            .get(path)
            .map(|file| file.facts.mentions & checker::mentions::LOOKUP != 0);
        let previous = self.unindex_file(path);
        let ParsedFile {
            ast,
            lex_warnings: _,
            functions,
            classes,
            constants,
            references,
            declaration_references,
            ignores,
            built_decls,
            anonymous_classes,
            duplicate_fn_sites,
            duplicate_class_sites,
            bodies,
            global_facts,
            pest_facts,
            declared_classes,
            facts,
            line_index,
            class_fn_refs: _,
            owner: _,
        } = parsed;
        let ast = ast.expect("a parse keeps its tree");

        let load_order = previous.as_ref().map_or_else(
            || {
                let order = self.next_load_order;
                self.next_load_order += 1;
                order
            },
            |previous| previous.load_order,
        );
        let mut rebuilt = DeclaredNames {
            functions: functions.clone(),
            classes: classes.clone(),
            constants: constants.clone(),
        };
        if let Some(previous) = previous {
            rebuilt.functions.extend(previous.functions);
            rebuilt.classes.extend(previous.classes);
            rebuilt.constants.extend(previous.constants);
        }
        self.rebuild_declarations(&rebuilt, Some((path, &built_decls, analyzed, load_order)));
        let id = self.files.id_for(path);
        for (name, site) in duplicate_fn_sites {
            self.duplicate_fn_sites.add(name, id, site);
        }
        for (name, site) in duplicate_class_sites {
            self.duplicate_class_sites.add(name, id, site);
        }
        register_references(&mut self.dependents, id, &references);
        register_references(&mut self.signature_dependents, id, &declaration_references);
        self.globals_stale.insert(id);
        self.forget_parse_of(path);
        let moves_lookup = facts.mentions & checker::mentions::LOOKUP != 0;
        self.files.insert(
            path.to_path_buf(),
            FileState {
                kept_ast: Some(ast),
                source_len: bytes.len(),
                content_hash: self.retain_source.then(digest),
                load_order,
                mentions_phpstan: mentions_phpstan(&functions, &classes, &references),
                functions,
                classes,
                constants,
                references,
                declaration_references,
                ignores,
                anonymous_classes,
                source: self.retain_source.then(|| text.into_owned()),
                analyzed,
                owner: self.owner_of_file(path),
                bodies,
                global_facts,
                built_decls,
                pest_facts,
                declared_classes,
                facts,
                line_index,
            },
        );
        let mut affected = self.finish_declarations(None);
        // The files that set the include path or move the working directory,
        // and the directories that hold files, feed the include environment.
        let feeds_include_env = moves_lookup_before.is_none_or(|before| before || moves_lookup);
        if feeds_include_env && self.refresh_include_env() {
            affected.extend(self.analyzed_files_that_include());
        }

        let mut results = FxHashMap::default();
        affected.insert(id);
        self.unchecked.clear();
        let mut affected = affected.into_iter();
        while let Some(file) = affected.next() {
            if should_abort() {
                self.unchecked.push(file);
                self.unchecked.extend(affected);
                break;
            }
            let (diagnostics, member_refs) = if self.member_index_built {
                let (d, m) = self.check_file_and_member_refs(file);
                (d, Some(m))
            } else {
                (self.check_file_by_id(file), None)
            };
            self.reindex_use_sites_for_file(file, member_refs);
            results.insert(file, diagnostics);
        }
        Ok(results)
    }

    /// The files the last cancelled `update_file_cancellable` affected but
    /// never rechecked; their previous diagnostics no longer hold.
    pub fn take_unchecked(&mut self) -> Vec<FileId> {
        std::mem::take(&mut self.unchecked)
    }

    /// Unwinds `path`'s prior contributions to the shared registry/
    /// `dependents` - the same undo `update_file` performs before
    /// re-indexing a new parse - but with no re-index step afterward:
    /// `path` is gone, not changed. Reruns the finishing passes, then
    /// re-checks the files reading a declaration they changed or removed
    /// (`affected_files`), as `update_file` does.
    ///
    /// The finishing passes start again from every remaining class's
    /// declaration as built, so nothing the removed files contributed
    /// survives in the classes using them (trait and `@mixin` members,
    /// inherited PHPDoc); a class still naming a removed one directly
    /// (`extends Base`) reads as it would with `Base` never loaded.
    ///
    /// `path`'s number is always a key in the returned map, with an empty
    /// `Vec` when `path` was never loaded in the first place (it is
    /// numbered then) - a harmless no-op a caller doesn't need to guard
    /// with a prior loaded-state check, and exactly the empty
    /// `publishDiagnostics` payload needed to clear whatever the editor
    /// was last showing for a file that no longer exists.
    pub fn remove_file(&mut self, path: &Path) -> FxHashMap<FileId, Vec<Diagnostic>> {
        self.remove_files(std::slice::from_ref(&path.to_path_buf()))
    }

    /// Batch counterpart to `remove_file` - unwinds every one of `paths`
    /// (each via `unindex_file`) and runs the shared registry recompute
    /// (`finish_loading`) at most ONCE for the whole batch, not once per
    /// path - the same "one recompute per batch, not per item" discipline
    /// `Server::add_workspace_folders` (3.2) needs on the load side.
    /// The number of every one of `paths` is always a key in the result
    /// (generalizing `remove_file`'s own guarantee - a check of an
    /// unloaded/just-removed file returns an empty `Vec`, exactly the
    /// clearing `publishDiagnostics` payload a caller needs for a file
    /// that no longer exists), even when none of them were actually
    /// loaded - but the recompute/BFS itself is skipped entirely in that
    /// case, since there is nothing for it to discover.
    pub fn remove_files(&mut self, paths: &[PathBuf]) -> FxHashMap<FileId, Vec<Diagnostic>> {
        let mut removed = DeclaredNames::default();
        let mut any_removed = false;
        for path in paths {
            if let Some(file) = self.unindex_file(path) {
                removed.functions.extend(file.functions);
                removed.classes.extend(file.classes);
                removed.constants.extend(file.constants);
                any_removed = true;
            }
        }
        let mut affected: FxHashSet<FileId> =
            paths.iter().map(|path| self.files.id_for(path)).collect();
        if any_removed {
            self.rebuild_declarations(&removed, None);
            affected.extend(self.finish_declarations(None));
            if self.refresh_include_env() {
                affected.extend(self.analyzed_files_that_include());
            }
        }

        affected
            .into_iter()
            .map(|file| {
                let (diagnostics, member_refs) = if self.member_index_built {
                    let (d, m) = self.check_file_and_member_refs(file);
                    (d, Some(m))
                } else {
                    (self.check_file_by_id(file), None)
                };
                self.reindex_use_sites_for_file(file, member_refs);
                (file, diagnostics)
            })
            .collect()
    }

    /// The files to recheck once the classes, functions and constants
    /// `changed` names (`deps::dependency_key`s) took a new resolved form:
    /// every file referencing one of them, or a declaration whose own
    /// signature, hierarchy or PHPDoc mentions one of them, transitively.
    /// A declaration taking its shape from a changed one is itself among
    /// `changed` when its resolved form changed; the signature closure
    /// reaches the files that read a changed declaration only through
    /// another's types (`f(A $a) { $a->b()->c(); }` where `A::b()`
    /// returns the changed `B`).
    fn walk_affected(
        &self,
        walk: &mut AffectedWalk,
        changed: impl IntoIterator<Item = deps::RefKey>,
    ) {
        let AffectedWalk {
            affected,
            seen,
            stub_files,
        } = walk;
        let corpus = self.registry.corpus();
        let mut frontier: Vec<deps::RefKey> = changed.into_iter().collect();
        while let Some(key) = frontier.pop() {
            if seen.contains(&key) {
                continue;
            }
            affected.extend(self.dependents.get(&key).into_iter().flatten().cloned());
            for file in self.signature_dependents.get(&key).into_iter().flatten() {
                let Some(state) = self.files.get_by_id(*file) else {
                    continue;
                };
                // The names the file declares, as the keys they are
                // referenced by, unless they were reached already or are
                // `key`, which is being walked.
                let reach = |next: deps::RefKey, frontier: &mut Vec<deps::RefKey>| {
                    if next != key && !seen.contains(&next) {
                        frontier.push(next);
                    }
                };
                for name in &state.functions {
                    reach(deps::RefKey::function(name), &mut frontier);
                }
                for name in &state.classes {
                    reach(deps::RefKey::class(name), &mut frontier);
                }
            }
            // A stub's name is a key already, and only a file that reads it
            // leads further: through the files that reference it, which
            // its stub file's closure holds.
            if let Some(stub_key) = key.stub_key() {
                for file in corpus.stub_dependent_files(stub_key) {
                    stub_files.add_closure(corpus, file, |file| {
                        for (is_class, name) in corpus.stub_file_names(file) {
                            if name == stub_key {
                                continue;
                            }
                            let next = if is_class {
                                deps::RefKey::class_text(name)
                            } else {
                                deps::RefKey::function_text(name)
                            };
                            if !seen.contains(&next)
                                && (self.dependents.contains_key(&next)
                                    || self.signature_dependents.contains_key(&next))
                            {
                                frontier.push(next);
                            }
                        }
                    });
                }
            }
            seen.insert(key);
        }
    }

    /// The checking settings, to change directly.
    #[cfg(test)]
    pub(crate) fn check_settings_mut(&mut self) -> &mut CheckSettings {
        &mut self.settings
    }

    /// `path`'s type dump: every statement's recorded variable types.
    #[cfg(test)]
    pub(crate) fn type_dump(&self, path: &Path) -> Vec<String> {
        let Some(id) = self.files.id_of(path) else {
            return Vec::new();
        };
        match self.check_files(&[id], None, false, true).pop() {
            Some((_, Ok(check))) => check.type_dump,
            Some((_, Err(message))) => panic!("{message}"),
            None => Vec::new(),
        }
    }

    /// Type-checks one already-loaded file against the project's current
    /// registry, returning its diagnostics. Its private property types are
    /// judged against the writes of every file making them - its own, and
    /// those declaring its classes' traits (`check_files`).
    #[hotpath::measure]
    pub fn check_file(&self, path: &Path) -> Vec<Diagnostic> {
        self.files
            .id_of(path)
            .map_or_else(Vec::new, |id| self.check_file_by_id(id))
    }

    /// `check_file` of the file numbered `id`.
    pub(crate) fn check_file_by_id(&self, id: FileId) -> Vec<Diagnostic> {
        self.check_one(id, false).diagnostics
    }

    /// The Tier-2 (member-reference) counterpart of `check_file` - the
    /// SAME `Checker`/`check_program` pass already used for diagnostics
    /// also collects member-reference sites, rather than paying for a
    /// second, independent `Checker` pass over the same file. Only
    /// `update_file`'s own affected-file loop calls this, and only once
    /// `member_index_built` is true - see `ensure_member_index_built`'s
    /// own doc comment.
    pub(crate) fn check_file_and_member_refs(
        &self,
        id: FileId,
    ) -> (Vec<Diagnostic>, Vec<(Span, String, String)>) {
        let check = self.check_one(id, true);
        (check.diagnostics, check.member_refs)
    }

    /// `check_files` for one file, whose panic propagates.
    fn check_one(&self, id: FileId, collect_member_refs: bool) -> FileCheck {
        let mut checks = self.check_files(&[id], None, collect_member_refs, false);
        match checks.pop() {
            Some((_, Ok(check))) => check,
            Some((_, Err(message))) => panic!("{message}"),
            None => FileCheck::empty(),
        }
    }

    /// Checks the files numbered `files` (in parallel on `pool` when there
    /// is one), each file's panic caught as its outcome. With
    /// `collect_member_refs`, the Tier-2 member-reference data too (fed to
    /// `seed_member_refs`); with `dump_types`, the file's `--dump-types`
    /// lines. The private property types each file declares are judged once
    /// every file is checked, against the writes all of them made plus those
    /// of any other file writing the same properties (`judge_property_types`).
    /// A number whose file is not held is checked as an empty file.
    pub(crate) fn check_files(
        &self,
        files: &[FileId],
        pool: Option<&rayon::ThreadPool>,
        collect_member_refs: bool,
        dump_types: bool,
    ) -> Vec<(FileId, CheckOutcome)> {
        let mut checks = map_on_pool_largest_first(
            pool,
            files,
            |&id| self.files.get_by_id(id).map_or(0, |file| file.source_len),
            |&id| {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.run_checker(id, collect_member_refs, dump_types)
                }))
                .map_err(panic_message);
                (id, outcome)
            },
        );
        self.judge_property_types(&mut checks, pool);
        // Each file's diagnostics are finished from the project as it is,
        // which nothing here changes, so the files are finished side by
        // side; they stay where they are in `checks`.
        let finish = |(id, outcome): &mut (FileId, CheckOutcome)| {
            if let Ok(check) = outcome {
                let diagnostics = std::mem::take(&mut check.diagnostics);
                check.diagnostics = self.finish_diagnostics(*id, diagnostics);
            }
        };
        match pool {
            Some(pool) => pool.install(|| checks.par_iter_mut().for_each(finish)),
            None => checks.iter_mut().for_each(finish),
        }
        checks
    }

    /// Judges the private property types `checks` recorded against the
    /// writes made in the scope of their classes: those in `checks`, and
    /// those of the other files declaring one of those classes or a trait
    /// they use (transitively), which are checked for their writes alone.
    /// Facts merge in path order, so a union's member order doesn't
    /// depend on scheduling. A file that panicked contributes nothing.
    fn judge_property_types(
        &self,
        checks: &mut [(FileId, CheckOutcome)],
        pool: Option<&rayon::ThreadPool>,
    ) {
        let mut writers: NameSet = NameSet::default();
        for (_, outcome) in checks.iter() {
            if let Ok(check) = outcome {
                for site in &check.property_type_sites {
                    writers.extend(site.writers().iter().cloned());
                }
            }
        }
        if writers.is_empty() {
            return;
        }
        let mut scopes: NameSet = NameSet::default();
        let mut stack: Vec<Name> = writers.iter().cloned().collect();
        while let Some(class) = stack.pop() {
            if let Some(info) = self.registry.classes.get(class.as_ref()) {
                stack.extend(info.uses().iter().cloned());
            }
            scopes.insert(class);
        }
        let mut checked = FileIdSet::default();
        for &(id, _) in checks.iter() {
            checked.insert(id);
        }
        let mut others: Vec<FileId> = self
            .files
            .iter_with_ids()
            .filter(|&(id, _, file)| {
                !checked.contains(id)
                    && file
                        .classes
                        .iter()
                        .any(|class| scopes.contains(class.as_str()))
            })
            .map(|(id, _, _)| id)
            .collect();
        others.sort_by_key(|&id| self.files.rank_of(id));
        let other_facts = map_on_pool(pool, &others, |&id| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.run_checker(id, false, false).property_assign_facts
            }))
            .ok()
        });
        // Each source with its path's place, so the facts merge in path order.
        let mut sources: Vec<(u32, PropertyAssignFacts)> = others
            .iter()
            .zip(other_facts)
            .filter_map(|(&id, facts)| Some((self.files.rank_of(id), facts?)))
            .collect();
        for (id, outcome) in checks.iter_mut() {
            if let Ok(check) = outcome {
                sources.push((
                    self.files.rank_of(*id),
                    std::mem::take(&mut check.property_assign_facts),
                ));
            }
        }
        sources.sort_by_key(|&(rank, _)| rank);
        let mut merged: PropertyAssignFacts = NameMap::default();
        for (_, facts) in sources {
            for (class, props) in facts {
                if !writers.contains(&class) {
                    continue;
                }
                let entry = merged.entry(class).or_default();
                for (prop, mut types) in props {
                    entry.entry(prop).or_default().append(&mut types);
                }
            }
        }
        let relation = self.settings.rule_levels.relation(&self.registry);
        for (id, outcome) in checks.iter_mut() {
            if let Ok(check) = outcome
                && let Some(file) = self.files.get_by_id(*id)
            {
                let sites = std::mem::take(&mut check.property_type_sites);
                check
                    .diagnostics
                    .extend(checker::check_too_wide_property_types(
                        &sites,
                        &merged,
                        &relation,
                        &file.line_index,
                    ));
            }
        }
    }

    /// A bare, un-run `Checker` for `path` plus its AST, for an LSP
    /// feature that needs to drive `check_program` itself with extra
    /// collection flags set first (e.g. `textDocument/inlayHint`'s
    /// `collect_inlay_hints`) rather than through `check_files`'
    /// fixed diagnostic pipeline. Mirrors `run_checker`'s setup exactly.
    pub(crate) fn checker_for(&self, path: &Path) -> Option<(Checker<'_>, AstRef<'_>)> {
        self.checker_for_file(self.files.id_of(path)?)
    }

    /// What the include rule knows of the project and its deployment: the
    /// settings, with the values the analysed files set the include path to,
    /// and the directories a run can start in.
    pub(crate) fn include_env(&self) -> &IncludeEnv {
        self.include_env.get_or_init(|| self.build_include_env())
    }

    fn build_include_env(&self) -> IncludeEnv {
        let mut changes = LookupChanges::default();
        for id in self.analyzed_files() {
            let moves_lookup = self
                .files
                .get_by_id(id)
                .is_some_and(|file| file.facts.mentions & checker::mentions::LOOKUP != 0);
            if !moves_lookup {
                continue;
            }
            if let Some((mut checker, ast)) = self.checker_without_includes_for_file(id) {
                let file_changes = checker.lookup_changes(&ast);
                changes.include_path.extend(file_changes.include_path);
                changes.chdir.extend(file_changes.chdir);
            }
        }
        IncludeEnv::build(
            &self.include_settings,
            self.absolute_root.as_deref(),
            || self.script_dirs(),
            changes,
        )
    }

    /// The directories a run can start in when the configuration declares
    /// none: the root, where a command-line run starts, and the directory of
    /// each analysed file, where a web server starts the script it runs.
    fn script_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = self
            .files
            .iter()
            .filter(|(_, file)| file.analyzed)
            .filter_map(|(path, _)| path.parent())
            .filter(|dir| !dir.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .collect::<FxHashSet<PathBuf>>()
            .into_iter()
            .collect();
        dirs.sort();
        if let Some(root) = &self.absolute_root {
            dirs.retain(|dir| dir != root);
            dirs.insert(0, root.clone());
        }
        dirs
    }

    /// Builds the include environment again from the files as they are now.
    /// Returns whether it is not the one it was: the files that set the
    /// include path changed, or a directory came or went among the places a
    /// script runs from, and every include may be judged otherwise.
    fn refresh_include_env(&mut self) -> bool {
        let Some(before) = self.include_env.take() else {
            return false;
        };
        before != *self.include_env()
    }

    /// `checker_for` the file numbered `id`.
    fn checker_for_file(&self, id: FileId) -> Option<(Checker<'_>, AstRef<'_>)> {
        let (checker, ast) = self.checker_without_includes_for_file(id)?;
        Some((checker.with_include_env(self.include_env()), ast))
    }

    /// `checker_for_file`, that judges no include: what the include
    /// environment is built from reads the files through it.
    fn checker_without_includes_for_file(&self, id: FileId) -> Option<(Checker<'_>, AstRef<'_>)> {
        let file = self.files.get_by_id(id)?;
        let path = self.files.path(id);
        let ast = self.ast_of(id)?;
        let mut checker = Checker::new(&self.registry)
            .with_sql(&self.sql)
            .with_settings(&self.settings)
            .with_bodies(&self.files)
            .with_line_index(&file.line_index);
        checker.set_pest_this_type(self.pest_config.resolve_this_type(path, &self.registry));
        checker.set_pest_hook_properties(self.pest_config.resolve_hook_properties(path));
        checker.set_current_file(path, shown_path(self.absolute_root.as_deref(), path), id);
        // A path that is absolute and normal is its own absolute path, which
        // the checker takes when it is given none.
        if !path_bytes::is_normal_absolute(path) {
            checker.set_current_file_absolute(self.absolute_path(path));
        }
        checker.set_root(self.absolute_root.clone());
        checker.set_mentions(file.facts.mentions);
        Some((checker, ast))
    }

    /// `textDocument/semanticTokens/full`'s own independent re-walk of
    /// `path` - no cache, no shared pass with `check_file`/`update_file`'s
    /// own diagnostics walk (see this item's own open-risks note on that
    /// cost). Built on `checker_for` exactly like `inlay_hints::collect`.
    pub(crate) fn semantic_tokens_for_file(
        &self,
        path: &Path,
    ) -> Vec<checker::semantic_tokens::UsageSemanticToken> {
        let Some((mut checker, ast)) = self.checker_for(path) else {
            return Vec::new();
        };
        checker.check_program(&ast);
        checker.take_semantic_tokens()
    }

    /// Runs the checker over the file numbered `id`, leaving its private
    /// property types unjudged and its diagnostics unfiltered
    /// (`finish_diagnostics`).
    fn run_checker(&self, id: FileId, collect_member_refs: bool, dump_types: bool) -> FileCheck {
        #[cfg(test)]
        tests::panic_injection::panic_if_injected(self.files.path(id));
        let Some((mut checker, ast)) = self.checker_for_file(id) else {
            return FileCheck::empty();
        };
        checker.set_defer_property_type_checks(true);
        checker.set_collect_member_refs(collect_member_refs);
        checker.set_dump_types(dump_types);
        checker.check_program(&ast);
        if collect_member_refs {
            checker.collect_default_and_const_value_member_refs(&ast);
        }
        FileCheck {
            member_refs: checker.take_member_ref_sites(),
            escaped_enums: checker.take_escaped_enums(),
            escaped_member_classes: checker.take_escaped_member_classes(),
            named_argument_calls: checker.take_named_argument_call_facts(),
            type_dump: checker.take_type_dump(),
            property_assign_facts: checker.take_property_assign_facts(),
            property_type_sites: checker.take_property_type_sites(),
            diagnostics: checker.into_diagnostics(),
        }
    }

    /// The checker `diagnostics` of the file numbered `id`, with the
    /// project-level ones added and sorted.
    fn finish_diagnostics(&self, id: FileId, mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
        let Some(file) = self.files.get_by_id(id) else {
            return diagnostics;
        };
        if self.settings.rules.check_duplicate_function_declarations {
            diagnostics.extend(duplicate_functions::check_duplicate_function_declarations(
                id,
                self.absolute_root(),
                &file.line_index,
                &|site| self.files.path(site),
                &|site, offset| self.line_in(site, offset),
                &self.duplicate_fn_sites,
            ));
        }
        if self.settings.rules.check_duplicate_class_declarations {
            diagnostics.extend(duplicate_classes::check_duplicate_class_declarations(
                id,
                self.absolute_root(),
                &file.line_index,
                &|site| self.files.path(site),
                &|site, offset| self.line_in(site, offset),
                &self.duplicate_class_sites,
            ));
        }
        for (line, message) in &file.ignores.parse_errors {
            diagnostics.push(
                diagnostics::DiagnosticBuilder::new(
                    format!("Parse error in @phpstan-ignore: {message}"),
                    Span::default(),
                    0,
                    "ignore.parseError",
                )
                .non_ignorable()
                .build_at(line_start_range(*line)),
            );
        }
        diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.col));
        if self.settings.phpstan_api {
            diagnostics.extend(checker::phpstan_api::check_phpstan_api_usage(
                file.ast(),
                &file.line_index,
                &self.registry,
                Some(file.owner.and_then(|owner| self.packages.name(owner))),
            ));
            // `into_diagnostics` above already sorted the checker's own
            // diagnostics into span order - re-sort once more now that
            // `check_phpstan_api_usage`'s own (already span-ordered
            // amongst themselves, but not yet interleaved) diagnostics
            // have been appended, so every caller still sees one
            // consistent, fully span-ordered list regardless of whether
            // this gate is on.
            diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.col));
        }
        diagnostics
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Runs every `rules` entry's own `check_class`/
    /// `check_function` against every class-like/plain-function this
    /// project's own registry currently knows about - the single entry
    /// point `reflection_rule::ReflectionRule`'s own doc comment
    /// promises. Iteration order follows `FxHashMap`'s own (unspecified,
    /// load-order-dependent) iteration - a caller that needs a stable
    /// output order should sort the returned `Vec` itself, the same way
    /// `main/check_project.rs`'s own batch-checking path already sorts
    /// before printing.
    pub fn run_reflection_rules(
        &self,
        rules: &[Box<dyn crate::reflection_rule::ReflectionRule>],
    ) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for (fqn, info) in &self.registry.classes {
            for rule in rules {
                out.extend(rule.check_class(info, fqn));
            }
        }
        for (fqn, sig) in &self.registry.functions {
            for rule in rules {
                out.extend(rule.check_function(sig, fqn));
            }
        }
        out
    }

    /// Runs the checker over exactly `paths` (which must already be
    /// loaded), on `pool`, keyed by each file's number; a path the project
    /// never numbered holds no file and has no entry. Panic-transparent - a
    /// real checker panic in `paths` propagates to the caller, which is what
    /// a unit test wants; see `check_paths_isolating_panics` for the
    /// batch-safe counterpart.
    pub fn check_paths(
        &self,
        paths: &[PathBuf],
        pool: &rayon::ThreadPool,
    ) -> FxHashMap<FileId, Vec<Diagnostic>> {
        self.check_numbered(&self.numbers_of(paths), pool)
    }

    /// `check_paths` of the files numbered `files`.
    fn check_numbered(
        &self,
        files: &[FileId],
        pool: &rayon::ThreadPool,
    ) -> FxHashMap<FileId, Vec<Diagnostic>> {
        self.check_files(files, Some(pool), false, false)
            .into_iter()
            .map(|(id, outcome)| match outcome {
                Ok(check) => (id, check.diagnostics),
                Err(message) => {
                    panic!(
                        "checking {} panicked: {message}",
                        self.path_of(id).display()
                    )
                }
            })
            .collect()
    }

    /// The numbers of those of `paths` the project has numbered, in order.
    fn numbers_of(&self, paths: &[PathBuf]) -> Vec<FileId> {
        paths
            .iter()
            .filter_map(|path| self.files.id_of(path))
            .collect()
    }

    /// Same as `check_paths`, but a panic while checking one file is
    /// caught and reported rather than aborting every other file's
    /// result, with the panic hook silenced meanwhile, since a
    /// whole-workspace pass runs over uncurated code the caller doesn't
    /// control, where one file's latent checker bug must not take down
    /// every other file's diagnostics. `Registry.sql_reflector`'s only interior mutability is
    /// a plain `OnceLock` - if its lazy-init closure panics mid-connection
    /// attempt, the cell is left simply uninitialized (not corrupted;
    /// `OnceLock::get_or_init`'s own contract), so a later observer sees
    /// either "not yet initialized" or a real, valid reflector - never a
    /// torn/partial one. `AssertUnwindSafe` in `check_files` is that
    /// specific, checked claim, not a blanket "trust me".
    pub fn check_paths_isolating_panics(
        &self,
        paths: &[PathBuf],
        pool: &rayon::ThreadPool,
    ) -> CheckPathsIsolatingPanics {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcomes = self.check_files(&self.numbers_of(paths), Some(pool), false, false);
        std::panic::set_hook(default_hook);

        let mut results = FxHashMap::default();
        let mut panics = Vec::new();
        for (id, outcome) in outcomes {
            match outcome {
                Ok(check) => {
                    results.insert(id, check.diagnostics);
                }
                Err(msg) => panics.push((self.path_of(id).to_path_buf(), msg)),
            }
        }
        panics.sort();
        (results, panics)
    }

    /// Runs the checker over every currently-loaded file, not just one -
    /// the whole-project counterpart to `check_file`. `--check-project`
    /// no longer uses this directly (it needs to check only the files it
    /// was asked to analyze, not every file loaded for context - see
    /// `main.rs::run_check_project`), but it's still the right tool for
    /// tests/tooling that need to see every diagnostic across a real
    /// corpus at once, rather than one file (and its
    /// transitively-affected dependents) at a time the way `update_file`
    /// does.
    pub fn check_all(&self, pool: &rayon::ThreadPool) -> FxHashMap<FileId, Vec<Diagnostic>> {
        let files: Vec<FileId> = self.files.iter_with_ids().map(|(id, _, _)| id).collect();
        self.check_numbered(&files, pool)
    }

    /// Resolves one whole-program finding's own reportable location and
    /// applies what every per-file diagnostic already gets for free, which
    /// a diagnostic built entirely outside `check_files` would otherwise
    /// never see: only first-party files are reported, never vendor or
    /// context ones swept in only because the registry records every
    /// loaded file's declarations, analyzed or not. `None` for an unloaded
    /// or context file. Inline ignore comments apply when the results are
    /// reported.
    fn whole_program_diagnostic(
        &self,
        file: FileId,
        span: Span,
        message: String,
        identifier: &'static str,
    ) -> Option<Diagnostic> {
        let file = self.files.get_by_id(file)?;
        if !file.analyzed {
            return None;
        }
        Some(DiagnosticBuilder::new(message, span, 5, identifier).build(&file.line_index))
    }

    /// Runs every opt-in whole-program diagnostic check (currently
    /// `check_dead_enum_cases`, `check_dead_code`, `check_circular_
    /// dependencies`, `check_dead_abstract_classes`, and `check_dead_
    /// members`) - see `project::whole_program`'s own module doc comment
    /// for why these can't run through the ordinary per-file `Checker`
    /// path `check_file`/`check_all` above use. `check-project` calls
    /// this once, after its own primary per-file pass has populated
    /// `results`, merging this method's own return value in before
    /// level-filtering/`ignore_errors`/baseline (so `@phpstan-ignore`/
    /// baseline suppression apply to a whole-program finding exactly
    /// like an ordinary one). `named_argument_calls` is the primary pass's
    /// named-argument call sites, merged in path order.
    ///
    /// `check_dead_enum_cases` and `check_dead_members` both need
    /// `ensure_member_index_built`'s own Tier-2 use-site index - a real
    /// cost when it has to build it itself (a second, redundant
    /// `Checker` pass per file), avoided entirely when the caller
    /// already seeded it via `seed_member_refs`/`mark_member_index_
    /// built` (`check-project`'s own primary pass does exactly this,
    /// whenever either flag is on - see its own doc comment) before
    /// calling here, in which case `ensure_member_index_built` is a
    /// complete no-op. `check_dead_code` needs only Tier 1 (`use_sites`'s
    /// class/function reference sites) - cheaper than Tier 2 (a plain
    /// syntactic walk, not a real `Checker` pass), but still real per-
    /// loaded-file cost, so `class_fn_refs_of` gates
    /// populating it in batch mode only on this exact flag. `check_circular_dependencies` and
    /// `check_dead_abstract_classes` need neither Tier - both read
    /// directly off `Registry.classes`' own already-resolved fields, so
    /// each costs nothing beyond its own traversal regardless of how
    /// large the project is.
    pub(crate) fn check_whole_program(
        &mut self,
        named_argument_calls: &NamedArgumentCallFactsWithFile,
    ) -> FxHashMap<FileId, Vec<Diagnostic>> {
        let mut results: FxHashMap<FileId, Vec<Diagnostic>> = FxHashMap::default();

        if self.settings.rules.check_dead_enum_cases {
            self.ensure_member_index_built();
            let dead = whole_program::dead_enum_cases::find_dead_cases(
                &self.registry,
                &self.use_sites,
                &self.escaped_enum_cases,
            );
            for (fqn, case) in dead {
                let Some((file, span)) = self.registry.constant_declaration(&fqn, &case) else {
                    continue;
                };
                if let Some(d) = self.whole_program_diagnostic(
                    file,
                    span,
                    format!("Enum case {fqn}::{case} is never used."),
                    "enumCase.unused",
                ) {
                    results.entry(file).or_default().push(d);
                }
            }
        }

        if self.settings.rules.check_dead_code {
            let dead_functions =
                whole_program::dead_code::find_dead_functions(&self.registry, &self.use_sites);
            for name in dead_functions {
                let Some((file, span)) = self.registry.function_declaration(&name) else {
                    continue;
                };
                if let Some(d) = self.whole_program_diagnostic(
                    file,
                    span,
                    format!("Function {name} is never referenced."),
                    "function.unused",
                ) {
                    results.entry(file).or_default().push(d);
                }
            }
            let dead_classes =
                whole_program::dead_code::find_dead_classes(&self.registry, &self.use_sites);
            for name in dead_classes {
                let Some((file, span)) = self.registry.class_declaration(&name) else {
                    continue;
                };
                if let Some(d) = self.whole_program_diagnostic(
                    file,
                    span,
                    format!("Class {name} is never referenced."),
                    "class.unused",
                ) {
                    results.entry(file).or_default().push(d);
                }
            }
        }

        if self.settings.rules.check_circular_dependencies {
            let cycles =
                whole_program::circular_namespace_deps::find_namespace_cycles(&self.registry);
            for cycle in cycles {
                let Some((file, span)) =
                    self.registry.class_declaration(&cycle.representative_class)
                else {
                    continue;
                };
                let namespaces = cycle.namespaces.join(", ");
                if let Some(d) = self.whole_program_diagnostic(
                    file,
                    span,
                    format!("Circular namespace dependency: {namespaces}."),
                    "namespace.circularDependency",
                ) {
                    results.entry(file).or_default().push(d);
                }
            }
        }

        if self.settings.rules.check_dead_abstract_classes {
            let dead_abstract_classes =
                whole_program::dead_abstract_classes::find_dead_abstract_classes(&self.registry);
            for name in dead_abstract_classes {
                let Some((file, span)) = self.registry.class_declaration(&name) else {
                    continue;
                };
                if let Some(d) = self.whole_program_diagnostic(
                    file,
                    span,
                    format!(
                        "Abstract class {name} has no concrete subclass anywhere in the project."
                    ),
                    "class.deadAbstract",
                ) {
                    results.entry(file).or_default().push(d);
                }
            }
        }

        if self.settings.rules.check_dead_members {
            self.ensure_member_index_built();
            let dead_members = whole_program::dead_members::find_dead_members(
                &self.registry,
                &self.use_sites,
                &self.escaped_member_classes,
            );
            for (owner, name, kind) in dead_members {
                let declaration = match kind {
                    whole_program::dead_members::MemberKind::Method => {
                        self.registry.method_declaration(&owner, &name)
                    }
                    whole_program::dead_members::MemberKind::Property => {
                        self.registry.property_declaration(&owner, &name)
                    }
                    whole_program::dead_members::MemberKind::Const => {
                        self.registry.constant_declaration(&owner, &name)
                    }
                };
                let Some((file, span)) = declaration else {
                    continue;
                };
                let (noun, identifier) = match kind {
                    whole_program::dead_members::MemberKind::Method => ("Method", "method.unused"),
                    whole_program::dead_members::MemberKind::Property => {
                        ("Property", "property.unused")
                    }
                    whole_program::dead_members::MemberKind::Const => {
                        ("Constant", "classConstant.unused")
                    }
                };
                if let Some(d) = self.whole_program_diagnostic(
                    file,
                    span,
                    format!("{noun} {owner}::{name} is never used."),
                    identifier,
                ) {
                    results.entry(file).or_default().push(d);
                }
            }
        }

        if self.settings.rules.check_renamed_named_arguments {
            let mut programs: Vec<(FileId, &[DeclaredClass])> = self
                .files
                .iter_with_ids()
                .map(|(id, _, file)| (id, file.declared_classes.as_slice()))
                .collect();
            programs.sort_by_key(|&(id, _)| self.files.rank_of(id));
            let overrides = whole_program::renamed_named_arguments::renamed_parameter_overrides(
                &self.registry,
                programs.into_iter().flat_map(|(_, classes)| classes),
            );
            let collisions =
                whole_program::renamed_named_arguments::find_renamed_named_argument_collisions(
                    named_argument_calls,
                    &overrides,
                );
            for (file, span, message) in collisions {
                if let Some(d) = self.whole_program_diagnostic(
                    file,
                    span,
                    message,
                    "argument.parameterRenamedInSubtype",
                ) {
                    results.entry(file).or_default().push(d);
                }
            }
        }

        results
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Every path currently loaded, analyzed or context alike - the
    /// `Server::remove_workspace_folders` "which of my loaded files are
    /// under this now-removed root" query needs every one of them, not
    /// just `analyzed_paths`' first-party subset.
    pub fn loaded_paths(&self) -> impl Iterator<Item = &Path> {
        self.files.keys().map(PathBuf::as_path)
    }

    /// Every loaded path whose own `analyzed` flag is `true` - a real,
    /// first-party file, as opposed to context (vendor/scan/stub).
    pub fn analyzed_paths(&self) -> Vec<PathBuf> {
        self.files
            .iter()
            .filter(|(_, state)| state.analyzed)
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// The numbers of `analyzed_paths`' files, in path order.
    pub(crate) fn analyzed_files(&self) -> Vec<FileId> {
        let mut files: Vec<FileId> = self
            .files
            .iter_with_ids()
            .filter(|(_, _, state)| state.analyzed)
            .map(|(id, _, _)| id)
            .collect();
        files.sort_by_key(|&id| self.files.rank_of(id));
        files
    }

    /// The numbers of the analysed files that include something, in path
    /// order: the only files whose reports the include environment can
    /// change. A `global` statement also sets `has_global_or_include`, which
    /// only widens the set.
    fn analyzed_files_that_include(&self) -> Vec<FileId> {
        let mut files = self.analyzed_files();
        files.retain(|&id| {
            self.files
                .get_by_id(id)
                .is_some_and(|file| file.facts.has_global_or_include)
        });
        files
    }

    /// `true` if `path` is loaded and analyzed; `false` for an unloaded
    /// path or one loaded only as context.
    pub fn is_analyzed(&self, path: &Path) -> bool {
        self.files.get(path).is_some_and(|state| state.analyzed)
    }

    /// `true` if `path` is loaded, analyzed or context alike.
    pub fn is_loaded(&self, path: &Path) -> bool {
        self.files.get(path).is_some()
    }
}

#[cfg(test)]
impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

/// The empty range at the start of `line`.
fn line_start_range(line: u32) -> syntax::span::Range {
    let at = syntax::span::LineCol { line, col: 1 };
    syntax::span::Range { start: at, end: at }
}

/// First and last line of every trait declared in `ast`.
fn trait_line_ranges(ast: &[Stmt], line_index: &LineIndex) -> Vec<(u32, u32)> {
    let mut ranges = Vec::new();
    for stmt in ast {
        syntax::visit::any_stmt_in_body(stmt, |s| {
            if let StmtKind::ClassDecl(c) = &s.kind
                && c.def.kind == syntax::ast::ClassKind::Trait
            {
                ranges.push((line_index.line(s.span.start), line_index.line(s.span.end)));
            }
            false
        });
    }
    ranges
}

/// The spellings a file's facts note, each list with the bits of the walks
/// it gates: the stub attributes' pass, the declarations' walk and the
/// checker's file pass.
static WATCHED: syntax::WatchList = syntax::WatchList(&[
    crate::stub_attributes::WATCH,
    file_declarations::WATCH,
    checker::mentions::WATCH,
]);

/// What the tree `ast` refers to, and what its declarations do: from what
/// the parse recorded, or by walking the tree when a pass changed it after.
fn file_references(
    ast: &[Stmt],
    recorded: Option<syntax::references::FileReferences>,
) -> (FxHashSet<deps::RefKey>, FxHashSet<deps::RefKey>) {
    let Some(recorded) = recorded else {
        return deps::both_referenced_names(ast);
    };
    let found = deps::recorded_references(recorded);
    // What a walk of the tree finds.
    #[cfg(debug_assertions)]
    {
        let walked = deps::both_referenced_names(ast);
        assert!(
            found.0 == walked.0,
            "the references the parse recorded and the walk finds differ: {:?}",
            symmetric_difference(&found.0, &walked.0)
        );
        assert!(
            found.1 == walked.1,
            "the declaration references the parse recorded and the walk finds differ: {:?}",
            symmetric_difference(&found.1, &walked.1)
        );
        let declared = deps::declaration_referenced_names(ast);
        assert!(
            found.1 == declared,
            "the declaration references the parse recorded and the declarations' walk finds differ: {:?}",
            symmetric_difference(&found.1, &declared)
        );
    }
    found
}

#[cfg(debug_assertions)]
fn symmetric_difference(
    a: &FxHashSet<deps::RefKey>,
    b: &FxHashSet<deps::RefKey>,
) -> (Vec<deps::RefKey>, Vec<deps::RefKey>) {
    (
        a.difference(b).cloned().collect(),
        b.difference(a).cloned().collect(),
    )
}

/// A file parsed, with its inline ignore comments.
struct Parsed {
    ast: Vec<Stmt>,
    // What the tree refers to, as the parse recorded it; none when a pass
    // changed the tree after.
    references: Option<syntax::references::FileReferences>,
    ignores: ignore_comments::IgnoreComments,
    warnings: Vec<LexWarning>,
    facts: FileFacts,
    line_index: Arc<LineIndex>,
}

fn parse_with_ignores<'a>(
    root: Option<&Path>,
    path: &Path,
    source: impl Into<syntax::Source<'a>>,
    names: Option<&mut syntax::name::NameCache>,
    skim_bodies: bool,
    analysed: bool,
) -> Result<Parsed, ParseError> {
    let parsed = if skim_bodies {
        syntax::parse_skimming(source, names, Some(&WATCHED))?
    } else {
        syntax::parse_watching(source, names, Some(&WATCHED))?
    };
    // Only an analysed file's ignore comments are read (`apply_inline_ignores`,
    // `check_banned`), so a context file keeps none, as its cached state does.
    let ignores = if analysed {
        ignore_comments::scan_parsed(&parsed)
    } else {
        ignore_comments::IgnoreComments::default()
    };
    let mut ast = parsed.stmts;
    // What the file does not name, or hold, there is nothing to find of by
    // walking it.
    let changed = parsed.facts.mentions & crate::stub_attributes::MENTIONS != 0
        && crate::stub_attributes::apply_stub_attributes(&mut ast);
    if parsed.facts.has_anonymous_class {
        crate::registry::name_anonymous_classes(
            &mut ast,
            &diagnostics::report_path(shown_path(root, path)),
            &parsed.line_index,
        );
    }
    Ok(Parsed {
        ast,
        references: (!changed).then_some(parsed.references),
        ignores,
        warnings: parsed.warnings,
        facts: parsed.facts,
        line_index: parsed.line_index,
    })
}

/// Everything about loading one file that's pure - parsing, plus every
/// fact derived only from the parsed AST or the raw source text, with no
/// dependency on (or mutation of) a `Project`'s shared registry. Computing
/// this for many files is embarrassingly parallel - `insert_declarations`,
/// `insert_file_tables` and `insert_numbered_file` below do the remaining
/// half, which mutates the project, in the order of the files (see their own
/// doc comments).
#[cfg_attr(test, derive(Clone))]
pub struct ParsedFile {
    // The tree; none for a file read back from the per-file cache, which
    // keeps what its tree gave and not the tree, and for a file whose tree the
    // project does not keep, once the thread that parsed it has dropped it.
    ast: Option<Vec<Stmt>>,
    // What the lexer skipped over (see `ParsedFile::lex_warnings`).
    lex_warnings: Vec<LexWarning>,
    functions: Vec<Name>,
    classes: Vec<Name>,
    constants: Vec<Name>,
    references: FxHashSet<deps::RefKey>,
    declaration_references: FxHashSet<deps::RefKey>,
    // Empty unless the file is analysed (see `parse_with_ignores`).
    ignores: ignore_comments::IgnoreComments,
    // This file's own declarations, already built into
    // registry-independent `ClassReflection`/`FunctionReflection`/`Type` values (see
    // `registry::build_declarations`'s own doc comment for why that
    // split is pure and safe to compute here, alongside everything else
    // in this struct) - `Project::insert_declarations` feeds these to the
    // registry's batch merge instead of re-deriving them from `ast` via
    // `Registry::index`, so the expensive "turn an AST node into a
    // ClassReflection/FunctionReflection" work runs here, in parallel,
    // rather than in the insert.
    built_decls: Vec<crate::registry::BuiltDecl>,
    // What the file binds, when it is a `Pest.php`.
    pest_facts: Option<PestFacts>,
    // The classes the file declares unconditionally.
    declared_classes: Vec<DeclaredClass>,
    // The file's own top-level function declaration sites, each with its name,
    // in walk order. `insert_file_tables` adds them to the project-wide table
    // beside the file's id.
    duplicate_fn_sites: Vec<(String, FnDeclSite)>,
    // The registry names of the file's anonymous classes.
    anonymous_classes: Vec<Name>,
    // The class-like counterpart to `duplicate_fn_sites` above. Each site
    // carries the `is_stub` the parse was given.
    duplicate_class_sites: Vec<(String, ClassDeclSite)>,
    bodies: FileBodies,
    // What it assigns to globals.
    global_facts: Arc<GlobalFacts>,
    // What is true of the file as a whole (`syntax::FileFacts`).
    facts: FileFacts,
    // Its lines, which the lines and columns of its spans are found by.
    line_index: Arc<LineIndex>,
    // The file's class and function use sites, indexed alongside the parse
    // (`Project::index_use_sites`); `insert_file_tables` takes them.
    class_fn_refs: Option<Vec<(Span, use_sites::RefTarget)>>,
    // The composer package that owns the file, as the walk that listed it
    // found it; `insert_numbered_file` finds it itself when no walk did.
    owner: Option<Option<composer::PackageOwner>>,
}

/// Computes a `ParsedFile` from `source` - see that type's own doc
/// comment. Safe to call concurrently for many files at once (no shared
/// state touched at all). `path` names the file's anonymous classes. The file
/// is parsed as an analysed one, so its ignore comments are kept.
#[hotpath::measure]
pub fn parse_file<'a>(
    path: &Path,
    source: impl Into<syntax::Source<'a>>,
    is_stub: bool,
) -> Result<ParsedFile, ParseError> {
    parse_file_with_names(path, None, source, is_stub, BodyText::Parsed, None, true)
}

/// What a parse keeps of the function and method bodies of a file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BodyText {
    /// Every body is parsed with the file.
    Parsed,
    /// Every body is parsed with the file, and kept as text where the
    /// file's facts allow, to be parsed again when a pass reads it.
    AsText,
    /// A body is not parsed with the file when nothing in it is read by
    /// the load (`syntax::parse_skimming`), and kept as text, to be parsed
    /// when a pass reads it. A syntax error in one is not found.
    Skipped,
}

/// `parse_file`, with the names the files of a load share: the cache is
/// asked for a name before the interner is. `root` is the project's root, if
/// it has one: the file is spelled as the reports spell it (see
/// `shown_path`), and its anonymous classes are named by that spelling.
/// `body_text` is what is kept of the file's function bodies, which a context
/// file's may be text read when a pass asks. `analysed` is whether the file is
/// analysed, which is what keeps its ignore comments.
pub fn parse_file_with_names<'a>(
    path: &Path,
    root: Option<&Path>,
    source: impl Into<syntax::Source<'a>>,
    is_stub: bool,
    body_text: BodyText,
    names: Option<&mut syntax::name::NameCache>,
    analysed: bool,
) -> Result<ParsedFile, ParseError> {
    let source: syntax::Source<'a> = source.into();
    let Parsed {
        ast,
        references,
        ignores,
        warnings: lex_warnings,
        facts,
        line_index,
    } = parse_with_ignores(
        root,
        path,
        source,
        names,
        body_text == BodyText::Skipped,
        analysed,
    )?;
    // Its declarations take the file's number once the file is numbered.
    let origin = Origin::PendingFile;
    let file_declarations::FileDeclarations {
        functions,
        classes,
        constants,
        built_decls,
        duplicate_fn_sites,
        duplicate_class_sites,
        declared_classes,
    } = file_declarations::collect(&ast, is_stub, &origin, &facts);
    let (references, declaration_references) = file_references(&ast, references);
    // What a walk of the declarations finds.
    #[cfg(debug_assertions)]
    assert_eq!(
        format!("{declared_classes:?}"),
        format!("{:?}", declared_classes::declared_classes(&ast)),
        "the classes found alongside the declarations and by their own walk differ"
    );
    // The anonymous classes are the declarations built last, one per
    // `new class`, so the file's own walk need not be repeated for them.
    let anonymous_classes = built_decls
        .iter()
        .filter_map(|decl| match decl {
            BuiltDecl::Class { name, info, .. } if info.is_anonymous() => Some(name.clone()),
            _ => None,
        })
        .collect();
    let bodies = FileBodies::of_file(
        &ast,
        &facts,
        (body_text != BodyText::Parsed).then_some(source),
    );
    let global_facts = Arc::new(GlobalFacts::of_file(&ast, &facts));
    let pest_facts = PestFacts::of(path, &ast);
    let parsed = ParsedFile {
        ast: Some(ast),
        lex_warnings,
        functions,
        classes,
        constants,
        references,
        declaration_references,
        built_decls,
        anonymous_classes,
        duplicate_fn_sites,
        duplicate_class_sites,
        ignores,
        bodies,
        global_facts,
        pest_facts,
        declared_classes,
        facts,
        line_index,
        class_fn_refs: None,
        owner: None,
    };
    // A file that skipped bodies gives what the whole of it gives, but for
    // the bodies, when it parses whole; a body with a syntax error is not
    // found by the skip.
    #[cfg(debug_assertions)]
    if parsed.facts.has_skimmed_bodies
        && let Ok(full) = parse_file_with_names(
            path,
            root,
            source,
            is_stub,
            BodyText::AsText,
            None,
            analysed,
        )
    {
        file_cache::assert_same_but_bodies(&parsed, &full);
    }
    Ok(parsed)
}

impl ParsedFile {
    /// What the lexer skipped over rather than failing on.
    pub fn lex_warnings(&self) -> &[LexWarning] {
        &self.lex_warnings
    }

    /// The declarations the file builds, in the order of the file.
    pub(crate) fn built_decls(&self) -> &[BuiltDecl] {
        &self.built_decls
    }

    /// Gives the file's declarations its number, once the file has one.
    pub(crate) fn bind_file(&mut self, file: FileId) {
        for decl in &mut self.built_decls {
            decl.bind_file(file);
        }
    }
}

/// Recursively collects every `.php` file under `path`, for the CLI's own
/// subcommands (`main/check.rs`, `main/incremental_bench.rs`). Deliberately
/// NOT `#[hotpath::measure]`d
/// itself - it's self-recursive (one call per directory in the tree), and
/// hotpath's per-call "Total" sums every call's own measured span, so a
/// recursive function's aggregate massively over-counts its true unique
/// wall-clock contribution (a parent directory's own span already
/// contains every descendant's separately-measured span too). `check_
/// project.rs`'s `collect_analyze_and_context_files` wraps the *whole*
/// top-level scan in one non-recursive, correctly-measured call instead -
/// see that function's own doc comment for the real number this found.
pub fn collect_php_files(path: &Path, out: &mut Vec<PathBuf>) {
    // Only the paths are kept, so no packages are given to find owners in.
    let packages = composer::PackageIndex::default();
    let mut walk = walk::Walk::new(Arc::default(), &packages);
    walk.add(path, walk::Role::Analysed, &[]);
    out.extend(
        walk.finish(&worker_pool(), &walk::Unstreamed)
            .files
            .into_iter()
            .map(|listed| listed.path.into_path_buf()),
    );
}

/// Directory names a workspace-wide scan skips by default when no real
/// config (1.1's own `phpstan-rs.toml` `paths`) says otherwise - a
/// hardcoded floor, not a substitute for real exclusion config. Matches
/// exactly one path component, not a prefix (`vendor-utils/` is
/// unaffected).
pub const DEFAULT_SCAN_EXCLUDED_DIR_NAMES: &[&str] = &["vendor", ".git", "node_modules"];

/// `true` if any component of `path` is one of the `excluded` directory
/// names - a pure name check, no filesystem access. Used in
/// `Project::update_file_cancellable`, as a never-before-seen path's default
/// `analyzed` classification (see that call site's own doc comment).
pub(crate) fn path_under_excluded_dir(path: &Path, excluded: walk::Excluded<'_>) -> bool {
    path.components()
        .any(|component| excluded.excludes(component.as_os_str()))
}

/// `path` as the reports spell it: relative to `root` when it lies under it,
/// else as given. Keys are absolute, so name-based checks read this instead.
pub fn shown_path<'p>(root: Option<&Path>, path: &'p Path) -> &'p Path {
    root.and_then(|root| path_bytes::strip_prefix(path, root))
        .unwrap_or(path)
}

/// What `Project::unindex_file` removed.
struct UnindexedFile {
    load_order: u64,
    functions: Vec<Name>,
    classes: Vec<Name>,
    constants: Vec<Name>,
}

/// Whether a file declaring `functions` and `classes` and referencing
/// `references` names anything in `PHPStan\`.
fn mentions_phpstan(
    functions: &[Name],
    classes: &[Name],
    references: &FxHashSet<deps::RefKey>,
) -> bool {
    let is_phpstan = |name: &Name| name.lower().starts_with("phpstan\\");
    references.iter().any(|r| is_phpstan(r.name()))
        || functions.iter().chain(classes).any(is_phpstan)
}

/// Names a file declares, by kind.
#[derive(Default)]
struct DeclaredNames {
    functions: Vec<Name>,
    classes: Vec<Name>,
    constants: Vec<Name>,
}

/// The function and class names one file's parse tree declares - exactly
/// what a registry entry needs to be identified by to be removed again
/// later. Enums share the registry's `classes` map (see
/// `Registry::index`), so they're collected into the same list here.
/// Walks the declarations `registry::build_declarations` builds entries
/// from, so an incremental update's removal step finds every key the
/// registry holds.
pub(crate) fn declared_names(stmts: &[Stmt]) -> (Vec<Name>, Vec<Name>) {
    let mut functions = Vec::new();
    let mut classes = Vec::new();
    syntax::declarations::for_each_declaration(stmts, |declaration, _, _| {
        match declaration.declared {
            Declared::Function(_) => functions.push(Name::from(declaration.name())),
            Declared::Class(_) => classes.push(Name::from(declaration.name())),
            Declared::Constant(_) | Declared::Define { .. } => {}
        }
    });
    (functions, classes)
}

fn register_references(
    dependents: &mut FxHashMap<deps::RefKey, FxHashSet<FileId>>,
    file: FileId,
    references: &FxHashSet<deps::RefKey>,
) {
    for name in references {
        register_reference(dependents, file, name);
    }
}

/// Records that `file` references `name`.
fn register_reference(
    dependents: &mut FxHashMap<deps::RefKey, FxHashSet<FileId>>,
    file: FileId,
    name: &deps::RefKey,
) {
    // The name is copied only for the first file to reference it.
    match dependents.get_mut(name) {
        Some(files) => {
            files.insert(file);
        }
        None => {
            dependents.insert(name.clone(), FxHashSet::from_iter([file]));
        }
    }
}

fn unregister_references(
    dependents: &mut FxHashMap<deps::RefKey, FxHashSet<FileId>>,
    file: FileId,
    references: &FxHashSet<deps::RefKey>,
) {
    for name in references {
        if let Some(files) = dependents.get_mut(name) {
            files.remove(&file);
            if files.is_empty() {
                dependents.remove(name);
            }
        }
    }
}

#[cfg(test)]
mod tests;
