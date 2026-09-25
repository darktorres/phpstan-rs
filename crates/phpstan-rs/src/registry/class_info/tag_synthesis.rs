use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::ast::Visibility;
use crate::docblock;
use crate::intern::intern;
use crate::name_resolution::NameContext;
use crate::registry::function_sig::{FunctionSig, ParamSig, mark_template_params};
use crate::registry::name_context::{collect_class_name_case_refs, resolve_names_in_type};
use crate::types::{self, TemplateParam, Type};

/// Mirrors real PHPStan's own `Type::getObjectClassNames()` for exactly
/// the shapes `@phpstan-require-extends`/`-implements`/`@phpstan-sealed`
/// ever need it for: a bare class reference, or a union of several (a
/// real, common `@phpstan-sealed A|B|C` shape). Any other member (an
/// unresolvable name still sitting as a plain `Object`-with-an-unknown-
/// name, a scalar, `self`/`static`, ...) contributes nothing - the same
/// "don't guess" leniency this project already applies to every other
/// type-bearing tag, not a new gap.
pub(crate) fn object_class_names(ty: Type) -> Vec<String> {
    match ty {
        Type::Object(name, _) => vec![name],
        Type::Union(members) => members.into_iter().flat_map(object_class_names).collect(),
        _ => Vec::new(),
    }
}

/// `@method`'s own tag-to-`FunctionSig` synthesis - mirrors the plain
/// `@property`-to-`PropertySig` synthesis right above it as closely as
/// the two grammars allow. `ty`/`ty_native` deliberately both end up the
/// same resolved value: a virtual member's own "native" type is a
/// meaningless question (there's no real declaration to have one), so
/// this simply reuses the docblock-resolved type for both, matching
/// `is_own_doc`/`explicit_mixed_hint` being irrelevant here too - a
/// synthesized member's own `FunctionSig` never participates in
/// `flatten_phpdoc_inheritance`'s own backfill machinery in the first
/// place (it's already fully resolved, not merged from anywhere).
pub(crate) fn method_tag_sig(
    tag: &docblock::MethodTag,
    template_params: &FxHashMap<String, TemplateParam>,
    ctx: &NameContext,
) -> FunctionSig {
    // Also collects `(raw, resolved)` case refs alongside the resolved
    // `Type` itself - `@method`'s own raw text flows through this exact
    // same mark-then-resolve pipeline every other PHPDoc-typed position
    // uses, so its case refs need the identical
    // `collect_class_name_case_refs` call between the two steps.
    // The tag's own templates, scoped to it like a real method's; a
    // bound or default may name a sibling or a class template.
    let mut template_params = template_params.clone();
    if !tag.templates.is_empty() {
        for (name, _, _) in &tag.templates {
            template_params.insert(name.clone(), TemplateParam::unbounded(name.clone()));
        }
        let shallow = template_params.clone();
        let resolve_bound = |text: &str| {
            let ty = mark_template_params(types::parse_type_string(text), &shallow);
            resolve_names_in_type(ty, ctx)
        };
        for (name, bound, default) in &tag.templates {
            template_params.insert(
                name.clone(),
                TemplateParam {
                    name: name.clone(),
                    bound: Box::new(bound.as_deref().map_or(Type::Mixed(false), resolve_bound)),
                    variance: types::Variance::Invariant,
                    scope: Some(tag.name.clone()),
                    default: default.as_deref().map(|d| Box::new(resolve_bound(d))),
                },
            );
        }
    }
    let template_params = &template_params;
    #[allow(clippy::type_complexity)]
    let resolve = |text: &str| -> (Type, Vec<(Arc<str>, Arc<str>)>) {
        let ty = types::parse_type_string(text);
        let ty = mark_template_params(ty, template_params);
        let mut case_refs = Vec::new();
        collect_class_name_case_refs(&ty, ctx, &mut case_refs);
        (resolve_names_in_type(ty, ctx), case_refs)
    };
    let (return_type, return_case_refs) = tag
        .return_type
        .as_deref()
        .map(resolve)
        .unwrap_or((Type::Mixed(false), Vec::new()));
    let params = tag
        .params
        .iter()
        .map(|p| {
            let (ty, case_refs) =
                p.ty.as_deref()
                    .map(resolve)
                    .unwrap_or((Type::Mixed(false), Vec::new()));
            ParamSig {
                name: intern(&p.name),
                ty: ty.clone(),
                ty_native: ty,
                variadic: p.variadic,
                has_default: p.has_default,
                default_ty: None,
                doc_ty: None,
                by_ref: p.by_ref,
                has_own_doc: true,
                explicit_mixed_hint: false,
                case_refs,
                closure_this_type: None,
                out_ty: None,
            }
        })
        .collect();
    FunctionSig {
        params,
        return_type: return_type.clone(),
        return_type_native: return_type,
        return_has_own_doc: true,
        return_doc_ty: None,
        self_out: None,
        return_case_refs,
        return_explicit_mixed_hint: false,
        no_discard: false,
        visibility: Visibility::Public,
        is_internal: false,
        is_api: false,
        has_override_attr: false,
        // A magic `@method` tag has no real declaration to carry
        // attributes on at all.
        attrs: Vec::new(),
        deprecated: None,
        throws: None,
        assert_if_true: Vec::new(),
        assert_if_false: Vec::new(),
        assert: Vec::new(),
        body: None,
        purity_dependencies: None,
        is_transitively_pure: false,
        is_stub: false,
        is_magic: true,
    }
}
