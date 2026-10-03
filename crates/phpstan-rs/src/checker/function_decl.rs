use crate::checker::GotoState;
use crate::checker::{
    ATTRIBUTE_TARGET_FUNCTION, ATTRIBUTE_TARGET_METHOD, ATTRIBUTE_TARGET_PARAMETER,
    ATTRIBUTE_TARGET_PROPERTY, Checker, Locals, body_contains_explicit_throw,
    generator_missing_return_type, is_constant_condition_family_identifier,
    type_allows_generator_return,
};
use crate::registry::{
    self, ClassReflection, FunctionReflection, mark_template_params, resolve_names_in_type,
};
use diagnostics::DiagnosticBuilder;
use phpdoc::docblock;
use syntax::ast::*;
use syntax::name::NameMap;
use syntax::span::Span;
use types::Name;
use types::php_version::PhpFeature;
use types::{self, Type};

impl<'a> Checker<'a> {
    /// A trait's method bodies are checked once per class that `use`s
    /// it, not standalone - `flatten_traits` (`registry.rs`) already
    /// merges a trait's methods/properties into every using class's
    /// `ClassReflection` before checking starts, so re-checking with
    /// `current_class` set to a real host's FQN means every lookup
    /// (`lookup_method`/`lookup_property`) transparently sees that
    /// host's true member set - a trait method calling
    /// `$this->hostOnlyMethod()` is valid PHP (the trait's author
    /// expects any using class to provide it), but would false-positive
    /// as "undefined method" when checked standalone. A trait with no
    /// user anywhere in the project still gets checked standalone.
    pub(crate) fn check_trait_methods(&mut self, fqn: &str, c: &ClassDecl) {
        let hosts = self.registry.classes_using_trait(fqn);
        let has_hosts = !hosts.is_empty();
        let contexts: Vec<Name> = if has_hosts {
            hosts
        } else {
            vec![Name::from(fqn)]
        };
        let previous_trait = self.current_trait.take();
        self.current_trait = Some(fqn.to_string());
        let start = self.diagnostics.len();
        // This exact literal declaration's own signature table - not
        // `self.registry.classes.get(fqn)`, which is whichever
        // conditional declaration of `fqn` won the registry's single
        // slot (first-wins, no tiebreak for classes/traits) and may be a
        // different branch's own signatures when a trait is declared
        // twice under a version guard with genuinely different method
        // shapes. Reproduces exactly what the registry would hold for
        // `c` had it won the slot - `ClassReflection::from_decl` is a pure
        // function of `(c, ctx)`, and no later pass ever mutates a
        // trait's registry entry after indexing, so this is a no-op
        // reconstruction for the common non-duplicated-trait case.
        let own_info = ClassReflection::from_decl(c, &self.name_ctx);
        let host_count = contexts.len();
        // A trait's private properties are written in its hosts' scope, or
        // in its own when it has none.
        self.record_property_type_site(fqn, contexts.clone(), &c.def);
        let mut family_verdicts_by_span: rustc_hash::FxHashMap<
            syntax::span::Range,
            (rustc_hash::FxHashSet<usize>, rustc_hash::FxHashSet<bool>),
        > = rustc_hash::FxHashMap::default();
        for (host_index, host) in contexts.into_iter().enumerate() {
            let host_start = self.diagnostics.len();
            for m in &c.def.methods {
                let site = m.function.name.span.start;
                let own_sig = own_info
                    .redeclared_methods()
                    .get(&site)
                    .or_else(|| own_info.methods().get(m.function.name.as_ref()));
                self.check_function_with_own_sig(
                    &m.function,
                    Some(&host),
                    m.is_static,
                    !m.is_abstract,
                    own_sig,
                );
            }
            for d in &self.diagnostics[host_start..] {
                if is_constant_condition_family_identifier(&d.identifier) {
                    let entry = family_verdicts_by_span.entry(d.span).or_default();
                    entry.0.insert(host_index);
                    entry
                        .1
                        .insert(d.identifier.to_ascii_lowercase().ends_with("true"));
                }
            }
        }
        self.current_trait = previous_trait;
        // A trait used by 2+ classes gets checked 2+ times - a
        // genuinely host-independent bug in the trait's body (no
        // `$this->...` involved) would otherwise be reported once per
        // host, since diagnostics are only sorted, never deduplicated
        // elsewhere. A real host-specific finding (missing on one host
        // but not another) survives this, since its message genuinely
        // differs per host. This local sort exists only to bring
        // duplicates adjacent for `dedup_by` (which only removes
        // consecutive matches). Only this trait's own diagnostics are
        // sorted and deduplicated; what precedes `start` is left as is.
        let mut own = self.diagnostics.split_off(start);
        own.sort_by(|a, b| (a.span.start, &a.message).cmp(&(b.span.start, &b.message)));
        own.dedup_by(|a, b| a.span == b.span && a.message == b.message);
        // Real PHPStan's own `ConstantConditionInTraitRule` (`checker/
        // control_flow/boolean_conditions.rs`'s own doc comment on why a
        // bare call condition is folded into the ordinary if/while/
        // ternary/&&/|| rules rather than getting a separate one) - a
        // constant-truth finding (`if.alwaysTrue`, `instanceof.alwaysFalse`,
        // ...) in a trait used by several hosts is kept only when every
        // host reports it with the same verdict. One host proving it
        // true while another proves it false, or while another can't
        // decide it at all, suppresses it for every host.
        if has_hosts {
            own.retain(|d| {
                !is_constant_condition_family_identifier(&d.identifier)
                    || family_verdicts_by_span
                        .get(&d.span)
                        .is_some_and(|(hosts, verdicts)| {
                            hosts.len() == host_count && verdicts.len() == 1
                        })
            });
        }
        self.diagnostics.extend(own);
    }

    /// Looks up this exact declaration's resolved signature in the
    /// registry (a free function by name, or a method by class+name).
    /// Returns a reference tied to the registry's lifetime, not `&self`'s,
    /// so holding it doesn't conflict with the `&mut self` calls that
    /// follow once its params/return type have been copied into `locals`.
    /// `Registry::index` keys `self.registry.functions` by the
    /// function's fully-qualified name, so a bare
    /// `self.registry.functions.get(&f.name)` here would look it up by
    /// its raw, unqualified AST name and never find a namespaced
    /// function's real signature. Methods don't share this issue:
    /// `class_name` is already the FQN by the time it gets here, and a
    /// method's name is never namespace-qualified in the first place -
    /// only the class it's declared in is.
    pub(crate) fn lookup_own_signature(
        &self,
        f: &FunctionDecl,
        class_name: Option<&str>,
    ) -> Option<&'a FunctionReflection> {
        match class_name {
            Some(c) => self.registry.classes.get(c)?.methods().get(f.name.as_ref()),
            None => self.registry.functions.get(f.fq_name()),
        }
    }

    /// Mirrors real PHPStan's `YieldInGeneratorRule`, narrowed to its
    /// declared-return-type-compatibility half (the full `Generator<K, V,
    /// TSend, TReturn>` generic shape - checking each `yield`'s key/value
    /// against `K`/`V`, a sent-in value's type against `TSend`, and a
    /// `return` statement inside a generator against `TReturn` - is left
    /// for later). Real PHP itself rejects `function f(): int { yield 1; }`
    /// at compile time with "Generator return type must be a supertype
    /// of Generator, int given," but is lenient about which return type
    /// shape satisfies this (`?Generator`, `iterable`, even
    /// `int|Generator` are all accepted, since only one member of a
    /// union needs to be Generator-compatible) - `type_allows_generator_
    /// return` mirrors that exactly, not PHPStan's stricter,
    /// `reportMaybes`-gated static heuristic.
    pub(crate) fn check_generator_return_type(
        &mut self,
        declared_return: Option<&Type>,
        span: Span,
    ) {
        if !self.current_function_is_generator {
            return;
        }
        let Some(declared) = declared_return else {
            return;
        };
        let resolved = match &self.current_class {
            Some(c) => self.resolve_pseudo(declared, c),
            None => declared.clone(),
        };
        if type_allows_generator_return(&resolved, self.registry) {
            return;
        }
        self.report(format!(
                "yields, but its declared return type {resolved} is not a supertype of Generator, Iterator, Traversable, or iterable"
            ),
            span,
            0,
            "generator.returnType",
        );
    }

    pub(crate) fn check_function(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        is_static: bool,
        is_concrete: bool,
    ) {
        // The registry keeps one signature per top-level function name, and
        // the union of a class body's declarations of one method name; a
        // declaration whose slot isn't its own is checked against its own.
        let own = match class_name {
            None => self.own_declaration_sig(f),
            Some(class) => self.own_redeclared_method_sig(f, class),
        };
        self.check_function_impl(f, class_name, is_static, is_concrete, own.as_ref());
    }

    /// `f`'s own signature when the registry's slot for its name was built
    /// from something else - another declaration, several merged, or a
    /// built-in stub; `None` when the slot is `f`'s.
    fn own_declaration_sig(&self, f: &FunctionDecl) -> Option<FunctionReflection> {
        let site = f.name.span.start;
        let slot_site = self
            .registry
            .function_decl_sites
            .get(crate::registry::function_key(f.fq_name()).as_ref());
        let slot_file = self
            .registry
            .functions
            .get(f.fq_name())
            .and_then(|slot| slot.provenance().file());
        let same_file = match (self.current_file.as_deref(), slot_file) {
            (Some(current), Some(slot)) => current == slot,
            _ => true,
        };
        if slot_site == Some(&Some(site)) && same_file {
            return None;
        }
        let fresh = FunctionReflection::from_decl(
            f,
            &self.name_ctx,
            &rustc_hash::FxHashMap::default(),
            &NameMap::default(),
            syntax::ast::Visibility::Public,
        );
        Some(self.registry.finalize_standalone_sig(fresh))
    }

    /// `f`'s own signature when `class` declares its name more than once.
    fn own_redeclared_method_sig(
        &self,
        f: &FunctionDecl,
        class: &str,
    ) -> Option<FunctionReflection> {
        let site = f.name.span.start;
        let sig = self
            .registry
            .classes
            .get(class)?
            .redeclared_methods()
            .get(&site)?;
        Some(self.registry.finalize_standalone_sig(sig.clone()))
    }

    /// For a trait method body whose own literal declaration (not the
    /// registry's single winning slot for this trait's name - see
    /// `check_trait_methods`'s doc comment) is already known. `own_sig`
    /// is a full substitute for the ordinary registry-derived `sig`, not
    /// a hint - a trait's own literal declaration genuinely is this
    /// method's real signature.
    pub(crate) fn check_function_with_own_sig(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        is_static: bool,
        is_concrete: bool,
        own_sig: Option<&FunctionReflection>,
    ) {
        self.check_function_impl(f, class_name, is_static, is_concrete, own_sig);
    }

    /// The real body shared by `check_function` and
    /// `check_function_with_own_sig`; `own_sig_override`, when `Some`,
    /// replaces the ordinary registry lookup for `sig` entirely.
    fn check_function_impl(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        is_static: bool,
        is_concrete: bool,
        own_sig_override: Option<&FunctionReflection>,
    ) {
        let sig = own_sig_override.or_else(|| self.lookup_own_signature(f, class_name));
        let label = match class_name {
            Some(c) => format!("{c}::{}", f.name),
            None => f.name.to_string(),
        };
        // A method's docblock is already covered by `check_unknown_
        // phpstan_tags_in_class`'s per-class walk (`checker.rs`'s
        // class-declaration loop) - only a genuine top-level function
        // needs it here, or every method would get double-checked.
        if class_name.is_none() {
            self.check_unknown_phpstan_tags_on(f.doc_comment.as_deref(), f.name.span);
        }
        if self.settings.sidz.check_magic_numbers {
            self.check_magic_numbers_in_params(&f.params);
        }
        self.check_declared_purity_tag(f, sig, class_name.is_some(), &label);
        self.check_declared_param_callable_tags(f);
        self.check_internal_param_and_return_types(f, class_name, &label);
        self.check_restricted_param_and_return_types(f, class_name);
        self.check_assert_tag_types(sig, f.span);
        // `method.templateTypeNotInParameter` - neither a closure nor an
        // arrow-fn is indexed into the registry at all (see
        // `check_template_type_not_in_parameter`'s doc comment), so this
        // only ever runs for a real, named function/method.
        if let Some(sig) = sig {
            self.check_template_type_not_in_parameter(sig, &f.name, class_name, &label, f.span);
        }
        // Mirrors real PHPStan's `FunctionDefinitionCheck`'s PHP-
        // version-gated native declaration-syntax checks - see each
        // one's doc comment. Real PHPStan runs these against a closure/
        // arrow-fn's params/return too (`checkAnonymousFunction`); scoped
        // here to a named function/method only, a deliberate narrower
        // cut given this family's low expected value against a project
        // targeting a modern PHP version.
        self.check_native_union_types_supported(f, class_name.is_some(), &label);
        self.check_unresolvable_native_types(f, &label);
        self.check_implicitly_nullable_params(f);
        self.check_required_parameter_after_optional(f);
        if let Some(sig) = sig {
            let previous_class =
                std::mem::replace(&mut self.current_class, class_name.map(str::to_string));
            self.check_default_parameter_types(f, sig, class_name);
            self.check_phpdoc_types_against_native(f, sig, class_name);
            self.check_conditional_types(f, sig, class_name);
            let description = if class_name.is_some() {
                "a method"
            } else {
                "a function"
            };
            self.check_misplaced_var_tag(f.doc_comment.as_deref(), description, f.name.span);
            if let Some(class) = class_name {
                self.check_self_out(f, sig, class, is_static, &label);
            }
            self.current_class = previous_class;
        }
        // Mirrors real PHPStan's `TemplateTypeCheck`'s declaration-
        // validity family plus `method.shadowTemplate`, method-level -
        // see `check_class_generic_bounds`'s identical class-level call
        // for the shared-check doc comment. Re-parses `f.doc_comment`
        // fresh (mirroring `registry::function_sig::generics::
        // build_template_params`'s shallow-then-resolve pipeline exactly,
        // so a bound/default referencing a sibling template on the same
        // docblock - `@template A` `@template B of A` - classifies
        // correctly instead of misreading `A` as an unknown class) since
        // `FunctionReflection` has no persisted raw template-bound/-default map
        // of its own. Cheap substring pre-filter before the real
        // docblock::parse_shared below - most documented methods declare no
        // `@template` at all, so this skips a real parse cost on every
        // one. Matched on the bare `template` substring rather than
        // any one spelling: `docblock::parse_shared` treats `@phpstan-`/
        // `@psalm-`/`@phan-template*` identically to `@template*`, and
        // a filter checking only some spellings would silently stop
        // catching the rest.
        let has_own_template = f
            .doc_comment
            .as_deref()
            .is_some_and(|d| d.contains("template"));
        if has_own_template
            && let Some(doc) = f.doc_comment.as_deref().map(docblock::parse_shared)
            && !doc.templates.is_empty()
        {
            let shallow: rustc_hash::FxHashMap<String, types::TemplateParam> = doc
                .templates
                .iter()
                .map(|n| (n.clone(), types::TemplateParam::unbounded(n.clone())))
                .collect();
            let resolve = |node: &phpdoc::DocType| -> Type {
                let ty = crate::registry::parse_type_node(node);
                let ty = mark_template_params(ty, &shallow);
                resolve_names_in_type(ty, &self.name_ctx)
            };
            let templates: Vec<Name> = doc.templates.iter().map(Name::from).collect();
            let bounds: syntax::name::NameMap<Type> = doc
                .template_bounds
                .iter()
                .map(|(k, v)| (Name::from(k), resolve(v)))
                .collect();
            let defaults: syntax::name::NameMap<Type> = doc
                .template_defaults
                .iter()
                .map(|(k, v)| (Name::from(k), resolve(v)))
                .collect();
            let type_aliases = class_name
                .and_then(|c| self.registry.classes.get(c))
                .map(|info| info.type_aliases().clone())
                .unwrap_or_default();
            let display_name = match class_name {
                Some(c) => format!("method {c}::{}()", f.name),
                None => format!("function {}()", f.fq_name()),
            };
            self.check_template_declaration_validity(
                &templates,
                &bounds,
                &defaults,
                &type_aliases,
                crate::checker::generics::TemplateSubject::Declared {
                    display_name: &display_name,
                },
                f.span,
            );
            // `method.shadowTemplate`: a method-level `@template` reusing
            // a name already declared on the enclosing class.
            if let Some(c) = class_name
                && let Some(info) = self.registry.classes.get(c)
            {
                let mut shadowed: Vec<&String> = doc
                    .templates
                    .iter()
                    .filter(|n| info.templates().iter().any(|t| t.as_ref() == n.as_str()))
                    .collect();
                shadowed.sort();
                for name in shadowed {
                    self.report(format!(
                        "PHPDoc tag @template {name} for method {c}::{}() shadows @template {name} for class {c}.",
                        f.name
                    ),
                    f.span,
                    6,
                    "method.shadowTemplate",
                );
                }
            }
        }

        // `(name, resolved type, AST span)` per parameter, collected
        // below in lockstep with `locals`'s per-branch loops - but before
        // either one's variadic-collects-into-`array` rewrite, since
        // `check_missing_function_typehints` wants the per-argument type
        // a genuinely untyped `...$args` would have (implicit `mixed`),
        // not the already-array-wrapped type `locals` needs for checking
        // the function's body.
        let mut resolved_params: Vec<(String, Type, Span)> = Vec::with_capacity(f.params.len());

        let mut locals: Locals = self.new_locals();
        // Turns declaration-span tracking on for this whole function-
        // body check exactly when the LSP's probe is active - see
        // `Locals::track`'s doc comment. Every clone of `locals` from
        // here on carries this forward automatically.
        locals.set_track(self.probe_span.get().is_some());
        // A PHPDoc type that can't be resolved is ignored: its position has
        // the native type (its tag is reported as unresolvable).
        let doc_nodes = f
            .doc_comment
            .as_deref()
            .filter(|doc| doc.contains('@'))
            .map(super::phpdoc_unresolvable::tag_type_nodes);
        let doc_unresolvable = |this: &Self, node: Option<&phpdoc::grammar::TypeNode>| {
            node.is_some_and(|n| this.type_node_unresolvable(n, class_name))
        };
        let return_doc_unresolvable = doc_unresolvable(
            self,
            doc_nodes.as_ref().and_then(|n| n.return_type.as_ref()),
        );
        match sig {
            // The registry's FunctionReflection already has PHPDoc types merged
            // in - using it here, instead of re-deriving from the raw
            // AST's native hints, is what makes an untyped-but-doc-
            // commented parameter usable inside its own function body,
            // not just checkable from the outside at call sites.
            Some(sig) => {
                for (param, ast_param) in sig.params().iter().zip(&f.params) {
                    // `self`/`static`/`parent` need resolving against the
                    // enclosing class the same way `$this` and a return
                    // type already are - without this, a `self`-typed
                    // parameter (`function equals(self $other): bool`)
                    // stays an unresolved pseudo-type for the whole
                    // function body, and infer_expr's PropertyAccess/
                    // MethodCall arms only match a concrete `Object`, so
                    // every property/method access through it silently
                    // fell through to `Mixed` - no undefined-property/
                    // method checking at all, unlike the identical access
                    // through `$this`.
                    let declared = if doc_unresolvable(
                        self,
                        doc_nodes
                            .as_ref()
                            .and_then(|n| n.params.get(param.name().as_ref())),
                    ) {
                        param.ty_native()
                    } else {
                        param.ty()
                    };
                    let ty = match class_name {
                        Some(c) => self.resolve_pseudo(declared, c),
                        None => declared.clone(),
                    };
                    // Mirrors real PHPStan's `ExistingClassesInTypehintsRule` -
                    // on the un-array-wrapped element type below (a
                    // variadic parameter's declared type, not the
                    // `array` it collects into), the same reasoning the
                    // Closure/arrow-fn arms in `checker/expr.rs` share.
                    let subject = ParameterSubject {
                        name: param.name(),
                        label: &label,
                    };
                    // The own PHPDoc type too: a member the native hint
                    // excludes no longer reaches `ty`, but naming an
                    // unknown class is still wrong.
                    let checked = match &param.doc_ty() {
                        Some(doc) => self.relation().union_of(vec![ty.clone(), doc.clone()]),
                        None => ty.clone(),
                    };
                    self.check_type_class_names_exist(&checked, subject, ast_param.span);
                    self.check_type_trait_refs(&ty, subject, ast_param.span, "parameter.trait");
                    // A native `parent` without a parent class; one written in
                    // PHPDoc is its unresolvable-type check's.
                    let native = match class_name {
                        Some(c) => self.resolve_pseudo(param.ty_native(), c),
                        None => param.ty_native().clone(),
                    };
                    self.check_no_parent_ref(
                        &native,
                        subject,
                        ast_param.span,
                        "parameter.noParent",
                    );
                    self.check_type_class_name_case(param.case_refs(), ast_param.span);
                    self.check_type_internal_usage(param.case_refs(), ast_param.span);
                    resolved_params.push((param.name().to_string(), ty.clone(), ast_param.span));
                    // `ParameterReflection::ty` holds a variadic parameter's own
                    // *element* type (the right shape for checking each
                    // individual argument at a call site) - inside the
                    // function's own body, real PHP always collects a
                    // variadic parameter into an `array` from the first
                    // statement onward, never the bare element type.
                    let ty = if param.variadic() {
                        Type::array(Type::unknown_array_key(), ty)
                    } else {
                        ty
                    };
                    // Inside the generic code itself a `key-of<T>`/`T[K]`
                    // is what `T`'s bound makes it.
                    let ty = if ty.contains_late() {
                        ty.resolve_late_to_bounds()
                    } else {
                        ty
                    };
                    self.probe_hit_at(ast_param.name.span, &ty);
                    locals.declare_var(param.name(), ty, ast_param.name.span);
                    let native = match class_name {
                        Some(c) => self.resolve_pseudo(param.ty_native(), c),
                        None => param.ty_native().clone(),
                    };
                    let native = if param.variadic() {
                        Type::array(Type::unknown_array_key(), native)
                    } else {
                        native
                    };
                    locals.set_native_var(param.name(), native);
                }
            }
            // A declaration the registry has no signature for. Unlike the
            // `Some(sig)` branch above, this one resolves each declared
            // type itself, against `self.name_ctx`, the same way
            // `FunctionReflection::from_decl` does, and resolves a
            // `self`/`static`/`parent` pseudo-type against the enclosing
            // class.
            None => {
                let doc = f.doc_comment.as_deref().map(docblock::parse_shared);
                for p in &f.params {
                    // Same inline-`@var`-on-a-promoted-property fallback
                    // as `FunctionReflection::from_decl`'s param loop.
                    let doc_ty: Option<phpdoc::DocType> = doc
                        .as_ref()
                        .and_then(|d| d.params.get(p.name.as_ref()))
                        .cloned()
                        .or_else(|| {
                            p.doc_comment
                                .as_deref()
                                .map(docblock::parse_shared)
                                .and_then(|d| d.var_type.clone())
                        });
                    // This path bypasses `FunctionReflection::from_decl`,
                    // so it collects the param's case refs itself, and resolves a `value-of<T>`
                    // on an enum as the registry's pipeline would.
                    let registry::DeclaredType {
                        ty,
                        native: native_ty,
                        case_refs,
                    } = registry::declared_type(
                        doc_ty.as_ref(),
                        p.type_hint.as_ref(),
                        &Default::default(),
                        &self.name_ctx,
                    );
                    let ty = self
                        .registry
                        .resolve_deferred_value_of_now_with_native_fallback(ty, &native_ty);
                    let ty = match class_name {
                        Some(c) => self.resolve_pseudo(&ty, c),
                        None => ty,
                    };
                    let subject = ParameterSubject {
                        name: &p.name,
                        label: &label,
                    };
                    self.check_type_class_names_exist(&ty, subject, p.span);
                    self.check_type_trait_refs(&ty, subject, p.span, "parameter.trait");
                    self.check_no_parent_ref(&ty, subject, p.span, "parameter.noParent");
                    self.check_type_class_name_case(&case_refs, p.span);
                    self.check_type_internal_usage(&case_refs, p.span);
                    resolved_params.push((p.name.to_string(), ty.clone(), p.span));
                    let ty = if p.variadic {
                        Type::array(Type::unknown_array_key(), ty)
                    } else {
                        ty
                    };
                    self.probe_hit_at(p.name.span, &ty);
                    locals.declare_var(&p.name, ty, p.name.span);
                }
            }
        }
        // A static method has no $this at all in real PHP (calling one
        // outside object context is a fatal error) - only bind it for a
        // genuinely non-static method, so a static method's own body
        // referencing $this correctly reads as an undefined variable
        // rather than silently resolving to the enclosing class.
        if let Some(c) = class_name
            && !is_static
        {
            // Its own templates stay templates inside the class.
            let own = self
                .registry
                .classes
                .get(c)
                .map(|info| info.own_template_args(c))
                .unwrap_or_default();
            locals.insert_var("this", Type::object(c.into(), own));
        }

        // A signature's return type already defaults to Mixed when nothing
        // was declared, and checking a return statement against Mixed
        // never flags anything - so this doesn't need to distinguish "no
        // return type" from "declared mixed" the way the fallback below
        // does with an Option.
        // Captured (moved into) by the `.or_else` closure below when
        // `sig` is `None`. Stays empty (and unused) when `sig` is
        // `Some`, since the closure never runs in that case.
        let mut declared_return_case_refs_none_path: Vec<(Name, Name)> = Vec::new();
        let declared_return = sig
            .map(|s| {
                if return_doc_unresolvable {
                    s.return_type_native().clone()
                } else {
                    s.return_type().clone()
                }
            })
            .or_else(|| {
                // The return-position sibling of the param loop above.
                let doc_return = f
                    .doc_comment
                    .as_deref()
                    .map(docblock::parse_shared)
                    .and_then(|d| d.return_type.clone());
                // The return-position sibling of the param loop's identical
                // fix above.
                let registry::DeclaredType {
                    ty,
                    native: native_ty,
                    case_refs,
                } = registry::declared_type(
                    doc_return.as_ref(),
                    f.return_type.as_ref(),
                    &Default::default(),
                    &self.name_ctx,
                );
                declared_return_case_refs_none_path = case_refs;
                let ty = self
                    .registry
                    .resolve_deferred_value_of_now_with_native_fallback(ty, &native_ty);
                Some(match class_name {
                    Some(c) => self.resolve_pseudo(&ty, c),
                    None => ty,
                })
            });
        // The native-only counterpart of `declared_return` above -
        // `check_generator_return_type` needs it specifically, not the
        // PHPDoc-refined type `declared_return` carries everywhere else
        // in this function. Real PHP's "Generator return type must be a
        // supertype of Generator" fatal error is checked against the
        // native declaration alone; a `@return Transformation[]`
        // docblock on a `: iterable`-typed generator is describing what
        // iterating the real Generator/iterable produces, not
        // redeclaring the native type - using `declared_return` here
        // would produce false positives on every docblock-typed-as-array
        // generator.
        let native_return = sig.map(|s| s.return_type_native().clone()).or_else(|| {
            let ty = f.return_type.as_ref().map(types::native_type)?;
            Some(match class_name {
                Some(c) => self.resolve_pseudo(&ty, c),
                None => ty,
            })
        });
        // Whichever path actually produced `declared_return` above -
        // `sig`'s own stored field when present, otherwise whatever the
        // `.or_else` closure collected inline into
        // `declared_return_case_refs_none_path` (empty and unused when
        // `sig` was `Some`, since that closure never ran).
        let declared_return_case_refs = sig
            .map(|s| s.return_case_refs().clone())
            .unwrap_or(declared_return_case_refs_none_path);
        // Mirrors real PHPStan's `ExistingClassesInTypehintsRule`'s
        // return-type check - the un-widened, exactly-as-declared type
        // (not the `?`-and-`resolve_pseudo`-adjusted `expected` the
        // falls-off-the-end check further down builds), same as every
        // other existence check in this function.
        let subject = ReturnSubject { label: &label };
        let declared = declared_return.clone().unwrap_or(Type::mixed());
        let checked = match sig.and_then(|s| s.return_doc_ty().as_ref()) {
            Some(doc) => self.relation().union_of(vec![declared, doc.clone()]),
            None => declared,
        };
        self.check_type_class_names_exist(&checked, subject, f.span);
        self.check_type_class_name_case(&declared_return_case_refs, f.span);
        self.check_type_internal_usage(&declared_return_case_refs, f.span);
        self.check_type_trait_refs(
            declared_return.as_ref().unwrap_or(&Type::mixed()),
            subject,
            f.span,
            "return.trait",
        );
        // `return.noParent` - `declared_return` above is deliberately
        // not `resolve_pseudo`'d yet, so a fresh, one-off resolve here
        // is needed specifically to detect "this class has no real
        // parent" - a top-level function has no enclosing class `parent`
        // could meaningfully reference.
        if let Some(c) = class_name {
            let native_return = match sig {
                Some(s) => s.return_type_native().clone(),
                None => declared_return.clone().unwrap_or(Type::mixed()),
            };
            let resolved_return = self.resolve_pseudo(&native_return, c);
            self.check_no_parent_ref(&resolved_return, subject, f.span, "return.noParent");
        }
        // `@throws` tag validity - see `checker::throws`'s module doc
        // comment.
        if let Some(sig) = sig
            && let Some(throws) = sig.throws()
        {
            self.check_throws_tag_validity(throws, f.span);
            // Mirrors real PHPStan's
            // `ThrowsVoidFunctionWithExplicitThrowPointRule`/
            // `ThrowsVoidMethodWithExplicitThrowPointRule` - needs no
            // opt-in flag: only ever fires when the docblock itself
            // explicitly claims "never throws," directly contradicted by
            // a real `throw` sitting right there in the body.
            if throws.is_void()
                && let Some(throw_span) = body_contains_explicit_throw(f.body_stmts())
            {
                let kind_word = if class_name.is_some() {
                    "Method"
                } else {
                    "Function"
                };
                self.report(format!(
                        "{kind_word} {label}() throws exception but the PHPDoc contains @throws void."
                    ),
                    throw_span,
                    3,
                    "throws.void",
                );
            }
        }

        // `check_function` is the single choke point every function-
        // shaped declaration passes through regardless of how it's
        // reached (a top-level function, an ordinary method, a trait
        // method re-checked once per host, a declaration nested inside
        // a block) - checking `f.attrs`/each parameter's attrs here
        // covers all of them at once.
        self.check_attributes(&f.attrs, &label);
        // Target-validity, mirroring real PHPStan's split between
        // `MethodAttributesRule`/`FunctionAttributesRule` (one or the
        // other, never both, for this same choke point).
        if class_name.is_some() {
            self.check_attribute_targets(&f.attrs, ATTRIBUTE_TARGET_METHOD, "method");
        } else {
            self.check_attribute_targets(&f.attrs, ATTRIBUTE_TARGET_FUNCTION, "function");
        }
        for p in &f.params {
            self.check_attributes(&p.attrs, &label);
            // `ParamAttributesRule`: a promoted param's own attributes
            // may target either a parameter or the property it also
            // declares - real PHP itself doesn't distinguish which one
            // an attribute "means" here, so either target is accepted.
            let (target, target_name) = if p.promotion.is_some() {
                (
                    ATTRIBUTE_TARGET_PARAMETER | ATTRIBUTE_TARGET_PROPERTY,
                    "parameter or property",
                )
            } else {
                (ATTRIBUTE_TARGET_PARAMETER, "parameter")
            };
            self.check_attribute_targets(&p.attrs, target, target_name);
        }

        // Bound-enforcement: checked once here, against the declaration
        // itself, not at every call site - and deliberately outside the
        // dynamic-var-construct truncation below, since a bound
        // violation is about the signature as written, not something an
        // `extract()` in the body should be able to hide.
        if let Some(sig) = sig {
            for (param, ast_param) in sig.params().iter().zip(&f.params) {
                self.check_generic_bounds(
                    param.ty(),
                    &format!("PHPDoc tag @param for parameter ${}", param.name()),
                    &label,
                    ast_param.span,
                );
            }
        }
        self.check_generic_bounds(
            declared_return.as_ref().unwrap_or(&Type::mixed()),
            "PHPDoc tag @return",
            &label,
            f.span,
        );

        // Variance-enforcement half - same "check the declaration once"
        // placement as bound-enforcement above, and only meaningful for
        // a method (a plain top-level function has no enclosing class to
        // have declared variance for). `__construct` is exempt, mirroring
        // real PHPStan's own rule: a constructor is only ever called to
        // build one specific, concrete instance (`new Success<Cat>($cat)`),
        // never invoked polymorphically through a variable typed as the
        // covariant supertype the way an ordinary method can be - so a
        // covariant `T` constructor parameter doesn't actually violate
        // covariance safety.
        if let (Some(sig), Some(c)) = (sig, class_name)
            && !f.is_constructor()
        {
            self.check_variance(sig, c, f.span);
        }
        // `function.variance`/`method.variance` - the own-declaration
        // half, unlike `check_variance` above, runs unconditionally
        // (including for `__construct`/private methods).
        self.check_own_template_variance_declaration(f, class_name.is_some(), &label, f.span);

        // Same "check the declaration once" placement as the bound/
        // variance checks above - a missing typehint is about the
        // signature as written, not something a body-local `extract()`
        // should be able to hide either.
        self.check_missing_function_typehints(
            f,
            class_name,
            &label,
            &resolved_params,
            declared_return.as_ref(),
            sig,
        );
        if self.settings.extras.check_native_type_hints
            && let Some(sig) = sig
        {
            self.check_native_type_hints(f, class_name, &label, sig);
        }

        let previous_class = self.current_class.take();
        self.current_class = class_name.map(|s| s.to_string());
        let previous_method = self.current_method.take();
        self.current_method = Some(f.name.to_string());
        let own_templates = match f.doc_comment.as_deref() {
            Some(doc) if doc.contains("template") => crate::registry::build_template_params(
                &phpdoc::docblock::parse_shared(doc),
                &f.name,
                &self.name_ctx,
            ),
            _ => rustc_hash::FxHashMap::default(),
        };
        let previous_method_templates =
            std::mem::replace(&mut self.current_method_templates, own_templates);
        // `checker::restricted_symbol_usage`'s own attribute-context
        // matching - see `current_class_attrs`'s own doc comment.
        let previous_class_attrs = std::mem::take(&mut self.current_class_attrs);
        self.current_class_attrs = class_name
            .and_then(|c| self.registry.classes.get(c))
            .map(|info| {
                info.attributes()
                    .iter()
                    .map(|a| a.name_arc().clone())
                    .collect()
            })
            .unwrap_or_default();
        let previous_method_attrs = std::mem::take(&mut self.current_method_attrs);
        self.current_method_attrs = f
            .attrs
            .iter()
            .map(|a| Name::from(&self.resolve_class_name(&a.name)))
            .collect();
        // Mirrors real PHPStan's `DeprecatedScopeHelper` - suppresses
        // every deprecated-usage diagnostic from within a deprecated
        // function/method, class, or trait's own body.
        let previous_scope_deprecated = self.current_scope_deprecated;
        self.current_scope_deprecated = sig.is_some_and(|s| s.deprecated().is_some())
            || self.current_class.as_deref().is_some_and(|c| {
                self.registry
                    .classes
                    .get(c)
                    .is_some_and(|i| i.deprecated().is_some())
            })
            || self.current_trait.as_deref().is_some_and(|t| {
                self.registry
                    .classes
                    .get(t)
                    .is_some_and(|i| i.deprecated().is_some())
            });

        let diagnostics_before = self.diagnostics.len();
        let previous_dynamic_flag = self.saw_dynamic_var_construct;
        self.saw_dynamic_var_construct = false;
        let previous_generator_flag = self.current_function_is_generator;
        self.current_function_is_generator = f.is_generator();
        let previous_declared_return_type = self.current_declared_return_type.take();
        self.current_declared_return_type =
            declared_return.as_ref().map(|t| match &self.current_class {
                Some(c) => self.resolve_pseudo(t, c),
                None => t.clone(),
            });
        let previous_declared_throws_type = self.current_declared_throws_type.take();
        self.current_declared_throws_type = sig.and_then(|s| s.throws().clone());
        let previous_goto_targets = std::mem::replace(
            &mut self.current_function_goto_targets,
            f.body
                .as_ref()
                .map(|b| GotoState::new(b.goto_labels.clone()))
                .unwrap_or_default(),
        );
        let previous_assigned_variables = self.enter_assigned_variables(f.body_stmts());
        // See `Checker::current_function_throw_points`'s doc comment.
        let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
        let previous_try_throw_states = std::mem::take(&mut self.try_throw_states);
        let previous_unannotated_call =
            std::mem::take(&mut self.current_function_has_unannotated_call);
        let previous_explicit_throw_points =
            std::mem::take(&mut self.current_function_explicit_throw_points);
        let previous_exit_points = std::mem::take(&mut self.current_function_exit_points);
        // See `Checker::current_function_return_points`'s doc comment -
        // reset/restored at the identical choke point as the throw-
        // tracking fields above.
        let previous_return_points = std::mem::take(&mut self.current_function_return_points);
        let previous_saw_explicit_return =
            std::mem::take(&mut self.current_function_saw_explicit_return);
        // Populates `current_function_by_ref_params` with every by-ref
        // parameter's check target (`@param-out` when present, else its
        // ordinary declared type, mirroring real PHPStan's
        // `getOutType() ?? getType()` fallback). Falls back to a raw
        // native-hint resolution when this declaration has no resolved
        // `sig` at all - correctly lenient-by-omission, matching every
        // other registry-aware check in this function.
        let previous_by_ref_params = std::mem::take(&mut self.current_function_by_ref_params);
        let previous_by_ref_exit_snapshots =
            std::mem::take(&mut self.current_function_by_ref_exit_snapshots);
        let previous_by_ref_params_with_tag =
            std::mem::take(&mut self.current_function_by_ref_params_with_tag);
        match sig {
            Some(sig) => {
                for param in sig.params() {
                    if param.by_ref() {
                        let name: Name = param.name().clone();
                        if let Some(out_ty) = &param.out_ty() {
                            self.current_function_by_ref_params_with_tag
                                .insert(name.clone(), out_ty.clone());
                        }
                        let check_ty = param.out_ty().clone().unwrap_or_else(|| param.ty().clone());
                        self.current_function_by_ref_params.insert(name, check_ty);
                    }
                }
            }
            None => {
                for p in &f.params {
                    if p.by_ref {
                        let ty = self.resolve_type_hint(p.type_hint.as_ref());
                        self.current_function_by_ref_params
                            .insert(p.name.name.clone(), ty);
                    }
                }
            }
        }
        self.check_generator_return_type(native_return.as_ref(), f.span);

        // `check_block`, not a bare loop over `check_stmt`: a function's
        // top-level body is exactly as eligible for `deadCode.unreachable`
        // as any nested `{ ... }` block is - it just needs to be routed
        // through the one place that check lives.
        let previous_implicit_calls = std::mem::take(&mut self.implicit_calls);
        let body_terminates = self
            .check_block(
                f.body_stmts(),
                &mut locals,
                declared_return.as_ref(),
                &label,
            )
            .completion
            .terminates;
        let implicit_calls = std::mem::replace(&mut self.implicit_calls, previous_implicit_calls);
        // Judged after the body, which finds the offsets that are calls.
        self.check_purity_impure_points(f, sig, class_name.is_some(), &label, &implicit_calls);

        // A function that falls off the end of its body with no `return`
        // at all implicitly returns `null` too - equally a real mismatch
        // against a non-nullable, non-void, non-mixed declared return
        // type: `Completion::terminates` (already used for guard-clause
        // narrowing) answers "does this statement definitely end control
        // flow" - a `false` here means this function's body doesn't
        // guarantee returning anything. A real generator is exempt:
        // falling off the end (or an explicit bare `return;`, handled in
        // check_stmt) implicitly ends the generator's iteration, not the
        // function's own return value - real PHPStan never checks either
        // against the function's declared Generator/Iterator/Traversable
        // return type. Not just `f.body.last()`: a statement anywhere in
        // the body unconditionally terminating is equally sound proof
        // the function never falls off the end, the same reasoning
        // `Completion::terminates`'s `Block` arm uses for a nested `{ ... }` -
        // `f.body` just isn't itself one `Stmt`, so it needs the same
        // check spelled out here directly. A generator stays exempt from
        // this whole check unless its declared `Generator<TKey, TValue,
        // TSend, TReturn>` carries a real, non-Mixed/Void TReturn - in
        // which case falling off the end is exactly as real a miss as it
        // is for an ordinary function, just checked against TReturn
        // instead of the outer Generator type itself.
        let generator_treturn = self
            .current_function_is_generator
            .then(|| {
                generator_missing_return_type(
                    self.current_declared_return_type.as_ref(),
                    self.registry,
                )
            })
            .flatten();
        // The falls-off-the-end path's two separate contributions to
        // `current_function_return_points`, mirroring real PHPStan's
        // `TooWideTypeCheck`: pushes `VoidType` unconditionally whenever
        // the body doesn't always-terminate (not folded into the `Null`
        // push below), and a `Null` push only when there's no native
        // return type at all (a native type always wins for "what does
        // falling off the end actually produce" - an untyped function is
        // the only shape where PHP's implicit-`null`-return is the real,
        // uncommitted fact). Never for a generator, same reasoning as
        // both `Return` arms in `stmt.rs`.
        let body_falls_off_end = !body_terminates;
        if body_falls_off_end {
            self.record_by_ref_exit_snapshot(&locals);
        }
        if !self.current_function_is_generator && body_falls_off_end {
            self.current_function_return_points.push(Type::void());
            if native_return.is_none() {
                self.current_function_return_points.push(Type::null());
            }
        }
        if let Some(expected) = &declared_return
            && (!f.body_stmts().is_empty()
                || (self.settings.extras.check_empty_method_bodies && is_concrete))
            && body_falls_off_end
            && (!self.current_function_is_generator || generator_treturn.is_some())
        {
            let is_generator_treturn = generator_treturn.is_some();
            let expected = if let Some(treturn) = generator_treturn {
                treturn
            } else {
                match &self.current_class {
                    Some(c) => self.resolve_pseudo(expected, c),
                    None => expected.clone(),
                }
            };
            // A `never`-typed function/method whose body simply falls
            // off the end without ever throwing/exiting is the same
            // "should never return" claim `Return(Some(e))`/
            // `Return(None)` already special-case, just reached via the
            // implicit fall-through instead of an explicit `return`
            // statement - real PHPStan's `return.never` covers both
            // shapes identically.
            if expected.is_never() {
                self.report(
                    "should never return, but a return statement was reached".to_string(),
                    f.span,
                    3,
                    "return.never",
                );
            } else if let Some(level) =
                missing_return_level(&expected, f.return_type.is_some(), is_generator_treturn)
            {
                let mut builder = DiagnosticBuilder::new(
                    format!("should return {expected} but return statement is missing"),
                    f.span,
                    level,
                    "return.missing",
                );
                if f.return_type.is_some() {
                    builder = builder.non_ignorable();
                }
                self.report_builder(builder);
            }
        }
        // `body_falls_off_end == false` means the body already provably
        // always terminates; paired with never seeing a real `return`
        // (a bare `return;` counts too), that's "never returns
        // normally". Scoped to an undeclared (reads as `Mixed`) or
        // `void`-declared return type only.
        if self.settings.extras.check_never_return_suggestions
            && !f.body_stmts().is_empty()
            && !self.current_function_is_generator
            && !body_falls_off_end
            && !self.current_function_saw_explicit_return
            && declared_return
                .as_ref()
                .is_some_and(|t| t.is_void() || t.is_mixed())
        {
            self.report(
                "could be declared to return never - it never returns normally".to_string(),
                f.return_type.as_ref().map(|h| h.span).unwrap_or(f.span),
                5,
                "return.neverCandidate",
            );
        }

        // Needs the same body-level extract()/eval() guard as everything
        // else checked in this function - placed before the truncation
        // below (rather than after) so a constructor that calls
        // extract()/eval() has this diagnostic discarded along with
        // every other one from its own body.
        self.check_unused_constructor_parameters(f, class_name);

        // Both run once the whole body (every real throw point it
        // contains) has been checked, still ahead of the
        // `saw_dynamic_var_construct` truncation below, so a declaration
        // using `extract()`/`eval()` gets these diagnostics swept away
        // along with everything else from its own body.
        //
        // Gated on the identical "has a real body at all" condition the
        // `return.missing`/`return.never` check above uses - an
        // abstract/interface declaration's `@throws` tag documents what
        // a real implementor is expected to throw, not what this
        // (nonexistent) body throws, so `check_too_wide_throw_type`'s
        // "nothing was thrown at all, flag every declared member"
        // fallback would otherwise unconditionally fire on every
        // `@throws` tag any interface/abstract method ever carries.
        if !f.body_stmts().is_empty()
            || (self.settings.extras.check_empty_method_bodies && is_concrete)
        {
            let subject = match class_name {
                Some(c) => format!("Method {c}::{}()", f.name),
                None => format!("Function {}()", f.name),
            };
            let throws_tag = sig.and_then(|s| s.throws().as_ref());
            if let Some(throws) = throws_tag {
                let should_check = match (class_name, sig) {
                    (Some(c), Some(s)) => {
                        self.should_check_too_wide_method_throw_type(c, &f.name, s.visibility())
                    }
                    // `TooWideFunctionThrowTypeRule` (a plain top-level
                    // function, or a method whose signature somehow
                    // isn't indexed) has no visibility/finality gate -
                    // see `should_check_too_wide_method_throw_type`'s
                    // doc comment.
                    _ => true,
                };
                if should_check {
                    self.check_too_wide_throw_type(throws, &subject, f.span);
                }
            }
            self.check_missing_checked_exceptions(throws_tag, &subject);

            // Same "has a real body at all" gate, same choke point as
            // the throw-type check above.
            if let Some(declared) = &declared_return {
                let declared = match &self.current_class {
                    Some(c) => self.resolve_pseudo(declared, c),
                    None => declared.clone(),
                };
                let should_check = match (class_name, sig) {
                    (Some(c), Some(s)) => {
                        self.should_check_too_wide_method_return_type(c, &f.name, s.visibility())
                    }
                    _ => true,
                };
                if should_check {
                    let is_first_declaration = match (class_name, sig) {
                        (Some(c), Some(_)) => self
                            .registry
                            .find_method_signature_prototype(c, &f.name)
                            .is_none(),
                        _ => true,
                    };
                    let is_private = sig.is_some_and(|s| s.visibility() == Visibility::Private);
                    self.check_too_wide_return_type(
                        &declared,
                        is_first_declaration,
                        is_private,
                        &subject,
                        f.span,
                    );
                }
            }
            // Real `ParameterOutExecutionEndTypeRule` has no
            // visibility/finality gate of its own (unlike the two
            // too-wide rules) - runs whenever a by-ref parameter
            // actually declares `@param-out` at all.
            self.check_by_ref_execution_end(f.span);
            let should_check_by_ref_too_wide = match (class_name, sig) {
                (Some(c), Some(s)) => {
                    self.should_check_too_wide_by_ref_type(c, &f.name, s.visibility())
                }
                _ => true,
            };
            if should_check_by_ref_too_wide {
                self.check_too_wide_by_ref_type(f.span);
            }
        }

        if self.saw_dynamic_var_construct {
            self.diagnostics.truncate(diagnostics_before);
        }
        self.saw_dynamic_var_construct = previous_dynamic_flag;
        self.current_function_is_generator = previous_generator_flag;
        self.current_declared_return_type = previous_declared_return_type;
        self.current_declared_throws_type = previous_declared_throws_type;
        self.current_function_throw_points = previous_throw_points;
        self.try_throw_states = previous_try_throw_states;
        self.current_function_has_unannotated_call = previous_unannotated_call;
        self.current_function_explicit_throw_points = previous_explicit_throw_points;
        self.current_function_exit_points = previous_exit_points;
        self.current_function_return_points = previous_return_points;
        self.current_function_saw_explicit_return = previous_saw_explicit_return;
        self.current_function_by_ref_params = previous_by_ref_params;
        self.current_function_by_ref_exit_snapshots = previous_by_ref_exit_snapshots;
        self.current_function_by_ref_params_with_tag = previous_by_ref_params_with_tag;
        self.current_function_goto_targets = previous_goto_targets;
        self.current_function_assigned_variables = previous_assigned_variables;

        self.current_class = previous_class;
        self.current_method = previous_method;
        self.current_method_templates = previous_method_templates;
        self.current_class_attrs = previous_class_attrs;
        self.current_method_attrs = previous_method_attrs;
        self.current_scope_deprecated = previous_scope_deprecated;
    }

    /// Mirrors real PHPStan's `parameter.unionTypeNotSupported`/`return.
    /// unionTypeNotSupported` - a native union type-hint (`int|string`)
    /// pre-8.0. Thin wrapper over `check_native_union_types_supported_raw`
    /// (below), which does the real work off raw text so it can also be
    /// called from property-hook checking - a hook has no `FunctionDecl`
    /// to hand this.
    fn check_native_union_types_supported(
        &mut self,
        f: &FunctionDecl,
        is_method: bool,
        label: &str,
    ) {
        let kind_word = if is_method { "Method" } else { "Function" };
        self.check_native_union_types_supported_raw(
            f.params.iter().map(|p| p.type_hint.as_ref()),
            f.return_type.as_ref(),
            format_args!(
                "{kind_word} {label}() uses native union types but they're supported only on PHP 8.0 and later."
            ),
            f.span,
        );
    }

    /// The signature-agnostic core of `check_native_union_types_supported`,
    /// shared the same way real PHPStan's own `FunctionDefinitionCheck::
    /// checkParametersAcceptor` is shared between an ordinary function/
    /// method and a property hook (`ExistingClassesInPropertyHookTypehintsRule`
    /// calls straight into it via `FunctionDefinitionCheck::checkClassMethod`).
    /// Works directly off each parameter's raw native type-hint text (a
    /// native-only union always contains a bare top-level `|` with no
    /// enclosing `?`/generic wrapper), not the docblock-merged `Type`,
    /// matching real PHPStan's native-AST-node-only scope. Reports at
    /// most once (first offending parameter, else the return type),
    /// matching real PHPStan's own `$unionTypeReported` latch.
    pub(crate) fn check_native_union_types_supported_raw<'p>(
        &mut self,
        param_hints: impl Iterator<Item = Option<&'p TypeHint>>,
        return_hint: Option<&TypeHint>,
        message: impl std::fmt::Display,
        span: Span,
    ) {
        if self
            .registry
            .php_supports_everywhere(PhpFeature::NativeUnionTypes)
            != Some(false)
        {
            return;
        }
        for hint in param_hints {
            if is_bare_native_union(hint) {
                self.report(
                    message.to_string(),
                    span,
                    0,
                    "parameter.unionTypeNotSupported",
                );
                return;
            }
        }
        if is_bare_native_union(return_hint) {
            self.report(message.to_string(), span, 0, "return.unionTypeNotSupported");
        }
    }

    /// Mirrors real PHPStan's `parameter.unresolvableNativeType`
    /// (`FunctionDefinitionCheck`/`UnresolvableTypeHelper`, gated on PHP
    /// 8.1 pure-intersection-type support) - fires once per parameter,
    /// unlike `check_native_union_types_supported_raw`'s single-report
    /// latch, matching real PHPStan's own per-parameter loop.
    fn check_unresolvable_native_types(&mut self, f: &FunctionDecl, label: &str) {
        for p in &f.params {
            self.check_unresolvable_native_type(
                p.type_hint.as_ref(),
                format_args!(
                    "Parameter ${} of {label}() has unresolvable native type.",
                    p.name
                ),
                "parameter.unresolvableNativeType",
                p.name.span,
            );
        }
        self.check_unresolvable_native_type(
            f.return_type.as_ref(),
            format_args!("{label}() has unresolvable native return type."),
            "return.unresolvableNativeType",
            f.span,
        );
    }

    /// The signature-agnostic core `check_unresolvable_native_types`
    /// wraps, shared with property-hook checking the same way `check_
    /// native_union_types_supported_raw` is - a hook has no `FunctionDecl`
    /// to hand the wrapper above. Resolves `hint`'s own native type (no
    /// docblock merge, matching real PHPStan's `getNativeType()` scope)
    /// and flags it via `Registry::is_intersection_possible` when it's a
    /// pure-object intersection with no possible concrete implementor.
    /// Only ever acts on a bare object-only intersection - any other
    /// member shape (an accessory type, a generic argument, ...) is left
    /// alone; this project's own intersection-legality logic doesn't
    /// model non-class impossibility at all, and guessing there would
    /// risk a false positive on a shape nothing has verified yet.
    pub(crate) fn check_unresolvable_native_type(
        &mut self,
        hint: Option<&TypeHint>,
        message: impl std::fmt::Display,
        identifier: &'static str,
        span: Span,
    ) {
        if self
            .registry
            .php_supports_somewhere(PhpFeature::PureIntersectionTypes)
            != Some(true)
        {
            return;
        }
        let Some(hint) = hint else {
            return;
        };
        if !matches!(hint.kind, TypeHintKind::Intersection(_)) {
            return;
        }
        let resolved = self.resolve_type_hint(Some(hint));
        let members = resolved.intersection_members();
        if members.len() < 2 {
            return;
        }
        let mut names = Vec::with_capacity(members.len());
        for m in members {
            let Some((name, _)) = m.as_object() else {
                return;
            };
            names.push(name.to_string());
        }
        if self.registry.is_intersection_possible(&names) {
            return;
        }
        self.report_builder(
            DiagnosticBuilder::new(message.to_string(), span, 0, identifier).non_ignorable(),
        );
    }

    /// Mirrors real PHPStan's `parameter.implicitlyNullable` (PHP 8.4) -
    /// a typed parameter whose only source of nullability is an
    /// `= null` default (`Foo $x = null`, not `?Foo $x = null`/a union
    /// already naming `null`/`mixed`). `$order` in
    /// `FunctionDefinitionCheck::checkImplicitlyNullableType` is the
    /// parameter's 1-based position among every parameter, typed or not -
    /// `i` below mirrors that exactly, not a typed-only counter.
    fn check_implicitly_nullable_params(&mut self, f: &FunctionDecl) {
        if self
            .registry
            .php_supports_somewhere(PhpFeature::ImplicitlyNullableParameterDeprecated)
            != Some(true)
        {
            return;
        }
        for (i, p) in f.params.iter().enumerate() {
            let Some(hint) = &p.type_hint else {
                continue;
            };
            let admits_null_or_is_intersection = match &hint.kind {
                TypeHintKind::Nullable(_)
                | TypeHintKind::Intersection(_)
                | TypeHintKind::Keyword(NativeType::Mixed | NativeType::Null, _) => true,
                TypeHintKind::Union(members) => members.iter().any(|m| {
                    matches!(
                        m.kind,
                        TypeHintKind::Intersection(_) | TypeHintKind::Keyword(NativeType::Null, _)
                    )
                }),
                _ => false,
            };
            if admits_null_or_is_intersection {
                continue;
            }
            let is_default_null = matches!(
                &p.default,
                Some(Expr {
                    kind: ExprKind::Literal(Literal::Null),
                    ..
                })
            );
            if !is_default_null {
                continue;
            }
            self.report(format!(
                    "Deprecated in PHP 8.4: Parameter #{} ${} ({hint}) is implicitly nullable via default value null.",
                    i + 1,
                    p.name,
                ),
                p.name.span,
                0,
                "parameter.implicitlyNullable",
            );
        }
    }

    /// Mirrors real PHPStan's `parameter.requiredAfterOptional` (PHP 8.0) -
    /// a required parameter (no default, not variadic) following an
    /// optional one (has a default). Deliberately narrower than real
    /// PHPStan's full version-layered logic: this project only ports the
    /// base PHP 8.0 rule (an ordinary optional parameter, of any type or
    /// none, followed by a required one) - real PHPStan's further PHP
    /// 8.1/8.3 special cases (a typed, `= null`-defaulted parameter only
    /// sometimes counting as "optional," depending on whether its type
    /// is nullable/a union naming `null`/`mixed`) are left unbuilt.
    fn check_required_parameter_after_optional(&mut self, f: &FunctionDecl) {
        if self
            .registry
            .php_supports_somewhere(PhpFeature::RequiredParameterAfterOptionalDeprecated)
            != Some(true)
        {
            return;
        }
        let mut optional_param: Option<&str> = None;
        for p in &f.params {
            if let Some(optional) = optional_param
                && p.default.is_none()
                && !p.variadic
            {
                self.report(format!(
                        "Deprecated in PHP 8.0: Required parameter ${} follows optional parameter ${optional}.",
                        p.name,
                    ),
                    p.name.span,
                    0,
                    "parameter.requiredAfterOptional",
                );
                continue;
            }
            if p.default.is_some() {
                optional_param = Some(&p.name);
            }
        }
    }

    /// Real PHPStan's own `Rules/Functions/
    /// IncompatibleDefaultParameterTypeRule.php`/`Rules/Methods/
    /// IncompatibleDefaultParameterTypeRule.php` (level 2) - a
    /// parameter's own default value expression's type must be
    /// assignable to its declared type (`function f(int $x = "bad")`).
    /// Scoped to a named function/method only, matching every other
    /// `sig`-gated check in `check_function_impl` - real PHPStan's own
    /// closure/arrow-fn variants (`IncompatibleClosureDefaultParameterTypeRule`/
    /// `IncompatibleArrowFunctionDefaultParameterTypeRule`) aren't
    /// attempted here. A variadic parameter can never have a real
    /// default in valid PHP, so it's skipped outright.
    ///
    /// The parameter's templates are resolved to their bounds at any
    /// depth (PHPStan's `TemplateTypeHelper::resolveToBounds`): a default
    /// is checked against what every binding of them allows.
    ///
    /// The real rule calls `Type::accepts` directly rather than through
    /// `RuleLevelHelper`, and a `mixed` value is accepted everywhere
    /// there, so a default whose type isn't known is never reported.
    fn check_default_parameter_types(
        &mut self,
        f: &FunctionDecl,
        sig: &FunctionReflection,
        class_name: Option<&str>,
    ) {
        for (i, p) in f.params.iter().enumerate() {
            let Some(default) = &p.default else {
                continue;
            };
            if p.variadic {
                continue;
            }
            let Some(param_sig) = sig.params().get(i) else {
                continue;
            };
            let param_ty = &param_sig.ty().substitute_templates_resolving_to_bounds(
                &rustc_hash::FxHashMap::default(),
                &|_| true,
                &self.relation(),
            );
            let default_ty = self.infer_expr(default, &self.new_locals());
            if default_ty.is_mixed() || self.accepts(param_ty, &default_ty).yes() {
                continue;
            }
            let target = match class_name {
                Some(c) => format!("method {c}::{}()", f.name),
                None => format!("function {}()", f.name),
            };
            self.report(format!(
                    "Default value of the parameter #{} ${} ({default_ty}) of {target} is incompatible with type {param_ty}.",
                    i + 1,
                    p.name,
                ),
                default.span,
                2,
                "parameter.defaultValue",
            );
        }
    }
}

impl<'a> Checker<'a> {
    /// `MissingReturnRule` for a closure literal, which is only checked
    /// when it declares a native return type. Called with the converged
    /// post-body `locals`, before the closure's generator flag is
    /// restored.
    pub(crate) fn check_closure_missing_return(
        &mut self,
        body_terminates: bool,
        has_native_return: bool,
        declared_return: Option<&Type>,
        span: Span,
    ) {
        if !has_native_return || self.current_function_is_generator {
            return;
        }
        let Some(expected) = declared_return else {
            return;
        };
        if body_terminates {
            return;
        }
        let expected = match &self.current_class {
            Some(c) => self.resolve_pseudo(expected, &c.clone()),
            None => expected.clone(),
        };
        if expected.is_never() {
            self.report_builder(
                DiagnosticBuilder::new(
                    "should never return, but a return statement was reached".to_string(),
                    span,
                    3,
                    "return.never",
                )
                .non_ignorable(),
            );
            return;
        }
        let Some(level) = missing_return_level(&expected, true, false) else {
            return;
        };
        self.report_builder(
            DiagnosticBuilder::new(
                format!("should return {expected} but return statement is missing"),
                span,
                level,
                "return.missing",
            )
            .non_ignorable(),
        );
    }
}

/// `MissingReturnRule`'s condition for a body that falls off its end:
/// `None` when no return is owed (a `void`-accepting type, or an
/// undeclared/PHPDoc-only `mixed`), otherwise the level to report at -
/// level 2 (`checkPhpDocMissingReturn`) for a generator's `TReturn` or a
/// PHPDoc-only nullable type, level 0 otherwise.
pub(crate) fn missing_return_level(
    expected: &Type,
    has_native_hint: bool,
    is_generator_treturn: bool,
) -> Option<u8> {
    if expected.is_void() {
        return None;
    }
    if expected.is_mixed() && (!has_native_hint || is_generator_treturn) {
        return None;
    }
    let contains_null = expected.union_members().iter().any(|m| m.is_null().yes());
    if is_generator_treturn || (!has_native_hint && contains_null) {
        Some(2)
    } else {
        Some(0)
    }
}

/// Whether a native type declaration is a plain union (`int|string`),
/// not a nullable single type or a DNF type with an intersection term.
fn is_bare_native_union(hint: Option<&TypeHint>) -> bool {
    hint.is_some_and(|h| match &h.kind {
        TypeHintKind::Union(members) => members
            .iter()
            .all(|m| !matches!(m.kind, TypeHintKind::Intersection(_))),
        _ => false,
    })
}

/// "parameter $name of label()": how a message names a parameter, written
/// out only when a message is.
#[derive(Clone, Copy)]
struct ParameterSubject<'a> {
    name: &'a dyn std::fmt::Display,
    label: &'a str,
}

impl std::fmt::Display for ParameterSubject<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "parameter ${} of {}()", self.name, self.label)
    }
}

/// "return type of label()", likewise.
#[derive(Clone, Copy)]
struct ReturnSubject<'a> {
    label: &'a str,
}

impl std::fmt::Display for ReturnSubject<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "return type of {}()", self.label)
    }
}
