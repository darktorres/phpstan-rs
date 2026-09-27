use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

use syntax::ast::Stmt;
use types::Type;

pub mod access_attributes;
mod anonymous_class;
mod assertion_table;
mod class_info;
mod function_sig;
mod function_table;
mod generics;
mod global_vars;
mod hierarchy;
mod index;
mod name_context;
pub mod provenance;
mod purity;
mod relation;
mod resolve;
pub mod settings;
mod type_node;

pub(crate) use anonymous_class::collect_anonymous_classes;
pub(crate) use class_info::object_class_names;
pub use class_info::{ClassInfo, ConstSig, PropertySig};
pub(crate) use class_info::{hook_body_calls_parent_hook, hook_body_references_property};
pub(crate) use function_sig::apply_implicit_nullable_default;
pub use function_sig::format_signature;
pub(crate) use function_sig::is_allow_dynamic_properties_attribute_name;
pub(crate) use function_sig::is_deprecated_attribute_name;
pub(crate) use function_sig::is_no_discard_attribute_name;
pub(crate) use function_sig::is_override_attribute_name;
pub(crate) use function_sig::param_label;
pub(crate) use function_sig::resolve_type;
pub use function_sig::{AssertInfo, AssertTarget, FunctionSig, ParamSig};
pub(crate) use function_sig::{
    build_template_params, infer_const_type, mark_template_params, substitute_type_aliases,
};
pub use function_table::FunctionTable;
pub(crate) use function_table::{folded_key, function_key};
pub(crate) use index::{BuiltDecl, build_declarations, register_global_constant};
pub(crate) use name_context::{collect_class_name_case_refs, resolve_names_in_type};
pub use provenance::{Origin, Provenance};
pub use purity::PurityDependency;
pub(crate) use purity::{collect_impure_points, is_non_overridable};
pub(crate) use type_node::{mentions_mixed, parse_type_node};

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
    // Configured `[[assertions]]`/`[[class_argument_returns]]` entries,
    // on top of the built-in ones (`assertion_table`).
    pub(crate) configured_assertions: Vec<crate::assertion_tables::AssertionEntry>,
    pub(crate) configured_class_argument_returns: Vec<crate::assertion_tables::ClassArgumentReturn>,
    pub functions: FunctionTable,
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
    // The one narrow crack in this struct's "path-agnostic" contract
    // (see `project/decl_locations.rs` for why `ClassInfo`/`Registry`
    // are otherwise path-agnostic) - needed to port real
    // `ApiRuleHelper::isPhpStanCode`'s vendor-directory check
    // (`is_phpstan_code`, `checker/phpstan_api.rs`). Only populated for a
    // name already passing `is_phpstan_namespace`, so it stays empty for
    // any corpus with no `PHPStan\*`-namespaced code.
    pub phpstan_class_files: FxHashMap<String, std::path::PathBuf>,
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
    // The PHP versions the project runs on: exactly the configured
    // `php_version`, else the range its composer.json declares. `None`
    // means neither says, and every version-gated rule stays silent. See
    // `types::php_version::PhpFeature` for the feature table.
    pub php_versions: Option<types::PhpVersions>,
    // Class names with at least one *unconditional* declaration indexed
    // so far - lets `insert_built_decl` tell "this name's entry is
    // itself unconditional, so a second one should merge onto it"
    // (Piwigo's own `phpstan-stubs/Imagick.stub.php` shape) apart from
    // "this name's entry is only a conditional stub, so the first real
    // unconditional declaration should replace it outright" - both look
    // identical from `self.classes.contains_key(&name)` alone.
    unconditionally_declared_classes: FxHashSet<Arc<str>>,
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
    pub(crate) built_in_stub_classes: FxHashSet<Arc<str>>,
    // The function-side sibling of the tracking set above -
    // phpstorm-stubs represents a real PHP internal function overload
    // (different signatures by argument shape, not version) as two
    // separate top-level `function` declarations sharing one name
    // (`strtr`: a 3-string-argument form and a 2-argument,
    // `array $replace_pairs` form). Needed so the second declaration
    // merges into a callable accepting either shape, instead of a blind
    // `self.functions.insert` discarding the first's params.
    unconditionally_declared_functions: FxHashSet<Arc<str>>,
    // The declaration (its name's `(line, col)`) a `functions` slot was
    // built from - `None` once two declarations merged into it. Lets the
    // checker tell a second same-named declaration from the one the slot
    // describes.
    pub(crate) function_decl_sites: FxHashMap<Arc<str>, Option<(u32, u32)>>,
    // Names `merge_stubs` provided a signature for, not yet
    // overwritten by a real, already-indexed conditional declaration -
    // the same ambiguity `unconditionally_declared_functions` resolves
    // on the unconditional side. Needed so the common
    // `if (!function_exists('str_contains')) { function str_contains
    // (?string $haystack, ...) {...} }` polyfill idiom (real shape:
    // symfony/polyfill-php80) doesn't lose its real, more-precise
    // (here, nullable) parameter types to `entry(...).or_insert_with(...)`
    // being a no-op once the built-in stub already occupies the slot.
    stub_provided_functions: FxHashSet<Arc<str>>,
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
    pub(crate) names_with_assert_tags: FxHashSet<Arc<str>>,
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
    /// Whether `feature` is available on every PHP version the project
    /// runs on; `None` when none is known.
    pub(crate) fn php_supports_everywhere(&self, feature: types::PhpFeature) -> Option<bool> {
        self.php_versions.map(|v| v.supports(feature).yes())
    }

    /// Whether `feature` is available on some PHP version the project runs
    /// on; `None` when none is known.
    pub(crate) fn php_supports_somewhere(&self, feature: types::PhpFeature) -> Option<bool> {
        self.php_versions.map(|v| !v.supports(feature).no())
    }

    /// The lowest PHP version the project runs on.
    pub(crate) fn php_version_min(&self) -> Option<types::PhpVersion> {
        self.php_versions.map(|v| v.min)
    }

    /// The highest PHP version the project runs on.
    pub(crate) fn php_version_max(&self) -> Option<types::PhpVersion> {
        self.php_versions.map(|v| v.max)
    }

    /// `eval`'s answer on every PHP version the project runs on: the same
    /// at both ends of the range (every version gate is one threshold),
    /// else `None`. With no known version, `eval(None)`.
    pub(crate) fn php_version_consensus<T: PartialEq>(
        &self,
        eval: impl Fn(Option<types::PhpVersion>) -> Option<T>,
    ) -> Option<T> {
        match self.php_versions {
            None => eval(None),
            Some(range) => {
                let low = eval(Some(range.min));
                (low == eval(Some(range.max))).then_some(low).flatten()
            }
        }
    }

    pub fn build(program: &[Stmt]) -> Registry {
        let mut registry = Registry::index_and_flatten(program, &Origin::Unknown);
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
    pub(crate) fn index_and_flatten(program: &[Stmt], origin: &Origin) -> Registry {
        let mut registry = Registry::default();
        registry.index(program, origin, false);
        registry.canonicalize_class_name_case();
        registry.flatten_traits();
        registry.resolve_method_tags();
        // `flatten_phpdoc_inheritance` consumes `lookup_ancestor_method_
        // with_owner`/`lookup_method_with_owner` internally, so the
        // member-owner maps must already reflect every name
        // `flatten_traits`/`resolve_method_tags` just added.
        registry.precompute_member_owners();
        // Aliases and deferred value types resolve first, so inherited
        // PHPDoc copies an ancestor's finished types.
        registry.resolve_deferred_type_imports();
        registry.resolve_deferred_value_of();
        registry.flatten_phpdoc_inheritance();
        registry.inherit_property_hooks();
        registry.apply_assertion_table();
        // Adds more method/property names (merges mixin members in) -
        // the member-owner maps need a second rebuild after this.
        registry.resolve_deferred_mixins();
        registry.precompute_ancestor_names();
        registry.resolve_constant_named_types();
        registry.drop_doc_members_excluded_by_native();
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
