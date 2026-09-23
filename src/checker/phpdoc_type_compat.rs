//! Real PHPStan's `IncompatiblePhpDocTypeRule` (`IncompatiblePhpDocTypeCheck`,
//! level 2) for a named function or method: every `@param`/`@param-out`
//! tag must name a real parameter (`parameter.notFound`), `@param-out`
//! needs a by-reference one (`parameter.notByRef`), and a `@param`/
//! `@param-out`/`@return` type must fit its native hint
//! (`parameter.phpDocType`/`return.phpDocType`) - "incompatible" when no
//! value can satisfy both, "not subtype" when only some can.
//! `IncompatiblePropertyPhpDocTypeRule` does the same for a property's
//! `@var` (a promoted property's constructor `@param` or inline `@var`,
//! reported as its "PHPDoc type") against its native type
//! (`property.phpDocType`).

use super::*;
use crate::docblock;
use crate::registry::{FunctionSig, Trinary};

impl<'a> Checker<'a> {
    pub(crate) fn check_phpdoc_types_against_native(
        &mut self,
        f: &FunctionDecl,
        sig: &FunctionSig,
        class_name: Option<&str>,
        label: &str,
    ) {
        let Some(doc) = f.doc_comment.as_deref() else {
            return;
        };
        if !doc.contains("@") {
            return;
        }
        let block = docblock::parse(doc);
        let nodes = super::phpdoc_unresolvable::tag_type_nodes(doc);
        let resolve = |this: &Self, ty: &Type| match class_name {
            Some(c) => this.resolve_pseudo(ty, c),
            None => ty.clone(),
        };
        // Reported in the order the tags are written.
        let in_doc_order = |map: &std::collections::HashMap<String, String>| {
            let mut names: Vec<String> = map.keys().cloned().collect();
            names.sort_by_key(|n| doc.find(&format!("${n}")).unwrap_or(usize::MAX));
            names
        };
        for (tag, names) in [
            ("@param", in_doc_order(&block.params)),
            ("@param-out", in_doc_order(&block.param_out)),
        ] {
            for name in &names {
                let Some(i) = f.params.iter().position(|p| *p.name == **name) else {
                    self.report(
                        label.to_string(),
                        format!("PHPDoc tag {tag} references unknown parameter: ${name}"),
                        f.name_span,
                        2,
                        "parameter.notFound",
                    );
                    continue;
                };
                let (Some(param), Some(param_sig)) = (f.params.get(i), sig.params.get(i)) else {
                    continue;
                };
                let node = if tag == "@param-out" {
                    nodes.param_out.get(name.as_str())
                } else {
                    nodes.params.get(name.as_str())
                };
                if node.is_some_and(|n| self.type_node_unresolvable(n, class_name)) {
                    self.report(
                        label.to_string(),
                        format!(
                            "PHPDoc tag {tag} for parameter ${name} contains unresolvable type."
                        ),
                        f.name_span,
                        2,
                        "parameter.unresolvableType",
                    );
                    continue;
                }
                let doc_ty = if tag == "@param-out" {
                    if !param.by_ref {
                        self.report(
                            label.to_string(),
                            format!(
                                "Parameter ${name} for PHPDoc tag {tag} is not passed by reference."
                            ),
                            f.name_span,
                            2,
                            "parameter.notByRef",
                        );
                        continue;
                    }
                    param_sig.out_ty.as_ref()
                } else {
                    param_sig.doc_ty.as_ref()
                };
                let Some(doc_ty) = doc_ty else {
                    continue;
                };
                let native = resolve(self, &param_sig.ty_native);
                let mut doc_ty = resolve(self, doc_ty);
                // A variadic `@param int[] ...$xs` describes the whole
                // collected array; its elements are what the native hint
                // types.
                if tag == "@param"
                    && param.variadic
                    && array_value_type(&doc_ty).is_some()
                    && !type_can_be_array(&native)
                {
                    doc_ty = array_value_type(&doc_ty).unwrap();
                }
                self.report_phpdoc_native_mismatch(
                    &format!("PHPDoc tag {tag} for parameter ${name}"),
                    &doc_ty,
                    &native,
                    f.name_span,
                    label,
                    "parameter.phpDocType",
                );
            }
        }
        if nodes
            .return_type
            .as_ref()
            .is_some_and(|n| self.type_node_unresolvable(n, class_name))
        {
            self.report(
                label.to_string(),
                "PHPDoc tag @return contains unresolvable type.".to_string(),
                f.name_span,
                2,
                "return.unresolvableType",
            );
        } else if let Some(doc_ty) = &sig.return_doc_ty {
            let native = resolve(self, &sig.return_type_native);
            let doc_ty = resolve(self, doc_ty);
            self.report_phpdoc_native_mismatch(
                "PHPDoc tag @return",
                &doc_ty,
                &native,
                f.name_span,
                label,
                "return.phpDocType",
            );
        }
    }

    pub(crate) fn check_property_phpdoc_types(&mut self, fqn: &str, c: &ClassDecl) {
        if !self.registry.classes.contains_key(fqn) {
            return;
        }
        // A promoted property's PHPDoc type is its constructor `@param`,
        // else its own inline `@var`.
        let ctor = c
            .methods
            .iter()
            .find(|m| m.function.name.as_ref() == "__construct");
        let ctor_nodes = ctor
            .and_then(|m| m.function.doc_comment.as_deref())
            .map(super::phpdoc_unresolvable::tag_type_nodes)
            .unwrap_or_default();
        let promoted = ctor
            .into_iter()
            .flat_map(|m| m.function.params.iter())
            .filter(|p| p.promoted_visibility.is_some())
            .map(|p| {
                let node = ctor_nodes.params.get(p.name.as_ref()).cloned().or_else(|| {
                    p.doc_comment
                        .as_deref()
                        .and_then(|d| super::phpdoc_unresolvable::tag_type_nodes(d).var)
                });
                (p.name.clone(), p.span, "PHPDoc type", node)
            });
        let declared = c.properties.iter().map(|p| {
            let node = p
                .doc_comment
                .as_deref()
                .and_then(|d| super::phpdoc_unresolvable::tag_type_nodes(d).var);
            (p.name.clone(), p.span, "PHPDoc tag @var", node)
        });
        let entries: Vec<_> = declared.chain(promoted).collect();
        let mut checks: Vec<(Type, Type, String, Span)> = Vec::new();
        for (name, span, description, node) in entries {
            let subject = format!("{description} for property {fqn}::${name}");
            if node.is_some_and(|n| self.type_node_unresolvable(&n, Some(fqn))) {
                self.report(
                    fqn.to_string(),
                    format!("{subject} contains unresolvable type."),
                    span,
                    2,
                    "property.unresolvableType",
                );
                continue;
            }
            let Some(prop) = self
                .registry
                .classes
                .get(fqn)
                .and_then(|info| info.properties.get(name.as_ref()))
            else {
                continue;
            };
            if !prop.has_native_type {
                continue;
            }
            let Some(doc) = prop.doc_ty.clone() else {
                continue;
            };
            checks.push((doc, prop.ty_native.clone(), subject, span));
        }
        for (doc, native, subject, span) in checks {
            let doc = self.resolve_pseudo(&doc, fqn);
            let native = self.resolve_pseudo(&native, fqn);
            self.report_phpdoc_native_mismatch(
                &subject,
                &doc,
                &native,
                span,
                fqn,
                "property.phpDocType",
            );
        }
    }

    /// The shared "incompatible"/"not subtype" report of every
    /// PHPDoc-vs-native rule, `subject` naming the tag and its target.
    pub(crate) fn report_phpdoc_native_mismatch(
        &mut self,
        subject: &str,
        doc_ty: &Type,
        native: &Type,
        span: Span,
        label: &str,
        identifier: &'static str,
    ) {
        let doc_ty = &self.registry.absorb_union_subtypes(doc_ty);
        let verdict = match self.registry.is_super_type_of(native, doc_ty) {
            Trinary::Yes => return,
            Trinary::No => "is incompatible with",
            Trinary::Maybe => "is not subtype of",
        };
        self.report(
            label.to_string(),
            format!("{subject} with type {doc_ty} {verdict} native type {native}."),
            span,
            2,
            identifier,
        );
    }
}

/// The element type of an array-typed PHPDoc value (`int[]`,
/// `array<string, int>`, `list<int>`), `None` for anything else.
fn array_value_type(ty: &Type) -> Option<Type> {
    match ty {
        Type::Array(_, value) => Some((**value).clone()),
        Type::Intersection(members) => members.iter().find_map(array_value_type),
        _ => None,
    }
}

/// Whether some value of a native hint could be an array - `array`,
/// `iterable`, `mixed`, or a union including one.
fn type_can_be_array(ty: &Type) -> bool {
    match ty {
        Type::Array(..) | Type::ArrayShape(_) | Type::Mixed(_) => true,
        Type::Union(members) | Type::Intersection(members) => members.iter().any(type_can_be_array),
        _ => false,
    }
}
