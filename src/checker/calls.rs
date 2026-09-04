use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::checker::{
    Checker, DiagnosticBuilder, Locals, body_contains_yield, literal_string_value,
    narrow_type_excluding, purge_stale_narrowing_for, union_types, unwrap_grouping,
};
use crate::registry::{FunctionSig, ParamSig};
use crate::token::Span;
use crate::types::{self, Type};

/// `check_args`'s own call bundle. `callee_label`/`label` are two
/// adjacent, identically-typed `&str` parameters (the callee's own
/// display name vs. the enclosing function/method being checked) - a
/// real mis-ordering risk a positional call site can't catch, which
/// named struct fields fix. `args` is the raw, as-written argument list
/// (needed for the two count-based checks `check_args` does, both of
/// which are about how many arguments were actually written, not which
/// parameter each one resolves to); `resolved` is Item 1's own per-
/// parameter resolution of that same list (`resolve_call_args`), used
/// for the "too few" check's own missing-required-parameter test
/// (correct for a named call that fills required parameters out of
/// order, where a raw argument *count* alone can't tell) and for the
/// per-argument type-checking loop.
pub(crate) struct CheckArgsCall<'a> {
    pub(crate) callee_label: &'a str,
    pub(crate) sig: &'a FunctionSig,
    pub(crate) args: &'a [Arg],
    pub(crate) resolved: &'a [Option<&'a Expr>],
    pub(crate) locals: &'a Locals,
    pub(crate) label: &'a str,
    pub(crate) defining_class: Option<&'a str>,
    pub(crate) span: Span,
}

impl<'a> Checker<'a> {
    /// A bare-variable argument that isn't defined yet is either a real bug
    /// or PHP's by-reference "output parameter" idiom
    /// (`preg_match($pattern, $subject, $matches)` defines `$matches` as a
    /// side effect). Without tracking every stdlib function's exact
    /// by-ref positions, the safe default for a call to an *unknown*
    /// function is to assume it might be exactly that and define the
    /// variable (as Mixed) rather than flag it; a *known* function only
    /// gets this leniency for the specific parameter position actually
    /// marked by-ref.
    /// `sig: None` (an unresolvable call) conservatively treats *any*
    /// bare-variable argument, in its own written position, as a
    /// definition site - the real reason this function exists at all
    /// (see its own callers' comments on `isset`/`empty`). Once a real
    /// signature is known, `resolved` (Item 1's own per-parameter
    /// resolution, already computed by the caller) is used instead of the
    /// raw argument list - a named or reordered-named by-ref argument
    /// needs to be matched against its *real* declared parameter, not
    /// whichever parameter happens to sit at the same index it was
    /// written at.
    pub(crate) fn predefine_byref_or_unknown_args(
        &self,
        call_name: Option<&str>,
        sig: Option<&FunctionSig>,
        args: &[Arg],
        resolved: &[Option<&Expr>],
        locals: &mut Locals,
    ) {
        let Some(sig) = sig else {
            for arg in args {
                let ExprKind::Variable(name) = &arg.value.kind else {
                    continue;
                };
                locals.entry(name.clone()).or_insert(Type::Mixed);
            }
            return;
        };
        for (i, slot) in resolved.iter().enumerate() {
            let Some(arg_expr) = slot else {
                continue;
            };
            let ExprKind::Variable(name) = &arg_expr.kind else {
                continue;
            };
            // Both the by-ref flag and (Phase 22 Finding 3) the declared
            // parameter itself need the identical primary-then-variadic-
            // fallback lookup - `resolved` can extend past `sig.params`'
            // own length for a variadic tail argument (confirmed via
            // `resolve_call_args`'s own `resolved.push(...)` there).
            let param = sig
                .params
                .get(i)
                .or_else(|| sig.params.last().filter(|p| p.variadic));
            let Some(param) = param else {
                continue;
            };
            if !param.by_ref {
                continue;
            }
            if !locals.contains_key(name) {
                locals.insert(name.clone(), Type::Mixed);
                continue;
            }
            // A round of Piwigo-corpus bucketing found the sort() family's
            // own real PHPStan dynamic type-specifying extensions weren't
            // modeled at all - every one fell into the generic widening
            // below, which discards precision down to the declared
            // `array &$array`/`array &$arg` param type regardless of what
            // the function actually does to it. Verified via dumpType():
            // sort/rsort/usort/shuffle always reindex their by-ref
            // argument to a real list, preserving the input's own element
            // type (and non-emptiness, when already known); asort/arsort/
            // ksort/krsort/uasort/uksort/natsort/natcasesort preserve the
            // exact key structure - and so the exact type - unchanged.
            if let Some(fn_name) = call_name {
                let current = locals.get(name).cloned().unwrap_or(Type::Mixed);
                match fn_name {
                    "sort" | "rsort" | "usort" | "shuffle" => {
                        if let Some(elem_ty) = Checker::array_element_type(&current) {
                            let mut members = vec![
                                Type::Array(Box::new(Type::unknown_array_key()), Box::new(elem_ty)),
                                Type::Accessory(types::AccessoryKind::ArrayList),
                            ];
                            if Checker::is_known_non_empty_array(&current) {
                                members.push(Type::Accessory(types::AccessoryKind::NonEmptyArray));
                            }
                            purge_stale_narrowing_for(locals, name);
                            locals.insert(name.clone(), Type::Intersection(members));
                            continue;
                        }
                    }
                    "asort" | "arsort" | "ksort" | "krsort" | "uasort" | "uksort" | "natsort"
                    | "natcasesort" => continue,
                    // A Piwigo-corpus round found this same "generic
                    // widening loses list-ness" gap for the other real,
                    // common by-ref array mutators: `array_push`/
                    // `array_unshift` always append/prepend at least one
                    // real value, so a list-like (or empty, per
                    // `Checker::is_list_like`'s existing "array{}
                    // satisfies list<T> vacuously" rule) `$array` stays a
                    // list afterward, now provably non-empty - element
                    // type widens to include whatever was actually
                    // pushed. Deliberately not attempting a non-list-like
                    // input (a real `array_push` on one keeps whatever
                    // odd key structure it already had, appending fresh
                    // integer keys past the existing max - not a
                    // "becomes a list" case at all) - falls through to
                    // the generic declared-type widening below unchanged.
                    "array_push" | "array_unshift" if Checker::is_list_like(&current) => {
                        if let Some(elem_ty) = Checker::array_element_type(&current) {
                            let pushed_ty = args[1..]
                                .iter()
                                .map(|a| self.infer_expr(&a.value, locals))
                                .reduce(union_types);
                            let combined_elem = match pushed_ty {
                                Some(pushed_ty) => union_types(elem_ty, pushed_ty),
                                None => elem_ty,
                            };
                            let members = vec![
                                Type::Array(
                                    Box::new(Type::unknown_array_key()),
                                    Box::new(combined_elem),
                                ),
                                Type::Accessory(types::AccessoryKind::ArrayList),
                                Type::Accessory(types::AccessoryKind::NonEmptyArray),
                            ];
                            purge_stale_narrowing_for(locals, name);
                            locals.insert(name.clone(), Type::Intersection(members));
                            continue;
                        }
                    }
                    // `array_shift`/`array_pop` remove an element rather
                    // than adding one - list-ness survives the same way
                    // (both reindex/leave sequential integer keys
                    // sequential), but non-emptiness doesn't: removing
                    // the one remaining element of a single-element list
                    // leaves `[]`, so any existing non-empty marker is
                    // deliberately dropped rather than carried through.
                    "array_shift" | "array_pop" if Checker::is_list_like(&current) => {
                        if let Some(elem_ty) = Checker::array_element_type(&current) {
                            let members = vec![
                                Type::Array(Box::new(Type::unknown_array_key()), Box::new(elem_ty)),
                                Type::Accessory(types::AccessoryKind::ArrayList),
                            ];
                            purge_stale_narrowing_for(locals, name);
                            locals.insert(name.clone(), Type::Intersection(members));
                            continue;
                        }
                    }
                    _ => {}
                }
            }
            // Phase 22 Finding 3: an *already-defined* variable passed
            // by-ref was previously left untouched entirely (only an
            // undefined one got the "assume Mixed" leniency above) - a
            // real corpus example (`Piwigo\Admin\Install\InstallService::
            // installDbConnect(array &$infos, array &$errors, ...)`
            // called as `installDbConnect($infos, $errors, ...)` where
            // `$errors` was already `$errors = [];`) kept its stale,
            // narrow pre-call type through the call even though the
            // callee is known (by signature) to mutate it by reference.
            // Widen to the *declared parameter's own type*, not a
            // blanket `Type::Mixed` - `array &$errors` becomes
            // `Type::Array(Mixed)`, preserving the real, guaranteed-by-
            // the-type-hint information that it's still an array, which
            // blanket `Mixed` would needlessly discard. `substitute_
            // template` with empty bindings replaces any unresolved
            // `TemplateParam` in the declared type with `Mixed` too -
            // proper substitution would need the receiver's own type
            // arguments, not available at this call site, so an
            // unresolved generic by-ref parameter degrades to the same
            // safe `Mixed` the undefined-variable case already uses.
            //
            // Only trust the substituted declared type when it's itself
            // plainly array-shaped (`Type::Array`/`Type::ArrayShape`) -
            // found via real corpus verification, not assumed: several
            // real stdlib by-ref parameters (`array_walk(array|object
            // &$array, ...)`, confirmed in the vendored phpstorm-stubs)
            // declare a *union* wider than what the already-array-typed
            // variable being passed in could actually become - the
            // parameter type says what the callee *accepts*, not what an
            // already-narrower-typed argument turns into, so blindly
            // trusting a `Union`/`Object`/other declared shape here
            // overshoots into new noise once the corpus is checked, not
            // just less precision. Anything else falls back to `Mixed`,
            // exactly this function's own pre-existing behavior for the
            // undefined-variable case above.
            let substituted = types::substitute_template(&param.ty, &HashMap::new());
            // Phase 14: `Checker::is_array_like` also trusts a `list<T>`/
            // `non-empty-array<T>`-declared by-ref parameter (a real
            // `Intersection` now) - without it, this fell into the same
            // "anything else" `Mixed` catch-all a wider declared `Union`/
            // `Object` already deliberately does, discarding real,
            // already-available precision.
            let widened = if Checker::is_array_like(&substituted) {
                substituted
            } else {
                Type::Mixed
            };
            // Phase 33 Finding 5: a by-ref call argument is exactly as
            // much a "this variable now holds something else" event as a
            // direct assignment - without this, a stale composite key
            // rooted at `name` (from narrowing before this call) would
            // keep answering for whatever the callee may have replaced it
            // with.
            purge_stale_narrowing_for(locals, name);
            locals.insert(name.clone(), widened);
        }
    }

    /// Phase 37: the statement half of real PHPStan's own
    /// `CallToFunctionStatementWithNoDiscardRule`/
    /// `CallToMethodStatementWithNoDiscardRule`/
    /// `CallToStaticMethodStatementWithNoDiscardRule` - a bare call
    /// (`foo();`, `$obj->foo();`, `Foo::bar();`) sitting on its own line
    /// as a whole statement, with its return value neither assigned,
    /// returned, nor otherwise consumed, is flagged when the resolved
    /// function/method's own signature carries PHP 8.5's `#[\NoDiscard]`
    /// marker (`FunctionSig::no_discard`, Phase 37's own registry-side
    /// half). Called once per `StmtKind::Expression`, on the statement's
    /// own top-level expression only - a `NoDiscard` call anywhere else
    /// (an argument, the right side of `=`, one operand of a larger
    /// expression) already has its result consumed by whatever it's
    /// nested inside, so it's not this rule's concern at all. Real
    /// PHPStan's own rule also special-cases a `(void)`-cast call as a
    /// separate, weaker diagnostic (still discarding, but explicitly
    /// acknowledged) - not modeled here, since this project's own
    /// `Cast` handling has no reason to distinguish `(void)` from any
    /// other cast target yet; a `(void)`-cast `NoDiscard` call is simply
    /// not flagged at all, the safe (false-negative, never false-
    /// positive) direction to skip in.
    pub(crate) fn check_no_discard_statement(&mut self, e: &Expr, locals: &Locals, label: &str) {
        let (sig, callee_desc, identifier) = match &e.kind {
            ExprKind::Call(callee, _) => {
                let ExprKind::Name(name) = &callee.kind else {
                    return;
                };
                if is_language_construct(name) {
                    return;
                }
                let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
                let Some((resolved_name, sig)) = self
                    .registry
                    .lookup_function(&primary)
                    .map(|sig| (primary.as_str(), sig))
                    .or_else(|| {
                        fallback
                            .as_deref()
                            .and_then(|g| self.registry.lookup_function(g).map(|sig| (g, sig)))
                    })
                else {
                    return;
                };
                (
                    sig,
                    format!("{resolved_name}()"),
                    "function.resultDiscarded",
                )
            }
            ExprKind::MethodCall(recv, method, _, _) => {
                let recv_ty = narrow_type_excluding(&self.infer_expr(recv, locals), &Type::Null);
                let Type::Object(class, _) = recv_ty else {
                    return;
                };
                let Some((owner, sig)) = self.registry.lookup_method_with_owner(&class, method)
                else {
                    return;
                };
                (
                    sig,
                    format!("{owner}::{method}()"),
                    "method.resultDiscarded",
                )
            }
            ExprKind::StaticCall(class_name, method, _) => {
                let Some(class) = self.resolve_static_class_name(class_name) else {
                    return;
                };
                let Some(sig) = self.registry.lookup_method(&class, method) else {
                    return;
                };
                (
                    sig,
                    format!("{class}::{method}()"),
                    "staticMethod.resultDiscarded",
                )
            }
            _ => return,
        };
        if !sig.no_discard {
            return;
        }
        self.report_builder(
            DiagnosticBuilder::new(
                label.to_string(),
                format!("Call to {callee_desc} on a separate line discards its return value."),
                e.span,
                0,
                identifier,
            )
            .non_ignorable(),
        );
    }

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
                    arrow_locals.insert(p.name.clone(), ty);
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
                let mut closure_locals: Locals = Locals::default();
                for u in uses {
                    let ty = locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    closure_locals.insert(u.name.clone(), ty);
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
                    closure_locals.insert(p.name.clone(), ty);
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

    pub(crate) fn check_args(&mut self, call: CheckArgsCall) {
        let CheckArgsCall {
            callee_label,
            sig,
            args,
            resolved,
            locals,
            label,
            defining_class,
            span,
        } = call;
        // Item 1 + Item 2: naming/ordering legality first - independent
        // of, and prior to, every count/type check below.
        self.check_argument_legality(callee_label, sig, args, label);

        // Item 2: a spread/unpack argument's own real contents can't be
        // resolved statically (see `resolve_call_args`'s own doc
        // comment) - every declared parameter at or past the positional
        // index the spread occupies might still be filled by it at
        // runtime, so neither the "missing required parameter" nor the
        // "too many arguments" check below can trust a `None` slot (or a
        // raw `args.len()`) at or past that point to mean what it
        // normally would. `None` here (rather than skipping the whole
        // call the way this project previously did entirely) still lets
        // both checks run precisely for everything *before* the spread.
        let spread_from = first_spread_positional_index(args);
        let required = sig
            .params
            .iter()
            .filter(|p| !p.variadic && !p.has_default)
            .count();
        let required_scan_len = spread_from.map_or(required, |sp| sp.min(required));
        let missing_required = resolved
            .iter()
            .take(required_scan_len)
            .filter(|s| s.is_none())
            .count();
        if missing_required > 0 {
            self.report(
                label.to_string(),
                format!(
                    "{callee_label}() expects at least {required} argument(s), {} given",
                    args.len()
                ),
                span,
                0,
                "arguments.count",
            );
        }

        // Finding 40: calling a non-variadic function/method with more
        // positional arguments than it can possibly accept is a real PHP
        // fatal error ("Too many arguments... expected exactly N"), just
        // as basic a check as the "too few" one right above it - but
        // nothing here caught it before, since the per-argument loop
        // below simply stops (via its own fallback once `sig.params.get(i)`
        // runs out and the last param isn't variadic) rather than
        // reporting anything. Uses the raw `args.len()` deliberately, not
        // `resolved`'s own length - a non-variadic signature's resolved
        // slice is always exactly `params.len()` long by
        // `resolve_call_args`'s own design, which would make this check
        // always read `false` if it read from that shape instead.
        let last_is_variadic = sig.params.last().is_some_and(|p| p.variadic);
        if spread_from.is_none() && !last_is_variadic && args.len() > sig.params.len() {
            let quantifier = if required == sig.params.len() {
                "exactly"
            } else {
                "at most"
            };
            self.report(
                label.to_string(),
                format!(
                    "{callee_label}() expects {quantifier} {} argument(s), {} given",
                    sig.params.len(),
                    args.len()
                ),
                span,
                0,
                "arguments.count",
            );
        }

        for (i, slot) in resolved.iter().enumerate() {
            let Some(arg) = slot else {
                continue;
            };
            let param = match sig.params.get(i) {
                Some(p) => p,
                None => match sig.params.last() {
                    Some(p) if p.variadic => p,
                    _ => continue,
                },
            };
            // By-ref target legality: independent of - and checked
            // before - the type check below, since it's about whether
            // `arg` is even a real storage location at all, not what
            // its value's type is. A nullsafe-chain root is checked
            // first and reported with its own, more specific reason
            // (short-circuiting, not "not an lvalue") even though a
            // nullsafe chain is *also* not one of `is_valid_byref_
            // argument_expr`'s legal shapes.
            if param.by_ref {
                if contains_nullsafe_access(arg) {
                    self.report(
                        label.to_string(),
                        format!(
                            "{callee_label}() argument #{} (${}) is passed by reference, but its access chain contains a nullsafe operator (?->), which may short-circuit and never write back",
                            i + 1,
                            param.name
                        ),
                        arg.span,
                        0,
                        "argument.byRef",
                    );
                } else if !is_valid_byref_argument_expr(arg) {
                    self.report(
                        label.to_string(),
                        format!(
                            "{callee_label}() argument #{} (${}) is passed by reference, so it expects a variable, not an expression",
                            i + 1,
                            param.name
                        ),
                        arg.span,
                        0,
                        "argument.byRef",
                    );
                }
            }
            let arg_ty = self.infer_expr(arg, locals);
            if arg_ty == Type::Mixed {
                continue;
            }
            let param_ty = match defining_class {
                Some(c) => self.resolve_pseudo(&param.ty, c),
                None => param.ty.clone(),
            };
            if !self.registry.is_assignable(&arg_ty, &param_ty) {
                self.report(
                    label.to_string(),
                    format!(
                        "{callee_label}() argument #{} (${}) expects {}, {} given",
                        i + 1,
                        param.name,
                        param_ty,
                        arg_ty
                    ),
                    arg.span,
                    5,
                    "argument.type",
                );
            }
        }
    }

    /// Phase 49 Item 1: validates a call's own argument list for the
    /// naming/ordering legality real PHP itself enforces, independent of
    /// which parameter each argument actually resolves to
    /// (`resolve_call_args`'s own job, used by every other check in
    /// `check_args`). Mirrors real PHPStan's own
    /// `FunctionCallParametersCheck::processArguments` plus its raw-args
    /// ordering pre-pass, reusing its identifiers verbatim
    /// (`argument.unknown`, `argument.duplicate`,
    /// `argument.positionalAfterNamed`, `argument.unpackAfterNamed`,
    /// `argument.nonUnpackAfterUnpacked`).
    ///
    /// The three ordering checks are deliberately `else if` against each
    /// other (real PHPStan instead runs an ordering pre-pass over the raw
    /// argument list and a separate, later pass over an *expanded*
    /// argument list - one that turns a spread over a known constant
    /// array into one synthetic argument per element - to get
    /// `positionalAfterNamed` from a genuinely different pass than
    /// `unpackAfterNamed`/`nonUnpackAfterUnpacked`; this project doesn't
    /// attempt that constant-array expansion, so the three collapse onto
    /// the same raw-args loop here) - a single malformed argument only
    /// ever gets one ordering complaint, not a pile of overlapping ones
    /// for what's really the same mistake.
    fn check_argument_legality(
        &mut self,
        callee_label: &str,
        sig: &FunctionSig,
        args: &[Arg],
        label: &str,
    ) {
        let variadic = sig.params.last().is_some_and(|p| p.variadic);
        let mut named_seen = false;
        let mut spread_seen = false;
        let mut used_param_names: HashSet<&str> = HashSet::new();
        let mut positional_index = 0usize;

        for arg in args {
            if named_seen && arg.name.is_none() && !arg.spread {
                self.report(
                    label.to_string(),
                    format!(
                        "named argument cannot be followed by a positional argument in call to {callee_label}()"
                    ),
                    arg.value.span,
                    0,
                    "argument.positionalAfterNamed",
                );
            } else if named_seen && arg.spread {
                self.report(
                    label.to_string(),
                    format!(
                        "named argument cannot be followed by an unpacked (...) argument in call to {callee_label}()"
                    ),
                    arg.value.span,
                    0,
                    "argument.unpackAfterNamed",
                );
            } else if spread_seen && arg.name.is_none() && !arg.spread {
                self.report(
                    label.to_string(),
                    format!(
                        "unpacked (...) argument cannot be followed by a non-unpacked argument in call to {callee_label}()"
                    ),
                    arg.value.span,
                    0,
                    "argument.nonUnpackAfterUnpacked",
                );
            }

            if arg.spread {
                spread_seen = true;
                continue;
            }

            if let Some(name) = &arg.name {
                named_seen = true;
                match sig.params.iter().find(|p| &p.name == name) {
                    // A named argument targeting the real, declared
                    // variadic parameter's own name (rather than an
                    // unmatched name collected *into* it, the `None`
                    // arm below) is just an ordinary named argument.
                    Some(_) => {
                        if !used_param_names.insert(name.as_str()) {
                            self.report(
                                label.to_string(),
                                format!(
                                    "argument for parameter ${name} has already been passed to {callee_label}()"
                                ),
                                arg.value.span,
                                0,
                                "argument.duplicate",
                            );
                        }
                    }
                    // No declared parameter has this name - legal only
                    // when there's a trailing variadic parameter to
                    // collect it into (real PHP's own named-argument-
                    // into-variadic support), and then only once per
                    // name.
                    None if variadic => {
                        if !used_param_names.insert(name.as_str()) {
                            self.report(
                                label.to_string(),
                                format!(
                                    "named parameter ${name} overwrites a previous argument to {callee_label}()"
                                ),
                                arg.value.span,
                                0,
                                "argument.duplicate",
                            );
                        }
                    }
                    None => {
                        self.report(
                            label.to_string(),
                            format!("unknown named parameter ${name} in call to {callee_label}()"),
                            arg.value.span,
                            0,
                            "argument.unknown",
                        );
                    }
                }
                continue;
            }

            // A plain positional argument, still legally placeable
            // (nothing above flagged it): record which declared
            // parameter it claims, the same way a named argument does
            // above, so a *later* named argument re-targeting that same
            // parameter is caught as a duplicate too.
            if !spread_seen && let Some(p) = sig.params.get(positional_index) {
                used_param_names.insert(p.name.as_str());
            }
            positional_index += 1;
        }
    }

    /// Phase 12 item 3 / Phase 69: validates a `sprintf`/`printf`/
    /// `fprintf`/`vsprintf`/`vprintf` call's format string against the
    /// arguments actually passed - only when the format string is a
    /// literal (mirrors `check_shape_key_access`'s own "only a literal,
    /// never a dynamic value" precedent). `fprintf`'s format string is
    /// its *second* argument (`fprintf(resource $stream, string
    /// $format, mixed ...$values)`), not its first - otherwise identical
    /// to `sprintf`/`printf`. `vsprintf`/`vprintf` take their values as
    /// one array argument rather than variadically - only checked when
    /// that argument is itself a literal (or otherwise a real
    /// `ArrayShape` with sequential canonical-int keys - a plain list
    /// literal in every way that matters here), the same "don't guess
    /// past what's statically known" restriction the format string
    /// itself already has.
    pub(crate) fn check_format_string_call(
        &mut self,
        name: &str,
        args: &[Arg],
        locals: &Locals,
        label: &str,
    ) {
        let fmt_idx = usize::from(name == "fprintf");
        let Some(fmt_expr) = args.get(fmt_idx) else {
            return;
        };
        let Some(fmt) = literal_string_value(&fmt_expr.value) else {
            return;
        };
        if matches!(name, "vsprintf" | "vprintf") {
            let Some(values_arg) = args.get(fmt_idx + 1) else {
                return;
            };
            let values_ty = self.infer_expr(&values_arg.value, locals);
            let Type::ArrayShape(fields) = &values_ty else {
                return;
            };
            if !fields
                .iter()
                .enumerate()
                .all(|(i, (k, _, _))| k == &i.to_string())
            {
                return;
            }
            let provided: Vec<(Type, Span)> = fields
                .iter()
                .map(|(_, ty, _)| (ty.clone(), values_arg.value.span))
                .collect();
            self.check_format_placeholders(name, &fmt, fmt_expr.value.span, &provided, label);
            return;
        }
        let provided: Vec<(Type, Span)> = args[fmt_idx + 1..]
            .iter()
            .map(|a| (self.infer_expr(&a.value, locals), a.value.span))
            .collect();
        self.check_format_placeholders(name, &fmt, fmt_expr.value.span, &provided, label);
    }

    /// The shared placeholder-vs-argument-type engine both call shapes
    /// `check_format_string_call` recognizes feed into - `provided` is
    /// already-resolved (type, span-to-report-at) pairs, so this itself
    /// doesn't care whether they came from real positional call
    /// arguments or a `vsprintf`/`vprintf` array literal's own fields.
    /// Tries a purely-sequential parse first (`parse_format_specifiers`,
    /// unchanged from Phase 12), then a purely-positional one
    /// (`parse_positional_format_specifiers`, Phase 69) - a format
    /// string mixing the two styles satisfies neither and is silently
    /// skipped, the same "don't guess" leniency each parser already has
    /// on its own.
    fn check_format_placeholders(
        &mut self,
        name: &str,
        fmt: &str,
        fmt_span: Span,
        provided: &[(Type, Span)],
        label: &str,
    ) {
        if let Some(specs) = parse_format_specifiers(fmt) {
            if specs.len() > provided.len() {
                self.report(
                    label.to_string(),
                    format!(
                        "{name}() format string expects {} argument(s), {} given",
                        specs.len(),
                        provided.len()
                    ),
                    fmt_span,
                    0,
                    "arguments.count",
                );
                return;
            }
            for (spec, (arg_ty, span)) in specs.iter().zip(provided) {
                if !format_specifier_accepts(*spec, arg_ty) {
                    self.report(
                        label.to_string(),
                        format!("{name}() format specifier %{spec} does not accept {arg_ty}"),
                        *span,
                        5,
                        "argument.type",
                    );
                }
            }
            return;
        }
        let Some(by_position) = parse_positional_format_specifiers(fmt) else {
            return;
        };
        if by_position.len() > provided.len() {
            self.report(
                label.to_string(),
                format!(
                    "{name}() format string expects {} argument(s), {} given",
                    by_position.len(),
                    provided.len()
                ),
                fmt_span,
                0,
                "arguments.count",
            );
            return;
        }
        for (specs, (arg_ty, span)) in by_position.iter().zip(provided) {
            for spec in specs {
                if !format_specifier_accepts(*spec, arg_ty) {
                    self.report(
                        label.to_string(),
                        format!("{name}() format specifier %{spec} does not accept {arg_ty}"),
                        *span,
                        5,
                        "argument.type",
                    );
                }
            }
        }
    }
}

/// PHP superglobals are implicitly defined in every scope without any
/// assignment ever appearing in source - undefined-variable detection must
/// never flag these.
/// Real PHP language constructs that are syntactically indistinguishable
/// from an ordinary function call in this parser (none of `isset`,
/// `empty`, `unset`, `eval`, `exit`, `die` are lexed as keywords - see
/// `token.rs` - so `isset($x)` parses as `ExprKind::Call(Name("isset"), ...)`
/// exactly like a real call). None of them are declared in any stub
/// source (they're compiler intrinsics, not functions - a real PHP
/// function can be called through a variable, `$f = 'isset'; $f();` is a
/// fatal error), so without this list every real codebase's constant use
/// of `isset`/`empty`/`unset` would be flagged as calling an undefined
/// function. Matched case-insensitively since PHP itself treats function
/// (and construct) names case-insensitively.
pub(crate) fn is_language_construct(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "isset"
            | "empty"
            | "unset"
            | "eval"
            | "exit"
            | "die"
            // The parser deliberately models these four as a call to a
            // synthetic name (see parser.rs's own comment on that
            // decision) rather than a dedicated AST node - real language
            // constructs, not functions, so they belong on this list for
            // exactly the same reason as isset/empty/unset above.
            | "include"
            | "include_once"
            | "require"
            | "require_once"
            // Same synthetic-call modeling, for the same reason.
            | "print"
    )
}

/// Parses a `sprintf`/`printf`-style format string into its ordered list
/// of conversion specifiers (`%d`, `%s`, ...), skipping `%%` (a literal
/// percent, not a placeholder). Returns `None` - "don't know, don't
/// guess" - for anything this doesn't confidently understand: a
/// positional specifier (`%1$s`, real and common but out of scope for
/// this pass, since a lone specifier count no longer maps 1:1 to argument
/// count once positions can repeat or skip), an unknown specifier
/// character, or a malformed/truncated `%` sequence. `%b`/`%c`/`%d`/`%e`/
/// `%E`/`%f`/`%F`/`%g`/`%G`/`%o`/`%s`/`%u`/`%x`/`%X` are real PHP
/// specifier characters (confirmed against the language's own
/// documented format).
pub(crate) fn parse_format_specifiers(fmt: &str) -> Option<Vec<char>> {
    let bytes = fmt.as_bytes();
    let mut specs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        let &next = bytes.get(i)?;
        if next == b'%' {
            i += 1;
            continue;
        }
        // A positional prefix (`%1$s`) is digits immediately followed by
        // `$` - checked before flags/width, since a bare width (`%5d`)
        // is also just digits and must NOT be mistaken for one.
        let digits_start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'$') {
            return None;
        }
        i = digits_start;
        while bytes
            .get(i)
            .is_some_and(|b| matches!(b, b'-' | b'+' | b' ' | b'0'))
        {
            i += 1;
        }
        if bytes.get(i) == Some(&b'\'') {
            i += 2; // custom padding char: the quote plus the pad character itself
        }
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'.') {
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        let &spec = bytes.get(i)?;
        if !b"bcdeEfFgGosuxX".contains(&spec) {
            return None;
        }
        specs.push(spec as char);
        i += 1;
    }
    Some(specs)
}

/// Phase 69: same job as `parse_format_specifiers` just above, but for a
/// format string using only *positional* specifiers (`%1$s`, `%2$d`,
/// ...) - real, common PHP for reordering arguments in localized message
/// strings. Returns `None` on anything not confidently understood, the
/// same "don't guess" standard `parse_format_specifiers` already holds
/// itself to - crucially including a format string that mixes a
/// positional specifier with an ordinary, unnumbered one. Real PHP's own
/// interaction between the two is genuinely quirky (confirmed directly:
/// `sprintf('%1$s-%s-%2$s', 'a', 'b', 'c')` produces `"a-a-b"`, not
/// `"a-b-c"` - the unnumbered specifier does *not* pick up wherever the
/// positional ones left off), quirky enough that guessing at it risks a
/// wrong answer worse than staying silent.
///
/// Returns one entry per 0-based argument *position* referenced by
/// *some* specifier, up to the highest position seen - a "gap" position
/// referenced only implicitly (`%3$s` alone still needs positions 0 and
/// 1 to exist as real arguments) gets an empty `Vec`: nothing to check
/// its own argument's type against, but it still counts toward the
/// minimum argument count. A position referenced by more than one
/// specifier (`%1$d %1$s`, a real if unusual idiom) collects every
/// specifier that references it - the argument there must satisfy all
/// of them.
pub(crate) fn parse_positional_format_specifiers(fmt: &str) -> Option<Vec<Vec<char>>> {
    let bytes = fmt.as_bytes();
    let mut by_position: Vec<Vec<char>> = Vec::new();
    let mut i = 0;
    let mut saw_any = false;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        let &next = bytes.get(i)?;
        if next == b'%' {
            i += 1;
            continue;
        }
        let digits_start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == digits_start || bytes.get(i) != Some(&b'$') {
            // No digits, or digits not immediately followed by `$` - an
            // ordinary, unnumbered specifier. This format string isn't
            // uniformly positional, so don't guess at how the two
            // styles interact.
            return None;
        }
        let position: usize = fmt[digits_start..i].parse().ok()?;
        i += 1; // skip '$'
        while bytes
            .get(i)
            .is_some_and(|b| matches!(b, b'-' | b'+' | b' ' | b'0'))
        {
            i += 1;
        }
        if bytes.get(i) == Some(&b'\'') {
            i += 2; // custom padding char: the quote plus the pad character itself
        }
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'.') {
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        let &spec = bytes.get(i)?;
        if !b"bcdeEfFgGosuxX".contains(&spec) {
            return None;
        }
        i += 1;
        saw_any = true;
        // Real PHP positions are 1-based; `%0$s` isn't valid PHP at all
        // (a `ValueError` at runtime) - don't guess at what it means.
        if position == 0 {
            return None;
        }
        let idx = position - 1;
        if by_position.len() <= idx {
            by_position.resize(idx + 1, Vec::new());
        }
        by_position[idx].push(spec as char);
    }
    if !saw_any {
        return None;
    }
    Some(by_position)
}

/// Whether a real PHP conversion specifier could plausibly accept
/// `arg_ty` - deliberately lenient (PHP casts numeric strings and floats
/// for `%d`, and almost anything with a `__toString` for `%s`), so this
/// only flags the unambiguous cases: an array (or a known array shape)
/// for any specifier, or an object for a numeric one - both produce
/// either a fatal error or the literal, useless string `"Array"` in real
/// PHP, never a legitimate value.
pub(crate) fn format_specifier_accepts(spec: char, arg_ty: &Type) -> bool {
    // Phase 14: `Checker::is_array_like` also recognizes a `list<T>`/
    // `non-empty-array<T>`-typed argument (a real `Intersection` now) -
    // without it, this silently stopped catching the exact real bug it
    // exists to catch for any refined-array argument.
    match spec {
        'b' | 'c' | 'd' | 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | 'o' | 'u' | 'x' | 'X' => {
            !Checker::is_array_like(arg_ty) && !matches!(arg_ty, Type::Object(_, _))
        }
        's' => !Checker::is_array_like(arg_ty),
        _ => true,
    }
}

/// Item 1: resolves a call's own argument list against the declared
/// parameter list it's being checked against, producing one slot per
/// declared parameter - `Some(&Expr)` when some argument (positional or
/// named) actually fills that parameter, `None` when it doesn't (an
/// omitted optional parameter, or a parameter a spread/unpack argument
/// might fill, whose real contents can't be resolved statically at
/// all). Every consumer that used to assume `args[i]` lines up with
/// `params[i]` - true only for the overwhelmingly common all-positional
/// case - now goes through this first, so a named or reordered-named
/// call resolves against the *real* parameter, not whichever one happens
/// to sit at the same index in the written argument list.
///
/// The returned `Vec` can be *longer* than `params.len()` when the last
/// parameter is variadic: real PHP has no way to fill a variadic
/// parameter by name (there's no single name that means "the rest of the
/// arguments"), so once a variadic parameter is in play, any further
/// positional argument is necessarily extra, appended past the initial
/// `params.len()`-sized block - the same shape `check_args`'s own
/// existing variadic fallback (`sig.params.last()` when `i` runs past
/// `params.len()`) already expects.
///
/// A positional argument fills the next open slot by declaration order,
/// tracked via a running count of positional arguments seen so far (not
/// "the next unfilled slot overall" - the two only diverge for a
/// positional argument written *after* a named one, which real PHP
/// itself forbids as a parse-time error; this project's own parser
/// doesn't enforce that ordering, so such input could reach here anyway,
/// but only needs to avoid panicking on it, not produce a meaningful
/// result for syntactically-invalid PHP).
///
/// Item 2: a spread/unpack argument's own real contents (how many
/// elements, which keys) aren't known statically, so no further
/// *positional* argument can be resolved to a slot once one is seen -
/// but, unlike this function's previous behavior (breaking out of the
/// whole loop the moment any spread was seen, silently dropping every
/// argument written after it, named or not), a *named* argument
/// following a spread (real, legal PHP: `f(...$a, name: $b)`) still
/// resolves exactly as it would anywhere else.
pub(crate) fn resolve_call_args<'a>(args: &'a [Arg], params: &[ParamSig]) -> Vec<Option<&'a Expr>> {
    let mut resolved: Vec<Option<&Expr>> = vec![None; params.len()];
    let mut positional_index = 0usize;
    let mut spread_seen = false;
    for arg in args {
        if arg.spread {
            spread_seen = true;
            continue;
        }
        if let Some(name) = &arg.name {
            if let Some(pos) = params.iter().position(|p| &p.name == name) {
                resolved[pos] = Some(&arg.value);
            }
            continue;
        }
        if spread_seen {
            // A plain positional argument written after a spread is
            // invalid PHP in the first place (`argument.
            // nonUnpackAfterUnpacked`, flagged separately by
            // `check_argument_legality`) - nothing meaningful to resolve
            // it to.
            continue;
        }
        if positional_index < resolved.len() {
            resolved[positional_index] = Some(&arg.value);
        } else if params.last().is_some_and(|p| p.variadic) {
            resolved.push(Some(&arg.value));
        }
        positional_index += 1;
    }
    resolved
}

/// The positional index `resolve_call_args` would have reached at the
/// moment it hits this call's first spread/unpack argument - `None` when
/// there isn't one. Every declared parameter at or past that index might
/// still be filled by the spread's own runtime contents, which can't be
/// resolved statically, so `check_args`'s "too few"/"too many" argument-
/// count checks must stop trusting a `None` slot (or a raw `args.len()`)
/// at or past that point to mean what it normally would.
fn first_spread_positional_index(args: &[Arg]) -> Option<usize> {
    let mut positional_index = 0usize;
    for arg in args {
        if arg.spread {
            return Some(positional_index);
        }
        if arg.name.is_none() {
            positional_index += 1;
        }
    }
    None
}

/// Whether `expr`'s own access chain passes through a nullsafe operator
/// anywhere along it (`$x?->y->z`, `$x->y?->z`, `$x?->y['k']`, ...) - the
/// generic "does this chain contain a nullsafe operator" walk the
/// roadmap's own audit flagged as missing from this project (`Property
/// Access`/`MethodCall`'s own `nullsafe: bool` field was previously only
/// ever read locally, per-node - see `checker/expr.rs`'s own several
/// direct reads of it - never walked as a whole chain). Mirrors real
/// PHPStan's own `NullsafeCheck::containsNullSafe`. A nullsafe access
/// short-circuits its *entire* enclosing expression to `null` the moment
/// its own base is `null`, so nothing built on top of one, however deep,
/// can ever be relied on to name a genuine, always-evaluated storage
/// location, which is exactly what passing an argument by reference
/// needs.
pub(crate) fn contains_nullsafe_access(expr: &Expr) -> bool {
    match &unwrap_grouping(expr).kind {
        ExprKind::PropertyAccess(base, _, nullsafe) => *nullsafe || contains_nullsafe_access(base),
        ExprKind::MethodCall(base, _, _, nullsafe) => *nullsafe || contains_nullsafe_access(base),
        ExprKind::DynamicPropertyAccess(base, _) => contains_nullsafe_access(base),
        ExprKind::DynamicMethodCall(base, _, _) => contains_nullsafe_access(base),
        ExprKind::Index(base, _) => contains_nullsafe_access(base),
        _ => false,
    }
}

/// Whether `expr` is one of the narrow expression shapes real PHP itself
/// allows as the target of a by-reference parameter - a plain variable,
/// or an array/property access rooted in one - independent of the
/// nullsafe-chain legality `contains_nullsafe_access` checks separately
/// (a nullsafe chain can be built from otherwise-legal shapes below and
/// still needs its own, more specific rejection reason). Mirrors the
/// shapes real PHPStan's own by-ref argument check treats as legal
/// (`Variable`, `ArrayDimFetch`, `PropertyFetch`, `StaticPropertyFetch`)
/// before falling back to `callReturnsByReference` for anything else -
/// not modeled here, since this project has no notion of a user-defined
/// function/method that itself returns by reference, so every remaining
/// shape (a call result, a literal, an operator expression, ...) is
/// conservatively treated as illegal.
pub(crate) fn is_valid_byref_argument_expr(expr: &Expr) -> bool {
    matches!(
        unwrap_grouping(expr).kind,
        ExprKind::Variable(_)
            | ExprKind::DynamicVariable(_)
            | ExprKind::Index(_, _)
            | ExprKind::PropertyAccess(_, _, _)
            | ExprKind::DynamicPropertyAccess(_, _)
            | ExprKind::StaticAccess(_, _)
    )
}
