//! The canonical prints (`syntax::canonical`) of the registry's reflection:
//! classes, functions and their members, as built, with every field. What
//! a field only keeps of the others for speed is left out where it is
//! listed.
//!
//! A project's registry also prints over the stub base it stands on
//! (`CanonicalOver`): each table, map and set as what the project adds,
//! changes and drops of the base's, by what the two hold, not by how their
//! layers are stored.

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
    DoctrineEntityIndex, FunctionReflection, ParameterReflection, PropertyReflection,
    PurityDependency, RawDeclarations, Registry,
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
    redeclared_methods,
    properties,
    consts,
    uses,
    abstract_methods,
    final_methods,
    static_methods,
    final_properties,
    final_methods_by_phpdoc,
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

canonical_struct!(PropertyReflection {
    ty,
    is_static,
    is_readonly,
    allows_private_mutation,
    readonly_by_phpdoc,
    tag_read_only,
    tag_write_only,
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
    purity_dependencies,
    is_transitively_pure,
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

canonical_enum!(PurityDependency {
    Function(name),
    Method(class, method),
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

/// A value printed as its difference from `base`, a value of its kind:
/// what a project's registry holds over the stub base it stands on.
pub(crate) trait CanonicalOver {
    fn canonical_over(&self, base: &Self, p: &mut Printer);
}

/// A value and the base it prints over, as a field of a node.
struct Over<'a, T: ?Sized>(&'a T, &'a T);

impl<T: CanonicalOver + ?Sized> Canonical for Over<'_, T> {
    fn canonical(&self, p: &mut Printer) {
        self.0.canonical_over(self.1, p);
    }
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
    fn canonical_over(&self, base: &Self, p: &mut Printer) {
        let changed: Vec<(&Name, &V)> = self
            .iter()
            .filter(|(name, value)| match base.get_with_name(name.as_str()) {
                Some((declared, was)) => declared.as_str() != name.as_str() || !same(*value, was),
                None => true,
            })
            .collect();
        let dropped: Vec<&Name> = base
            .keys()
            .filter(|name| !self.contains_key(name.as_str()))
            .collect();
        p.node(
            "Over",
            &[("changed", &ByName(changed)), ("dropped", &Names(dropped))],
        );
    }
}

/// `NameTable`'s difference, for a map by exact name.
impl<V: Canonical + PartialEq> CanonicalOver for NameMap<V> {
    fn canonical_over(&self, base: &Self, p: &mut Printer) {
        let changed: Vec<(&Name, &V)> = self
            .iter()
            .filter(|(name, value)| base.get(name.as_str()).is_none_or(|was| !same(*value, was)))
            .collect();
        let dropped: Vec<&Name> = base
            .keys()
            .filter(|name| !self.contains_key(name.as_str()))
            .collect();
        p.node(
            "Over",
            &[("changed", &ByName(changed)), ("dropped", &Names(dropped))],
        );
    }
}

/// The names the base lacks, and the base's names the set lacks.
impl CanonicalOver for NameSet {
    fn canonical_over(&self, base: &Self, p: &mut Printer) {
        let added: Vec<&Name> = self.iter().filter(|name| !base.contains(name)).collect();
        let dropped: Vec<&Name> = base.iter().filter(|name| !self.contains(name)).collect();
        p.node(
            "Over",
            &[("added", &Names(added)), ("dropped", &Names(dropped))],
        );
    }
}

impl<T: CanonicalOver + ?Sized> CanonicalOver for Arc<T> {
    fn canonical_over(&self, base: &Self, p: &mut Printer) {
        (**self).canonical_over(base, p);
    }
}

/// The declarations as built over the base's, when both keep them; whole
/// otherwise.
impl CanonicalOver for Option<Box<RawDeclarations>> {
    fn canonical_over(&self, base: &Self, p: &mut Printer) {
        match (self, base) {
            (Some(raw), Some(base)) => {
                let RawDeclarations { classes, functions } = &**raw;
                p.node(
                    "RawDeclarations",
                    &[
                        ("classes", &Over(classes, &base.classes)),
                        ("functions", &Over(functions, &base.functions)),
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
    fn canonical_over(&self, base: &Self, p: &mut Printer) {
        let Registry {
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
            ancestor_names,
            phpstan_ancestry,
            subtype_children,
            trait_users,
            recorded_parents,
            member_owners,
            effects,
            builtin_methods_running_code,
            base_builtin_methods_running_code,
            methods_by_name,
            global_sites,
        } = self;
        p.node(
            "Registry",
            &[
                ("classes", &Over(classes, &base.classes)),
                ("configured_assertions", configured_assertions),
                (
                    "configured_class_argument_returns",
                    configured_class_argument_returns,
                ),
                ("functions", &Over(functions, &base.functions)),
                (
                    "global_constants",
                    &Over(global_constants, &base.global_constants),
                ),
                ("global_variables", global_variables),
                (
                    "phpstan_class_files",
                    &Over(phpstan_class_files, &base.phpstan_class_files),
                ),
                ("doctrine_entity_index", doctrine_entity_index),
                ("raw", &Over(raw, &base.raw)),
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
                    ),
                ),
                (
                    "built_in_stub_classes",
                    &Over(built_in_stub_classes, &base.built_in_stub_classes),
                ),
                (
                    "unconditionally_declared_functions",
                    &Over(
                        unconditionally_declared_functions,
                        &base.unconditionally_declared_functions,
                    ),
                ),
                (
                    "function_decl_sites",
                    &Over(function_decl_sites, &base.function_decl_sites),
                ),
                (
                    "stub_provided_functions",
                    &Over(stub_provided_functions, &base.stub_provided_functions),
                ),
                (
                    "names_with_assert_tags",
                    &Over(names_with_assert_tags, &base.names_with_assert_tags),
                ),
                (
                    "base_assert_tagged",
                    &Over(base_assert_tagged, &base.base_assert_tagged),
                ),
                (
                    "ancestor_names",
                    &Over(ancestor_names, &base.ancestor_names),
                ),
                (
                    "phpstan_ancestry",
                    &Over(phpstan_ancestry, &base.phpstan_ancestry),
                ),
                (
                    "subtype_children",
                    &Over(subtype_children, &base.subtype_children),
                ),
                ("trait_users", &Over(trait_users, &base.trait_users)),
                (
                    "recorded_parents",
                    &Over(recorded_parents, &base.recorded_parents),
                ),
                ("member_owners", &Over(member_owners, &base.member_owners)),
                ("effects", effects),
                (
                    "builtin_methods_running_code",
                    &Over(
                        builtin_methods_running_code,
                        &base.builtin_methods_running_code,
                    ),
                ),
                (
                    "base_builtin_methods_running_code",
                    &Over(
                        base_builtin_methods_running_code,
                        &base.base_builtin_methods_running_code,
                    ),
                ),
                ("methods_by_name", methods_by_name),
                ("global_sites", global_sites),
            ],
        );
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
    ancestor_names,
    phpstan_ancestry,
    subtype_children,
    trait_users,
    recorded_parents,
    member_owners,
    effects,
    builtin_methods_running_code,
    base_builtin_methods_running_code,
    methods_by_name,
    global_sites,
} skip {
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
