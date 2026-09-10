use crate::ast::*;
use crate::checker::{
    ATTRIBUTE_TARGET_METHOD, Checker, DiagnosticBuilder, Locals, body_contains_yield,
    collect_goto_targets, expr_contains_yield,
};
use crate::registry::{self, PropertySig};
use crate::token::Span;
use crate::types::Type;

impl<'a> Checker<'a> {
    /// Phase 43: PHP 8.4 property hooks - real PHPStan's
    /// `GetNonVirtualPropertyHookReadRule`/`SetNonVirtualPropertyHookAssignRule`
    /// (does a non-virtual hook actually touch the property's own backing
    /// storage), `SetPropertyHookParameterRule`'s native-type-contravariance
    /// half (see below for why only the native half, not its docblock-
    /// merged sibling), and `ExistingClassesInPropertyHookTypehintsRule`'s
    /// existence-check half for the `set` hook's own parameter type,
    /// combined into one per-property pass. Each hook's own body is also
    /// checked as a real function-like scope here - modeled directly on
    /// `checker/expr.rs`'s existing `ExprKind::Closure`/`ExprKind::ArrowFn`
    /// handling (fresh `locals`, explicit `$this` binding, generator-flag/
    /// goto-targets save-restore), NOT on `check_function`'s registry-
    /// `FunctionSig`-lookup path: that path runs a lot of method-specific
    /// machinery (attribute-target validation for *methods*, `#[Override]`/
    /// `#[NoDiscard]`, first-class-callable eligibility) that would be
    /// actively wrong for a hook, which - unlike a real method - is never
    /// itself callable as `$obj->get()`.
    ///
    /// Only plain property declarations (`c.properties`) are covered - a
    /// promoted constructor property can also carry its own inline hooks
    /// (`Param::hooks`), but nothing in this project's registry resolves
    /// `set_ty`/`is_virtual` for that shape yet either (confirmed:
    /// `property_hook_set_type` takes a `&PropertyDecl`, structurally
    /// unable to accept a `&Param`) - a real, disclosed residual gap, not
    /// Phase 43.1: a promoted constructor property (`Param::hooks`) is
    /// checked too, via a second loop mirroring `check_property_overrides`'s
    /// own established precedent for the identical "plain declaration vs.
    /// promoted constructor parameter" split - reusing the exact same
    /// `check_get_hook`/`check_set_hook` logic (generalized to take
    /// scalar `name`/`span`/`type_hint` instead of a whole `&PropertyDecl`
    /// for exactly this reason).
    pub(crate) fn check_property_hooks(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for p in &c.properties {
            let Some(hooks) = &p.hooks else {
                continue;
            };
            let Some(prop_sig) = info.properties.get(&p.name).cloned() else {
                continue;
            };
            if let Some(body) = &hooks.get {
                self.check_get_hook(fqn, &p.name, p.span, body, &prop_sig, &hooks.get_attrs);
            }
            if let Some((param, body)) = &hooks.set {
                self.check_set_hook(
                    fqn,
                    &p.name,
                    p.span,
                    p.type_hint.as_deref(),
                    param.as_ref(),
                    body,
                    &prop_sig,
                    &hooks.set_attrs,
                );
            }
        }
        if let Some(ctor) = c.methods.iter().find(|m| m.function.name == "__construct") {
            for param in &ctor.function.params {
                if param.promoted_visibility.is_none() {
                    continue;
                }
                let Some(hooks) = param.hooks.as_deref() else {
                    continue;
                };
                let Some(prop_sig) = info.properties.get(&param.name).cloned() else {
                    continue;
                };
                if let Some(body) = &hooks.get {
                    self.check_get_hook(
                        fqn,
                        &param.name,
                        param.span,
                        body,
                        &prop_sig,
                        &hooks.get_attrs,
                    );
                }
                if let Some((set_param, body)) = &hooks.set {
                    self.check_set_hook(
                        fqn,
                        &param.name,
                        param.span,
                        param.type_hint.as_deref(),
                        set_param.as_ref(),
                        body,
                        &prop_sig,
                        &hooks.set_attrs,
                    );
                }
            }
        }
    }

    /// Phase 37: `PropertyHookAttributesRule` - a property hook's own
    /// `#[Attr]` group(s) are checked exactly like a method's (`Attribute
    /// ::TARGET_METHOD`), plus one hardcoded special case real PHPStan's
    /// own rule applies regardless of the attribute class's own declared
    /// target flags: `#[\NoDiscard]` can never be used on a hook at all
    /// (a hook is narrower than "any method" - it's never itself directly
    /// callable), reusing the same `attribute.target` identifier rather
    /// than a new one, matching real PHPStan exactly.
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

    /// `name`/`span`/`type_hint` are the only three `PropertyDecl` fields
    /// either this or `check_set_hook` actually touch - taken as scalars
    /// (Phase 43.1) so both a plain property declaration and a promoted
    /// constructor parameter (`ast::Param` has identically-named/-typed
    /// fields) can share this same checking logic without either being
    /// forced into the other's own struct shape.
    fn check_get_hook(
        &mut self,
        fqn: &str,
        name: &str,
        span: Span,
        body: &PropertyHookBody,
        prop_sig: &PropertySig,
        attrs: &[Attribute],
    ) {
        let label = format!("{fqn}::${name} (get hook)");
        self.check_hook_attributes(attrs, &label);
        // Phase 64 Item 1: an abstract hook has no body at all - "does
        // the body read the property" is meaningless for it, and
        // `notReadable`/`notWritable`/`abstractOverridingNonAbstractHook`
        // (the checks that actually apply to an abstract hook) live on
        // `check_property_hook_override` instead, not here.
        if matches!(body, PropertyHookBody::Abstract) {
            return;
        }
        // `GetNonVirtualPropertyHookReadRule`: a non-virtual property's
        // `get` hook that never references its own backing storage at
        // all almost certainly forgot to (real PHP still compiles it -
        // it just always returns whatever the hook body itself computes,
        // silently never reading the real stored value).
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
        // Phase 42 Sub-item A: same reasoning as every other function-
        // shaped save/restore site - `TooWidePropertyHookThrowTypeRule`
        // itself isn't ported (out of this phase's own scope), but a real
        // `throw` inside a hook body must still not leak into whatever
        // gets checked next.
        let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
        let previous_unannotated_call =
            std::mem::take(&mut self.current_function_has_unannotated_call);
        let previous_explicit_throw_points =
            std::mem::take(&mut self.current_function_explicit_throw_points);
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
                if body_ty != Type::Mixed && !self.registry.is_assignable(&body_ty, &prop_sig.ty) {
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
                // Same "falls off the end with no return" completeness
                // check `check_function`'s own tail performs for a named
                // function/method - a `get` hook block body is exactly as
                // eligible for `return.missing` as an ordinary typed
                // method body is (real PHPStan runs `MissingReturnRule`
                // against a property hook the same as any other function-
                // like node).
                if !stmts.is_empty()
                    && !stmts.iter().any(|s| self.always_terminates(s, &locals))
                    && !self.current_function_is_generator
                    && prop_sig.ty != Type::Mixed
                {
                    self.report(
                        label.clone(),
                        format!(
                            "should return {} but return statement is missing",
                            prop_sig.ty
                        ),
                        span,
                        3,
                        "return.missing",
                    );
                }
            }
            // No real statement/expression to type-check at all.
            PropertyHookBody::Abstract => {}
        }
        self.current_function_is_generator = previous_generator_flag;
        self.current_declared_return_type = previous_declared_return_type;
        self.current_function_throw_points = previous_throw_points;
        self.current_function_has_unannotated_call = previous_unannotated_call;
        self.current_function_explicit_throw_points = previous_explicit_throw_points;
        if let Some(previous) = previous_goto_targets {
            self.current_function_goto_targets = previous;
        }
        self.current_class = previous_class;
    }

    #[allow(clippy::too_many_arguments)]
    fn check_set_hook(
        &mut self,
        fqn: &str,
        name: &str,
        span: Span,
        type_hint: Option<&str>,
        param: Option<&Param>,
        body: &PropertyHookBody,
        prop_sig: &PropertySig,
        attrs: &[Attribute],
    ) {
        let label = format!("{fqn}::${name} (set hook)");
        self.check_hook_attributes(attrs, &label);
        // Phase 64 Item 1: an abstract hook has no body - "does it
        // assign the property" is meaningless for it (unlike `check_get_
        // hook`, the rest of this function - native-type contravariance,
        // class-existence on the parameter type - is still fully
        // meaningful for an abstract set hook's own declared parameter,
        // so only this one check is skipped, not the whole function).
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
        // half only - its own docblock-merged sibling
        // (`propertySetHook.parameterType`, real PHPStan's own
        // `checkPhpDocMethodSignatures`-gated check) is deliberately
        // skipped, mirroring this project's own established default for
        // override compatibility generally: real PHPStan's own flag
        // defaults off (confirmed neither Piwigo's own `phpstan.neon` nor
        // this project's corpus config enables it - see
        // `FunctionSig::return_type_native`'s own doc comment for the
        // identical reasoning already applied to method overrides).
        let param_has_native = param.is_some_and(|p| p.type_hint.is_some());
        let prop_has_native = type_hint.is_some();
        match (prop_has_native, param_has_native) {
            (false, true) => {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
                            "Parameter ${} of set hook has a native type but the property {fqn}::${name} does not.",
                            param.map(|p| p.name.as_str()).unwrap_or("value"),
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
                            param.map(|p| p.name.as_str()).unwrap_or("value"),
                        ),
                        span,
                        0,
                        "propertySetHook.nativeParameterType",
                    )
                    .non_ignorable(),
                );
            }
            (true, true) => {
                let param_ty = self.resolve_type_hint(param.and_then(|p| p.type_hint.as_deref()));
                let prop_native_ty = self.resolve_type_hint(type_hint);
                if !self.registry.is_assignable(&prop_native_ty, &param_ty) {
                    self.report_builder(
                        DiagnosticBuilder::new(
                            fqn.to_string(),
                            format!(
                                "Native type {param_ty} of set hook parameter ${} is not contravariant with native type {prop_native_ty} of property {fqn}::${name}.",
                                param.map(|p| p.name.as_str()).unwrap_or("value"),
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

        // `ExistingClassesInPropertyHookTypehintsRule`'s existence half -
        // `prop_sig.set_ty` is already resolved (registry-build time),
        // same as every other `check_type_class_names_exist` call site in
        // this checker (`check_function`, Closure/ArrowFn params).
        if let Some(set_ty) = &prop_sig.set_ty {
            self.check_type_class_names_exist(
                set_ty,
                &format!(
                    "Parameter ${} of set hook for property {fqn}::${name}",
                    param.map(|p| p.name.as_str()).unwrap_or("value"),
                ),
                span,
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
            .unwrap_or_else(|| "value".to_string());
        let param_span = param.map(|p| p.name_span).unwrap_or(span);
        let value_ty = prop_sig
            .set_ty
            .clone()
            .unwrap_or_else(|| prop_sig.ty.clone());
        locals.declare(param_name, value_ty, param_span);
        let declared_return = Some(Type::Void);
        let previous_generator_flag = self.current_function_is_generator;
        let previous_declared_return_type = std::mem::replace(
            &mut self.current_declared_return_type,
            declared_return.clone(),
        );
        // Phase 42 Sub-item A: see `check_get_hook`'s own identical
        // comment.
        let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
        let previous_unannotated_call =
            std::mem::take(&mut self.current_function_has_unannotated_call);
        let previous_explicit_throw_points =
            std::mem::take(&mut self.current_function_explicit_throw_points);
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
        self.current_function_is_generator = previous_generator_flag;
        self.current_declared_return_type = previous_declared_return_type;
        self.current_function_throw_points = previous_throw_points;
        self.current_function_has_unannotated_call = previous_unannotated_call;
        self.current_function_explicit_throw_points = previous_explicit_throw_points;
        if let Some(previous) = previous_goto_targets {
            self.current_function_goto_targets = previous;
        }
        self.current_class = previous_class;
    }
}
