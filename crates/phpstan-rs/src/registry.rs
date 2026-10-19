use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::name::Name;

use syntax::ast::Stmt;
use types::Type;

pub mod access_attributes;
mod anonymous_class;
mod assertion_table;
pub mod bodies;
mod class_info;
mod container;
mod declaration_merge;
mod demand;
mod function_sig;
mod generics;
mod global_vars;
mod guard;
mod hierarchy;
mod index;
pub(crate) mod init_flow;
mod members;
mod name_context;
mod name_map;
mod name_table;
mod packed;
mod property_writes;
pub mod provenance;
mod purity;
pub mod reflection_attribute;
mod relation;
mod resolve;
pub(crate) mod scope;
mod shared;
mod stub_cache;
pub use stub_cache::CacheDir;
pub(crate) use stub_cache::cache_dir;
pub(crate) use stub_cache::probe as probe_writable;
mod summaries;
pub(crate) use stub_cache::{
    load as load_cached, load_container, store as store_cached, store_container,
};
pub mod settings;
pub mod stub_set;
pub mod stubs;
mod type_node;

pub(crate) use anonymous_class::{anonymous_class_decls, name_anonymous_classes};
pub use bodies::{BodySource, FileBodies, NoBodies};
pub use class_info::{ClassReflection, ConstantReflection, PropertyReflection};
pub(crate) use class_info::{hook_body_calls_parent_hook, hook_body_references_property};
pub(crate) use declaration_merge::DeclaredFile;
pub(crate) use function_sig::apply_implicit_nullable_default;
pub use function_sig::format_signature;
pub(crate) use function_sig::is_allow_dynamic_properties_attribute_name;
pub(crate) use function_sig::is_deprecated_attribute_name;
pub(crate) use function_sig::is_no_discard_attribute_name;
pub(crate) use function_sig::is_override_attribute_name;
pub(crate) use function_sig::param_label;
pub use function_sig::{AssertInfo, AssertTarget, FunctionReflection, ParameterReflection};
pub(crate) use function_sig::{DeclaredType, declared_type};
pub(crate) use function_sig::{
    build_template_params, declared_template_params, infer_const_type, mark_template_params,
    substitute_type_aliases,
};
pub use global_vars::GlobalFacts;
pub use init_flow::InitFlow;
pub use members::Members;
pub use name_map::{NameMap, NameSet};
pub use name_table::NameTable;
pub(crate) use name_table::{Detached, FoldedKey, Lazy};
pub use shared::Shared;
pub use summaries::BodySummary;
pub type FunctionTable = NameTable<FunctionReflection>;
pub(crate) use index::{BuiltDecl, DeclKind, built_declaration, register_global_constant};
// The unit tests call this in every profile, not only in debug builds.
#[cfg(any(debug_assertions, test))]
pub(crate) use index::build_declarations;
#[cfg(debug_assertions)]
pub(crate) use index::{Keep, build_declarations_where};
pub(crate) use name_context::{collect_class_name_case_refs, resolve_names_in_type};
pub(crate) use name_table::{folded_key, function_key, with_function_key};
pub(crate) use property_writes::PropertyWrites;
pub use provenance::{Origin, Provenance};
pub use purity::PurityDependency;
pub(crate) use purity::{collect_impure_points, is_non_overridable};
pub use reflection_attribute::{AttributeArgument, AttributeValue, ReflectionAttribute};
pub(crate) use type_node::parse_type_node;

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
/// The fewest classes or functions a finishing pass hands one parallel
/// job: a pass over fewer runs on the calling thread alone, without
/// waking the pool, as an edit's passes mostly do.
pub(crate) const PASS_CHUNK: usize = 32;

/// `Registry::changed`: lowercased class and function names, and
/// constant names as declared.
#[derive(Debug, Default, Clone)]
pub(crate) struct ChangedNames {
    pub(crate) classes: FxHashSet<String>,
    pub(crate) functions: FxHashSet<String>,
    // With each constant's type before its first change.
    pub(crate) constants: FxHashMap<String, Option<Type>>,
}

impl ChangedNames {
    /// Records a class declared or removed under `name`.
    pub(crate) fn record_class(&mut self, name: &str) {
        self.classes.insert(name.to_ascii_lowercase());
    }

    /// Records a function declared or removed under `name`.
    pub(crate) fn record_function(&mut self, name: &str) {
        self.functions.insert(name.to_ascii_lowercase());
    }

    /// Records a constant changed under `name`. `before` gives its type
    /// before the change, and is called only for its first change.
    pub(crate) fn record_constant(&mut self, name: &str, before: impl FnOnce() -> Option<Type>) {
        if !self.constants.contains_key(name) {
            self.constants.insert(name.to_string(), before());
        }
    }

    /// Adds the names `other` recorded. Each constant keeps the type it had
    /// before its first change, from either side.
    pub(crate) fn merge(&mut self, other: ChangedNames) {
        self.classes.extend(other.classes);
        self.functions.extend(other.functions);
        for (name, before) in other.constants {
            self.constants.entry(name).or_insert(before);
        }
    }
}

/// `scope_keys`' entries of `resolved` reset to `raw`'s (or removed with
/// them). Returns, for each of `snapshot_keys` in order, what `resolved`
/// held under it: taken out where the key is reset, copied where it is not.
fn reset_scoped<V: Clone>(
    resolved: &mut NameTable<V>,
    raw: &NameTable<V>,
    scope_keys: Option<&FxHashSet<String>>,
    snapshot_keys: Option<&FxHashSet<String>>,
) -> Vec<Option<Detached<V>>> {
    let reset = |resolved: &mut NameTable<V>, key: &String| {
        let old = resolved.detach(&FoldedKey(key));
        if let Some((name, _)) = raw.get_with_name(&FoldedKey(key))
            && let Some(value) = raw.get_shared(&FoldedKey(key))
        {
            resolved.insert_shared(name.clone(), value);
        }
        old
    };
    let mut before = Vec::new();
    let mut reset_count = 0;
    for key in snapshot_keys.into_iter().flatten() {
        before.push(if scope_keys.is_some_and(|keys| keys.contains(key)) {
            reset_count += 1;
            reset(resolved, key)
        } else {
            resolved.get_shared(&FoldedKey(key)).map(Detached::Own)
        });
    }
    // Scope keys the snapshot has none of, and every one without a snapshot.
    if reset_count < scope_keys.map_or(0, FxHashSet::len) {
        for key in scope_keys.into_iter().flatten() {
            if !snapshot_keys.is_some_and(|keys| keys.contains(key)) {
                reset(resolved, key);
            }
        }
    }
    before
}

/// Whether the resolved entry `now` differs from the one `before` took out:
/// an entry no pass wrote is the very value it was, which is equal, and only
/// a copy is compared field by field.
fn differs<V: PartialEq>(now: Option<&V>, before: Option<&Detached<V>>) -> bool {
    match (now, before) {
        (Some(now), Some(before)) => !std::ptr::eq(now, before.get()) && now != before.get(),
        (None, None) => false,
        _ => true,
    }
}

/// What `Registry::end_declaration_changes` returns.
pub(crate) struct ResolvedSnapshot {
    // The entries of the scope's keys, in the order of iterating the keys:
    // the key sets are shared with the scope, never changed.
    class_keys: Option<Arc<FxHashSet<String>>>,
    classes: Vec<Option<Detached<ClassReflection>>>,
    function_keys: Option<Arc<FxHashSet<String>>>,
    functions: Vec<Option<Detached<FunctionReflection>>>,
    global_variables: syntax::name::NameMap<Type>,
}

/// The sections a stored registry is in: one per table, the declarations as
/// built, and the rest.
const REST: &str = "rest";
const RAW_CLASSES: &str = "table:raw.classes";
const RAW_FUNCTIONS: &str = "table:raw.functions";

/// `Registry::store_tables` and `load_tables` for the tables named: each a
/// section called `table:<field>`.
macro_rules! stored_tables {
    ($($field:ident),* $(,)?) => {
        fn store_tables(&self, writer: &mut container::Writer) -> Result<(), String> {
            $(self.$field.store(writer, concat!("table:", stringify!($field)))?;)*
            Ok(())
        }

        #[cfg(test)]
        fn check_tables(&self) -> Result<(), String> {
            $(self.$field.check_order(concat!("table:", stringify!($field)))?;)*
            Ok(())
        }

        fn load_tables(&mut self, reader: &container::Reader) -> Result<(), String> {
            $(self.$field = container::Stored::load(
                reader,
                concat!("table:", stringify!($field)),
            )?;)*
            Ok(())
        }
    };
}

/// `Registry::raw`.
#[derive(Debug, Clone)]
struct RawDeclarations {
    classes: NameTable<ClassReflection>,
    functions: FunctionTable,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Registry {
    /// The built-in stubs' index and declarations this registry was made
    /// from, shared by the registries made from it. `None` only for a
    /// registry nothing was resolved into yet.
    #[serde(skip)]
    pub(crate) corpus: Option<Arc<stubs::Corpus>>,
    #[serde(skip)]
    pub classes: NameTable<ClassReflection>,
    // Configured `[[assertions]]`/`[[class_argument_returns]]` entries,
    // on top of the built-in ones (`assertion_table`).
    #[serde(skip)]
    pub(crate) configured_assertions: Vec<crate::assertion_tables::AssertionEntry>,
    #[serde(skip)]
    pub(crate) configured_class_argument_returns: Vec<crate::assertion_tables::ClassArgumentReturn>,
    #[serde(skip)]
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
    #[serde(skip)]
    pub global_constants: NameMap<Type>,
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
    pub global_variables: syntax::name::NameMap<Type>,
    // The one narrow crack in this struct's "path-agnostic" contract
    // (see `project/decl_locations.rs` for why `ClassReflection`/`Registry`
    // are otherwise path-agnostic) - needed to port real
    // `ApiRuleHelper::isPhpStanCode`'s vendor-directory check
    // (`is_phpstan_code`, `checker/phpstan_api.rs`). Only populated for a
    // name already passing `is_phpstan_namespace`, so it stays empty for
    // any corpus with no `PHPStan\*`-namespaced code.
    #[serde(skip)]
    pub phpstan_class_files: NameMap<std::path::PathBuf>,
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
    #[serde(skip)]
    doctrine_entity_index: std::sync::OnceLock<DoctrineEntityIndex>,
    // `classes` and `functions` as indexing and the stub merges built
    // them, before the finishing passes resolved them - kept so the
    // passes can run again from scratch after declarations change
    // (`keep_raw_declarations`).
    #[serde(skip)]
    raw: Option<Box<RawDeclarations>>,
    // Whether `classes` and `functions` hold the declarations as built,
    // swapped with `raw`'s resolved ones while declarations change
    // (`begin_declaration_changes`).
    #[serde(skip)]
    raw_active: bool,
    // The names declarations changed under since the finishing passes
    // last ran, once they have.
    #[serde(skip)]
    changed: ChangedNames,
    // Whether PHPStan's own source is merged in with the built-in stubs
    // (`Project::sync_phpstan_stubs`).
    pub(crate) phpstan_stubs_loaded: bool,
    // Which classes and functions the finishing passes resolve.
    #[serde(skip)]
    scope: scope::Scope,
    #[serde(skip)]
    scoped_names: scope::ScopedNames,
    // The PHP versions the project runs on: exactly the configured
    // `php_version`, else the range its composer.json declares. `None`
    // means neither says, and every version-gated rule stays silent. See
    // `types::php_version::PhpFeature` for the feature table.
    #[serde(skip)]
    pub php_versions: Option<types::PhpVersions>,
    // Class names with at least one *unconditional* declaration indexed
    // so far - lets `insert_built_decl` tell "this name's entry is
    // itself unconditional, so a second one should merge onto it"
    // (Piwigo's own `phpstan-stubs/Imagick.stub.php` shape) apart from
    // "this name's entry is only a conditional stub, so the first real
    // unconditional declaration should replace it outright" - both look
    // identical from `self.classes.contains_key(&name)` alone.
    #[serde(skip)]
    unconditionally_declared_classes: NameSet,
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
    #[serde(skip)]
    pub(crate) built_in_stub_classes: NameTable<()>,
    // The function-side sibling of the tracking set above -
    // phpstorm-stubs represents a real PHP internal function overload
    // (different signatures by argument shape, not version) as two
    // separate top-level `function` declarations sharing one name
    // (`strtr`: a 3-string-argument form and a 2-argument,
    // `array $replace_pairs` form). Needed so the second declaration
    // merges into a callable accepting either shape, instead of a blind
    // `self.functions.insert` discarding the first's params.
    #[serde(skip)]
    unconditionally_declared_functions: NameSet,
    // The declaration (its name's `(line, col)`, in the file the slot's
    // provenance names) a `functions` slot was built from - `None` once
    // two declarations merged into it, and no entry when no declaration
    // in a file won the slot (a built-in stub's). Lets the checker tell a
    // second same-named declaration from the one the slot describes.
    #[serde(skip)]
    pub(crate) function_decl_sites: NameMap<Option<u32>>,
    // Names `merge_stubs` provided a signature for, not yet
    // overwritten by a real, already-indexed conditional declaration -
    // the same ambiguity `unconditionally_declared_functions` resolves
    // on the unconditional side. Needed so the common
    // `if (!function_exists('str_contains')) { function str_contains
    // (?string $haystack, ...) {...} }` polyfill idiom (real shape:
    // symfony/polyfill-php80) doesn't lose its real, more-precise
    // (here, nullable) parameter types to `entry(...).or_insert_with(...)`
    // being a no-op once the built-in stub already occupies the slot.
    #[serde(skip)]
    stub_provided_functions: NameSet,
    // Every bare function/method name with a non-empty `assert` list
    // on some indexed `FunctionReflection`. `@phpstan-assert` is rare in real
    // corpora (zero occurrences across the full Symfony monorepo plus
    // every vendored PHP stub), so `checker::stmt`'s bare-statement
    // narrowing pass checks this set-membership first and skips the
    // expensive receiver-type inference + hierarchy walk for a name that
    // provably can't match. Names are only ever added, never removed on
    // `remove_declarations` - a harmless, rare false positive (still a
    // "may still do the real check" set, not a "known good" one), never
    // a false negative.
    #[serde(skip)]
    pub(crate) names_with_assert_tags: NameSet,
    // Every class's precomputed `extends`+`implements` transitive
    // closure - see `is_subclass_of`/`precompute_ancestor_names`
    // (`registry/hierarchy.rs`). Rebuilt whenever the class graph could
    // have changed.
    #[serde(skip)]
    pub(crate) ancestor_names: NameTable<syntax::name::NameSet>,
    // The classes, as declared, with a `PHPStan\` name anywhere among what
    // they extend, implement or use, transitively: one answer for each, made
    // with `ancestor_names` (`precompute_ancestor_names`).
    #[serde(skip)]
    pub(crate) phpstan_ancestry: NameTable<()>,
    // The reverse of each class's direct `extends`/`implements` edges, and
    // of its `uses`: a class-like's direct subtypes and direct trait users
    // (`precompute_ancestor_names`).
    #[serde(skip)]
    pub(crate) subtype_children: NameTable<syntax::name::NameSet>,
    #[serde(skip)]
    pub(crate) trait_users: NameTable<syntax::name::NameSet>,
    // Each class's direct supertypes and used traits as those two indexes
    // record them, to take it out of them again.
    #[serde(skip)]
    recorded_parents: NameTable<(Vec<Name>, Vec<Name>)>,
    // Every registered class's precomputed member-name ->
    // owning-ancestor-name map, one per member kind - see
    // `lookup_method`/`precompute_member_owners` (`registry/hierarchy.rs`).
    #[serde(skip)]
    pub(crate) member_owners: NameTable<hierarchy::ClassOwners>,
    // The purity and property writes found when asked (`registry::demand`).
    #[serde(skip)]
    demand: demand::Demand,
    // The same, found for every declaration at once, which a debug build
    // checks the ones found when asked against.
    #[cfg(debug_assertions)]
    #[serde(skip)]
    as_built: Option<std::sync::Arc<AsBuilt>>,
    // What each file's walk for global variables found, kept for the files
    // a later finish doesn't walk again (`infer_global_variables_of`).
    #[serde(skip)]
    global_sites: FxHashMap<crate::file_id::FileId, global_vars::FileSites>,
}

/// The purity and property writes of every declaration with a body, found
/// at once.
#[cfg(debug_assertions)]
#[derive(Debug, Default)]
struct AsBuilt {
    pure: FxHashSet<property_writes::DeclKey>,
    writes: FxHashMap<property_writes::DeclKey, PropertyWrites>,
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

    /// Whether `feature` is available on every PHP version the project runs
    /// on, on none, or on some; `Maybe` when none is known.
    pub(crate) fn php_supports(&self, feature: types::PhpFeature) -> types::TrinaryLogic {
        self.php_versions
            .map_or(types::TrinaryLogic::Maybe, |v| v.supports(feature))
    }

    /// The next index of an array being built, as the PHP versions the
    /// project runs on number it.
    pub(crate) fn next_array_index(&self) -> types::NextArrayIndex {
        types::NextArrayIndex::new(
            self.php_supports(types::PhpFeature::NegativeArrayKeysAdvanceTheNextIndex),
        )
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
        registry.reset_demand(&FileBodies::of(program));
        registry
    }

    /// Every step `build` runs except the final purity classification.
    /// `registry::stubs` calls this directly instead of `build` to extract
    /// raw `FunctionReflection`/`ClassReflection` values from vendored stub source - a
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
        registry.precompute_member_owners_after_mixins();
        registry.infer_global_variables(&[(
            crate::file_id::FileId::new(0),
            std::path::Path::new("<flattened>"),
            &GlobalFacts::of(program),
        )]);
        registry
    }

    /// Forgets the purity and property writes found when asked, for the
    /// declarations they were found from to change; returns them, to find
    /// again once the declarations have (`changed_since`). In a debug build
    /// the same are found for every declaration at once, to check the ones
    /// found when asked against.
    pub(crate) fn reset_demand(
        &mut self,
        bodies: &dyn BodySource,
    ) -> Vec<(property_writes::DeclKey, demand::Answer)> {
        #[cfg(debug_assertions)]
        {
            self.as_built = Some(std::sync::Arc::new(AsBuilt {
                pure: self.pure_among_every_declaration(bodies),
                writes: self.writes_of_every_declaration(bodies),
            }));
        }
        #[cfg(not(debug_assertions))]
        let _ = bodies;
        self.demand.take()
    }

    /// The answer found when asked that `key` has now.
    fn answer(
        &self,
        key: &property_writes::DeclKey,
        old: demand::Answer,
        bodies: &dyn BodySource,
    ) -> demand::Answer {
        match old {
            demand::Answer::Pure(_) => {
                demand::Answer::Pure(self.declaration_is_pure(key.clone(), bodies))
            }
            demand::Answer::Writes(_) => {
                demand::Answer::Writes(self.declaration_writes(key.clone(), bodies))
            }
        }
    }

    /// Makes `classes` and `functions` the declarations as built, for
    /// declarations to change on, once the finishing passes have resolved
    /// them; a no-op before then, while they still are the declarations as
    /// built.
    pub(crate) fn begin_declaration_changes(&mut self) {
        if let Some(raw) = &mut self.raw
            && !self.raw_active
        {
            std::mem::swap(&mut self.classes, &mut raw.classes);
            std::mem::swap(&mut self.functions, &mut raw.functions);
            self.raw_active = true;
        }
    }

    /// Whether the finishing passes have resolved declarations that are
    /// kept apart from the declarations as built.
    pub(crate) fn has_resolved_declarations(&self) -> bool {
        self.raw.is_some()
    }

    /// Takes the names declarations changed under since the last
    /// `end_declaration_changes`.
    pub(crate) fn take_changed(&mut self) -> ChangedNames {
        std::mem::take(&mut self.changed)
    }

    /// Makes `classes` and `functions` the resolved declarations again,
    /// with the entries in `scope` reset to their declarations as built
    /// (or removed with them), for the finishing passes to resolve. The
    /// first time, with `keep_raw`, keeps a copy of the declarations as
    /// built instead.
    ///
    /// With a `body_scope` (a scope with `scope` in it), returns the
    /// resolved forms those entries had, each taken out of the table
    /// rather than copied where `scope` resets it, and every property-write
    /// verdict, for `changed_since` to compare against. Only a registry
    /// that keeps its declarations as built has resolved forms to return.
    pub(crate) fn end_declaration_changes(
        &mut self,
        scope: &scope::Scope,
        body_scope: Option<&scope::Scope>,
        keep_raw: bool,
    ) -> Option<ResolvedSnapshot> {
        let Some(raw) = &mut self.raw else {
            if keep_raw {
                self.raw = Some(Box::new(RawDeclarations {
                    classes: self.classes.clone(),
                    functions: self.functions.clone(),
                }));
            }
            return None;
        };
        // While declarations change, the resolved tables sit in `raw`.
        let swap_back = self.raw_active;
        if swap_back {
            std::mem::swap(&mut self.classes, &mut raw.classes);
            std::mem::swap(&mut self.functions, &mut raw.functions);
            self.raw_active = false;
        }
        let snapshot = |classes, functions, registry: &Registry| ResolvedSnapshot {
            class_keys: body_scope.and_then(scope::Scope::shared_class_keys),
            classes,
            function_keys: body_scope.and_then(scope::Scope::shared_function_keys),
            functions,
            global_variables: registry.global_variables.clone(),
        };
        if scope.is_all() {
            self.classes = raw.classes.clone();
            self.functions = raw.functions.clone();
            return body_scope.map(|_| snapshot(Vec::new(), Vec::new(), self));
        }
        let classes = reset_scoped(
            &mut self.classes,
            &raw.classes,
            scope.class_keys(),
            body_scope.and_then(scope::Scope::class_keys),
        );
        let functions = reset_scoped(
            &mut self.functions,
            &raw.functions,
            scope.function_keys(),
            body_scope.and_then(scope::Scope::function_keys),
        );
        body_scope.map(|_| snapshot(classes, functions, self))
    }

    /// Makes the class, function and derived tables a shared base, for
    /// clones of the registry to share rather than copy.
    pub(crate) fn freeze(&mut self) {
        self.classes.freeze();
        self.functions.freeze();
        self.built_in_stub_classes.freeze();
        self.ancestor_names.freeze();
        self.phpstan_ancestry.freeze();
        self.subtype_children.freeze();
        self.trait_users.freeze();
        self.recorded_parents.freeze();
        self.member_owners.freeze();
        if let Some(raw) = &mut self.raw {
            raw.classes.freeze();
            raw.functions.freeze();
        }
    }

    pub(crate) fn record_changed_class(&mut self, name: &str) {
        if self.raw.is_some() {
            self.changed.record_class(name);
        }
    }

    pub(crate) fn record_changed_function(&mut self, name: &str) {
        if self.raw.is_some() {
            self.changed.record_function(name);
        }
    }

    fn record_changed_constant(&mut self, name: &str) {
        if self.raw.is_some() {
            let global_constants = &self.global_constants;
            self.changed
                .record_constant(name, || global_constants.get(name).cloned());
        }
    }

    /// The keys of the classes, functions and constants whose resolved
    /// form differs from `before` (and, for constants, from `changed`'s
    /// earlier types).
    pub(crate) fn changed_since(
        &self,
        before: &ResolvedSnapshot,
        changed: &ChangedNames,
        asked: &[(property_writes::DeclKey, demand::Answer)],
        bodies: &dyn BodySource,
    ) -> FxHashSet<crate::deps::RefKey> {
        use crate::deps::RefKey;
        let mut keys = FxHashSet::default();
        let class_keys = before.class_keys.iter().flat_map(|keys| keys.iter());
        debug_assert_eq!(class_keys.clone().count(), before.classes.len());
        for (key, old) in class_keys.zip(&before.classes) {
            if differs(self.classes.get(&FoldedKey(key)), old.as_ref()) {
                keys.insert(RefKey::class_text(key));
            }
        }
        let function_keys = before.function_keys.iter().flat_map(|keys| keys.iter());
        debug_assert_eq!(function_keys.clone().count(), before.functions.len());
        for (key, old) in function_keys.zip(&before.functions) {
            if differs(self.functions.get(&FoldedKey(key)), old.as_ref()) {
                keys.insert(RefKey::function_text(key));
            }
        }
        for (name, old) in &changed.constants {
            if self.global_constants.get(name.as_str()) != old.as_ref() {
                keys.insert(RefKey::constant_text(name));
            }
        }
        // An answer nothing asked for cannot have changed what anything
        // reads, so only those found since the declarations last changed
        // are found again.
        for (key, old) in asked {
            if self.answer(key, *old, bodies) != *old {
                keys.insert(match &key.class {
                    Some(class) => RefKey::class_text(class),
                    None => RefKey::function_text(&key.member),
                });
            }
        }
        keys
    }

    /// The keys of the global variables whose inferred type differs from
    /// `before`'s.
    pub(crate) fn changed_global_variables(
        &self,
        before: &ResolvedSnapshot,
    ) -> FxHashSet<crate::deps::RefKey> {
        before
            .global_variables
            .keys()
            .chain(self.global_variables.keys())
            .filter(|name| before.global_variables.get(*name) != self.global_variables.get(*name))
            .map(|name| crate::deps::RefKey::global_variable(name))
            .collect()
    }

    /// Writes the registry into `writer`: its tables as sections of their
    /// own, and the rest of it, encoded, as one.
    pub(crate) fn store(&self, writer: &mut container::Writer) -> Result<(), String> {
        debug_assert!(
            !self.raw_active,
            "the declarations as built are stored with the resolved ones"
        );
        self.store_tables(writer)?;
        if let Some(raw) = &self.raw {
            raw.classes.store(writer, RAW_CLASSES)?;
            raw.functions.store(writer, RAW_FUNCTIONS)?;
        }
        writer.section(REST, &stub_cache::encode(self)?)
    }

    /// Whether every stored table, map and set of a registry read back is
    /// in the order a lookup by bisection relies on. A load checks only
    /// that its records are inside the file, so the tests of what is stored
    /// call this on what they read back.
    #[cfg(test)]
    pub(crate) fn check_stored(&self) -> Result<(), String> {
        self.check_tables()?;
        if let Some(raw) = &self.raw {
            raw.classes.check_order(RAW_CLASSES)?;
            raw.functions.check_order(RAW_FUNCTIONS)?;
        }
        Ok(())
    }

    /// The registry stored under `name` for this build, if there is a
    /// readable file of it.
    pub(crate) fn load_cached(
        dir: Option<&stub_cache::CacheDir>,
        name: &str,
    ) -> Option<Result<Registry, String>> {
        let reader = load_container(dir, name)?;
        Some(Registry::load(&reader))
    }

    /// Stores the registry under `name` for this build.
    pub(crate) fn store_cached(
        &self,
        dir: Option<&stub_cache::CacheDir>,
        name: &str,
    ) -> Result<(), String> {
        if !dir.is_some_and(stub_cache::CacheDir::can_write) {
            return Ok(());
        }
        let mut writer = container::Writer::new();
        self.store(&mut writer)?;
        store_container(dir, name, writer);
        Ok(())
    }

    /// The registry `store` wrote, its tables read in place.
    pub(crate) fn load(reader: &container::Reader) -> Result<Registry, String> {
        let rest = reader.section(REST)?;
        let mut registry: Registry = stub_cache::decode(&reader.buffer()[rest])?;
        registry.load_tables(reader)?;
        if reader.has_section(RAW_CLASSES) {
            registry.raw = Some(Box::new(RawDeclarations {
                classes: NameTable::load(reader, RAW_CLASSES)?,
                functions: NameTable::load(reader, RAW_FUNCTIONS)?,
            }));
        }
        Ok(registry)
    }

    stored_tables!(
        classes,
        functions,
        built_in_stub_classes,
        ancestor_names,
        subtype_children,
        trait_users,
        recorded_parents,
        member_owners,
        global_constants,
        unconditionally_declared_classes,
        unconditionally_declared_functions,
        stub_provided_functions,
        names_with_assert_tags,
        function_decl_sites,
        phpstan_class_files,
    );

    /// Replaces the declarations with `base`'s, keeping the settings and
    /// the names changed so far.
    pub(crate) fn switch_base(&mut self, mut base: Registry) {
        base.php_versions = self.php_versions.take();
        base.configured_assertions = std::mem::take(&mut self.configured_assertions);
        base.configured_class_argument_returns =
            std::mem::take(&mut self.configured_class_argument_returns);
        base.changed = std::mem::take(&mut self.changed);
        *self = base;
    }

    /// The built-in stubs this registry was made from, to hold across a
    /// change to the registry.
    ///
    /// # Panics
    /// If the registry holds none: every registry a `Project` checks with
    /// takes its stubs from a `Stubs`.
    pub(crate) fn shared_corpus(&self) -> Arc<stubs::Corpus> {
        Arc::clone(
            self.corpus
                .as_ref()
                .expect("the registry was made without the built-in stubs"),
        )
    }

    /// See `shared_corpus`.
    pub(crate) fn corpus(&self) -> &stubs::Corpus {
        self.corpus
            .as_deref()
            .expect("the registry was made without the built-in stubs")
    }

    /// Drops what was computed lazily from the classes, for the next
    /// lookup to compute again from the classes as they are now.
    pub(crate) fn reset_lazy_tables(&mut self) {
        self.doctrine_entity_index = std::sync::OnceLock::new();
    }

    /// The `DoctrineEntity`-mapped classes by table and by repository, from
    /// one pass over the project's own classes (a built-in stub declares no
    /// entity), made when first asked for and dropped with the other lazy
    /// tables.
    fn doctrine_entity_index(&self) -> &DoctrineEntityIndex {
        self.doctrine_entity_index.get_or_init(|| {
            let mut index = DoctrineEntityIndex::default();
            for (class_name, info) in self.classes.own_iter() {
                let Some(entity) = &info.doctrine_entity else {
                    continue;
                };
                if let Some(table_name) = &entity.table_name {
                    index
                        .by_table
                        .entry(table_name.clone())
                        .or_insert_with(|| class_name.clone());
                }
                if let Some(repository) = &entity.repository_class {
                    index
                        .by_repository
                        .entry(repository.clone())
                        .or_default()
                        .push(class_name.clone());
                }
            }
            index
        })
    }

    /// The one `DoctrineEntity`-mapped class whose own `#[ORM\Table(name:
    /// ...)]` matches `table` exactly, if any - see `doctrine_entity_
    /// index`'s own doc comment for why this is lazily built rather than
    /// maintained incrementally.
    pub fn doctrine_entity_for_table(
        &self,
        table: &str,
    ) -> Option<&crate::registry::class_info::DoctrineEntity> {
        let class_name = self.doctrine_entity_index().by_table.get(table)?;
        self.classes.get(class_name)?.doctrine_entity.as_ref()
    }

    /// The entity that names `repository_fqn` as its own
    /// `#[ORM\Entity(repositoryClass: ...)]` argument, when exactly one
    /// does: one repository class can serve several entities (real,
    /// valid Doctrine), and then nothing in its own method bodies says
    /// which one a call site's instance is bound to.
    pub(crate) fn repository_entity(&self, repository_fqn: &str) -> Option<&str> {
        match self
            .doctrine_entity_index()
            .by_repository
            .get(repository_fqn)?
            .as_slice()
        {
            [only] => Some(only.as_ref()),
            _ => None,
        }
    }
}

/// `Registry::doctrine_entity_index`.
#[derive(Debug, Default, Clone)]
struct DoctrineEntityIndex {
    /// The first class mapped to each table.
    by_table: syntax::name::NameMap<Name>,
    /// Every class naming each repository class.
    by_repository: syntax::name::NameMap<Vec<Name>>,
}

#[cfg(test)]
mod tests;
