use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::name::{Name, OrderedNameMap};

use syntax::ast::Stmt;
use types::Type;

pub mod access_attributes;
mod anonymous_class;
mod assertion_table;
pub mod bodies;
pub(crate) mod canonical;
mod class_info;
mod container;
mod effects;
mod file_decls;
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
pub mod provenance;
mod purity;
pub mod reflection_attribute;
mod relation;
mod resolve;
pub(crate) mod scope;
mod shared;
mod stub_cache;
pub(crate) use stub_cache::cache_dir;
pub(crate) mod summaries;
pub(crate) use stub_cache::{
    load as load_cached, load_container, store as store_cached, store_container,
};
pub mod settings;
pub mod stubs;
mod type_node;

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
pub(crate) use name_table::{Detached, FoldedKey, Lazy};
pub use shared::Shared;
pub use summaries::BodySummary;
pub type FunctionTable = NameTable<FunctionReflection>;
pub(crate) use effects::{CallContext, Effect, Holder, Storage, Transfer, Written, holder_of};
pub(crate) use file_decls::{ClassLayout, FileDecls, FileMember, FunctionLayout};
pub(crate) use guard::{Guard, GuardExpr};
pub(crate) use ids::{ClassId, FnId, IdSource, MethodRef};
pub(crate) use index::{BuiltDecl, DeclKind, built_declaration, register_global_constant};
pub(crate) use name_context::{collect_class_name_case_refs, resolve_names_in_type};
pub(crate) use name_table::{folded_key, function_key, with_function_key};
pub use provenance::{Origin, Provenance};
pub(crate) use purity::{PurityResults, collect_impure_points, is_non_overridable};
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
    // With each constant's type before its first change.
    pub(crate) constants: FxHashMap<String, Option<Type>>,
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
    effects: FxHashMap<effects::DeclKey, effects::Settled>,
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

/// The winning project declaration's address inside its parsed file.
#[derive(Clone)]
struct ClassDeclAt {
    file: Arc<FileDecls>,
    layout: la_arena::Idx<ClassLayout>,
}

impl std::fmt::Debug for ClassDeclAt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClassDeclAt")
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
struct FunctionDeclAt {
    file: Arc<FileDecls>,
    layout: la_arena::Idx<FunctionLayout>,
}

impl std::fmt::Debug for FunctionDeclAt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionDeclAt")
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Registry {
    /// Changes whenever the declaration id space is replaced. IDs kept by
    /// clients or pass tables from an older base cannot alias new entries.
    #[serde(skip)]
    id_generation: u32,
    #[serde(skip)]
    purity_results: PurityResults,
    #[serde(skip)]
    classes: NameTable<ClassReflection>,
    #[serde(skip)]
    class_ids: NameTable<ClassId>,
    #[serde(skip)]
    project_class_names: Vec<Name>,
    #[serde(skip)]
    project_class_decls: Vec<Option<ClassDeclAt>>,
    // Configured `[[assertions]]`/`[[class_argument_returns]]` entries,
    // on top of the built-in ones (`assertion_table`).
    #[serde(skip)]
    pub(crate) configured_assertions: Vec<crate::assertion_tables::AssertionEntry>,
    #[serde(skip)]
    pub(crate) configured_class_argument_returns: Vec<crate::assertion_tables::ClassArgumentReturn>,
    #[serde(skip)]
    functions: FunctionTable,
    #[serde(skip)]
    function_ids: NameTable<FnId>,
    #[serde(skip)]
    project_function_names: Vec<Name>,
    #[serde(skip)]
    project_function_decls: Vec<Option<FunctionDeclAt>>,
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
    pub(crate) ancestors_by_id: FxHashMap<ClassId, syntax::name::NameSet>,
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
    class_const_types_cache: NameTable<Vec<(u32, Type)>>,
    // Constants composed from traits, keyed by the using class and their
    // stable class-constant slot. The ClassReflection keeps only declarations.
    #[serde(skip)]
    pub(crate) class_constant_results:
        FxHashMap<ClassId, OrderedNameMap<(usize, Arc<ConstantReflection>)>>,
    #[serde(skip)]
    class_constant_results_cache: NameTable<Vec<(Name, u32, Arc<ConstantReflection>)>>,
    // Hook facts inherited by a redeclared property, keyed by the class
    // declaration that owns the completed property. The underlying
    // PropertyReflection remains the as-built declaration.
    #[serde(skip)]
    pub(crate) class_property_results: FxHashMap<ClassId, NameMap<Arc<PropertyReflection>>>,
    #[serde(skip)]
    class_property_results_cache: NameTable<Vec<(Name, Arc<PropertyReflection>)>>,
    // What a call to each function and method with a body may write
    // (`registry::effects`), keyed by declaration ID where one exists.
    #[serde(skip)]
    pub(crate) effects: FxHashMap<effects::EffectId, effects::Settled>,
    // The names, lowercased, of the builtin methods that run code passed
    // to them (`registry::effects`): the base's, and those of the classes
    // the registry holds itself, as the last finish found them. Stored
    // with the registry, whose classes a load reads lazily.
    #[serde(skip)]
    builtin_methods_running_code: NameSet,
    // `builtin_methods_running_code` of the base, as it was when frozen.
    #[serde(skip)]
    base_builtin_methods_running_code: Arc<NameSet>,
    // The loaded classes declaring each method name with a body, lowercased,
    // as the last finish found them (`registry::effects`).
    #[serde(skip)]
    methods_by_name: Arc<FxHashMap<String, Vec<ClassId>>>,
    // The properties the loaded files unset, as the last finish found them
    // (`registry::effects`).
    #[serde(skip)]
    unset_properties: Arc<bodies::UnsetProperties>,
    // What each file's walk for global variables found, kept for the files
    // a later finish doesn't walk again (`infer_global_variables_of`).
    #[serde(skip)]
    global_sites: FxHashMap<crate::file_id::FileId, global_vars::FileSites>,
}

impl Registry {
    /// The project's indexed classes, keyed by PHP's case-insensitive class name.
    pub fn classes(&self) -> &NameTable<ClassReflection> {
        &self.classes
    }

    /// The project's indexed functions, keyed by PHP's case-insensitive function name.
    pub fn functions(&self) -> &FunctionTable {
        &self.functions
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
        let (name, current) = self.classes.get_with_name(name)?;
        let id = *self.class_ids.get(name)?;
        Some((id, name, self.class_by_id(id).unwrap_or(current)))
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

    /// A class declaration addressed by its source-range id. While the
    /// registry exposes its as-built table for an edit, this reads an
    /// unmerged project declaration from its file arena.
    pub(crate) fn class_by_id(&self, id: ClassId) -> Option<&ClassReflection> {
        self.debug_check_id_generation(id.generation());
        match id.source()? {
            IdSource::Stub => {
                let name = self.classes.base_name(id.local() as usize)?;
                (self.class_ids.get(name) == Some(&id))
                    .then(|| self.classes.get(name))
                    .flatten()
            }
            // The context range is reserved for item 17's context base.
            IdSource::Context => None,
            IdSource::Project => {
                let name = self.project_class_names.get(id.local() as usize)?;
                if self.class_ids.get(name) != Some(&id) {
                    return None;
                }
                if self.raw_active
                    && let Some(declaration) = self.file_class_by_id(id)
                {
                    return Some(declaration);
                }
                self.classes.get(name)
            }
        }
    }

    pub(crate) fn class_name_by_id(&self, id: ClassId) -> Option<&Name> {
        self.debug_check_id_generation(id.generation());
        let name = match id.source()? {
            IdSource::Stub => self.classes.base_name(id.local() as usize)?,
            IdSource::Context => return None,
            IdSource::Project => self.project_class_names.get(id.local() as usize)?,
        };
        (self.class_ids.get(name) == Some(&id)).then_some(name)
    }

    /// The immutable declaration as built in its file, when this project id
    /// still names one unmerged file declaration. Resolved or merged values
    /// remain available through `class_by_id` until the Registry finishes its
    /// id-store migration.
    pub(crate) fn file_class_by_id(&self, id: ClassId) -> Option<&ClassReflection> {
        self.debug_check_id_generation(id.generation());
        if id.source()? != IdSource::Project {
            return None;
        }
        let declaration = self
            .project_class_decls
            .get(id.local() as usize)?
            .as_ref()?;
        Some(declaration.file.class(declaration.layout))
    }

    pub(crate) fn file_method_by_class_id(
        &self,
        id: ClassId,
        method: &str,
    ) -> Option<&FunctionReflection> {
        let method_ref = self.file_method_ref_by_class_id(id, method)?;
        self.file_method_by_ref(method_ref)
    }

    pub(crate) fn file_method_ref_by_class_id(
        &self,
        id: ClassId,
        method: &str,
    ) -> Option<MethodRef> {
        let class = self.class_by_id(id)?;
        let slot = *class.member_slots.get(method)?;
        Some(MethodRef::new(id, slot))
    }

    pub(crate) fn file_method_by_ref(&self, method: MethodRef) -> Option<&FunctionReflection> {
        let id = method.class();
        let class = self.class_by_id(id)?;
        let entry = class.member_order.get(usize::from(method.slot()))?;
        class.methods.get(&entry.name)
    }

    pub(crate) fn file_property_by_class_id(
        &self,
        id: ClassId,
        property: &str,
    ) -> Option<&PropertyReflection> {
        self.debug_check_id_generation(id.generation());
        if id.source()? != IdSource::Project {
            return None;
        }
        let declaration = self
            .project_class_decls
            .get(id.local() as usize)?
            .as_ref()?;
        declaration.file.property(declaration.layout, property)
    }

    pub(crate) fn file_constant_by_class_id(
        &self,
        id: ClassId,
        constant: &str,
    ) -> Option<&ConstantReflection> {
        self.debug_check_id_generation(id.generation());
        if id.source()? != IdSource::Project {
            return None;
        }
        let declaration = self
            .project_class_decls
            .get(id.local() as usize)?
            .as_ref()?;
        declaration.file.constant(declaration.layout, constant)
    }

    /// The id and declaration selected for `name` by PHP's function-name rules.
    pub(crate) fn function(&self, name: &str) -> Option<(FnId, &Name, &FunctionReflection)> {
        let (name, current) = self.functions.get_with_name(name)?;
        let id = *self.function_ids.get(name)?;
        Some((id, name, self.function_by_id(id).unwrap_or(current)))
    }

    /// A function declaration addressed by its source-range id. While the
    /// registry exposes its as-built table for an edit, this reads an
    /// unmerged project declaration from its file arena.
    pub(crate) fn function_by_id(&self, id: FnId) -> Option<&FunctionReflection> {
        self.debug_check_id_generation(id.generation());
        match id.source()? {
            IdSource::Stub => {
                let name = self.functions.base_name(id.local() as usize)?;
                (self.function_ids.get(name) == Some(&id))
                    .then(|| self.functions.get(name))
                    .flatten()
            }
            // The context range is reserved for item 17's context base.
            IdSource::Context => None,
            IdSource::Project => {
                let name = self.project_function_names.get(id.local() as usize)?;
                if self.function_ids.get(name) != Some(&id) {
                    return None;
                }
                if self.raw_active
                    && let Some(declaration) = self.file_function_by_id(id)
                {
                    return Some(declaration);
                }
                self.functions.get(name)
            }
        }
    }

    pub(crate) fn function_name_by_id(&self, id: FnId) -> Option<&Name> {
        self.debug_check_id_generation(id.generation());
        let name = match id.source()? {
            IdSource::Stub => self.functions.base_name(id.local() as usize)?,
            IdSource::Context => return None,
            IdSource::Project => self.project_function_names.get(id.local() as usize)?,
        };
        (self.function_ids.get(name) == Some(&id)).then_some(name)
    }

    /// The immutable function declaration as built in its file, when this
    /// project id still names one unmerged file declaration.
    pub(crate) fn file_function_by_id(&self, id: FnId) -> Option<&FunctionReflection> {
        self.debug_check_id_generation(id.generation());
        if id.source()? != IdSource::Project {
            return None;
        }
        let declaration = self
            .project_function_decls
            .get(id.local() as usize)?
            .as_ref()?;
        Some(declaration.file.function(declaration.layout))
    }

    fn ensure_class_id(&mut self, name: &Name) {
        if self.class_ids.contains_key(name) {
            return;
        }
        let local = u32::try_from(self.project_class_names.len())
            .expect("project class id range exhausted");
        let id = ClassId::project_in_generation(local, self.id_generation);
        self.class_ids.insert(name.clone(), id);
        self.project_class_names.push(name.clone());
        self.project_class_decls.push(None);
    }

    fn ensure_function_id(&mut self, name: &Name) {
        if self.function_ids.contains_key(name) {
            return;
        }
        let local = u32::try_from(self.project_function_names.len())
            .expect("project function id range exhausted");
        let id = FnId::project_in_generation(local, self.id_generation);
        self.function_ids.insert(name.clone(), id);
        self.project_function_names.push(name.clone());
        self.project_function_decls.push(None);
    }

    fn remove_project_class_id(&mut self, name: &Name) {
        if let Some(id) = self
            .class_ids
            .get(name)
            .copied()
            .filter(|id| id.source() == Some(IdSource::Project))
        {
            self.class_ids.remove(name);
            self.project_class_decls[id.local() as usize] = None;
            self.purity_results
                .pure_methods
                .retain(|method| method.class() != id);
            self.purity_results
                .method_dependencies
                .retain(|method, _| method.class() != id);
            self.class_const_types.retain(|(class, _), _| *class != id);
            self.class_constant_results.remove(&id);
            self.class_property_results.remove(&id);
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
            self.project_function_decls[id.local() as usize] = None;
            self.purity_results.pure_functions.remove(&id);
            self.purity_results.function_dependencies.remove(&id);
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

    pub fn build(program: &[Stmt]) -> Registry {
        let mut registry = Registry::index_and_flatten(program, &Origin::Unknown);
        registry.resolve_transitive_purity(&FileBodies::of(program));
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
        registry.index_assert_tagged_names();
        registry.index_doctrine_entities();
        registry.infer_global_variables(&[(
            crate::file_id::FileId::new(0),
            std::path::Path::new("<flattened>"),
            &GlobalFacts::of(program),
        )]);
        registry
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
            effects: registry.effects_for_names(),
            global_variables: registry.global_variables().clone(),
        };
        if scope.is_all() {
            self.class_const_types.clear();
            self.class_constant_results.clear();
            self.class_property_results.clear();
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
        let changed_class_ids: FxHashSet<ClassId> = self
            .class_ids
            .iter()
            .filter(|(name, _)| scope.has_class(name))
            .map(|(_, id)| *id)
            .collect();
        self.class_const_types
            .retain(|(id, _), _| !changed_class_ids.contains(id));
        self.class_constant_results
            .retain(|id, _| !changed_class_ids.contains(id));
        self.class_property_results
            .retain(|id, _| !changed_class_ids.contains(id));
        let functions = reset_scoped(
            &mut self.functions,
            &raw.functions,
            scope.function_keys(),
            body_scope.and_then(scope::Scope::function_keys),
        );
        body_scope.map(|_| snapshot(classes, functions, self))
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
                    .map(|ancestors| (name.clone(), ancestors.clone()))
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
        let class_constant_results = self.class_constant_results_for_cache();
        let class_property_results = self.class_property_results_for_cache();
        let effects = self.effects_for_names();
        self.classes.freeze();
        self.functions.freeze();
        self.class_ids = NameTable::default();
        self.project_class_names.clear();
        self.project_class_decls.clear();
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
        self.function_ids = NameTable::default();
        self.project_function_names.clear();
        self.project_function_decls.clear();
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
        self.rebuild_effect_ids(effects);
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
                        .map(|id| (id, ancestors.clone()))
                })
                .collect();
            self.ancestor_names_cache = NameTable::default();
        } else {
            self.ancestors_by_id = ancestor_names
                .into_iter()
                .filter_map(|(name, ancestors)| {
                    self.class_ids.get(&name).copied().map(|id| (id, ancestors))
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
        self.class_const_types_cache = NameTable::default();
        self.class_constant_results = class_constant_results
            .iter()
            .filter_map(|(name, constants)| {
                self.class_ids.get(name).copied().map(|id| {
                    (
                        id,
                        constants
                            .iter()
                            .map(|(name, slot, constant)| {
                                (
                                    name.clone(),
                                    (
                                        usize::try_from(*slot).expect("usize is at least 32 bits"),
                                        Arc::clone(constant),
                                    ),
                                )
                            })
                            .collect::<OrderedNameMap<_>>(),
                    )
                })
            })
            .collect();
        self.class_constant_results_cache = NameTable::default();
        self.class_property_results = class_property_results
            .iter()
            .filter_map(|(name, properties)| {
                self.class_ids
                    .get(name)
                    .copied()
                    .map(|id| (id, properties.iter().cloned().collect::<NameMap<_>>()))
            })
            .collect();
        self.class_property_results_cache = NameTable::default();
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
        if let Some(raw) = &mut self.raw {
            raw.classes.freeze();
            raw.functions.freeze();
        }
    }

    pub(crate) fn record_changed_class(&mut self, name: &str) {
        if self.raw.is_some() {
            self.changed.classes.insert(name.to_ascii_lowercase());
        }
    }

    pub(crate) fn record_changed_function(&mut self, name: &str) {
        if self.raw.is_some() {
            self.changed.functions.insert(name.to_ascii_lowercase());
        }
    }

    fn record_changed_constant(&mut self, name: &str) {
        if self.raw.is_some() && !self.changed.constants.contains_key(name) {
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
        let effects = self.effects_for_names();
        let decls = before
            .effects
            .keys()
            .chain(effects.keys())
            .filter(|key| before.effects.get(*key) != effects.get(*key));
        for key in decls {
            keys.insert(match &key.class {
                Some(class) => RefKey::class_text(class),
                None => RefKey::function_text(&key.member),
            });
        }
        keys
    }

    /// The traits the classes whose resolved form differs from `before`'s
    /// use, before the change and after it: a trait is checked as each class
    /// using it, so its file is checked again when one of them changes.
    pub(crate) fn traits_of_changed_classes(&self, before: &ResolvedSnapshot) -> Vec<Name> {
        let mut traits: Vec<Name> = Vec::new();
        let class_keys = before.class_keys.iter().flat_map(|keys| keys.iter());
        for (key, old) in class_keys.zip(&before.classes) {
            let now = self.classes.get(&FoldedKey(key));
            if !differs(now, old.as_ref()) {
                continue;
            }
            for info in now.into_iter().chain(old.as_ref().map(Detached::get)) {
                for used in &info.uses {
                    if !traits.contains(used) {
                        traits.push(used.clone());
                    }
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
        self.ancestor_names_for_cache()
            .store(writer, "table:ancestor_names")?;
        self.member_owners_for_cache()
            .store(writer, "table:member_owners")?;
        self.class_const_types_for_cache()
            .store(writer, "table:class_const_types")?;
        self.class_constant_results_for_cache()
            .store(writer, "table:class_constant_results")?;
        self.class_property_results_for_cache()
            .store(writer, "table:class_property_results")?;
        self.store_tables(writer)?;
        if let Some(raw) = &self.raw {
            raw.classes.store(writer, RAW_CLASSES)?;
            raw.functions.store(writer, RAW_FUNCTIONS)?;
        }
        writer.section(REST, &stub_cache::encode(self)?)
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
                    .map(|ancestors| (name.clone(), ancestors.clone()))
            })
            .collect()
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

    fn class_constant_results_for_cache(
        &self,
    ) -> NameTable<Vec<(Name, u32, Arc<ConstantReflection>)>> {
        if !self.class_constant_results_cache.is_empty() {
            return self.class_constant_results_cache.clone();
        }
        self.class_constant_results
            .iter()
            .filter_map(|(id, constants)| {
                let mut constants: Vec<_> = constants
                    .iter()
                    .map(|(name, (slot, constant))| {
                        (
                            name.clone(),
                            u32::try_from(*slot).expect("class constant slot range exhausted"),
                            Arc::clone(constant),
                        )
                    })
                    .collect();
                constants.sort_unstable_by_key(|(_, slot, _)| *slot);
                self.class_name_by_id(*id)
                    .map(|class| (class.clone(), constants))
            })
            .collect()
    }

    fn class_property_results_for_cache(&self) -> NameTable<Vec<(Name, Arc<PropertyReflection>)>> {
        if !self.class_property_results_cache.is_empty() {
            return self.class_property_results_cache.clone();
        }
        self.class_property_results
            .iter()
            .filter_map(|(id, properties)| {
                let mut properties: Vec<_> = properties
                    .iter()
                    .map(|(name, property)| (name.clone(), Arc::clone(property)))
                    .collect();
                properties.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
                self.class_name_by_id(*id)
                    .map(|name| (name.clone(), properties))
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
        self.ancestor_names_for_cache()
            .check_order("table:ancestor_names")?;
        self.member_owners_for_cache()
            .check_order("table:member_owners")?;
        self.class_const_types_for_cache()
            .check_order("table:class_const_types")?;
        self.class_constant_results_for_cache()
            .check_order("table:class_constant_results")?;
        self.class_property_results_for_cache()
            .check_order("table:class_property_results")?;
        if let Some(raw) = &self.raw {
            raw.classes.check_order(RAW_CLASSES)?;
            raw.functions.check_order(RAW_FUNCTIONS)?;
        }
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
        let mut registry: Registry = stub_cache::decode(&reader.buffer()[rest])?;
        registry.load_tables(reader)?;
        registry.ancestor_names_cache = NameTable::load(reader, "table:ancestor_names")?;
        registry.member_owners_cache = NameTable::load(reader, "table:member_owners")?;
        registry.class_const_types_cache = NameTable::load(reader, "table:class_const_types")?;
        registry.class_constant_results_cache =
            NameTable::load(reader, "table:class_constant_results")?;
        registry.class_property_results_cache =
            NameTable::load(reader, "table:class_property_results")?;
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
        base.phpstan_ancestry = base
            .phpstan_ancestry
            .iter()
            .map(|id| id.with_generation(generation))
            .collect();
        base.ancestors_by_id = base
            .ancestors_by_id
            .drain()
            .map(|(id, ancestors)| (id.with_generation(generation), ancestors))
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
        base.class_constant_results = base
            .class_constant_results
            .drain()
            .map(|(id, constants)| (id.with_generation(generation), constants))
            .collect();
        base.class_property_results = base
            .class_property_results
            .drain()
            .map(|(id, properties)| (id.with_generation(generation), properties))
            .collect();
        for (_, id) in base.function_ids.iter_mut() {
            id.set_generation(base.id_generation);
        }
        base.effects = base
            .effects
            .drain()
            .map(|(id, settled)| (id.with_generation(generation), settled))
            .collect();
        *self = base;
    }

    /// Indexes the `DoctrineEntity`-mapped classes by table and by
    /// repository, from one pass over the project's own classes (a
    /// built-in stub declares no entity), in the order of their names.
    pub(crate) fn index_doctrine_entities(&mut self) {
        let mut entities: Vec<(&Name, &crate::registry::class_info::DoctrineEntity)> = self
            .classes
            .own_iter()
            .filter_map(|(class_name, info)| Some((class_name, info.doctrine_entity.as_ref()?)))
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
