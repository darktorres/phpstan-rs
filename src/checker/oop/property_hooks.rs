use crate::ast::*;
use crate::checker::{
    Checker, DiagnosticBuilder, Locals, body_contains_yield, collect_goto_targets,
    expr_contains_yield,
};
use crate::registry::{self, PropertySig};
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
    /// attempted here (see ROADMAP.md's Phase 43 entry).
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
                self.check_get_hook(fqn, p, body, &prop_sig);
            }
            if let Some((param, body)) = &hooks.set {
                self.check_set_hook(fqn, p, param.as_ref(), body, &prop_sig);
            }
        }
    }

    fn check_get_hook(
        &mut self,
        fqn: &str,
        p: &PropertyDecl,
        body: &PropertyHookBody,
        prop_sig: &PropertySig,
    ) {
        let label = format!("{fqn}::${} (get hook)", p.name);
        // `GetNonVirtualPropertyHookReadRule`: a non-virtual property's
        // `get` hook that never references its own backing storage at
        // all almost certainly forgot to (real PHP still compiles it -
        // it just always returns whatever the hook body itself computes,
        // silently never reading the real stored value).
        if !prop_sig.is_virtual && !registry::hook_body_references_property(body, &p.name) {
            self.report(
                fqn.to_string(),
                format!(
                    "Get hook for non-virtual property {fqn}::${} does not read its value.",
                    p.name
                ),
                p.span,
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
        };
        self.check_generator_return_type(declared_return.as_ref(), p.span, &label);
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
                        p.span,
                        3,
                        "return.missing",
                    );
                }
            }
        }
        self.current_function_is_generator = previous_generator_flag;
        if let Some(previous) = previous_goto_targets {
            self.current_function_goto_targets = previous;
        }
        self.current_class = previous_class;
    }

    fn check_set_hook(
        &mut self,
        fqn: &str,
        p: &PropertyDecl,
        param: Option<&Param>,
        body: &PropertyHookBody,
        prop_sig: &PropertySig,
    ) {
        let label = format!("{fqn}::${} (set hook)", p.name);
        if !prop_sig.is_virtual && !registry::hook_body_references_property(body, &p.name) {
            self.report(
                fqn.to_string(),
                format!(
                    "Set hook for non-virtual property {fqn}::${} does not assign value to it.",
                    p.name
                ),
                p.span,
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
        let prop_has_native = p.type_hint.is_some();
        match (prop_has_native, param_has_native) {
            (false, true) => {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
                            "Parameter ${} of set hook has a native type but the property {fqn}::${} does not.",
                            param.map(|p| p.name.as_str()).unwrap_or("value"),
                            p.name
                        ),
                        p.span,
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
                            "Parameter ${} of set hook does not have a native type but the property {fqn}::${} does.",
                            param.map(|p| p.name.as_str()).unwrap_or("value"),
                            p.name
                        ),
                        p.span,
                        0,
                        "propertySetHook.nativeParameterType",
                    )
                    .non_ignorable(),
                );
            }
            (true, true) => {
                let param_ty = self.resolve_type_hint(param.and_then(|p| p.type_hint.as_deref()));
                let prop_native_ty = self.resolve_type_hint(p.type_hint.as_deref());
                if !self.registry.is_assignable(&prop_native_ty, &param_ty) {
                    self.report_builder(
                        DiagnosticBuilder::new(
                            fqn.to_string(),
                            format!(
                                "Native type {param_ty} of set hook parameter ${} is not contravariant with native type {prop_native_ty} of property {fqn}::${}.",
                                param.map(|p| p.name.as_str()).unwrap_or("value"),
                                p.name
                            ),
                            p.span,
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
                    "Parameter ${} of set hook for property {fqn}::${}",
                    param.map(|p| p.name.as_str()).unwrap_or("value"),
                    p.name
                ),
                p.span,
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
        let param_span = param.map(|p| p.name_span).unwrap_or(p.span);
        let value_ty = prop_sig
            .set_ty
            .clone()
            .unwrap_or_else(|| prop_sig.ty.clone());
        locals.declare(param_name, value_ty, param_span);
        let declared_return = Some(Type::Void);
        let previous_generator_flag = self.current_function_is_generator;
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
        };
        self.check_generator_return_type(declared_return.as_ref(), p.span, &label);
        match body {
            PropertyHookBody::Arrow(e) => {
                self.check_expr(e, &mut locals, &label);
            }
            PropertyHookBody::Block(stmts) => {
                self.check_block(stmts, &mut locals, declared_return.as_ref(), &label);
            }
        }
        self.current_function_is_generator = previous_generator_flag;
        if let Some(previous) = previous_goto_targets {
            self.current_function_goto_targets = previous;
        }
        self.current_class = previous_class;
    }
}
