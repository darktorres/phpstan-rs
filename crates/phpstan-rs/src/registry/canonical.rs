//! The canonical prints (`syntax::canonical`) of the registry's reflection:
//! classes, functions and their members, as built, with every field. What
//! a field only keeps of the others for speed is left out where it is
//! listed.
//!
//! A project's registry also prints over the stub base it stands on
//! (`CanonicalOver`): each table, map and set as what the project adds,
//! changes and drops of the base's, by what the two hold, not by how their
//! layers are stored. Where the two read from the base's very storage, only
//! the names either holds itself or hides are compared: every other name
//! finds the same entry in both.

use std::sync::Arc;

use syntax::canonical::{Canonical, Printer};
use syntax::name::Name;
use syntax::{canonical_enum, canonical_struct};

use crate::registry::access_attributes::{AccessAttributes, NamespaceVisibility};
use crate::registry::class_info::ResolvedTypeImport;
use crate::registry::class_info::TraitAlias;
use crate::registry::guard::{Compare, GuardExpr};
use crate::registry::index::BuiltDecl;
use crate::registry::name_map::{NameMap, NameSet};
use crate::registry::name_table::NameTable;
use crate::registry::{
    AssertInfo, AssertTarget, ChangedNames, ClassReflection, ConstantReflection,
    DoctrineEntityIndex, FunctionReflection, MethodEntry, ParameterReflection, PropertyReflection,
    RawDeclarations, Registry,
};

canonical_struct!(ClassReflection {
    kind,
    is_abstract,
    is_final,
    is_final_by_phpdoc,
    is_readonly,
    is_immutable,
    extends,
    implements,
    methods,
    member_order,
    member_slots,
    redeclared_methods,
    properties,
    consts,
    uses,
    final_properties,
    final_properties_by_phpdoc,
    trait_aliases,
    resolved_trait_conflicts,
    templates,
    extends_args,
    implements_args,
    uses_args,
    template_bounds,
    template_defaults,
    covariant_templates,
    contravariant_templates,
    is_api,
    is_api_do_not_implement,
    is_internal,
    is_consistent_constructor,
    deprecated,
    attribute_targets,
    attributes,
    enum_case_values,
    type_aliases,
    type_alias_case_refs,
    type_imports,
    method_tags,
    doctrine_entity,
    require_extends,
    require_implements,
    sealed,
    access,
    method_access,
    mixins,
    mixin_case_refs,
    is_anonymous,
    provenance,
});

canonical_struct!(MethodEntry {
    name,
    is_abstract,
    is_final,
    is_static,
    is_final_by_phpdoc,
});

canonical_struct!(PropertyReflection {
    ty,
    is_static,
    is_readonly,
    allows_private_mutation,
    readonly_by_phpdoc,
    tag_read_only,
    tag_write_only,
    from_tag,
    visibility,
    write_visibility,
    implicit_protected_set,
    set_ty,
    is_virtual,
    is_internal,
    deprecated,
    has_default,
    is_abstract,
    has_get_hook,
    has_set_hook,
    get_hook_body,
    set_hook_body,
    ty_native,
    doc_ty,
    has_native_type,
    case_refs,
    set_case_refs,
    attributes,
    provenance,
});

canonical_struct!(ConstantReflection {
    ty,
    visibility,
    is_final,
    native_ty,
    has_phpdoc_type,
    value_ty,
    is_internal,
    deprecated,
    attributes,
    provenance,
});

canonical_struct!(TraitAlias {
    trait_name,
    method,
    alias,
});

canonical_struct!(ResolvedTypeImport {
    local_key,
    source_class_fqn,
    source_alias_name,
});

canonical_struct!(AccessAttributes {
    friends,
    namespace_visibility,
    test_only,
    injectable_version,
    check_injectable_version,
    restrict_trait_to,
    sealed,
});

canonical_struct!(NamespaceVisibility {
    namespace,
    exclude_sub_namespaces,
});

canonical_struct!(ParameterReflection {
    name,
    ty,
    ty_native,
    variadic,
    has_default,
    default_ty,
    doc_ty,
    by_ref,
    has_own_doc,
    explicit_mixed_hint,
    case_refs,
    closure_this_type,
    out_ty,
    attributes,
});

canonical_struct!(FunctionReflection {
    params,
    return_type,
    return_type_native,
    return_has_own_doc,
    return_doc_ty,
    self_out,
    return_case_refs,
    return_explicit_mixed_hint,
    no_discard,
    returns_ref,
    must_use_result,
    visibility,
    is_internal,
    is_api,
    has_override_attr,
    deprecated,
    throws,
    assert_if_true,
    assert_if_false,
    assert,
    declared_pure,
    is_stub,
    is_magic,
    attributes,
    provenance,
} skip {
    stub_effect,
});

canonical_struct!(AssertInfo {
    negated,
    equality,
    ty,
    target,
    case_refs,
    written_ty,
});

canonical_enum!(AssertTarget {
    Param(name),
    ThisSelf,
    ThisProp(name),
    NotFound(name),
    Unknown(name),
});

canonical_enum!(GuardExpr {
    Opaque,
    VersionId(compare, version),
    Not(inner),
    And(terms),
    Or(terms),
});

canonical_enum!(Compare {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
});

canonical_enum!(BuiltDecl {
    Class {
        name,
        conditional,
        guard,
        info,
        namespace,
    },
    Function {
        name,
        conditional,
        guard,
        sig,
        site,
    },
    GlobalConst { name, guard, ty },
});

/// Which names a value and its base are compared at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompareAt {
    /// Where the two read from one base's storage, the names either holds
    /// itself or hides; every name otherwise.
    Layers,
    /// Every name of both: what a test checks the layers against.
    #[cfg(test)]
    Whole,
}

/// A value printed as its difference from `base`, a value of its kind:
/// what a project's registry holds over the stub base it stands on. Both
/// ways of comparing print the same.
pub(crate) trait CanonicalOver {
    fn canonical_over(&self, base: &Self, compare: CompareAt, p: &mut Printer);
}

/// A value and the base it prints over, as a field of a node.
struct Over<'a, T: ?Sized>(&'a T, &'a T, CompareAt);

impl<T: CanonicalOver + ?Sized> Canonical for Over<'_, T> {
    fn canonical(&self, p: &mut Printer) {
        self.0.canonical_over(self.1, self.2, p);
    }
}

/// The names to compare a value and its base at: those `apart` gives,
/// the names the two may hold apart when their layers tell, under
/// `CompareAt::Layers`; every name of both otherwise. Each once, by `key`.
fn names_to_compare<'a, K: Eq + std::hash::Hash>(
    compare: CompareAt,
    apart: impl FnOnce() -> Option<Vec<&'a Name>>,
    every: impl FnOnce() -> Vec<&'a Name>,
    key: impl Fn(&'a Name) -> K,
) -> Vec<&'a Name> {
    let names = match compare {
        CompareAt::Layers => apart().unwrap_or_else(every),
        #[cfg(test)]
        CompareAt::Whole => every(),
    };
    let mut seen: rustc_hash::FxHashSet<K> = rustc_hash::FxHashSet::default();
    names
        .into_iter()
        .filter(|name| seen.insert(key(name)))
        .collect()
}

/// Entries by name, as `Printer::by_name` prints them.
struct ByName<'a, V>(Vec<(&'a Name, &'a V)>);

impl<V: Canonical> Canonical for ByName<'_, V> {
    fn canonical(&self, p: &mut Printer) {
        p.by_name(self.0.iter().copied());
    }
}

/// Names, as a set.
struct Names<'a>(Vec<&'a Name>);

impl Canonical for Names<'_> {
    fn canonical(&self, p: &mut Printer) {
        p.set(self.0.iter().copied());
    }
}

/// Whether `value` is `was`: the same entry, read through to the base, or
/// an equal one.
fn same<V: PartialEq>(value: &V, was: &V) -> bool {
    std::ptr::eq(value, was) || value == was
}

/// The entries whose name, spelling or value the base does not have, and
/// the base's names the table lacks.
impl<V: Canonical + PartialEq> CanonicalOver for NameTable<V> {
    fn canonical_over(&self, base: &Self, compare: CompareAt, p: &mut Printer) {
        let names = names_to_compare(
            compare,
            || self.names_apart(base),
            || self.keys().chain(base.keys()).collect(),
            Name::lower,
        );
        let mut changed: Vec<(&Name, &V)> = Vec::new();
        let mut dropped: Vec<&Name> = Vec::new();
        for name in names {
            match (self.get_with_name(name), base.get_with_name(name)) {
                (Some((own, value)), Some((declared, was))) => {
                    if own.as_str() != declared.as_str() || !same(value, was) {
                        changed.push((own, value));
                    }
                }
                (Some(entry), None) => changed.push(entry),
                (None, Some((declared, _))) => dropped.push(declared),
                (None, None) => {}
            }
        }
        p.node(
            "Over",
            &[("changed", &ByName(changed)), ("dropped", &Names(dropped))],
        );
    }
}

/// `NameTable`'s difference, for a map by exact name.
impl<V: Canonical + PartialEq> CanonicalOver for NameMap<V> {
    fn canonical_over(&self, base: &Self, compare: CompareAt, p: &mut Printer) {
        let names = names_to_compare(
            compare,
            || self.names_apart(base),
            || self.keys().chain(base.keys()).collect(),
            Name::as_str,
        );
        let mut changed: Vec<(&Name, &V)> = Vec::new();
        let mut dropped: Vec<&Name> = Vec::new();
        for name in names {
            match (self.get(name.as_str()), base.get(name.as_str())) {
                (Some(value), Some(was)) => {
                    if !same(value, was) {
                        changed.push((name, value));
                    }
                }
                (Some(value), None) => changed.push((name, value)),
                (None, Some(_)) => dropped.push(name),
                (None, None) => {}
            }
        }
        p.node(
            "Over",
            &[("changed", &ByName(changed)), ("dropped", &Names(dropped))],
        );
    }
}

/// The names the base lacks, and the base's names the set lacks.
impl CanonicalOver for NameSet {
    fn canonical_over(&self, base: &Self, compare: CompareAt, p: &mut Printer) {
        let names = names_to_compare(
            compare,
            || self.names_apart(base),
            || self.iter().chain(base.iter()).collect(),
            Name::as_str,
        );
        let mut added: Vec<&Name> = Vec::new();
        let mut dropped: Vec<&Name> = Vec::new();
        for name in names {
            match (self.contains(name), base.contains(name)) {
                (true, false) => added.push(name),
                (false, true) => dropped.push(name),
                _ => {}
            }
        }
        p.node(
            "Over",
            &[("added", &Names(added)), ("dropped", &Names(dropped))],
        );
    }
}

impl<T: CanonicalOver + ?Sized> CanonicalOver for Arc<T> {
    fn canonical_over(&self, base: &Self, compare: CompareAt, p: &mut Printer) {
        (**self).canonical_over(base, compare, p);
    }
}

/// The declarations as built over the base's, when both keep them; whole
/// otherwise.
impl CanonicalOver for Option<Box<RawDeclarations>> {
    fn canonical_over(&self, base: &Self, compare: CompareAt, p: &mut Printer) {
        match (self, base) {
            (Some(raw), Some(base)) => {
                let RawDeclarations { classes, functions } = &**raw;
                p.node(
                    "RawDeclarations",
                    &[
                        ("classes", &Over(classes, &base.classes, compare)),
                        ("functions", &Over(functions, &base.functions, compare)),
                    ],
                );
            }
            _ => self.canonical(p),
        }
    }
}

/// Every field, each table, map and set over the base's and the rest
/// whole: a project's own state and settings, its effects and sites, which
/// a base holds none of.
impl CanonicalOver for Registry {
    fn canonical_over(&self, base: &Self, compare: CompareAt, p: &mut Printer) {
        let ancestor_names = self.ancestor_names_for_cache();
        let base_ancestor_names = base.ancestor_names_for_cache();
        let member_owners = self.member_owners_for_cache();
        let base_member_owners = base.member_owners_for_cache();
        let effects = self.effects_for_names();
        let base_effects = base.effects_for_names();
        let methods_by_name: rustc_hash::FxHashMap<_, Vec<_>> = self
            .methods_by_name
            .iter()
            .map(|(method, ids)| {
                (
                    method.clone(),
                    ids.iter()
                        .filter_map(|id| self.class_name_by_id(*id).cloned())
                        .collect(),
                )
            })
            .collect();
        let Registry {
            id_generation: _,
            purity_results: _,
            classes,
            class_ids: _,
            project_class_names: _,
            project_class_decls: _,
            configured_assertions,
            configured_class_argument_returns,
            functions,
            function_ids: _,
            project_function_names: _,
            project_function_decls: _,
            global_constants,
            global_variables,
            phpstan_class_files,
            doctrine_entity_index,
            raw,
            raw_active,
            changed,
            phpstan_stubs_loaded,
            scope,
            // What the last lookups of the names in scope made of `scope`
            // and the tables.
            scoped_names: _,
            php_versions,
            unconditionally_declared_classes,
            built_in_stub_classes,
            unconditionally_declared_functions,
            function_decl_sites,
            stub_provided_functions,
            names_with_assert_tags,
            base_assert_tagged,
            ancestors_by_id: _,
            ancestor_names_cache: _,
            phpstan_ancestry: _,
            subtype_children,
            trait_users,
            recorded_parents,
            member_owners: _,
            member_owners_cache: _,
            effects: _,
            builtin_methods_running_code,
            base_builtin_methods_running_code,
            methods_by_name: _,
            unset_properties,
            global_sites,
        } = self;
        p.node(
            "Registry",
            &[
                ("classes", &Over(classes, &base.classes, compare)),
                ("configured_assertions", configured_assertions),
                (
                    "configured_class_argument_returns",
                    configured_class_argument_returns,
                ),
                ("functions", &Over(functions, &base.functions, compare)),
                (
                    "global_constants",
                    &Over(global_constants, &base.global_constants, compare),
                ),
                ("global_variables", global_variables),
                (
                    "phpstan_class_files",
                    &Over(phpstan_class_files, &base.phpstan_class_files, compare),
                ),
                ("doctrine_entity_index", doctrine_entity_index),
                ("raw", &Over(raw, &base.raw, compare)),
                ("raw_active", raw_active),
                ("changed", changed),
                ("phpstan_stubs_loaded", phpstan_stubs_loaded),
                ("scope", scope),
                ("php_versions", php_versions),
                (
                    "unconditionally_declared_classes",
                    &Over(
                        unconditionally_declared_classes,
                        &base.unconditionally_declared_classes,
                        compare,
                    ),
                ),
                (
                    "built_in_stub_classes",
                    &Over(built_in_stub_classes, &base.built_in_stub_classes, compare),
                ),
                (
                    "unconditionally_declared_functions",
                    &Over(
                        unconditionally_declared_functions,
                        &base.unconditionally_declared_functions,
                        compare,
                    ),
                ),
                (
                    "function_decl_sites",
                    &Over(function_decl_sites, &base.function_decl_sites, compare),
                ),
                (
                    "stub_provided_functions",
                    &Over(
                        stub_provided_functions,
                        &base.stub_provided_functions,
                        compare,
                    ),
                ),
                (
                    "names_with_assert_tags",
                    &Over(
                        names_with_assert_tags,
                        &base.names_with_assert_tags,
                        compare,
                    ),
                ),
                (
                    "base_assert_tagged",
                    &Over(base_assert_tagged, &base.base_assert_tagged, compare),
                ),
                (
                    "ancestor_names",
                    &Over(&ancestor_names, &base_ancestor_names, compare),
                ),
                (
                    "subtype_children",
                    &Over(subtype_children, &base.subtype_children, compare),
                ),
                (
                    "trait_users",
                    &Over(trait_users, &base.trait_users, compare),
                ),
                (
                    "recorded_parents",
                    &Over(recorded_parents, &base.recorded_parents, compare),
                ),
                (
                    "member_owners",
                    &Over(&member_owners, &base_member_owners, compare),
                ),
                ("effects", &Over(&effects, &base_effects, compare)),
                (
                    "builtin_methods_running_code",
                    &Over(
                        builtin_methods_running_code,
                        &base.builtin_methods_running_code,
                        compare,
                    ),
                ),
                (
                    "base_builtin_methods_running_code",
                    &Over(
                        base_builtin_methods_running_code,
                        &base.base_builtin_methods_running_code,
                        compare,
                    ),
                ),
                ("methods_by_name", &methods_by_name),
                ("unset_properties", unset_properties),
                ("global_sites", global_sites),
            ],
        );
    }
}

impl CanonicalOver
    for rustc_hash::FxHashMap<crate::registry::effects::DeclKey, crate::registry::effects::Settled>
{
    fn canonical_over(&self, base: &Self, _compare: CompareAt, p: &mut Printer) {
        p.node("Effects", &[("current", self), ("base", base)]);
    }
}

canonical_struct!(Registry {
    classes,
    configured_assertions,
    configured_class_argument_returns,
    functions,
    global_constants,
    global_variables,
    phpstan_class_files,
    doctrine_entity_index,
    raw,
    raw_active,
    changed,
    phpstan_stubs_loaded,
    scope,
    php_versions,
    unconditionally_declared_classes,
    built_in_stub_classes,
    unconditionally_declared_functions,
    function_decl_sites,
    stub_provided_functions,
    names_with_assert_tags,
    base_assert_tagged,
    subtype_children,
    trait_users,
    recorded_parents,
    builtin_methods_running_code,
    base_builtin_methods_running_code,
    unset_properties,
    global_sites,
} skip {
    id_generation,
    purity_results,
    phpstan_ancestry,
    ancestors_by_id,
    ancestor_names_cache,
    member_owners,
    member_owners_cache,
    effects,
    methods_by_name,
    class_ids,
    project_class_names,
    project_class_decls,
    function_ids,
    project_function_names,
    project_function_decls,
    // What the last lookups of the names in scope made of `scope` and the
    // tables.
    scoped_names,
});

canonical_struct!(RawDeclarations { classes, functions });

canonical_struct!(ChangedNames {
    classes,
    functions,
    constants,
});

canonical_struct!(DoctrineEntityIndex {
    by_table,
    by_repository,
});
