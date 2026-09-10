use crate::ast::*;
use crate::checker::{
    ATTRIBUTE_TARGET_FUNCTION, ATTRIBUTE_TARGET_METHOD, ATTRIBUTE_TARGET_PARAMETER,
    ATTRIBUTE_TARGET_PROPERTY, Checker, Locals, body_contains_explicit_throw, body_contains_yield,
    collect_goto_targets, type_allows_generator_return,
};
use crate::docblock;
use crate::registry::{FunctionSig, Registry, resolve_names_in_type, resolve_type};
use crate::token::Span;
use crate::types::{self, Type};

impl<'a> Checker<'a> {
    /// Phase 13 Finding E: a trait's own method bodies are checked once
    /// per class that `use`s it, not standalone - `flatten_traits`
    /// (`registry.rs`) already merges a trait's own methods/properties
    /// into every using class's own `ClassInfo` before checking starts,
    /// so re-checking with `current_class` set to a real host's FQN
    /// means every existing lookup (`lookup_method`/`lookup_property`)
    /// transparently sees that host's true member set - a trait method
    /// calling `$this->hostOnlyMethod()` is valid PHP (the trait's
    /// author expects any using class to provide it), but false-
    /// positived as "undefined method" when checked standalone. A trait
    /// with no user anywhere in the project (an orphaned/library trait)
    /// still gets checked standalone exactly as before - a real
    /// regression otherwise, since today's behavior checks every
    /// trait's body unconditionally.
    pub(crate) fn check_trait_methods(&mut self, fqn: &str, c: &ClassDecl) {
        let hosts = self.registry.classes_using_trait(fqn);
        let contexts: Vec<String> = if hosts.is_empty() {
            vec![fqn.to_string()]
        } else {
            hosts
        };
        let previous_trait = self.current_trait.take();
        self.current_trait = Some(fqn.to_string());
        let start = self.diagnostics.len();
        for host in contexts {
            for m in &c.methods {
                self.check_function(&m.function, Some(&host), m.is_static, !m.is_abstract);
            }
        }
        self.current_trait = previous_trait;
        // A trait used by 2+ classes gets checked 2+ times - a
        // genuinely host-independent bug in the trait's own body (no
        // `$this->...` involved at all) would otherwise be reported
        // once per host, since diagnostics are only ever sorted, never
        // deduplicated elsewhere. A real host-specific finding (missing
        // on one host but not another) survives this, since its own
        // message genuinely differs per host. This local sort exists
        // only to bring duplicates adjacent for `dedup_by` (which only
        // ever removes consecutive matches) - the final, whole-program
        // sort downstream of `check_program` reorders everything
        // correctly regardless of this block's own local order.
        self.diagnostics[start..].sort_by(|a, b| {
            (a.span.start.line, a.span.start.col, &a.message).cmp(&(
                b.span.start.line,
                b.span.start.col,
                &b.message,
            ))
        });
        self.diagnostics
            .dedup_by(|a, b| a.span == b.span && a.message == b.message);
    }

    /// Looks up this exact declaration's own resolved signature in the
    /// registry (a free function by name, or a method by class+name).
    /// Returns a reference tied to the registry's lifetime, not `&self`'s,
    /// so holding it doesn't conflict with the `&mut self` calls that
    /// follow once its params/return type have been copied into `locals`.
    /// A real bug this found: for a top-level function, `Registry::index`
    /// keys `self.registry.functions` by the function's fully-qualified
    /// name (Phase 9.5), but a bare `self.registry.functions.get(&f.name)`
    /// here would look it up by its raw, unqualified AST name - a
    /// namespaced function's own signature (with correctly-resolved
    /// param/return types) was silently never found, falling through to
    /// `check_function`'s `None` branch, which re-derives types straight
    /// from native hints with *no* namespace resolution applied at all.
    /// Methods don't share this bug: `class_name` is already the FQN by
    /// the time it gets here (`check_program`'s own `StmtKind::ClassDecl`
    /// handling resolves it), and a method's own name is never namespace-
    /// qualified in the first place - only the class it's declared in is.
    pub(crate) fn lookup_own_signature(
        &self,
        f: &FunctionDecl,
        class_name: Option<&str>,
    ) -> Option<&'a FunctionSig> {
        match class_name {
            Some(c) => self.registry.classes.get(c)?.methods.get(&f.name),
            None => self
                .registry
                .functions
                .get(&self.name_ctx.declared_name(&f.name)),
        }
    }

    /// Phase 70: `YieldInGeneratorRule`, narrowed to its declared-return-
    /// type-compatibility half (the full `Generator<K, V, TSend,
    /// TReturn>` generic shape - checking each `yield`'s own key/value
    /// against `K`/`V`, a sent-in value's type against `TSend`, and a
    /// `return` statement inside a generator against `TReturn` - is left
    /// for later; see ROADMAP.md). Confirmed directly via `php`/`php -l`
    /// as a real, unconditional fatal error at *compile* time (not just
    /// first-call): `function f(): int { yield 1; }` fails to parse at
    /// all with "Generator return type must be a supertype of Generator,
    /// int given" - real PHP's own check is lenient about *which* return
    /// type shape satisfies this (confirmed: `?Generator`, `iterable`,
    /// and even the deliberately-unrelated-seeming `int|Generator` are
    /// all accepted, since only ONE member of a union needs to be
    /// Generator-compatible), which `type_allows_generator_return`
    /// mirrors exactly rather than guessing at PHPStan's own stricter,
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
        self.check_function_with_ancestor_hint(f, class_name, is_static, is_concrete, None);
    }

    /// `check_function`'s real body - `ancestor_sig_hint` (Phase 90) is
    /// `Some` only for an anonymous class's own method, where it's the
    /// first ancestor's own signature found by walking `decl.extends`/
    /// `decl.implements` directly at the call site (`ExprKind::
    /// NewAnonymousClass`'s own handling in `checker/expr.rs`) - an
    /// anonymous class is never indexed into the registry under any
    /// stable name (see `lookup_own_signature`'s own doc comment), so
    /// `sig` below is always `None` for one, and every registry-aware
    /// check in this function (bound/variance-enforcement, the per-param
    /// type resolution above) stays exactly as lenient-by-omission as it
    /// already was. `ancestor_sig_hint` is deliberately used *only* at
    /// `check_missing_function_typehints`'s own call site below, not
    /// substituted in for `sig` throughout this whole function - correct
    /// there (that check only ever reads a signature's own docblock-
    /// provenance bits, `has_own_doc`/`explicit_mixed_hint`, which an
    /// ancestor's genuinely still is), but not obviously safe for every
    /// other `Some(sig)`-gated check here (e.g. variance/bound
    /// enforcement checking the anonymous class's *own* body against a
    /// signature it never actually redeclared).
    pub(crate) fn check_function_with_ancestor_hint(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        is_static: bool,
        is_concrete: bool,
        ancestor_sig_hint: Option<&FunctionSig>,
    ) {
        let sig = self.lookup_own_signature(f, class_name);
        // Moved up from its original position right before the bound-
        // enforcement block below - Phase 48's own per-parameter/return
        // existence check (right below, inside both match arms) needs a
        // real label to attach its diagnostics to too, and nothing
        // between here and there ever depended on `label` not existing
        // yet.
        let label = match class_name {
            Some(c) => format!("{c}::{}", f.name),
            None => f.name.clone(),
        };
        // A method's own docblock is already covered by `check_unknown_
        // phpstan_tags_in_class`'s own per-class walk (`checker.rs`'s own
        // class-declaration loop) - only a genuine top-level function
        // needs it here, or every method would get double-checked.
        if class_name.is_none() {
            self.check_unknown_phpstan_tags_on(&label, f.doc_comment.as_deref(), f.name_span);
        }
        self.check_declared_purity_tag(f, sig, class_name.is_some(), &label);
        self.check_declared_param_callable_tags(f, &label);

        // Phase 58: `(name, resolved type, AST span)` per parameter,
        // collected below in lockstep with `locals`'s own per-branch
        // loops - but *before* either one's variadic-collects-into-
        // `array` rewrite, since `check_missing_function_typehints`
        // wants the per-argument type a genuinely untyped `...$args`
        // would have (implicit `mixed`), not the already-array-wrapped
        // type `locals` itself needs for checking the function's body.
        let mut resolved_params: Vec<(String, Type, Span)> = Vec::with_capacity(f.params.len());

        let mut locals: Locals = Locals::default();
        // Phase 17 follow-up: turns declaration-span tracking on for this
        // whole function-body check exactly when the LSP's own probe is
        // active - see `Locals::track`'s own doc comment. Every clone of
        // `locals` from here on (every branch/loop this function's body
        // contains) carries this forward automatically.
        locals.set_track(self.probe_span.is_some());
        match sig {
            // The registry's FunctionSig already has PHPDoc types merged in
            // (Phase 2) - using it here, instead of re-deriving from the
            // raw AST's native hints, is what makes an untyped-but-
            // doc-commented parameter usable inside its own function body,
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
                    // Phase 48: `ExistingClassesInTypehintsRule`'s own
                    // check - on the un-array-wrapped element type below
                    // (a variadic parameter's declared type, not the
                    // `array` it collects into), the same reasoning the
                    // Closure/arrow-fn arms in `checker/expr.rs` share.
                    self.check_type_class_names_exist(
                        &ty,
                        &format!("parameter ${} of {label}()", param.name),
                        ast_param.span,
                        &label,
                    );
                    resolved_params.push((param.name.clone(), ty.clone(), ast_param.span));
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
                    locals.declare(param.name.clone(), ty, ast_param.name_span);
                }
            }
            // Meant as a "shouldn't happen, but degrade rather than
            // panic" fallback (this function had to be indexed to be
            // reachable here at all) - except it's exactly what every
            // anonymous class method hits, unconditionally: `lookup_own_
            // signature` looks up `self.registry.classes.get(c)` for
            // `class_name = Some(&decl.name)`, and an anonymous class's
            // synthetic name is never indexed into the registry (see
            // `ExprKind::NewAnonymousClass`'s own comment) - so `sig` is
            // always `None` there. Unlike the `Some(sig)` branch above,
            // this one never resolved a `use`-imported type-hint name to
            // its FQN (Phase 22 Finding 5 - confirmed directly from a
            // real diagnostic's own wording: "expected ResponseInterface,
            // got Nyholm\Psr7\Response", the expected side printing bare/
            // unqualified where a resolved one would be fully qualified),
            // nor resolved a `self`/`static`/`parent` pseudo-type against
            // the enclosing (possibly anonymous) class the way line 385
            // above does. Both fixed the same way, using `self.name_ctx` -
            // the exact per-file context real class declarations get
            // resolved against - since this branch (unlike the `Some`
            // one) builds each `Type` fresh from a raw string on every
            // call anyway, with no immutable-AST obstacle to work around.
            //
            // Phase 91: this branch used to build `ty` from `p.type_hint`
            // alone, never once looking at `f.doc_comment` - real corpus
            // false positive: an anonymous class's own `/** @return
            // array<string, object> */` above a fully-typed `getConnections
            // (): array` was silently discarded, since nothing here ever
            // parsed it, leaving `array`'s own value type looking
            // implicitly `Mixed` even though the source spelled out a real
            // one. `doc`/`resolve_type` below mirror `FunctionSig::
            // from_decl`'s own param-resolution exactly (see that
            // function's own `doc_ty`/`resolve_type` call) - the one
            // registry-side merge step this whole branch otherwise skips.
            None => {
                let doc = f.doc_comment.as_deref().map(docblock::parse);
                for (i, p) in f.params.iter().enumerate() {
                    // Phase 93: same inline-`@var`-on-a-promoted-property
                    // fallback as `FunctionSig::from_decl`'s own param
                    // loop - see that one's doc comment.
                    let doc_ty: Option<String> = doc
                        .as_ref()
                        .and_then(|d| d.params.get(&p.name))
                        .cloned()
                        .or_else(|| {
                            p.doc_comment
                                .as_deref()
                                .map(docblock::parse)
                                .and_then(|d| d.var_type)
                        });
                    let ty = resolve_type(doc_ty.as_deref(), p.type_hint.as_deref());
                    let ty = resolve_names_in_type(ty, &self.name_ctx);
                    // Phase 80 follow-up: see `Checker::resolve_type_hint`'s
                    // own doc comment - this anonymous-class param
                    // docblock fallback parses raw text independently of
                    // the registry's own pipeline too, so it needs the
                    // identical `value-of<T>`-on-an-enum resolution -
                    // `resolve_type` just above already went through the
                    // same native-reconciliation `FunctionSig::from_decl`
                    // does, so an unresolvable `value-of<T>` needs the
                    // same native-type fallback here too.
                    let native_ty = self.resolve_type_hint(p.type_hint.as_deref());
                    let ty = self
                        .registry
                        .resolve_deferred_value_of_now_with_native_fallback(ty, &native_ty);
                    // Phase 92: `doc_ty` being `None` means this exact
                    // declaration has nothing of its own to say about
                    // this position - but an anonymous class relying on
                    // an implemented interface's own real, non-`mixed`
                    // documented type (Phase 90's own `explicit_mixed_
                    // hint` fallback only ever suppresses a *mixed*
                    // finding, never backfills a *specific* one) never
                    // got the actual type at all. Mirrors `Registry::
                    // flatten_phpdoc_inheritance_one_pass`'s own three
                    // safety guards exactly (see their own doc comments)
                    // - `__construct` matches by name, everything else
                    // positionally, the same real-PHP-constructor-
                    // signature-compatibility distinction that pass
                    // draws. Real corpus false positive: `new readonly
                    // class(...) implements MailRecipientRepositoryInterface
                    // { public function findAdminsAndWebmasters(array
                    // $userStatuses, ...): array {...} }`, no own doc,
                    // relying entirely on the interface's own `@return
                    // list<MailRecipient>`.
                    let ty = if doc_ty.is_none() {
                        ancestor_sig_hint
                            .and_then(|s| {
                                if f.name == "__construct" {
                                    s.params.iter().find(|ap| ap.name == p.name)
                                } else {
                                    s.params.get(i)
                                }
                            })
                            .filter(|ap| {
                                ap.has_own_doc
                                    && Registry::safe_to_backfill_mixed(&ap.ty, &ty)
                                    && !Registry::type_mentions_template_param(&ap.ty)
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
                    self.check_type_class_names_exist(
                        &ty,
                        &format!("parameter ${} of {label}()", p.name),
                        p.span,
                        &label,
                    );
                    resolved_params.push((p.name.clone(), ty.clone(), p.span));
                    let ty = if p.variadic {
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty))
                    } else {
                        ty
                    };
                    locals.declare(p.name.clone(), ty, p.name_span);
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
        let declared_return = sig.map(|s| s.return_type.clone()).or_else(|| {
            // Phase 91: same "the registry-side merge this branch
            // otherwise skips" fix as the param loop above, for the
            // return position - an anonymous class's own `@return`
            // docblock tag is only ever reached here, never via `sig`
            // (always `None` for one).
            let doc_return = f
                .doc_comment
                .as_deref()
                .map(docblock::parse)
                .and_then(|d| d.return_type);
            let ty = resolve_type(doc_return.as_deref(), f.return_type.as_deref());
            let ty = resolve_names_in_type(ty, &self.name_ctx);
            // Phase 80 follow-up: see `Checker::resolve_type_hint`'s own
            // doc comment - the return-position sibling of the param
            // loop's identical fix just above, including the same
            // native-fallback need.
            let native_ty = self.resolve_type_hint(f.return_type.as_deref());
            let ty = self
                .registry
                .resolve_deferred_value_of_now_with_native_fallback(ty, &native_ty);
            // Phase 92: same ancestor-backfill as the param loop above,
            // for the return position - see its own doc comment.
            let ty = if doc_return.is_none() {
                ancestor_sig_hint
                    .filter(|s| {
                        s.return_has_own_doc
                            && Registry::safe_to_backfill_mixed(&s.return_type, &ty)
                            && !Registry::type_mentions_template_param(&s.return_type)
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
        // Phase 70: the *native*-only counterpart of `declared_return`
        // just above - `check_generator_return_type` needs it
        // specifically, not the PHPDoc-refined type `declared_return`
        // carries everywhere else in this function. Real PHP's own
        // "Generator return type must be a supertype of Generator"
        // fatal error is checked against the native declaration alone;
        // a `@return Transformation[]` docblock on a `: iterable`-typed
        // generator (a real, common shape - confirmed via
        // vendor/doctrine/inflector's own `Inflectible::getSingular()`)
        // is describing what iterating the real Generator/iterable
        // *produces*, not redeclaring the native type at all - using
        // `declared_return` here produced 84 false positives against
        // the Piwigo corpus before this fix (every docblock-typed-as-
        // array generator in vendor/doctrine/inflector alone).
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
        // Phase 48: `ExistingClassesInTypehintsRule`'s own return-type
        // check - the un-widened, exactly-as-declared type (not the
        // `?`-and-`resolve_pseudo`-adjusted `expected` the falls-off-the-
        // end check further down builds), same as every other existence
        // check in this function.
        self.check_type_class_names_exist(
            declared_return.as_ref().unwrap_or(&Type::Mixed),
            &format!("return type of {label}()"),
            f.span,
            &label,
        );
        // Phase 42's own self-contained first slice: `@throws` tag
        // validity - see `checker::throws`'s own module doc comment.
        if let Some(sig) = sig
            && let Some(throws) = &sig.throws
        {
            self.check_throws_tag_validity(throws, f.span, &label);
            // `ThrowsVoidFunctionWithExplicitThrowPointRule`/
            // `ThrowsVoidMethodWithExplicitThrowPointRule` - needs no
            // opt-in flag (unlike the throw-type-covariance rule family
            // this same phase's ROADMAP entry explicitly calls out as
            // needing one): only ever fires when the docblock *itself*
            // explicitly claims "never throws," directly contradicted by
            // a real `throw` sitting right there in the body - no risk
            // of the inconsistent-@throws-tagging false-positive class
            // the covariance rule would have (declined this pass,
            // pending corpus verification - see ROADMAP.md).
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

        // Phase 37: this one `check_function` is the single choke point
        // every function-shaped declaration passes through regardless of
        // how it's reached (a top-level function, an ordinary method, a
        // trait method re-checked once per host, a declaration nested
        // inside a block) - checking `f.attrs`/each parameter's own attrs
        // here covers all of them at once, rather than duplicating the
        // same two loops at every one of those call sites.
        self.check_attributes(&f.attrs, &label);
        // Phase 37 follow-up: target-validity, mirroring real PHPStan's
        // own split between `MethodAttributesRule`/`FunctionAttributesRule`
        // (one or the other, never both, for this same choke point).
        if class_name.is_some() {
            self.check_attribute_targets(&f.attrs, ATTRIBUTE_TARGET_METHOD, "method", &label);
        } else {
            self.check_attribute_targets(&f.attrs, ATTRIBUTE_TARGET_FUNCTION, "function", &label);
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

        // Phase 10.5's bound-enforcement: checked once here, against the
        // declaration itself, not at every call site - and deliberately
        // outside the dynamic-var-construct truncation below, since a
        // bound violation is about the signature as written, not
        // something a `extract()` in the *body* should be able to hide.
        if let Some(sig) = sig {
            for (param, ast_param) in sig.params.iter().zip(&f.params) {
                self.check_generic_bounds(&param.ty, &label, ast_param.span);
            }
        }
        self.check_generic_bounds(
            declared_return.as_ref().unwrap_or(&Type::Mixed),
            &label,
            f.span,
        );

        // Phase 10.5's variance-enforcement half - same "check the
        // declaration once" placement as bound-enforcement above, and
        // only meaningful for a method (a plain top-level function has no
        // enclosing class to have declared variance for at all).
        // `__construct` is exempt (real PHPStan's own rule exempts it too):
        // a constructor is only ever called to build one specific,
        // concrete instance (`new Success<Cat>($cat)`), never invoked
        // polymorphically through a variable typed as the covariant
        // supertype the way an ordinary method can be - so a covariant
        // `T` constructor parameter doesn't actually violate covariance
        // safety at all. Real-world confirmation: every one of the 27
        // diagnostics this check produced on the first real-corpus pass,
        // with zero exceptions, was on a constructor - the extremely
        // common `readonly` value-holder pattern (`public function
        // __construct(private readonly T $value) {}`), not a genuine bug.
        if let (Some(sig), Some(c)) = (sig, class_name)
            && f.name != "__construct"
        {
            self.check_variance(sig, c, &label, f.span);
        }

        // Phase 58: same "check the declaration once" placement as the
        // bound/variance checks just above - a missing typehint is about
        // the signature as written, not something a body-local
        // `extract()` should be able to hide either.
        self.check_missing_function_typehints(
            f,
            class_name,
            &label,
            &resolved_params,
            declared_return.as_ref(),
            sig.or(ancestor_sig_hint),
        );

        let previous_class = self.current_class.take();
        self.current_class = class_name.map(|s| s.to_string());
        let previous_method = self.current_method.take();
        self.current_method = Some(f.name.clone());
        // Phase 39.2: real PHPStan's own `DeprecatedScopeHelper` -
        // suppresses every deprecated-usage diagnostic from within a
        // deprecated function/method, class, or trait's own body.
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
        let previous_goto_targets = std::mem::replace(
            &mut self.current_function_goto_targets,
            collect_goto_targets(&f.body),
        );
        self.check_generator_return_type(native_return.as_ref(), f.span, &label);

        // `check_block`, not a bare loop over `check_stmt`: a function's
        // top-level body is exactly as eligible for Phase 15's
        // `deadCode.unreachable` as any nested `{ ... }` block is - it
        // just wasn't routed through the one place that check lives.
        self.check_block(&f.body, &mut locals, declared_return.as_ref(), &label);

        // A function that falls off the end of its body with no `return`
        // at all implicitly returns `null` too - equally a real mismatch
        // against a non-nullable, non-void, non-mixed declared return
        // type, and equally easy to miss without this: `always_terminates`
        // (already used for guard-clause narrowing) answers "does this
        // statement definitely end control flow" - a `false` here means
        // this function's own body doesn't guarantee returning anything.
        // A real generator (Finding 3) is exempt: falling off the end (or
        // an explicit bare `return;`, handled in check_stmt) implicitly
        // ends the *generator's* iteration, not the function's own return
        // value - real PHPStan never checks either against the function's
        // declared Generator/Iterator/Traversable return type.
        // Not just `f.body.last()`: a statement anywhere in the body
        // unconditionally terminating (the rest being dead code, now
        // itself flagged by `check_block`'s own `deadCode.unreachable`
        // above) is equally sound proof the function never falls off the
        // end, the same reasoning `always_terminates`'s own `Block` arm
        // uses for a nested `{ ... }` - `f.body` just isn't itself one
        // `Stmt`, so it needs the same check spelled out here directly.
        if let Some(expected) = &declared_return
            && (!f.body.is_empty() || (self.registry.check_empty_method_bodies && is_concrete))
            && !f.body.iter().any(|s| self.always_terminates(s, &locals))
            && !self.current_function_is_generator
        {
            let expected = match &self.current_class {
                Some(c) => self.resolve_pseudo(expected, c),
                None => expected.clone(),
            };
            // Phase 61: a `never`-typed function/method whose body simply
            // falls off the end without ever throwing/exiting is the same
            // "should never return" claim `Return(Some(e))`/`Return(None)`
            // already special-case, just reached via the implicit
            // fall-through instead of an explicit `return` statement -
            // real PHPStan's own `return.never` covers both shapes
            // identically.
            if expected == Type::Never {
                self.report(
                    label.clone(),
                    "should never return, but a return statement was reached".to_string(),
                    f.span,
                    3,
                    "return.never",
                );
            } else if expected != Type::Void
                && expected != Type::Mixed
                && !self.registry.is_assignable(&Type::Null, &expected)
            {
                self.report(
                    label.clone(),
                    format!("should return {expected} but return statement is missing"),
                    f.span,
                    3,
                    "return.missing",
                );
            }
        }

        // Phase 59: needs the same body-level extract()/eval() guard as
        // everything else checked in this function - placed before the
        // truncation below (rather than after) specifically so a
        // constructor that calls extract()/eval() has this diagnostic
        // discarded along with every other one from its own body, exactly
        // like the checks already running inside the loop above.
        self.check_unused_constructor_parameters(f, class_name, &label);

        if self.saw_dynamic_var_construct {
            self.diagnostics.truncate(diagnostics_before);
        }
        self.saw_dynamic_var_construct = previous_dynamic_flag;
        self.current_function_is_generator = previous_generator_flag;
        self.current_function_goto_targets = previous_goto_targets;

        self.current_class = previous_class;
        self.current_method = previous_method;
        self.current_scope_deprecated = previous_scope_deprecated;
    }
}
