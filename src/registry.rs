use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::HashSet;
use std::sync::Arc;

use crate::ast::Stmt;
use crate::types::Type;

mod anonymous_class;
mod assignability;
mod class_info;
mod function_sig;
mod generics;
mod global_vars;
mod hierarchy;
mod index;
mod name_context;
mod purity;
mod resolve;

pub(crate) use anonymous_class::collect_anonymous_classes;
pub(crate) use class_info::hook_body_references_property;
pub(crate) use class_info::object_class_names;
pub use class_info::{ClassInfo, ConstSig, PropertySig};
pub use function_sig::format_signature;
pub(crate) use function_sig::is_allow_dynamic_properties_attribute_name;
pub(crate) use function_sig::is_deprecated_attribute_name;
pub(crate) use function_sig::is_no_discard_attribute_name;
pub(crate) use function_sig::is_override_attribute_name;
pub(crate) use function_sig::param_label;
pub(crate) use function_sig::resolve_type;
pub use function_sig::{AssertInfo, AssertTarget, FunctionSig, ParamSig};
pub(crate) use function_sig::{infer_const_type, mark_template_params, substitute_type_aliases};
pub(crate) use index::{BuiltDecl, build_declarations, register_global_constant};
pub(crate) use name_context::{NameContext, collect_class_name_case_refs, resolve_names_in_type};
pub use purity::PurityDependency;
pub(crate) use purity::{collect_impure_points, is_non_overridable};

/// Real PHPStan's own `ApiRuleHelper::isPhpStanName()` - whether
/// `fqn` is under the `PHPStan\` namespace itself (excluding the two
/// bundled-but-not-really-PHPStan namespaces it explicitly carves out).
/// Relocated here (from `checker/phpstan_api.rs`) so the vendor-directory
/// check's own registry-side plumbing (`phpstan_class_files`, populated
/// during indexing) can call it without a backward `registry -> checker`
/// dependency.
pub(crate) fn is_phpstan_namespace(fqn: &str) -> bool {
    // Real `isPhpStanName()` special-cases a bare `namespace PHPStan;`
    // (no trailing backslash, 7 characters) as true too
    // (`ApiRuleHelperTest.php`'s own `PHPStan`/no-declaring-file row
    // expects `true`) - a plain `fqn.len() >= 8` guard would reject it.
    fqn.eq_ignore_ascii_case("PHPStan")
        || (fqn.len() >= 8
            && fqn.as_bytes()[..8].eq_ignore_ascii_case(b"PHPStan\\")
            && !fqn.starts_with("PHPStan\\PhpDocParser\\")
            && !fqn.starts_with("PHPStan\\BetterReflection\\"))
}

/// A symbol table built by indexing already-parsed source, standing in for
/// the reflection a real PHP runtime would normally provide (see the
/// discussion this project started from: no live interpreter means no
/// `ReflectionClass`, so class/function signatures come from parsing
/// everything up front instead). `merge_stubs` layers in curated built-in
/// signatures (`stubs.rs`) the same way php-src stubs would sit alongside
/// project code in a real implementation.
#[derive(Debug, Default)]
pub struct Registry {
    pub classes: FxHashMap<Arc<str>, ClassInfo>,
    pub functions: FxHashMap<Arc<str>, FunctionSig>,
    // Global-constant name -> inferred-value-type table, covering both
    // `const NAME = value;` and `define('NAME', value)` (keyed like
    // `functions`: a `const` gets its namespace-qualified name; a
    // `define()`'s name is inserted verbatim, since real PHP evaluates
    // it as a plain runtime string regardless of the surrounding
    // namespace). Only a literal-shaped value is captured
    // (`infer_const_type`'s scope) - anything more dynamic is left
    // unindexed, the same "don't know, don't guess" leniency that
    // function already documents.
    pub global_constants: FxHashMap<Arc<str>, Type>,
    // `global $x;`-declared variable name -> inferred-type table, built by
    // folding every real assignment site to that name anywhere in the
    // corpus (`global_vars::infer_global_variables`) - the project-wide
    // vantage point real PHPStan's own per-file `GlobalHandler` doesn't
    // have, so a bare `global $x;` doesn't have to resolve to `Mixed`
    // unconditionally the way it otherwise would (see
    // `checker::stmt`'s own `StmtKind::Global` arm). Absent entry (not a
    // `Mixed`-valued one) means "no assignment site found or resolved" -
    // callers should treat a missing key the same as they already treat
    // `Mixed`.
    pub global_variables: FxHashMap<Arc<str>, Type>,
    // A strictness-parameter half: an unknown array literal's type
    // (`Array(Mixed)`) freely satisfies any array/shape type it's
    // checked against. Defaults to `false` (today's permissive
    // behavior).
    //
    // This and every `check_*`/opt-in flag below follow the same wiring
    // chain (config.rs field -> CLI flag -> `Project` setter -> here) and
    // the same rationale: matching real PHPStan's own opt-in default
    // (not part of any standard level 0-10 rule set) because this
    // project's version is either narrower than the real rule or the
    // real corpus shows a specific false-positive class it would
    // otherwise produce. Only what's specific to each flag is called out
    // below.
    pub strict_mixed_arrays: bool,
    // `Checker::check_uninitialized_properties` - this project's version
    // is a substantially simpler heuristic than PHPStan's real
    // `ConstructorsHelper`-backed dataflow (see `checker/uninitialized.rs`).
    pub check_uninitialized_properties: bool,
    // `Checker::check_internal_*` (`internal_usage.rs`).
    pub check_internal_usage: bool,
    // `check_use_function_imports`'s own `function.nameCase` half.
    // `use const`'s own existence check (`constant.notFound`) is
    // unconditional, not gated by this flag - real PHP constants are
    // case-sensitive, so there's no "resolves fine but wrong case" case
    // the way there is for a function/class name.
    pub check_function_name_case: bool,
    // `Checker::check_deprecated_*` (`deprecated_usage.rs`) - real
    // PHPStan's equivalent (`phpstan/phpstan-deprecation-rules`) is a
    // separate, opt-in Composer package, not part of core
    // `phpstan/phpstan`; this project has no "separate package" concept,
    // so an opt-in flag is the closest equivalent.
    pub check_deprecated_usage: bool,
    // `Checker::check_missing_checked_exceptions`
    // (`checker/throws/missing.rs`) - real PHPStan has no such rule at
    // all; the riskiest of these opt-in checks given the real corpus's
    // documented inconsistent `@throws` tagging.
    pub check_missing_checked_exceptions: bool,
    // The "genuinely empty concrete method body" return-completeness
    // check (`Checker::check_function_with_ancestor_hint`'s own
    // `return.missing`/`return.never` fall-through) - a real
    // false-positive class (IDE-stub/native-extension declaration files
    // with intentionally empty `{}` bodies) means this needs opt-in for
    // a project that scopes its own `paths`/excludes `vendor/`.
    pub check_empty_method_bodies: bool,
    // Strict (vs. the default, castable)
    // `PrintfPlaceholder::doesArgumentTypeMatchPlaceholder` matching mode
    // for `sprintf`/`printf`/`vsprintf`/`vprintf`/`fprintf`
    // (`checker/calls/format_string.rs`) - castable is real PHPStan's
    // stock default too.
    pub check_strict_printf_placeholder_types: bool,
    // Mirrors real PHPStan's own
    // `checkTooWideThrowTypesInProtectedAndPublicMethods` - without it,
    // `TooWideMethodThrowTypeRule` only runs for a `private` method, a
    // method on a `final` class, or one itself marked `final` (otherwise
    // any ordinary overridable method with an unused `@throws` tag would
    // be flagged, even though a subclass override might need the wider
    // type). When on, an overridable method still gets a pass on its
    // first declaration (`is_first_declaration` below) - only a
    // *re-declaration* narrowing an ancestor's wider `@throws` is
    // checked.
    pub check_too_wide_throw_type_in_protected_and_public_methods: bool,
    // The return-type analogue of the flag above, mirroring real
    // PHPStan's own `checkTooWideReturnTypesInProtectedAndPublicMethods`
    // for `TooWideMethodReturnTypehintRule`.
    pub check_too_wide_return_type_in_protected_and_public_methods: bool,
    // `MethodThrowTypeCovarianceRule` (`checker/oop/method_overrides.rs`)
    // - a real PHPStan rule, but shipped opt-in here rather than
    // mirroring its unconditional-by-default registration: this
    // project's `@throws` model has no call-graph-wide implicit-throw
    // inference, so even this rule's pure declaration-vs-declaration
    // comparison is worth verifying against a real corpus before
    // defaulting on.
    pub check_method_throw_type_covariance: bool,
    // `checker::duplicate_functions::check_duplicate_function_declarations`.
    // Real PHPStan's own `DuplicateFunctionDeclarationRule` is
    // `#[ValidatesStubFiles]` and never runs against ordinary scanned
    // files - two unconditionally-declared same-named top-level
    // functions in separate test files that are never loaded together in
    // the same PHP process are a real, structural false-positive this
    // checker can't rule out in general.
    pub check_duplicate_function_declarations: bool,
    // `checker::duplicate_classes::check_duplicate_class_declarations` -
    // its one known false-positive shape (a vendored built-in stub
    // override, e.g. `Imagick.stub.php`, colliding with the real PHP
    // extension's class) can only be defused by a project's `stub_paths`
    // config being correctly populated. See `checker::duplicate_classes`
    // for why this goes beyond real PHPStan, which has no whole-project
    // equivalent to mirror.
    pub check_duplicate_class_declarations: bool,
    // A bare `true`/`false` literal passed positionally into a
    // bare-`bool`-typed parameter is always type-correct, so no rule
    // can see it; opt-in since an unambiguous single-bool-parameter call
    // is common and would otherwise be noise project-wide.
    pub check_boolean_trap: bool,
    // Array-key cast warnings (a literal float/bool/null/canonical-
    // integer-string array key silently cast at runtime) - opt-in since
    // there's no real-world precedent for how noisy this is.
    pub check_array_key_cast: bool,
    // Flags a constructor that never calls an existing parent
    // constructor - opt-in since a legitimate constructor can genuinely
    // choose not to (composition-over-inheritance, a no-op parent).
    pub check_parent_constructor_call: bool,
    // Constructor-property-promotion suggestion - a purely stylistic
    // suggestion, not a bug, so opt-in.
    pub check_promoted_property_suggestions: bool,
    // Flags a condition/`&&`/`||`/`!` operand whose type isn't
    // confidently `bool` - opt-in.
    pub check_boolean_conditions: bool,
    // Strict `checkDynamicProperties` behavior inside `isset()`/
    // `empty()`/`??`/`??=` - the lenient default silently allows probing
    // a property that doesn't exist there; the stricter mode still
    // flags it. Opt-in.
    pub check_dynamic_properties: bool,
    // Swapped-adjacent-argument-name detection - a disclosed heuristic
    // (two same-typed parameters are invisible to any type-based check
    // as "different roles"), not a soundness gap, so opt-in.
    pub check_swapped_arguments: bool,
    // `never`-return-type suggestion for a body that provably never
    // returns normally - opt-in, a stylistic suggestion.
    pub check_never_return_suggestions: bool,
    // A `switch` with no `default:` arm - opt-in, since a switch over an
    // exhaustively-listed set of values (e.g. every case of a bounded
    // string enumeration) can be a deliberate, correct choice this
    // checker has no way to confirm is truly exhaustive.
    pub check_switch_missing_default: bool,
    // `throw new Exception()` (or any other class) with zero
    // constructor arguments - opt-in, since a custom exception subclass
    // can legitimately hardcode its own message internally, making a
    // bare `new` call at the throw site correct rather than a mistake.
    pub check_throw_without_message: bool,
    // `throw new Exception(...)`/`throw new Error(...)` - the bare base
    // class instead of a specific subclass - opt-in, since deliberately
    // throwing the generic base class (a project with no custom
    // exception hierarchy at all) is a legitimate style choice, not
    // necessarily an oversight.
    pub check_throw_base_class: bool,
    // `new SomeException(...)` constructed but discarded as a bare
    // statement, never thrown - opt-in, since a plain `new Foo();`
    // statement for a class with a genuine constructor side effect is
    // ordinary, legal code; only a `Throwable` subtype is even
    // considered, but this project can't rule out every legitimate
    // reason to build one without throwing it (e.g. handing it to a
    // logger).
    pub check_new_exception_discarded: bool,
    // The project's own `level` setting, *before* `check-project`'s own
    // `MAX_LEVEL`-driven `.unwrap_or(6)` collapse - `None` when never
    // explicitly configured, which `accepts_at_configured_level` treats
    // as "keep today's always-strict answer," not real PHPStan's lenient
    // level-0 default. Set once from `check-project`'s already-resolved
    // `level`/`config.level` merge, not a flag of its own.
    pub configured_level: Option<u8>,
    // Whether this project consumes real PHPStan as a dependency at all
    // (set once from `composer::detect_phpstan_dependency` before any
    // file is checked). Gates the main `MethodCall` arm's own
    // `phpstanApi.method`/`phpParser.nodeConnectingAttribute` checks
    // (`checker/expr.rs`, `checker/phpstan_api.rs`); every other
    // PHPStan-API-usage check (instantiation, static calls, class-const
    // fetches, `instanceof`, `extends`/`implements`/trait-use) runs
    // unconditionally.
    pub phpstan_api_check_enabled: bool,
    // The one narrow crack in this struct's "path-agnostic" contract
    // (see `project/decl_locations.rs` for why `ClassInfo`/`Registry`
    // are otherwise path-agnostic) - needed to port real
    // `ApiRuleHelper::isPhpStanCode`'s vendor-directory check
    // (`is_phpstan_code`, `checker/phpstan_api.rs`). Only populated for a
    // name already passing `is_phpstan_namespace`, so it stays empty for
    // any corpus with no `PHPStan\*`-namespaced code.
    pub phpstan_class_files: FxHashMap<String, std::path::PathBuf>,
    // A project's own configured `restricted_symbols` list, compiled
    // once via `RestrictedSymbols::compile` (`Project::
    // set_restricted_symbols`).
    pub restricted_symbols: crate::restricted_symbols::RestrictedSymbols,
    // A project's own `[[doctrine_custom_functions]]` config (PHASE_BACKLOG
    // Phase 168 168h) - DQL function name (lowercased, matching DQL's own
    // case-insensitive function-name lookup) -> declared category.
    pub doctrine_custom_functions:
        FxHashMap<String, crate::doctrine_functions::DoctrineFunctionKind>,
    // A project's own configured `[sql_reflection]` block (`Project::
    // set_sql_reflection_config`) - `None` when unconfigured, which every
    // raw-SQL call site must treat as "keep today's fallback."
    pub sql_reflection_config: Option<crate::sql_reflection_config::SqlReflectionConfig>,
    // A project's own configured `schema_file` (sub-phase A2), parsed
    // once at startup (`Project::set_sql_reflection_schema`) - `None`
    // when no `schema_file` is configured.
    pub sql_reflection_schema: Option<crate::sql_reflection_schema::SchemaCatalog>,
    // The live SQL reflector (sub-phase C), lazily constructed on first
    // real use from `sql_reflection_config` - see
    // `sql_reflection_reflector::SqlReflectorHandle`'s own doc comment.
    pub sql_reflector: crate::sql_reflection_reflector::SqlReflectorHandle,
    // Resolution-order step 2 (`checker::sql_reflection::resolve`):
    // caches a resolved-or-undecidable answer by the literal SQL text
    // alone, not also a schema/config-identity hash - this project
    // supports exactly one schema source and one connection for the
    // whole analysis run (a deliberate design constraint, not an
    // oversight), so that identity is invariant for the run's own
    // lifetime and needs no separate cache-key component. Shared,
    // in-memory, single-run only (never persisted to disk) - avoids a
    // repeated live `PREPARE` round-trip for the same query text
    // appearing at multiple call sites, `rayon::par_iter`-safe via the
    // `Mutex` (mirrors the real, confirmed concurrency hazard
    // `sql_reflector` above already accounts for).
    pub sql_reflection_cache: std::sync::Mutex<FxHashMap<String, Option<Type>>>,
    // Real DB table name -> the one mapped `DoctrineEntity`'s own class
    // name (`checker::sql_reflection::resolve`'s own entity cross-
    // reference, gap #9 of the live SQL reflection phase) - lazily
    // built once from `classes` on first real use, mirroring `sql_
    // reflector`'s own `OnceLock` shape, since it's a whole-program
    // computation (every class must already be indexed) rather than
    // something computable per-class as `classes` itself fills in.
    // Two entities mapped to the same real table name is a genuine,
    // if unusual, Doctrine possibility (e.g. a `readOnly` reporting
    // entity mapped onto another entity's own table) - the first
    // one found wins rather than picking arbitrarily on every lookup,
    // since this project has no real corpus case to verify a "which
    // one wins" rule against.
    doctrine_entity_table_index: std::sync::OnceLock<FxHashMap<Arc<str>, Arc<str>>>,
    // The project's configured target PHP version, if any - `None`
    // means "not configured," which every version-gated rule should
    // treat as "nothing to check yet." See `crate::php_version::PhpFeature`
    // for the feature-availability lookup table this unlocks.
    pub php_version: Option<crate::php_version::PhpVersion>,
    // Class names with at least one *unconditional* declaration indexed
    // so far - lets `insert_built_decl` tell "this name's entry is
    // itself unconditional, so a second one should merge onto it"
    // (Piwigo's own `phpstan-stubs/Imagick.stub.php` shape) apart from
    // "this name's entry is only a conditional stub, so the first real
    // unconditional declaration should replace it outright" - both look
    // identical from `self.classes.contains_key(&name)` alone.
    unconditionally_declared_classes: HashSet<Arc<str>>,
    // Names `merge_stub_classes` provided a built-in native-class stub
    // for (jetbrains/phpstorm-stubs) - a *subset* of
    // `unconditionally_declared_classes` above, distinguishing "a
    // type-checking fiction, never really instantiated" from "a real,
    // unconditionally-declared vendor/first-party class." Merging a
    // version-gated polyfill's members onto a built-in stub is safe
    // (the stub is fictional either way); merging onto a *real* vendor
    // class (`PHPUnit\Framework\TestCase`-vs-`class_exists`-guarded-
    // fallback) would wrongly leak a defensive fallback's members onto
    // the real class everything else sees.
    pub(crate) built_in_stub_classes: HashSet<Arc<str>>,
    // The function-side sibling of the tracking set above -
    // phpstorm-stubs represents a real PHP internal function overload
    // (different signatures by argument shape, not version) as two
    // separate top-level `function` declarations sharing one name
    // (`strtr`: a 3-string-argument form and a 2-argument,
    // `array $replace_pairs` form). Needed so the second declaration
    // merges into a callable accepting either shape, instead of a blind
    // `self.functions.insert` discarding the first's params.
    unconditionally_declared_functions: HashSet<Arc<str>>,
    // Names `merge_stubs` provided a signature for, not yet
    // overwritten by a real, already-indexed conditional declaration -
    // the same ambiguity `unconditionally_declared_functions` resolves
    // on the unconditional side. Needed so the common
    // `if (!function_exists('str_contains')) { function str_contains
    // (?string $haystack, ...) {...} }` polyfill idiom (real shape:
    // symfony/polyfill-php80) doesn't lose its real, more-precise
    // (here, nullable) parameter types to `entry(...).or_insert_with(...)`
    // being a no-op once the built-in stub already occupies the slot.
    stub_provided_functions: HashSet<Arc<str>>,
    // Every bare function/method name with a non-empty `assert` list
    // on some indexed `FunctionSig`. `@phpstan-assert` is rare in real
    // corpora (zero occurrences across the full Symfony monorepo plus
    // every vendored PHP stub), so `checker::stmt`'s bare-statement
    // narrowing pass checks this set-membership first and skips the
    // expensive receiver-type inference + hierarchy walk for a name that
    // provably can't match. Names are only ever added, never removed on
    // `remove_declarations` - a harmless, rare false positive (still a
    // "may still do the real check" set, not a "known good" one), never
    // a false negative.
    pub(crate) names_with_assert_tags: HashSet<Arc<str>>,
    // Every class's precomputed `extends`+`implements` transitive
    // closure - see `is_subclass_of`/`precompute_ancestor_names`
    // (`registry/hierarchy.rs`). Rebuilt whenever the class graph could
    // have changed.
    pub(crate) ancestor_names: FxHashMap<Arc<str>, FxHashSet<Arc<str>>>,
    // Every registered class's lowercased name -> canonical key - see
    // `lookup_class`. A byproduct of `canonicalize_class_name_case`'s
    // already-built map, persisted here instead of discarded.
    pub(crate) classes_lowercase: FxHashMap<String, Arc<str>>,
    // Every registered class's precomputed member-name ->
    // owning-ancestor-name map, one per member kind - see
    // `lookup_method`/`precompute_member_owners` (`registry/hierarchy.rs`).
    pub(crate) method_owner_by_class: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Arc<str>>>,
    pub(crate) ancestor_method_owner_by_class: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Arc<str>>>,
    pub(crate) property_owner_by_class: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Arc<str>>>,
    pub(crate) ancestor_property_owner_by_class: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Arc<str>>>,
    pub(crate) const_owner_by_class: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Arc<str>>>,
}

impl Registry {
    pub fn build(program: &[Stmt]) -> Registry {
        let mut registry = Registry::index_and_flatten(program);
        registry.resolve_transitive_purity();
        registry
    }

    /// Every step `build` runs except the final purity classification.
    /// `php_stubs.rs` calls this directly instead of `build` to extract
    /// raw `FunctionSig`/`ClassInfo` values from vendored stub source - a
    /// stub's function body is always empty, so classifying it here
    /// would wrongly mark every PHP built-in (`header()`, `exit()`, ...)
    /// as provably pure, baked permanently into the `LazyLock`-cached
    /// stub signatures every real project's registry later merges in.
    pub(crate) fn index_and_flatten(program: &[Stmt]) -> Registry {
        let mut registry = Registry::default();
        registry.index(program, false);
        registry.canonicalize_class_name_case();
        registry.flatten_traits();
        registry.resolve_method_tags();
        // `flatten_phpdoc_inheritance` consumes `lookup_ancestor_method_
        // with_owner`/`lookup_method_with_owner` internally, so the
        // member-owner maps must already reflect every name
        // `flatten_traits`/`resolve_method_tags` just added.
        registry.precompute_member_owners();
        registry.flatten_phpdoc_inheritance();
        registry.resolve_deferred_value_of();
        registry.resolve_deferred_type_imports();
        // Adds more method/property names (merges mixin members in) -
        // the member-owner maps need a second rebuild after this.
        registry.resolve_deferred_mixins();
        registry.precompute_ancestor_names();
        registry.precompute_member_owners();
        registry.infer_global_variables(&[(std::path::Path::new("<flattened>"), program)]);
        registry
    }

    /// The one `DoctrineEntity`-mapped class whose own `#[ORM\Table(name:
    /// ...)]` matches `table` exactly, if any - see `doctrine_entity_
    /// table_index`'s own doc comment for why this is lazily built
    /// rather than maintained incrementally.
    pub fn doctrine_entity_for_table(
        &self,
        table: &str,
    ) -> Option<&crate::registry::class_info::DoctrineEntity> {
        let index = self.doctrine_entity_table_index.get_or_init(|| {
            let mut index = FxHashMap::default();
            for (class_name, info) in &self.classes {
                if let Some(entity) = &info.doctrine_entity
                    && let Some(table_name) = &entity.table_name
                {
                    index
                        .entry(table_name.clone())
                        .or_insert_with(|| class_name.clone());
                }
            }
            index
        });
        let class_name = index.get(table)?;
        self.classes.get(class_name)?.doctrine_entity.as_ref()
    }
}

#[cfg(test)]
mod tests;
