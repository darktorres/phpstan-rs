use crate::ast::*;
use crate::checker::{Checker, Locals, body_contains_yield, collect_goto_targets, union_types};
use crate::types::Type;

impl<'a> Checker<'a> {
    /// Checks a closure/arrow-fn literal's body with `$this` seeded to
    /// `this_ty` instead of whatever (if anything) the enclosing scope's
    /// own `locals` would otherwise provide - used by `Closure::bind`/
    /// `->bindTo()` (Finding 49) and Pest's own file-scoped `$this`
    /// binding, all of which rebind `$this` independently of where the
    /// closure was lexically declared. A clone of `locals`, not a
    /// mutation of it: the seeded `$this` (and anything the closure's
    /// own body binds) must not leak back into the caller's real scope,
    /// matching how `ExprKind::Closure`'s own ordinary handling already
    /// keeps `closure_locals` separate.
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
        locals: &Locals,
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
    }

    /// Phase 53: `array_map`'s own contextual parameter-type injection -
    /// checks a closure/arrow-fn literal argument's own *body* with each
    /// unhinted parameter bound to the matching entry of `param_
    /// overrides` (the mapped array's own element type) instead of the
    /// `Mixed` an unhinted parameter otherwise gets, catching a real bug
    /// inside the callback itself (e.g. calling an undefined method on
    /// the mapped element) - something `infer_closure_return_with_
    /// param_overrides` (return-type inference only, `&self`, no
    /// `report(...)` access) can never surface a diagnostic for. A
    /// parameter with its own declared type hint keeps that hint -
    /// contextual inference only fills in what's otherwise `Mixed`,
    /// mirroring real PHPStan's own precedence (a declared type is
    /// always authoritative over an inferred one). Mirrors the ordinary
    /// `ExprKind::Closure`/`ArrowFn` arms in `check_expr` almost
    /// verbatim - the override is the only real difference - so a bug
    /// fixed there needs the same fix reflected here. Scoped to the same
    /// shape `infer_closure_return_with_param_overrides` already
    /// restricts itself to: `false` (falls back to the caller's own
    /// plain `check_expr`) whenever the override count doesn't exactly
    /// match the closure/arrow-fn's own parameter count, or any
    /// parameter is variadic or by-ref.
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
                let mut arrow_locals = locals.clone();
                for (p, override_ty) in params.iter().zip(param_overrides) {
                    let ty = if p.type_hint.is_some() {
                        self.resolve_type_hint(p.type_hint.as_deref())
                    } else {
                        override_ty.clone()
                    };
                    self.check_type_class_names_exist(
                        &ty,
                        &format!("parameter ${} of arrow function", p.name),
                        p.span,
                        label,
                    );
                    arrow_locals.declare(p.name.clone(), ty, p.name_span);
                }
                let body_ty = self.check_expr(body, &mut arrow_locals, label);
                if return_type.is_some() {
                    let expected = self.resolve_type_hint(return_type.as_deref());
                    self.check_type_class_names_exist(
                        &expected,
                        "return type of arrow function",
                        closure_expr.span,
                        label,
                    );
                    if body_ty != Type::Mixed && !self.registry.is_assignable(&body_ty, &expected) {
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
                let mut closure_locals: Locals = locals.fresh_scope();
                for u in uses {
                    let ty = locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    let provenance = locals.get_provenance(&u.name).cloned().unwrap_or_default();
                    closure_locals.insert_with_provenance(u.name.clone(), ty, provenance);
                }
                if !*is_static && let Some(this_ty) = locals.get("this") {
                    closure_locals.insert("this".to_string(), this_ty.clone());
                }
                let closure_label = format!("{label} (closure)");
                for (p, override_ty) in params.iter().zip(param_overrides) {
                    let ty = if p.type_hint.is_some() {
                        self.resolve_type_hint(p.type_hint.as_deref())
                    } else {
                        override_ty.clone()
                    };
                    self.check_type_class_names_exist(
                        &ty,
                        &format!("parameter ${} of anonymous function", p.name),
                        p.span,
                        &closure_label,
                    );
                    closure_locals.declare(p.name.clone(), ty, p.name_span);
                }
                let declared_return = Some(self.resolve_type_hint(return_type.as_deref()));
                self.check_type_class_names_exist(
                    declared_return.as_ref().unwrap_or(&Type::Mixed),
                    "return type of anonymous function",
                    closure_expr.span,
                    &closure_label,
                );
                let previous_generator_flag = self.current_function_is_generator;
                self.current_function_is_generator = body_contains_yield(body);
                let previous_goto_targets = std::mem::replace(
                    &mut self.current_function_goto_targets,
                    collect_goto_targets(body),
                );
                self.check_generator_return_type(
                    declared_return.as_ref(),
                    closure_expr.span,
                    &closure_label,
                );
                self.check_block(
                    body,
                    &mut closure_locals,
                    declared_return.as_ref(),
                    &closure_label,
                );
                self.current_function_is_generator = previous_generator_flag;
                self.current_function_goto_targets = previous_goto_targets;
                self.check_unused_closure_uses(uses, body, &closure_label, closure_expr.span);
                for u in uses {
                    if !u.by_ref {
                        continue;
                    }
                    let original = locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    let mutated = closure_locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    locals.insert(u.name.clone(), union_types(original, mutated));
                }
                true
            }
            _ => false,
        }
    }
}
