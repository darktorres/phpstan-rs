use crate::ast::*;
use crate::checker::{
    Checker, Locals, LoopZeroIterations, collect_goto_targets, purge_stale_narrowing_for,
};
use crate::diagnostic::DiagnosticBuilder;
use crate::types::Type;
use crate::visit::body_contains_yield;

impl<'a> Checker<'a> {
    /// Checks a closure/arrow-fn literal's body with `$this` seeded to
    /// `this_ty` instead of whatever (if anything) the enclosing scope's
    /// own `locals` would otherwise provide - used by `Closure::bind`/
    /// `->bindTo()` and Pest's file-scoped `$this` binding, all of which
    /// rebind `$this` independently of where the closure was lexically
    /// declared. Checked against a clone of `locals` so the seeded
    /// `$this` doesn't leak back into the caller's scope; each `use
    /// (&$x)` capture is copied back afterwards, since the closure body's
    /// writes to it are real writes to the caller's variable.
    ///
    /// `scope_class` is real PHP's own `$newScope` (`Closure::bind`/
    /// `->bindTo()`'s third argument, or Pest's own bound test-class):
    /// while the closure body is checked, `self.current_class` is
    /// temporarily swapped to it, then restored afterward (ordinary
    /// control flow only, no unwind-safety concern to guard against
    /// here). Real PHP grants the rebound closure private/protected
    /// access as if it were declared *inside* that class, independent
    /// of `$newThis`'s own type or where the closure was lexically
    /// written. `None` (a call site with no literal class-name/`::class`
    /// third argument to resolve at all) leaves `current_class` alone -
    /// real PHP's own default (`$newScope = 'static'`) keeps whatever
    /// scope the closure already had, which this project has no way to
    /// recover once the closure literal itself is being checked outside
    /// its own original declaration site anyway.
    pub(crate) fn check_closure_literal_with_this_and_scope(
        &mut self,
        closure_expr: &Expr,
        locals: &mut Locals,
        label: &str,
        this_ty: Type,
        scope_class: Option<String>,
    ) {
        let mut seeded = locals.clone();
        seeded.insert("this".to_string(), this_ty);
        if let Some(scope) = scope_class {
            let previous_class = self.current_class.replace(scope);
            self.check_expr(closure_expr, &mut seeded, label);
            self.current_class = previous_class;
        } else {
            self.check_expr(closure_expr, &mut seeded, label);
        }
        let ExprKind::Closure { uses, .. } = &closure_expr.kind else {
            return;
        };
        for u in uses.iter().filter(|u| u.by_ref) {
            let Some(ty) = seeded.get(&u.name).cloned() else {
                continue;
            };
            purge_stale_narrowing_for(locals, &u.name);
            if locals.contains_key(&u.name) {
                locals.insert(u.name.to_string(), ty);
                locals.clear_native(&u.name);
            } else {
                locals.declare(u.name.to_string(), ty, u.span);
            }
        }
    }

    /// `array_map`'s contextual parameter-type injection - checks a
    /// closure/arrow-fn literal argument's body with each unhinted
    /// parameter bound to the matching entry of `param_overrides` (the
    /// mapped array's element type) instead of the `Mixed` an unhinted
    /// parameter otherwise gets, catching a real bug inside the callback
    /// itself - something `infer_closure_return_with_param_overrides`
    /// (return-type inference only, `&self`, no `report(...)` access)
    /// can never surface a diagnostic for. A parameter with its own
    /// declared type hint keeps that hint, mirroring real PHPStan's
    /// precedence (a declared type is always authoritative over an
    /// inferred one). Mirrors the ordinary `ExprKind::Closure`/`ArrowFn`
    /// arms in `check_expr` almost verbatim - a bug fixed there needs
    /// the same fix reflected here. Scoped to the same shape
    /// `infer_closure_return_with_param_overrides` already restricts
    /// itself to: `false` (falls back to the caller's plain
    /// `check_expr`) whenever the override count doesn't exactly match
    /// the closure/arrow-fn's parameter count, or any parameter is
    /// variadic or by-ref.
    pub(crate) fn check_closure_literal_with_param_overrides(
        &mut self,
        closure_expr: &Expr,
        param_overrides: &[Type],
        locals: &mut Locals,
        label: &str,
    ) -> bool {
        match &closure_expr.kind {
            ExprKind::Grouping(inner) => self.check_closure_literal_with_param_overrides(
                inner,
                param_overrides,
                locals,
                label,
            ),
            ExprKind::ArrowFn {
                params,
                body,
                return_type,
                ..
            } => {
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return false;
                }
                if self.registry.check_native_type_hints {
                    self.suggest_closure_native_params(params, param_overrides, label);
                }
                let mut arrow_locals = locals.clone();
                for (p, override_ty) in params.iter().zip(param_overrides) {
                    let (ty, case_refs) = if p.type_hint.is_some() {
                        self.resolve_type_hint_with_case_refs(
                            p.type_hint.as_ref().map(TypeHint::to_string).as_deref(),
                        )
                    } else {
                        (override_ty.clone(), Vec::new())
                    };
                    self.check_type_class_names_exist(
                        &ty,
                        &format!("parameter ${} of arrow function", p.name),
                        p.span,
                        label,
                    );
                    self.check_type_trait_refs(
                        &ty,
                        &format!("parameter ${} of arrow function", p.name),
                        p.span,
                        label,
                        "parameter.trait",
                    );
                    self.check_type_class_name_case(&case_refs, label, p.span);
                    arrow_locals.declare(p.name.to_string(), ty, p.name_span);
                    arrow_locals.set_native(&p.name, self.closure_param_native(p));
                }
                let body_ty = self.check_expr(body, &mut arrow_locals, label);
                if return_type.is_some() {
                    let (expected, expected_case_refs) = self.resolve_type_hint_with_case_refs(
                        return_type.as_deref().map(TypeHint::to_string).as_deref(),
                    );
                    self.check_type_class_names_exist(
                        &expected,
                        "return type of arrow function",
                        closure_expr.span,
                        label,
                    );
                    self.check_type_class_name_case(&expected_case_refs, label, closure_expr.span);
                    self.check_type_trait_refs(
                        &expected,
                        "return type of arrow function",
                        closure_expr.span,
                        label,
                        "return.trait",
                    );
                    if let Some(c) = self.current_class.clone() {
                        let resolved_return = self.resolve_pseudo(&expected, &c);
                        self.check_no_parent_ref(
                            &resolved_return,
                            "return type of arrow function",
                            closure_expr.span,
                            label,
                            "return.noParent",
                        );
                    }
                    if body_ty != Type::Mixed(false)
                        && !self
                            .registry
                            .accepts_at_configured_level(&body_ty, &expected)
                    {
                        self.report(
                            label.to_string(),
                            format!("return type mismatch: expected {expected}, got {body_ty}"),
                            body.span,
                            3,
                            "return.type",
                        );
                    }
                }
                true
            }
            ExprKind::Closure {
                params,
                uses,
                body,
                is_static,
                return_type,
                ..
            } => {
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return false;
                }
                if self.registry.check_native_type_hints {
                    self.suggest_closure_native_params(params, param_overrides, label);
                }
                let mut closure_locals: Locals = locals.fresh_scope();
                for u in uses {
                    // See the identical fix's own comment at
                    // `checker::expr`'s `ExprKind::Closure` arm.
                    if u.name == "this".into() {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                label.to_string(),
                                "Cannot use $this as lexical variable.".to_string(),
                                u.span,
                                0,
                                "closure.useThis",
                            )
                            .non_ignorable(),
                        );
                    } else if is_superglobal(&u.name) {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                label.to_string(),
                                format!(
                                    "Cannot use superglobal variable ${} as lexical variable.",
                                    u.name
                                ),
                                u.span,
                                0,
                                "closure.useSuperGlobal",
                            )
                            .non_ignorable(),
                        );
                    } else if params.iter().any(|p| p.name == u.name) {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                label.to_string(),
                                format!(
                                    "Cannot use lexical variable ${} since a parameter with the same name already exists.",
                                    u.name
                                ),
                                u.span,
                                0,
                                "closure.useDuplicate",
                            )
                            .non_ignorable(),
                        );
                    } else if !u.by_ref && !locals.contains_key(&u.name) {
                        self.report(
                            label.to_string(),
                            format!("undefined variable ${}", u.name),
                            u.span,
                            0,
                            "variable.undefined",
                        );
                    }
                    let ty = locals.get(&u.name).cloned().unwrap_or(Type::Mixed(false));
                    let provenance = locals.get_provenance(&u.name).cloned().unwrap_or_default();
                    if self.probe_span.get() == Some(u.span) {
                        *self.probe_var_provenance.borrow_mut() =
                            Some(provenance.iter().copied().collect());
                    }
                    let ty = if u.by_ref {
                        crate::checker::expr::unseal_array_shapes(&ty)
                    } else {
                        ty
                    };
                    closure_locals.insert_with_provenance(u.name.to_string(), ty, provenance);
                }
                if !*is_static && let Some(this_ty) = locals.get("this") {
                    closure_locals.insert("this".to_string(), this_ty.clone());
                }
                let closure_label = format!("{label} (closure)");
                for (p, override_ty) in params.iter().zip(param_overrides) {
                    let (ty, case_refs) = if p.type_hint.is_some() {
                        self.resolve_type_hint_with_case_refs(
                            p.type_hint.as_ref().map(TypeHint::to_string).as_deref(),
                        )
                    } else {
                        (override_ty.clone(), Vec::new())
                    };
                    self.check_type_class_names_exist(
                        &ty,
                        &format!("parameter ${} of anonymous function", p.name),
                        p.span,
                        &closure_label,
                    );
                    self.check_type_trait_refs(
                        &ty,
                        &format!("parameter ${} of anonymous function", p.name),
                        p.span,
                        &closure_label,
                        "parameter.trait",
                    );
                    self.check_type_class_name_case(&case_refs, &closure_label, p.span);
                    closure_locals.declare(p.name.to_string(), ty, p.name_span);
                    closure_locals.set_native(&p.name, self.closure_param_native(p));
                }
                let (declared_return_ty, declared_return_case_refs) = self
                    .resolve_type_hint_with_case_refs(
                        return_type.as_deref().map(TypeHint::to_string).as_deref(),
                    );
                let declared_return = Some(declared_return_ty);
                self.check_type_class_names_exist(
                    declared_return.as_ref().unwrap_or(&Type::Mixed(false)),
                    "return type of anonymous function",
                    closure_expr.span,
                    &closure_label,
                );
                self.check_type_trait_refs(
                    declared_return.as_ref().unwrap_or(&Type::Mixed(false)),
                    "return type of anonymous function",
                    closure_expr.span,
                    &closure_label,
                    "return.trait",
                );
                self.check_type_class_name_case(
                    &declared_return_case_refs,
                    &closure_label,
                    closure_expr.span,
                );
                if let Some(c) = self.current_class.clone() {
                    let resolved_return = self.resolve_pseudo(
                        declared_return.as_ref().unwrap_or(&Type::Mixed(false)),
                        &c,
                    );
                    self.check_no_parent_ref(
                        &resolved_return,
                        "return type of anonymous function",
                        closure_expr.span,
                        &closure_label,
                        "return.noParent",
                    );
                }
                let previous_generator_flag = self.current_function_is_generator;
                self.current_function_is_generator = body_contains_yield(body);
                let previous_declared_return_type = self.current_declared_return_type.take();
                self.current_declared_return_type =
                    declared_return.as_ref().map(|t| match &self.current_class {
                        Some(c) => self.resolve_pseudo(t, c),
                        None => t.clone(),
                    });
                let previous_goto_targets = std::mem::replace(
                    &mut self.current_function_goto_targets,
                    collect_goto_targets(body),
                );
                let previous_assigned_variables = self.enter_assigned_variables(body);
                // See `checker/expr.rs`'s `Closure` arm's identical comment.
                let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
                let previous_unannotated_call =
                    std::mem::take(&mut self.current_function_has_unannotated_call);
                let previous_explicit_throw_points =
                    std::mem::take(&mut self.current_function_explicit_throw_points);
                self.check_generator_return_type(
                    declared_return.as_ref(),
                    closure_expr.span,
                    &closure_label,
                );
                // See `checker::expr`'s `ExprKind::Closure` arm's
                // identical `converge_loop_body` fix and comment.
                let (converged, _) = self.converge_loop_body(
                    &closure_locals,
                    declared_return.as_ref(),
                    &closure_label,
                    LoopZeroIterations::Possible {
                        subject_exempt: None,
                    },
                    |this, pass_locals, declared_return, label, _pass| {
                        this.check_block(body, pass_locals, declared_return, label);
                    },
                );
                closure_locals = converged;
                self.check_closure_missing_return(
                    body,
                    return_type.is_some(),
                    declared_return.as_ref(),
                    &closure_locals,
                    closure_expr.span,
                    &closure_label,
                );
                self.current_function_is_generator = previous_generator_flag;
                self.current_declared_return_type = previous_declared_return_type;
                self.current_function_throw_points = previous_throw_points;
                self.current_function_has_unannotated_call = previous_unannotated_call;
                self.current_function_explicit_throw_points = previous_explicit_throw_points;
                self.current_function_goto_targets = previous_goto_targets;
                self.current_function_assigned_variables = previous_assigned_variables;
                self.check_unused_closure_uses(uses, body, &closure_label, closure_expr.span);
                for u in uses {
                    if !u.by_ref {
                        continue;
                    }
                    // Already the fixed-point-converged result unioned
                    // with the pre-closure entry state - see the
                    // identical fix's own comment at `checker::expr`'s
                    // `ExprKind::Closure` arm.
                    let mutated = closure_locals
                        .get(&u.name)
                        .cloned()
                        .unwrap_or(Type::Mixed(false));
                    if !locals.contains_key(&u.name) {
                        // See the identical fix's own comment at
                        // `checker::expr`'s `ExprKind::Closure` arm.
                        locals.declare(u.name.to_string(), mutated, u.span);
                        if self.probe_span.get() == Some(u.span) {
                            *self.probe_var_provenance.borrow_mut() =
                                Some([u.span].into_iter().collect());
                        }
                    } else {
                        locals.insert(u.name.to_string(), mutated);
                        // The closure may write anything to it.
                        locals.clear_native(&u.name);
                    }
                }
                true
            }
            _ => false,
        }
    }
}
