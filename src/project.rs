use rustc_hash::FxHashMap;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ast::{ClassKind, Stmt, StmtKind, Visibility};
use crate::checker::{self, Checker, Diagnostic, PropertyAssignFacts};
use crate::deps;
use crate::ignore_comments;
use crate::lexer::Lexer;
use crate::parser::{ParseError, Parser};
use crate::pest_config::PestConfig;
use crate::php_stubs;
use crate::registry::{NameContext, Registry};

mod decl_locations;
pub(crate) mod duplicate_class_sites;
pub(crate) mod duplicate_fn_sites;

use decl_locations::DeclLocations;
use duplicate_class_sites::DuplicateClassSites;
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
    // `@phpstan-ignore-line`/`-next-line` - computed once here, while the
    // raw source is still on hand, rather than at `check_file` time (the
    // AST alone doesn't carry comment text at all, and re-reading the
    // file from disk there would be new, avoidable I/O coupling
    // `check_file` doesn't otherwise have).
    ignore_lines: HashSet<u32>,
    // The fuller bare `@phpstan-ignore <identifier list>` form - which
    // diagnostic identifiers are suppressed on a given line, computed
    // alongside `ignore_lines` above for the identical reason.
    ignore_identifiers: HashMap<u32, Vec<String>>,
    // See `ParsedFile::anonymous_class_names`'s own doc comment - carried
    // here too so `Project::check_file_impl` can hand it to a fresh
    // `Checker` on every re-check, not just the initial load.
    anonymous_class_names: HashMap<crate::token::Span, String>,
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
    // Gates `checker::phpstan_api::check_phpstan_api_usage` - set once
    // via `set_phpstan_api_check_enabled` rather than re-detected here on
    // every load/check, since `Project` itself has no fixed notion of
    // "the project's own root directory" - its files are just whatever
    // absolute paths were handed to `load_file`. Defaults to `false`.
    phpstan_api_check_enabled: bool,
    // Whether `load_file`/`update_file` keep each file's own raw source
    // text around in its `FileState` (see that field's own doc comment) -
    // off by default. Only `lsp.rs`'s `Server::new()` turns this on:
    // hover/definition's own text-split technique (`lsp::text_split`)
    // needs a known span's exact source text, including whatever an
    // editor's `didChange` most recently sent - not necessarily what's on
    // disk. Also gates `decl_locations` below, for the identical "only
    // the LSP needs this" reasoning.
    retain_source: bool,
    // See `DeclLocations`'s own doc comment. Populated/pruned alongside
    // `registry.index`/`remove_declarations` in `load_file`/
    // `update_file`, gated behind `retain_source` the same as
    // `FileState.source` - a batch `check-project` run never does a
    // position- or name-based lookup after checking, so building this
    // index there would be pure overhead.
    decl_locations: DeclLocations,
    // Every top-level function's own unconditional declaration site(s) -
    // always populated (no `retain_source` gate, unlike `decl_locations`
    // above), the same indexing-side/check-side split
    // `duplicate_class_sites` below uses: its own gate is at the check
    // side, `Registry::check_duplicate_function_declarations`, not the
    // indexing side. See `DuplicateFunctionSites`'s own doc comment.
    duplicate_fn_sites: DuplicateFunctionSites,
    // The class-like counterpart to `duplicate_fn_sites` above - always
    // populated too (its own gate is at the check side,
    // `Registry::check_duplicate_class_declarations`, not the indexing
    // side, so an opt-out project pays only the cost of an unused table,
    // not a missing one it might later want turned on incrementally).
    // See `DuplicateClassSites`'s own doc comment.
    duplicate_class_sites: DuplicateClassSites,
}

impl Project {
    pub fn new() -> Self {
        let mut registry = Registry::default();
        registry.merge_stubs(php_stubs::generated_stubs());
        registry.merge_stub_classes(php_stubs::generated_stub_classes());
        registry.merge_stub_constants(php_stubs::generated_stub_constants());
        Project {
            files: FxHashMap::default(),
            dependents: FxHashMap::default(),
            registry,
            pest_config: PestConfig::default(),
            phpstan_api_check_enabled: false,
            retain_source: false,
            decl_locations: DeclLocations::default(),
            duplicate_fn_sites: DuplicateFunctionSites::default(),
            duplicate_class_sites: DuplicateClassSites::default(),
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

    /// Opts the whole project's checking into
    /// `registry::Registry::strict_mixed_arrays` - a narrow,
    /// purpose-built setter rather than exposing a general `&mut
    /// Registry` accessor, which would let a caller bypass
    /// `load_file`/`update_file`'s own bookkeeping around what's
    /// currently indexed.
    pub fn set_strict_mixed_arrays(&mut self, strict: bool) {
        self.registry.strict_mixed_arrays = strict;
    }

    /// Opts the whole project's checking into `Checker::
    /// check_uninitialized_properties` - see `registry::Registry::check_
    /// uninitialized_properties`'s own doc comment for why this defaults
    /// to `false`.
    pub fn set_check_uninitialized_properties(&mut self, enabled: bool) {
        self.registry.check_uninitialized_properties = enabled;
    }

    /// Opts the whole project's checking into the `@internal`
    /// restricted-usage engine - see `registry::Registry::
    /// check_internal_usage`'s own doc comment for why this defaults to
    /// `false`.
    pub fn set_check_internal_usage(&mut self, enabled: bool) {
        self.registry.check_internal_usage = enabled;
    }

    /// Compiles a project's own configured `restricted_symbols` list
    /// into the `Registry`-held index `checker::restricted_symbol_usage`
    /// consults. Empty by default (no entries configured), so this is a
    /// zero-cost no-op call for every ordinary project.
    pub fn set_restricted_symbols(
        &mut self,
        entries: &[crate::restricted_symbols::RestrictedSymbolEntry],
    ) {
        self.registry.restricted_symbols =
            crate::restricted_symbols::RestrictedSymbols::compile(entries);
    }

    /// Compiles a project's own configured `[[doctrine_custom_functions]]`
    /// list (PHASE_BACKLOG Phase 168 168h) into the `Registry`-held
    /// lookup `checker::doctrine::result_shape` consults. Empty by
    /// default, so this is a zero-cost no-op call for every project with
    /// no custom DQL function registrations.
    pub fn set_doctrine_custom_functions(
        &mut self,
        entries: &[crate::doctrine_functions::DoctrineCustomFunctionEntry],
    ) {
        self.registry.doctrine_custom_functions = entries
            .iter()
            .map(|e| (e.name.to_ascii_lowercase(), e.kind))
            .collect();
    }

    /// Stores a project's own configured `[sql_reflection]` block on the
    /// `Registry`, inert until the live SQL query reflection feature
    /// (`checker::sql_reflection`) is built on top of it. `None` (no
    /// block configured) is the default and keeps every raw-SQL call
    /// site on today's fallback behavior, unchanged.
    pub fn set_sql_reflection_config(
        &mut self,
        config: Option<crate::sql_reflection_config::SqlReflectionConfig>,
    ) {
        self.registry.sql_reflection_config = config;
    }

    /// Stores a project's own parsed `schema_file` catalog (sub-phase
    /// A2), inert until sub-phase C's reflector consults it. `None`
    /// when no `schema_file` is configured.
    pub fn set_sql_reflection_schema(
        &mut self,
        schema: Option<crate::sql_reflection_schema::SchemaCatalog>,
    ) {
        self.registry.sql_reflection_schema = schema;
    }

    /// Opts the whole project's checking into `use function`'s own
    /// `function.nameCase` check - see `registry::Registry::
    /// check_function_name_case`'s own doc comment for why this defaults
    /// to `false`.
    pub fn set_check_function_name_case(&mut self, enabled: bool) {
        self.registry.check_function_name_case = enabled;
    }

    /// Opts the whole project's checking into `@deprecated` usage
    /// warnings - see `registry::Registry::check_deprecated_usage`'s own
    /// doc comment for why this defaults to `false`.
    pub fn set_check_deprecated_usage(&mut self, enabled: bool) {
        self.registry.check_deprecated_usage = enabled;
    }

    /// Opts the whole project's checking into the "missing checked
    /// exception" family - see `registry::Registry::
    /// check_missing_checked_exceptions`'s own doc comment for why this
    /// defaults to `false`.
    pub fn set_check_missing_checked_exceptions(&mut self, enabled: bool) {
        self.registry.check_missing_checked_exceptions = enabled;
    }

    /// Opts the whole project's checking into the reinstated
    /// empty-concrete-method-body completeness check - see
    /// `registry::Registry::check_empty_method_bodies`'s own doc comment
    /// for why this defaults to `false`.
    pub fn set_check_empty_method_bodies(&mut self, enabled: bool) {
        self.registry.check_empty_method_bodies = enabled;
    }

    /// Opts the whole project's checking into the strict (vs. the
    /// default, castable) printf placeholder-type matching mode - see
    /// `registry::Registry::check_strict_printf_placeholder_types`'s own
    /// doc comment for why this defaults to `false`.
    pub fn set_check_strict_printf_placeholder_types(&mut self, enabled: bool) {
        self.registry.check_strict_printf_placeholder_types = enabled;
    }

    /// Opts the whole project's checking into `TooWideMethodThrowTypeRule`'s
    /// wider, re-declaration-only mode - see
    /// `registry::Registry::check_too_wide_throw_type_in_protected_and_public_methods`'s
    /// own doc comment for why this defaults to `false`.
    pub fn set_check_too_wide_throw_type_in_protected_and_public_methods(&mut self, enabled: bool) {
        self.registry
            .check_too_wide_throw_type_in_protected_and_public_methods = enabled;
    }

    /// The return-type analogue - see `registry::Registry::check_too_wide_return_type_in_protected_and_public_methods`'s
    /// own doc comment.
    pub fn set_check_too_wide_return_type_in_protected_and_public_methods(
        &mut self,
        enabled: bool,
    ) {
        self.registry
            .check_too_wide_return_type_in_protected_and_public_methods = enabled;
    }

    /// Opts the whole project's checking into
    /// `MethodThrowTypeCovarianceRule` - see `registry::Registry::
    /// check_method_throw_type_covariance`'s own doc comment for why
    /// this defaults to `false`.
    pub fn set_check_method_throw_type_covariance(&mut self, enabled: bool) {
        self.registry.check_method_throw_type_covariance = enabled;
    }

    /// Corrected from unconditional to opt-in - see `registry::Registry::
    /// check_duplicate_function_declarations`'s own doc comment for the
    /// real false positive that motivated this.
    pub fn set_check_duplicate_function_declarations(&mut self, enabled: bool) {
        self.registry.check_duplicate_function_declarations = enabled;
    }

    /// Opts the whole project's checking into `checker::
    /// duplicate_classes::check_duplicate_class_declarations` - see
    /// `registry::Registry::check_duplicate_class_declarations`'s own doc
    /// comment for why this defaults to `false`.
    pub fn set_check_duplicate_class_declarations(&mut self, enabled: bool) {
        self.registry.check_duplicate_class_declarations = enabled;
    }

    /// Opts the whole project's checking into boolean-trap detection -
    /// see `registry::Registry::check_boolean_trap`'s own doc comment for
    /// why this defaults to `false`.
    pub fn set_check_boolean_trap(&mut self, enabled: bool) {
        self.registry.check_boolean_trap = enabled;
    }

    /// Opts the whole project's checking into array-key cast warnings -
    /// see `registry::Registry::check_array_key_cast`'s own doc comment
    /// for why this defaults to `false`.
    pub fn set_check_array_key_cast(&mut self, enabled: bool) {
        self.registry.check_array_key_cast = enabled;
    }

    /// Opts the whole project's checking into
    /// `RequireParentConstructCallRule` - see `registry::Registry::
    /// check_parent_constructor_call`'s own doc comment for why this
    /// defaults to `false`.
    pub fn set_check_parent_constructor_call(&mut self, enabled: bool) {
        self.registry.check_parent_constructor_call = enabled;
    }

    /// Opts the whole project's checking into constructor-property-
    /// promotion suggestions - see `registry::Registry::
    /// check_promoted_property_suggestions`'s own doc comment for why
    /// this defaults to `false`.
    pub fn set_check_promoted_property_suggestions(&mut self, enabled: bool) {
        self.registry.check_promoted_property_suggestions = enabled;
    }

    /// Opts the whole project's checking into the "only booleans in a
    /// condition" family - see `registry::Registry::
    /// check_boolean_conditions`'s own doc comment for why this
    /// defaults to `false`.
    pub fn set_check_boolean_conditions(&mut self, enabled: bool) {
        self.registry.check_boolean_conditions = enabled;
    }

    /// Opts the whole project's checking into strict `checkDynamic
    /// Properties` behavior inside `isset()`/`empty()`/`??`/`??=` - see
    /// `registry::Registry::check_dynamic_properties`'s own doc comment
    /// for why this defaults to `false`.
    pub fn set_check_dynamic_properties(&mut self, enabled: bool) {
        self.registry.check_dynamic_properties = enabled;
    }

    /// Opts the whole project's checking into swapped-adjacent-
    /// argument-name detection - see `registry::Registry::
    /// check_swapped_arguments`'s own doc comment for why this defaults
    /// to `false`.
    pub fn set_check_swapped_arguments(&mut self, enabled: bool) {
        self.registry.check_swapped_arguments = enabled;
    }

    /// Opts the whole project's checking into `never`-return-type
    /// suggestions - see `registry::Registry::
    /// check_never_return_suggestions`'s own doc comment for why this
    /// defaults to `false`.
    pub fn set_check_never_return_suggestions(&mut self, enabled: bool) {
        self.registry.check_never_return_suggestions = enabled;
    }

    /// Opts the whole project's checking into missing-`default:`-arm
    /// detection - see `registry::Registry::check_switch_missing_
    /// default`'s own doc comment for why this defaults to `false`.
    pub fn set_check_switch_missing_default(&mut self, enabled: bool) {
        self.registry.check_switch_missing_default = enabled;
    }

    /// Opts the whole project's checking into missing-exception-message
    /// detection - see `registry::Registry::check_throw_without_message`'s
    /// own doc comment for why this defaults to `false`.
    pub fn set_check_throw_without_message(&mut self, enabled: bool) {
        self.registry.check_throw_without_message = enabled;
    }

    /// Opts the whole project's checking into base-Exception/Error-class
    /// detection - see `registry::Registry::check_throw_base_class`'s
    /// own doc comment for why this defaults to `false`.
    pub fn set_check_throw_base_class(&mut self, enabled: bool) {
        self.registry.check_throw_base_class = enabled;
    }

    /// Opts the whole project's checking into discarded-`new`-exception
    /// detection - see `registry::Registry::check_new_exception_
    /// discarded`'s own doc comment for why this defaults to `false`.
    pub fn set_check_new_exception_discarded(&mut self, enabled: bool) {
        self.registry.check_new_exception_discarded = enabled;
    }

    /// Configures the project's own `level` setting for
    /// `registry::Registry::accepts_at_configured_level` - see that
    /// field's own doc comment for why this is `Option<u8>` (not the
    /// collapsed `u8` `check-project` otherwise uses for diagnostic
    /// filtering) and why `None` must keep today's always-strict
    /// behavior.
    pub fn set_configured_level(&mut self, level: Option<u8>) {
        self.registry.configured_level = level;
    }

    /// Configures the project's target PHP version
    /// (`registry::Registry::php_version`) - a narrow setter rather than
    /// a general `&mut Registry` accessor, for the same reason given at
    /// `set_strict_mixed_arrays`. `None` means "not configured" (the
    /// default), not "PHP version zero" - every version-gated rule
    /// reading this field needs to treat the two differently.
    pub fn set_php_version(&mut self, php_version: Option<crate::php_version::PhpVersion>) {
        self.registry.php_version = php_version;
    }

    /// Opts every subsequent `check_file`/`check_all` call into
    /// `checker::phpstan_api::check_phpstan_api_usage` - the caller
    /// (`main.rs`) decides this once, via
    /// `composer::detect_phpstan_dependency` against the project's own
    /// `composer.json`, the same way it already decides whether to load
    /// `composer::discover_vendor_context`'s own `vendor/` context. Also
    /// mirrored into `self.registry.phpstan_api_check_enabled`, since
    /// the main `Checker` pass's own `MethodCall` arm needs to read this
    /// too (`phpstanApi.method`/`phpParser.nodeConnectingAttribute`), and
    /// `Checker` only ever has `&Registry` in hand, not `Project` itself.
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
        // `is_stub: false` - `load_file`/`load_analyzed_file` are the
        // LSP/test-harness convenience path, which has no `stub_paths`
        // concept at all (that's a batch `check-project`-only config key,
        // wired through `read_and_parse_file` instead) and no way to
        // enable `check_duplicate_class_declarations` in the first place.
        let parsed = parse_file(&path, source, false)?;
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
            anonymous_class_decls,
            anonymous_class_names,
            duplicate_fn_sites,
            duplicate_class_sites,
        } = parsed;
        for decl in built_decls.into_iter().chain(anonymous_class_decls) {
            self.registry.insert_built_decl(decl, analyzed);
        }
        if self.retain_source {
            self.decl_locations
                .index(&ast, &mut NameContext::default(), &path);
        }
        // `record_phpstan_class_file` no-ops unless
        // `phpstan_api_check_enabled` and the name is `PHPStan\*`-
        // namespaced, so this is a zero-cost loop for every ordinary
        // corpus.
        for name in &classes {
            self.registry.record_phpstan_class_file(name, &path);
        }
        self.duplicate_fn_sites.merge(duplicate_fn_sites);
        self.duplicate_class_sites.merge(duplicate_class_sites);
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
                anonymous_class_names,
                source: self.retain_source.then_some(source),
                analyzed,
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
        self.registry.flatten_phpdoc_inheritance();
        self.registry.resolve_deferred_value_of();
        self.registry.resolve_deferred_type_imports();
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
            self.duplicate_class_sites
                .remove_declarations(path, &old.classes);
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
        if self.retain_source {
            self.decl_locations
                .index(&ast, &mut NameContext::default(), path);
        }
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
        self.registry.flatten_phpdoc_inheritance();
        self.registry.resolve_deferred_value_of();
        self.registry.resolve_deferred_type_imports();
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
                anonymous_class_names,
                source: self.retain_source.then(|| new_source.to_string()),
                analyzed,
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
    pub fn remove_file(&mut self, path: &Path) -> HashMap<PathBuf, Vec<Diagnostic>> {
        let mut changed_symbols: HashSet<String> = HashSet::new();

        if let Some(old) = self.files.remove(path) {
            self.registry
                .remove_declarations(&old.functions, &old.classes);
            if self.retain_source {
                self.decl_locations
                    .remove_declarations(&old.functions, &old.classes);
            }
            self.duplicate_fn_sites
                .remove_declarations(path, &old.functions);
            self.duplicate_class_sites
                .remove_declarations(path, &old.classes);
            changed_symbols.extend(old.functions.iter().map(|s| bare_name(s).to_string()));
            changed_symbols.extend(old.classes.iter().map(|s| bare_name(s).to_string()));
            unregister_references(&mut self.dependents, path, &old.references);

            self.registry.canonicalize_class_name_case();
            self.registry.flatten_traits();
            self.registry.resolve_method_tags();
            self.registry.precompute_member_owners();
            self.registry.flatten_phpdoc_inheritance();
            self.registry.resolve_deferred_value_of();
            self.registry.resolve_deferred_type_imports();
            self.registry.resolve_deferred_mixins();
            self.registry.precompute_member_owners();
            self.registry.resolve_transitive_purity();
            self.registry.precompute_ancestor_names();
            self.rebuild_pest_config();
        }

        self.transitive_affected_files(path, changed_symbols)
            .into_iter()
            .map(|p| {
                let diagnostics = self.check_file(&p);
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
    /// registry, returning its diagnostics. Same-file property-too-wide-
    /// type facts only (see `Checker::cross_file_property_assign_facts`'s
    /// own doc comment) - correct for incremental/LSP use (a single-file
    /// re-check has no cheap way to also re-derive every other file's own
    /// contribution), and for every caller that doesn't need the full
    /// two-pass project-wide merge. `check_file_with_property_assign_facts`
    /// is the batch counterpart that does.
    #[hotpath::measure]
    pub fn check_file(&self, path: &Path) -> Vec<Diagnostic> {
        self.check_file_impl(path, None)
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
        self.check_file_impl(path, Some(facts))
    }

    fn check_file_impl(
        &self,
        path: &Path,
        cross_file_property_assign_facts: Option<&Arc<PropertyAssignFacts>>,
    ) -> Vec<Diagnostic> {
        let Some(file) = self.files.get(path) else {
            return Vec::new();
        };
        let mut checker = Checker::new(&self.registry);
        checker.set_pest_this_type(self.pest_config.resolve_this_type(path, &self.registry));
        checker.set_pest_hook_properties(self.pest_config.resolve_hook_properties(path));
        checker.set_anonymous_class_names(file.anonymous_class_names.clone());
        if let Some(facts) = cross_file_property_assign_facts {
            checker.set_cross_file_property_assign_facts(Arc::clone(facts));
        }
        checker.check_program(&file.ast);
        let mut diagnostics = checker.into_diagnostics();
        if self.registry.check_duplicate_function_declarations {
            diagnostics.extend(
                checker::duplicate_functions::check_duplicate_function_declarations(
                    path,
                    &self.duplicate_fn_sites,
                ),
            );
        }
        if self.registry.check_duplicate_class_declarations {
            diagnostics.extend(
                checker::duplicate_classes::check_duplicate_class_declarations(
                    path,
                    &self.duplicate_class_sites,
                ),
            );
        }
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
        // `@phpstan-ignore-line`/`-next-line` - `non_ignorable`
        // diagnostics are exempt, mirroring real PHPStan's own identical
        // exemption (reserved for a handful of its own rules, e.g.
        // flagging a malformed `@phpstan-ignore` comment itself -
        // nonsensical to let another ignore comment suppress).
        if !file.ignore_lines.is_empty() {
            diagnostics
                .retain(|d| d.non_ignorable || !file.ignore_lines.contains(&d.span.start.line));
        }
        // The fuller bare `@phpstan-ignore <identifier list>` form -
        // unlike the two line markers above, this only suppresses a
        // diagnostic whose own `identifier` exactly matches one named on
        // that line.
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
    /// file purely to gather `Checker::property_assign_facts` (this
    /// pass's own diagnostics are discarded - `check_file`/`check_file_
    /// with_property_assign_facts` are the real, diagnostic-producing
    /// pass), then merges every file's own per-fqn/per-property
    /// contribution into one whole-project map. A trait re-checked once
    /// per host (`check_trait_methods`) already attributes its own
    /// contribution to each host's real fqn (see `property_assign_facts`'s
    /// own doc comment), so merging here needs no extra trait-awareness
    /// of its own - just concatenating every file's own per-key `Vec`.
    pub fn collect_property_assign_facts(&self) -> PropertyAssignFacts {
        let mut merged: PropertyAssignFacts = HashMap::new();
        for path in self.files.keys() {
            let Some(file) = self.files.get(path) else {
                continue;
            };
            if !self.file_may_write_private_property(file) {
                continue;
            }
            let mut checker = Checker::new(&self.registry);
            checker.set_facts_only(true);
            checker.check_program(&file.ast);
            for (fqn, props) in checker.take_property_assign_facts() {
                let entry = merged.entry(fqn).or_default();
                for (prop, mut types) in props {
                    entry.entry(prop).or_default().append(&mut types);
                }
            }
        }
        merged
    }

    /// Runs the checker over every currently-loaded file, not just one -
    /// the whole-project counterpart to `check_file`. `--check-project`
    /// no longer uses this directly (it needs to check only the files it
    /// was asked to analyze, not every file loaded for context - see
    /// `main.rs::run_check_project`), but it's still the right tool for
    /// tests/tooling that need to see every diagnostic across a real
    /// corpus at once, rather than one file (and its
    /// transitively-affected dependents) at a time the way `update_file`
    /// does. Two-pass: `collect_property_assign_facts` runs first so a
    /// trait-donated property write discovered in one file reaches
    /// `check_too_wide_property_types` in whichever file declares the
    /// host using it.
    pub fn check_all(&self) -> HashMap<PathBuf, Vec<Diagnostic>> {
        let facts = Arc::new(self.collect_property_assign_facts());
        self.files
            .keys()
            .map(|path| {
                (
                    path.clone(),
                    self.check_file_with_property_assign_facts(path, &facts),
                )
            })
            .collect()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
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

fn parse(source: &str) -> Result<Vec<Stmt>, ParseError> {
    let tokens = Lexer::new(source).tokenize_with_spans();
    Parser::new(tokens).parse_program()
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
    functions: Vec<String>,
    classes: Vec<String>,
    references: HashSet<String>,
    ignore_lines: HashSet<u32>,
    ignore_identifiers: HashMap<u32, Vec<String>>,
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
    anonymous_class_names: HashMap<crate::token::Span, String>,
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
pub fn parse_file(path: &Path, source: &str, is_stub: bool) -> Result<ParsedFile, ParseError> {
    let ast = parse(source)?;
    let (functions, classes) = declared_names(&ast);
    let references = deps::referenced_names(&ast);
    let ignore_lines = ignore_comments::ignored_lines(source);
    let ignore_identifiers = ignore_comments::ignored_identifiers(source);
    let built_decls = crate::registry::build_declarations(&ast);
    let (anonymous_class_decls, anonymous_class_names) =
        crate::registry::collect_anonymous_classes(&ast, &path.display().to_string());
    let mut duplicate_fn_sites = DuplicateFunctionSites::default();
    duplicate_fn_sites.index(&ast, &mut NameContext::default(), path, false);
    let mut duplicate_class_sites = DuplicateClassSites::default();
    duplicate_class_sites.index(&ast, &mut NameContext::default(), path, false, is_stub);
    Ok(ParsedFile {
        ast,
        functions,
        classes,
        references,
        built_decls,
        anonymous_class_decls,
        anonymous_class_names,
        duplicate_fn_sites,
        duplicate_class_sites,
        ignore_lines,
        ignore_identifiers,
    })
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
