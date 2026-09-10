use rustc_hash::FxHashMap;
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
pub use class_info::{ClassInfo, ConstSig, PropertySig};
pub use function_sig::format_signature;
pub(crate) use function_sig::resolve_type;
pub use function_sig::{AssertInfo, AssertTarget, FunctionSig, ParamSig};
pub(crate) use function_sig::{infer_const_type, mark_template_params, substitute_type_aliases};
pub(crate) use name_context::{NameContext, resolve_names_in_type};
pub use purity::PurityDependency;
pub(crate) use purity::is_non_overridable;

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
    // declaration indexed so far - lets `index_with_context` tell "this
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
    // alone, but `index_with_context`'s own `if conditional` branch
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
    built_in_stub_classes: HashSet<String>,
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
    // declaration - lets `index_with_context`'s own `StmtKind::
    // FunctionDecl` conditional branch tell "this name's existing entry
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
        registry.index(program);
        registry.flatten_traits();
        registry.resolve_method_tags();
        registry.flatten_phpdoc_inheritance();
        registry.resolve_deferred_value_of();
        registry.resolve_deferred_type_imports();
        registry.resolve_deferred_mixins();
        registry
    }
}

#[cfg(test)]
mod tests;
