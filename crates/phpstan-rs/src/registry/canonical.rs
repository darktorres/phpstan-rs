//! The canonical prints (`syntax::canonical`) of the registry's reflection:
//! classes, functions and their members, as built, with every field. What
//! a field only keeps of the others for speed is left out where it is
//! listed.

use syntax::{canonical_enum, canonical_struct};

use crate::registry::access_attributes::{AccessAttributes, NamespaceVisibility};
use crate::registry::class_info::ResolvedTypeImport;
use crate::registry::class_info::TraitAlias;
use crate::registry::guard::{Compare, GuardExpr};
use crate::registry::index::BuiltDecl;
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
