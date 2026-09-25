use crate::ast::*;
use crate::checker::type_union::union_types;
use crate::checker::{
    ATTRIBUTE_TARGET_METHOD, Checker, Locals, body_contains_explicit_throw, collect_goto_targets,
    expr_contains_explicit_throw,
};
use crate::diagnostic::DiagnosticBuilder;
use crate::docblock;
use crate::registry::{self, PropertySig, resolve_names_in_type};
use crate::token::Span;
use crate::types::{self, Type};
use crate::visit::{body_contains_yield, expr_contains_yield};

impl<'a> Checker<'a> {
    /// PHP 8.4 property hooks: `GetNonVirtualPropertyHookReadRule`/
    /// `SetNonVirtualPropertyHookAssignRule`, `SetPropertyHookParameterRule`'s
    /// native-type-contravariance half (see below), and
    /// `ExistingClassesInPropertyHookTypehintsRule`'s existence check for
    /// the `set` hook's parameter type, combined into one per-property
    /// pass. Each hook's body is checked as its own function-like scope
    /// (mirroring `checker/expr.rs`'s `Closure`/`ArrowFn` handling), not
    /// through `check_function`'s registry lookup - a hook is never
    /// itself callable, so it doesn't need that path's method-specific
    /// machinery (`#[Override]`/`#[NoDiscard]`, first-class-callable
    /// eligibility).
    ///
    /// Also covers a promoted constructor property's inline hooks
    /// (`Param::hooks`), reusing `check_get_hook`/`check_set_hook` via
    /// scalar `name`/`span`/`type_hint` arguments.
    pub(crate) fn check_property_hooks(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for p in &c.properties {
            let Some(hooks) = &p.hooks else {
                continue;
            };
            let Some(prop_sig) = info.properties.get(p.name.as_ref()).cloned() else {
                continue;
            };
            if let Some(body) = &hooks.get {
                self.check_hook_phpdoc_types(
                    fqn,
                    p.type_hint.as_ref(),
                    None,
                    hooks.get_doc.as_deref(),
                    p.span,
                    &format!("{fqn}::${} (get hook)", p.name),
                );
                self.check_get_hook(
                    fqn,
                    &p.name,
                    p.span,
                    body,
                    &prop_sig,
                    &hooks.get_attrs,
                    hooks.get_doc.as_deref(),
                    hooks.get_final,
                );
            }
            if let Some((param, body)) = &hooks.set {
                self.check_set_hook(
                    fqn,
                    &p.name,
                    p.span,
                    p.type_hint.as_ref(),
                    param.as_ref(),
                    body,
                    &prop_sig,
                    &hooks.set_attrs,
                    hooks.set_doc.as_deref(),
                    hooks.set_final,
                );
            }
        }
        if let Some(ctor) = c
            .methods
            .iter()
            .find(|m| m.function.name == "__construct".into())
        {
            for param in &ctor.function.params {
                if param.promoted_visibility.is_none() {
                    continue;
                }
                let Some(hooks) = param.hooks.as_deref() else {
                    continue;
                };
                let Some(prop_sig) = info.properties.get(param.name.as_ref()).cloned() else {
                    continue;
                };
                if let Some(body) = &hooks.get {
                    self.check_hook_phpdoc_types(
                        fqn,
                        param.type_hint.as_ref(),
                        None,
                        hooks.get_doc.as_deref(),
                        param.span,
                        &format!("{fqn}::${} (get hook)", param.name),
                    );
                    self.check_get_hook(
                        fqn,
                        &param.name,
                        param.span,
                        body,
                        &prop_sig,
                        &hooks.get_attrs,
                        hooks.get_doc.as_deref(),
                        hooks.get_final,
                    );
                }
                if let Some((set_param, body)) = &hooks.set {
                    self.check_set_hook(
                        fqn,
                        &param.name,
                        param.span,
                        param.type_hint.as_ref(),
                        set_param.as_ref(),
                        body,
                        &prop_sig,
                        &hooks.set_attrs,
                        hooks.set_doc.as_deref(),
                        hooks.set_final,
                    );
                }
            }
        }
    }

    /// `PropertyHookAttributesRule` - a hook's own `#[Attr]` groups are
    /// checked like a method's (`Attribute::TARGET_METHOD`), plus
    /// `#[\NoDiscard]`, which real PHPStan bans on any hook (never itself
    /// directly callable).
    fn check_hook_attributes(&mut self, attrs: &[Attribute], label: &str) {
        self.check_attributes(attrs, label);
        self.check_attribute_targets(attrs, ATTRIBUTE_TARGET_METHOD, "method", label);
        if let Some(nodiscard) = attrs
            .iter()
            .find(|a| registry::is_no_discard_attribute_name(&a.name))
        {
            let fqn = self.name_ctx.resolve(&nodiscard.name);
            self.report_builder(
                DiagnosticBuilder::new(
                    label.to_string(),
                    format!("Attribute class {fqn} cannot be used on property hooks."),
                    nodiscard.span,
                    0,
                    "attribute.target",
                )
                .non_ignorable(),
            );
        }
    }

    /// Resolves a hook's own `@throws` tag into a real `Type`, computed
    /// at check time since a hook has no `FunctionSig` of its own to
    /// cache it on.
    fn resolve_hook_throws(&self, doc: Option<&str>) -> Option<Type> {
        let block = docblock::parse(doc?);
        block.throws.iter().fold(None::<Type>, |acc, raw| {
            let ty = types::parse_type_string(raw);
            let ty = resolve_names_in_type(ty, &self.name_ctx);
            Some(match acc {
                Some(prev) => union_types(prev, ty),
                None => ty,
            })
        })
    }

    /// Mirrors `TooWideMethodThrowTypeRule`'s property-hook sibling,
    /// `TooWidePropertyHookThrowTypeRule` - skip unless the property or
    /// hook is private/final, the declaring class is final, or the
    /// shared `check_too_wide_throw_type_in_protected_and_public_methods`
    /// opt-in flag is on.
    fn should_check_too_wide_property_hook_throw_type(
        &self,
        class: &str,
        property: &str,
        visibility: Visibility,
        hook_final: bool,
    ) -> bool {
        if visibility == Visibility::Private || hook_final {
            return true;
        }
        let Some(info) = self.registry.classes.get(class) else {
            return true;
        };
        if info.is_final || info.final_properties.contains(property) {
            return true;
        }
        self.registry
            .check_too_wide_throw_type_in_protected_and_public_methods
    }

    /// Scalar `name`/`span`/`type_hint` (rather than a whole
    /// `&PropertyDecl`) let this and `check_set_hook` handle both a plain
    /// property and a promoted constructor parameter uniformly.
    #[allow(clippy::too_many_arguments)]
    fn check_get_hook(
        &mut self,
        fqn: &str,
        name: &str,
        span: Span,
        body: &PropertyHookBody,
        prop_sig: &PropertySig,
        attrs: &[Attribute],
        doc: Option<&str>,
        hook_final: bool,
    ) {
        let label = format!("{fqn}::${name} (get hook)");
        self.check_hook_attributes(attrs, &label);
        // `InvalidThrowsPhpDocValueRule`'s @throws-validity check runs
        // even for a bodyless hook; the too-wide/missing checks below
        // need a real body to compare against.
        let throws = self.resolve_hook_throws(doc);
        if let Some(throws) = &throws {
            self.check_throws_tag_validity(throws, span, &label);
            // `ThrowsVoidPropertyHookWithExplicitThrowPointRule`, same
            // simplified wording as `check_function_with_ancestor_hint`.
            if *throws == Type::Void {
                let throw_span = match body {
                    PropertyHookBody::Arrow(e) => expr_contains_explicit_throw(e),
                    PropertyHookBody::Block(stmts) => body_contains_explicit_throw(stmts),
                    PropertyHookBody::Abstract => None,
                };
                if let Some(throw_span) = throw_span {
                    self.report(
                        label.clone(),
                        format!(
                            "Get hook for property {fqn}::${name} throws exception but the PHPDoc contains @throws void."
                        ),
                        throw_span,
                        3,
                        "throws.void",
                    );
                }
            }
        }
        // An abstract hook has no body - read/write checks don't apply;
        // see `check_property_hook_override` for the ones that do.
        if matches!(body, PropertyHookBody::Abstract) {
            return;
        }
        // `GetNonVirtualPropertyHookReadRule`: a non-virtual get hook
        // that never reads its own backing storage almost certainly
        // forgot to.
        if !prop_sig.is_virtual && !registry::hook_body_references_property(body, name) {
            self.report(
                fqn.to_string(),
                format!(
                    "Get hook for non-virtual property {fqn}::${name} does not read its value."
                ),
                span,
                3,
                "propertyGetHook.noRead",
            );
        }

        let previous_class = self.current_class.take();
        self.current_class = Some(fqn.to_string());
        let mut locals = Locals::default();
        locals.insert(
            "this".to_string(),
            Type::Object(fqn.to_string(), Vec::new()),
        );
        let declared_return = Some(prop_sig.ty.clone());
        let previous_generator_flag = self.current_function_is_generator;
        let previous_declared_return_type = std::mem::replace(
            &mut self.current_declared_return_type,
            declared_return.clone(),
        );
        // Same save/restore as any function-shaped body, so a `throw`
        // here doesn't leak into whatever's checked next.
        let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
        let previous_unannotated_call =
            std::mem::take(&mut self.current_function_has_unannotated_call);
        let previous_explicit_throw_points =
            std::mem::take(&mut self.current_function_explicit_throw_points);
        let previous_assigned_variables = match body {
            PropertyHookBody::Block(stmts) => self.enter_assigned_variables(stmts),
            _ => self.enter_assigned_variables(&[]),
        };
        let previous_goto_targets = match body {
            PropertyHookBody::Arrow(e) => {
                self.current_function_is_generator = expr_contains_yield(e);
                None
            }
            PropertyHookBody::Block(stmts) => {
                self.current_function_is_generator = body_contains_yield(stmts);
                Some(std::mem::replace(
                    &mut self.current_function_goto_targets,
                    collect_goto_targets(stmts),
                ))
            }
            // No real body at all - nothing to scan for yield/goto.
            PropertyHookBody::Abstract => {
                self.current_function_is_generator = false;
                None
            }
        };
        self.check_generator_return_type(declared_return.as_ref(), span, &label);
        match body {
            PropertyHookBody::Arrow(e) => {
                let body_ty = self.check_expr(e, &mut locals, &label);
                if body_ty != Type::Mixed(false)
                    && !self.registry.is_assignable(&body_ty, &prop_sig.ty)
                {
                    self.report(
                        label.clone(),
                        format!(
                            "return type mismatch: expected {}, got {body_ty}",
                            prop_sig.ty
                        ),
                        e.span,
                        3,
                        "return.type",
                    );
                }
            }
            PropertyHookBody::Block(stmts) => {
                self.check_block(stmts, &mut locals, declared_return.as_ref(), &label);
                // Same fall-off-the-end completeness check `check_function`
                // performs - real PHPStan runs `MissingReturnRule` against
                // a hook the same as any other function-like node.
                if !stmts.iter().any(|s| self.always_terminates(s, &locals))
                    && !self.current_function_is_generator
                    && let Some(level) = crate::checker::function_decl::missing_return_level(
                        &prop_sig.ty,
                        prop_sig.has_native_type,
                        false,
                    )
                {
                    let mut builder = DiagnosticBuilder::new(
                        label.clone(),
                        format!(
                            "should return {} but return statement is missing",
                            prop_sig.ty
                        ),
                        span,
                        level,
                        "return.missing",
                    );
                    if prop_sig.has_native_type {
                        builder = builder.non_ignorable();
                    }
                    self.report_builder(builder);
                }
            }
            // No real statement/expression to type-check at all.
            PropertyHookBody::Abstract => {}
        }
        // `TooWideMethodThrowTypeRule`'s property-hook sibling plus the
        // "missing checked exception" check - both need the full
        // throw-point set, so they run here before state resets.
        let subject = format!("Get hook for property {fqn}::${name}");
        if let Some(throws) = &throws
            && self.should_check_too_wide_property_hook_throw_type(
                fqn,
                name,
                prop_sig.visibility.clone(),
                hook_final,
            )
        {
            self.check_too_wide_throw_type(throws, &subject, span, &label);
        }
        self.check_missing_checked_exceptions(throws.as_ref(), &subject, &label);
        self.current_function_is_generator = previous_generator_flag;
        self.current_declared_return_type = previous_declared_return_type;
        self.current_function_throw_points = previous_throw_points;
        self.current_function_has_unannotated_call = previous_unannotated_call;
        self.current_function_explicit_throw_points = previous_explicit_throw_points;
        if let Some(previous) = previous_goto_targets {
            self.current_function_goto_targets = previous;
        }
        self.current_function_assigned_variables = previous_assigned_variables;
        self.current_class = previous_class;
    }

    #[allow(clippy::too_many_arguments)]
    fn check_set_hook(
        &mut self,
        fqn: &str,
        name: &str,
        span: Span,
        type_hint: Option<&TypeHint>,
        param: Option<&Param>,
        body: &PropertyHookBody,
        prop_sig: &PropertySig,
        attrs: &[Attribute],
        doc: Option<&str>,
        hook_final: bool,
    ) {
        let label = format!("{fqn}::${name} (set hook)");
        self.check_hook_attributes(attrs, &label);
        self.check_hook_phpdoc_types(fqn, type_hint, Some(param), doc, span, &label);
        // See `check_get_hook` - runs regardless of body.
        let throws = self.resolve_hook_throws(doc);
        if let Some(throws) = &throws {
            self.check_throws_tag_validity(throws, span, &label);
            if *throws == Type::Void {
                let throw_span = match body {
                    PropertyHookBody::Arrow(e) => expr_contains_explicit_throw(e),
                    PropertyHookBody::Block(stmts) => body_contains_explicit_throw(stmts),
                    PropertyHookBody::Abstract => None,
                };
                if let Some(throw_span) = throw_span {
                    self.report(
                        label.clone(),
                        format!(
                            "Set hook for property {fqn}::${name} throws exception but the PHPDoc contains @throws void."
                        ),
                        throw_span,
                        3,
                        "throws.void",
                    );
                }
            }
        }
        // An abstract hook has no body - only the assign check is
        // skipped; native-type/class-existence checks on the parameter
        // still apply.
        if !matches!(body, PropertyHookBody::Abstract)
            && !prop_sig.is_virtual
            && !registry::hook_body_references_property(body, name)
        {
            self.report(
                fqn.to_string(),
                format!(
                    "Set hook for non-virtual property {fqn}::${name} does not assign value to it."
                ),
                span,
                3,
                "propertySetHook.noAssign",
            );
        }

        // `SetPropertyHookParameterRule`'s native-type-contravariance
        // half only; its docblock-merged sibling is skipped, matching
        // real PHPStan's own default-off override-compatibility flag
        // (see `FunctionSig::return_type_native`).
        let param_has_native = param.is_some_and(|p| p.type_hint.is_some());
        let prop_has_native = type_hint.is_some();
        match (prop_has_native, param_has_native) {
            (false, true) => {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
                            "Parameter ${} of set hook has a native type but the property {fqn}::${name} does not.",
                            param.map(|p| p.name.as_ref()).unwrap_or("value"),
                        ),
                        span,
                        0,
                        "propertySetHook.nativeParameterType",
                    )
                    .non_ignorable(),
                );
            }
            (true, false) => {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
                            "Parameter ${} of set hook does not have a native type but the property {fqn}::${name} does.",
                            param.map(|p| p.name.as_ref()).unwrap_or("value"),
                        ),
                        span,
                        0,
                        "propertySetHook.nativeParameterType",
                    )
                    .non_ignorable(),
                );
            }
            (true, true) => {
                let param_ty = self.resolve_type_hint(
                    param
                        .and_then(|p| p.type_hint.as_ref())
                        .map(TypeHint::to_string)
                        .as_deref(),
                );
                let prop_native_ty =
                    self.resolve_type_hint(type_hint.map(TypeHint::to_string).as_deref());
                if !self.registry.is_assignable(&prop_native_ty, &param_ty) {
                    self.report_builder(
                        DiagnosticBuilder::new(
                            fqn.to_string(),
                            format!(
                                "Native type {param_ty} of set hook parameter ${} is not contravariant with native type {prop_native_ty} of property {fqn}::${name}.",
                                param.map(|p| p.name.as_ref()).unwrap_or("value"),
                            ),
                            span,
                            0,
                            "propertySetHook.nativeParameterType",
                        )
                        .non_ignorable(),
                    );
                }
            }
            (false, false) => {}
        }

        // `ExistingClassesInPropertyHookTypehintsRule`'s existence half;
        // `prop_sig.set_ty` is already resolved.
        if let Some(set_ty) = &prop_sig.set_ty {
            self.check_type_class_names_exist(
                set_ty,
                &format!(
                    "Parameter ${} of set hook for property {fqn}::${name}",
                    param.map(|p| p.name.as_ref()).unwrap_or("value"),
                ),
                span,
                &label,
            );
            // Real PHPStan also checks `parameter.trait` here, not
            // `property.trait`.
            self.check_type_trait_refs(
                set_ty,
                &format!(
                    "Parameter ${} of set hook for property {fqn}::${name}",
                    param.map(|p| p.name.as_ref()).unwrap_or("value"),
                ),
                span,
                &label,
                "parameter.trait",
            );
            // `class.nameCase` for the set hook's parameter type.
            self.check_type_class_name_case(&prop_sig.set_case_refs, &label, span);
        }

        // `ExistingClassesInPropertyHookTypehintsRule`'s native-union-
        // pre-8.0 half, reusing `check_function_impl`'s own raw-text
        // core - a hook has no `FunctionDecl` to hand it. Only the `set`
        // hook's own parameter can ever carry a native union type; a
        // hook has no return-type position at all (`nikic/php-parser`'s
        // own `PropertyHook::getReturnType()` is hardcoded `null` -
        // confirmed real PHPStan's own equivalent call passes an
        // impossible-to-trigger return-type message for this exact
        // reason), so this only ever checks the parameter half.
        if let Some(p) = param {
            self.check_native_union_types_supported_raw(
                std::iter::once(p.type_hint.as_ref()),
                None,
                &format!(
                    "Set hook for property {fqn}::${name} uses native union types but they're supported only on PHP 8.0 and later."
                ),
                p.name_span,
                &label,
            );
            // `ExistingClassesInPropertyHookTypehintsRule`'s
            // `parameter.unresolvableNativeType` half - real PHP hooks
            // have no return-type position (see above), so only the
            // parameter half is reachable here.
            self.check_unresolvable_native_type(
                p.type_hint.as_ref().map(TypeHint::to_string).as_deref(),
                &format!(
                    "Parameter ${} of set hook for property {fqn}::${name} has unresolvable native type.",
                    p.name,
                ),
                "parameter.unresolvableNativeType",
                p.name_span,
                &label,
            );
        }

        let previous_class = self.current_class.take();
        self.current_class = Some(fqn.to_string());
        let mut locals = Locals::default();
        locals.insert(
            "this".to_string(),
            Type::Object(fqn.to_string(), Vec::new()),
        );
        let param_name = param
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "value".to_string().into());
        let param_span = param.map(|p| p.name_span).unwrap_or(span);
        let value_ty = prop_sig
            .set_ty
            .clone()
            .unwrap_or_else(|| prop_sig.ty.clone());
        locals.declare(param_name.to_string(), value_ty, param_span);
        let declared_return = Some(Type::Void);
        let previous_generator_flag = self.current_function_is_generator;
        let previous_declared_return_type = std::mem::replace(
            &mut self.current_declared_return_type,
            declared_return.clone(),
        );
        // See `check_get_hook`.
        let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
        let previous_unannotated_call =
            std::mem::take(&mut self.current_function_has_unannotated_call);
        let previous_explicit_throw_points =
            std::mem::take(&mut self.current_function_explicit_throw_points);
        let previous_assigned_variables = match body {
            PropertyHookBody::Block(stmts) => self.enter_assigned_variables(stmts),
            _ => self.enter_assigned_variables(&[]),
        };
        let previous_goto_targets = match body {
            PropertyHookBody::Arrow(e) => {
                self.current_function_is_generator = expr_contains_yield(e);
                None
            }
            PropertyHookBody::Block(stmts) => {
                self.current_function_is_generator = body_contains_yield(stmts);
                Some(std::mem::replace(
                    &mut self.current_function_goto_targets,
                    collect_goto_targets(stmts),
                ))
            }
            PropertyHookBody::Abstract => {
                self.current_function_is_generator = false;
                None
            }
        };
        self.check_generator_return_type(declared_return.as_ref(), span, &label);
        match body {
            PropertyHookBody::Arrow(e) => {
                self.check_expr(e, &mut locals, &label);
            }
            PropertyHookBody::Block(stmts) => {
                self.check_block(stmts, &mut locals, declared_return.as_ref(), &label);
            }
            PropertyHookBody::Abstract => {}
        }
        // See `check_get_hook` - `check_set_hook` never returns early for
        // an abstract body, so this guard is explicit instead.
        let subject = format!("Set hook for property {fqn}::${name}");
        if let Some(throws) = &throws
            && !matches!(body, PropertyHookBody::Abstract)
            && self.should_check_too_wide_property_hook_throw_type(
                fqn,
                name,
                prop_sig.visibility.clone(),
                hook_final,
            )
        {
            self.check_too_wide_throw_type(throws, &subject, span, &label);
        }
        if !matches!(body, PropertyHookBody::Abstract) {
            self.check_missing_checked_exceptions(throws.as_ref(), &subject, &label);
        }
        self.current_function_is_generator = previous_generator_flag;
        self.current_declared_return_type = previous_declared_return_type;
        self.current_function_throw_points = previous_throw_points;
        self.current_function_has_unannotated_call = previous_unannotated_call;
        self.current_function_explicit_throw_points = previous_explicit_throw_points;
        if let Some(previous) = previous_goto_targets {
            self.current_function_goto_targets = previous;
        }
        self.current_function_assigned_variables = previous_assigned_variables;
        self.current_class = previous_class;
    }
}
