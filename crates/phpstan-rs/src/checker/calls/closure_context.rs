use crate::checker::GotoState;
use crate::checker::{Checker, Locals, LoopZeroIterations, purge_stale_narrowing_for_var};
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use types::Type;

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
        closure_expr: &ExprNode,
        locals: &mut Locals,
        label: &str,
        this_ty: Type,
        scope_class: Option<String>,
    ) {
        let mut seeded = locals.clone();
        seeded.insert_var("this", this_ty);
        if let Some(scope) = scope_class {
            let previous_class = self.current_class.replace(scope);
            self.check_expr(closure_expr, &mut seeded, label);
            self.current_class = previous_class;
        } else {
            self.check_expr(closure_expr, &mut seeded, label);
        }
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        let ExprKind::Closure(id) = &closure_expr.kind else {
            return;
        };
        let Some(ClosurePayload::Closure(closure)) = arenas.closure(*id) else {
            return;
        };
        for u in arenas
            .closure_uses(&closure.uses)
            .iter()
            .filter(|u| u.by_ref)
        {
            let Some(ty) = seeded.get_var(&u.name).cloned() else {
                continue;
            };
            purge_stale_narrowing_for_var(locals, &u.name);
            if locals.contains_var(&u.name) {
                locals.insert_var(&u.name, ty);
                locals.clear_native_var(&u.name);
            } else {
                locals.declare_var(&u.name, ty, u.name.span);
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
        closure_expr: &ExprNode,
        param_overrides: &[Type],
        locals: &mut Locals,
        label: &str,
    ) -> bool {
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return false;
        };
        let arenas = syntax_owner.arenas();
        let Some(id) = arenas.peeled(closure_expr.id) else {
            return false;
        };
        let Some(closure_expr) = arenas.expr(id) else {
            return false;
        };
        match &closure_expr.kind {
            ExprKind::ArrowFn(id) => {
                let Some(ClosurePayload::ArrowFn(arrow)) = arenas.closure(*id) else {
                    return false;
                };
                let params = arenas.params(arrow.params.clone());
                let body = arenas
                    .expr(arrow.body)
                    .expect("arrow body resolves in its owner");
                let return_type = &arrow.return_type;
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return false;
                }
                if self.settings.extras.check_native_type_hints {
                    self.suggest_closure_native_params(params, param_overrides);
                }
                let mut arrow_locals = locals.clone();
                for (p, override_ty) in params.iter().zip(param_overrides) {
                    let (ty, case_refs) = if p.type_hint.is_some() {
                        self.resolve_type_hint_with_case_refs(p.type_hint.as_ref())
                    } else {
                        (override_ty.clone(), Vec::new())
                    };
                    self.check_type_class_names_exist(
                        &ty,
                        format!("parameter ${} of arrow function", p.name),
                        p.span,
                    );
                    self.check_type_trait_refs(
                        &ty,
                        format!("parameter ${} of arrow function", p.name),
                        p.span,
                        "parameter.trait",
                    );
                    self.check_type_class_name_case(&case_refs, p.span);
                    arrow_locals.declare_var(&p.name, ty, p.name.span);
                    arrow_locals.set_native_var(&p.name, self.closure_param_native(arenas, p));
                }
                let body_ty = self.check_expr(body, &mut arrow_locals, label);
                if return_type.is_some() {
                    let (expected, expected_case_refs) =
                        self.resolve_type_hint_with_case_refs(return_type.as_deref());
                    self.check_type_class_names_exist(
                        &expected,
                        "return type of arrow function",
                        self.expr_span(closure_expr.id),
                    );
                    self.check_type_class_name_case(
                        &expected_case_refs,
                        self.expr_span(closure_expr.id),
                    );
                    self.check_type_trait_refs(
                        &expected,
                        "return type of arrow function",
                        self.expr_span(closure_expr.id),
                        "return.trait",
                    );
                    if let Some(c) = self.current_class.clone() {
                        let resolved_return = self.resolve_pseudo(&expected, &c);
                        self.check_no_parent_ref(
                            &resolved_return,
                            "return type of arrow function",
                            self.expr_span(closure_expr.id),
                            "return.noParent",
                        );
                    }
                    if !body_ty.is_mixed()
                        && let Some(from) = self.rejected_from(&expected, &body_ty)
                    {
                        self.report_found_from(
                            format!("return type mismatch: expected {expected}, got {body_ty}"),
                            self.expr_span(body.id),
                            3,
                            from,
                            "return.type",
                        );
                    }
                }
                true
            }
            ExprKind::Closure(id) => {
                let Some(ClosurePayload::Closure(closure)) = arenas.closure(*id) else {
                    return false;
                };
                let params = arenas.params(closure.params.clone());
                let uses = arenas.closure_uses(&closure.uses);
                let body = arenas.stmt_ids(&closure.body);
                let goto_labels = &closure.goto_labels;
                let is_generator = closure.is_generator;
                let is_static = closure.is_static;
                let return_type = &closure.return_type;
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return false;
                }
                if self.settings.extras.check_native_type_hints {
                    self.suggest_closure_native_params(params, param_overrides);
                }
                let mut closure_locals: Locals = locals.closure_scope();
                // Each variable captured by reference, with what the closure
                // is entered with in it.
                let mut by_ref_entries: Vec<(syntax::name::Name, Type)> = Vec::new();
                for u in uses {
                    // See the identical fix's own comment at
                    // `checker::expr`'s `ExprKind::Closure` arm.
                    if u.name == "this" {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                "Cannot use $this as lexical variable.".to_string(),
                                u.name.span,
                                0,
                                "closure.useThis",
                            )
                            .non_ignorable(),
                        );
                    } else if is_superglobal(&u.name) {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                format!(
                                    "Cannot use superglobal variable ${} as lexical variable.",
                                    u.name
                                ),
                                u.name.span,
                                0,
                                "closure.useSuperGlobal",
                            )
                            .non_ignorable(),
                        );
                    } else if params.iter().any(|p| p.name.name == u.name.name) {
                        self.report_builder(
                            DiagnosticBuilder::new(format!(
                                    "Cannot use lexical variable ${} since a parameter with the same name already exists.",
                                    u.name
                                ),
                                u.name.span,
                                0,
                                "closure.useDuplicate",
                            )
                            .non_ignorable(),
                        );
                    } else if !u.by_ref && !locals.contains_var(&u.name) {
                        self.report(
                            format!("undefined variable ${}", u.name),
                            u.name.span,
                            0,
                            "variable.undefined",
                        );
                    }
                    let ty = locals.get_var(&u.name).cloned().unwrap_or(Type::mixed());
                    let provenance = locals
                        .get_provenance_var(&u.name)
                        .cloned()
                        .unwrap_or_default();
                    if self.probe_span.get() == Some(u.name.span) {
                        *self.probe_var_provenance.borrow_mut() =
                            Some(provenance.iter().copied().collect());
                    }
                    // See `checker::expr`'s `ExprKind::Closure` arm.
                    let ty = if u.by_ref {
                        let ty =
                            self.capture_entry(self.expr_span(closure_expr.id), &u.name.name, ty);
                        by_ref_entries.push((u.name.name.clone(), ty.clone()));
                        ty.unsealed_shapes()
                    } else {
                        ty
                    };
                    closure_locals.insert_with_provenance_var(&u.name, ty, provenance);
                }
                if !is_static && let Some(this_ty) = locals.get_var("this") {
                    closure_locals.insert_var("this", this_ty.clone());
                }
                let closure_label = format!("{label} (closure)");
                for (p, override_ty) in params.iter().zip(param_overrides) {
                    let (ty, case_refs) = if p.type_hint.is_some() {
                        self.resolve_type_hint_with_case_refs(p.type_hint.as_ref())
                    } else {
                        (override_ty.clone(), Vec::new())
                    };
                    self.check_type_class_names_exist(
                        &ty,
                        format!("parameter ${} of anonymous function", p.name),
                        p.span,
                    );
                    self.check_type_trait_refs(
                        &ty,
                        format!("parameter ${} of anonymous function", p.name),
                        p.span,
                        "parameter.trait",
                    );
                    self.check_type_class_name_case(&case_refs, p.span);
                    closure_locals.declare_var(&p.name, ty, p.name.span);
                    closure_locals.set_native_var(&p.name, self.closure_param_native(arenas, p));
                }
                let (declared_return_ty, declared_return_case_refs) =
                    self.resolve_type_hint_with_case_refs(return_type.as_deref());
                let declared_return = Some(declared_return_ty);
                self.check_type_class_names_exist(
                    declared_return.as_ref().unwrap_or(&Type::mixed()),
                    "return type of anonymous function",
                    self.expr_span(closure_expr.id),
                );
                self.check_type_trait_refs(
                    declared_return.as_ref().unwrap_or(&Type::mixed()),
                    "return type of anonymous function",
                    self.expr_span(closure_expr.id),
                    "return.trait",
                );
                self.check_type_class_name_case(
                    &declared_return_case_refs,
                    self.expr_span(closure_expr.id),
                );
                if let Some(c) = self.current_class.clone() {
                    let resolved_return =
                        self.resolve_pseudo(declared_return.as_ref().unwrap_or(&Type::mixed()), &c);
                    self.check_no_parent_ref(
                        &resolved_return,
                        "return type of anonymous function",
                        self.expr_span(closure_expr.id),
                        "return.noParent",
                    );
                }
                let previous_generator_flag = self.current_function_is_generator;
                self.current_function_is_generator = is_generator;
                let previous_declared_return_type = self.current_declared_return_type.take();
                self.current_declared_return_type =
                    declared_return.as_ref().map(|t| match &self.current_class {
                        Some(c) => self.resolve_pseudo(t, c),
                        None => t.clone(),
                    });
                let previous_goto_targets = std::mem::replace(
                    &mut self.current_function_goto_targets,
                    GotoState::new(goto_labels.to_owned()),
                );
                let previous_assigned_variables = self.enter_assigned_variables(arenas, body);
                let enclosing_scope =
                    self.enter_shared_scope(crate::checker::shared_scope::SharedScope {
                        closure: true,
                        ..crate::checker::shared_scope::SharedScope::of(
                            self.registry,
            arenas,
                            body,
                            uses,
                            Some((
                                self.current_class.as_deref().map(syntax::name::Name::from),
                                syntax::name::Name::from("{closure}"),
                            )),
                            false,
                        )
                    });
                // The closure's own by-reference state, as `checker::expr`'s
                // `ExprKind::Closure` arm keeps it: its captures by
                // reference are snapshotted where its body returns.
                let previous_by_ref_params =
                    std::mem::take(&mut self.current_function_by_ref_params);
                let previous_by_ref_exit_snapshots =
                    std::mem::take(&mut self.current_function_by_ref_exit_snapshots);
                let previous_by_ref_params_with_tag =
                    std::mem::take(&mut self.current_function_by_ref_params_with_tag);
                let previous_byref_snapshot_names = std::mem::replace(
                    &mut self.current_function_byref_snapshot_names,
                    uses.iter()
                        .filter(|u| u.by_ref)
                        .map(|u| u.name.name.clone())
                        .collect(),
                );
                // See `checker/expr.rs`'s `Closure` arm's identical comment.
                let previous_throw_points = std::mem::take(&mut self.current_function_throw_points);
                let previous_try_throw_states = std::mem::take(&mut self.try_throw_states);
                let previous_unannotated_call =
                    std::mem::take(&mut self.current_function_has_unannotated_call);
                let previous_explicit_throw_points =
                    std::mem::take(&mut self.current_function_explicit_throw_points);
                let previous_exit_points = std::mem::take(&mut self.current_function_exit_points);
                self.check_generator_return_type(
                    declared_return.as_ref(),
                    self.expr_span(closure_expr.id),
                );
                // See `checker::expr`'s `ExprKind::Closure` arm's
                // identical `converge_loop_body` fix and comment.
                let mut body_terminates = false;
                let by_ref_names: Vec<syntax::name::Name> = uses
                    .iter()
                    .filter(|u| u.by_ref)
                    .map(|u| u.name.name.clone())
                    .collect();
                let mut entry_locals = closure_locals.clone();
                let (converged, _) =
                    self.check_capturing_scope(&mut entry_locals, |this, entry| {
                        this.converge_loop_body(
                            entry,
                            declared_return.as_ref(),
                            &closure_label,
                            LoopZeroIterations::Possible {
                                subject_exempt: None,
                                subject_key: None,
                            },
                            |this, pass_locals, declared_return, label, _pass| {
                                body_terminates = this
                                    .check_block(body, pass_locals, declared_return, label)
                                    .completion
                                    .terminates;
                                this.fold_returned_captures(&by_ref_names, pass_locals);
                            },
                        )
                    });
                closure_locals = converged;
                self.record_by_ref_exit_snapshot(&closure_locals);
                let closure_byref_exit_snapshots = std::mem::replace(
                    &mut self.current_function_by_ref_exit_snapshots,
                    previous_by_ref_exit_snapshots,
                );
                self.current_function_by_ref_params = previous_by_ref_params;
                self.current_function_by_ref_params_with_tag = previous_by_ref_params_with_tag;
                self.current_function_byref_snapshot_names = previous_byref_snapshot_names;
                self.check_closure_missing_return(
                    body_terminates,
                    return_type.is_some(),
                    declared_return.as_ref(),
                    self.expr_span(closure_expr.id),
                );
                self.current_function_is_generator = previous_generator_flag;
                self.current_declared_return_type = previous_declared_return_type;
                self.current_function_throw_points = previous_throw_points;
                self.try_throw_states = previous_try_throw_states;
                self.current_function_has_unannotated_call = previous_unannotated_call;
                self.current_function_explicit_throw_points = previous_explicit_throw_points;
                self.current_function_exit_points = previous_exit_points;
                self.current_function_goto_targets = previous_goto_targets;
                self.current_function_assigned_variables = previous_assigned_variables;
                self.leave_shared_scope(enclosing_scope);
                self.check_unused_closure_uses(uses, body, self.expr_span(closure_expr.id));
                self.write_back_captures(
                    self.expr_span(closure_expr.id),
                    uses,
                    &by_ref_entries,
                    &closure_byref_exit_snapshots,
                    &closure_locals,
                    locals,
                );
                true
            }
            _ => false,
        }
    }
}
