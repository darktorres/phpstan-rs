use rustc_hash::FxHashMap;
use syntax::name::Name;

use crate::registry::function_sig::{
    FunctionReflection, ParameterReflection, mark_template_params,
};
use crate::registry::name_context::resolve_names_in_type;
use crate::registry::type_node::parse_type_node;
use phpdoc::WrittenType;
use phpdoc::docblock;
use syntax::ast::Visibility;
use syntax::name_resolution::NameContext;
use types::{self, TemplateParam, Type};

/// `@method`'s own tag-to-`FunctionReflection` synthesis - mirrors the plain
/// `@property`-to-`PropertyReflection` synthesis right above it as closely as
/// the two grammars allow. `ty`/`ty_native` deliberately both end up the
/// same resolved value: a virtual member's own "native" type is a
/// meaningless question (there's no real declaration to have one), so
/// this simply reuses the docblock-resolved type for both, matching
/// `is_own_doc`/`explicit_mixed_hint` being irrelevant here too - a
/// synthesized member's own `FunctionReflection` never participates in
/// `flatten_phpdoc_inheritance`'s own backfill machinery in the first
/// place (it's already fully resolved, not merged from anywhere).
pub(crate) fn method_tag_sig(
    tag: &docblock::MethodTag,
    template_params: &FxHashMap<String, TemplateParam>,
    ctx: &NameContext,
) -> FunctionReflection {
    // Also collects `(raw, resolved)` case refs alongside the resolved
    // `Type` itself - `@method`'s own raw text flows through this exact
    // same mark-then-resolve pipeline every other PHPDoc-typed position
    // uses, so its case refs need the identical
    // `collect_class_name_case_refs` call between the two steps.
    // The tag's own templates, scoped to it like a real method's; a
    // bound or default may name a sibling or a class template.
    let mut template_params = template_params.clone();
    if !tag.templates.is_empty() {
        for t in &tag.templates {
            template_params.insert(t.name.clone(), TemplateParam::unbounded(t.name.clone()));
        }
        let shallow = template_params.clone();
        let resolve_bound = |node: &dyn WrittenType| {
            let ty = mark_template_params(parse_type_node(node), &shallow);
            resolve_names_in_type(ty, ctx)
        };
        for t in &tag.templates {
            template_params.insert(
                t.name.clone(),
                TemplateParam {
                    name: t.name.clone(),
                    bound: Box::new(t.bound.as_ref().map_or(Type::mixed(), |b| resolve_bound(b))),
                    variance: types::Variance::Invariant,
                    scope: Some(tag.name.clone()),
                    default: t.default.as_ref().map(|d| Box::new(resolve_bound(d))),
                },
            );
        }
    }
    let template_params = &template_params;
    let resolve = |node: &dyn WrittenType| -> Type {
        let ty = parse_type_node(node);
        let ty = mark_template_params(ty, template_params);
        resolve_names_in_type(ty, ctx)
    };
    let return_type = tag
        .return_type
        .as_ref()
        .map(|r| resolve(r))
        .unwrap_or_else(Type::mixed);
    let params = tag
        .params
        .iter()
        .map(|p| {
            let ty =
                p.ty.as_ref()
                    .map(|t| resolve(t))
                    .unwrap_or_else(Type::mixed);
            ParameterReflection {
                attributes: Vec::new(),
                name: Name::from(&p.name),
                ty: ty.clone(),
                ty_native: ty,
                variadic: p.variadic,
                has_default: p.has_default,
                default_ty: None,
                doc_ty: None,
                by_ref: p.by_ref,
                has_own_doc: true,
                closure_this_type: None,
                out_ty: None,
            }
        })
        .collect();
    FunctionReflection {
        provenance: crate::registry::provenance::Provenance::default(),
        params,
        return_type: return_type.clone(),
        return_type_native: return_type,
        return_has_own_doc: true,
        return_doc_ty: None,
        self_out: None,
        no_discard: false,
        returns_ref: false,
        must_use_result: false,
        visibility: Visibility::Public,
        is_internal: false,
        is_api: false,
        has_override_attr: false,
        // A magic `@method` tag has no real declaration to carry
        // attributes on at all.
        attributes: Vec::new(),
        deprecated: None,
        throws: None,
        assert_if_true: Vec::new(),
        assert_if_false: Vec::new(),
        assert: Vec::new(),
        declared_pure: false,
        is_stub: false,
        is_magic: true,
    }
}
