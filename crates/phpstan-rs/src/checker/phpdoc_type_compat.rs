//! Real PHPStan's `IncompatiblePhpDocTypeRule` (`IncompatiblePhpDocTypeCheck`,
//! level 2) for a named function or method: every `@param`/`@param-out`
//! tag must name a real parameter (`parameter.notFound`), `@param-out`
//! needs a by-reference one (`parameter.notByRef`), and a `@param`/
//! `@return` type must fit its native hint (`parameter.phpDocType`/
//! `return.phpDocType`) - "incompatible" when no value can satisfy both,
//! "not subtype" when only some can. A `@param-out` type needn't: PHP
//! checks a parameter's native type on entry only, so a by-reference
//! argument may leave holding anything.
//! `IncompatiblePropertyPhpDocTypeRule` does the same for a property's
//! `@var` (a promoted property's constructor `@param` or inline `@var`,
//! reported as its "PHPDoc type") against its native type
//! (`property.phpDocType`).

use super::*;
use crate::registry::FunctionReflection;
use phpdoc::docblock;
use types::TrinaryLogic;

impl<'a> Checker<'a> {
    pub(crate) fn check_phpdoc_types_against_native(
        &mut self,
        f: &FunctionDecl,
        sig: &FunctionReflection,
        class_name: Option<&str>,
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
        let in_doc_order = |map: &rustc_hash::FxHashMap<String, phpdoc::grammar::TypeNode>| {
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
                        format!("PHPDoc tag {tag} references unknown parameter: ${name}"),
                        f.name_span,
                        2,
                        "parameter.notFound",
                    );
                    continue;
                };
                let (Some(param), Some(param_sig)) = (f.params.get(i), sig.params().get(i)) else {
                    continue;
                };
                let node = if tag == "@param-out" {
                    nodes.param_out.get(name.as_str())
                } else {
                    nodes.params.get(name.as_str())
                };
                if node.is_some_and(|n| self.type_node_unresolvable(n, class_name)) {
                    self.report(
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
                            format!(
                                "Parameter ${name} for PHPDoc tag {tag} is not passed by reference."
                            ),
                            f.name_span,
                            2,
                            "parameter.notByRef",
                        );
                        continue;
                    }
                    param_sig.out_ty().as_ref()
                } else {
                    param_sig.doc_ty().as_ref()
                };
                let Some(doc_ty) = doc_ty else {
                    continue;
                };
                let native = resolve(self, param_sig.ty_native());
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
                if tag == "@param" {
                    self.report_phpdoc_native_mismatch(
                        &format!("PHPDoc tag {tag} for parameter ${name}"),
                        &doc_ty,
                        &native,
                        f.name_span,
                        "parameter.phpDocType",
                    );
                }
                self.check_generic_callables(
                    &doc_ty,
                    &format!("{tag} for parameter ${name}"),
                    Some((f.name.as_ref(), block.templates.as_slice())),
                    class_name,
                    f.name_span,
                );
            }
        }
        if nodes
            .return_type
            .as_ref()
            .is_some_and(|n| self.type_node_unresolvable(n, class_name))
        {
            self.report(
                "PHPDoc tag @return contains unresolvable type.".to_string(),
                f.name_span,
                2,
                "return.unresolvableType",
            );
        } else if let Some(doc_ty) = sig.return_doc_ty() {
            let native = resolve(self, sig.return_type_native());
            let doc_ty = resolve(self, doc_ty);
            self.report_phpdoc_native_mismatch(
                "PHPDoc tag @return",
                &doc_ty,
                &native,
                f.name_span,
                "return.phpDocType",
            );
            self.check_generic_callables(
                &doc_ty,
                "@return",
                Some((f.name.as_ref(), block.templates.as_slice())),
                class_name,
                f.name_span,
            );
        }
    }

    /// `GenericCallableRuleHelper`: every callable type with its own
    /// templates inside `ty` (the type of the tag at `location`) - their
    /// declarations validated like `@template` tags, and none may reuse
    /// the name of `function`'s (name, templates) or `class`'s templates
    /// (`callable.shadowTemplate`).
    pub(crate) fn check_generic_callables(
        &mut self,
        ty: &Type,
        location: &str,
        function: Option<(&str, &[String])>,
        class: Option<&str>,
        span: Span,
    ) {
        fn collect(ty: &Type, out: &mut Vec<Type>) {
            if let Some(sig) = ty.callable_sig()
                && matches!(ty, Type::Callable(_) | Type::Intersection(_))
            {
                if !sig.templates.is_empty() {
                    out.push(ty.clone());
                }
                for p in &sig.params {
                    collect(&p.ty, out);
                }
                collect(&sig.return_type, out);
                return;
            }
            let _ = ty.map_children(&mut |c| {
                collect(c, out);
                c.clone()
            });
        }
        let mut callables = Vec::new();
        collect(ty, &mut callables);
        if callables.is_empty() {
            return;
        }
        let class_info = class.and_then(|c| self.registry.classes.get(c));
        let type_aliases = class_info
            .map(|info| info.type_aliases().clone())
            .unwrap_or_default();
        let class_templates: Vec<String> = class_info
            .map(|info| info.templates().iter().map(|t| t.to_string()).collect())
            .unwrap_or_default();
        for callable in callables {
            let Some(sig) = callable.callable_sig() else {
                continue;
            };
            let description = callable.to_string();
            let names: Vec<std::sync::Arc<str>> = sig
                .templates
                .iter()
                .map(|t| t.name.as_str().into())
                .collect();
            let bounds = sig
                .templates
                .iter()
                .filter(|t| !matches!(*t.bound, Type::Mixed(_)))
                .map(|t| (t.name.as_str().into(), (*t.bound).clone()))
                .collect();
            let defaults = sig
                .templates
                .iter()
                .filter_map(|t| Some((t.name.as_str().into(), (**t.default.as_ref()?).clone())))
                .collect();
            self.check_template_declaration_validity(
                &names,
                &bounds,
                &defaults,
                &type_aliases,
                crate::checker::generics::TemplateSubject::Callable {
                    location,
                    description: &description,
                },
                span,
            );
            let mut shadowed = Vec::new();
            if let Some((function_name, function_templates)) = function {
                let owner = match class {
                    Some(c) => format!("method {c}::{function_name}"),
                    None => format!("function {}", self.name_ctx.declared_name(function_name)),
                };
                for t in function_templates {
                    if sig.templates.iter().any(|ct| ct.name == *t) {
                        shadowed.push(format!(
                            "PHPDoc tag {location} template {t} of {description} shadows @template {t} for {owner}."
                        ));
                    }
                }
            }
            if let Some(c) = class {
                for t in &class_templates {
                    if sig.templates.iter().any(|ct| ct.name == *t) {
                        shadowed.push(format!(
                            "PHPDoc tag {location} template {t} of {description} shadows @template {t} for class {c}."
                        ));
                    }
                }
            }
            for message in shadowed {
                self.report(message, span, 2, "callable.shadowTemplate");
            }
        }
    }

    /// `IncompatiblePropertyHookPhpDocTypeRule`: a hook's own `@param`/
    /// `@return` tags checked like a method's, against the hook's native
    /// signature - a `set` hook's `$value` (explicit, or implicitly typed
    /// like the property) returning `void`, a `get` hook returning the
    /// property's type. No hook parameter is by reference.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_hook_phpdoc_types(
        &mut self,
        fqn: &str,
        property_type_hint: Option<&TypeHint>,
        set_param: Option<Option<&Param>>,
        doc: Option<&str>,
        span: Span,
    ) {
        let Some(doc) = doc else {
            return;
        };
        if !doc.contains('@') {
            return;
        }
        let (name, params, return_type) = match set_param {
            Some(param) => {
                let value = match param {
                    Some(p) => p.clone(),
                    None => Param {
                        id: syntax::ast::NodeId::SYNTHETIC,
                        name: "value".into(),
                        name_span: span,
                        type_hint: property_type_hint.cloned(),
                        default: None,
                        by_ref: false,
                        variadic: false,
                        promoted_visibility: None,
                        promoted_set_visibility: None,
                        promoted_readonly: false,
                        promoted_final: false,
                        hooks: None,
                        attrs: Vec::new(),
                        doc_comment: None,
                        span,
                    },
                };
                let void = TypeHint {
                    kind: TypeHintKind::Name(QualifiedName::new("void")),
                    span,
                };
                ("set", vec![value], Some(void))
            }
            None => ("get", Vec::new(), property_type_hint.cloned()),
        };
        let decl = FunctionDecl {
            namespaced_name: None,
            name: name.into(),
            name_span: span,
            params,
            return_type,
            body: std::sync::Arc::new(Vec::new()),
            has_body: false,
            body_open: None,
            by_ref: false,
            doc_comment: Some(doc.into()),
            attrs: Vec::new(),
            span,
        };
        let previous_class = self.current_class.replace(fqn.to_string());
        let class_templates = self.current_class_template_params();
        self.current_class = previous_class;
        let aliases = self
            .registry
            .classes
            .get(fqn)
            .map(|info| info.type_aliases().clone())
            .unwrap_or_default();
        let sig = FunctionReflection::from_decl(
            &decl,
            &self.name_ctx,
            &class_templates,
            &aliases,
            Visibility::Public,
        );
        let sig = self.registry.finalize_standalone_sig(sig);
        self.check_phpdoc_types_against_native(&decl, &sig, Some(fqn));
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
                (&*p.name, p.span, "PHPDoc type", node)
            });
        let declared = c.properties.iter().map(|p| {
            let node = p
                .doc_comment
                .as_deref()
                .and_then(|d| super::phpdoc_unresolvable::tag_type_nodes(d).var);
            (&*p.name, p.span, "PHPDoc tag @var", node)
        });
        let entries: Vec<_> = declared.chain(promoted).collect();
        let mut checks: Vec<(Type, Type, String, Span)> = Vec::new();
        let mut callables: Vec<(Type, Span)> = Vec::new();
        for (name, span, description, node) in entries {
            let subject = format!("{description} for property {fqn}::${name}");
            if node.is_some_and(|n| self.type_node_unresolvable(&n, Some(fqn))) {
                self.report(
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
                .and_then(|info| info.properties().get(name))
            else {
                continue;
            };
            if description == "PHPDoc tag @var"
                && let Some(doc) = prop.doc_ty()
            {
                callables.push((doc.clone(), span));
            }
            if !prop.has_native_type() {
                continue;
            }
            let Some(doc) = prop.doc_ty().clone() else {
                continue;
            };
            checks.push((doc, prop.ty_native().clone(), subject, span));
        }
        for (doc, span) in callables {
            self.check_generic_callables(&doc, "@var", None, Some(fqn), span);
        }
        for (doc, native, subject, span) in checks {
            let doc = self.resolve_pseudo(&doc, fqn);
            let native = self.resolve_pseudo(&native, fqn);
            self.report_phpdoc_native_mismatch(
                &subject,
                &doc,
                &native,
                span,
                "property.phpDocType",
            );
        }
    }

    /// Real PHPStan's `ConditionalReturnTypeRuleHelper`: every
    /// conditional type in a signature (`($x is int ? A : B)`, or a
    /// template subject's `(T is int ? A : B)`) must name a real
    /// parameter or `@template`, and a condition decided by the subject's
    /// own declared type alone is reported as always true or false.
    pub(crate) fn check_conditional_types(
        &mut self,
        f: &FunctionDecl,
        sig: &FunctionReflection,
        class_name: Option<&str>,
    ) {
        // Only this declaration's own PHPDoc: an inherited conditional is
        // checked once, where it's written, with its own templates.
        let mut conditionals: Vec<Type> = Vec::new();
        for p in sig.params() {
            for own in [p.doc_ty(), p.out_ty(), p.closure_this_type()]
                .into_iter()
                .flatten()
            {
                collect_conditionals(own, &mut conditionals);
            }
        }
        if let Some(ret) = sig.return_doc_ty() {
            collect_conditionals(ret, &mut conditionals);
        }
        if conditionals.is_empty() {
            return;
        }
        let templates = self.declared_template_names(f, class_name);
        for c in conditionals {
            let Type::Conditional {
                subject,
                target,
                negated,
                template_subject,
                ..
            } = &c
            else {
                continue;
            };
            let subject_ty = if *template_subject {
                // Its own `@template`s, its class's, or one it inherited
                // along with the tag (`{@inheritDoc}`).
                let inherited = find_template_param(sig, subject);
                if !templates.contains(subject) && inherited.is_none() {
                    let shown = self.resolve_type_hint(Some(subject));
                    self.report(format!(
                            "Conditional return type uses subject type {shown} which is not part of PHPDoc @template tags."
                        ),
                        f.name_span,
                        2,
                        "conditionalType.subjectNotFound",
                    );
                    continue;
                }
                match inherited {
                    Some(tp) => tp,
                    None => continue,
                }
            } else {
                match sig.params().iter().find(|p| **p.name() == **subject) {
                    Some(p) => p.ty().clone(),
                    None => {
                        self.report(
                            format!(
                                "Conditional return type references unknown parameter ${subject}."
                            ),
                            f.name_span,
                            2,
                            "parameter.notFound",
                        );
                        continue;
                    }
                }
            };
            let subject_ty = match class_name {
                Some(c) => self.resolve_pseudo(&subject_ty, c),
                None => subject_ty,
            };
            let holds = match self.registry.is_super_type_of(target, &subject_ty) {
                TrinaryLogic::Maybe => continue,
                TrinaryLogic::Yes => true,
                TrinaryLogic::No => false,
            };
            let always = holds != *negated;
            let word = if *negated { "is not" } else { "is" };
            self.report(format!(
                    "Condition \"{subject_ty} {word} {target}\" in conditional return type is always {always}."
                ),
                f.name_span,
                2,
                if always {
                    "conditionalType.alwaysTrue"
                } else {
                    "conditionalType.alwaysFalse"
                },
            );
        }
    }

    /// Real PHPStan's `IncompatibleSelfOutTypeRule`: a method's
    /// `@phpstan-self-out` type must be its own class (or narrower), the
    /// tag is meaningless on a static method, and the type must resolve.
    pub(crate) fn check_self_out(
        &mut self,
        f: &FunctionDecl,
        sig: &FunctionReflection,
        class: &str,
        is_static: bool,
        label: &str,
    ) {
        let Some(out) = sig.self_out() else {
            return;
        };
        let raw = f
            .doc_comment
            .as_deref()
            .and_then(|d| docblock::parse(d).self_out);
        let unresolvable = raw.is_some_and(|node| self.type_node_unresolvable(&node, Some(class)));
        let class_ty = Type::object(class.into(), Vec::new());
        let resolved = self.resolve_pseudo(out, class);
        // An unresolvable type is reported as such, not also as a
        // mismatch against the class.
        if !unresolvable
            && self.registry.is_super_type_of(&class_ty, &resolved) != TrinaryLogic::Yes
        {
            self.report(
                format!(
                    "Self-out type {resolved} of method {class}::{} is not subtype of {class}.",
                    f.name
                ),
                f.name_span,
                2,
                "selfOut.type",
            );
        }
        if is_static {
            self.report(format!(
                    "PHPDoc tag @phpstan-self-out is not supported above static method {class}::{}().",
                    f.name
                ),
                f.name_span,
                2,
                "selfOut.static",
            );
        }
        if unresolvable {
            self.report(format!(
                    "PHPDoc tag @phpstan-self-out for method {class}::{}() contains unresolvable type.",
                    f.name
                ),
                f.name_span,
                2,
                "selfOut.unresolvableType",
            );
        }
        self.check_generic_bounds(
            &resolved,
            "PHPDoc tag @phpstan-self-out",
            label,
            f.name_span,
        );
    }

    /// `varTag.misplaced`: a `@var` in a declaration's own docblock (a
    /// class, interface, enum, function or method) describes nothing.
    pub(crate) fn check_misplaced_var_tag(
        &mut self,
        doc: Option<&str>,
        description: &str,
        span: Span,
    ) {
        let Some(doc) = doc else {
            return;
        };
        if !doc.contains("var") || super::phpdoc_unresolvable::var_tag_nodes(doc).is_empty() {
            return;
        }
        self.report(
            format!("PHPDoc tag @var above {description} has no effect."),
            span,
            2,
            "varTag.misplaced",
        );
    }

    /// The `@template` names visible in `f`: its own and its class's.
    fn declared_template_names(&self, f: &FunctionDecl, class_name: Option<&str>) -> Vec<String> {
        let mut names: Vec<String> = f
            .doc_comment
            .as_deref()
            .map(|d| docblock::parse(d).templates)
            .unwrap_or_default();
        if let Some(info) = class_name.and_then(|c| self.registry.classes.get(c)) {
            names.extend(info.templates().iter().map(|t| t.to_string()));
        }
        names
    }

    /// The shared "incompatible"/"not subtype" report of every
    /// PHPDoc-vs-native rule, `subject` naming the tag and its target.
    pub(crate) fn report_phpdoc_native_mismatch(
        &mut self,
        subject: &str,
        doc_ty: &Type,
        native: &Type,
        span: Span,
        identifier: &'static str,
    ) {
        let doc_ty = &self.registry.absorb_union_subtypes(doc_ty);
        let verdict = match self.registry.is_super_type_of(native, doc_ty) {
            TrinaryLogic::Yes => return,
            TrinaryLogic::No => "is incompatible with",
            TrinaryLogic::Maybe => "is not subtype of",
        };
        self.report(
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

/// Every `Type::Conditional` inside `ty`, outermost first.
fn collect_conditionals(ty: &Type, out: &mut Vec<Type>) {
    if matches!(ty, Type::Conditional { .. }) {
        out.push(ty.clone());
    }
    let _ = ty.map_children(&mut |child| {
        collect_conditionals(child, out);
        child.clone()
    });
}

/// The `TemplateParam` named `name` somewhere in `sig`, with its bound.
fn find_template_param(sig: &FunctionReflection, name: &str) -> Option<Type> {
    fn find(ty: &Type, name: &str) -> Option<Type> {
        if let Type::TemplateParam(tp) = ty
            && tp.name == name
        {
            return Some(ty.clone());
        }
        let mut found = None;
        let _ = ty.map_children(&mut |child| {
            if found.is_none() {
                found = find(child, name);
            }
            child.clone()
        });
        found
    }
    sig.params()
        .iter()
        .find_map(|p| find(p.ty(), name))
        .or_else(|| find(sig.return_type(), name))
}
