use std::sync::{Arc, OnceLock};

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::name::{Name, OrderedNameMap};

use syntax::ast::{NodeArenas, StmtId};
use types::Type;

pub mod access_attributes;
mod anonymous_class;
mod assertion_table;
pub mod bodies;
pub(crate) mod canonical;
mod class_info;
use class_info::{ResolvedTypeImport, TraitAlias};
mod container;
mod effects;
mod file_decls;
mod fresh;
mod function_sig;
mod generics;
mod global_vars;
mod guard;
mod hierarchy;
mod ids;
mod index;
pub(crate) mod init_flow;
mod members;
mod name_context;
mod name_map;
mod name_table;
mod packed;
mod per_class;
mod property_topology;
pub mod provenance;
mod purity;
mod reach_memo;
pub mod reflection_attribute;
mod relation;
mod resolve;
mod retention;
pub(crate) mod scope;
mod shared;
mod stub_cache;
pub(crate) use stub_cache::cache_dir;
pub(crate) mod summaries;
pub(crate) use stub_cache::{
    cache_key, load as load_cached, load_container, store as store_cached, store_container,
};
pub mod settings;
pub mod stubs;
mod type_node;

pub(crate) use container::{
    Reader as CacheReader, Unreadable as CacheUnreadable, Writer as CacheWriter,
};

pub(crate) use anonymous_class::{anonymous_class_decls, name_anonymous_classes};
pub use bodies::{BodySource, FileBodies, NoBodies};
pub(crate) use class_info::MethodEntry;
pub use class_info::{ClassReflection, ConstantReflection, PropertyReflection};
pub(crate) use class_info::{hook_body_calls_parent_hook, hook_body_references_property};
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
    build_template_params, infer_const_type, mark_template_params, substitute_type_aliases,
};
pub use global_vars::GlobalFacts;
pub use init_flow::InitFlow;
pub use members::Members;
pub use name_map::{NameMap, NameSet};
pub use name_table::NameTable;
pub(crate) use name_table::{Detached, Lazy};
pub use shared::Shared;
pub use summaries::BodySummary;
pub type FunctionTable = NameTable<FunctionReflection>;
pub(crate) use effects::{
    CallContext, Effect, Holder, Storage, Transfer, Written, elements_holder_of, holder_of,
};
pub(crate) use file_decls::{FileDecls, FileMember};
pub(crate) use guard::{Guard, GuardExpr};
pub(crate) use ids::{ClassId, FnId, IdSource, MethodRef, PropertyRef};
pub(crate) use index::{BuiltDecl, DeclKind, built_declaration, register_global_constant};
pub(crate) use name_context::{collect_class_name_case_refs, resolve_names_in_type};
pub(crate) use name_table::{folded_key, function_key, with_function_key};
pub(crate) use per_class::PerClass;
use property_topology::PropertyTopology;
pub(crate) use provenance::Origin;
pub use provenance::Provenance;
pub(crate) use purity::{PurityResults, collect_impure_points, is_non_overridable};
pub(crate) use reach_memo::ReachMemo;
pub use reflection_attribute::{AttributeArgument, AttributeValue, ReflectionAttribute};
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
    class_ids_before: FxHashMap<String, Option<ClassId>>,
    class_declarations_before: FxHashMap<ClassId, Arc<ClassReflection>>,
    class_names_before: FxHashMap<String, Name>,
    // Prior function state for names changed since the last finish. Keep the
    // values by declaration ID so removal or rebinding does not lose the
    // old winner before `changed_since` compares it.
    function_ids_before: FxHashMap<String, FnId>,
    function_declarations_before: FxHashMap<FnId, Arc<FunctionReflection>>,
    function_names_before: FxHashMap<String, Name>,
    function_results_before: FxHashMap<FnId, FunctionReflection>,
    // Class result tables are removed as soon as an edited declaration is
    // reindexed, so keep that winner's prior result by its old declaration ID.
    class_results_before: FxHashMap<ClassId, (Name, ClassResultOverlay)>,
    // With each constant's type before its first change.
    pub(crate) constants: FxHashMap<String, Option<Type>>,
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

/// A function's name, its declaration and its resolved result, as a
/// snapshot keeps them.
type FunctionSnapshot = (
    Name,
    Option<Detached<FunctionReflection>>,
    Option<FunctionReflection>,
);

/// What `Registry::end_declaration_changes` returns.
pub(crate) struct ResolvedSnapshot {
    // Keep the identities selected by the scope. Names are needed only when
    // a changed declaration is converted to a dependency key.
    class_ids: FxHashSet<ClassId>,
    functions: FxHashMap<FnId, FunctionSnapshot>,
    class_results: FxHashMap<ClassId, (Name, ClassResultOverlay)>,
    effects: FxHashMap<effects::EffectId, (Option<effects::DeclKey>, effects::Settled)>,
    global_variables: syntax::name::NameMap<Type>,
    global_scopes: Arc<global_vars::GlobalScopes>,
    include_targets: Arc<FxHashMap<(crate::file_id::FileId, u32), Option<crate::file_id::FileId>>>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ClassReferenceResults {
    pub(crate) extends: Vec<Name>,
    pub(crate) implements: Vec<Name>,
    pub(crate) uses: Vec<Name>,
    pub(crate) mixins: Vec<Name>,
    pub(crate) trait_aliases: Vec<TraitAlias>,
    pub(crate) type_imports: Vec<ResolvedTypeImport>,
}

pub(crate) struct ClassReferences<'a> {
    pub(crate) extends: &'a [Name],
    pub(crate) implements: &'a [Name],
    pub(crate) uses: &'a [Name],
    pub(crate) mixins: &'a [Name],
    pub(crate) trait_aliases: &'a [TraitAlias],
    pub(crate) type_imports: &'a [ResolvedTypeImport],
}

#[derive(Debug, Clone, Default, PartialEq)]
struct ClassResultOverlay {
    methods: Vec<(Name, FunctionReflection)>,
    topology: Option<hierarchy::MemberTopology>,
    properties: Option<Vec<(Name, Arc<PropertyReflection>)>>,
    constants: Option<OrderedNameMap<(usize, Arc<ConstantReflection>)>>,
    const_types: Vec<(usize, Type)>,
    types: Option<ClassTypeResults>,
    imported_aliases: Option<Vec<(Name, Type)>>,
    references: ClassReferenceResults,
}

/// Class-level generic types completed after class constants are available.
/// Names and values stay separate from the as-built ClassReflection.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ClassTypeResults {
    pub(crate) template_bounds: Vec<(Name, Type)>,
    pub(crate) extends_args: Option<Vec<Type>>,
    pub(crate) implements_args: Vec<(Name, Vec<Type>)>,
}

impl ClassTypeResults {
    pub(crate) fn is_empty(&self) -> bool {
        self.template_bounds.is_empty()
            && self.extends_args.is_none()
            && self.implements_args.is_empty()
    }
}

/// The sections a stored registry is in: one per table, the declarations as
/// built, and the rest.
const REST: &str = "rest";
const CLASS_REFERENCES: &str = "table:class_references";
const EFFECTS: &str = "table:effects";

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

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Registry {
    /// Changes whenever the declaration id space is replaced. IDs kept by
    /// clients or pass tables from an older base cannot alias new entries.
    #[serde(skip)]
    id_generation: u32,
    #[serde(skip)]
    relation_generation: Option<u64>,
    /// The answers to the hierarchy queries of effect resolution for the
    /// graph as it is; emptied whenever the graph can change.
    #[serde(skip)]
    reach_memo: ReachMemo,
    #[serde(skip)]
    purity_results: PurityResults,
    // Signatures completed by finishing passes, keyed by declaration ID.
    // The function table remains the as-built declaration source.
    #[serde(skip)]
    function_results: FxHashMap<FnId, FunctionReflection>,
    #[serde(skip)]
    function_results_cache: FunctionTable,
    #[serde(skip)]
    deferred_function_results: FxHashMap<FnId, OnceLock<Option<FunctionReflection>>>,
    #[serde(skip)]
    classes: NameTable<ClassReflection>,
    // Immutable declarations selected by the Registry's active ID space.
    // The name tables remain the insertion/cache boundary while declarations
    // are indexed; readers resolve through these ID-addressed Arc stores.
    #[serde(skip)]
    class_declarations: FxHashMap<ClassId, Arc<ClassReflection>>,
    #[serde(skip)]
    class_ids: NameTable<ClassId>,
    #[serde(skip)]
    project_class_names: Vec<Option<Name>>,
    /// The first class id local a declaration the registry places itself
    /// (a stub merged over a project's declarations) may take: the
    /// project's own high-water mark, so that no id it has handed to a
    /// file is taken twice.
    #[serde(skip)]
    class_local_floor: u32,
    #[serde(skip)]
    function_local_floor: u32,
    // Configured `[[assertions]]`/`[[class_argument_returns]]` entries,
    // on top of the built-in ones (`assertion_table`).
    #[serde(skip)]
    pub(crate) configured_assertions: Vec<crate::assertion_tables::AssertionEntry>,
    #[serde(skip)]
    pub(crate) configured_class_argument_returns: Vec<crate::assertion_tables::ClassArgumentReturn>,
    #[serde(skip)]
    functions: FunctionTable,
    #[serde(skip)]
    function_declarations: FxHashMap<FnId, Arc<FunctionReflection>>,
    #[serde(skip)]
    function_ids: NameTable<FnId>,
    #[serde(skip)]
    project_function_names: Vec<Option<Name>>,
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
    global_constants: NameMap<Type>,
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
    global_variables: syntax::name::NameMap<Type>,
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
    // reference), and repository -> the entities naming it, indexed from
    // the resolved classes after the finishing passes
    // (`index_doctrine_entities`). Two entities mapped to the same real
    // table name is a genuine, if unusual, Doctrine possibility (e.g. a
    // `readOnly` reporting entity mapped onto another entity's own table)
    // - the one first by name wins, whatever order the classes were
    // declared in.
    #[serde(skip)]
    doctrine_entity_index: Arc<DoctrineEntityIndex>,
    // Whether finishing passes have completed at least once; persisted so
    // incremental tracking remains active after loading the cache.
    resolved_declarations: bool,
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
    // Every bare function and method name a resolved declaration's
    // `assert` list is not empty under (`index_assert_tagged_names`): the
    // base's, and those of the declarations the registry holds itself.
    // `@phpstan-assert` is rare in real corpora, so `checker::stmt`'s
    // bare-statement narrowing pass checks this set-membership first and
    // skips the expensive receiver-type inference + hierarchy walk for a
    // name that provably can't match.
    #[serde(skip)]
    pub(crate) names_with_assert_tags: NameSet,
    // `names_with_assert_tags` of the base, as it was when frozen.
    #[serde(skip)]
    base_assert_tagged: Arc<NameSet>,
    // Every class's precomputed `extends`+`implements` transitive closure,
    // keyed by declaration ID. See `is_subclass_of` and
    // `precompute_ancestor_names` (`registry/hierarchy.rs`). Rebuilt whenever
    // the class graph could have changed.
    #[serde(skip)]
    pub(crate) ancestors_by_id: FxHashMap<ClassId, hierarchy::AncestorIds>,
    #[serde(skip)]
    ancestor_names_cache: NameTable<syntax::name::NameSet>,
    // The classes, as declared, with a `PHPStan\` name anywhere among what
    // they extend, implement or use, transitively: one answer for each, made
    // with `ancestor_names` (`precompute_ancestor_names`).
    #[serde(skip)]
    pub(crate) phpstan_ancestry: FxHashSet<ClassId>,
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
    // owning-ancestor-name map, keyed by class ID - see
    // `lookup_method`/`precompute_member_owners` (`registry/hierarchy.rs`).
    #[serde(skip)]
    pub(crate) member_owners: FxHashMap<ClassId, hierarchy::ClassOwners>,
    #[serde(skip)]
    member_owners_cache: NameTable<hierarchy::ClassOwners>,
    // Resolved class-constant values keyed by immutable declaration
    // identity. The reflection still carries a compatibility copy while
    // consumers move to this result table.
    #[serde(skip)]
    pub(crate) class_const_types: FxHashMap<(ClassId, usize), Type>,
    #[serde(skip)]
    class_const_types_by_id: FxHashMap<ClassId, Vec<(usize, Type)>>,
    #[serde(skip)]
    class_const_types_cache: NameTable<Vec<(u32, Type)>>,
    // Constants composed from traits, keyed by the using class and their
    // stable class-constant slot. Names are kept separately as topology for
    // lookup and cache projection; the result values themselves are slot-keyed.
    #[serde(skip)]
    pub(crate) class_constant_results: FxHashMap<(ClassId, usize), Arc<ConstantReflection>>,
    #[serde(skip)]
    class_constant_results_cache: NameTable<Vec<(Name, u32, Arc<ConstantReflection>)>>,
    #[serde(skip)]
    class_constant_names: FxHashMap<ClassId, Vec<Option<Name>>>,
    // Methods introduced by traits or mixins, keyed by the class that has
    // the local MethodRef. ClassReflection.methods remains as-built.
    #[serde(skip)]
    pub(crate) class_method_results: PerClass<MethodRef, Arc<Lazy<FunctionReflection>>>,
    #[serde(skip)]
    class_method_results_cache: NameTable<Members<FunctionReflection>>,
    // Hook facts inherited by a redeclared property, keyed by the class
    // declaration that owns the completed property. The underlying
    // PropertyReflection remains the as-built declaration.
    #[serde(skip)]
    pub(crate) class_property_results: PerClass<PropertyRef, Arc<PropertyReflection>>,
    #[serde(skip)]
    class_property_results_cache: NameTable<Vec<(Name, Arc<PropertyReflection>)>>,
    #[serde(skip)]
    class_property_topology: FxHashMap<ClassId, PropertyTopology>,
    #[serde(skip)]
    class_property_topology_cache: NameTable<Vec<Name>>,
    // Resolved class-level generic types, keyed by declaration ID.
    #[serde(skip)]
    pub(crate) class_type_results: FxHashMap<ClassId, ClassTypeResults>,
    #[serde(skip)]
    class_type_results_cache: NameTable<ClassTypeResults>,
    #[serde(skip)]
    deferred_class_results: FxHashMap<ClassId, OnceLock<Option<resolve::DeferredClassResults>>>,
    #[serde(skip)]
    deferred_types_ready: bool,
    #[serde(skip)]
    class_reference_results: FxHashMap<ClassId, ClassReferenceResults>,
    #[serde(skip)]
    class_reference_results_cache: NameTable<ClassReferenceResults>,
    // Resolved `@phpstan-import-type` aliases, keyed by the importing
    // declaration. `ClassReflection::type_aliases` contains local aliases
    // only and remains the immutable declaration source.
    #[serde(skip)]
    pub(crate) imported_type_aliases: FxHashMap<ClassId, NameMap<Type>>,
    #[serde(skip)]
    imported_type_aliases_cache: NameTable<Vec<(Name, Type)>>,
    // Host-local method entries introduced by finishing passes. The
    // declaration topology remains on `ClassReflection`; these extend it
    // by ID and preserve the slot numbers consumed by MethodRef.
    #[serde(skip)]
    pub(crate) class_member_topology: FxHashMap<ClassId, hierarchy::MemberTopology>,
    #[serde(skip)]
    class_member_topology_cache: NameTable<Vec<MethodEntry>>,
    // What a call to each function and method with a body may write
    // (`registry::effects`), keyed by declaration ID where one exists.
    #[serde(skip)]
    pub(crate) effects: FxHashMap<effects::EffectId, effects::Settled>,
    // The cache stores effects by stable declaration names, then freeze
    // translates them back to generation-checked EffectIds.
    #[serde(skip)]
    effects_cache: Option<Vec<(effects::DeclKey, effects::Settled)>>,
    // Whether a stub may run code passed to it, keyed by the same stable
    // declaration IDs used by effect settlement. Rebuilt before each effects
    // pass; unlike the signature, this is a derived Registry result.
    #[serde(skip)]
    pub(crate) stub_effect_results: FxHashMap<effects::EffectId, bool>,
    // The stub classes, lowercased, that no run of PHP has seen and that
    // have a method taking code or an object, whose other methods may run
    // what that one keeps. Rebuilt with `stub_effect_results`.
    #[serde(skip)]
    pub(crate) stub_holding_classes: FxHashSet<String>,
    // The builtin methods that run code passed to them
    // (`registry::effects`), by name, lowercased, with the classes declaring
    // each: the base's, and those of the classes the registry holds itself,
    // as the last finish found them. Stored with the registry, whose
    // classes a load reads lazily.
    #[serde(skip)]
    builtin_methods_running_code: NameMap<Vec<Name>>,
    // `builtin_methods_running_code` of the base, as it was when frozen.
    #[serde(skip)]
    base_builtin_methods_running_code: Arc<NameMap<Vec<Name>>>,
    // The loaded classes declaring each method name with a body, lowercased,
    // as the last finish found them (`registry::effects`).
    #[serde(skip)]
    methods_by_name: Arc<FxHashMap<String, Vec<ClassId>>>,
    // The properties the loaded files unset, as the last finish found them
    // (`registry::effects`).
    #[serde(skip)]
    unset_properties: Arc<bodies::UnsetProperties>,
    // The classes the loaded files register as stream wrappers, as the last
    // finish found them (`registry::effects`).
    #[serde(skip)]
    stream_wrappers: Arc<bodies::StreamWrappers>,
    // What each file's walk for global variables found, kept for the files
    // a later finish doesn't walk again (`infer_global_variables_of`).
    #[serde(skip)]
    global_sites: FxHashMap<crate::file_id::FileId, global_vars::FileSites>,
    // What the global variables may hold where each file's top level runs,
    // by the files that share one global scope.
    #[serde(skip)]
    global_scopes: Arc<global_vars::GlobalScopes>,
    // The file each include of the loaded files names, by the file it is in
    // and the id of its expression there; `None` when it names no single
    // loaded file. Found before the effects, which read it.
    #[serde(skip)]
    include_targets: Arc<FxHashMap<(crate::file_id::FileId, u32), Option<crate::file_id::FileId>>>,
}

impl Registry {
    /// Whether these registries share the same class-table base.
    pub(crate) fn shares_class_base(&self, other: &Registry) -> bool {
        self.classes.shares_base(&other.classes)
    }

    /// The literal-shaped global constants known to the project.
    pub fn global_constants(&self) -> &NameMap<Type> {
        &self.global_constants
    }

    /// The inferred global variables known to the project.
    pub fn global_variables(&self) -> &syntax::name::NameMap<Type> {
        &self.global_variables
    }

    /// The id and declaration selected for `name` by PHP's class-name rules.
    pub(crate) fn class(&self, name: &str) -> Option<(ClassId, &Name, &ClassReflection)> {
        let (name, id) = self.class_ids.get_with_name(name)?;
        Some((*id, name, self.class_by_id(*id)?))
    }

    /// The active runtime ID selected for a class name.
    pub(crate) fn class_id(&self, name: &str) -> Option<ClassId> {
        self.class_ids.get(name).copied()
    }

    /// The selected class declaration for a PHP class name, through its ID.
    pub fn class_by_name(&self, name: &str) -> Option<&ClassReflection> {
        self.class(name).map(|(_, _, declaration)| declaration)
    }

    /// Whether this Registry has an active declaration for `name`.
    pub fn has_class(&self, name: &str) -> bool {
        self.class_by_name(name).is_some()
    }

    /// Active class declarations in the Registry's name-table iteration
    /// order, resolved through their IDs.
    pub(crate) fn classes_by_id(&self) -> impl Iterator<Item = (ClassId, &Name, &ClassReflection)> {
        self.class_ids.iter().filter_map(|(name, id)| {
            self.class_by_id(*id)
                .map(|declaration| (*id, name, declaration))
        })
    }

    /// Iterates active class declarations with their selected names.
    pub fn iter_classes(&self) -> impl Iterator<Item = (&Name, &ClassReflection)> {
        self.classes_by_id()
            .map(|(_, name, declaration)| (name, declaration))
    }

    /// Active declarations owned by this Registry rather than its shared base.
    pub(crate) fn own_classes_by_id(
        &self,
    ) -> impl Iterator<Item = (ClassId, &Name, &ClassReflection)> {
        self.class_ids.own_iter().filter_map(|(name, id)| {
            self.class_by_id(*id)
                .map(|declaration| (*id, name, declaration))
        })
    }

    fn debug_check_id_generation(&self, generation: u32) {
        #[cfg(debug_assertions)]
        debug_assert_eq!(
            generation, self.id_generation,
            "declaration id belongs to a stale Registry generation"
        );
        #[cfg(not(debug_assertions))]
        let _ = generation;
    }

    /// The selected immutable class declaration addressed by its Registry ID.
    pub(crate) fn class_by_id(&self, id: ClassId) -> Option<&ClassReflection> {
        self.debug_check_id_generation(id.generation());
        self.class_declarations.get(&id).map(Arc::as_ref)
    }

    pub(crate) fn class_references(&self, id: ClassId) -> Option<ClassReferences<'_>> {
        let declaration = self.class_by_id(id)?;
        let results = self.class_reference_results.get(&id);
        Some(ClassReferences {
            extends: results.map_or(&declaration.extends, |refs| &refs.extends),
            implements: results.map_or(&declaration.implements, |refs| &refs.implements),
            uses: results.map_or(&declaration.uses, |refs| &refs.uses),
            mixins: results.map_or(&declaration.mixins, |refs| &refs.mixins),
            trait_aliases: results.map_or(&declaration.trait_aliases, |refs| &refs.trait_aliases),
            type_imports: results.map_or(&declaration.type_imports, |refs| &refs.type_imports),
        })
    }

    pub(crate) fn class_references_by_name(&self, name: &str) -> Option<ClassReferences<'_>> {
        self.class_ids
            .get(name)
            .copied()
            .and_then(|id| self.class_references(id))
    }

    pub(crate) fn set_class_const_type(&mut self, id: ClassId, slot: usize, ty: Type) {
        self.invalidate_deferred_class_result(id);
        self.class_const_types.insert((id, slot), ty.clone());
        let entries = self.class_const_types_by_id.entry(id).or_default();
        if let Some((_, previous)) = entries.iter_mut().find(|(existing, _)| *existing == slot) {
            *previous = ty;
        } else {
            entries.push((slot, ty));
        }
    }

    pub(crate) fn class_constant_result_by_name(
        &self,
        id: ClassId,
        name: &str,
    ) -> Option<&Arc<ConstantReflection>> {
        let slot = self.class_constant_slot_by_name(id, name)?;
        self.class_constant_results.get(&(id, slot))
    }

    pub(crate) fn class_constant_slot_by_name(&self, id: ClassId, name: &str) -> Option<usize> {
        let info = self.class_by_id(id)?;
        info.consts.get_index(name).or_else(|| {
            self.class_constant_names
                .get(&id)?
                .iter()
                .position(|candidate| candidate.as_deref() == Some(name))
                .map(|index| info.consts.len() + index)
        })
    }

    pub(crate) fn class_constant_name_by_slot(&self, id: ClassId, slot: usize) -> Option<&Name> {
        let info = self.class_by_id(id)?;
        if let Some(name) = info.consts.get_index_name(slot) {
            return Some(name);
        }
        self.class_constant_names
            .get(&id)?
            .get(slot.checked_sub(info.consts.len())?)?
            .as_ref()
    }

    pub(crate) fn class_constant_results_for_class(
        &self,
        id: ClassId,
    ) -> impl Iterator<Item = (usize, &Name, &Arc<ConstantReflection>)> {
        let declaration_len = self.class_by_id(id).map_or(0, |info| info.consts.len());
        self.class_constant_names
            .get(&id)
            .into_iter()
            .flat_map(move |names| names.iter().enumerate())
            .filter_map(move |(index, name)| {
                let slot = declaration_len + index;
                Some((
                    slot,
                    name.as_ref()?,
                    self.class_constant_results.get(&(id, slot))?,
                ))
            })
    }

    pub(crate) fn set_class_constant_result(
        &mut self,
        id: ClassId,
        slot: usize,
        name: Name,
        constant: Arc<ConstantReflection>,
    ) {
        let declaration_len = self.class_by_id(id).map_or(0, |info| info.consts.len());
        if slot >= declaration_len {
            let names = self.class_constant_names.entry(id).or_default();
            let composed_slot = slot - declaration_len;
            if names.len() <= composed_slot {
                names.resize_with(composed_slot + 1, || None);
            }
            names[composed_slot] = Some(name);
        }
        self.invalidate_deferred_class_result(id);
        self.class_constant_results.insert((id, slot), constant);
    }

    /// Local aliases declared on `id`, overlaid with aliases resolved from
    /// `@phpstan-import-type` tags. Callers get an owned map so the source
    /// declaration remains immutable.
    pub(crate) fn effective_type_aliases(&self, id: ClassId) -> syntax::name::NameMap<Type> {
        let mut aliases = self
            .class_by_id(id)
            .map(|class| class.type_aliases().clone())
            .unwrap_or_default();
        if let Some(imported) = self.imported_type_aliases.get(&id) {
            aliases.extend(imported.iter().map(|(name, ty)| (name.clone(), ty.clone())));
        }
        aliases
    }

    /// Name-addressed convenience for checker paths which already carry a
    /// class name rather than its declaration ID.
    pub(crate) fn effective_type_aliases_by_name(&self, name: &str) -> syntax::name::NameMap<Type> {
        self.class_ids
            .get(name)
            .copied()
            .map(|id| self.effective_type_aliases(id))
            .unwrap_or_default()
    }

    pub(crate) fn effective_template_bounds(&self, id: ClassId) -> syntax::name::NameMap<Type> {
        let mut bounds = self
            .class_by_id(id)
            .map(|class| class.template_bounds().clone())
            .unwrap_or_default();
        if let Some(results) = self.class_type_results.get(&id) {
            bounds.extend(results.template_bounds.clone());
        }
        if let Some(results) = self.deferred_class_result(id) {
            bounds.extend(results.types.template_bounds.clone());
        }
        bounds
    }

    pub(crate) fn effective_template_bounds_by_name(
        &self,
        name: &str,
    ) -> syntax::name::NameMap<Type> {
        self.class_ids
            .get(name)
            .copied()
            .map(|id| self.effective_template_bounds(id))
            .unwrap_or_default()
    }

    pub(crate) fn effective_extends_args(&self, id: ClassId) -> Vec<Type> {
        if let Some(args) = self
            .deferred_class_result(id)
            .and_then(|results| results.types.extends_args.as_ref())
        {
            return args.clone();
        }
        if let Some(args) = self
            .class_type_results
            .get(&id)
            .and_then(|results| results.extends_args.as_ref())
        {
            return args.clone();
        }
        self.class_by_id(id)
            .map(|class| class.extends_args().clone())
            .unwrap_or_default()
    }

    pub(crate) fn effective_extends_args_by_name(&self, name: &str) -> Vec<Type> {
        self.class_ids
            .get(name)
            .copied()
            .map(|id| self.effective_extends_args(id))
            .unwrap_or_default()
    }

    pub(crate) fn effective_implements_args(
        &self,
        id: ClassId,
    ) -> syntax::name::NameMap<Vec<Type>> {
        let mut args = self
            .class_by_id(id)
            .map(|class| class.implements_args().clone())
            .unwrap_or_default();
        if let Some(results) = self.class_type_results.get(&id) {
            args.extend(results.implements_args.clone());
        }
        if let Some(results) = self.deferred_class_result(id) {
            args.extend(results.types.implements_args.clone());
        }
        args
    }

    pub(crate) fn effective_implements_args_by_name(
        &self,
        name: &str,
    ) -> syntax::name::NameMap<Vec<Type>> {
        self.class_ids
            .get(name)
            .copied()
            .map(|id| self.effective_implements_args(id))
            .unwrap_or_default()
    }

    pub(crate) fn class_name_by_id(&self, id: ClassId) -> Option<&Name> {
        self.debug_check_id_generation(id.generation());
        let name = match id.source()? {
            IdSource::Stub => self.classes.base_name(id.local() as usize)?,
            IdSource::Context => return None,
            IdSource::Project => self
                .project_class_names
                .get(id.local() as usize)?
                .as_ref()?,
        };
        (self.class_ids.get(name) == Some(&id)).then_some(name)
    }

    /// The id and declaration selected for `name` by PHP's function-name rules.
    pub(crate) fn function(&self, name: &str) -> Option<(FnId, &Name, &FunctionReflection)> {
        let (name, id) = self.function_ids.get_with_name(name)?;
        Some((*id, name, self.function_by_id(*id)?))
    }

    /// The selected function signature for a PHP function name, through its ID.
    pub fn function_by_name(&self, name: &str) -> Option<&FunctionReflection> {
        self.function(name).map(|(_, _, declaration)| declaration)
    }

    /// Whether this Registry has an active declaration for `name`.
    pub fn has_function(&self, name: &str) -> bool {
        self.function_by_name(name).is_some()
    }

    /// Active function declarations in the Registry's name-table iteration
    /// order, resolved through their IDs.
    pub(crate) fn functions_by_id(
        &self,
    ) -> impl Iterator<Item = (FnId, &Name, &FunctionReflection)> {
        self.function_ids.iter().filter_map(|(name, id)| {
            self.function_by_id(*id)
                .map(|declaration| (*id, name, declaration))
        })
    }

    /// Iterates active function declarations with their selected names.
    pub fn iter_functions(&self) -> impl Iterator<Item = (&Name, &FunctionReflection)> {
        self.functions_by_id()
            .map(|(_, name, declaration)| (name, declaration))
    }

    /// Active declarations owned by this Registry rather than its shared base.
    pub(crate) fn own_functions_by_id(
        &self,
    ) -> impl Iterator<Item = (FnId, &Name, &FunctionReflection)> {
        self.function_ids.own_iter().filter_map(|(name, id)| {
            self.function_by_id(*id)
                .map(|declaration| (*id, name, declaration))
        })
    }

    /// The selected immutable function signature addressed by its Registry ID.
    pub(crate) fn function_by_id(&self, id: FnId) -> Option<&FunctionReflection> {
        self.debug_check_id_generation(id.generation());
        if self.deferred_types_ready && id.source() != Some(IdSource::Stub) {
            if let Some(result) = self.function_results.get(&id) {
                return Some(result);
            }
            if let Some(cell) = self.deferred_function_results.get(&id)
                && let Some(result) = cell.get_or_init(|| self.resolved_deferred_function(id))
            {
                return Some(result);
            }
        }
        if let Some(result) = self.function_results.get(&id) {
            return Some(result);
        }
        self.function_declaration_by_id(id)
    }

    /// The as-built function declaration selected for a PHP function name,
    /// without applying resolved signature results.
    #[cfg(test)]
    pub(crate) fn function_declaration_by_name(&self, name: &str) -> Option<&FunctionReflection> {
        self.function_ids
            .get(name)
            .copied()
            .and_then(|id| self.function_declaration_by_id(id))
    }

    /// The active runtime ID selected for a function name.
    pub(crate) fn function_id(&self, name: &str) -> Option<FnId> {
        self.function_ids.get(name).copied()
    }

    /// The immutable as-built function declaration. A resolved signature,
    /// when present, is read separately from `function_results`.
    pub(crate) fn function_declaration_by_id(&self, id: FnId) -> Option<&FunctionReflection> {
        self.debug_check_id_generation(id.generation());
        self.function_declarations.get(&id).map(Arc::as_ref)
    }

    pub(crate) fn function_name_by_id(&self, id: FnId) -> Option<&Name> {
        self.debug_check_id_generation(id.generation());
        let name = match id.source()? {
            IdSource::Stub => self.functions.base_name(id.local() as usize)?,
            IdSource::Context => return None,
            IdSource::Project => self
                .project_function_names
                .get(id.local() as usize)?
                .as_ref()?,
        };
        (self.function_ids.get(name) == Some(&id)).then_some(name)
    }

    /// The class id local for a declaration placed without one reserved:
    /// above every name held and every local the project has handed out.
    fn next_class_local(&mut self) -> u32 {
        let local = u32::try_from(self.project_class_names.len())
            .expect("project class id range exhausted")
            .max(self.class_local_floor);
        self.class_local_floor = local
            .checked_add(1)
            .expect("project class id range exhausted");
        local
    }

    fn next_function_local(&mut self) -> u32 {
        let local = u32::try_from(self.project_function_names.len())
            .expect("project function id range exhausted")
            .max(self.function_local_floor);
        self.function_local_floor = local
            .checked_add(1)
            .expect("project function id range exhausted");
        local
    }

    /// Raises the first id locals the registry itself places declarations
    /// at to the project's high-water marks.
    pub(crate) fn raise_local_floors(&mut self, classes: u32, functions: u32) {
        self.class_local_floor = self.class_local_floor.max(classes);
        self.function_local_floor = self.function_local_floor.max(functions);
    }

    /// The first class and function id locals nothing has taken.
    pub(crate) fn local_floors(&self) -> (u32, u32) {
        (self.class_local_floor, self.function_local_floor)
    }

    fn ensure_class_id(&mut self, name: &Name, reserved: Option<u32>) {
        if let Some(existing_id) = self.class_ids.get(name).copied() {
            if existing_id.source() == Some(IdSource::Project) {
                return;
            }
            let declaration = self.class_declarations.get(&existing_id).cloned();
            let local = reserved.unwrap_or_else(|| self.next_class_local());
            let id = ClassId::project_in_generation(local, self.id_generation);
            self.class_ids.insert(name.clone(), id);
            if self.project_class_names.len() <= id.local() as usize {
                self.project_class_names
                    .resize_with(id.local() as usize + 1, || None);
            }
            self.project_class_names[id.local() as usize] = Some(name.clone());
            if let Some(declaration) = declaration {
                self.class_declarations.insert(id, declaration);
            }
            return;
        }
        let local = reserved.unwrap_or_else(|| self.next_class_local());
        let id = ClassId::project_in_generation(local, self.id_generation);
        self.class_ids.insert(name.clone(), id);
        if self.project_class_names.len() <= id.local() as usize {
            self.project_class_names
                .resize_with(id.local() as usize + 1, || None);
        }
        self.project_class_names[id.local() as usize] = Some(name.clone());
    }

    fn ensure_function_id(&mut self, name: &Name, reserved: Option<u32>) {
        if let Some(existing_id) = self.function_ids.get(name).copied() {
            if existing_id.source() == Some(IdSource::Project) {
                return;
            }
            let declaration = self.function_declarations.get(&existing_id).cloned();
            let local = reserved.unwrap_or_else(|| self.next_function_local());
            let id = FnId::project_in_generation(local, self.id_generation);
            self.function_ids.insert(name.clone(), id);
            if self.project_function_names.len() <= id.local() as usize {
                self.project_function_names
                    .resize_with(id.local() as usize + 1, || None);
            }
            self.project_function_names[id.local() as usize] = Some(name.clone());
            if let Some(declaration) = declaration {
                self.function_declarations.insert(id, declaration);
            }
            return;
        }
        let local = reserved.unwrap_or_else(|| self.next_function_local());
        let id = FnId::project_in_generation(local, self.id_generation);
        self.function_ids.insert(name.clone(), id);
        if self.project_function_names.len() <= id.local() as usize {
            self.project_function_names
                .resize_with(id.local() as usize + 1, || None);
        }
        self.project_function_names[id.local() as usize] = Some(name.clone());
    }

    fn remove_project_class_id(&mut self, name: &Name) {
        if let Some(id) = self
            .class_ids
            .get(name)
            .copied()
            .filter(|id| id.source() == Some(IdSource::Project))
        {
            self.class_ids.remove(name);
            self.class_declarations.remove(&id);
            self.purity_results
                .pure_methods
                .retain(|method| method.class() != id);
            self.purity_results
                .method_dependencies
                .retain(|method, _| method.class() != id);
            self.purity_results
                .fresh_methods
                .retain(|method| method.class() != id);
            self.class_const_types.retain(|(class, _), _| *class != id);
            self.class_const_types_by_id.remove(&id);
            self.class_constant_results
                .retain(|(class, _), _| *class != id);
            self.class_constant_names.remove(&id);
            self.class_method_results.remove_class(id);
            self.class_member_topology.remove(&id);
            self.class_property_results.remove_class(id);
            self.class_property_topology.remove(&id);
            self.class_type_results.remove(&id);
            self.imported_type_aliases.remove(&id);
        }
    }

    fn remove_project_function_id(&mut self, name: &Name) {
        if let Some(id) = self
            .function_ids
            .get(name)
            .copied()
            .filter(|id| id.source() == Some(IdSource::Project))
        {
            self.function_ids.remove(name);
            self.function_declarations.remove(&id);
            self.function_results.remove(&id);
            self.purity_results.pure_functions.remove(&id);
            self.purity_results.function_dependencies.remove(&id);
            self.purity_results
                .released_parameters
                .retain(|(function, _)| *function != id);
            self.purity_results.fresh_functions.remove(&id);
        }
    }

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

    pub fn build(arenas: &NodeArenas, program: &[StmtId]) -> Registry {
        let mut registry = Registry::index_and_flatten(arenas, program, &Origin::Unknown);
        registry.resolve_transitive_purity(&FileBodies::of(arenas, program));
        registry
    }

    /// Every step `build` runs except the final purity classification.
    /// `registry::stubs` calls this directly instead of `build` to extract
    /// raw `FunctionReflection`/`ClassReflection` values from vendored stub source - a
    /// stub's function body is always empty, so classifying it here
    /// would wrongly mark every PHP built-in (`header()`, `exit()`, ...)
    /// as provably pure, baked permanently into the `LazyLock`-cached
    /// stub signatures every real project's registry later merges in.
    pub(crate) fn index_and_flatten(
        arenas: &NodeArenas,
        program: &[StmtId],
        origin: &Origin,
    ) -> Registry {
        let mut registry = Registry::default();
        registry.index(arenas, program, origin, false);
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
        registry.index_assert_tagged_names();
        registry.index_doctrine_entities();
        registry.infer_global_variables(&[(
            crate::file_id::FileId::new(0),
            std::path::Path::new("<flattened>"),
            &GlobalFacts::of(arenas, program),
        )]);
        registry.advance_relation_generation();
        registry
    }

    /// Makes the class table the declarations as built, for
    /// declarations to change on, once the finishing passes have resolved
    /// them; a no-op before then, while they still are the declarations as
    /// built.
    pub(crate) fn begin_declaration_changes(&mut self) {
        self.relation_generation = None;
        self.reach_memo = ReachMemo::default();
        self.deferred_types_ready = false;
        self.deferred_function_results.clear();
        self.deferred_class_results.clear();
        // Declarations remain as built. Finishing results are reset by ID.
        self.stub_effect_results.clear();
        self.stub_holding_classes.clear();
    }

    /// Starts a stable hierarchy snapshot for the next read-only finishing
    /// pass. The following pass receives a distinct memo generation.
    pub(crate) fn advance_relation_generation(&mut self) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.relation_generation = Some(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        self.reach_memo = ReachMemo::default();
    }

    /// Whether the finishing passes have resolved declarations that are
    /// kept apart from the declarations as built.
    pub(crate) fn has_resolved_declarations(&self) -> bool {
        self.resolved_declarations
    }

    /// Takes the names declarations changed under since the last
    /// `end_declaration_changes`.
    pub(crate) fn take_changed(&mut self) -> ChangedNames {
        std::mem::take(&mut self.changed)
    }

    /// Makes the class table the resolved declarations again, with the
    /// entries in `scope` reset to their declarations as built (or removed
    /// with them), for the finishing passes to resolve. Function
    /// declarations stay in their as-built table; finishing-pass results
    /// are kept in `function_results`. The first time, with `keep_raw`,
    /// keeps a copy of the declarations as built instead.
    ///
    /// With a `body_scope` (a scope with `scope` in it), returns the
    /// resolved class forms those entries had, each taken out of the table
    /// rather than copied where `scope` resets it, the prior function
    /// declarations and result overlays, and every property-write verdict,
    /// for `changed_since` to compare against. Only a registry that keeps
    /// its declarations as built has resolved forms to return.
    pub(crate) fn end_declaration_changes(
        &mut self,
        scope: &scope::Scope,
        body_scope: Option<&scope::Scope>,
        keep_raw: bool,
        changed: Option<&ChangedNames>,
    ) -> Option<ResolvedSnapshot> {
        if !self.resolved_declarations {
            self.resolved_declarations = keep_raw;
            return None;
        }
        let mut snapshot_class_ids = FxHashSet::default();
        let mut snapshot_function_ids = FxHashSet::default();
        if let Some(body_scope) = body_scope {
            if let Some(ids) = body_scope.class_ids() {
                snapshot_class_ids.extend(ids);
            } else if body_scope.is_all() {
                snapshot_class_ids.extend(self.class_ids.values().copied());
            }
            if let Some(ids) = body_scope.function_ids() {
                snapshot_function_ids.extend(ids);
            } else if body_scope.is_all() {
                snapshot_function_ids.extend(self.function_ids.values().copied());
            }
            for name in body_scope.class_keys().into_iter().flatten() {
                if let Some(id) = self.class_ids.get(name.as_str()) {
                    snapshot_class_ids.insert(*id);
                }
            }
            for name in body_scope.function_keys().into_iter().flatten() {
                if let Some(id) = self.function_ids.get(name.as_str()) {
                    snapshot_function_ids.insert(*id);
                }
                if let Some(id) = changed.and_then(|changed| changed.function_ids_before.get(name))
                {
                    snapshot_function_ids.insert(*id);
                }
            }
        }
        // A changed declaration may already have been removed or rebound in
        // the active ID table. Keep its previous ID in the snapshot so the
        // comparison still reports the old winner.
        if let (Some(body_scope), Some(changed)) = (body_scope, changed) {
            for (name, old_id) in &changed.function_ids_before {
                if body_scope.is_all()
                    || self
                        .function_ids
                        .get(name.as_str())
                        .is_some_and(|id| snapshot_function_ids.contains(id))
                {
                    snapshot_function_ids.insert(*old_id);
                }
            }
            if let Some(names) = body_scope.function_keys() {
                for name in names {
                    if let Some(id) = changed.function_ids_before.get(name) {
                        snapshot_function_ids.insert(*id);
                    }
                }
            }
            for (name, old_id) in &changed.class_ids_before {
                if (body_scope.is_all()
                    || self
                        .class_ids
                        .get(name.as_str())
                        .is_some_and(|id| snapshot_class_ids.contains(id)))
                    && let Some(old_id) = old_id
                {
                    snapshot_class_ids.insert(*old_id);
                }
            }
            if let Some(names) = body_scope.class_keys() {
                for name in names {
                    if let Some(Some(id)) = changed.class_ids_before.get(name) {
                        snapshot_class_ids.insert(*id);
                    }
                }
            }
        }
        let active_function_names: FxHashMap<FnId, Name> = self
            .function_ids
            .iter()
            .map(|(name, id)| (*id, name.clone()))
            .collect();
        let functions_before: FxHashMap<FnId, FunctionSnapshot> = snapshot_function_ids
            .into_iter()
            .filter_map(|id| {
                let name = active_function_names.get(&id).cloned().or_else(|| {
                    changed.and_then(|changed| {
                        changed
                            .function_ids_before
                            .iter()
                            .find_map(|(name, changed_id)| {
                                (*changed_id == id).then(|| Name::from(name))
                            })
                    })
                })?;
                let declaration = changed
                    .and_then(|changed| changed.function_declarations_before.get(&id))
                    .cloned()
                    .or_else(|| self.function_declarations.get(&id).cloned());
                let result = changed
                    .and_then(|changed| changed.function_results_before.get(&id))
                    .cloned()
                    .or_else(|| self.function_results.get(&id).cloned());
                Some((id, (name, declaration.map(Detached::Own), result)))
            })
            .collect();
        let mut class_results_before = FxHashMap::default();
        if let Some(body_scope) = body_scope {
            let active_class_names: FxHashMap<ClassId, Name> = self
                .class_ids
                .iter()
                .map(|(name, id)| (*id, name.clone()))
                .collect();
            for id in body_scope.class_ids().into_iter().flatten() {
                let Some(class_name) = active_class_names.get(&id) else {
                    continue;
                };
                class_results_before.insert(
                    id,
                    (
                        class_name.clone(),
                        self.class_result_overlay(id, self.class_by_id(id)),
                    ),
                );
            }
            for key in body_scope.class_keys().into_iter().flatten() {
                let Some((class_name, id)) = self.class_ids.get_with_name(key) else {
                    continue;
                };
                class_results_before.insert(
                    *id,
                    (
                        class_name.clone(),
                        self.class_result_overlay(*id, self.class_by_id(*id)),
                    ),
                );
            }
        }
        if let Some(changed) = changed {
            class_results_before.extend(changed.class_results_before.clone());
        }
        snapshot_class_ids.extend(class_results_before.keys().copied());
        let snapshot = |registry: &Registry| ResolvedSnapshot {
            class_ids: snapshot_class_ids.clone(),
            functions: functions_before
                .iter()
                .map(|(id, (name, declaration, result))| {
                    (
                        *id,
                        (
                            name.clone(),
                            declaration.as_ref().map(|declaration| {
                                Detached::Own(Arc::clone(match declaration {
                                    Detached::Own(value) => value,
                                }))
                            }),
                            result.clone(),
                        ),
                    )
                })
                .collect(),
            effects: registry
                .effects
                .iter()
                .map(|(id, settled)| (id.clone(), (registry.effect_decl_key(id), settled.clone())))
                .collect(),
            global_variables: registry.global_variables().clone(),
            global_scopes: Arc::clone(&registry.global_scopes),
            include_targets: Arc::clone(&registry.include_targets),
            class_results: class_results_before.clone(),
        };
        if scope.is_all() {
            self.stub_effect_results.clear();
            self.stub_holding_classes.clear();
            self.class_const_types.clear();
            self.class_const_types_by_id.clear();
            self.function_results.clear();
            self.class_constant_results.clear();
            self.class_constant_names.clear();
            self.class_method_results.clear();
            self.class_member_topology.clear();
            self.class_property_results.clear();
            self.imported_type_aliases.clear();
            self.class_type_results.clear();
            self.class_reference_results.clear();
            return body_scope.map(|_| snapshot(self));
        }
        let changed_class_ids: FxHashSet<ClassId> = self
            .class_ids
            .iter()
            .filter(|(name, id)| scope.has_class_id(**id) || scope.has_class(name))
            .map(|(_, id)| *id)
            .collect();
        self.class_const_types
            .retain(|(id, _), _| !changed_class_ids.contains(id));
        self.class_const_types_by_id
            .retain(|id, _| !changed_class_ids.contains(id));
        self.class_constant_results
            .retain(|(id, _), _| !changed_class_ids.contains(id));
        self.class_constant_names
            .retain(|id, _| !changed_class_ids.contains(id));
        self.class_method_results
            .retain_classes(|id| !changed_class_ids.contains(&id));
        self.class_member_topology
            .retain(|id, _| !changed_class_ids.contains(id));
        self.class_property_results
            .retain_classes(|id| !changed_class_ids.contains(&id));
        self.imported_type_aliases
            .retain(|id, _| !changed_class_ids.contains(id));
        self.class_type_results
            .retain(|id, _| !changed_class_ids.contains(id));
        self.class_reference_results
            .retain(|id, _| !changed_class_ids.contains(id));
        let changed_function_ids: FxHashSet<FnId> = self
            .function_ids
            .iter()
            .filter(|(name, id)| scope.has_function_id(**id) || scope.has_function(name))
            .map(|(_, id)| *id)
            .collect();
        self.function_results
            .retain(|id, _| !changed_function_ids.contains(id));
        self.stub_effect_results.clear();
        self.stub_holding_classes.clear();
        body_scope.map(|_| snapshot(self))
    }

    /// Takes from `base`, the stub base the registry now stands on, what
    /// it has of its stubs alone: which classes and functions are built-in
    /// stubs, and the names its declarations assert with or run code
    /// under. Declarations rebuilt over another base leave these as the
    /// old base had them.
    pub(crate) fn adopt_stub_tables(&mut self, base: &Registry) {
        self.built_in_stub_classes = base.built_in_stub_classes.clone();
        self.stub_provided_functions = base.stub_provided_functions.clone();
        self.base_assert_tagged = Arc::clone(&base.base_assert_tagged);
        self.base_builtin_methods_running_code =
            Arc::clone(&base.base_builtin_methods_running_code);
    }

    /// Makes the class, function and derived tables a shared base, for
    /// clones of the registry to share rather than copy.
    pub(crate) fn freeze(&mut self) {
        // A frozen base and its on-disk form use a compact name-keyed table.
        // Build that projection from the authoritative selected ID stores so
        // freezing a Registry also preserves project-owned declarations.
        self.classes = self.classes_for_cache();
        self.functions = self.functions_for_cache();
        // IDs may be reassigned from project to stub space below. The next
        // effect pass rebuilds this ephemeral memo against the frozen IDs.
        self.stub_effect_results.clear();
        self.stub_holding_classes.clear();
        self.base_assert_tagged = Arc::new(self.names_with_assert_tags.clone());
        self.base_builtin_methods_running_code =
            Arc::new(self.builtin_methods_running_code.clone());
        let phpstan_ancestry_names: FxHashSet<Name> = self
            .class_ids
            .iter()
            .filter(|(_, id)| self.phpstan_ancestry.contains(id))
            .map(|(name, _)| name.clone())
            .collect();
        let ancestor_names: FxHashMap<Name, syntax::name::NameSet> = self
            .class_ids
            .iter()
            .filter_map(|(name, id)| {
                self.ancestors_by_id
                    .get(id)
                    .map(|ancestors| (name.clone(), ancestors.names(self)))
            })
            .collect();
        let member_owners: FxHashMap<Name, hierarchy::ClassOwners> = self
            .class_ids
            .iter()
            .filter_map(|(name, id)| {
                self.member_owners
                    .get(id)
                    .map(|owners| (name.clone(), owners.clone()))
            })
            .collect();
        let class_const_types = self.class_const_types_for_cache();
        let function_results = self.function_results_for_cache();
        let class_constant_results = self.class_constant_results_for_cache();
        let class_method_results = self.class_method_results_for_cache();
        let class_member_topology = self.class_member_topology_for_cache();
        let class_property_results = self.class_property_results_for_cache();
        let class_property_topology = self.class_property_topology_for_cache();
        let class_type_results = self.class_type_results_for_cache();
        let class_reference_results = self.class_reference_results_for_cache();
        let imported_type_aliases = self.imported_type_aliases_for_cache();
        let effects = self.effects_cache.take().map_or_else(
            || self.effects_for_names(),
            |entries| entries.into_iter().collect(),
        );
        self.classes.freeze();
        self.functions.freeze();
        self.class_ids = NameTable::default();
        self.project_class_names.clear();
        for index in 0..self.classes.base_len() {
            let local = u32::try_from(index).expect("stub class id range exhausted");
            let name = self
                .classes
                .base_name(index)
                .expect("base index is in range");
            self.class_ids.insert(
                name.clone(),
                ClassId::stub(local).with_generation(self.id_generation),
            );
        }
        self.class_ids.freeze();
        self.class_declarations = self
            .class_ids
            .iter()
            .filter_map(|(name, id)| {
                self.classes
                    .get_shared(name)
                    .map(|declaration| (*id, declaration))
            })
            .collect();
        self.function_ids = NameTable::default();
        self.project_function_names.clear();
        for index in 0..self.functions.base_len() {
            let local = u32::try_from(index).expect("stub function id range exhausted");
            let name = self
                .functions
                .base_name(index)
                .expect("base index is in range");
            self.function_ids.insert(
                name.clone(),
                FnId::stub(local).with_generation(self.id_generation),
            );
        }
        self.function_ids.freeze();
        self.function_declarations = self
            .function_ids
            .iter()
            .filter_map(|(name, id)| {
                self.functions
                    .get_shared(name)
                    .map(|declaration| (*id, declaration))
            })
            .collect();
        self.function_results = function_results
            .iter()
            .filter_map(|(name, result)| {
                self.function_ids
                    .get(name)
                    .copied()
                    .map(|id| (id, result.clone()))
            })
            .collect();
        self.function_results_cache = FunctionTable::default();
        if ancestor_names.is_empty() {
            // The cached form is persisted by name for stable cache reads;
            // declaration IDs are assigned only after that table is read.
            self.ancestors_by_id = self
                .ancestor_names_cache
                .iter()
                .filter_map(|(name, ancestors)| {
                    self.class_ids
                        .get(name)
                        .copied()
                        .map(|id| (id, hierarchy::AncestorIds::from_names(self, ancestors)))
                })
                .collect();
            self.ancestor_names_cache = NameTable::default();
        } else {
            self.ancestors_by_id = ancestor_names
                .into_iter()
                .filter_map(|(name, ancestors)| {
                    self.class_ids
                        .get(&name)
                        .copied()
                        .map(|id| (id, hierarchy::AncestorIds::from_names(self, &ancestors)))
                })
                .collect();
        }
        if self.ancestors_by_id.is_empty() && !self.classes.is_empty() {
            self.precompute_ancestor_names();
        }
        self.member_owners = if member_owners.is_empty() {
            self.member_owners_cache
                .iter()
                .filter_map(|(name, owners)| {
                    self.class_ids
                        .get(name)
                        .copied()
                        .map(|id| (id, owners.clone()))
                })
                .collect()
        } else {
            member_owners
                .into_iter()
                .filter_map(|(name, owners)| {
                    self.class_ids.get(&name).copied().map(|id| (id, owners))
                })
                .collect()
        };
        self.member_owners_cache = NameTable::default();
        self.class_const_types = class_const_types
            .iter()
            .flat_map(|(name, results)| {
                let Some(id) = self.class_ids.get(name).copied() else {
                    return Vec::new();
                };
                results
                    .iter()
                    .map(|(slot, ty)| {
                        (
                            (
                                id,
                                usize::try_from(*slot).expect("usize is at least 32 bits"),
                            ),
                            ty.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        self.class_const_types_by_id.clear();
        for ((id, slot), ty) in &self.class_const_types {
            self.class_const_types_by_id
                .entry(*id)
                .or_default()
                .push((*slot, ty.clone()));
        }
        self.class_const_types_cache = NameTable::default();
        self.class_constant_results.clear();
        self.class_constant_names.clear();
        for (name, constants) in &class_constant_results {
            let Some(id) = self.class_ids.get(name).copied() else {
                continue;
            };
            for (constant_name, slot, constant) in constants {
                self.set_class_constant_result(
                    id,
                    usize::try_from(*slot).expect("usize is at least 32 bits"),
                    constant_name.clone(),
                    Arc::clone(constant),
                );
            }
        }
        self.class_constant_results_cache = NameTable::default();
        self.class_member_topology = class_member_topology
            .iter()
            .filter_map(|(name, entries)| {
                let id = self.class_ids.get(name).copied()?;
                let declaration_len = self.class_by_id(id)?.member_order.len();
                let mut topology = hierarchy::MemberTopology::default();
                for entry in entries {
                    topology.add(declaration_len, entry.clone())?;
                }
                Some((id, topology))
            })
            .collect();
        self.class_member_topology_cache = NameTable::default();
        self.class_method_results = class_method_results
            .iter()
            .flat_map(|(name, methods)| {
                let Some(class) = self.class_ids.get(name).copied() else {
                    return Vec::new();
                };
                methods
                    .iter()
                    .filter_map(|(method_name, _)| {
                        let slot = self.member_slot(name, method_name)?;
                        Some((
                            MethodRef::new(class, slot),
                            methods.shared_get(method_name)?,
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        self.class_method_results_cache = NameTable::default();
        self.class_property_topology = class_property_topology
            .iter()
            .filter_map(|(name, properties)| {
                let id = self.class_ids.get(name).copied()?;
                let mut topology = PropertyTopology::default();
                for property in properties {
                    topology.append(property.clone())?;
                }
                Some((id, topology))
            })
            .collect();
        self.class_property_topology_cache = NameTable::default();
        let active_classes: Vec<ClassId> = self.class_ids.iter().map(|(_, id)| *id).collect();
        for id in active_classes {
            self.ensure_property_topology(id);
        }
        self.class_property_results.clear();
        let restored_properties: Vec<_> = class_property_results
            .iter()
            .flat_map(|(name, properties)| {
                let Some(id) = self.class_ids.get(name).copied() else {
                    return Vec::new();
                };
                properties
                    .iter()
                    .map(|(property_name, property)| {
                        (id, property_name.clone(), Arc::clone(property))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        for (id, name, property) in restored_properties {
            self.set_property_result(id, name, property);
        }
        self.class_property_results_cache = NameTable::default();
        self.class_type_results = class_type_results
            .iter()
            .filter_map(|(name, results)| {
                self.class_ids
                    .get(name)
                    .copied()
                    .map(|id| (id, results.clone()))
            })
            .collect();
        self.class_type_results_cache = NameTable::default();
        self.class_reference_results = class_reference_results
            .iter()
            .filter_map(|(name, refs)| {
                self.class_ids
                    .get(name)
                    .copied()
                    .map(|id| (id, refs.clone()))
            })
            .collect();
        self.class_reference_results_cache = NameTable::default();
        self.imported_type_aliases = imported_type_aliases
            .iter()
            .filter_map(|(name, aliases)| {
                self.class_ids.get(name).copied().map(|id| {
                    (
                        id,
                        aliases
                            .iter()
                            .map(|(name, ty)| (name.clone(), ty.clone()))
                            .collect(),
                    )
                })
            })
            .collect();
        self.imported_type_aliases_cache = NameTable::default();
        // Method and property-hook IDs are reconstructed from the effective
        // class-local slots, so restore their result overlays before
        // translating the name-keyed cache representation back to IDs.
        self.rebuild_effect_ids(effects);
        self.rebuild_methods_by_name();
        if phpstan_ancestry_names.is_empty() {
            // Cached registries omit runtime ids and this derived set; walk
            // the hierarchy only for that path.
            self.rebuild_phpstan_ancestry_ids();
        } else {
            self.phpstan_ancestry = phpstan_ancestry_names
                .iter()
                .filter_map(|name| self.class_ids.get(name).copied())
                .collect();
        }
        self.built_in_stub_classes.freeze();
        self.subtype_children.freeze();
        self.trait_users.freeze();
        self.recorded_parents.freeze();
    }

    pub(crate) fn record_changed_class(&mut self, name: &str) {
        if self.resolved_declarations {
            let key = name.to_ascii_lowercase();
            if self.changed.classes.insert(key.clone()) {
                let found = self.class_ids.get_with_name(name);
                let id = found.map(|(_, id)| *id);
                let declaration_snapshot =
                    id.and_then(|id| self.class_declarations.get(&id).cloned());
                self.changed.class_ids_before.insert(key.clone(), id);
                if let Some((declared_name, _)) = found {
                    self.changed
                        .class_names_before
                        .insert(key.clone(), declared_name.clone());
                }
                if let (Some(id), Some(declaration)) = (id, declaration_snapshot.as_ref()) {
                    self.changed
                        .class_declarations_before
                        .insert(id, Arc::clone(declaration));
                }
                if let Some(id) = id {
                    let overlay = self.class_result_overlay(id, declaration_snapshot.as_deref());
                    self.changed
                        .class_results_before
                        .insert(id, (Name::from(name), overlay));
                }
            }
        }
    }

    fn class_result_overlay(
        &self,
        id: ClassId,
        declaration: Option<&ClassReflection>,
    ) -> ClassResultOverlay {
        let mut methods: Vec<_> = self
            .method_results_for_class(id)
            .filter_map(|(method, signature)| {
                self.member_entry_by_ref(method)
                    .map(|entry| (entry.name.clone(), signature.clone()))
            })
            .collect();
        methods.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
        let mut const_types = self
            .class_const_types_by_id
            .get(&id)
            .cloned()
            .unwrap_or_default();
        const_types.sort_unstable_by_key(|(slot, _)| *slot);
        let mut properties: Vec<_> = self
            .property_results_for_class(id)
            .filter_map(|(property_ref, property)| {
                self.property_name_by_ref(property_ref)
                    .map(|name| (name.clone(), Arc::clone(property)))
            })
            .collect();
        properties.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
        let properties = (!properties.is_empty()).then_some(properties);
        let imported_aliases = self.imported_type_aliases.get(&id).map(|aliases| {
            let mut aliases: Vec<_> = aliases
                .iter()
                .map(|(name, ty)| (name.clone(), ty.clone()))
                .collect();
            aliases.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
            aliases
        });
        ClassResultOverlay {
            methods,
            topology: self.class_member_topology.get(&id).cloned(),
            properties,
            constants: {
                let constants: OrderedNameMap<_> = self
                    .class_constant_results_for_class(id)
                    .map(|(slot, name, constant)| (name.clone(), (slot, Arc::clone(constant))))
                    .collect();
                (!constants.is_empty()).then_some(constants)
            },
            const_types,
            types: self.class_type_results.get(&id).cloned(),
            imported_aliases,
            references: self
                .class_reference_results
                .get(&id)
                .cloned()
                .or_else(|| {
                    declaration.map(|info| ClassReferenceResults {
                        extends: info.extends.clone(),
                        implements: info.implements.clone(),
                        uses: info.uses.clone(),
                        mixins: info.mixins.clone(),
                        trait_aliases: info.trait_aliases.clone(),
                        type_imports: info.type_imports.clone(),
                    })
                })
                .unwrap_or_default(),
        }
    }

    pub(crate) fn record_changed_function(&mut self, name: &str) {
        if self.resolved_declarations {
            let key = name.to_ascii_lowercase();
            if !self.changed.functions.insert(key.clone()) {
                return;
            }
            if let Some((declared_name, id)) = self.function_ids.get_with_name(name) {
                let id = *id;
                self.changed.function_ids_before.insert(key, id);
                self.changed
                    .function_names_before
                    .insert(name.to_ascii_lowercase(), declared_name.clone());
                if let Some(declaration) = self.function_declarations.get(&id).cloned() {
                    self.changed
                        .function_declarations_before
                        .insert(id, declaration);
                }
                if let Some(result) = self.function_results.get(&id) {
                    self.changed
                        .function_results_before
                        .insert(id, result.clone());
                }
            }
        }
    }

    fn record_changed_constant(&mut self, name: &str) {
        if self.resolved_declarations && !self.changed.constants.contains_key(name) {
            let before = self.global_constants.get(name).cloned();
            self.changed.constants.insert(name.to_string(), before);
        }
    }

    /// The keys of the classes, functions and constants whose resolved
    /// form differs from `before` (and, for constants, from `changed`'s
    /// earlier types).
    pub(crate) fn changed_since(
        &self,
        before: &ResolvedSnapshot,
        changed: &ChangedNames,
    ) -> FxHashSet<crate::deps::RefKey> {
        use crate::deps::RefKey;
        let mut keys = FxHashSet::default();
        for name in &changed.classes {
            let old_id = changed.class_ids_before.get(name).copied().flatten();
            let old = old_id.and_then(|id| changed.class_declarations_before.get(&id));
            let now_id = self.class_ids.get(name.as_str()).copied();
            let now = now_id.and_then(|id| self.class_by_id(id));
            let declaration_changed = match (old, now) {
                (Some(old), Some(now)) => {
                    !std::ptr::eq(Arc::as_ptr(old), now) && old.as_ref() != now
                }
                (None, None) => false,
                _ => true,
            };
            if old_id != now_id || declaration_changed {
                keys.insert(RefKey::class_text(name));
            }
        }
        for name in self.changed_class_results(before).into_values() {
            keys.insert(RefKey::class_text(name.as_str()));
        }
        for name in &changed.functions {
            let old_id = changed.function_ids_before.get(name).copied();
            let old = old_id.and_then(|id| changed.function_declarations_before.get(&id));
            let now_id = self.function_ids.get(name.as_str()).copied();
            let now = now_id.and_then(|id| self.function_declaration_by_id(id));
            let declaration_changed = match (old, now) {
                (Some(old), Some(now)) => {
                    !std::ptr::eq(Arc::as_ptr(old), now) && old.as_ref() != now
                }
                (None, None) => false,
                _ => true,
            };
            if old_id != now_id || declaration_changed {
                keys.insert(RefKey::function_text(name));
            }
        }
        let changed_functions: Vec<Name> = before
            .functions
            .par_iter()
            .filter_map(|(id, (name, old, old_result))| {
                let declaration = self.function_declaration_by_id(*id);
                let result = self.function_results.get(id);
                (differs(declaration, old.as_ref()) || old_result.as_ref() != result)
                    .then(|| name.clone())
            })
            .collect();
        keys.extend(
            changed_functions
                .iter()
                .map(|name| RefKey::function_text(name.as_str())),
        );
        for (name, old) in &changed.constants {
            if self.global_constants.get(name.as_str()) != old.as_ref() {
                keys.insert(RefKey::constant_text(name));
            }
        }
        let mut effect_ids: FxHashSet<_> = before.effects.keys().cloned().collect();
        effect_ids.extend(self.effects.keys().cloned());
        let effect_names: Vec<_> = effect_ids
            .into_par_iter()
            .filter_map(|id| {
                let old = before.effects.get(&id);
                let now = self.effects.get(&id);
                (old.map(|(_, settled)| settled) != now)
                    .then(|| {
                        now.and_then(|_| self.effect_decl_key(&id))
                            .or_else(|| old.and_then(|(key, _)| key.clone()))
                    })
                    .flatten()
            })
            .collect();
        for key in effect_names {
            keys.insert(match &key.class {
                Some(class) => RefKey::class_text(class),
                None => RefKey::function_text(&key.member),
            });
        }
        keys
    }

    /// The class and function dependency keys named by the declarations
    /// whose ids changed. Existing names come from their prior ID entries;
    /// added names come from the active ID table. Constants remain name
    /// keyed because they do not have declaration ids.
    pub(crate) fn changed_frontier(&self, changed: &ChangedNames) -> Vec<crate::deps::RefKey> {
        let mut frontier = Vec::new();
        for name in &changed.classes {
            let old = changed
                .class_names_before
                .get(name)
                .map(crate::deps::RefKey::class);
            let current = self
                .class_ids
                .get(name.as_str())
                .copied()
                .and_then(|id| self.class_name_by_id(id))
                .map(crate::deps::RefKey::class);
            if let Some(key) = old.or(current) {
                frontier.push(key);
            }
        }
        for name in &changed.functions {
            let old = changed
                .function_names_before
                .get(name)
                .map(crate::deps::RefKey::function);
            let current = self
                .function_ids
                .get(name.as_str())
                .copied()
                .and_then(|id| self.function_name_by_id(id))
                .map(crate::deps::RefKey::function);
            if let Some(key) = old.or(current) {
                frontier.push(key);
            }
        }
        frontier.extend(
            changed
                .constants
                .keys()
                .map(|name| crate::deps::RefKey::constant_text(name)),
        );
        frontier
    }

    fn changed_class_results(&self, before: &ResolvedSnapshot) -> FxHashMap<ClassId, Name> {
        let active_class_names: FxHashMap<ClassId, Name> = self
            .class_ids
            .iter()
            .map(|(name, id)| (*id, name.clone()))
            .collect();
        before
            .class_ids
            .par_iter()
            .filter_map(|id| {
                let id = *id;
                let old = before.class_results.get(&id);
                let name = active_class_names
                    .get(&id)
                    .cloned()
                    .or_else(|| old.map(|(name, _)| name.clone()))?;
                let current = self.class_by_id(id).map(|declaration| {
                    (
                        name.clone(),
                        self.class_result_overlay(id, Some(declaration)),
                    )
                });
                (old != current.as_ref()).then_some((id, name))
            })
            .collect()
    }

    /// The traits the classes whose resolved form differs from `before`'s
    /// use, before the change and after it: a trait is checked as each class
    /// using it, so its file is checked again when one of them changes.
    pub(crate) fn traits_of_changed_classes(
        &self,
        before: &ResolvedSnapshot,
        changed: Option<&ChangedNames>,
    ) -> Vec<Name> {
        let mut traits: Vec<Name> = Vec::new();
        if let Some(changed) = changed {
            for name in &changed.classes {
                let old_id = changed.class_ids_before.get(name).copied().flatten();
                let now_id = self.class_ids.get(name.as_str()).copied();
                for used in old_id
                    .and_then(|id| changed.class_declarations_before.get(&id))
                    .into_iter()
                    .flat_map(|declaration| declaration.uses.iter())
                    .chain(
                        now_id
                            .and_then(|id| self.class_by_id(id))
                            .into_iter()
                            .flat_map(|declaration| declaration.uses.iter()),
                    )
                {
                    if !traits.contains(used) {
                        traits.push(used.clone());
                    }
                }
            }
        }
        let result_changes = self.changed_class_results(before);
        for (id, _) in result_changes {
            let old = before
                .class_results
                .get(&id)
                .map(|(_, result)| &result.references);
            let now = self.class_references(id);
            for used in old
                .into_iter()
                .flat_map(|references| references.uses.iter())
                .chain(now.iter().flat_map(|references| references.uses.iter()))
            {
                if !traits.contains(used) {
                    traits.push(used.clone());
                }
            }
        }
        traits
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
            .cloned()
            .chain(self.changed_global_scope_names(before))
            .map(|name| crate::deps::RefKey::global_variable(&name))
            .collect()
    }

    /// Writes the registry into `writer`: its tables as sections of their
    /// own, and the rest of it, encoded, as one.
    pub(crate) fn store(&self, writer: &mut container::Writer) -> Result<(), String> {
        let mut ready = self.clone();
        ready.materialize_deferred_types_for_cache();
        ready.store_materialized(writer)
    }

    fn store_materialized(&self, writer: &mut container::Writer) -> Result<(), String> {
        let mut effects: Vec<_> = self.effects_for_names().into_iter().collect();
        effects.sort_unstable_by(|(left, _), (right, _)| {
            left.class
                .cmp(&right.class)
                .then_with(|| left.member.cmp(&right.member))
        });
        let encoded_effects = writer.encode(&effects)?;
        writer.section(EFFECTS, &encoded_effects)?;
        self.ancestor_names_for_cache()
            .store(writer, "table:ancestor_names")?;
        self.member_owners_for_cache()
            .store(writer, "table:member_owners")?;
        self.class_const_types_for_cache()
            .store(writer, "table:class_const_types")?;
        self.function_results_for_cache()
            .store(writer, "table:function_results")?;
        self.class_constant_results_for_cache()
            .store(writer, "table:class_constant_results")?;
        self.class_method_results_for_cache()
            .store(writer, "table:class_method_results")?;
        self.class_member_topology_for_cache()
            .store(writer, "table:class_member_topology")?;
        self.class_property_results_for_cache()
            .store(writer, "table:class_property_results")?;
        self.class_property_topology_for_cache()
            .store(writer, "table:class_property_topology")?;
        self.class_type_results_for_cache()
            .store(writer, "table:class_type_results")?;
        self.class_reference_results_for_cache()
            .store(writer, CLASS_REFERENCES)?;
        self.imported_type_aliases_for_cache()
            .store(writer, "table:imported_type_aliases")?;
        // Serialize the selected ID stores directly. Building temporary
        // name-keyed NameTables here duplicated every declaration solely to
        // satisfy the old cache shape.
        let classes: Vec<_> = if self.class_ids.is_empty() {
            self.classes.iter().collect()
        } else {
            self.class_ids
                .iter()
                .filter_map(|(name, id)| {
                    self.class_declarations
                        .get(id)
                        .map(|declaration| (name, declaration.as_ref()))
                })
                .collect()
        };
        name_table::store_entries(writer, "table:classes", classes, true)?;
        let functions: Vec<_> = if self.function_ids.is_empty() {
            self.functions.iter().collect()
        } else {
            self.function_ids
                .iter()
                .filter_map(|(name, id)| {
                    self.function_declarations
                        .get(id)
                        .map(|declaration| (name, declaration.as_ref()))
                })
                .collect()
        };
        name_table::store_entries(writer, "table:functions", functions, true)?;
        self.store_tables(writer)?;
        let rest = writer.encode(self)?;
        writer.section(REST, &rest)
    }

    /// The on-disk cache keeps names stable; runtime hierarchy lookups use
    /// declaration IDs and reconstruct this view only while writing.
    fn ancestor_names_for_cache(&self) -> NameTable<syntax::name::NameSet> {
        if !self.ancestor_names_cache.is_empty() {
            return self.ancestor_names_cache.clone();
        }
        self.class_ids
            .iter()
            .filter_map(|(name, id)| {
                self.ancestors_by_id
                    .get(id)
                    .map(|ancestors| (name.clone(), ancestors.names(self)))
            })
            .collect()
    }

    /// A name-keyed view used when a frozen base or canonical comparison
    /// needs to expose the selected declarations as a table. Cache writes
    /// serialize the ID stores directly.
    fn classes_for_cache(&self) -> NameTable<ClassReflection> {
        if self.class_ids.is_empty() {
            return self.classes.clone();
        }
        let mut classes = NameTable::default();
        for (name, id) in self.class_ids.iter() {
            if let Some(declaration) = self.class_declarations.get(id) {
                classes.insert_shared(name.clone(), declaration.clone());
            }
        }
        classes
    }

    /// As-built name-keyed function view for a frozen base or canonical
    /// comparison. Cache writes serialize the ID store directly.
    fn functions_for_cache(&self) -> FunctionTable {
        if self.function_ids.is_empty() {
            return self.functions.clone();
        }
        let mut functions = FunctionTable::default();
        for (name, id) in self.function_ids.iter() {
            if let Some(declaration) = self.function_declarations.get(id) {
                functions.insert_shared(name.clone(), declaration.clone());
            }
        }
        functions
    }

    fn member_owners_for_cache(&self) -> NameTable<hierarchy::ClassOwners> {
        if !self.member_owners_cache.is_empty() {
            return self.member_owners_cache.clone();
        }
        self.class_ids
            .iter()
            .filter_map(|(name, id)| {
                self.member_owners
                    .get(id)
                    .map(|owners| (name.clone(), owners.clone()))
            })
            .collect()
    }

    /// The disk cache uses class names and declaration slots; runtime results
    /// use generation-checked class IDs and slots.
    pub(crate) fn class_const_types_for_cache(&self) -> NameTable<Vec<(u32, Type)>> {
        if !self.class_const_types_cache.is_empty() {
            return self.class_const_types_cache.clone();
        }
        let mut by_name: FxHashMap<Name, Vec<(u32, Type)>> = FxHashMap::default();
        for ((id, slot), ty) in &self.class_const_types {
            let name = self
                .class_name_by_id(*id)
                .expect("class constant result names an active class");
            let slot = u32::try_from(*slot).expect("class constant slot range exhausted");
            by_name
                .entry(name.clone())
                .or_default()
                .push((slot, ty.clone()));
        }
        for results in by_name.values_mut() {
            results.sort_unstable_by_key(|(slot, _)| *slot);
        }
        by_name.into_iter().collect()
    }

    /// The disk cache uses function names; runtime results use generation-
    /// checked function IDs.
    fn function_results_for_cache(&self) -> FunctionTable {
        if !self.function_results_cache.is_empty() {
            return self.function_results_cache.clone();
        }
        self.function_results
            .iter()
            .filter_map(|(id, result)| {
                self.function_name_by_id(*id)
                    .map(|name| (name.clone(), result.clone()))
            })
            .collect()
    }

    fn class_constant_results_for_cache(
        &self,
    ) -> NameTable<Vec<(Name, u32, Arc<ConstantReflection>)>> {
        if !self.class_constant_results_cache.is_empty() {
            return self.class_constant_results_cache.clone();
        }
        let mut results: NameTable<Vec<(Name, u32, Arc<ConstantReflection>)>> =
            NameTable::default();
        for ((id, slot), constant) in &self.class_constant_results {
            let (Some(class_name), Some(constant_name)) = (
                self.class_name_by_id(*id),
                self.class_constant_name_by_slot(*id, *slot),
            ) else {
                continue;
            };
            results
                .get_or_insert_with(class_name.clone(), Vec::new)
                .push((
                    constant_name.clone(),
                    u32::try_from(*slot).expect("class constant slot range exhausted"),
                    Arc::clone(constant),
                ));
        }
        for constants in results.values_mut() {
            constants.sort_unstable_by_key(|(_, slot, _)| *slot);
        }
        results
    }

    fn class_method_results_for_cache(&self) -> NameTable<Members<FunctionReflection>> {
        if !self.class_method_results_cache.is_empty() {
            return self.class_method_results_cache.clone();
        }
        let mut results: NameTable<Members<FunctionReflection>> = NameTable::default();
        for (method, signature) in self.class_method_results.iter() {
            let Some(class_name) = self.class_name_by_id(method.class()) else {
                continue;
            };
            let Some(entry) = self.member_entry_by_ref(method) else {
                continue;
            };
            results
                .get_or_insert_with(class_name.clone(), Members::default)
                .insert_shared_if_absent(entry.name.clone(), Arc::clone(signature));
        }
        results
    }

    fn class_member_topology_for_cache(&self) -> NameTable<Vec<MethodEntry>> {
        if !self.class_member_topology_cache.is_empty() {
            return self.class_member_topology_cache.clone();
        }
        self.class_member_topology
            .iter()
            .filter_map(|(id, topology)| {
                self.class_name_by_id(*id)
                    .map(|class| (class.clone(), topology.entries.clone()))
            })
            .collect()
    }

    fn class_property_results_for_cache(&self) -> NameTable<Vec<(Name, Arc<PropertyReflection>)>> {
        if !self.class_property_results_cache.is_empty() {
            return self.class_property_results_cache.clone();
        }
        let mut results: NameTable<Vec<(Name, Arc<PropertyReflection>)>> = NameTable::default();
        for (property_ref, property) in self.class_property_results.iter() {
            let Some(class_name) = self.class_name_by_id(property_ref.class()) else {
                continue;
            };
            let Some(property_name) = self.property_name_by_ref(property_ref) else {
                continue;
            };
            results
                .get_or_insert_with(class_name.clone(), Vec::new)
                .push((property_name.clone(), Arc::clone(property)));
        }
        for (_, properties) in results.iter_mut() {
            properties.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
        }
        results
    }

    fn class_property_topology_for_cache(&self) -> NameTable<Vec<Name>> {
        if !self.class_property_topology_cache.is_empty() {
            return self.class_property_topology_cache.clone();
        }
        self.class_property_topology
            .iter()
            .filter_map(|(id, topology)| {
                self.class_name_by_id(*id)
                    .map(|name| (name.clone(), topology.entries().to_vec()))
            })
            .collect()
    }

    fn imported_type_aliases_for_cache(&self) -> NameTable<Vec<(Name, Type)>> {
        if !self.imported_type_aliases_cache.is_empty() {
            return self.imported_type_aliases_cache.clone();
        }
        self.imported_type_aliases
            .iter()
            .filter_map(|(id, aliases)| {
                let mut aliases: Vec<_> = aliases
                    .iter()
                    .map(|(name, ty)| (name.clone(), ty.clone()))
                    .collect();
                aliases.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
                self.class_name_by_id(*id)
                    .map(|name| (name.clone(), aliases))
            })
            .collect()
    }

    fn class_type_results_for_cache(&self) -> NameTable<ClassTypeResults> {
        if !self.class_type_results_cache.is_empty() {
            return self.class_type_results_cache.clone();
        }
        self.class_type_results
            .iter()
            .filter_map(|(id, results)| {
                let mut results = results.clone();
                results
                    .template_bounds
                    .sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
                results
                    .implements_args
                    .sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
                self.class_name_by_id(*id)
                    .map(|name| (name.clone(), results))
            })
            .collect()
    }

    fn class_reference_results_for_cache(&self) -> NameTable<ClassReferenceResults> {
        if !self.class_reference_results_cache.is_empty() {
            return self.class_reference_results_cache.clone();
        }
        self.class_reference_results
            .iter()
            .filter_map(|(id, refs)| {
                self.class_name_by_id(*id)
                    .map(|name| (name.clone(), refs.clone()))
            })
            .collect()
    }

    /// Whether every stored table, map and set of a registry read back is
    /// in the order a lookup by bisection relies on. A load checks only
    /// that its records are inside the file, so the tests of what is stored
    /// call this on what they read back.
    #[cfg(test)]
    pub(crate) fn check_stored(&self) -> Result<(), String> {
        self.check_tables()?;
        self.classes_for_cache().check_order("table:classes")?;
        self.functions_for_cache().check_order("table:functions")?;
        self.ancestor_names_for_cache()
            .check_order("table:ancestor_names")?;
        self.member_owners_for_cache()
            .check_order("table:member_owners")?;
        self.class_const_types_for_cache()
            .check_order("table:class_const_types")?;
        self.function_results_for_cache()
            .check_order("table:function_results")?;
        self.class_constant_results_for_cache()
            .check_order("table:class_constant_results")?;
        self.class_method_results_for_cache()
            .check_order("table:class_method_results")?;
        self.class_member_topology_for_cache()
            .check_order("table:class_member_topology")?;
        self.class_property_results_for_cache()
            .check_order("table:class_property_results")?;
        self.class_property_topology_for_cache()
            .check_order("table:class_property_topology")?;
        self.class_type_results_for_cache()
            .check_order("table:class_type_results")?;
        self.class_reference_results_for_cache()
            .check_order(CLASS_REFERENCES)?;
        self.imported_type_aliases_for_cache()
            .check_order("table:imported_type_aliases")?;
        Ok(())
    }

    /// The registry stored under `name` for this build, if there is a
    /// readable file of it.
    pub(crate) fn load_cached(name: &str) -> Option<Result<Registry, String>> {
        let reader = load_container(name)?;
        Some(Registry::load(&reader))
    }

    /// Stores the registry under `name` for this build.
    pub(crate) fn store_cached(&self, name: &str) -> Result<(), String> {
        let mut writer = container::Writer::new();
        self.store(&mut writer)?;
        store_container(name, writer);
        Ok(())
    }

    /// The registry `store` wrote, its tables read in place.
    pub(crate) fn load(reader: &container::Reader) -> Result<Registry, String> {
        let rest = reader.section(REST)?;
        let names = reader.names().clone();
        let mut registry: Registry = syntax::name::with_cache_name_table(&names, || {
            stub_cache::decode(&reader.buffer()[rest])
        })?;
        registry.load_tables(reader)?;
        registry.classes = NameTable::load(reader, "table:classes")?;
        registry.functions = FunctionTable::load(reader, "table:functions")?;
        let effects = reader.section(EFFECTS)?;
        registry.effects_cache = Some(syntax::name::with_cache_name_table(&names, || {
            stub_cache::decode(&reader.buffer()[effects])
        })?);
        registry.ancestor_names_cache = NameTable::load(reader, "table:ancestor_names")?;
        registry.member_owners_cache = NameTable::load(reader, "table:member_owners")?;
        registry.class_const_types_cache = NameTable::load(reader, "table:class_const_types")?;
        registry.function_results_cache = FunctionTable::load(reader, "table:function_results")?;
        registry.class_constant_results_cache =
            NameTable::load(reader, "table:class_constant_results")?;
        registry.class_method_results_cache =
            NameTable::load(reader, "table:class_method_results")?;
        registry.class_member_topology_cache =
            NameTable::load(reader, "table:class_member_topology")?;
        registry.class_property_results_cache =
            NameTable::load(reader, "table:class_property_results")?;
        registry.class_property_topology_cache =
            NameTable::load(reader, "table:class_property_topology")?;
        registry.class_type_results_cache = NameTable::load(reader, "table:class_type_results")?;
        registry.class_reference_results_cache = NameTable::load(reader, CLASS_REFERENCES)?;
        registry.imported_type_aliases_cache =
            NameTable::load(reader, "table:imported_type_aliases")?;
        registry.advance_relation_generation();
        Ok(registry)
    }

    stored_tables!(
        built_in_stub_classes,
        subtype_children,
        trait_users,
        recorded_parents,
        global_constants,
        unconditionally_declared_classes,
        unconditionally_declared_functions,
        stub_provided_functions,
        names_with_assert_tags,
        builtin_methods_running_code,
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
        base.id_generation = self
            .id_generation
            .checked_add(1)
            .expect("registry id generation exhausted");
        let generation = base.id_generation;
        base.purity_results = PurityResults::default();
        for (_, id) in base.class_ids.iter_mut() {
            id.set_generation(base.id_generation);
        }
        base.class_declarations = base
            .class_declarations
            .drain()
            .map(|(id, declaration)| (id.with_generation(generation), declaration))
            .collect();
        base.phpstan_ancestry = base
            .phpstan_ancestry
            .iter()
            .map(|id| id.with_generation(generation))
            .collect();
        base.ancestors_by_id = base
            .ancestors_by_id
            .drain()
            .map(|(id, ancestors)| {
                (
                    id.with_generation(generation),
                    ancestors.with_generation(generation),
                )
            })
            .collect();
        base.member_owners = base
            .member_owners
            .drain()
            .map(|(id, owners)| (id.with_generation(generation), owners))
            .collect();
        base.class_const_types = base
            .class_const_types
            .drain()
            .map(|((id, slot), ty)| ((id.with_generation(generation), slot), ty))
            .collect();
        base.class_const_types_by_id.clear();
        for ((id, slot), ty) in &base.class_const_types {
            base.class_const_types_by_id
                .entry(*id)
                .or_default()
                .push((*slot, ty.clone()));
        }
        base.function_results = base
            .function_results
            .drain()
            .map(|(id, result)| (id.with_generation(generation), result))
            .collect();
        base.class_constant_results = base
            .class_constant_results
            .drain()
            .map(|((id, slot), constant)| ((id.with_generation(generation), slot), constant))
            .collect();
        base.class_constant_names = base
            .class_constant_names
            .drain()
            .map(|(id, names)| (id.with_generation(generation), names))
            .collect();
        base.class_method_results
            .retag_classes(|id| id.with_generation(generation));
        base.class_member_topology = base
            .class_member_topology
            .drain()
            .map(|(id, topology)| (id.with_generation(generation), topology))
            .collect();
        base.class_property_results
            .retag_classes(|id| id.with_generation(generation));
        base.class_property_topology = base
            .class_property_topology
            .drain()
            .map(|(id, topology)| (id.with_generation(generation), topology))
            .collect();
        base.class_type_results = base
            .class_type_results
            .drain()
            .map(|(id, results)| (id.with_generation(generation), results))
            .collect();
        base.class_reference_results = base
            .class_reference_results
            .drain()
            .map(|(id, refs)| (id.with_generation(generation), refs))
            .collect();
        base.imported_type_aliases = base
            .imported_type_aliases
            .drain()
            .map(|(id, aliases)| (id.with_generation(generation), aliases))
            .collect();
        for (_, id) in base.function_ids.iter_mut() {
            id.set_generation(base.id_generation);
        }
        base.function_declarations = base
            .function_declarations
            .drain()
            .map(|(id, declaration)| (id.with_generation(generation), declaration))
            .collect();
        base.effects = base
            .effects
            .drain()
            .map(|(id, settled)| (id.with_generation(generation), settled))
            .collect();
        base.stub_effect_results.clear();
        base.stub_holding_classes.clear();
        *self = base;
        self.advance_relation_generation();
    }

    /// Indexes the `DoctrineEntity`-mapped classes by table and by
    /// repository, from one pass over the project's own classes (a
    /// built-in stub declares no entity), in the order of their names.
    pub(crate) fn index_doctrine_entities(&mut self) {
        let mut entities: Vec<(&Name, &crate::registry::class_info::DoctrineEntity)> = self
            .own_classes_by_id()
            .filter_map(|(_, class_name, info)| Some((class_name, info.doctrine_entity.as_ref()?)))
            .collect();
        entities.sort_unstable_by(|(a, _), (b, _)| {
            (a.lower(), a.as_str()).cmp(&(b.lower(), b.as_str()))
        });
        let mut index = DoctrineEntityIndex::default();
        for (class_name, entity) in entities {
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
        self.doctrine_entity_index = Arc::new(index);
    }

    fn doctrine_entity_index(&self) -> &DoctrineEntityIndex {
        &self.doctrine_entity_index
    }

    /// The one `DoctrineEntity`-mapped class whose own `#[ORM\Table(name:
    /// ...)]` matches `table` exactly, if any (`index_doctrine_entities`).
    pub fn doctrine_entity_for_table(
        &self,
        table: &str,
    ) -> Option<&crate::registry::class_info::DoctrineEntity> {
        let class_name = self.doctrine_entity_index().by_table.get(table)?;
        self.class_by_name(class_name)?.doctrine_entity.as_ref()
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
