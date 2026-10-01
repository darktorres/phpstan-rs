use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::checker::constant_resolver::ConstantResolver;
use crate::checker::rule_level_helper::RuleLevelHelper;
use crate::checker::settings::CheckSettings;
use crate::checker::{self, Checker, PropertyAssignFacts};
use crate::deps;
use crate::ignore_comments;
use crate::pest_config::PestConfig;
use crate::registry::scope::Scope;
use crate::registry::stubs;
use crate::registry::{BodySource, BuiltDecl, FileBodies, Origin, Provenance, Registry};
use diagnostics::{self, Diagnostic, DiagnosticBuilder};
use syntax::ast::{Stmt, StmtKind};
use syntax::declarations::Declared;
use syntax::lexer::LexWarning;
use syntax::name::Interner;
use syntax::parser::ParseError;
use syntax::token::Span;
use types::Name;
use whole_program::renamed_named_arguments::NamedArgumentCallFactsWithPath;

mod architecture;
mod banned;
mod class_shape;
mod composer_hygiene;
pub(crate) mod duplicate_class_sites;
mod duplicate_classes;
pub(crate) mod duplicate_fn_sites;
mod duplicate_functions;
mod mock_hygiene;
pub mod session;
pub(crate) mod use_sites;
mod whole_program;

use duplicate_class_sites::DuplicateClassSites;
use duplicate_fn_sites::DuplicateFunctionSites;
use use_sites::UseSiteIndex;

/// One file's parsed state, kept around so `Project::update_file` can undo
/// exactly what that file previously contributed - both to the shared
/// registry and to `dependents` below - before re-indexing its new parse.
/// That undo/redo pair is the difference between an incremental update and
/// a full reload.
struct FileState {
    ast: Vec<Stmt>,
    functions: Vec<String>,
    classes: Vec<String>,
    // The global constants it declares (`const`, `define()`).
    constants: Vec<String>,
    // Whether it declares or references a `PHPStan\` name.
    mentions_phpstan: bool,
    references: FxHashSet<String>,
    // The names its declarations themselves mention
    // (`deps::declaration_referenced_names`).
    declaration_references: FxHashSet<String>,
    // Inline `@phpstan-ignore*` comments, read while the file was lexed
    // (the AST carries no comment text).
    ignores: ignore_comments::IgnoreComments,
    // The registry names of the file's anonymous classes.
    anonymous_classes: Vec<String>,
    // This file's own raw source text, kept only when
    // `Project::retain_source` is on (see that field's own doc comment) -
    // `None` for every ordinary `check-project` batch run, which never
    // needs it again once this file's own diagnostics are computed.
    source: Option<String>,
    // Same meaning as `load_analyzed_file`'s own `analyzed` parameter -
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
}

/// The loaded files' function and method bodies, found by provenance.
impl BodySource for FxHashMap<PathBuf, FileState> {
    fn body(&self, provenance: &Provenance) -> Option<&Arc<Vec<Stmt>>> {
        self.get(provenance.file()?)?
            .bodies
            .get(provenance.declaration_span())
    }
}

/// A multi-file PHP project: every loaded file's own parse tree, one
/// registry merged across all of them (so a class in one file can extend a
/// class declared in another), and a reverse index - `dependents[name]` is
/// every file that *references* `name` - used to find which other files
/// need re-checking when a file's declarations change.
///
/// `load_file` is for the initial, full load of a codebase; `update_file`
/// is the incremental path: re-parsing and re-checking only the file that
/// changed, plus the files reading a declaration whose resolved form the
/// change altered (see `affected_files`) - every other file's parse tree
/// and registry entries are untouched.
pub struct Project {
    files: FxHashMap<PathBuf, FileState>,
    // What the names in this project's parsed files are interned into,
    // freed with the project.
    interner: Arc<Interner>,
    dependents: FxHashMap<String, FxHashSet<PathBuf>>,
    // `dependents` for the names files' declarations themselves mention
    // (`deps::declaration_referenced_names`).
    signature_dependents: FxHashMap<String, FxHashSet<PathBuf>>,
    registry: Registry,
    sql: sql::context::SqlContext,
    // Directory-scoped Pest `$this`-binding config, rebuilt from every
    // currently-loaded `Pest.php` file whenever the loaded file set could
    // have changed (`finish_loading`/`update_file`) - see `pest_config`'s
    // own module doc comment for the full resolution algorithm.
    pest_config: PestConfig,
    // The directory relative include paths resolve against first, as PHP
    // resolves them against the working directory. `None` means the
    // process's own working directory.
    working_dir: Option<PathBuf>,
    // `working_dir` canonicalized, which
    // `absolute_path` resolves analysed paths against.
    absolute_root: Option<PathBuf>,
    // The composer packages and which directories they own; each file's
    // owner is kept in its `FileState`.
    packages: composer::PackageIndex,
    // What the checks are configured with (`apply_settings`).
    settings: CheckSettings,
    // When set, per-file and whole-program checks leave inline
    // `@phpstan-ignore*` suppression to `apply_inline_ignores`.
    defer_inline_ignores: bool,
    // Whether `load_file`/`update_file` keep each file's own raw source
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
    // The files a cancelled recheck never reached, whose last diagnostics
    // are therefore stale (`take_unchecked`).
    unchecked: Vec<PathBuf>,
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
    // two-tier split. Tier 1 (class/function references) is indexed
    // eagerly here whenever `retain_source` is on (the LSP; real, narrow
    // per-identifier spans) or `check_dead_code` is on (a batch
    // `check-project` run with no retained source; real but less
    // narrowly-scoped AST-node spans instead - see `use_sites::
    // index_class_fn_refs_without_source`'s own doc comment) - NOT
    // unconditionally: measured on a real
    // corpus, this walk is too expensive to run for every loaded file
    // when nothing in batch mode consumes it. Tier 2 (member references)
    // is NOT populated by ordinary indexing at all - see
    // `member_index_built`.
    use_sites: UseSiteIndex,
    // Whether Tier 2 (member references) of `use_sites` has ever been
    // built for this project - `false` until the first
    // `ensure_member_index_built` call. Deliberately NOT built as part
    // of `finish_loading`/`load_file`: Tier 2 needs one real `Checker`
    // pass per file, which would make every workspace load pay a cost
    // only `textDocument/references`/`textDocument/rename` actually
    // need - see `ensure_member_index_built`'s own doc comment.
    member_index_built: bool,
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

/// The process's pool of one `WORKER_STACK_SIZE` thread per core, which
/// runs every parallel parse, check and registry pass - one pool, so
/// concurrent sessions and projects share the cores rather than each
/// starting threads of its own.
pub fn worker_pool() -> Arc<rayon::ThreadPool> {
    static POOL: std::sync::LazyLock<Arc<rayon::ThreadPool>> = std::sync::LazyLock::new(|| {
        Arc::new(
            rayon::ThreadPoolBuilder::new()
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
pub type CheckPathsIsolatingPanics = (FxHashMap<PathBuf, Vec<Diagnostic>>, Vec<(PathBuf, String)>);

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
            property_assign_facts: FxHashMap::default(),
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

/// Everything a project is configured with, applied together
/// (`Project::apply_settings`).
#[derive(Default, Clone, PartialEq)]
pub struct ProjectSettings {
    /// The directory relative include paths resolve against first, as PHP
    /// resolves them against the working directory; `None` means the
    /// process's own.
    pub working_dir: Option<PathBuf>,
    /// `working_dir` canonicalized, which analysed paths resolve against.
    pub absolute_root: Option<PathBuf>,
    /// The composer packages, and which directories each owns.
    pub packages: composer::PackageIndex,
    pub php_versions: Option<types::PhpVersions>,
    /// The level types are accepted at; `None` when it's unset.
    pub configured_level: Option<u8>,
    /// Whether the project consumes PHPStan as a library, which the
    /// PHPStan-API rules apply to.
    pub phpstan_api_check_enabled: bool,
    /// Leaves inline `@phpstan-ignore*` suppression to
    /// `apply_inline_ignores`.
    pub defer_inline_ignores: bool,
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
    /// Every restricted symbol, the presets' included
    /// (`DisallowedCallsSettings::entries`).
    pub restricted_symbols: Vec<crate::restricted_symbols::RestrictedSymbolEntry>,
    pub doctrine: crate::ext::doctrine::DoctrineSettings,
    /// `EngineSettings::dynamic_constant_entries`.
    pub dynamic_constant_names: Vec<(String, Option<String>)>,
    pub overlays: crate::assertion_tables::OverlaySettings,
    pub sql_reflection: Option<sql::config::SqlReflectionConfig>,
    pub sql_schema: Option<sql::schema::SchemaCatalog>,
}

impl Project {
    /// What this project's names are interned into; a file parsed for it
    /// elsewhere (`parse_file`) should use it too.
    pub fn interner(&self) -> &Arc<Interner> {
        &self.interner
    }

    /// A project holding the built-in stubs, resolved: a clone of a
    /// registry resolved once per process, sharing its tables.
    pub fn new() -> Self {
        static BASE: std::sync::LazyLock<Registry> = std::sync::LazyLock::new(|| {
            if let Some(mut registry) = crate::registry::load_cached::<Registry>("stub-base") {
                registry.freeze();
                return registry;
            }
            let mut registry = Registry::default();
            registry.merge_stubs(stubs::builtin_stubs());
            registry.merge_stub_classes(stubs::builtin_stub_classes());
            registry.merge_stub_constants(stubs::builtin_stub_constants());
            let mut base = Project::with_registry(registry);
            base.finish_loading();
            let mut registry = base.registry;
            crate::registry::store_cached("stub-base", &registry);
            registry.freeze();
            registry
        });
        Project::with_registry(BASE.clone())
    }

    fn with_registry(registry: Registry) -> Self {
        Project {
            files: FxHashMap::default(),
            interner: Arc::new(Interner::new()),
            dependents: FxHashMap::default(),
            signature_dependents: FxHashMap::default(),
            registry,
            sql: sql::context::SqlContext::default(),
            pest_config: PestConfig::default(),
            working_dir: None,
            absolute_root: None,
            packages: composer::PackageIndex::default(),
            settings: CheckSettings::default(),
            defer_inline_ignores: false,
            retain_source: false,
            built_once: false,
            finished: false,
            unchecked: Vec::new(),
            duplicate_fn_sites: DuplicateFunctionSites::default(),
            duplicate_class_sites: DuplicateClassSites::default(),
            use_sites: UseSiteIndex::default(),
            member_index_built: false,
            escaped_enum_cases: FxHashSet::default(),
            escaped_member_classes: FxHashSet::default(),
        }
    }

    /// Configures the project with `settings`, replacing what it had.
    /// Each loaded file's package owner is recomputed.
    pub fn apply_settings(&mut self, settings: ProjectSettings) {
        let ProjectSettings {
            working_dir,
            absolute_root,
            packages,
            php_versions,
            configured_level,
            phpstan_api_check_enabled,
            defer_inline_ignores,
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
        self.working_dir = working_dir;
        self.absolute_root = absolute_root;
        self.defer_inline_ignores = defer_inline_ignores;
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
            .map(|path| (path.clone(), self.packages.owner(&self.absolute_path(path))))
            .collect();
        for (path, owner) in owners {
            if let Some(file) = self.files.get_mut(&path) {
                file.owner = owner;
            }
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
                .map(|(path, state)| (path.as_path(), state.ast.as_slice())),
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

    /// Applies every analyzed file's inline ignore comments to `results`
    /// and, with `report_unmatched`, reports each ignore that suppressed
    /// nothing (`ignore.unmatchedLine`/`ignore.unmatchedIdentifier`),
    /// except inside a trait, whose diagnostics depend on its users.
    pub fn apply_inline_ignores(
        &self,
        results: &mut FxHashMap<PathBuf, Vec<Diagnostic>>,
        report_unmatched: bool,
    ) {
        for (path, file) in &self.files {
            if !file.analyzed {
                continue;
            }
            let ignores = &file.ignores;
            let mut matched_lines: FxHashSet<u32> = FxHashSet::default();
            let mut matched_ids: FxHashSet<(u32, &str)> = FxHashSet::default();
            if let Some(diagnostics) = results.get_mut(path) {
                diagnostics.retain(|d| {
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
            let trait_lines = trait_line_ranges(&file.ast);
            let in_trait = |line: u32| trait_lines.iter().any(|(a, b)| (*a..=*b).contains(&line));
            let mut unmatched: Vec<Diagnostic> = Vec::new();
            let mut unmatched_line = |line: u32, message: String, identifier: &'static str| {
                let at = syntax::token::Position { line, col: 1 };
                unmatched.push(
                    diagnostics::DiagnosticBuilder::new(
                        message,
                        Span { start: at, end: at },
                        0,
                        identifier,
                    )
                    .non_ignorable()
                    .build(),
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
                let entry = results.entry(path.clone()).or_default();
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

    /// `path`, spelled from the working directory, as an absolute path:
    /// the working directory swapped for `absolute_root`, then `.` and
    /// `..` resolved lexically. Without an absolute root, only the latter.
    pub(crate) fn absolute_path(&self, path: &Path) -> PathBuf {
        let joined = match (&self.absolute_root, &self.working_dir) {
            (Some(absolute_root), Some(working_dir)) => match path.strip_prefix(working_dir) {
                Ok(rest) => absolute_root.join(rest),
                Err(_) => absolute_root.join(path),
            },
            (Some(absolute_root), None) => absolute_root.join(path),
            (None, _) => path.to_path_buf(),
        };
        composer::manifest::lexically_normalized(&joined)
    }

    /// The name of the composer package `path` belongs to.
    pub(crate) fn package_name(&self, path: &Path) -> Option<&str> {
        let owner = self.files.get(path)?.owner?;
        self.packages.name(owner)
    }

    /// A loaded file's own retained raw source text, when `retain_source`
    /// is on and this exact path is currently loaded - `None` otherwise
    /// (including for a real, loaded file, if retention isn't enabled).
    pub fn source(&self, path: &Path) -> Option<&str> {
        self.files.get(path)?.source.as_deref()
    }

    /// The AST of a currently-loaded file, if any - the position-walker's
    /// (`lsp::navigation`) own entry point.
    pub(crate) fn ast(&self, path: &Path) -> Option<&[Stmt]> {
        Some(&self.files.get(path)?.ast)
    }

    /// Parses one file and indexes its declarations into the shared
    /// registry. Call `finish_loading` once after the initial batch of
    /// `load_file` calls - trait flattening is a global pass over every
    /// class, so it's wasteful to re-run it after each individual file
    /// during a bulk load the way `update_file` has to for a single
    /// incremental change.
    #[hotpath::measure]
    pub fn load_file(&mut self, path: PathBuf, source: &str) -> Result<(), ParseError> {
        self.load_file_impl(path, source, false)
    }

    /// Same as `load_file`, but marks the file as a first-party,
    /// actually-analyzed one rather than context (`--scan-dir`/composer
    /// vendor) - see `Registry::insert_built_decl`'s own `analyzed`
    /// parameter doc comment for why this distinction has to reach all
    /// the way down to class indexing: an analyzed file's own declaration
    /// must always win a same-name collision outright, never merge with a
    /// pre-existing context/stub entry.
    pub fn load_analyzed_file(&mut self, path: PathBuf, source: &str) -> Result<(), ParseError> {
        self.load_file_impl(path, source, true)
    }

    fn load_file_impl(
        &mut self,
        path: PathBuf,
        source: &str,
        analyzed: bool,
    ) -> Result<(), ParseError> {
        // `is_stub: false` - `load_file`/`load_analyzed_file` are the
        // LSP/test-harness convenience path, which has no `stub_paths`
        // concept at all (that's a batch `check-project`-only config key,
        // wired through `read_and_parse_file` instead) and no way to
        // enable `check_duplicate_class_declarations` in the first place.
        let parsed = parse_file(&path, source.as_bytes(), false, &self.interner)?;
        self.insert_parsed_file(path, parsed, source.to_string(), analyzed);
        Ok(())
    }

    /// The order-sensitive second half of `load_file`, taking an
    /// already-computed `ParsedFile` (see that type's own doc comment) -
    /// the split exists so a caller loading many files at once (batch
    /// `check-project`) can compute every file's `ParsedFile` in parallel
    /// (pure, no `&self`/registry dependency at all) and then feed them
    /// into this method one at a time, sequentially, in whatever order
    /// its own collision-resolution policy needs - exactly the loop
    /// `load_file` above already ran, just no longer coupled to also
    /// doing the parse itself synchronously. `analyzed` has the same
    /// meaning as `load_analyzed_file`'s own - see that method's doc
    /// comment.
    pub fn insert_parsed_file(
        &mut self,
        path: PathBuf,
        parsed: ParsedFile,
        source: String,
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
            duplicate_fn_sites,
            duplicate_class_sites,
            bodies,
            class_fn_refs,
        } = parsed;
        self.begin_declaration_changes();
        for decl in built_decls {
            self.registry.insert_built_decl(decl, analyzed);
        }
        let class_fn_refs = class_fn_refs.unwrap_or_else(|| self.class_fn_refs_of(&ast, &source));
        for (span, target) in class_fn_refs {
            self.use_sites.insert(&path, target, span);
        }
        // `record_phpstan_class_file` no-ops unless the name is
        // `PHPStan\*`-namespaced, so this is a zero-cost loop for every
        // ordinary corpus.
        for name in &classes {
            self.registry.record_phpstan_class_file(name, &path);
        }
        self.duplicate_fn_sites.merge(duplicate_fn_sites);
        self.duplicate_class_sites.merge(duplicate_class_sites);
        register_references(&mut self.dependents, &path, &references);
        register_references(
            &mut self.signature_dependents,
            &path,
            &declaration_references,
        );
        let owner = self.packages.owner(&self.absolute_path(&path));
        self.files.insert(
            path,
            FileState {
                ast,
                mentions_phpstan: mentions_phpstan(&functions, &classes, &references),
                functions,
                classes,
                constants,
                references,
                declaration_references,
                ignores,
                anonymous_classes,
                source: self.retain_source.then_some(source),
                analyzed,
                owner,
                bodies,
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
    /// own wall time. Gated on `check_dead_code` (its only batch-mode
    /// consumer) for exactly that reason - a run with the check left
    /// off, the default, pays nothing extra here at all.
    fn class_fn_refs_of(&self, ast: &[Stmt], source: &str) -> Vec<(Span, use_sites::RefTarget)> {
        if self.retain_source {
            use_sites::index_class_fn_refs(ast, source)
        } else if self.settings.rules.check_dead_code {
            use_sites::index_class_fn_refs_without_source(ast)
        } else {
            Vec::new()
        }
    }

    /// Indexes `parsed`'s use sites ahead of `insert_parsed_file`, so a
    /// caller parsing many files in parallel indexes them there too.
    pub(crate) fn index_use_sites(&self, parsed: &mut ParsedFile, source: &str) {
        parsed.class_fn_refs = Some(self.class_fn_refs_of(&parsed.ast, source));
    }

    /// Reads, parses and indexes `files` in parallel on `pool`, then
    /// inserts them in order - context and analyzed alike (`analyzed`
    /// as `load_analyzed_file`'s). A file that can't be read is skipped;
    /// one that fails to parse is skipped and returned with its error.
    pub fn load_files(
        &mut self,
        files: &[(PathBuf, bool)],
        pool: Option<&rayon::ThreadPool>,
    ) -> Vec<(PathBuf, ParseError)> {
        let interner = Arc::clone(&self.interner);
        let this = &*self;
        let parsed = map_on_pool(pool, files, |(path, _)| {
            let bytes = fs::read(path).ok()?;
            let source = String::from_utf8_lossy(&bytes).into_owned();
            Some(
                parse_file(path, source.as_bytes(), false, &interner).map(|mut parsed| {
                    this.index_use_sites(&mut parsed, &source);
                    (source, parsed)
                }),
            )
        });
        let mut failed = Vec::new();
        for ((path, analyzed), parsed) in files.iter().zip(parsed) {
            match parsed {
                Some(Ok((source, parsed))) => {
                    self.insert_parsed_file(path.clone(), parsed, source, *analyzed)
                }
                Some(Err(e)) => failed.push((path.clone(), e)),
                None => {}
            }
        }
        failed
    }

    /// Merges PHPStan's own source in with the built-in stubs while some
    /// loaded file mentions a `PHPStan\` name, and takes it out again once
    /// none does: most projects never name it, and it is over half of the
    /// stub classes. The names it declares are rebuilt, from the stubs
    /// first and then every file declaring them, as `Project::new`'s merge
    /// and the loads after it would have built them.
    fn sync_phpstan_stubs(&mut self) {
        let wanted = self.files.values().any(|f| f.mentions_phpstan);
        if wanted == self.registry.phpstan_stubs_loaded {
            return;
        }
        self.registry.phpstan_stubs_loaded = wanted;
        let names = stubs::phpstan_stub_names();
        self.rebuild_declarations(
            &DeclaredNames {
                functions: names.functions,
                classes: names.classes,
                constants: names.constants,
            },
            None,
        );
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

    /// The classes and functions whose resolved form `changed` can have
    /// changed: the changed ones themselves, and every class and function
    /// declared where a name in the scope is referenced, transitively - in
    /// a loaded file or a built-in stub file.
    fn resolution_scope(&self, changed: &crate::registry::ChangedNames) -> Scope {
        let mut classes: FxHashSet<String> = changed.classes.clone();
        let mut functions: FxHashSet<String> = changed.functions.clone();
        let mut frontier: Vec<String> = classes
            .iter()
            .chain(&functions)
            .cloned()
            .chain(changed.constants.keys().map(|c| deps::dependency_key(c)))
            .collect();
        let mut seen: FxHashSet<String> = FxHashSet::default();
        while let Some(key) = frontier.pop() {
            if !seen.insert(key.clone()) {
                continue;
            }
            let mut reached = |name: &str, is_class: bool| {
                let key = deps::dependency_key(name);
                let set = if is_class {
                    &mut classes
                } else {
                    &mut functions
                };
                if set.insert(key.clone()) {
                    frontier.push(key);
                }
            };
            for file in self.dependents.get(&key).into_iter().flatten() {
                let Some(state) = self.files.get(file) else {
                    continue;
                };
                for name in state.classes.iter().chain(&state.anonymous_classes) {
                    reached(name, true);
                }
                for name in &state.functions {
                    reached(name, false);
                }
            }
            for (is_class, name) in stubs::stub_dependents(&key) {
                reached(name, *is_class);
            }
        }
        Scope::of(
            classes.iter().map(String::as_str),
            functions.iter().map(String::as_str),
        )
    }

    /// Finalizes an initial batch of `load_file` calls (trait flattening).
    #[hotpath::measure]
    pub fn finish_loading(&mut self) {
        worker_pool().install(|| self.run_finishing_passes());
    }

    /// Runs the finishing passes, returning the `deps::dependency_key`s of
    /// the classes, functions and constants whose resolved form they
    /// changed (none the first time, when everything is new).
    fn run_finishing_passes(&mut self) -> FxHashSet<String> {
        self.begin_declaration_changes();
        self.sync_phpstan_stubs();
        let (scope, changed) = if self.registry.has_resolved_declarations() {
            let changed = self.registry.take_changed();
            (self.resolution_scope(&changed), Some(changed))
        } else {
            (Scope::all(), None)
        };
        let before = changed
            .as_ref()
            .map(|_| self.registry.resolved_snapshot(&scope));
        self.registry
            .end_declaration_changes(&scope, !self.built_once);
        self.registry.set_scope(scope);
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
        // Adds more method/property names (mixin members) -
        // `resolve_transitive_purity` right after this also consumes
        // `lookup_method_with_owner`/`lookup_ancestor_method_with_owner`
        // internally, so the member-owner maps take in the mixin members
        // here, before it runs.
        self.registry.resolve_deferred_mixins();
        self.registry.precompute_member_owners_after_mixins();
        // Neither `resolve_transitive_purity` nor anything below it adds
        // or removes a method/property/const name anywhere, so the
        // member-owner maps built just above stay correct through this
        // point and for the checker's own later use - no third rebuild
        // needed.
        self.registry.resolve_transitive_purity(&self.files);
        self.registry.precompute_ancestor_names();
        self.registry.resolve_constant_named_types();
        self.registry.drop_doc_members_excluded_by_native();
        let files: Vec<(&Path, &[Stmt])> = self
            .files
            .iter()
            .map(|(p, f)| (p.as_path(), f.ast.as_slice()))
            .collect();
        self.registry.infer_global_variables(&files);
        self.registry.set_scope(Scope::all());
        self.rebuild_pest_config();
        match (before, changed) {
            (Some(before), Some(changed)) => self.registry.changed_since(before, &changed),
            _ => FxHashSet::default(),
        }
    }

    /// Re-parses one already-loaded file, patches just its own registry
    /// contributions and `dependents` entries, and re-checks it plus every
    /// other file that references a symbol it declared (before or after
    /// the change - a symbol the new version dropped still needs its old
    /// dependents re-checked, since they'll now see it as undefined).
    /// Returns every re-checked file's fresh diagnostics, keyed by path -
    /// the changed file is always included, even with zero diagnostics.
    /// Unwinds `path`'s prior contributions to the shared registry/
    /// `dependents` - the same undo both `update_file` (before
    /// re-indexing a new parse) and `remove_files` (permanently) need.
    /// Returns what the removed file was, or `None` when `path` wasn't
    /// loaded (a no-op - both callers treat that as "nothing to undo").
    fn unindex_file(&mut self, path: &Path) -> Option<UnindexedFile> {
        self.begin_declaration_changes();
        let old = self.files.remove(path)?;
        let classes: Vec<String> = old
            .classes
            .iter()
            .chain(&old.anonymous_classes)
            .cloned()
            .collect();
        self.registry.remove_declarations(&old.functions, &classes);
        self.use_sites.remove_file(path);
        self.duplicate_fn_sites
            .remove_declarations(path, &old.functions);
        self.duplicate_class_sites
            .remove_declarations(path, &old.classes);
        unregister_references(&mut self.dependents, path, &old.references);
        unregister_references(
            &mut self.signature_dependents,
            path,
            &old.declaration_references,
        );
        Some(UnindexedFile {
            analyzed: old.analyzed,
            functions: old.functions,
            classes: old.classes,
            constants: old.constants,
        })
    }

    /// Rebuilds the registry entries of `functions` and `classes` from
    /// every declaration of them still loaded: the built-in stub's first,
    /// as `Project::new` merges it, then each file's, context files before
    /// analyzed ones and each group by path - the order a project loads
    /// them in. `pending` is a file whose parse isn't in `self.files` yet;
    /// all of its declarations are inserted, at its place in that order.
    fn rebuild_declarations(
        &mut self,
        names: &DeclaredNames,
        pending: Option<(&Path, &[Stmt], bool)>,
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
        let constant_keys: FxHashSet<&str> = names.constants.iter().map(String::as_str).collect();
        let with_phpstan = self.registry.phpstan_stubs_loaded;
        for name in &constant_keys {
            self.registry.remove_global_constant(name);
        }
        for name in &function_keys {
            self.registry
                .merge_stubs(stubs::generated_stubs_named(name, with_phpstan));
        }
        for name in &class_keys {
            self.registry
                .merge_stub_classes(stubs::generated_stub_classes_named(name, with_phpstan));
        }
        for name in &constant_keys {
            self.registry
                .merge_stub_constants(stubs::generated_stub_constants_named(name, with_phpstan));
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
        let mut sources: Vec<(bool, &Path, &[Stmt], bool)> = self
            .files
            .iter()
            .filter(|(_, state)| declares_one(state))
            .map(|(path, state)| (state.analyzed, path.as_path(), state.ast.as_slice(), false))
            .collect();
        if let Some((path, ast, analyzed)) = pending {
            sources.push((analyzed, path, ast, true));
        }
        sources.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        for (analyzed, path, ast, is_pending) in sources {
            let origin = Origin::File(Arc::from(path));
            for decl in crate::registry::build_declarations(ast, &origin) {
                let rebuilt = match &decl {
                    BuiltDecl::Class { name, .. } => {
                        class_keys.contains(&name.to_ascii_lowercase())
                    }
                    BuiltDecl::Function { name, .. } => {
                        function_keys.contains(&name.to_ascii_lowercase())
                    }
                    BuiltDecl::GlobalConst { name, .. } => constant_keys.contains(name.as_str()),
                };
                if !rebuilt && !is_pending {
                    continue;
                }
                if let BuiltDecl::Class { name, .. } = &decl {
                    self.registry.record_phpstan_class_file(name, path);
                }
                self.registry.insert_built_decl(decl, analyzed);
            }
        }
    }

    /// Recomputes `path`'s own Tier-1 (class/function) use-sites from its
    /// currently-loaded `FileState`, replacing whatever was previously
    /// indexed for it - a no-op past the initial `use_sites.remove_file`
    /// when `retain_source` is off or `path` isn't currently loaded at
    /// all. Called for EVERY
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
        path: &Path,
        member_refs: Option<Vec<(Span, String, String)>>,
    ) {
        self.use_sites.remove_file(path);
        if self.retain_source
            && let Some(file) = self.files.get(path)
            && let Some(source) = &file.source
        {
            for (span, target) in use_sites::index_class_fn_refs(&file.ast, source) {
                self.use_sites.insert(path, target, span);
            }
        }
        if let Some(member_refs) = member_refs {
            for (span, owner, member) in member_refs {
                self.use_sites
                    .insert(path, use_sites::RefTarget::Member(owner, member), span);
            }
        }
    }

    /// Builds Tier 2 (member-reference) of `use_sites` for every
    /// currently-loaded file, exactly once - idempotent, a no-op on every
    /// call after the first. Deliberately NOT run as part of
    /// `finish_loading`/`load_workspace`: unlike Tier 1 (purely
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
        let paths: Vec<PathBuf> = self.files.keys().cloned().collect();
        for path in paths {
            let check = self.run_checker(&path, true, false);
            self.seed_member_refs(
                &path,
                check.member_refs,
                check.escaped_enums,
                check.escaped_member_classes,
            );
        }
        self.mark_member_index_built();
    }

    /// Adopts already-computed Tier-2 member-ref/escaped-enum/escaped-
    /// member-class data for `path` into `use_sites`/`escaped_enum_
    /// cases`/`escaped_member_classes` - the per-file work `ensure_
    /// member_index_built`'s own loop body does above, factored out so
    /// `check-project`'s own primary pass can feed it data it already
    /// computed inline (via `check_files`) instead of
    /// `ensure_member_index_built`
    /// re-deriving it via a second, redundant pass. Caller must call
    /// `mark_member_index_built` once every file has been seeded.
    pub fn seed_member_refs(
        &mut self,
        path: &Path,
        member_refs: Vec<(Span, String, String)>,
        escaped_enums: FxHashSet<String>,
        escaped_member_classes: FxHashSet<String>,
    ) {
        for (span, owner, member) in member_refs {
            self.use_sites
                .insert(path, use_sites::RefTarget::Member(owner, member), span);
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
        new_source: &str,
    ) -> Result<FxHashMap<PathBuf, Vec<Diagnostic>>, ParseError> {
        self.update_file_cancellable(path, new_source, || false)
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
        new_source: &str,
        mut should_abort: impl FnMut() -> bool,
    ) -> Result<FxHashMap<PathBuf, Vec<Diagnostic>>, ParseError> {
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
        let (ast, ignores, _) = parse_with_ignores(path, new_source.as_bytes(), &self.interner)?;
        let previous = self.unindex_file(path);
        let (functions, classes) = declared_names(&ast);
        let constants = declared_constants(&ast);
        let references = deps::referenced_names(&ast);
        let declaration_references = deps::declaration_referenced_names(&ast);

        // A file `update_file` has already seen keeps its own recorded
        // `analyzed` flag (an edit doesn't change what kind of file it
        // is). A never-before-seen path - reachable via `textDocument/
        // didOpen` for ANY URI the editor opens, including one
        // `discover_vendor_context`/`load_workspace`'s own excluded-
        // dir walk never touched (a vendor file outside any discovered
        // autoload entry, or with no composer.json at all) - defaults
        // to context (`analyzed: false`) when it's under a directory
        // name `DEFAULT_SCAN_EXCLUDED_DIR_NAMES` lists, matching
        // `load_workspace`'s own walk; otherwise `true`, the same "a
        // brand-new first-party file is analyzed by default" behavior
        // this had before this distinction existed. Without this, a
        // same-name collision (`insert_built_decl`'s own doc comment)
        // would let an opened-but-never-discovered vendor file outright
        // overwrite a real declaration instead of merging into it.
        let analyzed = previous.as_ref().map_or_else(
            || !path_under_excluded_dir(path, DEFAULT_SCAN_EXCLUDED_DIR_NAMES),
            |previous| previous.analyzed,
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
        self.rebuild_declarations(&rebuilt, Some((path, &ast, analyzed)));
        let anonymous_classes = anonymous_class_names(&ast);
        self.duplicate_fn_sites.index(&ast, path);
        // `is_stub: false` - same reasoning as `load_file_impl`'s own
        // identical note: `update_file` is the LSP/incremental-edit path,
        // which has no `stub_paths` concept.
        self.duplicate_class_sites.index(&ast, path, false);
        let bodies = FileBodies::of(&ast);
        register_references(&mut self.dependents, path, &references);
        register_references(
            &mut self.signature_dependents,
            path,
            &declaration_references,
        );
        self.files.insert(
            path.to_path_buf(),
            FileState {
                ast,
                mentions_phpstan: mentions_phpstan(&functions, &classes, &references),
                functions,
                classes,
                constants,
                references,
                declaration_references,
                ignores,
                anonymous_classes,
                source: self.retain_source.then(|| new_source.to_string()),
                analyzed,
                owner: self.packages.owner(&self.absolute_path(path)),
                bodies,
            },
        );
        let changed = worker_pool().install(|| self.run_finishing_passes());

        let mut results = FxHashMap::default();
        let mut affected = self.affected_files(changed);
        affected.insert(path.to_path_buf());
        self.unchecked.clear();
        let mut affected = affected.into_iter();
        while let Some(p) = affected.next() {
            if should_abort() {
                self.unchecked.push(p);
                self.unchecked.extend(affected);
                break;
            }
            let (diagnostics, member_refs) = if self.member_index_built {
                let (d, m) = self.check_file_and_member_refs(&p);
                (d, Some(m))
            } else {
                (self.check_file(&p), None)
            };
            self.reindex_use_sites_for_file(&p, member_refs);
            results.insert(p, diagnostics);
        }
        Ok(results)
    }

    /// The files the last cancelled `update_file_cancellable` affected but
    /// never rechecked; their previous diagnostics no longer hold.
    pub fn take_unchecked(&mut self) -> Vec<PathBuf> {
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
    /// `path` itself is always a key in the returned map, with an empty `Vec` when `path` was never loaded in the first
    /// place - a harmless no-op a caller doesn't need to guard with a
    /// prior loaded-state check, and exactly the empty
    /// `publishDiagnostics` payload needed to clear whatever the editor
    /// was last showing for a file that no longer exists.
    pub fn remove_file(&mut self, path: &Path) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        self.remove_files(std::slice::from_ref(&path.to_path_buf()))
    }

    /// Batch counterpart to `remove_file` - unwinds every one of `paths`
    /// (each via `unindex_file`) and runs the shared registry recompute
    /// (`finish_loading`) at most ONCE for the whole batch, not once per
    /// path - the same "one recompute per batch, not per item" discipline
    /// `Server::add_workspace_folders` (3.2) needs on the load side.
    /// Every one of `paths` is always a key in the result (generalizing
    /// `remove_file`'s own guarantee - `check_file` already returns an
    /// empty `Vec` for an unloaded/just-removed path, exactly the
    /// clearing `publishDiagnostics` payload a caller needs for a file
    /// that no longer exists), even when none of them were actually
    /// loaded - but the recompute/BFS itself is skipped entirely in that
    /// case, since there is nothing for it to discover.
    pub fn remove_files(&mut self, paths: &[PathBuf]) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
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
        let mut affected: FxHashSet<PathBuf> = paths.iter().cloned().collect();
        if any_removed {
            self.rebuild_declarations(&removed, None);
            let changed = worker_pool().install(|| self.run_finishing_passes());
            affected.extend(self.affected_files(changed));
        }

        affected
            .into_iter()
            .map(|p| {
                let (diagnostics, member_refs) = if self.member_index_built {
                    let (d, m) = self.check_file_and_member_refs(&p);
                    (d, Some(m))
                } else {
                    (self.check_file(&p), None)
                };
                self.reindex_use_sites_for_file(&p, member_refs);
                (p, diagnostics)
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
    fn affected_files(&self, changed: FxHashSet<String>) -> FxHashSet<PathBuf> {
        let mut affected = FxHashSet::default();
        let mut seen: FxHashSet<String> = FxHashSet::default();
        let mut frontier: Vec<String> = changed.into_iter().collect();
        while let Some(key) = frontier.pop() {
            if !seen.insert(key.clone()) {
                continue;
            }
            affected.extend(self.dependents.get(&key).into_iter().flatten().cloned());
            for file in self.signature_dependents.get(&key).into_iter().flatten() {
                let Some(state) = self.files.get(file) else {
                    continue;
                };
                frontier.extend(
                    state
                        .functions
                        .iter()
                        .chain(&state.classes)
                        .map(|name| deps::dependency_key(name)),
                );
            }
            frontier.extend(
                stubs::stub_dependents(&key)
                    .iter()
                    .map(|(_, name)| deps::dependency_key(name)),
            );
        }
        affected
    }

    /// The checking settings, to change directly.
    #[cfg(test)]
    pub(crate) fn check_settings_mut(&mut self) -> &mut CheckSettings {
        &mut self.settings
    }

    /// `path`'s type dump: every statement's recorded variable types.
    #[cfg(test)]
    pub(crate) fn type_dump(&self, path: &Path) -> Vec<String> {
        match self
            .check_files(&[path.to_path_buf()], None, false, true)
            .pop()
        {
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
        self.check_one(path, false).diagnostics
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
        path: &Path,
    ) -> (Vec<Diagnostic>, Vec<(Span, String, String)>) {
        let check = self.check_one(path, true);
        (check.diagnostics, check.member_refs)
    }

    /// `check_files` for one file, whose panic propagates.
    fn check_one(&self, path: &Path, collect_member_refs: bool) -> FileCheck {
        let paths = [path.to_path_buf()];
        let mut checks = self.check_files(&paths, None, collect_member_refs, false);
        match checks.pop() {
            Some((_, Ok(check))) => check,
            Some((_, Err(message))) => panic!("{message}"),
            None => FileCheck::empty(),
        }
    }

    /// Checks `paths` (in parallel on `pool` when there is one), each
    /// file's panic caught as its outcome. With `collect_member_refs`, the
    /// Tier-2 member-reference data too (fed to `seed_member_refs`); with
    /// `dump_types`, the file's `--dump-types` lines. The private property
    /// types each file declares are judged once every file is checked,
    /// against the writes all of them made plus those of any other file
    /// writing the same properties (`judge_property_types`).
    pub(crate) fn check_files(
        &self,
        paths: &[PathBuf],
        pool: Option<&rayon::ThreadPool>,
        collect_member_refs: bool,
        dump_types: bool,
    ) -> Vec<(PathBuf, CheckOutcome)> {
        let mut checks = map_on_pool(pool, paths, |path| {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.run_checker(path, collect_member_refs, dump_types)
            }))
            .map_err(panic_message);
            (path.clone(), outcome)
        });
        self.judge_property_types(&mut checks, pool);
        for (path, outcome) in &mut checks {
            if let Ok(check) = outcome {
                let diagnostics = std::mem::take(&mut check.diagnostics);
                check.diagnostics = self.finish_diagnostics(path, diagnostics);
            }
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
        checks: &mut [(PathBuf, CheckOutcome)],
        pool: Option<&rayon::ThreadPool>,
    ) {
        let mut writers: FxHashSet<Name> = FxHashSet::default();
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
        let mut scopes: FxHashSet<Name> = FxHashSet::default();
        let mut stack: Vec<Name> = writers.iter().cloned().collect();
        while let Some(class) = stack.pop() {
            if let Some(info) = self.registry.classes.get(class.as_ref()) {
                stack.extend(info.uses().iter().cloned());
            }
            scopes.insert(class);
        }
        let checked: FxHashSet<&PathBuf> = checks.iter().map(|(path, _)| path).collect();
        let mut others: Vec<PathBuf> = self
            .files
            .iter()
            .filter(|(path, file)| {
                !checked.contains(path)
                    && file
                        .classes
                        .iter()
                        .any(|class| scopes.contains(class.as_str()))
            })
            .map(|(path, _)| path.clone())
            .collect();
        others.sort();
        let other_facts = map_on_pool(pool, &others, |path| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.run_checker(path, false, false).property_assign_facts
            }))
            .ok()
        });
        let mut sources: Vec<(&PathBuf, PropertyAssignFacts)> = others
            .iter()
            .zip(other_facts)
            .filter_map(|(path, facts)| Some((path, facts?)))
            .collect();
        for (path, outcome) in checks.iter_mut() {
            if let Ok(check) = outcome {
                sources.push((path, std::mem::take(&mut check.property_assign_facts)));
            }
        }
        sources.sort_by(|a, b| a.0.cmp(b.0));
        let mut merged: PropertyAssignFacts = FxHashMap::default();
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
        for (_, outcome) in checks.iter_mut() {
            if let Ok(check) = outcome {
                let sites = std::mem::take(&mut check.property_type_sites);
                check
                    .diagnostics
                    .extend(checker::check_too_wide_property_types(
                        &sites, &merged, &relation,
                    ));
            }
        }
    }

    /// A bare, un-run `Checker` for `path` plus its AST, for an LSP
    /// feature that needs to drive `check_program` itself with extra
    /// collection flags set first (e.g. `textDocument/inlayHint`'s
    /// `collect_inlay_hints`) rather than through `check_files`'
    /// fixed diagnostic pipeline. Mirrors `run_checker`'s setup exactly.
    pub(crate) fn checker_for(&self, path: &Path) -> Option<(Checker<'_>, &[Stmt])> {
        let file = self.files.get(path)?;
        let mut checker = Checker::new(&self.registry)
            .with_sql(&self.sql)
            .with_settings(&self.settings)
            .with_bodies(&self.files);
        checker.set_pest_this_type(self.pest_config.resolve_this_type(path, &self.registry));
        checker.set_pest_hook_properties(self.pest_config.resolve_hook_properties(path));
        checker.set_current_file(path);
        checker.set_current_file_absolute(self.absolute_path(path));
        checker.set_working_dir(self.working_dir.clone());
        Some((checker, &file.ast))
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
        checker.check_program(ast);
        checker.take_semantic_tokens()
    }

    /// Runs the checker over `path`, leaving its private property types
    /// unjudged and its diagnostics unfiltered (`finish_diagnostics`).
    fn run_checker(&self, path: &Path, collect_member_refs: bool, dump_types: bool) -> FileCheck {
        #[cfg(test)]
        tests::panic_injection::panic_if_injected(path);
        let Some((mut checker, ast)) = self.checker_for(path) else {
            return FileCheck::empty();
        };
        checker.set_defer_property_type_checks(true);
        checker.set_collect_member_refs(collect_member_refs);
        checker.set_dump_types(dump_types);
        checker.check_program(ast);
        if collect_member_refs {
            checker.collect_default_and_const_value_member_refs(ast);
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

    /// `path`'s checker `diagnostics` with the project-level ones added,
    /// sorted, and its inline ignore comments applied.
    fn finish_diagnostics(&self, path: &Path, mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
        let Some(file) = self.files.get(path) else {
            return diagnostics;
        };
        if self.settings.rules.check_duplicate_function_declarations {
            diagnostics.extend(duplicate_functions::check_duplicate_function_declarations(
                path,
                &self.duplicate_fn_sites,
            ));
        }
        if self.settings.rules.check_duplicate_class_declarations {
            diagnostics.extend(duplicate_classes::check_duplicate_class_declarations(
                path,
                &self.duplicate_class_sites,
            ));
        }
        for (line, message) in &file.ignores.parse_errors {
            let at = syntax::token::Position {
                line: *line,
                col: 1,
            };
            diagnostics.push(
                diagnostics::DiagnosticBuilder::new(
                    format!("Parse error in @phpstan-ignore: {message}"),
                    Span { start: at, end: at },
                    0,
                    "ignore.parseError",
                )
                .non_ignorable()
                .build(),
            );
        }
        diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.col));
        if self.settings.phpstan_api {
            diagnostics.extend(checker::phpstan_api::check_phpstan_api_usage(
                &file.ast,
                &self.registry,
                Some(self.package_name(path)),
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
        // `@phpstan-ignore-line`/`-next-line` - `non_ignorable`
        // diagnostics are exempt, mirroring real PHPStan's own identical
        // exemption (reserved for a handful of its own rules, e.g.
        // flagging a malformed `@phpstan-ignore` comment itself -
        // nonsensical to let another ignore comment suppress).
        if !self.defer_inline_ignores && !file.ignores.lines.is_empty() {
            diagnostics
                .retain(|d| d.non_ignorable || !file.ignores.lines.contains(&d.span.start.line));
        }
        // The fuller bare `@phpstan-ignore <identifier list>` form -
        // unlike the two line markers above, this only suppresses a
        // diagnostic whose own `identifier` exactly matches one named on
        // that line.
        if !self.defer_inline_ignores && !file.ignores.identifiers.is_empty() {
            diagnostics.retain(|d| {
                d.non_ignorable
                    || !file
                        .ignores
                        .identifiers
                        .get(&d.span.start.line)
                        .is_some_and(|ids| ids.iter().any(|id| **id == *d.identifier))
            });
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
    /// loaded), on `pool`. Panic-transparent - a real checker panic in
    /// `paths` propagates to the caller, which is what a unit test wants;
    /// see `check_paths_isolating_panics` for the batch-safe counterpart.
    pub fn check_paths(
        &self,
        paths: &[PathBuf],
        pool: &rayon::ThreadPool,
    ) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        self.check_files(paths, Some(pool), false, false)
            .into_iter()
            .map(|(path, outcome)| match outcome {
                Ok(check) => (path, check.diagnostics),
                Err(message) => panic!("checking {} panicked: {message}", path.display()),
            })
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
        let outcomes = self.check_files(paths, Some(pool), false, false);
        std::panic::set_hook(default_hook);

        let mut results = FxHashMap::default();
        let mut panics = Vec::new();
        for (path, outcome) in outcomes {
            match outcome {
                Ok(check) => {
                    results.insert(path, check.diagnostics);
                }
                Err(msg) => panics.push((path, msg)),
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
    pub fn check_all(&self, pool: &rayon::ThreadPool) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        self.check_paths(&self.files.keys().cloned().collect::<Vec<_>>(), pool)
    }

    /// Resolves one whole-program finding's own reportable location and
    /// applies the same two things every per-file diagnostic already
    /// gets for free, that a diagnostic built entirely outside
    /// `check_files` would otherwise never see at all: "only report
    /// first-party files, never vendor/context ones swept in only
    /// because the registry records every loaded file's declarations, analyzed or
    /// not" (a real bug on an earlier pass of this exact method - a
    /// vendor-only dead enum case used to create a brand new, never-
    /// otherwise-checked entry in `check-project`'s own `results` map),
    /// and `@phpstan-ignore-line`/`-next-line`/`@phpstan-ignore
    /// <identifiers>` suppression (`finish_diagnostics`' own identical
    /// `retain` calls only ever see the per-file diagnostics). `None` for an unloaded/context path, or one whose own
    /// ignore-comment already covers `identifier` on `span`'s line.
    fn whole_program_diagnostic(
        &self,
        path: &Path,
        span: Span,
        message: String,
        identifier: &'static str,
    ) -> Option<Diagnostic> {
        let file = self.files.get(path)?;
        if !file.analyzed {
            return None;
        }
        if !self.defer_inline_ignores && file.ignores.lines.contains(&span.start.line)
            || !self.defer_inline_ignores
                && file
                    .ignores
                    .identifiers
                    .get(&span.start.line)
                    .is_some_and(|ids| ids.iter().any(|id| id == identifier))
        {
            return None;
        }
        Some(DiagnosticBuilder::new(message, span, 5, identifier).build())
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
    /// loaded-file cost, so `Project::insert_parsed_file` itself gates
    /// populating it in batch mode on this exact flag - see that call
    /// site's own doc comment. `check_circular_dependencies` and
    /// `check_dead_abstract_classes` need neither Tier - both read
    /// directly off `Registry.classes`' own already-resolved fields, so
    /// each costs nothing beyond its own traversal regardless of how
    /// large the project is.
    pub(crate) fn check_whole_program(
        &mut self,
        named_argument_calls: &NamedArgumentCallFactsWithPath,
    ) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        let mut results: FxHashMap<PathBuf, Vec<Diagnostic>> = FxHashMap::default();

        if self.settings.rules.check_dead_enum_cases {
            self.ensure_member_index_built();
            let dead = whole_program::dead_enum_cases::find_dead_cases(
                &self.registry,
                &self.use_sites,
                &self.escaped_enum_cases,
            );
            for (fqn, case) in dead {
                let Some((path, span)) = self.registry.constant_declaration(&fqn, &case) else {
                    continue;
                };
                let path = path.to_path_buf();
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    span,
                    format!("Enum case {fqn}::{case} is never used."),
                    "enumCase.unused",
                ) {
                    results.entry(path).or_default().push(d);
                }
            }
        }

        if self.settings.rules.check_dead_code {
            let dead_functions =
                whole_program::dead_code::find_dead_functions(&self.registry, &self.use_sites);
            for name in dead_functions {
                let Some((path, span)) = self.registry.function_declaration(&name) else {
                    continue;
                };
                let path = path.to_path_buf();
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    span,
                    format!("Function {name} is never referenced."),
                    "function.unused",
                ) {
                    results.entry(path).or_default().push(d);
                }
            }
            let dead_classes =
                whole_program::dead_code::find_dead_classes(&self.registry, &self.use_sites);
            for name in dead_classes {
                let Some((path, span)) = self.registry.class_declaration(&name) else {
                    continue;
                };
                let path = path.to_path_buf();
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    span,
                    format!("Class {name} is never referenced."),
                    "class.unused",
                ) {
                    results.entry(path).or_default().push(d);
                }
            }
        }

        if self.settings.rules.check_circular_dependencies {
            let cycles =
                whole_program::circular_namespace_deps::find_namespace_cycles(&self.registry);
            for cycle in cycles {
                let Some((path, span)) =
                    self.registry.class_declaration(&cycle.representative_class)
                else {
                    continue;
                };
                let path = path.to_path_buf();
                let namespaces = cycle.namespaces.join(", ");
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    span,
                    format!("Circular namespace dependency: {namespaces}."),
                    "namespace.circularDependency",
                ) {
                    results.entry(path).or_default().push(d);
                }
            }
        }

        if self.settings.rules.check_dead_abstract_classes {
            let dead_abstract_classes =
                whole_program::dead_abstract_classes::find_dead_abstract_classes(&self.registry);
            for name in dead_abstract_classes {
                let Some((path, span)) = self.registry.class_declaration(&name) else {
                    continue;
                };
                let path = path.to_path_buf();
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    span,
                    format!(
                        "Abstract class {name} has no concrete subclass anywhere in the project."
                    ),
                    "class.deadAbstract",
                ) {
                    results.entry(path).or_default().push(d);
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
                let Some((path, span)) = declaration else {
                    continue;
                };
                let path = path.to_path_buf();
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
                    &path,
                    span,
                    format!("{noun} {owner}::{name} is never used."),
                    identifier,
                ) {
                    results.entry(path).or_default().push(d);
                }
            }
        }

        if self.settings.rules.check_renamed_named_arguments {
            let mut programs: Vec<(&PathBuf, &[Stmt])> = self
                .files
                .iter()
                .map(|(path, file)| (path, file.ast.as_slice()))
                .collect();
            programs.sort_by(|a, b| a.0.cmp(b.0));
            let overrides = whole_program::renamed_named_arguments::renamed_parameter_overrides(
                &self.registry,
                programs.into_iter().map(|(_, ast)| ast),
            );
            let collisions =
                whole_program::renamed_named_arguments::find_renamed_named_argument_collisions(
                    named_argument_calls,
                    &overrides,
                );
            for (path, span, message) in collisions {
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    span,
                    message,
                    "argument.parameterRenamedInSubtype",
                ) {
                    results.entry(path).or_default().push(d);
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

    /// `true` if `path` is loaded and analyzed; `false` for an unloaded
    /// path or one loaded only as context.
    pub fn is_analyzed(&self, path: &Path) -> bool {
        self.files.get(path).is_some_and(|state| state.analyzed)
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

/// First and last line of every trait declared in `ast`.
fn trait_line_ranges(ast: &[Stmt]) -> Vec<(u32, u32)> {
    let mut ranges = Vec::new();
    for stmt in ast {
        syntax::visit::any_stmt_in_body(stmt, |s| {
            if let StmtKind::ClassDecl(c) = &s.kind
                && c.def.kind == syntax::ast::ClassKind::Trait
            {
                ranges.push((s.span.start.line, s.span.end.line));
            }
            false
        });
    }
    ranges
}

/// Parses `source`, with its inline ignore comments.
type Parsed = (Vec<Stmt>, ignore_comments::IgnoreComments, Vec<LexWarning>);

fn parse_with_ignores(
    path: &Path,
    source: &[u8],
    interner: &Arc<Interner>,
) -> Result<Parsed, ParseError> {
    let parsed = syntax::parse(source, interner)?;
    let ignores = ignore_comments::scan_parsed(&parsed);
    let mut ast = parsed.stmts;
    crate::stub_attributes::apply_stub_attributes(&mut ast);
    crate::registry::name_anonymous_classes(&mut ast, &path.display().to_string());
    Ok((ast, ignores, parsed.warnings))
}

/// Everything about loading one file that's pure - parsing, plus every
/// fact derived only from the parsed AST or the raw source text, with no
/// dependency on (or mutation of) a `Project`'s shared registry. Computing
/// this for many files is embarrassingly parallel - `insert_parsed_file`
/// below does the remaining, order-sensitive registry-mutating half,
/// meant to stay sequential and run in whatever order a caller's own
/// collision-resolution policy needs (see that method's own doc comment).
pub struct ParsedFile {
    ast: Vec<Stmt>,
    // What the lexer skipped over (see `ParsedFile::lex_warnings`).
    lex_warnings: Vec<LexWarning>,
    functions: Vec<String>,
    classes: Vec<String>,
    constants: Vec<String>,
    references: FxHashSet<String>,
    declaration_references: FxHashSet<String>,
    ignores: ignore_comments::IgnoreComments,
    // This file's own declarations, already built into
    // registry-independent `ClassReflection`/`FunctionReflection`/`Type` values (see
    // `registry::build_declarations`'s own doc comment for why that
    // split is pure and safe to compute here, alongside everything else
    // in this struct) - `insert_parsed_file` feeds these into
    // `Registry::insert_built_decl` one at a time instead of re-deriving
    // them from `ast` via `Registry::index`, so the expensive "turn an
    // AST node into a ClassReflection/FunctionReflection" work runs here, in
    // parallel, rather than in the sequential insert loop.
    built_decls: Vec<crate::registry::BuiltDecl>,
    // This file's own top-level function declaration sites, already
    // built (see `DuplicateFunctionSites::merge`'s own doc comment) -
    // `insert_parsed_file` merges these into the project-wide table
    // instead of re-walking `ast` via `DuplicateFunctionSites::index`,
    // moving the per-function type-hint cloning `record` does into this
    // already-parallel step. Unlike `built_decls`, this needs `path`
    // (baked into every `FnDeclSite`), which is why `parse_file` below
    // takes one now.
    duplicate_fn_sites: DuplicateFunctionSites,
    // The registry names of the file's anonymous classes.
    anonymous_classes: Vec<String>,
    // The class-like counterpart to `duplicate_fn_sites` above, built the
    // same way and for the same reason - `parse_file`'s own `is_stub`
    // parameter is baked into every `ClassDeclSite` here (see that
    // field's own doc comment for why it's needed at all).
    duplicate_class_sites: DuplicateClassSites,
    bodies: FileBodies,
    // The file's class and function use sites, when indexed alongside the
    // parse (`Project::index_use_sites`); `insert_parsed_file` indexes
    // them itself otherwise.
    class_fn_refs: Option<Vec<(Span, use_sites::RefTarget)>>,
}

/// Computes a `ParsedFile` from `source` - see that type's own doc
/// comment. Safe to call concurrently for many files at once (no shared
/// state touched at all). `path` is used only to tag
/// `duplicate_fn_sites`'s own records (see `ParsedFile`'s doc comment on
/// that field) - never read to make any decision, so it's fine that a
/// caller building many `ParsedFile`s concurrently passes a different
/// `path` to each call with no coordination needed between them.
#[hotpath::measure]
pub fn parse_file(
    path: &Path,
    source: &[u8],
    is_stub: bool,
    interner: &Arc<Interner>,
) -> Result<ParsedFile, ParseError> {
    let (ast, ignores, lex_warnings) = parse_with_ignores(path, source, interner)?;
    let (functions, classes) = declared_names(&ast);
    let constants = declared_constants(&ast);
    let references = deps::referenced_names(&ast);
    let declaration_references = deps::declaration_referenced_names(&ast);
    let origin = Origin::File(Arc::from(path));
    let built_decls = crate::registry::build_declarations(&ast, &origin);
    let anonymous_classes = anonymous_class_names(&ast);
    let mut duplicate_fn_sites = DuplicateFunctionSites::default();
    duplicate_fn_sites.index(&ast, path);
    let mut duplicate_class_sites = DuplicateClassSites::default();
    duplicate_class_sites.index(&ast, path, is_stub);
    let bodies = FileBodies::of(&ast);
    Ok(ParsedFile {
        ast,
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
        class_fn_refs: None,
    })
}

/// The registry names of the anonymous classes in `ast`.
fn anonymous_class_names(ast: &[Stmt]) -> Vec<String> {
    struct Names(Vec<String>);
    impl<'a> syntax::visit::Visitor<'a> for Names {
        fn visit_expr(&mut self, expr: &'a syntax::ast::Expr) {
            if let syntax::ast::ExprKind::NewAnonymousClass(class) = &expr.kind
                && let Some(name) = &class.name
            {
                self.0.push(name.to_string());
            }
            syntax::visit::walk_expr(self, expr);
        }
    }
    let mut names = Names(Vec::new());
    for stmt in ast {
        syntax::visit::Visitor::visit_stmt(&mut names, stmt);
    }
    names.0
}

impl ParsedFile {
    /// What the lexer skipped over rather than failing on.
    pub fn lex_warnings(&self) -> &[LexWarning] {
        &self.lex_warnings
    }
}

/// Recursively collects every `.php` file under `path` - shared by the CLI
/// (`main.rs`'s own subcommands) and the LSP's own workspace-loading path
/// (`lsp.rs`'s `load_workspace`). Deliberately NOT `#[hotpath::measure]`d
/// itself - it's self-recursive (one call per directory in the tree), and
/// hotpath's per-call "Total" sums every call's own measured span, so a
/// recursive function's aggregate massively over-counts its true unique
/// wall-clock contribution (a parent directory's own span already
/// contains every descendant's separately-measured span too). `check_
/// project.rs`'s `collect_analyze_and_context_files` wraps the *whole*
/// top-level scan in one non-recursive, correctly-measured call instead -
/// see that function's own doc comment for the real number this found.
pub fn collect_php_files(path: &Path, out: &mut Vec<PathBuf>) {
    collect_php_files_excluding(path, &[], out);
}

/// Directory names a workspace-wide scan skips by default when no real
/// config (1.1's own `phpstan-rs.toml` `paths`) says otherwise - a
/// hardcoded floor, not a substitute for real exclusion config. Matches
/// exactly one path component, not a prefix (`vendor-utils/` is
/// unaffected).
pub const DEFAULT_SCAN_EXCLUDED_DIR_NAMES: &[&str] = &["vendor", ".git", "node_modules"];

/// `true` if any component of `path` is named exactly one of
/// `excluded_dir_names` - a pure name check, no filesystem access. Used
/// both to prune `collect_php_files_excluding`'s own walk and, in
/// `Project::update_file`, as a never-before-seen path's default
/// `analyzed` classification (see that call site's own doc comment).
pub fn path_under_excluded_dir(path: &Path, excluded_dir_names: &[&str]) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| excluded_dir_names.contains(&s))
    })
}

/// Same as `collect_php_files`, but prunes any directory whose name
/// appears in `excluded_dir_names` before recursing into it - see
/// `DEFAULT_SCAN_EXCLUDED_DIR_NAMES`.
pub fn collect_php_files_excluding(
    path: &Path,
    excluded_dir_names: &[&str],
    out: &mut Vec<PathBuf>,
) {
    if path.is_dir() {
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| excluded_dir_names.contains(&n))
        {
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            collect_php_files_excluding(&entry.path(), excluded_dir_names, out);
        }
    } else if path.extension().is_some_and(|e| e == "php") {
        out.push(path.to_path_buf());
    }
}

/// What `Project::unindex_file` removed.
struct UnindexedFile {
    analyzed: bool,
    functions: Vec<String>,
    classes: Vec<String>,
    constants: Vec<String>,
}

/// Whether a file declaring `functions` and `classes` and referencing
/// `references` (`deps::dependency_key`s) names anything in `PHPStan\`.
fn mentions_phpstan(
    functions: &[String],
    classes: &[String],
    references: &FxHashSet<String>,
) -> bool {
    let is_phpstan = |key: &str| key.starts_with("phpstan\\");
    references.iter().any(|r| is_phpstan(r))
        || functions
            .iter()
            .chain(classes)
            .any(|n| is_phpstan(&deps::dependency_key(n)))
}

/// Names a file declares, by kind.
#[derive(Default)]
struct DeclaredNames {
    functions: Vec<String>,
    classes: Vec<String>,
    constants: Vec<String>,
}

/// The global constants one file's parse tree declares, by the names
/// `Registry::global_constants` holds them under.
fn declared_constants(stmts: &[Stmt]) -> Vec<String> {
    let mut constants = Vec::new();
    syntax::declarations::for_each_declaration(stmts, |declaration, _| {
        if let Declared::Constant(_) | Declared::Define { .. } = declaration.declared {
            constants.push(declaration.name().trim_start_matches('\\').to_string());
        }
    });
    constants
}

/// The function and class names one file's parse tree declares - exactly
/// what a registry entry needs to be identified by to be removed again
/// later. Enums share the registry's `classes` map (see
/// `Registry::index`), so they're collected into the same list here.
/// Walks the declarations `registry::build_declarations` builds entries
/// from, so an incremental update's removal step finds every key the
/// registry holds.
pub(crate) fn declared_names(stmts: &[Stmt]) -> (Vec<String>, Vec<String>) {
    let mut functions = Vec::new();
    let mut classes = Vec::new();
    syntax::declarations::for_each_declaration(stmts, |declaration, _| {
        match declaration.declared {
            Declared::Function(_) => functions.push(declaration.name()),
            Declared::Class(_) => classes.push(declaration.name()),
            Declared::Constant(_) | Declared::Define { .. } => {}
        }
    });
    (functions, classes)
}

fn register_references(
    dependents: &mut FxHashMap<String, FxHashSet<PathBuf>>,
    path: &Path,
    references: &FxHashSet<String>,
) {
    for name in references {
        dependents
            .entry(name.clone())
            .or_default()
            .insert(path.to_path_buf());
    }
}

fn unregister_references(
    dependents: &mut FxHashMap<String, FxHashSet<PathBuf>>,
    path: &Path,
    references: &FxHashSet<String>,
) {
    for name in references {
        if let Some(files) = dependents.get_mut(name) {
            files.remove(path);
            if files.is_empty() {
                dependents.remove(name);
            }
        }
    }
}

#[cfg(test)]
mod tests;
