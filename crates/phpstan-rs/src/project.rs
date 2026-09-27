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
use crate::php_stubs;
use crate::registry::Registry;
use diagnostics::{self, Diagnostic, DiagnosticBuilder};
use syntax::ast::{ClassKind, Stmt, StmtKind, Visibility};
use syntax::lexer::{LexWarning, Lexer};
use syntax::name::Interner;
use syntax::name_resolution::NameContext;
use syntax::parser::{ParseError, Parser};
use syntax::token::Span;
use whole_program::renamed_named_arguments::NamedArgumentCallFactsWithPath;

mod architecture;
mod banned;
mod class_shape;
mod composer_hygiene;
mod decl_locations;
pub(crate) mod duplicate_class_sites;
mod duplicate_classes;
pub(crate) mod duplicate_fn_sites;
mod duplicate_functions;
mod mock_hygiene;
pub mod session;
pub(crate) mod use_sites;
mod whole_program;

use decl_locations::DeclLocations;
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
    references: FxHashSet<String>,
    // Inline `@phpstan-ignore*` comments, read while the file was lexed
    // (the AST carries no comment text).
    ignores: ignore_comments::IgnoreComments,
    // See `ParsedFile::anonymous_class_names`'s own doc comment - carried
    // here too so `Project::check_file_impl` can hand it to a fresh
    // `Checker` on every re-check, not just the initial load.
    anonymous_class_names: FxHashMap<syntax::token::Span, String>,
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
}

/// A multi-file PHP project: every loaded file's own parse tree, one
/// registry merged across all of them (so a class in one file can extend a
/// class declared in another), and a reverse index - `dependents[name]` is
/// every file that *references* `name` - used to find which other files
/// need re-checking when a file's declarations change.
///
/// `load_file` is for the initial, full load of a codebase; `update_file`
/// is the incremental path: re-parsing and re-checking only the file that
/// changed, plus the *transitive* closure of every file that references a
/// symbol it declares, or references a symbol declared by one of those
/// files, and so on (see `transitive_affected_files`) - every other file's
/// parse tree and registry entries are untouched.
///
/// What this does NOT do: narrow by *which* aspect of a symbol changed -
/// any file reachable through that chain gets re-checked, even if the
/// specific thing it uses didn't change. That's a safe over-approximation
/// (it costs an occasional extra re-check, never a missed one) and
/// straightforward to tighten later if the extra re-checking ever matters
/// in practice.
pub struct Project {
    files: FxHashMap<PathBuf, FileState>,
    // What the names in this project's parsed files are interned into,
    // freed with the project.
    interner: Arc<Interner>,
    dependents: FxHashMap<String, FxHashSet<PathBuf>>,
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
    // See `DeclLocations`'s own doc comment. Populated/pruned alongside
    // `registry.index`/`remove_declarations` in `load_file`/
    // `update_file`, unconditionally - unlike `FileState.source`/Tier-1
    // `use_sites`, this needs no retained source text at all (`index`
    // takes only `&ast`/`&path`), so there's no reason to gate it behind
    // `retain_source` too. Was gated behind it until `check_whole_
    // program` (`Project`'s own doc comment) started needing a batch
    // `check-project` run to resolve a whole-program fact's own
    // declaring file/span - the "a batch run never does a position- or
    // name-based lookup" assumption this comment used to make no longer
    // holds.
    decl_locations: DeclLocations,
    // Every top-level function's own unconditional declaration site(s) -
    // always populated (no `retain_source` gate, same as `decl_locations`
    // above), the same indexing-side/check-side split
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
    // unconditionally, unlike `decl_locations` above: measured on a real
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
    // above, just for Phase 86's B1 (`whole_program::
    // dead_members::find_dead_members`) instead of Phase 92.
    escaped_member_classes: FxHashSet<String>,
}

/// The stack each checking thread gets: the checker's recursive walk, and
/// in a debug build the parser's precedence-climbing chain, need far more
/// than a default thread's ~2 MiB.
pub const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;

/// A pool of one `WORKER_STACK_SIZE` thread per core. A process builds one
/// and runs every parallel parse and check on it: the global rayon pool's
/// threads have too little stack for the checker.
pub fn worker_pool() -> Arc<rayon::ThreadPool> {
    Arc::new(
        rayon::ThreadPoolBuilder::new()
            .stack_size(WORKER_STACK_SIZE)
            .build()
            .expect("failed to build the worker pool"),
    )
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

    pub fn new() -> Self {
        let mut registry = Registry::default();
        registry.merge_stubs(php_stubs::generated_stubs());
        registry.merge_stub_classes(php_stubs::generated_stub_classes());
        registry.merge_stub_constants(php_stubs::generated_stub_constants());
        Project {
            files: FxHashMap::default(),
            interner: Arc::new(Interner::new()),
            dependents: FxHashMap::default(),
            registry,
            sql: sql::context::SqlContext::default(),
            pest_config: PestConfig::default(),
            working_dir: None,
            absolute_root: None,
            packages: composer::PackageIndex::default(),
            settings: CheckSettings::default(),
            defer_inline_ignores: false,
            retain_source: false,
            decl_locations: DeclLocations::default(),
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

    /// See `DeclLocations`'s own doc comment.
    pub(crate) fn decl_locations(&self) -> &DeclLocations {
        &self.decl_locations
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
        let parsed = parse_file(&path, source, false, &self.interner)?;
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
            references,
            ignores,
            built_decls,
            anonymous_class_decls,
            anonymous_class_names,
            duplicate_fn_sites,
            duplicate_class_sites,
        } = parsed;
        for decl in built_decls.into_iter().chain(anonymous_class_decls) {
            self.registry.insert_built_decl(decl, analyzed);
        }
        self.decl_locations
            .index(&ast, &mut NameContext::default(), &path);
        // Real source text (`retain_source`) buys narrower per-identifier
        // spans (`use_sites::index_class_fn_refs`'s own doc comment) - a
        // batch run with no retained source can still get real, just less
        // precisely-scoped, entries via `index_class_fn_refs_without_
        // source`, which `whole_program::dead_code` (existence-
        // only, never reads a use-site's span) can't tell apart from the
        // precise version at all. But unlike `decl_locations` above, this
        // walk is NOT cheap enough to run unconditionally in batch mode -
        // measured on a real corpus (Piwigo, ~14k loaded files including
        // vendor context): running it for every loaded file regardless of
        // whether anything consumes it roughly doubled `check-project`'s
        // own wall time. Gated on `check_dead_code` (its only batch-mode
        // consumer) for exactly that reason - a run with the check left
        // off, the default, pays nothing extra here at all.
        let class_fn_refs = if self.retain_source {
            use_sites::index_class_fn_refs(&ast, &source)
        } else if self.settings.rules.check_dead_code {
            use_sites::index_class_fn_refs_without_source(&ast)
        } else {
            Vec::new()
        };
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
        let owner = self.packages.owner(&self.absolute_path(&path));
        self.files.insert(
            path,
            FileState {
                ast,
                functions,
                classes,
                references,
                ignores,
                anonymous_class_names,
                source: self.retain_source.then_some(source),
                analyzed,
                owner,
            },
        );
    }

    /// Finalizes an initial batch of `load_file` calls (trait flattening).
    #[hotpath::measure]
    pub fn finish_loading(&mut self) {
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
        // internally, so the member-owner maps need a second rebuild
        // here, before it runs, not just once at the very end.
        self.registry.resolve_deferred_mixins();
        self.registry.precompute_member_owners();
        // Neither `resolve_transitive_purity` nor anything below it adds
        // or removes a method/property/const name anywhere, so the
        // member-owner maps built just above stay correct through this
        // point and for the checker's own later use - no third rebuild
        // needed.
        self.registry.resolve_transitive_purity();
        self.registry.precompute_ancestor_names();
        self.registry.resolve_constant_named_types();
        self.registry.drop_doc_members_excluded_by_native();
        let files: Vec<(&Path, &[Stmt])> = self
            .files
            .iter()
            .map(|(p, f)| (p.as_path(), f.ast.as_slice()))
            .collect();
        self.registry.infer_global_variables(&files);
        self.rebuild_pest_config();
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
    /// re-indexing a new parse) and `remove_files` (permanently) need -
    /// extending `changed_symbols` with every bare name it owned.
    /// Returns whether `path` was actually loaded (a no-op, `false`
    /// return otherwise - both callers treat that as "nothing to undo").
    fn unindex_file(&mut self, path: &Path, changed_symbols: &mut FxHashSet<String>) -> bool {
        let Some(old) = self.files.remove(path) else {
            return false;
        };
        self.registry
            .remove_declarations(&old.functions, &old.classes);
        self.decl_locations
            .remove_declarations(&old.functions, &old.classes);
        self.use_sites.remove_file(path);
        self.duplicate_fn_sites
            .remove_declarations(path, &old.functions);
        self.duplicate_class_sites
            .remove_declarations(path, &old.classes);
        changed_symbols.extend(old.functions.iter().map(|s| bare_name(s).to_string()));
        changed_symbols.extend(old.classes.iter().map(|s| bare_name(s).to_string()));
        unregister_references(&mut self.dependents, path, &old.references);
        true
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
            let check = self.check_file_impl(&path, None, true, false);
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
    /// computed inline (via `check_analysed_file`) instead of
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
        // case), this must leave every existing registry/decl_locations/
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
        let (ast, ignores, _) = parse_with_ignores(new_source, &self.interner)?;
        let mut changed_symbols: FxHashSet<String> = FxHashSet::default();
        self.unindex_file(path, &mut changed_symbols);
        let (functions, classes) = declared_names(&ast);
        let references = deps::referenced_names(&ast);
        // `changed_symbols` seeds transitive_affected_files' BFS, which
        // looks names up in `self.dependents` - keyed by the bare names
        // deps::referenced_names collects (a deliberate simplification,
        // see that function's own doc comment), not the fully-qualified
        // ones `functions`/`classes` carry (needed for correct Registry
        // removal above). Feeding FQNs into a bare-keyed lookup would
        // silently find nothing for any namespaced class, so editing a
        // widely-depended-on namespaced class wouldn't cascade to any of
        // its real dependents at all.
        changed_symbols.extend(functions.iter().map(|s| bare_name(s).to_string()));
        changed_symbols.extend(classes.iter().map(|s| bare_name(s).to_string()));

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
        let analyzed = match self.files.get(path) {
            Some(existing) => existing.analyzed,
            None => !path_under_excluded_dir(path, DEFAULT_SCAN_EXCLUDED_DIR_NAMES),
        };
        self.registry.index(&ast, analyzed);
        // Best-effort, not full incremental invalidation: a prior version
        // of this file's own anonymous-class entries (registered under
        // synthetic names keyed by `path:line[:index]`) aren't removed
        // here first - `functions`/`classes` above (what `remove_
        // declarations` purges) never included them to begin with, since
        // `declared_names` only sees top-level declarations. A stale
        // entry from a since-deleted-or-moved anonymous class can
        // therefore linger in the registry across edits; harmless (an
        // orphaned class nothing references), just imprecise, the same
        // tradeoff this path already accepts elsewhere.
        let (anonymous_class_decls, anonymous_class_names) =
            crate::registry::collect_anonymous_classes(&ast, &path.display().to_string());
        for decl in anonymous_class_decls {
            self.registry.insert_built_decl(decl, analyzed);
        }
        for name in &classes {
            self.registry.record_phpstan_class_file(name, path);
        }
        self.decl_locations
            .index(&ast, &mut NameContext::default(), path);
        self.duplicate_fn_sites
            .index(&ast, &mut NameContext::default(), path, false);
        // `is_stub: false` - same reasoning as `load_file_impl`'s own
        // identical note: `update_file` is the LSP/incremental-edit path,
        // which has no `stub_paths` concept.
        self.duplicate_class_sites
            .index(&ast, &mut NameContext::default(), path, false, false);
        // A changed class's `use TraitName;` list (added, removed, or a
        // trait's own members changed in this same file) means the whole
        // registry's trait flattening can't be assumed still correct -
        // re-run it. Still far cheaper than a full reload: a HashMap scan
        // over already-parsed ClassInfo, not a re-parse of anything.
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
        // `resolve_transitive_purity` right after this also consumes a
        // lookup internally, so the member-owner maps need a second
        // rebuild here, before it runs - neither it nor anything below
        // it adds/removes a name, so this second rebuild is also the
        // final, correct one for the checker's own later use.
        self.registry.resolve_deferred_mixins();
        self.registry.precompute_member_owners();
        self.registry.resolve_transitive_purity();
        self.registry.precompute_ancestor_names();
        self.registry.resolve_constant_named_types();
        self.registry.drop_doc_members_excluded_by_native();
        {
            let mut files: Vec<(&Path, &[Stmt])> = self
                .files
                .iter()
                .map(|(p, f)| (p.as_path(), f.ast.as_slice()))
                .collect();
            files.push((path, ast.as_slice()));
            self.registry.infer_global_variables(&files);
        }
        register_references(&mut self.dependents, path, &references);

        self.files.insert(
            path.to_path_buf(),
            FileState {
                ast,
                functions,
                classes,
                references,
                ignores,
                anonymous_class_names,
                source: self.retain_source.then(|| new_source.to_string()),
                analyzed,
                owner: self.packages.owner(&self.absolute_path(path)),
            },
        );
        self.rebuild_pest_config();

        let mut results = FxHashMap::default();
        for p in self.transitive_affected_files(path, changed_symbols) {
            if should_abort() {
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

    /// Unwinds `path`'s prior contributions to the shared registry/
    /// `dependents` - the same undo `update_file` performs before
    /// re-indexing a new parse - but with no re-index step afterward:
    /// `path` is gone, not changed. Reruns `update_file`'s own
    /// finishing-pass sequence, then re-checks the transitive closure of
    /// files that referenced any declaration `path` used to own, via the
    /// same `transitive_affected_files` + `check_file` pipeline
    /// `update_file` already uses.
    ///
    /// KNOWN LIMITATION (confirmed empirically, not assumed): the
    /// finishing-pass rerun below only ever recomputes registry-wide
    /// derived state off each *still-loaded* class's own, unchanged
    /// declaration - it never reparses any file besides `path` itself.
    /// So a still-loaded class that names a just-removed one directly
    /// (`class Child extends Base {}`, removing `base.php`) stays stale:
    /// `class_extends_implements_closure` reads `Child`'s own `extends`
    /// field verbatim and records "Base" as an ancestor unconditionally,
    /// with no check that "Base" is still a real `self.classes` entry,
    /// so `is_subclass_of("Child", "Base")` incorrectly stays `true`.
    /// What the rebuild *does* fix is a relationship that was only ever
    /// transitively true *through* the removed class: for `Grandchild
    /// extends Child extends Base` (three files), removing `child.php`
    /// correctly flips `is_subclass_of("Grandchild", "Base")` to `false`
    /// (the closure can no longer look `self.classes["Child"]` up to
    /// walk past it to "Base"), even though
    /// `is_subclass_of("Grandchild", "Child")` itself stays stale for
    /// the identical direct-reference reason as the `Child`/`Base` case.
    /// The same staleness class also covers a trait's or `@mixin`'s
    /// members that some OTHER, still-loaded class already baked into
    /// its own persistent `ClassInfo.methods`/`properties` (via
    /// `flatten_traits`/`resolve_deferred_mixins`'s
    /// `Entry::Vacant`-gated `or_insert` - additive-only, no provenance
    /// tracking, nothing in the registry ever clears/resets a class's own
    /// baked-in member set). This is a pre-existing gap `update_file`
    /// already has today for the identical reason (editing a trait to
    /// drop a method, or a class to drop `extends`, doesn't unstick an
    /// existing user/subclass either, unless that file is also
    /// reparsed) - not a regression this introduces, and closing it
    /// needs own-vs-baked provenance tracking on `ClassInfo`, a
    /// separate, larger registry-architecture change.
    ///
    /// `path` itself is always a key in the returned map
    /// (`transitive_affected_files` unconditionally seeds `affected` with
    /// it), with an empty `Vec` when `path` was never loaded in the first
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
        let mut changed_symbols: FxHashSet<String> = FxHashSet::default();
        let mut any_removed = false;
        for path in paths {
            any_removed |= self.unindex_file(path, &mut changed_symbols);
        }
        let mut affected: FxHashSet<PathBuf> = paths.iter().cloned().collect();
        if any_removed {
            self.finish_loading();
            affected.extend(self.transitive_affected_files_from(changed_symbols));
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

    /// The full, transitive closure of files that need re-checking after
    /// `changed_file`'s declared symbols became `seed_symbols` (its own
    /// old+new declared names) - not just the files that directly
    /// reference one of those symbols, but the fixed point of "declares ->
    /// is referenced by -> declares -> is referenced by -> ...".
    ///
    /// This is what makes invalidation correct across inheritance chains:
    /// if file A references class B (`function f(B $b) { ... $b->x ... }`)
    /// and B `extends` class C declared in a third file, a change to C's
    /// shape can change what A's use of `$b->x` type-checks to, even
    /// though A's own source never mentions C by name. A one-hop lookup
    /// from C's dependents would find B but stop there and miss A; this
    /// keeps expanding the frontier through every newly-affected file's
    /// own declarations until nothing new turns up.
    fn transitive_affected_files(
        &self,
        changed_file: &Path,
        seed_symbols: FxHashSet<String>,
    ) -> FxHashSet<PathBuf> {
        let mut affected = self.transitive_affected_files_from(seed_symbols);
        affected.insert(changed_file.to_path_buf());
        affected
    }

    /// The pure BFS core of `transitive_affected_files`, without that
    /// method's own anchor-file insertion - for a caller with no single
    /// surviving "the file that changed" to seed with (`remove_files`'s
    /// batch of files that no longer exist to check).
    fn transitive_affected_files_from(
        &self,
        seed_symbols: FxHashSet<String>,
    ) -> FxHashSet<PathBuf> {
        let mut affected = FxHashSet::default();

        let mut seen_symbols = FxHashSet::default();
        let mut frontier: Vec<String> = Vec::new();
        for symbol in seed_symbols {
            if seen_symbols.insert(symbol.clone()) {
                frontier.push(symbol);
            }
        }

        while let Some(symbol) = frontier.pop() {
            let Some(files) = self.dependents.get(&symbol) else {
                continue;
            };
            for file in files {
                if !affected.insert(file.clone()) {
                    continue;
                }
                // `file` is newly affected - its own effective shape may
                // have just changed too (e.g. it `extends` the symbol that
                // changed), so anything depending on what *it* declares
                // needs to be considered next.
                let Some(state) = self.files.get(file) else {
                    continue;
                };
                // Bare, not FQN - see update_file's identical note on
                // changed_symbols; this frontier feeds the same
                // bare-keyed `self.dependents` lookup above.
                for name in state.functions.iter().chain(state.classes.iter()) {
                    let bare = bare_name(name).to_string();
                    if seen_symbols.insert(bare.clone()) {
                        frontier.push(bare);
                    }
                }
            }
        }

        affected
    }

    /// Type-checks one already-loaded file against the project's current
    /// registry, returning its diagnostics. Same-file property-too-wide-
    /// type facts only (see `Checker::cross_file_property_assign_facts`'s
    /// own doc comment) - correct for incremental/LSP use (a single-file
    /// re-check has no cheap way to also re-derive every other file's own
    /// contribution), and for every caller that doesn't need the full
    /// two-pass project-wide merge. `check_file_with_property_assign_facts`
    /// is the batch counterpart that does.
    #[hotpath::measure]
    pub fn check_file(&self, path: &Path) -> Vec<Diagnostic> {
        self.check_file_impl(path, None, false, false).diagnostics
    }

    /// The Tier-2 (member-reference) counterpart of `check_file` - the
    /// SAME `Checker`/`check_program` pass already used for diagnostics
    /// also collects member-reference sites, rather than paying for a
    /// second, independent `Checker` pass over the same file. Only
    /// `update_file`'s own affected-file loop calls this, and only once
    /// `member_index_built` is true - see `ensure_member_index_built`'s
    /// own doc comment for why a file's own use-sites have no cross-file
    /// ordering dependency the way `property_assign_facts` does, so no
    /// separate first pass is needed here the way it is there.
    pub(crate) fn check_file_and_member_refs(
        &self,
        path: &Path,
    ) -> (Vec<Diagnostic>, Vec<(Span, String, String)>) {
        let check = self.check_file_impl(path, None, true, false);
        (check.diagnostics, check.member_refs)
    }

    /// The two-pass counterpart of `check_file` - `facts` is the whole-
    /// project merge `collect_property_assign_facts` computed from a
    /// prior pass over every loaded file, letting a trait-donated
    /// property write discovered while checking a *different* file
    /// reach this file's own `check_too_wide_property_types` call.
    #[hotpath::measure]
    pub fn check_file_with_property_assign_facts(
        &self,
        path: &Path,
        facts: &Arc<PropertyAssignFacts>,
    ) -> Vec<Diagnostic> {
        self.check_file_impl(path, Some(facts), false, false)
            .diagnostics
    }

    /// The session's main-pass check of an analysed file, with the
    /// project-wide property-assign `facts`: its diagnostics and the data
    /// the whole-program checks read afterwards. With
    /// `collect_member_refs`, the Tier-2 member-reference data too (fed to
    /// `seed_member_refs`), so no second pass is needed for it; with
    /// `dump_types`, the file's `--dump-types` lines.
    pub(crate) fn check_analysed_file(
        &self,
        path: &Path,
        facts: &Arc<PropertyAssignFacts>,
        collect_member_refs: bool,
        dump_types: bool,
    ) -> FileCheck {
        self.check_file_impl(path, Some(facts), collect_member_refs, dump_types)
    }

    /// A bare, un-run `Checker` for `path` plus its AST, for an LSP
    /// feature that needs to drive `check_program` itself with extra
    /// collection flags set first (e.g. `textDocument/inlayHint`'s
    /// `collect_inlay_hints`) rather than through `check_file_impl`'s own
    /// fixed diagnostic pipeline. Mirrors `check_file_impl`'s Pest/
    /// anonymous-class setup exactly, minus the cross-file property-
    /// assign-facts wiring (an LSP one-off request has no batch-wide
    /// facts pass to plug in).
    pub(crate) fn checker_for(&self, path: &Path) -> Option<(Checker<'_>, &[Stmt])> {
        let file = self.files.get(path)?;
        let mut checker = Checker::new(&self.registry)
            .with_sql(&self.sql)
            .with_settings(&self.settings);
        checker.set_pest_this_type(self.pest_config.resolve_this_type(path, &self.registry));
        checker.set_pest_hook_properties(self.pest_config.resolve_hook_properties(path));
        checker.set_anonymous_class_names(file.anonymous_class_names.clone());
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

    fn check_file_impl(
        &self,
        path: &Path,
        cross_file_property_assign_facts: Option<&Arc<PropertyAssignFacts>>,
        collect_member_refs: bool,
        dump_types: bool,
    ) -> FileCheck {
        let Some(file) = self.files.get(path) else {
            return FileCheck {
                diagnostics: Vec::new(),
                member_refs: Vec::new(),
                escaped_enums: FxHashSet::default(),
                escaped_member_classes: FxHashSet::default(),
                named_argument_calls: FxHashMap::default(),
                type_dump: Vec::new(),
            };
        };
        let mut checker = Checker::new(&self.registry)
            .with_sql(&self.sql)
            .with_settings(&self.settings);
        checker.set_pest_this_type(self.pest_config.resolve_this_type(path, &self.registry));
        checker.set_pest_hook_properties(self.pest_config.resolve_hook_properties(path));
        checker.set_anonymous_class_names(file.anonymous_class_names.clone());
        checker.set_current_file(path);
        checker.set_current_file_absolute(self.absolute_path(path));
        checker.set_working_dir(self.working_dir.clone());
        if let Some(facts) = cross_file_property_assign_facts {
            checker.set_cross_file_property_assign_facts(Arc::clone(facts));
        }
        checker.set_collect_member_refs(collect_member_refs);
        checker.set_dump_types(dump_types);
        checker.check_program(&file.ast);
        if collect_member_refs {
            checker.collect_default_and_const_value_member_refs(&file.ast);
        }
        let member_refs = checker.take_member_ref_sites();
        let escaped_enums = checker.take_escaped_enums();
        let escaped_member_classes = checker.take_escaped_member_classes();
        let named_argument_calls = checker.take_named_argument_call_facts();
        let type_dump = checker.take_type_dump();
        let mut diagnostics = checker.into_diagnostics();
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
        FileCheck {
            diagnostics,
            member_refs,
            escaped_enums,
            escaped_member_classes,
            named_argument_calls,
            type_dump,
        }
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

    /// A file whose classes (including anonymous ones) can structurally
    /// never contribute a `property_assign_facts` entry is safe to skip -
    /// `Checker::check_property_assignment` only records a `$this->prop
    /// = expr` write when `prop_sig.write_visibility ==
    /// Visibility::Private` (see that function's own gate). Skipping such
    /// a file lets `collect_property_assign_facts` avoid constructing a
    /// `Checker` and running the full diagnostic-producing
    /// `check_program` pass on it purely to discover nothing.
    ///
    /// Two things a naive "does this class declare a private property"
    /// check would miss:
    /// - Anonymous classes: checked via `file.anonymous_class_names` too,
    ///   not just `file.classes` - an anonymous class's own
    ///   private-property writes are processed the same way during the
    ///   same file's `check_program` walk (via `check_expr`'s
    ///   `NewAnonymousClass` handling).
    /// - Traits: a trait's own methods are re-checked once per *real
    ///   host* that uses it (`Checker::check_trait_methods`), with that
    ///   host's own class context - a `$this->prop = ...` write inside a
    ///   trait method can assign to a *host's* private property even
    ///   though the trait itself declares none. A file declaring a trait
    ///   is only skippable if none of its (transitively resolved) real
    ///   hosts have a private property either.
    ///
    /// `Registry::classes`' own `ClassInfo::properties` map is already
    /// trait-flattened by the time this runs (`Project::finish_loading`
    /// flattens traits before `collect_property_assign_facts` is ever
    /// called), so a plain lookup here is sound.
    fn file_may_write_private_property(&self, file: &FileState) -> bool {
        fn has_private_property(info: &crate::registry::ClassInfo) -> bool {
            info.properties
                .values()
                .any(|p| p.write_visibility == Visibility::Private)
        }
        let class_may_write = |name: &str| {
            let Some(info) = self.registry.classes.get(name) else {
                return false;
            };
            if has_private_property(info) {
                return true;
            }
            info.kind == ClassKind::Trait
                && self.registry.classes_using_trait(name).iter().any(|host| {
                    self.registry
                        .classes
                        .get(host)
                        .is_some_and(has_private_property)
                })
        };
        file.classes.iter().any(|name| class_may_write(name))
            || file
                .anonymous_class_names
                .values()
                .any(|name| class_may_write(name))
    }

    /// The collect half of the two-pass (collect-then-consume) checking
    /// architecture: runs the checker once over every currently-loaded
    /// file that may write a private property, purely to gather
    /// `Checker::property_assign_facts` (this pass's own diagnostics are
    /// discarded - `check_file`/`check_file_with_property_assign_facts`
    /// are the real, diagnostic-producing pass), then merges every file's
    /// own per-fqn/per-property contribution into one whole-project map.
    /// A trait re-checked once per host (`check_trait_methods`) already
    /// attributes its own contribution to each host's real fqn (see
    /// `property_assign_facts`'s own doc comment), so merging here needs
    /// no extra trait-awareness of its own - just concatenating every
    /// file's own per-key `Vec`.
    ///
    /// Files are checked in parallel on `pool` and merged in path order,
    /// so the result doesn't depend on scheduling or on how the file map
    /// was built. A file whose check panics contributes nothing; the
    /// others keep their facts.
    pub fn collect_property_assign_facts(&self, pool: &rayon::ThreadPool) -> PropertyAssignFacts {
        let mut files: Vec<(&PathBuf, &FileState)> = self
            .files
            .iter()
            .filter(|(_, file)| self.file_may_write_private_property(file))
            .collect();
        files.sort_by(|a, b| a.0.cmp(b.0));
        let per_file: Vec<Option<PropertyAssignFacts>> = pool.install(|| {
            files
                .par_iter()
                .map(|(path, file)| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        self.file_property_assign_facts(path, file)
                    }))
                    .ok()
                })
                .collect()
        });
        let mut merged: PropertyAssignFacts = FxHashMap::default();
        for facts in per_file.into_iter().flatten() {
            for (fqn, props) in facts {
                let entry = merged.entry(fqn).or_default();
                for (prop, mut types) in props {
                    entry.entry(prop).or_default().append(&mut types);
                }
            }
        }
        merged
    }

    /// One file's contribution to `collect_property_assign_facts`.
    fn file_property_assign_facts(&self, path: &Path, file: &FileState) -> PropertyAssignFacts {
        #[cfg(test)]
        tests::panic_injection::panic_if_injected(path);
        #[cfg(not(test))]
        let _ = path;
        let mut checker = Checker::new(&self.registry)
            .with_sql(&self.sql)
            .with_settings(&self.settings);
        checker.set_facts_only(true);
        checker.check_program(&file.ast);
        checker.take_property_assign_facts()
    }

    /// Runs the checker over exactly `paths` (which must already be
    /// loaded) - the two-pass `collect_property_assign_facts`-then-
    /// `check_file_with_property_assign_facts` architecture `check_all`
    /// uses, but scoped to a caller-chosen subset rather than every
    /// loaded file. The facts pass itself still runs over every loaded
    /// file regardless of `paths` (see `collect_property_assign_facts`'s
    /// own doc comment for why: a trait-donated property write
    /// discovered in a file outside `paths` must still reach a host
    /// inside it). The facts pass runs on `pool`. Panic-transparent - a
    /// real checker panic in `paths` propagates to the caller, which is
    /// what a unit test wants; see `check_paths_isolating_panics` for the
    /// batch-safe counterpart.
    pub fn check_paths(
        &self,
        paths: &[PathBuf],
        pool: &rayon::ThreadPool,
    ) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        let facts = Arc::new(self.collect_property_assign_facts(pool));
        paths
            .iter()
            .map(|path| {
                (
                    path.clone(),
                    self.check_file_with_property_assign_facts(path, &facts),
                )
            })
            .collect()
    }

    /// Same as `check_paths`, but a panic while checking one file (or
    /// while collecting property-assign facts) is caught and reported
    /// rather than aborting every other file's result - mirrors
    /// `main/check_project.rs`'s own catch_unwind-with-silenced-hook
    /// pattern exactly, for the same reason: a whole-workspace pass runs
    /// over uncurated code the caller doesn't control, where one file's
    /// latent checker bug must not take down every other file's
    /// diagnostics. `Registry.sql_reflector`'s only interior mutability is
    /// a plain `OnceLock` - if its lazy-init closure panics mid-connection
    /// attempt, the cell is left simply uninitialized (not corrupted;
    /// `OnceLock::get_or_init`'s own contract), so a later observer sees
    /// either "not yet initialized" or a real, valid reflector - never a
    /// torn/partial one. `AssertUnwindSafe` here is that specific, checked
    /// claim, not a blanket "trust me" (matches `check_project.rs`'s own
    /// identical reasoning). The paths are checked in parallel on `pool`.
    pub fn check_paths_isolating_panics(
        &self,
        paths: &[PathBuf],
        pool: &rayon::ThreadPool,
    ) -> CheckPathsIsolatingPanics {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let facts = Arc::new(self.collect_property_assign_facts(pool));
        let outcomes = pool.install(|| self.check_paths_catching_panics(paths, &facts));
        std::panic::set_hook(default_hook);

        let mut results = FxHashMap::default();
        let mut panics = Vec::new();
        for (path, outcome) in outcomes {
            match outcome {
                Ok(diagnostics) => {
                    results.insert(path, diagnostics);
                }
                Err(msg) => panics.push((path, msg)),
            }
        }
        panics.sort();
        (results, panics)
    }

    /// `check_paths_isolating_panics`' per-path checks, run on its pool.
    fn check_paths_catching_panics(
        &self,
        paths: &[PathBuf],
        facts: &Arc<PropertyAssignFacts>,
    ) -> Vec<(PathBuf, Result<Vec<Diagnostic>, String>)> {
        paths
            .par_iter()
            .map(|path| {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.check_file_with_property_assign_facts(path, facts)
                }))
                .map_err(|payload| {
                    payload
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "unknown panic".to_string())
                });
                (path.clone(), outcome)
            })
            .collect()
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
    /// `check_file_impl` would otherwise never see at all: "only report
    /// first-party files, never vendor/context ones swept in only
    /// because `decl_locations` indexes every loaded file, analyzed or
    /// not" (a real bug on an earlier pass of this exact method - a
    /// vendor-only dead enum case used to create a brand new, never-
    /// otherwise-checked entry in `check-project`'s own `results` map),
    /// and `@phpstan-ignore-line`/`-next-line`/`@phpstan-ignore
    /// <identifiers>` suppression (`check_file_impl`'s own identical
    /// `retain` calls only ever run *inside* the per-file `Checker`
    /// pass). `None` for an unloaded/context path, or one whose own
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
                let Some((path, span)) = self
                    .decl_locations
                    .members
                    .get(&(fqn.clone(), case.clone()))
                else {
                    continue;
                };
                let path = path.clone();
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    *span,
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
                let Some((path, span)) = self.decl_locations.functions.get(&name) else {
                    continue;
                };
                let path = path.clone();
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    *span,
                    format!("Function {name} is never referenced."),
                    "function.unused",
                ) {
                    results.entry(path).or_default().push(d);
                }
            }
            let dead_classes =
                whole_program::dead_code::find_dead_classes(&self.registry, &self.use_sites);
            for name in dead_classes {
                let Some((path, span)) = self.decl_locations.classes.get(&name) else {
                    continue;
                };
                let path = path.clone();
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    *span,
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
                    self.decl_locations.classes.get(&cycle.representative_class)
                else {
                    continue;
                };
                let path = path.clone();
                let namespaces = cycle.namespaces.join(", ");
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    *span,
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
                let Some((path, span)) = self.decl_locations.classes.get(&name) else {
                    continue;
                };
                let path = path.clone();
                if let Some(d) = self.whole_program_diagnostic(
                    &path,
                    *span,
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
                let Some((path, span)) = self
                    .decl_locations
                    .members
                    .get(&(owner.clone(), name.clone()))
                else {
                    continue;
                };
                let path = path.clone();
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
                    *span,
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
    let mut stack: Vec<&Stmt> = ast.iter().collect();
    while let Some(stmt) = stack.pop() {
        match &stmt.kind {
            StmtKind::ClassDecl(c) if c.kind == syntax::ast::ClassKind::Trait => {
                ranges.push((stmt.span.start.line, stmt.span.end.line));
            }
            StmtKind::ClassDecl(_) | StmtKind::FunctionDecl(_) | StmtKind::EnumDecl(_) => {}
            _ => stack.extend(syntax::visit::child_stmts(stmt)),
        }
    }
    ranges
}

/// Parses `source`, reading its inline ignore comments from the same
/// lexing.
type Parsed = (Vec<Stmt>, ignore_comments::IgnoreComments, Vec<LexWarning>);

fn parse_with_ignores(source: &str, interner: &Arc<Interner>) -> Result<Parsed, ParseError> {
    let (tokens, comments, warnings) = Lexer::new(source)
        .with_interner(interner)
        .tokenize_with_comments();
    let ignores = ignore_comments::scan_tokens(&tokens, &comments);
    let mut ast = Parser::new(tokens)
        .with_interner(interner)
        .parse_program()?;
    crate::stub_attributes::apply_stub_attributes(&mut ast);
    Ok((ast, ignores, warnings))
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
    references: FxHashSet<String>,
    ignores: ignore_comments::IgnoreComments,
    // This file's own declarations, already built into
    // registry-independent `ClassInfo`/`FunctionSig`/`Type` values (see
    // `registry::build_declarations`'s own doc comment for why that
    // split is pure and safe to compute here, alongside everything else
    // in this struct) - `insert_parsed_file` feeds these into
    // `Registry::insert_built_decl` one at a time instead of re-deriving
    // them from `ast` via `Registry::index`, so the expensive "turn an
    // AST node into a ClassInfo/FunctionSig" work runs here, in
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
    // Every `new class { ... }` expression this file contains, already
    // built into `BuiltDecl::Class`es under their own stable synthetic
    // names (see `registry::collect_anonymous_classes`'s own doc comment)
    // - merged into `built_decls` by `insert_parsed_file` the same way an
    // ordinary class is, just computed as a separate pass since it needs
    // `path` (to seed the name hash) the same way `duplicate_fn_sites`
    // does.
    anonymous_class_decls: Vec<crate::registry::BuiltDecl>,
    // `NewAnonymousClass` expression span -> that same synthetic name,
    // so `Checker`'s own inference site can look its type up without
    // recomputing the name a second time (see the module doc comment on
    // `registry::anonymous_class` for why that matters). Carried on
    // `FileState` (below) rather than only used transiently here, since
    // `check_file_impl` needs it again on every re-check.
    anonymous_class_names: FxHashMap<syntax::token::Span, String>,
    // The class-like counterpart to `duplicate_fn_sites` above, built the
    // same way and for the same reason - `parse_file`'s own `is_stub`
    // parameter is baked into every `ClassDeclSite` here (see that
    // field's own doc comment for why it's needed at all).
    duplicate_class_sites: DuplicateClassSites,
}

/// Computes a `ParsedFile` from `source` - see that type's own doc
/// comment. Safe to call concurrently for many files at once (no shared
/// state touched at all); still runs under the lexer's own, separate,
/// still-panic-based error handling (unlike the parser, which no longer
/// panics on ordinary malformed syntax - see `main/check.rs`'s identical
/// note), so a caller parsing many files in parallel should still wrap
/// each call in `catch_unwind`, the same precaution `check.rs`'s own
/// lex+parse worker threads already take. `path` is used only to tag
/// `duplicate_fn_sites`'s own records (see `ParsedFile`'s doc comment on
/// that field) - never read to make any decision, so it's fine that a
/// caller building many `ParsedFile`s concurrently passes a different
/// `path` to each call with no coordination needed between them.
#[hotpath::measure]
pub fn parse_file(
    path: &Path,
    source: &str,
    is_stub: bool,
    interner: &Arc<Interner>,
) -> Result<ParsedFile, ParseError> {
    let (ast, ignores, lex_warnings) = parse_with_ignores(source, interner)?;
    let (functions, classes) = declared_names(&ast);
    let references = deps::referenced_names(&ast);
    let built_decls = crate::registry::build_declarations(&ast);
    let (anonymous_class_decls, anonymous_class_names) =
        crate::registry::collect_anonymous_classes(&ast, &path.display().to_string());
    let mut duplicate_fn_sites = DuplicateFunctionSites::default();
    duplicate_fn_sites.index(&ast, &mut NameContext::default(), path, false);
    let mut duplicate_class_sites = DuplicateClassSites::default();
    duplicate_class_sites.index(&ast, &mut NameContext::default(), path, false, is_stub);
    Ok(ParsedFile {
        ast,
        lex_warnings,
        functions,
        classes,
        references,
        built_decls,
        anonymous_class_decls,
        anonymous_class_names,
        duplicate_fn_sites,
        duplicate_class_sites,
        ignores,
    })
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

/// The last segment of a fully-qualified name (`"App\Foo\Bar"` becomes
/// `"Bar"`). `declared_names` returns FQNs (needed to match
/// `Registry::index`'s namespace-aware keys, for correct removal), but
/// the invalidation graph (`dependents`, built from
/// `deps::referenced_names`) is deliberately keyed by bare names, so
/// anything feeding that graph's BFS needs converting back down to bare
/// first.
fn bare_name(fqn: &str) -> &str {
    fqn.rsplit('\\').next().unwrap_or(fqn)
}

/// The top-level function/class/enum names one file's parse tree declares -
/// exactly what a registry entry needs to be identified by to be removed
/// again later. Enums share the registry's `classes` map (see
/// `Registry::index`), so they're collected into the same list here.
/// Must produce exactly the same names `Registry::index` actually keys
/// its entries under - otherwise an incremental update's removal step
/// would look up the wrong key and leave a stale entry behind. Mirrors
/// `registry::build_declarations`'s structure (including its positional,
/// mutated-in-place `NameContext` tracking - see that function's own doc
/// comment on why a namespace can't just be computed once up front for
/// the whole file) for exactly that reason.
pub(crate) fn declared_names(stmts: &[Stmt]) -> (Vec<String>, Vec<String>) {
    let mut ctx = NameContext::default();
    declared_names_with_context(stmts, &mut ctx)
}

fn declared_names_with_context(
    stmts: &[Stmt],
    ctx: &mut NameContext,
) -> (Vec<String>, Vec<String>) {
    let mut functions = Vec::new();
    let mut classes = Vec::new();
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Namespace(name) => ctx.set_namespace(name),
            StmtKind::Use(imports) => ctx.add_uses(imports),
            StmtKind::FunctionDecl(f) => functions.push(ctx.declared_name(&f.name)),
            StmtKind::ClassDecl(c) => classes.push(ctx.declared_name(&c.name)),
            StmtKind::EnumDecl(e) => classes.push(ctx.declared_name(&e.name)),
            // Must mirror Registry::index's recursion into `if`/block
            // bodies (the `if (!function_exists(...)) { function foo()
            // {} }` polyfill idiom) - otherwise an incremental update
            // wouldn't know to remove a conditionally-declared symbol
            // that index() itself did pick up, leaving a stale entry
            // behind after the declaring file changes or is removed.
            StmtKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                let (f, c) = declared_names_with_context(
                    std::slice::from_ref(then_branch),
                    &mut ctx.clone(),
                );
                functions.extend(f);
                classes.extend(c);
                if let Some(else_branch) = else_branch {
                    let (f, c) = declared_names_with_context(
                        std::slice::from_ref(else_branch),
                        &mut ctx.clone(),
                    );
                    functions.extend(f);
                    classes.extend(c);
                }
            }
            StmtKind::Block(inner) => {
                let (f, c) = declared_names_with_context(inner, ctx);
                functions.extend(f);
                classes.extend(c);
            }
            _ => {}
        }
    }
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
