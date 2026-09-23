use crate::ast::*;
use crate::checker::{
    ATTRIBUTE_TARGET_FUNCTION, ATTRIBUTE_TARGET_METHOD, ATTRIBUTE_TARGET_PARAMETER,
    ATTRIBUTE_TARGET_PROPERTY, Checker, DiagnosticBuilder, Locals, body_contains_explicit_throw,
    body_contains_yield, collect_goto_targets, generator_missing_return_type,
    is_constant_condition_family_identifier, type_allows_generator_return,
};
use crate::docblock;
use crate::php_version::PhpFeature;
use crate::registry::{
    ClassInfo, FunctionSig, Registry, collect_class_name_case_refs, mark_template_params,
    resolve_names_in_type, resolve_type,
};
use crate::token::Span;
use crate::types::{self, Type};

impl<'a> Checker<'a> {
    /// A trait's method bodies are checked once per class that `use`s
    /// it, not standalone - `flatten_traits` (`registry.rs`) already
    /// merges a trait's methods/properties into every using class's
    /// `ClassInfo` before checking starts, so re-checking with
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
        let contexts: Vec<std::sync::Arc<str>> = if has_hosts {
            hosts
        } else {
            vec![std::sync::Arc::from(fqn)]
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
        // `c` had it won the slot - `ClassInfo::from_decl` is a pure
        // function of `(c, ctx)`, and no later pass ever mutates a
        // trait's registry entry after indexing, so this is a no-op
        // reconstruction for the common non-duplicated-trait case.
        let own_info = ClassInfo::from_decl(c, &self.name_ctx);
        let host_count = contexts.len();
        let mut family_verdicts_by_span: std::collections::HashMap<
            Span,
            (
                std::collections::HashSet<usize>,
                std::collections::HashSet<bool>,
            ),
        > = std::collections::HashMap::new();
        for (host_index, host) in contexts.into_iter().enumerate() {
            let host_start = self.diagnostics.len();
            for m in &c.methods {
                let own_sig = own_info.methods.get(m.function.name.as_ref());
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
        // A trait genuinely unused by any real host is its own only
        // possible consumer of its own property-too-wide-type facts -
        // safe to consume right here, in this same self-contained pass,
        // unlike the real-host case (left to whichever host's own
        // top-level `ClassDecl` processing calls `check_too_wide_
        // property_types` for its own fqn, since a real host's own
        // property-assignment facts can also come from its own directly-
        // declared methods, checked at an unpredictable point relative
        // to this trait - only the two-pass `cross_file_property_assign_
        // facts` merge, not this single-pass call, can see both sides
        // reliably).
        if !has_hosts {
            self.check_too_wide_property_types(fqn, c);
        }
        // A trait used by 2+ classes gets checked 2+ times - a
        // genuinely host-independent bug in the trait's body (no
        // `$this->...` involved) would otherwise be reported once per
        // host, since diagnostics are only sorted, never deduplicated
        // elsewhere. A real host-specific finding (missing on one host
        // but not another) survives this, since its message genuinely
        // differs per host. This local sort exists only to bring
        // duplicates adjacent for `dedup_by` (which only removes
        // consecutive matches).
        self.diagnostics[start..].sort_by(|a, b| {
            (a.span.start.line, a.span.start.col, &a.message).cmp(&(
                b.span.start.line,
                b.span.start.col,
                &b.message,
            ))
        });
        self.diagnostics
            .dedup_by(|a, b| a.span == b.span && a.message == b.message);
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
            let tail = self.diagnostics.split_off(start);
            self.diagnostics.extend(tail.into_iter().filter(|d| {
                !is_constant_condition_family_identifier(&d.identifier)
                    || family_verdicts_by_span
                        .get(&d.span)
                        .is_some_and(|(hosts, verdicts)| {
                            hosts.len() == host_count && verdicts.len() == 1
                        })
            }));
        }
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
    ) -> Option<&'a FunctionSig> {
        match class_name {
            Some(c) => self.registry.classes.get(c)?.methods.get(f.name.as_ref()),
            None => self
                .registry
                .functions
                .get(self.name_ctx.declared_name(&f.name).as_str()),
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
        label: &str,
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
        self.report(
            label.to_string(),
            format!(
                "yields, but its declared return type {resolved} is not a supertype of Generator, Iterator, Traversable, or iterable"
            ),
            span,
            0,
            "generator.returnType",
        );
    }

    #[hotpath::measure]
    pub(crate) fn check_function(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        is_static: bool,
        is_concrete: bool,
    ) {
        // The registry keeps one signature per top-level function name; a
        // second declaration of the same name elsewhere is checked against
        // its own.
        if class_name.is_none()
            && let Some(own) = self.own_declaration_sig(f)
        {
            self.check_function_impl(f, class_name, is_static, is_concrete, None, Some(&own));
            return;
        }
        self.check_function_with_ancestor_hint(f, class_name, is_static, is_concrete, None);
    }

    /// `f`'s own signature when the registry's slot for its name was built
    /// from a different declaration (or merged from several); `None` when
    /// the slot is `f`'s.
    fn own_declaration_sig(&self, f: &FunctionDecl) -> Option<FunctionSig> {
        let name = self.name_ctx.declared_name(&f.name);
        let site = *self.registry.function_decl_sites.get(name.as_str())?;
        if site == Some((f.name_span.start.line, f.name_span.start.col)) {
            return None;
        }
        let fresh = FunctionSig::from_decl(
            f,
            &self.name_ctx,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            crate::ast::Visibility::Public,
        );
        Some(self.registry.finalize_standalone_sig(fresh))
    }

    /// `check_function`'s real body - `ancestor_sig_hint` is `Some` only
    /// for an anonymous class's own method, where it's the first
    /// ancestor's signature found by walking `decl.extends`/
    /// `decl.implements` directly at the call site (`ExprKind::
    /// NewAnonymousClass`'s handling in `checker/expr.rs`) - an
    /// anonymous class is never indexed into the registry under any
    /// stable name (see `lookup_own_signature`'s doc comment), so `sig`
    /// below is always `None` for one. `ancestor_sig_hint` is
    /// deliberately used only at `check_missing_function_typehints`'s
    /// call site below, not substituted in for `sig` throughout this
    /// whole function - correct there (that check only ever reads a
    /// signature's own docblock-provenance bits, `has_own_doc`/
    /// `explicit_mixed_hint`, which an ancestor's genuinely still is),
    /// but not obviously safe for every other `Some(sig)`-gated check
    /// here (e.g. variance/bound enforcement checking the anonymous
    /// class's own body against a signature it never actually
    /// redeclared).
    pub(crate) fn check_function_with_ancestor_hint(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        is_static: bool,
        is_concrete: bool,
        ancestor_sig_hint: Option<&FunctionSig>,
    ) {
        self.check_function_impl(
            f,
            class_name,
            is_static,
            is_concrete,
            ancestor_sig_hint,
            None,
        );
    }

    /// For a trait method body whose own literal declaration (not the
    /// registry's single winning slot for this trait's name - see
    /// `check_trait_methods`'s doc comment) is already known. `own_sig`
    /// is a full substitute for the ordinary registry-derived `sig`, not
    /// a hint - a trait's own literal declaration genuinely is this
    /// method's real signature, unlike `ancestor_sig_hint`'s narrower,
    /// single-call-site-only role.
    pub(crate) fn check_function_with_own_sig(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        is_static: bool,
        is_concrete: bool,
        own_sig: Option<&FunctionSig>,
    ) {
        self.check_function_impl(f, class_name, is_static, is_concrete, None, own_sig);
    }

    /// The real body shared by `check_function`/
    /// `check_function_with_ancestor_hint`/`check_function_with_own_sig` -
    /// `ancestor_sig_hint` is `Some` only for an anonymous class's own
    /// method (see `check_function_with_ancestor_hint`'s doc comment);
    /// `own_sig_override`, when `Some`, replaces the ordinary registry
    /// lookup for `sig` entirely (used only by `check_trait_methods`).
    fn check_function_impl(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        is_static: bool,
        is_concrete: bool,
        ancestor_sig_hint: Option<&FunctionSig>,
        own_sig_override: Option<&FunctionSig>,
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
        if !self.facts_only {
            if class_name.is_none() {
                self.check_unknown_phpstan_tags_on(&label, f.doc_comment.as_deref(), f.name_span);
            }
            self.check_declared_purity_tag(f, sig, class_name.is_some(), &label);
            self.check_purity_impure_points(f, sig, class_name.is_some(), &label);
            self.check_declared_param_callable_tags(f, &label);
            self.check_internal_param_and_return_types(f, class_name, &label);
            self.check_restricted_param_and_return_types(f, class_name, &label);
            self.check_assert_tag_types(sig, f.span, &label);
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
            self.check_implicitly_nullable_params(f, &label);
            self.check_required_parameter_after_optional(f, &label);
            if let Some(sig) = sig {
                let previous_class =
                    std::mem::replace(&mut self.current_class, class_name.map(str::to_string));
                self.check_default_parameter_types(f, sig, class_name, &label);
                self.check_phpdoc_types_against_native(f, sig, class_name, &label);
                self.check_conditional_types(f, sig, class_name, &label);
                let description = if class_name.is_some() {
                    "a method"
                } else {
                    "a function"
                };
                self.check_misplaced_var_tag(
                    f.doc_comment.as_deref(),
                    description,
                    f.name_span,
                    &label,
                );
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
            // `FunctionSig` has no persisted raw template-bound/-default map
            // of its own. Cheap substring pre-filter before the real
            // docblock::parse below - most documented methods declare no
            // `@template` at all, so this skips a real parse cost on every
            // one. Matched on the bare `template` substring rather than
            // any one spelling: `docblock::parse` treats `@phpstan-`/
            // `@psalm-`/`@phan-template*` identically to `@template*`, and
            // a filter checking only some spellings would silently stop
            // catching the rest.
            let has_own_template = f
                .doc_comment
                .as_deref()
                .is_some_and(|d| d.contains("template"));
            if let Some(c) = class_name
                && has_own_template
                && let Some(doc) = f.doc_comment.as_deref().map(docblock::parse)
                && !doc.templates.is_empty()
            {
                let shallow: std::collections::HashMap<String, types::TemplateParam> = doc
                    .templates
                    .iter()
                    .map(|n| (n.clone(), types::TemplateParam::unbounded(n.clone())))
                    .collect();
                let resolve = |raw: &str| -> Type {
                    let ty = types::parse_type_string(raw);
                    let ty = mark_template_params(ty, &shallow);
                    resolve_names_in_type(ty, &self.name_ctx)
                };
                let templates: Vec<std::sync::Arc<str>> = doc
                    .templates
                    .iter()
                    .map(|s| crate::intern::intern(s))
                    .collect();
                let bounds: std::collections::HashMap<std::sync::Arc<str>, Type> = doc
                    .template_bounds
                    .iter()
                    .map(|(k, v)| (crate::intern::intern(k), resolve(v)))
                    .collect();
                let defaults: std::collections::HashMap<std::sync::Arc<str>, Type> = doc
                    .template_defaults
                    .iter()
                    .map(|(k, v)| (crate::intern::intern(k), resolve(v)))
                    .collect();
                let type_aliases = self
                    .registry
                    .classes
                    .get(c)
                    .map(|info| info.type_aliases.clone())
                    .unwrap_or_default();
                let display_name = format!("method {c}::{}()", f.name);
                self.check_template_declaration_validity(
                    &templates,
                    &bounds,
                    &defaults,
                    &type_aliases,
                    &display_name,
                    &label,
                    f.span,
                );
                // `method.shadowTemplate`: a method-level `@template` reusing
                // a name already declared on the enclosing class.
                if let Some(info) = self.registry.classes.get(c) {
                    let mut shadowed: Vec<&String> = doc
                        .templates
                        .iter()
                        .filter(|n| info.templates.iter().any(|t| t.as_ref() == n.as_str()))
                        .collect();
                    shadowed.sort();
                    for name in shadowed {
                        self.report(
                        label.to_string(),
                        format!(
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
        }

        // `(name, resolved type, AST span)` per parameter, collected
        // below in lockstep with `locals`'s per-branch loops - but before
        // either one's variadic-collects-into-`array` rewrite, since
        // `check_missing_function_typehints` wants the per-argument type
        // a genuinely untyped `...$args` would have (implicit `mixed`),
        // not the already-array-wrapped type `locals` needs for checking
        // the function's body.
        let mut resolved_params: Vec<(String, Type, Span)> = Vec::with_capacity(f.params.len());

        let mut locals: Locals = Locals::default();
        // Turns declaration-span tracking on for this whole function-
        // body check exactly when the LSP's probe is active - see
        // `Locals::track`'s doc comment. Every clone of `locals` from
        // here on carries this forward automatically.
        locals.set_track(self.probe_span.get().is_some());
        match sig {
            // The registry's FunctionSig already has PHPDoc types merged
            // in - using it here, instead of re-deriving from the raw
            // AST's native hints, is what makes an untyped-but-doc-
            // commented parameter usable inside its own function body,
            // not just checkable from the outside at call sites.
            Some(sig) => {
                for (param, ast_param) in sig.params.iter().zip(&f.params) {
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
                    let ty = match class_name {
                        Some(c) => self.resolve_pseudo(&param.ty, c),
                        None => param.ty.clone(),
                    };
                    // Mirrors real PHPStan's `ExistingClassesInTypehintsRule` -
                    // on the un-array-wrapped element type below (a
                    // variadic parameter's declared type, not the
                    // `array` it collects into), the same reasoning the
                    // Closure/arrow-fn arms in `checker/expr.rs` share.
                    let subject = format!("parameter ${} of {label}()", param.name);
                    // The own PHPDoc type too: a member the native hint
                    // excludes no longer reaches `ty`, but naming an
                    // unknown class is still wrong.
                    let checked = match &param.doc_ty {
                        Some(doc) => Type::Union(vec![ty.clone(), doc.clone()]),
                        None => ty.clone(),
                    };
                    self.check_type_class_names_exist(&checked, &subject, ast_param.span, &label);
                    self.check_type_trait_refs(
                        &ty,
                        &subject,
                        ast_param.span,
                        &label,
                        "parameter.trait",
                    );
                    // `ty` above is already `resolve_pseudo`'d.
                    self.check_no_parent_ref(
                        &ty,
                        &subject,
                        ast_param.span,
                        &label,
                        "parameter.noParent",
                    );
                    self.check_type_class_name_case(&param.case_refs, &label, ast_param.span);
                    self.check_type_internal_usage(&param.case_refs, &label, ast_param.span);
                    resolved_params.push((param.name.to_string(), ty.clone(), ast_param.span));
                    // `ParamSig::ty` holds a variadic parameter's own
                    // *element* type (the right shape for checking each
                    // individual argument at a call site) - inside the
                    // function's own body, real PHP always collects a
                    // variadic parameter into an `array` from the first
                    // statement onward, never the bare element type.
                    let ty = if param.variadic {
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty))
                    } else {
                        ty
                    };
                    self.probe_hit_at(ast_param.name_span, &ty);
                    locals.declare(param.name.to_string(), ty, ast_param.name_span);
                }
            }
            // This is exactly what every anonymous class method hits,
            // unconditionally: `lookup_own_signature` looks up
            // `self.registry.classes.get(c)` for `class_name =
            // Some(&decl.name)`, and an anonymous class's synthetic name
            // is never indexed into the registry - so `sig` is always
            // `None` there. Unlike the `Some(sig)` branch above, this one
            // must separately resolve a `use`-imported type-hint name to
            // its FQN, and resolve a `self`/`static`/`parent` pseudo-type
            // against the enclosing (possibly anonymous) class. Both are
            // fixed the same way, using `self.name_ctx` - the exact
            // per-file context real class declarations get resolved
            // against - since this branch (unlike the `Some` one) builds
            // each `Type` fresh from a raw string on every call anyway.
            //
            // `doc`/`resolve_type` below mirror `FunctionSig::from_decl`'s
            // param-resolution exactly, so an anonymous class's own
            // `/** @return array<string, object> */` above a fully-typed
            // method isn't silently discarded.
            None => {
                let doc = f.doc_comment.as_deref().map(docblock::parse);
                for (i, p) in f.params.iter().enumerate() {
                    // Same inline-`@var`-on-a-promoted-property fallback
                    // as `FunctionSig::from_decl`'s param loop.
                    let doc_ty: Option<String> = doc
                        .as_ref()
                        .and_then(|d| d.params.get(p.name.as_ref()))
                        .cloned()
                        .or_else(|| {
                            p.doc_comment
                                .as_deref()
                                .map(docblock::parse)
                                .and_then(|d| d.var_type)
                        });
                    let ty = resolve_type(doc_ty.as_deref(), p.type_hint.as_deref());
                    // This anonymous-class fallback path builds `ty`
                    // straight from raw text, bypassing `FunctionSig::
                    // from_decl`'s own `ParamSig::case_refs` population,
                    // so this needs the collection call done inline,
                    // right before `resolve_names_in_type` destroys the
                    // pre-resolution spelling.
                    let mut case_refs = Vec::new();
                    collect_class_name_case_refs(&ty, &self.name_ctx, &mut case_refs);
                    let ty = resolve_names_in_type(ty, &self.name_ctx);
                    // This anonymous-class param docblock fallback parses
                    // raw text independently of the registry's own
                    // pipeline, so it needs the identical `value-of<T>`-
                    // on-an-enum resolution `resolve_type` above already
                    // went through, so an unresolvable `value-of<T>`
                    // needs the same native-type fallback here too.
                    let native_ty = self.resolve_type_hint(p.type_hint.as_deref());
                    let ty = self
                        .registry
                        .resolve_deferred_value_of_now_with_native_fallback(ty, &native_ty);
                    // `doc_ty` being `None` means this exact declaration
                    // has nothing to say about this position - but an
                    // anonymous class relying on an implemented
                    // interface's real, non-`mixed` documented type still
                    // needs it backfilled here. Mirrors `Registry::
                    // flatten_phpdoc_inheritance_one_pass`'s three safety
                    // guards: `__construct` matches by name, everything
                    // else positionally, the same real-PHP-constructor-
                    // signature-compatibility distinction that pass draws.
                    let ty = if doc_ty.is_none() {
                        ancestor_sig_hint
                            .and_then(|s| {
                                if f.name.as_ref() == "__construct" {
                                    s.params
                                        .iter()
                                        .find(|ap| ap.name.as_ref() == p.name.as_ref())
                                } else {
                                    s.params.get(i)
                                }
                            })
                            .filter(|ap| {
                                ap.has_own_doc
                                    && Registry::safe_to_backfill_mixed(&ap.ty, &ty)
                                    && !Registry::type_mentions_foreign_scoped_template_param(
                                        &ap.ty, &f.name,
                                    )
                                    && Registry::safe_to_backfill_object_type(&ap.ty, &ty)
                            })
                            .map(|ap| ap.ty.clone())
                            .unwrap_or(ty)
                    } else {
                        ty
                    };
                    let ty = match class_name {
                        Some(c) => self.resolve_pseudo(&ty, c),
                        None => ty,
                    };
                    let subject = format!("parameter ${} of {label}()", p.name);
                    self.check_type_class_names_exist(&ty, &subject, p.span, &label);
                    self.check_type_trait_refs(&ty, &subject, p.span, &label, "parameter.trait");
                    self.check_no_parent_ref(&ty, &subject, p.span, &label, "parameter.noParent");
                    self.check_type_class_name_case(&case_refs, &label, p.span);
                    self.check_type_internal_usage(&case_refs, &label, p.span);
                    resolved_params.push((p.name.to_string(), ty.clone(), p.span));
                    let ty = if p.variadic {
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty))
                    } else {
                        ty
                    };
                    self.probe_hit_at(p.name_span, &ty);
                    locals.declare(p.name.to_string(), ty, p.name_span);
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
            locals.insert("this".to_string(), Type::Object(c.to_string(), Vec::new()));
        }

        // A signature's return type already defaults to Mixed when nothing
        // was declared, and checking a return statement against Mixed
        // never flags anything - so this doesn't need to distinguish "no
        // return type" from "declared mixed" the way the fallback below
        // does with an Option.
        // Captured (moved into) by the `.or_else` closure below when
        // `sig` is `None`. Stays empty (and unused) when `sig` is
        // `Some`, since the closure never runs in that case.
        let mut declared_return_case_refs_none_path: Vec<(
            std::sync::Arc<str>,
            std::sync::Arc<str>,
        )> = Vec::new();
        let declared_return = sig.map(|s| s.return_type.clone()).or_else(|| {
            // Same "the registry-side merge this branch otherwise skips"
            // fix as the param loop above, for the return position - an
            // anonymous class's `@return` docblock tag is only ever
            // reached here, never via `sig` (always `None` for one).
            let doc_return = f
                .doc_comment
                .as_deref()
                .map(docblock::parse)
                .and_then(|d| d.return_type);
            let ty = resolve_type(doc_return.as_deref(), f.return_type.as_deref());
            // Same "checker builds a Type straight from raw text,
            // bypassing FunctionSig::from_decl" pattern as the param
            // loop's identical fix above.
            collect_class_name_case_refs(
                &ty,
                &self.name_ctx,
                &mut declared_return_case_refs_none_path,
            );
            let ty = resolve_names_in_type(ty, &self.name_ctx);
            // The return-position sibling of the param loop's identical
            // fix above, including the same native-fallback need.
            let native_ty = self.resolve_type_hint(f.return_type.as_deref());
            let ty = self
                .registry
                .resolve_deferred_value_of_now_with_native_fallback(ty, &native_ty);
            // Same ancestor-backfill as the param loop above, for the
            // return position.
            let ty = if doc_return.is_none() {
                ancestor_sig_hint
                    .filter(|s| {
                        s.return_has_own_doc
                            && Registry::safe_to_backfill_mixed(&s.return_type, &ty)
                            && !Registry::type_mentions_foreign_scoped_template_param(
                                &s.return_type,
                                &f.name,
                            )
                            && Registry::safe_to_backfill_object_type(&s.return_type, &ty)
                    })
                    .map(|s| s.return_type.clone())
                    .unwrap_or(ty)
            } else {
                ty
            };
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
        let native_return = sig.map(|s| s.return_type_native.clone()).or_else(|| {
            let ty = f
                .return_type
                .as_deref()
                .map(types::parse_type_string)
                .map(|ty| resolve_names_in_type(ty, &self.name_ctx))?;
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
            .map(|s| s.return_case_refs.clone())
            .unwrap_or(declared_return_case_refs_none_path);
        // Mirrors real PHPStan's `ExistingClassesInTypehintsRule`'s
        // return-type check - the un-widened, exactly-as-declared type
        // (not the `?`-and-`resolve_pseudo`-adjusted `expected` the
        // falls-off-the-end check further down builds), same as every
        // other existence check in this function.
        let subject = format!("return type of {label}()");
        let declared = declared_return.clone().unwrap_or(Type::Mixed(false));
        let checked = match sig.and_then(|s| s.return_doc_ty.as_ref()) {
            Some(doc) => Type::Union(vec![declared, doc.clone()]),
            None => declared,
        };
        self.check_type_class_names_exist(&checked, &subject, f.span, &label);
        self.check_type_class_name_case(&declared_return_case_refs, &label, f.span);
        self.check_type_internal_usage(&declared_return_case_refs, &label, f.span);
        self.check_type_trait_refs(
            declared_return.as_ref().unwrap_or(&Type::Mixed(false)),
            &subject,
            f.span,
            &label,
            "return.trait",
        );
        // `return.noParent` - `declared_return` above is deliberately
        // not `resolve_pseudo`'d yet, so a fresh, one-off resolve here
        // is needed specifically to detect "this class has no real
        // parent" - a top-level function has no enclosing class `parent`
        // could meaningfully reference.
        if let Some(c) = class_name {
            let resolved_return =
                self.resolve_pseudo(declared_return.as_ref().unwrap_or(&Type::Mixed(false)), c);
            self.check_no_parent_ref(
                &resolved_return,
                &subject,
                f.span,
                &label,
                "return.noParent",
            );
        }
        // `@throws` tag validity - see `checker::throws`'s module doc
        // comment.
        if !self.facts_only
            && let Some(sig) = sig
            && let Some(throws) = &sig.throws
        {
            self.check_throws_tag_validity(throws, f.span, &label);
            // Mirrors real PHPStan's
            // `ThrowsVoidFunctionWithExplicitThrowPointRule`/
            // `ThrowsVoidMethodWithExplicitThrowPointRule` - needs no
            // opt-in flag: only ever fires when the docblock itself
            // explicitly claims "never throws," directly contradicted by
            // a real `throw` sitting right there in the body.
            if *throws == Type::Void
                && let Some(throw_span) = body_contains_explicit_throw(&f.body)
            {
                let kind_word = if class_name.is_some() {
                    "Method"
                } else {
                    "Function"
                };
                self.report(
                    label.to_string(),
                    format!(
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
        if !self.facts_only {
            self.check_attributes(&f.attrs, &label);
            // Target-validity, mirroring real PHPStan's split between
            // `MethodAttributesRule`/`FunctionAttributesRule` (one or the
            // other, never both, for this same choke point).
            if class_name.is_some() {
                self.check_attribute_targets(&f.attrs, ATTRIBUTE_TARGET_METHOD, "method", &label);
            } else {
                self.check_attribute_targets(
                    &f.attrs,
                    ATTRIBUTE_TARGET_FUNCTION,
                    "function",
                    &label,
                );
            }
            for p in &f.params {
                self.check_attributes(&p.attrs, &label);
                // `ParamAttributesRule`: a promoted param's own attributes
                // may target either a parameter or the property it also
                // declares - real PHP itself doesn't distinguish which one
                // an attribute "means" here, so either target is accepted.
                let (target, target_name) = if p.promoted_visibility.is_some() {
                    (
                        ATTRIBUTE_TARGET_PARAMETER | ATTRIBUTE_TARGET_PROPERTY,
                        "parameter or property",
                    )
                } else {
                    (ATTRIBUTE_TARGET_PARAMETER, "parameter")
                };
                self.check_attribute_targets(&p.attrs, target, target_name, &label);
            }
        }

        // Bound-enforcement: checked once here, against the declaration
        // itself, not at every call site - and deliberately outside the
        // dynamic-var-construct truncation below, since a bound
        // violation is about the signature as written, not something an
        // `extract()` in the body should be able to hide.
        if !self.facts_only {
            if let Some(sig) = sig {
                for (param, ast_param) in sig.params.iter().zip(&f.params) {
                    self.check_generic_bounds(&param.ty, &label, ast_param.span);
                }
            }
            self.check_generic_bounds(
                declared_return.as_ref().unwrap_or(&Type::Mixed(false)),
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
                && f.name != "__construct".into()
            {
                self.check_variance(sig, c, &label, f.span);
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
                sig.or(ancestor_sig_hint),
            );
        }

        let previous_class = self.current_class.take();
        self.current_class = class_name.map(|s| s.to_string());
        let previous_method = self.current_method.take();
        self.current_method = Some(f.name.to_string());
        // `checker::restricted_symbol_usage`'s own attribute-context
        // matching - see `current_class_attrs`'s own doc comment.
        let previous_class_attrs = std::mem::take(&mut self.current_class_attrs);
        self.current_class_attrs = class_name
            .and_then(|c| self.registry.classes.get(c))
            .map(|info| info.attrs.clone())
            .unwrap_or_default();
        let previous_method_attrs = std::mem::take(&mut self.current_method_attrs);
        self.current_method_attrs = f
            .attrs
            .iter()
            .map(|a| crate::intern::intern(&a.name))
            .collect();
        // Mirrors real PHPStan's `DeprecatedScopeHelper` - suppresses
        // every deprecated-usage diagnostic from within a deprecated
        // function/method, class, or trait's own body.
        let previous_scope_deprecated = self.current_scope_deprecated;
        self.current_scope_deprecated = sig.is_some_and(|s| s.deprecated.is_some())
            || self.current_class.as_deref().is_some_and(|c| {
                self.registry
                    .classes
                    .get(c)
                    .is_some_and(|i| i.deprecated.is_some())
            })
            || self.current_trait.as_deref().is_some_and(|t| {
                self.registry
                    .classes
                    .get(t)
                    .is_some_and(|i| i.deprecated.is_some())
            });

        let diagnostics_before = self.diagnostics.len();
        let previous_dynamic_flag = self.saw_dynamic_var_construct;
        self.saw_dynamic_var_construct = false;
        let previous_generator_flag = self.current_function_is_generator;
        self.current_function_is_generator = body_contains_yield(&f.body);
        let previous_declared_return_type = self.current_declared_return_type.take();
        self.current_declared_return_type =
            declared_return.as_ref().map(|t| match &self.current_class {
                Some(c) => self.resolve_pseudo(t, c),
                None => t.clone(),
            });
        let previous_declared_throws_type = self.current_declared_throws_type.take();
        self.current_declared_throws_type = sig.and_then(|s| s.throws.clone());
        let previous_goto_targets = std::mem::replace(
            &mut self.current_function_goto_targets,
            collect_goto_targets(&f.body),
        );
        // See `Checker::current_function_throw_points`'s doc comment.
        let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
        let previous_unannotated_call =
            std::mem::take(&mut self.current_function_has_unannotated_call);
        let previous_explicit_throw_points =
            std::mem::take(&mut self.current_function_explicit_throw_points);
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
                for param in &sig.params {
                    if param.by_ref {
                        let name: std::sync::Arc<str> = std::sync::Arc::from(param.name.as_ref());
                        if let Some(out_ty) = &param.out_ty {
                            self.current_function_by_ref_params_with_tag
                                .insert(name.clone(), out_ty.clone());
                        }
                        let check_ty = param.out_ty.clone().unwrap_or_else(|| param.ty.clone());
                        self.current_function_by_ref_params.insert(name, check_ty);
                    }
                }
            }
            None => {
                for p in &f.params {
                    if p.by_ref {
                        let ty = self.resolve_type_hint(p.type_hint.as_deref());
                        self.current_function_by_ref_params
                            .insert(p.name.clone(), ty);
                    }
                }
            }
        }
        self.check_generator_return_type(native_return.as_ref(), f.span, &label);

        // `check_block`, not a bare loop over `check_stmt`: a function's
        // top-level body is exactly as eligible for `deadCode.unreachable`
        // as any nested `{ ... }` block is - it just needs to be routed
        // through the one place that check lives.
        self.check_block(&f.body, &mut locals, declared_return.as_ref(), &label);

        // A function that falls off the end of its body with no `return`
        // at all implicitly returns `null` too - equally a real mismatch
        // against a non-nullable, non-void, non-mixed declared return
        // type: `always_terminates` (already used for guard-clause
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
        // `always_terminates`'s `Block` arm uses for a nested `{ ... }` -
        // `f.body` just isn't itself one `Stmt`, so it needs the same
        // check spelled out here directly. A generator stays exempt from
        // this whole check unless its declared `Generator<TKey, TValue,
        // TSend, TReturn>` carries a real, non-Mixed/Void TReturn - in
        // which case falling off the end is exactly as real a miss as it
        // is for an ordinary function, just checked against TReturn
        // instead of the outer Generator type itself.
        if !self.facts_only {
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
            let body_falls_off_end = !f.body.iter().any(|s| self.always_terminates(s, &locals));
            if body_falls_off_end {
                self.record_by_ref_exit_snapshot(&locals);
            }
            if !self.current_function_is_generator && body_falls_off_end {
                self.current_function_return_points.push(Type::Void);
                if native_return.is_none() {
                    self.current_function_return_points.push(Type::Null);
                }
            }
            if let Some(expected) = &declared_return
                && (!f.body.is_empty() || (self.registry.check_empty_method_bodies && is_concrete))
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
                if expected == Type::Never {
                    self.report(
                        label.clone(),
                        "should never return, but a return statement was reached".to_string(),
                        f.span,
                        3,
                        "return.never",
                    );
                } else if let Some(level) =
                    missing_return_level(&expected, f.return_type.is_some(), is_generator_treturn)
                {
                    let mut builder = DiagnosticBuilder::new(
                        label.clone(),
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
            if self.registry.check_never_return_suggestions
                && !f.body.is_empty()
                && !self.current_function_is_generator
                && !body_falls_off_end
                && !self.current_function_saw_explicit_return
                && matches!(declared_return, Some(Type::Void) | Some(Type::Mixed(_)))
            {
                self.report(
                    label.clone(),
                    "could be declared to return never - it never returns normally".to_string(),
                    f.return_type_span.unwrap_or(f.span),
                    5,
                    "return.neverCandidate",
                );
            }

            // Needs the same body-level extract()/eval() guard as everything
            // else checked in this function - placed before the truncation
            // below (rather than after) so a constructor that calls
            // extract()/eval() has this diagnostic discarded along with
            // every other one from its own body.
            self.check_unused_constructor_parameters(f, class_name, &label);

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
            if !f.body.is_empty() || (self.registry.check_empty_method_bodies && is_concrete) {
                let subject = match class_name {
                    Some(c) => format!("Method {c}::{}()", f.name),
                    None => format!("Function {}()", f.name),
                };
                let throws_tag = sig.and_then(|s| s.throws.as_ref());
                if let Some(throws) = throws_tag {
                    let should_check = match (class_name, sig) {
                        (Some(c), Some(s)) => self.should_check_too_wide_method_throw_type(
                            c,
                            &f.name,
                            s.visibility.clone(),
                        ),
                        // `TooWideFunctionThrowTypeRule` (a plain top-level
                        // function, or a method whose signature somehow
                        // isn't indexed) has no visibility/finality gate -
                        // see `should_check_too_wide_method_throw_type`'s
                        // doc comment.
                        _ => true,
                    };
                    if should_check {
                        self.check_too_wide_throw_type(throws, &subject, f.span, &label);
                    }
                }
                self.check_missing_checked_exceptions(throws_tag, &subject, &label);

                // Same "has a real body at all" gate, same choke point as
                // the throw-type check above.
                if let Some(declared) = &declared_return {
                    let declared = match &self.current_class {
                        Some(c) => self.resolve_pseudo(declared, c),
                        None => declared.clone(),
                    };
                    let should_check = match (class_name, sig) {
                        (Some(c), Some(s)) => self.should_check_too_wide_method_return_type(
                            c,
                            &f.name,
                            s.visibility.clone(),
                        ),
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
                        let is_private = sig.is_some_and(|s| s.visibility == Visibility::Private);
                        self.check_too_wide_return_type(
                            &declared,
                            is_first_declaration,
                            is_private,
                            &subject,
                            f.span,
                            &label,
                        );
                    }
                }
                // Real `ParameterOutExecutionEndTypeRule` has no
                // visibility/finality gate of its own (unlike the two
                // too-wide rules) - runs whenever a by-ref parameter
                // actually declares `@param-out` at all.
                self.check_by_ref_execution_end(&label, f.span);
                let should_check_by_ref_too_wide = match (class_name, sig) {
                    (Some(c), Some(s)) => {
                        self.should_check_too_wide_by_ref_type(c, &f.name, s.visibility.clone())
                    }
                    _ => true,
                };
                if should_check_by_ref_too_wide {
                    self.check_too_wide_by_ref_type(&label, f.span);
                }
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
        self.current_function_has_unannotated_call = previous_unannotated_call;
        self.current_function_explicit_throw_points = previous_explicit_throw_points;
        self.current_function_return_points = previous_return_points;
        self.current_function_saw_explicit_return = previous_saw_explicit_return;
        self.current_function_by_ref_params = previous_by_ref_params;
        self.current_function_by_ref_exit_snapshots = previous_by_ref_exit_snapshots;
        self.current_function_by_ref_params_with_tag = previous_by_ref_params_with_tag;
        self.current_function_goto_targets = previous_goto_targets;

        self.current_class = previous_class;
        self.current_method = previous_method;
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
        let message = format!(
            "{kind_word} {label}() uses native union types but they're supported only on PHP 8.0 and later."
        );
        self.check_native_union_types_supported_raw(
            f.params.iter().map(|p| p.type_hint.as_deref()),
            f.return_type.as_deref(),
            &message,
            f.span,
            label,
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
        param_hints: impl Iterator<Item = Option<&'p str>>,
        return_hint: Option<&str>,
        message: &str,
        span: Span,
        label: &str,
    ) {
        let Some(version) = self.registry.php_version else {
            return;
        };
        if PhpFeature::NativeUnionTypes.is_available(version) {
            return;
        }
        for hint in param_hints {
            if is_bare_native_union(hint) {
                self.report(
                    label.to_string(),
                    message.to_string(),
                    span,
                    0,
                    "parameter.unionTypeNotSupported",
                );
                return;
            }
        }
        if is_bare_native_union(return_hint) {
            self.report(
                label.to_string(),
                message.to_string(),
                span,
                0,
                "return.unionTypeNotSupported",
            );
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
                p.type_hint.as_deref(),
                &format!(
                    "Parameter ${} of {label}() has unresolvable native type.",
                    p.name
                ),
                "parameter.unresolvableNativeType",
                p.name_span,
                label,
            );
        }
        self.check_unresolvable_native_type(
            f.return_type.as_deref(),
            &format!("{label}() has unresolvable native return type."),
            "return.unresolvableNativeType",
            f.span,
            label,
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
        hint: Option<&str>,
        message: &str,
        identifier: &'static str,
        span: Span,
        label: &str,
    ) {
        let Some(version) = self.registry.php_version else {
            return;
        };
        if !PhpFeature::PureIntersectionTypes.is_available(version) {
            return;
        }
        let Some(hint) = hint else {
            return;
        };
        if !hint.contains('&') {
            return;
        }
        let Type::Intersection(members) = self.resolve_type_hint(Some(hint)) else {
            return;
        };
        let mut names = Vec::with_capacity(members.len());
        for m in &members {
            let Type::Object(name, _) = m else {
                return;
            };
            names.push(name.clone());
        }
        if self.registry.is_intersection_possible(&names) {
            return;
        }
        self.report_builder(
            DiagnosticBuilder::new(label.to_string(), message.to_string(), span, 0, identifier)
                .non_ignorable(),
        );
    }

    /// Mirrors real PHPStan's `parameter.implicitlyNullable` (PHP 8.4) -
    /// a typed parameter whose only source of nullability is an
    /// `= null` default (`Foo $x = null`, not `?Foo $x = null`/a union
    /// already naming `null`/`mixed`). `$order` in
    /// `FunctionDefinitionCheck::checkImplicitlyNullableType` is the
    /// parameter's 1-based position among every parameter, typed or not -
    /// `i` below mirrors that exactly, not a typed-only counter.
    fn check_implicitly_nullable_params(&mut self, f: &FunctionDecl, label: &str) {
        let Some(version) = self.registry.php_version else {
            return;
        };
        if !PhpFeature::ImplicitlyNullableParameterDeprecated.is_available(version) {
            return;
        }
        for (i, p) in f.params.iter().enumerate() {
            let Some(hint) = p.type_hint.as_deref() else {
                continue;
            };
            if hint.starts_with('?') || hint.contains('&') {
                continue;
            }
            if hint.eq_ignore_ascii_case("mixed") || hint.eq_ignore_ascii_case("null") {
                continue;
            }
            if hint
                .split('|')
                .any(|member| member.eq_ignore_ascii_case("null"))
            {
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
            self.report(
                label.to_string(),
                format!(
                    "Deprecated in PHP 8.4: Parameter #{} ${} ({hint}) is implicitly nullable via default value null.",
                    i + 1,
                    p.name,
                ),
                p.name_span,
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
    fn check_required_parameter_after_optional(&mut self, f: &FunctionDecl, label: &str) {
        let Some(version) = self.registry.php_version else {
            return;
        };
        if !PhpFeature::RequiredParameterAfterOptionalDeprecated.is_available(version) {
            return;
        }
        let mut optional_param: Option<&str> = None;
        for p in &f.params {
            if let Some(optional) = optional_param
                && p.default.is_none()
                && !p.variadic
            {
                self.report(
                    label.to_string(),
                    format!(
                        "Deprecated in PHP 8.0: Required parameter ${} follows optional parameter ${optional}.",
                        p.name,
                    ),
                    p.name_span,
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
    /// `param_ty` mirrors real PHPStan's own explicit `TemplateTypeHelper::
    /// resolveToBounds` step - without it, a `@template T`-typed
    /// parameter's own default would be checked against the bare,
    /// unresolved `TemplateParam` itself, which `registry::is_assignable`
    /// has no dedicated rule for on the *super*-type side (only the
    /// *sub*-type side, `(Type::TemplateParam(tp), _) => ... self.
    /// is_assignable(&tp.bound, sup)`, already used elsewhere for the
    /// opposite direction). Deliberately shallow - only a bare top-level
    /// `T $x = ...`, not real PHPStan's own recursive substitution
    /// through the whole type tree (e.g. a template nested inside
    /// `array<T> $x = []`) - a disclosed narrower scope: a default value
    /// is overwhelmingly a scalar literal/array/`null`, where a *nested*
    /// unresolved template essentially never actually decides whether
    /// that literal is assignable to it.
    ///
    /// The real rule calls `Type::accepts` directly rather than through
    /// `RuleLevelHelper`, and a `mixed` value is accepted everywhere
    /// there, so a default whose type isn't known is never reported.
    fn check_default_parameter_types(
        &mut self,
        f: &FunctionDecl,
        sig: &FunctionSig,
        class_name: Option<&str>,
        label: &str,
    ) {
        for (i, p) in f.params.iter().enumerate() {
            let Some(default) = &p.default else {
                continue;
            };
            if p.variadic {
                continue;
            }
            let Some(param_sig) = sig.params.get(i) else {
                continue;
            };
            let param_ty = match &param_sig.ty {
                Type::TemplateParam(tp) => tp.bound.as_ref(),
                other => other,
            };
            let default_ty = self.infer_expr(default, &Locals::default());
            if matches!(default_ty, Type::Mixed(_))
                || self.registry.is_assignable(&default_ty, param_ty)
            {
                continue;
            }
            let target = match class_name {
                Some(c) => format!("method {c}::{}()", f.name),
                None => format!("function {}()", f.name),
            };
            self.report(
                label.to_string(),
                format!(
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
        body: &[Stmt],
        has_native_return: bool,
        declared_return: Option<&Type>,
        locals: &Locals,
        span: Span,
        label: &str,
    ) {
        if !has_native_return || self.current_function_is_generator {
            return;
        }
        let Some(expected) = declared_return else {
            return;
        };
        if body.iter().any(|s| self.always_terminates(s, locals)) {
            return;
        }
        let expected = match &self.current_class {
            Some(c) => self.resolve_pseudo(expected, &c.clone()),
            None => expected.clone(),
        };
        if expected == Type::Never {
            self.report_builder(
                DiagnosticBuilder::new(
                    label.to_string(),
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
                label.to_string(),
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
    if matches!(expected, Type::Void) {
        return None;
    }
    if matches!(expected, Type::Mixed(_)) && (!has_native_hint || is_generator_treturn) {
        return None;
    }
    let contains_null = match expected {
        Type::Null => true,
        Type::Union(members) => members.contains(&Type::Null),
        _ => false,
    };
    if is_generator_treturn || (!has_native_hint && contains_null) {
        Some(2)
    } else {
        Some(0)
    }
}

/// Whether a native type-hint (raw `Param::type_hint`/`FunctionDecl::
/// return_type` text) is a bare, top-level union (`int|string`) - `?`-
/// prefixed nullable-single-type and any type containing `&` (an
/// intersection, which can't also be a union) are excluded first, since
/// `parse_type_hint`'s construction never mixes `&` into a `|`-joined
/// list in a way that would make a naive `contains('|')` check safe
/// without that exclusion.
fn is_bare_native_union(hint: Option<&str>) -> bool {
    hint.is_some_and(|h| !h.contains('&') && h.contains('|'))
}
