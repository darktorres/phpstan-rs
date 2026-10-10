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
use types::{ConditionalSubject, TrinaryLogic};

impl<'a> Checker<'a> {
    pub(crate) fn check_phpdoc_types_against_native(
        &mut self,
        params: &[Param],
        f: &FunctionDecl,
        sig: &FunctionReflection,
        class_name: Option<&str>,
    ) {
        let source = self.syntax_source_owner();
        let Some(doc) = f
            .doc_comment
            .as_ref()
            .and_then(|ranges| source.as_ref().and_then(|source| ranges.text(source)))
        else {
            return;
        };
        if !doc.contains("@") {
            return;
        }
        let block = docblock::parse_shared(doc.as_ref());
        let nodes = super::phpdoc_unresolvable::tag_type_nodes(doc.as_ref());
        let resolve = |this: &Self, ty: &Type| match class_name {
            Some(c) => this.resolve_pseudo(ty, c),
            None => ty.clone(),
        };
        // Reported in the order the tags are written.
        let in_doc_order = |map: &rustc_hash::FxHashMap<String, phpdoc::DocType>| {
            let mut names: Vec<String> = map.keys().cloned().collect();
            names.sort_by_key(|n| doc.find(&format!("${n}")).unwrap_or(usize::MAX));
            names
        };
        for (tag, names) in [
            ("@param", in_doc_order(&block.params)),
            ("@param-out", in_doc_order(&block.param_out)),
        ] {
            for name in &names {
                let Some(i) = params.iter().position(|p| *p.name == **name) else {
                    report!(
                        self,
                        format!("PHPDoc tag {tag} references unknown parameter: ${name}"),
                        f.name.span,
                        2,
                        "parameter.notFound",
                    );
                    continue;
                };
                let (Some(param), Some(param_sig)) = (params.get(i), sig.params().get(i)) else {
                    continue;
                };
                let node = if tag == "@param-out" {
                    nodes.param_out.get(name.as_str())
                } else {
                    nodes.params.get(name.as_str())
                };
                if node.is_some_and(|n| self.type_node_unresolvable(n, class_name)) {
                    report!(
                        self,
                        format!(
                            "PHPDoc tag {tag} for parameter ${name} contains unresolvable type."
                        ),
                        f.name.span,
                        2,
                        "parameter.unresolvableType",
                    );
                    continue;
                }
                let doc_ty = if tag == "@param-out" {
                    if !param.by_ref {
                        report!(
                            self,
                            format!(
                                "Parameter ${name} for PHPDoc tag {tag} is not passed by reference."
                            ),
                            f.name.span,
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
                    && doc_ty.is_array().yes()
                    && native.is_array().no()
                    && let Some(value) = doc_ty.array_value_type()
                {
                    doc_ty = value;
                }
                if tag == "@param" {
                    self.report_phpdoc_native_mismatch(
                        &format!("PHPDoc tag {tag} for parameter ${name}"),
                        &doc_ty,
                        &native,
                        f.name.span,
                        "parameter.phpDocType",
                    );
                }
                self.check_generic_callables(
                    &doc_ty,
                    &format!("{tag} for parameter ${name}"),
                    Some((f.name.as_ref(), block.templates.as_slice())),
                    class_name,
                    f.name.span,
                );
            }
        }
        if nodes
            .return_type
            .as_ref()
            .is_some_and(|n| self.type_node_unresolvable(n, class_name))
        {
            report!(
                self,
                "PHPDoc tag @return contains unresolvable type.".to_string(),
                f.name.span,
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
                f.name.span,
                "return.phpDocType",
            );
            self.check_generic_callables(
                &doc_ty,
                "@return",
                Some((f.name.as_ref(), block.templates.as_slice())),
                class_name,
                f.name.span,
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
            if let Some(sig) = ty.callable_sig() {
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
        let class_info = class.and_then(|c| self.registry.class_by_name(c));
        let type_aliases = class
            .map(|class| self.registry.effective_type_aliases_by_name(class))
            .unwrap_or_default();
        let class_templates: Vec<String> = class_info
            .map(|info| info.templates().iter().map(|t| t.to_string()).collect())
            .unwrap_or_default();
        for callable in callables {
            let Some(sig) = callable.callable_sig() else {
                continue;
            };
            let description = callable.to_string();
            let names: Vec<Name> = sig
                .templates
                .iter()
                .map(|t| t.name.as_str().into())
                .collect();
            let bounds = sig
                .templates
                .iter()
                .filter(|t| !t.bound.is_mixed())
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
                report!(self, message, span, 2, "callable.shadowTemplate");
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
        let mut arenas = NodeArenas::default();
        let (name, params, return_type) = match set_param {
            Some(param) => {
                let value = match param {
                    Some(p) => {
                        let attributes = self
                            .syntax_arenas()
                            .map(|owner| owner.attributes(p.attrs.clone()).to_vec())
                            .unwrap_or_default();
                        let mut value = p.clone();
                        value.attrs = arenas.alloc_attributes(attributes);
                        value
                    }
                    None => {
                        let attrs = arenas.alloc_attributes(std::iter::empty());
                        Param {
                            name: Ident {
                                name: "value".into(),
                                span,
                                name_id: None,
                            },
                            type_hint: property_type_hint.cloned(),
                            type_aware_override: None,
                            default: None,
                            by_ref: false,
                            variadic: false,
                            promotion: None,
                            hooks: None,
                            attrs,
                            span,
                        }
                    }
                };
                let void = TypeHint {
                    kind: TypeHintKind::Keyword(NativeType::Void, "void".into()),
                    span,
                };
                ("set", vec![value], Some(void))
            }
            None => ("get", Vec::new(), property_type_hint.cloned()),
        };
        let params = arenas.alloc_params(params);
        let attrs = arenas.alloc_attributes(std::iter::empty());
        let decl = FunctionDecl {
            namespaced_name: None,
            name: Ident {
                name: name.into(),
                span,
                name_id: None,
            },
            params: params.clone(),
            return_type,
            body: None,
            by_ref: false,
            doc_comment: Some(syntax::ast::DocCommentRanges::Appended {
                spans: std::sync::Arc::from([]),
                text: std::sync::Arc::from(doc),
            }),
            attrs,
            span,
        };
        let params = arenas.params(params.clone());
        let previous_class = self.current_class.replace(fqn.to_string());
        let class_templates = self.current_class_template_params();
        self.current_class = previous_class;
        let aliases = self.registry.effective_type_aliases_by_name(fqn);
        let sig = FunctionReflection::from_decl(
            &decl,
            &arenas,
            &self.name_ctx,
            &class_templates,
            &aliases,
            Visibility::Public,
        );
        let sig = self.registry.finalize_standalone_sig(sig);
        self.check_phpdoc_types_against_native(params, &decl, &sig, Some(fqn));
    }

    pub(crate) fn check_property_phpdoc_types(&mut self, fqn: &str, c: &ClassDef) {
        if !self.registry.has_class(fqn) {
            return;
        }
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        let source = self.syntax_source_owner();
        // A promoted property's PHPDoc type is its constructor `@param`,
        // else its own inline `@var`.
        let ctor = arenas
            .methods(c.methods.clone())
            .iter()
            .find(|m| m.function.is_constructor());
        let ctor_nodes = ctor
            .and_then(|m| {
                m.function
                    .doc_comment
                    .as_ref()
                    .and_then(|ranges| source.as_ref().and_then(|source| ranges.text(source)))
            })
            .as_deref()
            .map(super::phpdoc_unresolvable::tag_type_nodes)
            .unwrap_or_default();
        let promoted = ctor
            .into_iter()
            .flat_map(|m| arenas.params(m.function.params.clone()).iter())
            .filter(|p| p.promotion.is_some())
            .map(|p| {
                let node = ctor_nodes.params.get(p.name.as_ref()).cloned().or_else(|| {
                    arenas
                        .doc_site(p.span.start)
                        .map(|site| site.text.as_ref())
                        .and_then(|d| super::phpdoc_unresolvable::tag_type_nodes(d).var)
                });
                (&*p.name, p.span, "PHPDoc type", node)
            });
        let declared = c.properties(arenas).iter().map(|p| {
            let node = p
                .doc_comment
                .as_ref()
                .and_then(|ranges| source.as_ref().and_then(|source| ranges.text(source)))
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
                report!(
                    self,
                    format!("{subject} contains unresolvable type."),
                    span,
                    2,
                    "property.unresolvableType",
                );
                continue;
            }
            let Some(prop) = self.registry.lookup_property(fqn, name) else {
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
            let Some((subject, target, negated)) = c.as_conditional() else {
                continue;
            };
            let subject_ty = match subject {
                ConditionalSubject::Template(subject) => {
                    // Its own `@template`s, its class's, or one it inherited
                    // along with the tag (`{@inheritDoc}`).
                    let inherited = find_template_param(sig, subject);
                    if !templates.iter().any(|t| t == subject) && inherited.is_none() {
                        let shown = self.resolve_type_node(
                            &phpdoc::grammar::TypeNode::Identifier(subject.to_string()),
                            "conditional return subject",
                        );
                        report!(
                            self,
                            format!(
                                "Conditional return type uses subject type {shown} which is not part of PHPDoc @template tags."
                            ),
                            f.name.span,
                            2,
                            "conditionalType.subjectNotFound",
                        );
                        continue;
                    }
                    match inherited {
                        Some(tp) => tp,
                        None => continue,
                    }
                }
                ConditionalSubject::Parameter(subject) => {
                    match sig.params().iter().find(|p| p.name() == subject) {
                        Some(p) => p.ty().clone(),
                        None => {
                            report!(
                                self,
                                format!(
                                    "Conditional return type references unknown parameter ${subject}."
                                ),
                                f.name.span,
                                2,
                                "parameter.notFound",
                            );
                            continue;
                        }
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
            let always = holds != negated;
            let word = if negated { "is not" } else { "is" };
            report!(
                self,
                format!(
                    "Condition \"{subject_ty} {word} {target}\" in conditional return type is always {always}."
                ),
                f.name.span,
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
            .as_ref()
            .and_then(|ranges| self.syntax_doc_comment_text(ranges))
            .as_deref()
            .and_then(|d| docblock::parse_shared(d).self_out.clone());
        let unresolvable = raw.is_some_and(|node| self.type_node_unresolvable(&node, Some(class)));
        let class_ty = Type::object(class.into(), Vec::new());
        let resolved = self.resolve_pseudo(out, class);
        // An unresolvable type is reported as such, not also as a
        // mismatch against the class.
        if !unresolvable
            && self.registry.is_super_type_of(&class_ty, &resolved) != TrinaryLogic::Yes
        {
            report!(
                self,
                format!(
                    "Self-out type {resolved} of method {class}::{} is not subtype of {class}.",
                    f.name
                ),
                f.name.span,
                2,
                "selfOut.type",
            );
        }
        if is_static {
            report!(
                self,
                format!(
                    "PHPDoc tag @phpstan-self-out is not supported above static method {class}::{}().",
                    f.name
                ),
                f.name.span,
                2,
                "selfOut.static",
            );
        }
        if unresolvable {
            report!(
                self,
                format!(
                    "PHPDoc tag @phpstan-self-out for method {class}::{}() contains unresolvable type.",
                    f.name
                ),
                f.name.span,
                2,
                "selfOut.unresolvableType",
            );
        }
        self.check_generic_bounds(
            &resolved,
            "PHPDoc tag @phpstan-self-out",
            label,
            f.name.span,
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
        report!(
            self,
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
            .as_ref()
            .and_then(|ranges| self.syntax_doc_comment_text(ranges))
            .as_deref()
            .map(|d| docblock::parse_shared(d).templates.clone())
            .unwrap_or_default();
        if let Some(info) = class_name.and_then(|c| self.registry.class_by_name(c)) {
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
        report!(
            self,
            format!("{subject} with type {doc_ty} {verdict} native type {native}."),
            span,
            2,
            identifier,
        );
    }
}

/// Every conditional type inside `ty`, outermost first.
fn collect_conditionals(ty: &Type, out: &mut Vec<Type>) {
    syntax::stack::guard(|| {
        if ty.as_conditional().is_some() {
            out.push(ty.clone());
        }
        let _ = ty.map_children(&mut |child| {
            collect_conditionals(child, out);
            child.clone()
        });
    })
}

/// The `TemplateParam` named `name` somewhere in `sig`, with its bound.
fn find_template_param(sig: &FunctionReflection, name: &str) -> Option<Type> {
    sig.params()
        .iter()
        .map(|p| p.ty())
        .chain(std::iter::once(sig.return_type()))
        .find_map(|ty| ty.nested_templates().into_iter().find(|tp| tp.name == name))
        .map(|tp| Type::template_param(tp.clone()))
}
