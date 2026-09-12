use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::HashSet;

use crate::ast::Stmt;
use crate::types::Type;

mod assignability;
mod class_info;
mod function_sig;
mod generics;
mod hierarchy;
mod index;
mod name_context;
mod purity;
mod resolve;

pub(crate) use class_info::hook_body_references_property;
pub(crate) use class_info::object_class_names;
pub use class_info::{ClassInfo, ConstSig, PropertySig};
pub use function_sig::format_signature;
pub(crate) use function_sig::is_allow_dynamic_properties_attribute_name;
pub(crate) use function_sig::is_deprecated_attribute_name;
pub(crate) use function_sig::is_no_discard_attribute_name;
pub(crate) use function_sig::is_override_attribute_name;
pub(crate) use function_sig::resolve_type;
pub use function_sig::{AssertInfo, AssertTarget, FunctionSig, ParamSig};
pub(crate) use function_sig::{infer_const_type, mark_template_params, substitute_type_aliases};
pub(crate) use index::{BuiltDecl, build_declarations, register_global_constant};
pub(crate) use name_context::{NameContext, collect_class_name_case_refs, resolve_names_in_type};
pub use purity::PurityDependency;
pub(crate) use purity::{collect_impure_points, is_non_overridable};

/// Phase 40: real PHPStan's own `ApiRuleHelper::isPhpStanName()` - whether
/// `fqn` is under the `PHPStan\` namespace itself (excluding the two
/// bundled-but-not-really-PHPStan namespaces it explicitly carves out).
/// Relocated here (from `checker/phpstan_api.rs`) so the vendor-directory
/// check's own registry-side plumbing (`phpstan_class_files`, populated
/// during indexing) can call it without a backward `registry -> checker`
/// dependency.
pub(crate) fn is_phpstan_namespace(fqn: &str) -> bool {
    // Phase 40 follow-up: real `isPhpStanName()` special-cases a bare
    // `namespace PHPStan;` (no trailing backslash, 7 characters) as true
    // too (confirmed: `ApiRuleHelperTest.php`'s own `PHPStan`/no-
    // declaring-file row expects `true`) - this project's own `fqn.len()
    // >= 8` guard below rejected it before this fix.
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
    pub classes: FxHashMap<String, ClassInfo>,
    pub functions: FxHashMap<String, FunctionSig>,
    // A global-constant name -> inferred-value-type table, covering both
    // real `const NAME = value;` top-level declarations and `define(
    // 'NAME', value)` calls (jetbrains/phpstorm-stubs' own
    // `define('T_WHITESPACE', 397);`-style constants, real corpus scale:
    // hundreds across that one vendored tree alone). Before this, a bare
    // reference to *any* global constant resolved to plain `Mixed`
    // unconditionally (no table existed at all - see `checker/expr.rs`'s
    // own `bare_name_literal_type` doc comment) - real PHPStan resolves
    // both the same way (`ReflectionConstant`'s own value-based type
    // inference doesn't distinguish how the constant was declared), so
    // this table doesn't either. Only a `const`'s own literal-shaped
    // value (`infer_const_type`'s existing scope: int/float/string/bool/
    // null/negative-int/string-concat literals) or a `define()`'s own
    // literal second argument is captured - anything more dynamic
    // (referencing another constant, a function call, ...) is left
    // unindexed, the identical "don't know, don't guess" leniency
    // `infer_const_type` itself already documents. Keyed the same way as
    // `functions`: a `const` gets its own namespace-qualified name
    // (`NameContext::declared_name`, real PHP's own resolution for `const`
    // declarations); a `define()`'s own name is *never* namespace-relative
    // (real PHP evaluates it as a plain runtime string regardless of the
    // surrounding `namespace` block), so it's inserted verbatim instead.
    // Looked up via `NameContext::resolve_function_candidates` at
    // check-time (`Checker::global_constant_type`) - constants share
    // exactly the same "check the current namespace first, fall back to
    // the global namespace for an unqualified reference" resolution rule
    // real PHP functions do, unlike class names.
    pub global_constants: FxHashMap<String, Type>,
    // Phase 11 Stage 5's strictness-parameter half (real PHPStan's own
    // 7-10-style progressively-stricter handling, here narrowed to the
    // one real leniency `is_assignable` has purely on its own: an
    // unknown array literal's type, `Array(Mixed)`, freely satisfying any
    // specific array/shape type it's checked against). Defaults to
    // `false` (today's existing permissive behavior, unchanged) via
    // `#[derive(Default)]` - a plain `pub` field rather than a
    // constructor parameter, since every one of this struct's 10
    // existing construction sites already goes through `build`/
    // `default()` and none of them need to change to keep compiling.
    pub strict_mixed_arrays: bool,
    // Phase 62: opts into `Checker::check_uninitialized_properties`.
    // Defaults to `false`, matching real PHPStan's own default *exactly*
    // - `UninitializedPropertyRule` sits behind its own
    // `checkUninitializedProperties` parameter (`false` in `conf/
    // config.neon`, not part of any of the standard level 0-10 rule
    // sets) - real PHPStan itself doesn't trust this rule enough to run
    // it by default, and this project's own version is a substantially
    // simpler heuristic than PHPStan's real `ConstructorsHelper`-backed
    // dataflow (see `checker/uninitialized.rs`'s own doc comment for
    // exactly how much narrower). Same `strict_mixed_arrays` precedent:
    // a plain `pub` field via `#[derive(Default)]`, config -> CLI flag ->
    // `Project` setter -> here.
    pub check_uninitialized_properties: bool,
    // Phase 67: opts into `Checker::check_internal_*` (`internal_usage.rs`).
    // Defaults to `false`, matching real PHPStan's own default *exactly*
    // - the whole `RestrictedInternal*UsageExtension` family sits behind
    // `featureToggles.internalTag` (`false` in `conf/config.neon`, not
    // part of any of the standard level 0-10 rule sets), the same
    // non-default posture `check_uninitialized_properties` above already
    // established a precedent for. Same wiring chain: a plain `pub`
    // field via `#[derive(Default)]`, config -> CLI flag -> `Project`
    // setter -> here.
    pub check_internal_usage: bool,
    // ROADMAP "Known but not yet scheduled": opts into `check_use_
    // function_imports`'s own `function.nameCase` half - real PHPStan's
    // `ExistingNamesInUseRule` checks this too, but only when its own
    // `checkFunctionNameCase` parameter is enabled (`false` in `conf/
    // config.neon`, not part of any of the standard level 0-10 rule
    // sets, no level ever overrides it) - the same non-default posture
    // `check_internal_usage` above already established a precedent for.
    // Same wiring chain: a plain `pub` field via `#[derive(Default)]`,
    // config -> CLI flag -> `Project` setter -> here. `use const`'s own
    // existence check (`constant.notFound`) is unconditional, not gated
    // by this flag - real PHPStan has no name-case equivalent for a
    // constant at all (constants are case-sensitive in PHP, so there's
    // no "resolves fine but wrong case" scenario the way there is for a
    // function/class name).
    pub check_function_name_case: bool,
    // Phase 39: opts into `Checker::check_deprecated_*`
    // (`deprecated_usage.rs`). Defaults to `false` - real PHPStan's own
    // equivalent (`phpstan/phpstan-deprecation-rules`) isn't even part of
    // core `phpstan/phpstan` at all, but a separate, opt-in Composer
    // package most real projects install alongside it; this project has
    // no such "separate package" concept, so an opt-in flag is the
    // closest equivalent, following `check_internal_usage`'s exact same
    // wiring chain and "real value, but real vendor-corpus noise"
    // rationale.
    pub check_deprecated_usage: bool,
    // Phase 42 bonus item: opts into `Checker::check_missing_checked_exceptions`
    // (`checker/throws/missing.rs`) - flags a real, explicit `throw`
    // whose own type isn't covered by any `@throws` tag on the enclosing
    // declaration. Defaults to `false` - real PHPStan has no such rule at
    // all (checked-exception enforcement isn't part of core `phpstan/
    // phpstan`, and even the closest real third-party analogue isn't a
    // simple drop-in), and this project's own design doc explicitly flags
    // it as the riskiest item in the whole Phase 42 batch given the real
    // corpus's documented inconsistent `@throws` tagging - same "real
    // value, but real vendor-corpus noise" rationale, and the identical
    // wiring chain, as `check_internal_usage`/`check_deprecated_usage`.
    pub check_missing_checked_exceptions: bool,
    // Phase 61: opts into the "genuinely empty concrete method body"
    // return-completeness check (`Checker::check_function_with_ancestor_hint`'s
    // own `return.missing`/`return.never` fall-through). Defaults to
    // `false` - this was previously shipped, then reverted, after a
    // confirmed real corpus false-positive class (IDE-stub/native-
    // extension declaration files whose bodies are intentionally `{}`,
    // see ROADMAP.md's own Reverted section) - opt-in for a project that
    // scopes its own `paths`/excludes `vendor/` the way a realistic
    // PHPStan setup does. Same wiring chain as the three flags above.
    pub check_empty_method_bodies: bool,
    // Phase 69: opts into the strict (vs. the default, castable)
    // `PrintfPlaceholder::doesArgumentTypeMatchPlaceholder` matching mode
    // for `sprintf`/`printf`/`vsprintf`/`vprintf`/`fprintf` argument-type
    // checks (`checker/calls/format_string.rs`). Defaults to `false`,
    // matching real PHPStan's own stock config exactly (castable is its
    // default too) - same wiring chain as the flags above.
    pub check_strict_printf_placeholder_types: bool,
    // Phase 42 gating fix: mirrors real PHPStan's own `checkTooWideThrowTypesInProtectedAndPublicMethods`
    // (`vendor/phpstan-src/conf/config.neon`, default `false`) - without
    // it, `TooWideMethodThrowTypeRule` only ever runs for a `private`
    // method, a method on a `final` class, or a method itself marked
    // `final`, exactly matching real PHPStan's own default-off gate (a
    // real, corpus-confirmed false-positive flood otherwise: any ordinary
    // overridable method with an unused `@throws` tag is flagged even
    // though a subclass override might genuinely need the wider type).
    // When this flag is on, an overridable method still gets a pass the
    // first time it's declared (`is_first_declaration` below) - only a
    // *re-declaration* narrowing an ancestor's own wider `@throws` gets
    // checked. Same wiring chain as the flags above.
    pub check_too_wide_throw_type_in_protected_and_public_methods: bool,
    // Phase 45 (item 11, 45A): the return-type analogue of the flag just
    // above - mirrors real PHPStan's own `checkTooWideReturnTypesInProtectedAndPublicMethods`
    // gate for `TooWideMethodReturnTypehintRule`, identical wiring chain
    // and identical default-off reasoning (an overridable method's own
    // "unused" declared return-type member might genuinely be needed by
    // a subclass override).
    pub check_too_wide_return_type_in_protected_and_public_methods: bool,
    // Phase 42: opts into `MethodThrowTypeCovarianceRule` (`checker/oop/
    // method_overrides.rs`) - a real PHPStan rule (unlike `check_missing_
    // checked_exceptions`), but shipped opt-in here rather than mirroring
    // its own unconditional-by-default registration: this project's own
    // "@throws" model has no call-graph-wide implicit-throw inference,
    // and the real corpus's own documented inconsistent `@throws` tagging
    // makes even this rule's pure declaration-vs-declaration comparison
    // (own signature vs. immediate parent class/every transitive
    // interface/abstract trait declaration, real PHPStan's own default
    // `exceptions.implicitThrows: true` treating an untagged declaration
    // as "throws Throwable" throughout - hardcoded here rather than
    // separately configurable, since this project's own `@throws` never
    // distinguishes "explicitly untagged" from "implicit" the way real
    // PHPStan's `ResolvedPhpDocBlock` provenance does) worth verifying
    // against a real corpus before defaulting on. Same wiring chain as
    // the flags above.
    pub check_method_throw_type_covariance: bool,
    // Phase 83: opts into `checker::duplicate_classes::check_duplicate_
    // class_declarations` - unlike `duplicate_functions`'s own always-on
    // equivalent, this one is opt-in (default `false`) because its one
    // known corpus false-positive shape (a vendored built-in stub
    // override, e.g. `Imagick.stub.php`, colliding with the real PHP
    // extension's own class) can only be defused by a project's `stub_
    // paths` config being correctly populated, and no existing
    // `phpstan-rs.toml` does that yet. Same wiring chain as the flags
    // above; see `checker::duplicate_classes`'s own module doc comment
    // for the full design and why this goes beyond real PHPStan (which
    // has no whole-project equivalent of this check to mirror at all).
    pub check_duplicate_class_declarations: bool,
    // Phase 84: the project's own `level` setting, *before* `check-
    // project`'s own `MAX_LEVEL`-driven `.unwrap_or(6)` collapse - `None`
    // when never explicitly configured (CLI `--level`/config `level`
    // both absent), which `Registry::accepts_at_configured_level` treats
    // as "keep today's always-strict answer," not real PHPStan's own
    // lenient level-0 default. Not a CLI-configurable flag of its own the
    // way every other flag in this group is - set once from `check-
    // project`'s own already-resolved `level`/`config.level` merge,
    // mirroring `phpstan_api_check_enabled`'s own "computed once,
    // upstream of any single flag" precedent.
    pub configured_level: Option<u8>,
    // Phase 40.C: whether this project consumes real PHPStan as a
    // dependency at all (mirrors `Project::phpstan_api_check_enabled`,
    // set once from `composer::detect_phpstan_dependency` before any
    // file is checked - not a CLI-configurable flag of its own the way
    // every other flag in this group is). Gates the main `MethodCall`
    // arm's own `phpstanApi.method`/`phpParser.nodeConnectingAttribute`
    // checks (`checker/expr.rs`, `checker/phpstan_api.rs`), folded in
    // from `phpstan_api.rs`'s own previously-separate `Walker` pass,
    // which still runs separately for every *other* PHPStan-API-usage
    // check (instantiation, static calls, class-const fetches,
    // `instanceof`, `extends`/`implements`/trait-use). Defaults to
    // `false`, so every existing call site untouched by this is
    // completely unaffected.
    pub phpstan_api_check_enabled: bool,
    // Phase 40 follow-up: the one narrow, disclosed crack in this
    // struct's own "path-agnostic" contract (see `project/decl_
    // locations.rs`'s doc comment for why `ClassInfo`/`Registry` are
    // deliberately path-agnostic everywhere else) - needed to port real
    // `ApiRuleHelper::isPhpStanCode`'s own vendor-directory check
    // (`is_phpstan_code`, `checker/phpstan_api.rs`). Only ever populated
    // for a name already passing `is_phpstan_namespace`, so it stays
    // empty (zero cost) for every ordinary corpus with no `PHPStan\*`-
    // namespaced code of its own.
    pub phpstan_class_files: FxHashMap<String, std::path::PathBuf>,
    // Phase 67: a project's own configured `restricted_symbols` list
    // (`crate::restricted_symbols::RestrictedSymbolEntry`), compiled once
    // via `RestrictedSymbols::compile` - `Project::set_restricted_symbols`.
    // Empty by default, following `check_internal_usage`'s own registry-
    // field wiring shape (minus the CLI-flag hop, since this is a list,
    // not a boolean).
    pub restricted_symbols: crate::restricted_symbols::RestrictedSymbols,
    // Phase 38: the project's configured target PHP version, if any -
    // `None` (the default) means "not configured", which every
    // version-gated rule that reads this should treat as "nothing to
    // check yet" rather than assuming any particular version. Set via
    // `Project::set_php_version`, following `strict_mixed_arrays`'s exact
    // precedent (config.rs field -> CLI flag -> merge -> Project setter ->
    // this field). See `crate::php_version::PhpFeature` for the small
    // feature-availability lookup table this unlocks.
    pub php_version: Option<crate::php_version::PhpVersion>,
    // Phase 25: class names that have had at least one *unconditional*
    // declaration indexed so far - lets `Registry::insert_built_decl` tell "this
    // name's existing entry is itself unconditional, so a second
    // unconditional declaration should merge onto it" (Piwigo's own
    // `phpstan-stubs/Imagick.stub.php` shape) apart from "this name's
    // existing entry is only a conditional stub, so the first real
    // unconditional declaration should still replace it outright" (the
    // pre-existing `PHPUnit\Framework\TestCase` shape) - both look
    // identical from `self.classes.contains_key(&name)` alone.
    unconditionally_declared_classes: HashSet<String>,
    // Names `merge_stub_classes` provided a built-in native-class stub
    // for (jetbrains/phpstorm-stubs) - a *subset* of `unconditionally_
    // declared_classes` above, distinguishing "this name's existing
    // entry is itself only a type-checking fiction, never really
    // instantiated" from "this name's existing entry is a real,
    // unconditionally-declared vendor/first-party class". Both look
    // identical from `unconditionally_declared_classes.contains(&name)`
    // alone, but `Registry::insert_built_decl`'s own `if conditional` branch
    // needs to tell them apart: merging a version-gated real polyfill
    // implementation's own members onto a built-in stub is safe and
    // desired (the stub is fictional either way, see that branch's own
    // doc comment - `symfony/polyfill-php84`'s own `ReflectionConstant`
    // colliding with jetbrains's native-class stub for it), but doing
    // the identical merge onto a *real*, already-known vendor class
    // (the pre-existing `PHPUnit\Framework\TestCase`-vs-`class_exists`-
    // guarded-fallback shape `unconditionally_declared_classes` itself
    // was added for) would wrongly leak a defensive fallback's own
    // never-taken members onto the real class everything else sees -
    // exactly the regression `unconditional_class_wins_over_a_later_
    // conditional_stub` catches.
    pub(crate) built_in_stub_classes: HashSet<String>,
    // Phase 32 Finding C: the function-side sibling of the tracking set
    // above - phpstorm-stubs represents a real PHP internal function
    // overload (different signatures by argument shape, not version) as
    // two separate top-level `function` declarations sharing one name
    // (`strtr`'s own real stub: a 3-string-argument form and a
    // 2-argument, `array $replace_pairs` form, back to back in the same
    // file) - without this, the second declaration's blind
    // `self.functions.insert` silently discards the first's own params
    // entirely, instead of merging into a callable that accepts either
    // shape.
    unconditionally_declared_functions: HashSet<String>,
    // Phase 60 corpus find: names `merge_stubs` provided a signature for,
    // not yet overwritten by any real, already-indexed conditional
    // declaration - lets `Registry::insert_built_decl`'s own `BuiltDecl::
    // Function` conditional branch tell "this name's existing entry
    // is only the built-in stub's" apart from "this name's existing
    // entry is a real, already-indexed conditional declaration of the
    // same name from an earlier file" (both look identical from
    // `self.functions.contains_key(&name)` alone, the same ambiguity
    // `unconditionally_declared_functions` already exists to resolve on
    // the *unconditional* side). Without this, the extremely common
    // `if (!function_exists('str_contains')) { function str_contains
    // (?string $haystack, ?string $needle): bool {...} }` polyfill idiom
    // (real corpus shape: symfony/polyfill-php80) always lost to the
    // built-in stub's own native-only signature, silently discarding the
    // polyfill's real, more-precise (here, nullable) parameter types -
    // `entry(...).or_insert_with(...)` is a no-op once the stub already
    // occupies the slot, which `merge_stubs` guarantees happens first.
    stub_provided_functions: HashSet<String>,
    // Every bare function/method name that at least one indexed
    // `FunctionSig` anywhere in the registry has a non-empty `assert`
    // list for. `@phpstan-assert` is a rare docblock tag in real corpora
    // (confirmed: zero occurrences across the full Symfony monorepo plus
    // every vendored PHP stub) - `checker::stmt`'s own bare-statement
    // narrowing pass used to pay a full receiver-type inference plus
    // registry hierarchy walk on *every* bare `$x->method();`/
    // `Class::method();`/`func();` statement just to find out `sig.assert`
    // is empty; this lets it check simple set membership by name first
    // and skip straight past the expensive lookup for every name that
    // provably can't match. Populated once per `insert_built_decl` call
    // (see that method's own doc comment) - a name is only ever added,
    // never removed on `remove_declarations`, which just means an edited-
    // away assert tag leaves its old name in the set as a harmless,
    // rare false positive (still fully correct: this is a "may still do
    // the real check" set, not a "known good" one) rather than a
    // dangerous false negative - kept correct on every mutation path
    // this way (both the parallel batch build and the single-file LSP
    // `update_file` path route through the same `insert_built_decl`).
    pub(crate) names_with_assert_tags: HashSet<String>,
    // Every class's own precomputed `extends`+`implements` transitive
    // closure - see `is_subclass_of`/`precompute_ancestor_names`'s own
    // doc comments (`registry/hierarchy.rs`). Rebuilt from scratch
    // whenever the class graph could have changed (`Registry::
    // index_and_flatten`, `Project::finish_loading`/`update_file`) -
    // empty until the first such call, same "not yet built" precondition
    // every other post-load pass's own output already has.
    pub(crate) ancestor_names: FxHashMap<String, FxHashSet<String>>,
    // Every currently-registered class's own lowercased name -> canonical
    // key - see `lookup_class`'s own doc comment. A byproduct of
    // `canonicalize_class_name_case`'s own already-built `canonical_by_
    // lower` map, persisted here instead of discarded - that pass already
    // runs at every point this registry's class graph can have settled,
    // so this stays correct on exactly the same schedule `ancestor_names`
    // does, with no separate invalidation path of its own to get wrong.
    pub(crate) classes_lowercase: FxHashMap<String, String>,
    // Every currently-registered class's own precomputed member-name ->
    // owning-ancestor-name map, one per member kind - see `lookup_method`/
    // `Registry::precompute_member_owners`'s own doc comments
    // (`registry/hierarchy.rs`) for what these replace and why. Rebuilt
    // from scratch at exactly the same settle points `ancestor_names`/
    // `classes_lowercase` already are.
    pub(crate) method_owner_by_class: FxHashMap<String, FxHashMap<String, String>>,
    pub(crate) ancestor_method_owner_by_class: FxHashMap<String, FxHashMap<String, String>>,
    pub(crate) property_owner_by_class: FxHashMap<String, FxHashMap<String, String>>,
    pub(crate) ancestor_property_owner_by_class: FxHashMap<String, FxHashMap<String, String>>,
    pub(crate) const_owner_by_class: FxHashMap<String, FxHashMap<String, String>>,
}

impl Registry {
    pub fn build(program: &[Stmt]) -> Registry {
        let mut registry = Registry::index_and_flatten(program);
        registry.resolve_transitive_purity();
        registry
    }

    /// Every step `build` runs, *except* the final purity classification -
    /// see `resolve_transitive_purity`'s own call site in `build` for why
    /// that split exists. `php_stubs.rs`'s own `generate_from_source`/
    /// `generate_classes_from_source` call this directly instead of
    /// `build`: they only ever build a registry to extract raw
    /// `FunctionSig`/`ClassInfo` values out of vendored stub source, and a
    /// stub file's own function body is always empty - classifying it
    /// would mean every one of PHP's own built-in functions (`header()`,
    /// `exit()`, ...) looks provably pure, then gets that wrong verdict
    /// baked permanently into the `LazyLock`-cached stub signatures every
    /// real project's own registry later merges in (a real bug, found via
    /// full-corpus verification, not assumed) - `FunctionSig::is_stub`
    /// isn't even set yet at this point (`php_stubs.rs` sets it
    /// afterward, on the values this function returns), so classifying
    /// here couldn't have skipped them via that gate either way.
    pub(crate) fn index_and_flatten(program: &[Stmt]) -> Registry {
        let mut registry = Registry::default();
        registry.index(program, false);
        registry.canonicalize_class_name_case();
        registry.flatten_traits();
        registry.resolve_method_tags();
        // `flatten_phpdoc_inheritance` consumes `lookup_ancestor_method_
        // with_owner`/`lookup_method_with_owner` internally - the member-
        // owner maps must already reflect every name `flatten_traits`/
        // `resolve_method_tags` just added before it runs (see `Registry::
        // precompute_member_owners`'s own doc comment on why this can't
        // just be a single call at the very end the way `precompute_
        // ancestor_names` is).
        registry.precompute_member_owners();
        registry.flatten_phpdoc_inheritance();
        registry.resolve_deferred_value_of();
        registry.resolve_deferred_type_imports();
        // Adds more method/property names (`or_insert`-merges mixin
        // members in) - the member-owner maps need a second rebuild
        // after this, before anything else (this function's own callers,
        // or `resolve_transitive_purity` in `Registry::build`/`Project::
        // finish_loading`/`update_file`) consumes a lookup again.
        registry.resolve_deferred_mixins();
        registry.precompute_ancestor_names();
        registry.precompute_member_owners();
        registry
    }
}

#[cfg(test)]
mod tests;
