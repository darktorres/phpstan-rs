use rustc_hash::FxHashMap;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{Stmt, StmtKind};
use crate::checker::{self, Checker, Diagnostic};
use crate::deps;
use crate::ignore_comments;
use crate::lexer::Lexer;
use crate::parser::{ParseError, Parser};
use crate::pest_config::PestConfig;
use crate::php_stubs;
use crate::registry::{NameContext, Registry};

mod decl_locations;
pub(crate) mod duplicate_fn_sites;

use decl_locations::DeclLocations;
use duplicate_fn_sites::DuplicateFunctionSites;

/// One file's parsed state, kept around so `Project::update_file` can undo
/// exactly what that file previously contributed - both to the shared
/// registry and to `dependents` below - before re-indexing its new parse.
/// That undo/redo pair is the difference between an incremental update and
/// a full reload.
struct FileState {
    ast: Vec<Stmt>,
    functions: Vec<String>,
    classes: Vec<String>,
    references: HashSet<String>,
    // Phase 44: `@phpstan-ignore-line`/`-next-line` - computed once here,
    // while the raw source is still on hand, rather than at `check_file`
    // time (the AST alone doesn't carry comment text at all, and
    // re-reading the file from disk there would be new, avoidable I/O
    // coupling `check_file` doesn't otherwise have).
    ignore_lines: HashSet<u32>,
    // Phase 44 follow-up: the fuller bare `@phpstan-ignore <identifier
    // list>` form - which diagnostic identifiers are suppressed on a
    // given line, computed alongside `ignore_lines` above for the
    // identical reason.
    ignore_identifiers: HashMap<u32, Vec<String>>,
    // Phase 17: this file's own raw source text, kept only when
    // `Project::retain_source` is on (see that field's own doc comment) -
    // `None` for every ordinary `check-project` batch run, which never
    // needs it again once this file's own diagnostics are computed.
    source: Option<String>,
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
    dependents: FxHashMap<String, HashSet<PathBuf>>,
    registry: Registry,
    // Directory-scoped Pest `$this`-binding config, rebuilt from every
    // currently-loaded `Pest.php` file whenever the loaded file set could
    // have changed (`finish_loading`/`update_file`) - see `pest_config`'s
    // own module doc comment for the full resolution algorithm.
    pest_config: PestConfig,
    // Phase 40: gates `checker::phpstan_api::check_phpstan_api_usage` -
    // set once via `set_phpstan_api_check_enabled` (by the same CLI
    // wiring that already calls `composer::discover_vendor_context`,
    // using its own new sibling `composer::detect_phpstan_dependency`)
    // rather than re-detected here on every load/check, since `Project`
    // itself has no fixed notion of "the project's own root directory" -
    // its files are just whatever absolute paths were handed to
    // `load_file`. Defaults to `false`, so every existing caller
    // (`Project::new`, the ~300+ direct construction sites in this
    // project's own test suite, the LSP) is completely unaffected.
    phpstan_api_check_enabled: bool,
    // Phase 17: whether `load_file`/`update_file` keep each file's own raw
    // source text around in its `FileState` (see that field's own doc
    // comment) - off by default, following `phpstan_api_check_enabled`'s
    // own precedent, so every existing caller (`main.rs`'s batch
    // `check-project` path, ~300+ direct-construction tests) is
    // unaffected. Only `lsp.rs`'s `Server::new()` turns this on: hover/
    // definition's own text-split technique (`lsp::text_split`) needs a
    // known span's *exact* source text, including whatever an editor's
    // `didChange` most recently sent - not necessarily what's on disk.
    // Also gates `decl_locations` below, for the identical "only the LSP
    // needs this" reasoning.
    retain_source: bool,
    // Phase 17: see `DeclLocations`'s own doc comment. Populated/pruned
    // alongside `registry.index`/`remove_declarations` in `load_file`/
    // `update_file`, gated behind `retain_source` the same as `FileState.
    // source` - a batch `check-project` run never does a position- or
    // name-based lookup after checking, so building this index there
    // would be pure overhead.
    decl_locations: DeclLocations,
    // Phase 63: every top-level function's own unconditional declaration
    // site(s) - always populated (no `retain_source` gate, unlike
    // `decl_locations` above), since `check_duplicate_function_declarations`
    // needs to run by default for `check-project`. See
    // `DuplicateFunctionSites`'s own doc comment.
    duplicate_fn_sites: DuplicateFunctionSites,
}

impl Project {
    pub fn new() -> Self {
        let mut registry = Registry::default();
        registry.merge_stubs(php_stubs::generated_stubs());
        registry.merge_stub_classes(php_stubs::generated_stub_classes());
        Project {
            files: FxHashMap::default(),
            dependents: FxHashMap::default(),
            registry,
            pest_config: PestConfig::default(),
            phpstan_api_check_enabled: false,
            retain_source: false,
            decl_locations: DeclLocations::default(),
            duplicate_fn_sites: DuplicateFunctionSites::default(),
        }
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

    /// Opts the whole project's checking into Phase 11 Stage 5's
    /// strictness parameter (`registry::Registry::strict_mixed_arrays`) -
    /// a narrow, purpose-built setter rather than exposing a general
    /// `&mut Registry` accessor, which would let a caller bypass
    /// `load_file`/`update_file`'s own bookkeeping around what's
    /// currently indexed.
    pub fn set_strict_mixed_arrays(&mut self, strict: bool) {
        self.registry.strict_mixed_arrays = strict;
    }

    /// Phase 62: opts the whole project's checking into `Checker::
    /// check_uninitialized_properties`, following `set_strict_mixed_
    /// arrays`'s exact precedent - see `registry::Registry::check_
    /// uninitialized_properties`'s own doc comment for why this defaults
    /// to `false`.
    pub fn set_check_uninitialized_properties(&mut self, enabled: bool) {
        self.registry.check_uninitialized_properties = enabled;
    }

    /// Phase 67: opts the whole project's checking into the `@internal`
    /// restricted-usage engine, following `set_check_uninitialized_
    /// properties`'s exact precedent - see `registry::Registry::
    /// check_internal_usage`'s own doc comment for why this defaults to
    /// `false`.
    pub fn set_check_internal_usage(&mut self, enabled: bool) {
        self.registry.check_internal_usage = enabled;
    }

    /// Phase 39: opts the whole project's checking into `@deprecated`
    /// usage warnings, following `set_check_internal_usage`'s exact
    /// precedent - see `registry::Registry::check_deprecated_usage`'s
    /// own doc comment for why this defaults to `false`.
    pub fn set_check_deprecated_usage(&mut self, enabled: bool) {
        self.registry.check_deprecated_usage = enabled;
    }

    /// Phase 42 bonus item: opts the whole project's checking into the
    /// "missing checked exception" family, following `set_check_
    /// deprecated_usage`'s exact precedent - see `registry::Registry::
    /// check_missing_checked_exceptions`'s own doc comment for why this
    /// defaults to `false`.
    pub fn set_check_missing_checked_exceptions(&mut self, enabled: bool) {
        self.registry.check_missing_checked_exceptions = enabled;
    }

    /// Phase 61: opts the whole project's checking into the reinstated
    /// empty-concrete-method-body completeness check, following
    /// `set_check_deprecated_usage`'s exact precedent - see
    /// `registry::Registry::check_empty_method_bodies`'s own doc comment
    /// for why this defaults to `false`.
    pub fn set_check_empty_method_bodies(&mut self, enabled: bool) {
        self.registry.check_empty_method_bodies = enabled;
    }

    /// Phase 69: opts the whole project's checking into the strict (vs.
    /// the default, castable) printf placeholder-type matching mode,
    /// following `set_check_empty_method_bodies`'s exact precedent - see
    /// `registry::Registry::check_strict_printf_placeholder_types`'s own
    /// doc comment for why this defaults to `false`.
    pub fn set_check_strict_printf_placeholder_types(&mut self, enabled: bool) {
        self.registry.check_strict_printf_placeholder_types = enabled;
    }

    /// Phase 42 gating fix: opts the whole project's checking into
    /// `TooWideMethodThrowTypeRule`'s wider, re-declaration-only mode,
    /// following `set_check_empty_method_bodies`'s exact precedent - see
    /// `registry::Registry::check_too_wide_throw_type_in_protected_and_public_methods`'s
    /// own doc comment for why this defaults to `false`.
    pub fn set_check_too_wide_throw_type_in_protected_and_public_methods(&mut self, enabled: bool) {
        self.registry
            .check_too_wide_throw_type_in_protected_and_public_methods = enabled;
    }

    /// Phase 42: opts the whole project's checking into `MethodThrowType
    /// CovarianceRule`, following `set_check_empty_method_bodies`'s exact
    /// precedent - see `registry::Registry::check_method_throw_type_
    /// covariance`'s own doc comment for why this defaults to `false`.
    pub fn set_check_method_throw_type_covariance(&mut self, enabled: bool) {
        self.registry.check_method_throw_type_covariance = enabled;
    }

    /// Phase 38: configures the project's target PHP version
    /// (`registry::Registry::php_version`), following
    /// `set_strict_mixed_arrays`'s exact precedent - a narrow setter
    /// rather than a general `&mut Registry` accessor, for the same
    /// reason given there. `None` means "not configured" (the default),
    /// not "PHP version zero" - every version-gated rule reading this
    /// field needs to treat the two differently.
    pub fn set_php_version(&mut self, php_version: Option<crate::php_version::PhpVersion>) {
        self.registry.php_version = php_version;
    }

    /// Phase 40: opts every subsequent `check_file`/`check_all` call into
    /// `checker::phpstan_api::check_phpstan_api_usage` - the caller
    /// (`main.rs`) decides this once, via
    /// `composer::detect_phpstan_dependency` against the project's own
    /// `composer.json`, the same way it already decides whether to load
    /// `composer::discover_vendor_context`'s own `vendor/` context.
    /// Phase 40.C: also mirrored into `self.registry.phpstan_api_check_
    /// enabled`, following every other opt-in check's exact precedent -
    /// the main `Checker` pass's own `MethodCall` arm needs to read this
    /// too now (`phpstanApi.method`/`phpParser.nodeConnectingAttribute`,
    /// folded in from this module's own previously-separate `Walker`
    /// pass), and `Checker` only ever has `&Registry` in hand, not
    /// `Project` itself.
    pub fn set_phpstan_api_check_enabled(&mut self, enabled: bool) {
        self.phpstan_api_check_enabled = enabled;
        self.registry.phpstan_api_check_enabled = enabled;
    }

    /// See `retain_source`'s own doc comment.
    pub fn set_retain_source(&mut self, enabled: bool) {
        self.retain_source = enabled;
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
        let parsed = parse_file(&path, source)?;
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
            functions,
            classes,
            references,
            ignore_lines,
            ignore_identifiers,
            built_decls,
            duplicate_fn_sites,
        } = parsed;
        for decl in built_decls {
            self.registry.insert_built_decl(decl, analyzed);
        }
        if self.retain_source {
            self.decl_locations
                .index(&ast, &mut NameContext::default(), &path);
        }
        self.duplicate_fn_sites.merge(duplicate_fn_sites);
        register_references(&mut self.dependents, &path, &references);
        self.files.insert(
            path,
            FileState {
                ast,
                functions,
                classes,
                references,
                ignore_lines,
                ignore_identifiers,
                source: self.retain_source.then_some(source),
            },
        );
    }

    /// Finalizes an initial batch of `load_file` calls (trait flattening).
    #[hotpath::measure]
    pub fn finish_loading(&mut self) {
        self.registry.canonicalize_class_name_case();
        self.registry.flatten_traits();
        self.registry.resolve_method_tags();
        self.registry.flatten_phpdoc_inheritance();
        self.registry.resolve_deferred_value_of();
        self.registry.resolve_deferred_type_imports();
        self.registry.resolve_deferred_mixins();
        self.registry.resolve_transitive_purity();
        self.rebuild_pest_config();
    }

    /// Re-parses one already-loaded file, patches just its own registry
    /// contributions and `dependents` entries, and re-checks it plus every
    /// other file that references a symbol it declared (before or after
    /// the change - a symbol the new version dropped still needs its old
    /// dependents re-checked, since they'll now see it as undefined).
    /// Returns every re-checked file's fresh diagnostics, keyed by path -
    /// the changed file is always included, even with zero diagnostics.
    pub fn update_file(
        &mut self,
        path: &Path,
        new_source: &str,
    ) -> Result<HashMap<PathBuf, Vec<Diagnostic>>, ParseError> {
        let mut changed_symbols: HashSet<String> = HashSet::new();

        if let Some(old) = self.files.get(path) {
            self.registry
                .remove_declarations(&old.functions, &old.classes);
            if self.retain_source {
                self.decl_locations
                    .remove_declarations(&old.functions, &old.classes);
            }
            self.duplicate_fn_sites
                .remove_declarations(path, &old.functions);
            changed_symbols.extend(old.functions.iter().map(|s| bare_name(s).to_string()));
            changed_symbols.extend(old.classes.iter().map(|s| bare_name(s).to_string()));
            unregister_references(&mut self.dependents, path, &old.references);
        }

        let ast = parse(new_source)?;
        let (functions, classes) = declared_names(&ast);
        let references = deps::referenced_names(&ast);
        let ignore_lines = ignore_comments::ignored_lines(new_source);
        let ignore_identifiers = ignore_comments::ignored_identifiers(new_source);
        // `changed_symbols` seeds transitive_affected_files' BFS, which
        // looks names up in `self.dependents` - keyed by the *bare* names
        // deps::referenced_names collects (a deliberate simplification,
        // see that function's own doc comment), not the fully-qualified
        // ones `functions`/`classes` carry (needed for correct
        // Registry removal above, Phase 9.5). Feeding FQNs into a
        // bare-keyed lookup silently found nothing for any namespaced
        // class - confirmed as a real regression this uncovered:
        // editing a widely-depended-on namespaced class stopped
        // cascading to any of its real dependents at all.
        changed_symbols.extend(functions.iter().map(|s| bare_name(s).to_string()));
        changed_symbols.extend(classes.iter().map(|s| bare_name(s).to_string()));

        // `true`: `update_file` only ever re-indexes a file already known
        // to the project (an edit to it), so it's a first-party/analyzed
        // file by construction - see `load_analyzed_file`'s own doc
        // comment. Mostly inert in practice, since the removal above
        // already clears any prior unconditional entry this file itself
        // owned before this re-index runs.
        self.registry.index(&ast, true);
        if self.retain_source {
            self.decl_locations
                .index(&ast, &mut NameContext::default(), path);
        }
        self.duplicate_fn_sites
            .index(&ast, &mut NameContext::default(), path, false);
        // A changed class's `use TraitName;` list (added, removed, or a
        // trait's own members changed in this same file) means the whole
        // registry's trait flattening can't be assumed still correct -
        // re-run it. Still far cheaper than a full reload: a HashMap scan
        // over already-parsed ClassInfo, not a re-parse of anything.
        self.registry.canonicalize_class_name_case();
        self.registry.flatten_traits();
        self.registry.resolve_method_tags();
        self.registry.flatten_phpdoc_inheritance();
        self.registry.resolve_deferred_value_of();
        self.registry.resolve_deferred_type_imports();
        self.registry.resolve_deferred_mixins();
        self.registry.resolve_transitive_purity();
        register_references(&mut self.dependents, path, &references);

        self.files.insert(
            path.to_path_buf(),
            FileState {
                ast,
                functions,
                classes,
                references,
                ignore_lines,
                ignore_identifiers,
                source: self.retain_source.then(|| new_source.to_string()),
            },
        );
        self.rebuild_pest_config();

        Ok(self
            .transitive_affected_files(path, changed_symbols)
            .into_iter()
            .map(|p| {
                let diagnostics = self.check_file(&p);
                (p, diagnostics)
            })
            .collect())
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
        seed_symbols: HashSet<String>,
    ) -> HashSet<PathBuf> {
        let mut affected = HashSet::new();
        affected.insert(changed_file.to_path_buf());

        let mut seen_symbols = HashSet::new();
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
    /// registry, returning its diagnostics.
    #[hotpath::measure]
    pub fn check_file(&self, path: &Path) -> Vec<Diagnostic> {
        let Some(file) = self.files.get(path) else {
            return Vec::new();
        };
        let mut checker = Checker::new(&self.registry);
        checker.set_pest_this_type(self.pest_config.resolve_this_type(path, &self.registry));
        checker.set_pest_hook_properties(self.pest_config.resolve_hook_properties(path));
        checker.check_program(&file.ast);
        let mut diagnostics = checker.into_diagnostics();
        diagnostics.extend(
            checker::duplicate_functions::check_duplicate_function_declarations(
                path,
                &self.duplicate_fn_sites,
            ),
        );
        diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.col));
        if self.phpstan_api_check_enabled {
            diagnostics.extend(checker::phpstan_api::check_phpstan_api_usage(
                &file.ast,
                &self.registry,
                Some(path),
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
        // Phase 44: `@phpstan-ignore-line`/`-next-line` - `non_ignorable`
        // diagnostics are exempt, mirroring real PHPStan's own identical
        // exemption (reserved for a handful of its own rules, e.g.
        // flagging a malformed `@phpstan-ignore` comment itself -
        // nonsensical to let another ignore comment suppress).
        if !file.ignore_lines.is_empty() {
            diagnostics
                .retain(|d| d.non_ignorable || !file.ignore_lines.contains(&d.span.start.line));
        }
        // Phase 44 follow-up: the fuller bare `@phpstan-ignore
        // <identifier list>` form - unlike the two line markers above,
        // this only suppresses a diagnostic whose own `identifier`
        // exactly matches one named on that line.
        if !file.ignore_identifiers.is_empty() {
            diagnostics.retain(|d| {
                d.non_ignorable
                    || !file
                        .ignore_identifiers
                        .get(&d.span.start.line)
                        .is_some_and(|ids| ids.iter().any(|id| id == &d.identifier))
            });
        }
        diagnostics
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
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
    pub fn check_all(&self) -> HashMap<PathBuf, Vec<Diagnostic>> {
        self.files
            .keys()
            .map(|path| (path.clone(), self.check_file(path)))
            .collect()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

fn parse(source: &str) -> Result<Vec<Stmt>, ParseError> {
    let tokens = Lexer::new(source).tokenize_with_spans();
    Parser::new(tokens).parse_program()
}

/// Everything about loading one file that's pure - parsing, plus every
/// fact derived only from the parsed AST or the raw source text, with no
/// dependency on (or mutation of) a `Project`'s shared registry. Computing
/// this for many files is embarrassingly parallel (confirmed measured:
/// on a full real-world monorepo corpus, lexing+parsing this deriving
/// data for one large batch of files took ~63% of a `check-project` run's
/// total wall time once `check_file` itself was already parallelized -
/// the dominant remaining sequential cost) - `insert_parsed_file` below
/// does the remaining, order-sensitive registry-mutating half, meant to
/// stay sequential and run in whatever order a caller's own
/// collision-resolution policy needs (see that method's own doc comment).
pub struct ParsedFile {
    ast: Vec<Stmt>,
    functions: Vec<String>,
    classes: Vec<String>,
    references: HashSet<String>,
    ignore_lines: HashSet<u32>,
    ignore_identifiers: HashMap<u32, Vec<String>>,
    // Phase (parallelize insert+finish_loading): this file's own
    // declarations, already built into registry-independent `ClassInfo`/
    // `FunctionSig`/`Type` values (see `registry::build_declarations`'s
    // own doc comment for why that split is pure and safe to compute
    // here, alongside everything else in this struct) - `insert_parsed_
    // file` feeds these into `Registry::insert_built_decl` one at a
    // time instead of re-deriving them from `ast` via `Registry::index`,
    // so the expensive "turn an AST node into a ClassInfo/FunctionSig"
    // work (confirmed the majority of `Registry::index`'s own cost) runs
    // here, in parallel, rather than in the sequential insert loop.
    built_decls: Vec<crate::registry::BuiltDecl>,
    // Phase (parallelize insert+finish_loading): this file's own
    // top-level function declaration sites, already built (see
    // `DuplicateFunctionSites::merge`'s own doc comment) - `insert_
    // parsed_file` merges these into the project-wide table instead of
    // re-walking `ast` via `DuplicateFunctionSites::index`, moving the
    // per-function type-hint cloning `record` does into this already-
    // parallel step. Unlike `built_decls`, this needs `path` (baked into
    // every `FnDeclSite`), which is why `parse_file` below takes one now.
    duplicate_fn_sites: DuplicateFunctionSites,
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
pub fn parse_file(path: &Path, source: &str) -> Result<ParsedFile, ParseError> {
    let ast = parse(source)?;
    let (functions, classes) = declared_names(&ast);
    let references = deps::referenced_names(&ast);
    let ignore_lines = ignore_comments::ignored_lines(source);
    let ignore_identifiers = ignore_comments::ignored_identifiers(source);
    let built_decls = crate::registry::build_declarations(&ast);
    let mut duplicate_fn_sites = DuplicateFunctionSites::default();
    duplicate_fn_sites.index(&ast, &mut NameContext::default(), path, false);
    Ok(ParsedFile {
        ast,
        functions,
        classes,
        references,
        built_decls,
        duplicate_fn_sites,
        ignore_lines,
        ignore_identifiers,
    })
}

/// Recursively collects every `.php` file under `path` - shared by the CLI
/// (`main.rs`'s own subcommands) and the LSP's own workspace-loading path
/// (`lsp.rs`'s `load_workspace`), which is why this lives in the library
/// rather than as CLI-only glue in `main.rs`.
pub fn collect_php_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            collect_php_files(&entry.path(), out);
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
fn declared_names(stmts: &[Stmt]) -> (Vec<String>, Vec<String>) {
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
    dependents: &mut FxHashMap<String, HashSet<PathBuf>>,
    path: &Path,
    references: &HashSet<String>,
) {
    for name in references {
        dependents
            .entry(name.clone())
            .or_default()
            .insert(path.to_path_buf());
    }
}

fn unregister_references(
    dependents: &mut FxHashMap<String, HashSet<PathBuf>>,
    path: &Path,
    references: &HashSet<String>,
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
