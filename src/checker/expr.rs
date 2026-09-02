use crate::ast::*;
use crate::checker::{
    CheckArgsCall, Checker, Locals, TemplateBindings, body_contains_yield, is_language_construct,
    is_real_variable_key, is_superglobal, literal_string_key, literal_string_value, merge_scopes,
    narrow_type_excluding, narrowing_key, purge_stale_narrowing_for, resolve_call_args,
};
use crate::pest_config;
use crate::regex_groups;
use crate::registry::FunctionSig;
use crate::types::{self, Type};

impl<'a> Checker<'a> {
    /// Checks an `isset()`/`??`-style "quiet" left-hand-side chain: walks
    /// down through `Index`/`PropertyAccess`/`DynamicPropertyAccess`,
    /// checking each level's own sub-expressions (an index's key, a
    /// dynamic property's name) normally, but suppressing the root
    /// variable's own "undefined variable" check and `PropertyAccess`'s
    /// own "access to undefined property" check at every level along the
    /// way - exactly what `isset()`/`??` themselves suppress in real PHP,
    /// and nothing more. `StaticAccess` is never part of this walk (an
    /// undeclared *static* property is a real, non-suppressible PHP
    /// fatal error), and anything past a function/method call is checked
    /// normally, since `isset()`/`??` don't evaluate quietly past one.
    pub(crate) fn check_expr_quiet_root(
        &mut self,
        e: &Expr,
        locals: &mut Locals,
        label: &str,
    ) -> Type {
        // Every arm here is consumed only for its side effects - see this
        // function's own doc comment and Phase 3's plan note: neither `??`
        // nor `??=` (its only two call sites) depend on the quiet-checked
        // left side's own type, so the exact value returned doesn't matter
        // beyond satisfying the type system post-unification.
        match &e.kind {
            ExprKind::Variable(_) => {
                // Deliberately not `check_expr` - a bare variable here is
                // exactly the "might not be defined" case `??` exists to
                // handle safely.
                Type::Mixed
            }
            ExprKind::Index(base, idx) => {
                self.check_expr_quiet_root(base, locals, label);
                if let Some(i) = idx {
                    self.check_expr(i, locals, label);
                }
                Type::Mixed
            }
            ExprKind::PropertyAccess(base, _, _) => {
                self.check_expr_quiet_root(base, locals, label);
                Type::Mixed
            }
            ExprKind::DynamicPropertyAccess(base, name_expr) => {
                self.check_expr_quiet_root(base, locals, label);
                self.check_expr(name_expr, locals, label);
                Type::Mixed
            }
            _ => self.check_expr(e, locals, label),
        }
    }

    #[hotpath::measure]
    pub(crate) fn check_expr(&mut self, e: &Expr, locals: &mut Locals, label: &str) -> Type {
        match &e.kind {
            ExprKind::Call(callee, args) => {
                // `unset($x)` genuinely removes `$x` from scope in real
                // PHP - a real bug if it's used afterward. Checked before
                // anything else below: `predefine_byref_or_unknown_args`
                // treats *any* bare-variable argument to an unknown-
                // signature call as a definition site (a mechanism built
                // for a different reason - conservatively assuming an
                // unresolvable call might take the argument by
                // reference - that happens to also make `isset`/`empty`
                // safe to use on a maybe-undefined variable), which is
                // backwards for `unset` specifically: it would make
                // `unset($x)` look like it *defines* `$x` instead of
                // removing it.
                if let ExprKind::Name(name) = &callee.kind
                    && name == "unset"
                {
                    for a in args {
                        if let ExprKind::Variable(var_name) = &a.value.kind {
                            locals.remove(var_name);
                        } else {
                            self.check_expr(&a.value, locals, label);
                        }
                    }
                    return Type::Mixed;
                }
                // `PHPStan\Testing\assertType('expected type string', $expr)`
                // is real PHPStan's own testing-DSL function, used
                // throughout its vendored `tests/PHPStan/Analyser/nsrt/`
                // fixture corpus (see Item 2 of the architecture-audit
                // plan) to pin down an exact expected inferred type at a
                // point of interest. Recognizing it here - comparing this
                // project's own `infer_expr` result (rendered via `Type`'s
                // existing PHPStan-style `Display` impl) against the
                // literal expected string - turns that whole corpus into
                // directly-runnable ground truth against this project's
                // own checker, with no separate test harness needed.
                // Real PHPStan itself treats this the same way (a no-op
                // stub function whose call sites its *own* internal
                // tooling specially recognizes), so this is additive,
                // not a real diagnostic anyone would trip on outside of
                // a fixture file that imports it.
                if let ExprKind::Name(name) = &callee.kind
                    && (name == "assertType" || name.ends_with("\\assertType"))
                    && let [expected_arg, actual_arg] = args.as_slice()
                    && let ExprKind::Literal(Literal::String(expected)) = &expected_arg.value.kind
                {
                    let actual = self.infer_expr(&actual_arg.value, locals).to_string();
                    if &actual != expected {
                        self.report(label.to_string(), format!(
                                "assertType() mismatch: expected type \"{expected}\", actual type \"{actual}\""
                            ), actual_arg.value.span, 0);
                    }
                    self.check_expr(&actual_arg.value, locals, label);
                    return Type::Mixed;
                }
                if let ExprKind::Name(name) = &callee.kind
                    && (name == "extract" || name == "eval")
                {
                    self.saw_dynamic_var_construct = true;
                }
                // Resolved once up front (namespace/`use`-import rules
                // applied, with the same global-namespace fallback real
                // PHP gives an unqualified function call) so both the
                // existing arg-checking path and the undefined-call
                // diagnostic below see the same name(s) a real call would
                // actually resolve to - previously `lookup_function` used
                // the raw, unnormalized name, so e.g. `\class_exists(...)`
                // silently skipped arg-checking even when the plain name
                // was a known signature. `is_language_construct` is
                // checked against the *original* name, before any
                // namespace-qualification - `isset`/`empty`/etc. are
                // never real namespaced symbols, unlike an actual function
                // call, even though the parser models both the same way.
                let resolved_name = match &callee.kind {
                    ExprKind::Name(name) if is_language_construct(name) => None,
                    ExprKind::Name(name) => {
                        let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
                        if self.registry.lookup_function(&primary).is_some() {
                            Some(primary)
                        } else if let Some(global) = &fallback
                            && self.registry.lookup_function(global).is_some()
                        {
                            Some(global.clone())
                        } else {
                            Some(primary)
                        }
                    }
                    _ => None,
                };
                let sig = resolved_name
                    .as_deref()
                    .and_then(|name| self.registry.lookup_function(name));
                // Item 1: resolves the call's own arguments against the
                // real signature (named/reordered-named arguments no
                // longer get checked as if they were positional in
                // written order) before any consumer below sees them.
                let resolved = sig
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.predefine_byref_or_unknown_args(sig, args, &resolved, locals);
                // Finding 1: `it`/`test`/`describe`/`beforeEach`/
                // `afterEach`/`beforeAll`/`afterAll` rebind `$this` inside
                // their closure/arrow-fn argument at runtime (Pest's own
                // `Closure::bindTo()`) - reuses the same
                // check_closure_literal_with_this helper the
                // Closure::bind/bindTo special case below already uses
                // for the identical "externally-bound $this" shape. The
                // base scanner never spells out Pest's own function names
                // - it only asks pest_config::is_this_binding_call. Only
                // a closure/arrow-fn *literal* argument is recognized,
                // same restriction the existing bind/bindTo case already
                // has - `it('...', $variableHoldingAClosure)` falls
                // through to plain check_expr, unrebound.
                let is_this_binding_call = matches!(&callee.kind, ExprKind::Name(name) if pest_config::is_this_binding_call(name));
                for a in args {
                    if is_this_binding_call
                        && matches!(
                            unwrap_grouping(&a.value).kind,
                            ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                        )
                    {
                        let this_ty = self.pest_this_type.clone();
                        self.check_closure_literal_with_this(
                            unwrap_grouping(&a.value),
                            locals,
                            label,
                            this_ty,
                        );
                    } else {
                        self.check_expr(&a.value, locals, label);
                    }
                }
                if let Some(name) = &resolved_name {
                    if let Some(sig) = sig {
                        // Finding 29: a spread argument (`f(...$args)`)
                        // means the real argument count/types aren't
                        // known statically at all - checking count/type
                        // here would treat the spread array itself as if
                        // it were meant to fill the very next positional
                        // slot, producing a real, severe false positive
                        // for the standard "forward a dynamically-sized
                        // argument list" idiom.
                        let has_spread = args.iter().any(|a| a.spread);
                        if !has_spread {
                            self.check_args(CheckArgsCall {
                                callee_label: name,
                                sig,
                                args,
                                resolved: &resolved,
                                locals,
                                label,
                                defining_class: None,
                                span: e.span,
                            });
                            if matches!(name.as_str(), "sprintf" | "printf") {
                                self.check_format_string_call(name, args, locals, label);
                            }
                        }
                    } else {
                        // Since resolve_function_candidates always
                        // produces a definite name (there's no "can't
                        // resolve" case left for functions the way
                        // `ExprKind::New`'s class resolution still has for a
                        // multi-segment name with no matching import),
                        // every miss here is a real undefined-function
                        // finding.
                        self.report(
                            label.to_string(),
                            format!("call to undefined function {name}()"),
                            e.span,
                            0,
                        );
                    }
                }
                // Phase 12 Findings A/B/C: `preg_match`'s by-ref
                // `$matches` shape, including `PREG_OFFSET_CAPTURE`/
                // `PREG_UNMATCHED_AS_NULL` recognition - `parse_preg_flags`
                // bails (`None`) on any other flag, leaving this whole
                // check inert for it. Overrides the generic widening
                // `predefine_byref_or_unknown_args` already did above,
                // rather than replacing it - anything outside this scope
                // keeps that call's own (Mixed-or-widened) fallback
                // unchanged. Deliberately placed *after* `check_args`
                // above, not right after `predefine_byref_or_unknown_args`
                // like an earlier draft had it - a real bug found by
                // testing, not just reasoned about: `check_args` checks
                // whether the *argument* `$matches` is compatible with
                // the declared `?array<string> $matches` parameter (the
                // real phpstorm-stubs stub is more specific than the
                // bare native `?array` hint), and that check needs to see
                // `$matches`'s own *pre-call* type, not this override's
                // freshly-computed *post-call* shape - placing this
                // before `check_args` made every `$matches` shape whose
                // leaf type isn't plain `string` (Finding B's `[string,
                // int]` tuple, Finding C's `string|null`) spuriously fail
                // that check, since Finding A's own all-`string`-leaves
                // shape had accidentally masked the bug by coincidentally
                // matching the declared type exactly.
                if resolved_name.as_deref() == Some("preg_match")
                    && resolved.get(1).copied().flatten().is_some()
                    && let Some(pattern_arg) = resolved.first().copied().flatten()
                    && let Some(matches_arg) = resolved.get(2).copied().flatten()
                    && let ExprKind::Variable(matches_name) = &matches_arg.kind
                    && let Some(pattern_str) = literal_string_value(pattern_arg)
                    && let Some(groups) = regex_groups::parse_capture_groups(pattern_str)
                    && let Some(flags) = resolved
                        .get(3)
                        .copied()
                        .flatten()
                        .map_or(Some(PregFlags::default()), |e| parse_preg_flags(e, false))
                {
                    let leaf = preg_leaf_type(flags.offset_capture);
                    let matched = preg_match_matches_shape(&groups, &leaf, flags.unmatched_as_null);
                    // Phase 12 Finding E, Part 3: unioned with an empty
                    // shape for the real `0`-return/no-match case (real
                    // PHP genuinely sets `$matches = []` then) - sound on
                    // its own only because `check_shape_key_access`/
                    // `infer_expr`'s own `Index` arm are now `Union`-aware
                    // (Part 1), and precision for the extremely common
                    // `if (preg_match(...)) { use $matches }` idiom is
                    // recovered by `apply_narrowing`'s own new
                    // `preg_match_condition` recognizer (Part 2), not by
                    // this unconditional type itself.
                    let shape = union_types(matched, Type::ArrayShape(vec![]));
                    purge_stale_narrowing_for(locals, matches_name);
                    locals.insert(matches_name.clone(), shape);
                }
                // Phase 12 Finding D: `preg_match_all`'s by-ref
                // `$matches` shape - a sibling special case to
                // `preg_match`'s own above, sharing the same pattern-
                // parsing/flag-parsing/leaf-type machinery but building a
                // structurally different shape (`PREG_PATTERN_ORDER`, the
                // real default, groups results by capture group across
                // every match; `PREG_SET_ORDER` groups them by match,
                // reusing `preg_match_matches_shape` unchanged for each
                // entry). Placed after `check_args`, same reasoning as
                // Finding A/B/C just above.
                if resolved_name.as_deref() == Some("preg_match_all")
                    && resolved.get(1).copied().flatten().is_some()
                    && let Some(pattern_arg) = resolved.first().copied().flatten()
                    && let Some(matches_arg) = resolved.get(2).copied().flatten()
                    && let ExprKind::Variable(matches_name) = &matches_arg.kind
                    && let Some(pattern_str) = literal_string_value(pattern_arg)
                    && let Some(groups) = regex_groups::parse_capture_groups(pattern_str)
                    && let Some(flags) = resolved
                        .get(3)
                        .copied()
                        .flatten()
                        .map_or(Some(PregFlags::default()), |e| parse_preg_flags(e, true))
                {
                    let leaf = preg_leaf_type(flags.offset_capture);
                    let shape = if flags.set_order {
                        Type::Array(Box::new(preg_match_matches_shape(
                            &groups,
                            &leaf,
                            flags.unmatched_as_null,
                        )))
                    } else {
                        preg_match_all_pattern_order_shape(&groups, &leaf, flags.unmatched_as_null)
                    };
                    purge_stale_narrowing_for(locals, matches_name);
                    locals.insert(matches_name.clone(), shape);
                }
                // Reuses the `resolved_name`/`sig` already resolved above
                // - the same lookup `infer_expr`'s own `Call` arm would
                // otherwise redundantly redo from scratch.
                sig.map(|s| {
                    self.infer_builtin_call_return_type(
                        resolved_name.as_deref().unwrap_or_default(),
                        s,
                        args,
                        locals,
                    )
                })
                .unwrap_or_else(|| call_fallback_return_type(callee))
            }
            ExprKind::MethodCall(recv, method, args, nullsafe) => {
                // Finding 49's other real shape: `(function () {...})
                // ->bindTo($newThis, $scope)` - same idea as
                // `Closure::bind`, just on the receiver instead of the
                // first argument.
                //
                // The `bindTo` branch can't reuse `check_expr`'s own
                // return value the way the ordinary branch below does -
                // `check_closure_literal_with_this` checks the closure's
                // *body* under a rebound `$this`, it doesn't compute
                // `recv`'s own type - so `recv` still needs its own
                // `infer_expr` call there specifically.
                let recv_ty = if method == "bindTo"
                    && matches!(
                        unwrap_grouping(recv).kind,
                        ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                    ) {
                    let this_ty = args
                        .first()
                        .map(|a| self.infer_expr(&a.value, locals))
                        .unwrap_or(Type::Mixed);
                    self.check_closure_literal_with_this(
                        unwrap_grouping(recv),
                        locals,
                        label,
                        this_ty,
                    );
                    self.infer_expr(recv, locals)
                } else {
                    self.check_expr(recv, locals, label)
                };
                // Re-enabled (Phase 9.5): the registry is now
                // namespace-aware, so a method lookup trusts the specific
                // `ClassInfo` stored under a *fully-qualified* name, not a
                // bare one two unrelated packages could collide on - see
                // the module-level doc comment's "Known limitation" note,
                // now resolved, for the full history.
                //
                // A `Union`/`Intersection` receiver (e.g. a multi-type
                // `catch` variable, or an ordinarily union-typed
                // parameter) used to fall to `_ => None` here entirely,
                // silently skipping this whole check - `member_missing`
                // below picks up the "should this be flagged" question
                // for those cases instead, using a representative member
                // (the first `Object` found) for argument-checking.
                let recv_class = match &recv_ty {
                    Type::Object(class, type_args) => Some((class.clone(), type_args.clone())),
                    Type::Union(members) | Type::Intersection(members) => {
                        members.iter().find_map(|m| match m {
                            Type::Object(class, type_args) => {
                                Some((class.clone(), type_args.clone()))
                            }
                            _ => None,
                        })
                    }
                    _ => None,
                };
                // Substituted through the receiver's own carried type
                // arguments (Phase 10), walking up to whichever ancestor
                // actually declares the method if it's inherited (Phase
                // 10.5) - a method declared `add(T $item)` on
                // `Collection<User>`, or inherited from `Collection` by a
                // concrete `IntCollection extends Collection<int>`, gets
                // checked against `User`/`int`, not an unbound `T`/`Mixed`.
                let sig = recv_class.as_ref().and_then(|(class, type_args)| {
                    self.registry
                        .lookup_method_with_owner(class, method)
                        .map(|(owner, sig)| {
                            self.substitute_class_templates(sig, class, type_args, &owner)
                        })
                });
                let resolved = sig
                    .as_ref()
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.predefine_byref_or_unknown_args(sig.as_ref(), args, &resolved, locals);
                for a in args {
                    self.check_expr(&a.value, locals, label);
                }
                if let Some((class, _)) = &recv_class {
                    // Checked *before* falling back to whether the
                    // representative member's own `sig` was found: for a
                    // `Union`, the representative is just the first
                    // `Object` member encountered, which could easily be
                    // one that *does* have the method even though another
                    // member doesn't - the real question is never "does
                    // the first member have it," it's "is it missing from
                    // any member (Union) / every member (Intersection)."
                    // A class not known to the registry at all (an
                    // unresolvable/uninstalled dependency, the same gap
                    // `ExprKind::New`'s check documents) is left unchecked
                    // rather than risking a false positive - only fires
                    // once the class itself is confirmed known. `__call`
                    // anywhere in the chain means any method name is
                    // legitimately callable at runtime, so its presence
                    // suppresses this entirely, the same way real
                    // PHPStan treats it.
                    if self.registry.classes.contains_key(class)
                        && self.member_confirmed_missing(&recv_ty, |c| {
                            self.registry.lookup_method(c, method).is_some()
                                || self.registry.lookup_method(c, "__call").is_some()
                        })
                    {
                        self.report(
                            label.to_string(),
                            format!("call to undefined method {recv_ty}::{method}()"),
                            e.span,
                            0,
                        );
                    } else if let Some(sig) = &sig
                        && !args.iter().any(|a| a.spread)
                    {
                        // Finding 29: see the identical check on the
                        // plain-function Call arm above.
                        let qualified = format!("{class}::{method}");
                        self.check_args(CheckArgsCall {
                            callee_label: &qualified,
                            sig,
                            args,
                            resolved: &resolved,
                            locals,
                            label,
                            defining_class: Some(class),
                            span: e.span,
                        });
                    }
                }
                let result = match &recv_class {
                    Some((class, _)) => sig
                        .as_ref()
                        .map(|s| {
                            let resolved_ty = self.infer_call_return_type(s, &resolved, locals);
                            self.resolve_pseudo(&resolved_ty, class)
                        })
                        .unwrap_or(Type::Mixed),
                    None => Type::Mixed,
                };
                // `$x?->method()` short-circuits to `null` when `$x` is
                // null, on top of whatever the method's own return type
                // is - real, implicit nullability a plain `->` doesn't
                // carry.
                if *nullsafe && result != Type::Mixed {
                    union_types(result, Type::Null)
                } else {
                    result
                }
            }
            // `&&`/`||` short-circuit, so the right operand only ever runs
            // once the left one's already settled the outcome - it needs
            // to see whatever that implies (real code leans on this
            // constantly: `$x instanceof Foo && $x->bar()`,
            // `$x !== null && $x->prop`). Without this, checking the
            // right side against the un-narrowed left-hand type produced
            // real false positives once undefined-property detection
            // started exercising exactly this idiom on real code.
            ExprKind::Binary(BinOp::And, l, r) => {
                self.check_expr(l, locals, label);
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, true);
                self.check_expr(r, &mut narrowed, label);
                merge_new_variable_definitions(locals, &narrowed);
                self.infer_binary(&BinOp::And, l, r, locals)
            }
            ExprKind::Binary(BinOp::Or, l, r) => {
                self.check_expr(l, locals, label);
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, false);
                self.check_expr(r, &mut narrowed, label);
                merge_new_variable_definitions(locals, &narrowed);
                self.infer_binary(&BinOp::Or, l, r, locals)
            }
            // `??` is built on isset()-style quiet semantics - its left
            // side is evaluated without ever raising an undefined-
            // variable/undefined-property notice, even when the root
            // variable was never assigned anywhere in scope. Real PHP
            // silences this for the whole dereference chain on the left
            // (`$config['key'] ?? 'default'` is just as warning-free as
            // a bare `$x ?? 'default'`), which is exactly what
            // `check_expr_quiet_root` walks. `??`'s own result type is
            // *usually* just the right side's type - reusing
            // `check_expr(r, ...)`'s return value directly avoids a real,
            // confirmed double-walk of `r` - except when `widen_coalesce_
            // result` (Phase 22 Finding 4) says the left side's own type
            // carries real information the right side's bare fallback
            // would otherwise discard.
            ExprKind::Binary(BinOp::Coalesce, l, r) => {
                self.check_expr_quiet_root(l, locals, label);
                let r_ty = self.check_expr(r, locals, label);
                self.widen_coalesce_result(l, r_ty, locals)
            }
            ExprKind::Binary(op, l, r) => {
                self.check_expr(l, locals, label);
                self.check_expr(r, locals, label);
                self.infer_binary(op, l, r, locals)
            }
            ExprKind::Variable(name) => {
                if !locals.contains_key(name) && !is_superglobal(name) {
                    self.report(
                        label.to_string(),
                        format!("undefined variable ${name}"),
                        e.span,
                        0,
                    );
                }
                locals.get(name).cloned().unwrap_or(Type::Mixed)
            }
            ExprKind::Unary(
                op @ (UnaryOp::PreInc | UnaryOp::PostInc | UnaryOp::PreDec | UnaryOp::PostDec),
                inner,
            ) => {
                self.check_expr(inner, locals, label);
                // Real PHP: incrementing `null` produces `int(1)`;
                // decrementing `null`, and either direction on a `bool`,
                // is a no-op (the value is unchanged) - only the
                // null-to-int case actually changes the tracked type.
                // A string operand's own exact value isn't modeled
                // precisely enough to distinguish numeric from
                // alphanumeric-increment behavior, so it conservatively
                // stays a string either way.
                if let ExprKind::Variable(name) = &inner.kind {
                    let current = locals.get(name).cloned().unwrap_or(Type::Mixed);
                    let is_increment = matches!(op, UnaryOp::PreInc | UnaryOp::PostInc);
                    let updated = match &current {
                        Type::Null if is_increment => Type::Int,
                        other => other.clone(),
                    };
                    purge_stale_narrowing_for(locals, name);
                    locals.insert(name.clone(), updated);
                }
                self.infer_expr(inner, locals)
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                self.check_expr(inner, locals, label);
                Type::Bool
            }
            // Real PHP's unary minus/plus convert a numeric string
            // operand to a number (`-"5"` is `int(-5)`, not `"-5"`) - the
            // same implicit-numeric-coercion rule arithmetic operators
            // apply generally.
            ExprKind::Unary(UnaryOp::Neg | UnaryOp::Plus, inner) => {
                match self.check_expr(inner, locals, label) {
                    Type::String | Type::StringLiteral(_) => Type::Int,
                    other => other,
                }
            }
            ExprKind::Unary(_, inner) | ExprKind::Grouping(inner) => {
                self.check_expr(inner, locals, label)
            }
            ExprKind::Cast(ty, inner) => {
                self.check_expr(inner, locals, label);
                match ty.as_str() {
                    "int" => Type::Int,
                    "float" => Type::Float,
                    "string" => Type::String,
                    "bool" => Type::Bool,
                    "array" => Type::Array(Box::new(Type::Mixed)),
                    "object" => Type::Object("object".to_string(), Vec::new()),
                    "unset" => Type::Null,
                    _ => Type::Mixed,
                }
            }
            // A throw expression never actually produces a value - Never
            // is exactly the right type for it, the same way it's used
            // for a function that always throws or exits.
            ExprKind::Throw(inner) => {
                self.check_expr(inner, locals, label);
                Type::Never
            }
            ExprKind::YieldFrom(inner) => {
                self.check_expr(inner, locals, label);
                Type::Mixed
            }
            // Never given a stable nominal type by infer_expr either,
            // since multiple clone-with-overrides expressions could
            // produce runtime objects that don't share a common shape
            // this checker tracks - the clone target's own type is the
            // honest answer.
            ExprKind::Clone(inner, overrides) => {
                let inner_ty = self.check_expr(inner, locals, label);
                if let Some(o) = overrides {
                    self.check_expr(o, locals, label);
                }
                inner_ty
            }
            ExprKind::Yield(key, value) => {
                if let Some(k) = key {
                    self.check_expr(k, locals, label);
                }
                if let Some(v) = value {
                    self.check_expr(v, locals, label);
                }
                Type::Mixed
            }
            ExprKind::Instanceof(inner, target) => {
                self.check_expr(inner, locals, label);
                if let InstanceofTarget::Expr(e) = target {
                    self.check_expr(e, locals, label);
                }
                Type::Mixed
            }
            ExprKind::Assign(lhs, rhs) => {
                let rhs_ty = self.check_expr(rhs, locals, label);
                // A bare variable (or a destructuring target inside one)
                // on the left is a definition site, not a read - checking
                // it here, before it's bound below, would make undefined-
                // variable detection flag every variable on the very
                // statement that defines it. PropertyAccess/Index targets
                // still need their *base* checked (`$obj->prop = ...`
                // reads $obj).
                self.check_assignment_target(lhs, locals, label);
                self.check_property_assignment(lhs, rhs, locals, label);
                self.bind_assignment_target(lhs, rhs_ty.clone(), locals);
                rhs_ty
            }
            ExprKind::CompoundAssign(op, lhs, rhs) => {
                // `$x ??= $default;` is defined as exactly
                // `$x = $x ?? $default;` - the same quiet-left-side
                // exemption as plain `??` applies to its own left side.
                if *op == BinOp::Coalesce {
                    self.check_expr_quiet_root(lhs, locals, label);
                } else {
                    self.check_expr(lhs, locals, label);
                }
                self.check_expr(rhs, locals, label);
                if let ExprKind::Variable(name) = &lhs.kind {
                    let ty = self.infer_binary(op, lhs, rhs, locals);
                    purge_stale_narrowing_for(locals, name);
                    locals.insert(name.clone(), ty);
                }
                Type::Mixed
            }
            ExprKind::Ternary(cond, then_, else_) => {
                self.check_expr(cond, locals, label);
                // Same clone-and-narrow shape as `StmtKind::If` - a
                // ternary is just as branch-based, and without this,
                // `$x === null ? "default" : $x` never benefits from the
                // narrowing an equivalent `if` already gets.
                let mut then_locals = locals.clone();
                self.apply_narrowing(cond, &mut then_locals, true);
                let then_ty = then_
                    .as_ref()
                    .map(|t| self.check_expr(t, &mut then_locals, label));
                let mut else_locals = locals.clone();
                self.apply_narrowing(cond, &mut else_locals, false);
                let else_ty = self.check_expr(else_, &mut else_locals, label);
                *locals = merge_scopes(&then_locals, &else_locals);
                // Phase 23: `union_types`, not a raw `Type::Union(vec![...])`
                // - either branch's own type is very commonly already a
                // `Union` itself (e.g. a stub-declared `resource|false`),
                // and a bare `vec![then_ty, else_ty]` nests it unflattened
                // instead of merging its members in - a later `!== false`
                // narrowing filters only the *outer* union's members, so
                // it can strip a bare `false` sitting alongside the nested
                // union but can't reach inside that union to strip its own
                // `false` member. `union_types` already independently
                // implements the same "either side Mixed => Mixed" /
                // "equal sides collapse" rules this match used to
                // duplicate by hand, so this also simplifies the two arms
                // into one call.
                match then_ty {
                    Some(then_ty) => union_types(then_ty, else_ty),
                    None => Type::Mixed,
                }
            }
            ExprKind::ArrayLiteral(items) => {
                for item in items {
                    if let Some(k) = &item.key {
                        self.check_expr(k, locals, label);
                    }
                    self.check_expr(&item.value, locals, label);
                }
                self.infer_array_literal_type(items, locals)
            }
            ExprKind::Index(base, idx) => {
                let base_ty = self.check_expr(base, locals, label);
                match idx {
                    Some(i) => {
                        self.check_expr(i, locals, label);
                        self.check_shape_key_access(base, i, locals, label);
                        // Phase 32 Finding F (found while testing it): the
                        // `Index` counterpart of `PropertyAccess`'s own
                        // "check the narrowed key first" handling just
                        // above - `check_expr` (unlike `infer_expr`, which
                        // already does this for `Index`) went straight to
                        // re-deriving the type from the base's own
                        // declared/inferred shape, silently discarding
                        // whatever `apply_narrowing` had written under
                        // this exact `narrowing_key`. Real impact: a bare
                        // `return $x['k'];` right after narrowing
                        // `$x['k']` saw the un-narrowed type, while
                        // passing the same expression as a function
                        // argument (going through `infer_expr` for the
                        // argument-type comparison) already didn't.
                        if let Some(key) = narrowing_key(e)
                            && let Some(narrowed) = locals.get(&key)
                        {
                            return narrowed.clone();
                        }
                        match base_ty {
                            Type::Array(inner) => *inner,
                            Type::ArrayShape(fields) => literal_string_key(i)
                                .and_then(|key| {
                                    fields.into_iter().find(|(name, _, _)| *name == key)
                                })
                                .map(|(_, ty, _)| ty)
                                .unwrap_or(Type::Mixed),
                            _ => Type::Mixed,
                        }
                    }
                    None => Type::Mixed,
                }
            }
            ExprKind::New(_, args) => {
                // `new Foo()` never carries known type arguments itself -
                // real PHP's constructor syntax has no way to write
                // `new Foo<Bar>()` at all, and inferring them from the
                // constructor's own arguments (the way real PHPStan often
                // does) isn't attempted here, so this is always `Object`
                // with an empty arg list.
                let new_ty = self.infer_expr(e, locals);
                let resolved_class = match &new_ty {
                    Type::Object(class, _) => Some(class.clone()),
                    _ => None,
                };
                // A dynamic target (`new $className()`) or an unresolved
                // pseudo-type (`new parent()` with no parent class) has no
                // concrete name to check - only a plain `new Name()` (or
                // `self`/`static` resolving to the current class) reaches
                // here with `Some`.
                let ctor_sig = resolved_class
                    .as_deref()
                    .and_then(|class| self.registry.lookup_method(class, "__construct"));
                let resolved = ctor_sig
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.predefine_byref_or_unknown_args(ctor_sig, args, &resolved, locals);
                for a in args {
                    self.check_expr(&a.value, locals, label);
                }
                if let Some(class) = &resolved_class {
                    // `resolved_class` is already the fully-qualified name
                    // (`infer_expr`'s New arm resolves it via `name_ctx`),
                    // so every miss here is a real undefined-class finding
                    // - no "can't resolve a namespaced name" case left, the
                    // way there used to be before Phase 9.5's namespace
                    // support.
                    if let Some(info) = self.registry.classes.get(class) {
                        if info.kind == ClassKind::Interface {
                            self.report(
                                label.to_string(),
                                format!("cannot instantiate interface {class}"),
                                e.span,
                                0,
                            );
                        } else if info.is_abstract {
                            self.report(
                                label.to_string(),
                                format!("cannot instantiate abstract class {class}"),
                                e.span,
                                0,
                            );
                        } else if let Some(sig) = ctor_sig
                            && !args.iter().any(|a| a.spread)
                        {
                            self.check_args(CheckArgsCall {
                                callee_label: &format!("{class}::__construct"),
                                sig,
                                args,
                                resolved: &resolved,
                                locals,
                                label,
                                defining_class: Some(class),
                                span: e.span,
                            });
                        }
                    } else {
                        self.report(
                            label.to_string(),
                            format!("instantiated class {class} not found"),
                            e.span,
                            0,
                        );
                    }
                }
                new_ty
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                for a in args {
                    self.check_expr(&a.value, locals, label);
                }
                // The anonymous class's own methods are still worth
                // checking, even though the class itself isn't indexed
                // into the registry under any stable name - check_function
                // only needs a FunctionDecl and a label to check against.
                for m in &decl.methods {
                    self.check_function(&m.function, Some(&decl.name), m.is_static);
                }
                Type::Mixed
            }
            ExprKind::StaticCall(class_name, method, args) => {
                // Mirrors ExprKind::MethodCall's own structure exactly -
                // resolve first (self/parent/static-aware), predefine
                // byref/unknown args using the real signature before
                // checking them, then the existence/arg-type check.
                let resolved_class = self.resolve_static_class_name(class_name);
                let sig = resolved_class
                    .as_ref()
                    .and_then(|class| self.registry.lookup_method(class, method));
                let resolved = sig
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.predefine_byref_or_unknown_args(sig, args, &resolved, locals);
                // Finding 49: `Closure::bind($closure, $newThis, $scope)`
                // makes `$this` (and, for private/protected access, the
                // calling scope) available inside a closure that
                // otherwise wouldn't have it - real, idiomatic PHP for
                // reaching into an object's state from outside its class.
                // Only special-cased when the first argument is literally
                // a closure expression right here (not some arbitrary
                // `Type::Object("Closure", _)`-typed value) - the closure
                // body needs to still be reachable as an `Expr` to
                // re-check with the rebound `$this`.
                if resolved_class.as_deref() == Some("Closure")
                    && method == "bind"
                    && !args.is_empty()
                    && matches!(
                        unwrap_grouping(&args[0].value).kind,
                        ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                    )
                {
                    let this_ty = args
                        .get(1)
                        .map(|a| self.infer_expr(&a.value, locals))
                        .unwrap_or(Type::Mixed);
                    self.check_closure_literal_with_this(
                        unwrap_grouping(&args[0].value),
                        locals,
                        label,
                        this_ty,
                    );
                    for a in &args[1..] {
                        self.check_expr(&a.value, locals, label);
                    }
                } else {
                    for a in args {
                        self.check_expr(&a.value, locals, label);
                    }
                }
                if let Some(class) = &resolved_class {
                    if let Some(sig) = sig {
                        // Finding 29: see the identical check on the
                        // plain-function Call arm above.
                        if !args.iter().any(|a| a.spread) {
                            let qualified = format!("{class}::{method}");
                            self.check_args(CheckArgsCall {
                                callee_label: &qualified,
                                sig,
                                args,
                                resolved: &resolved,
                                locals,
                                label,
                                defining_class: Some(class),
                                span: e.span,
                            });
                        }
                    } else if self.registry.classes.contains_key(class)
                        && self.registry.lookup_method(class, "__callStatic").is_none()
                    {
                        // Same leniency as MethodCall's own undefined-
                        // method check: only fires once the class itself
                        // is confirmed known, and __callStatic (the
                        // static equivalent of __call) suppresses it
                        // entirely, matching real PHPStan.
                        self.report(
                            label.to_string(),
                            format!("call to undefined static method {class}::{method}()"),
                            e.span,
                            0,
                        );
                    }
                }
                match (&resolved_class, sig) {
                    (Some(class), Some(s)) => {
                        let resolved_ty = self.infer_call_return_type(s, &resolved, locals);
                        self.resolve_pseudo(&resolved_ty, class)
                    }
                    _ => Type::Mixed,
                }
            }
            ExprKind::StaticAccess(class_name, member) => {
                // `Foo::class` (member == "class") always resolves to a
                // real string constant, not an actual member of the
                // class - and a fully dynamic member name (`Foo::{$e}`/
                // `Foo::$$e`) has no statically known name to check in
                // the first place. Both are the parser's own documented
                // encoding for these two cases, not something to check.
                if member == "class" {
                    return match self.resolve_static_class_name(class_name) {
                        Some(class) => Type::ClassString(Box::new(Type::Object(class, Vec::new()))),
                        // A genuinely dynamic target (`$obj::class`) -
                        // unresolvable, unchanged from before.
                        None => Type::String,
                    };
                }
                if member == "{dynamic}" {
                    return Type::Mixed;
                }
                let Some(class) = self.resolve_static_class_name(class_name) else {
                    return Type::Mixed;
                };
                if !self.registry.classes.contains_key(&class) {
                    return Type::Mixed;
                }
                if let Some(prop_name) = member.strip_prefix('$') {
                    // Unlike an instance property, PHP has no legal way
                    // to dynamically create a static property at all -
                    // no "could be legitimate" case to stay lenient for
                    // here, the way check_property_assignment's write
                    // side has to for instance properties. Finding 9: a
                    // property that exists but was declared as an
                    // ordinary *instance* property is equally invalid
                    // through `::` in real PHP ("Access to undeclared
                    // static property") - from the caller's own
                    // perspective there's no real static property by
                    // this name either way, so both share one message.
                    let prop = self.registry.lookup_property(&class, prop_name);
                    let is_valid_static = prop.is_some_and(|sig| sig.is_static);
                    if !is_valid_static {
                        self.report(
                            label.to_string(),
                            format!("access to undefined static property {class}::${prop_name}"),
                            e.span,
                            0,
                        );
                    }
                    prop.map(|p| self.resolve_pseudo(&p.ty, &class))
                        .unwrap_or(Type::Mixed)
                } else {
                    let const_ty = self.registry.lookup_const(&class, member);
                    if const_ty.is_none() {
                        self.report(
                            label.to_string(),
                            format!("access to undefined constant {class}::{member}"),
                            e.span,
                            0,
                        );
                    }
                    const_ty
                        .map(|ty| self.resolve_pseudo(ty, &class))
                        .unwrap_or(Type::Mixed)
                }
            }
            ExprKind::DynamicVariable(name_expr) => {
                self.check_expr(name_expr, locals, label);
                Type::Mixed
            }
            ExprKind::PropertyAccess(base, prop, nullsafe) => {
                // Unlike a *write* (check_property_assignment, and the
                // Assign/ArrayLiteral-destructuring arms below that
                // deliberately route a PropertyAccess target through only
                // its base, not here) - a genuinely undeclared property
                // creates nothing on read, it just returns null with a
                // warning in real PHP (unless __get exists). So there's
                // no "could be a legitimate dynamic property" case to
                // stay lenient for here, the way there is on write.
                // Same `Union`/`Intersection` handling as `MethodCall`
                // just above, and the identical reasoning for why: a
                // multi-type `catch` variable, or an ordinarily
                // union-typed parameter, used to skip this check
                // entirely rather than fall to `_ => {}`.
                let base_ty = self.check_expr(base, locals, label);
                // Phase 22 Finding 1: a Pest `beforeEach(...)` closure's own
                // `$this->prop = ...` legitimately defines a dynamic
                // property every other Pest closure in the file/directory
                // scope can read, real PHPUnit\Framework\TestCase
                // declaration or not - see `pest_hook_properties`'s own
                // doc comment. Keyed off the literal `$this` expression
                // (unlike the real `pestphp/pest-plugin-phpstan`
                // extension, which resolves by the base's type) - a
                // disclosed, evidence-free simplification.
                let is_pest_this = matches!(&base.kind, ExprKind::Variable(name) if name == "this");
                // Phase 34 Finding D: `stdClass` (a real, registered,
                // zero-property class in the vendored stubs -
                // `Core/Core_c.php`'s own `class stdClass {}`) is PHP's
                // canonical "arbitrary property bag" - real PHP (and
                // real PHPStan) never flag a dynamic property read or
                // write on it, by design. Scoped to *this* property-
                // access call site only, not `member_confirmed_missing`
                // itself, which two method-call checks below also share
                // - an undefined *method* call on `stdClass` is still a
                // real fatal error real PHPStan also flags.
                if self.member_confirmed_missing(&base_ty, |c| {
                    c == "stdClass"
                        || self.registry.lookup_property(c, prop).is_some()
                        || self.registry.lookup_method(c, "__get").is_some()
                        || (is_pest_this && self.pest_hook_properties.contains(prop.as_str()))
                }) {
                    self.report(
                        label.to_string(),
                        format!("access to undefined property {base_ty}::${prop}"),
                        e.span,
                        2,
                    );
                }
                if let Some(base_key) = narrowing_key(base)
                    && let Some(narrowed) = locals.get(&format!("{base_key}->{prop}"))
                {
                    return narrowed.clone();
                }
                let result = match narrow_type_excluding(&base_ty, &Type::Null) {
                    Type::Object(class, type_args) => self
                        .registry
                        .lookup_property_with_owner(&class, prop)
                        .map(|(owner, p)| {
                            let bindings = self.class_template_bindings(&class, &type_args, &owner);
                            let resolved = types::substitute_template(&p.ty, &bindings);
                            self.resolve_pseudo(&resolved, &class)
                        })
                        .unwrap_or(Type::Mixed),
                    _ => Type::Mixed,
                };
                if *nullsafe && result != Type::Mixed {
                    union_types(result, Type::Null)
                } else {
                    result
                }
            }
            ExprKind::DynamicPropertyAccess(base, name_expr) => {
                self.check_expr(base, locals, label);
                self.check_expr(name_expr, locals, label);
                Type::Mixed
            }
            ExprKind::DynamicMethodCall(recv, name_expr, args) => {
                self.check_expr(recv, locals, label);
                self.check_expr(name_expr, locals, label);
                for a in args {
                    self.check_expr(&a.value, locals, label);
                }
                Type::Mixed
            }
            ExprKind::Interpolated(parts) => {
                for p in parts {
                    self.check_expr(p, locals, label);
                }
                Type::String
            }
            ExprKind::Match {
                subject,
                arms,
                default,
            } => {
                self.check_expr(subject, locals, label);
                // Real `match` has no fallthrough - each arm is mutually
                // exclusive, so (unlike `switch`) a later arm/default can
                // always safely assume every earlier arm's own condition
                // was false. `match(true) { $cond => ... }` is a common,
                // deliberate `switch`-replacement idiom specifically
                // because each arm's own condition is checked with `===`
                // - treat the arm's condition itself as an `if`-style
                // condition in that case; otherwise narrow via an
                // implicit `subject === cond` equality, the same as an
                // ordinary `switch` case. Only attempted for a
                // single-condition arm (`1, 2 => ...` multi-value arms
                // fall back to unnarrowed, rather than guessing at a
                // combined "or" narrowing).
                let match_true = matches!(&subject.kind, ExprKind::Literal(Literal::Bool(true)));
                let mut accumulated = locals.clone();
                let mut merged: Option<Locals> = None;
                let mut result_types: Vec<Type> = Vec::new();
                for (conds, result) in arms {
                    for c in conds {
                        self.check_expr(c, locals, label);
                    }
                    let mut arm_locals = accumulated.clone();
                    if let [only_cond] = conds.as_slice() {
                        if match_true {
                            self.apply_narrowing(only_cond, &mut arm_locals, true);
                        } else {
                            self.apply_equality_narrowing(
                                subject,
                                only_cond,
                                &mut arm_locals,
                                true,
                            );
                        }
                    }
                    result_types.push(self.check_expr(result, &mut arm_locals, label));
                    merged = Some(match merged {
                        Some(m) => merge_scopes(&m, &arm_locals),
                        None => arm_locals.clone(),
                    });
                    if let [only_cond] = conds.as_slice() {
                        if match_true {
                            self.apply_narrowing(only_cond, &mut accumulated, false);
                        } else {
                            self.apply_equality_narrowing(
                                subject,
                                only_cond,
                                &mut accumulated,
                                false,
                            );
                        }
                    }
                }
                if let Some(d) = default {
                    let mut default_locals = accumulated.clone();
                    result_types.push(self.check_expr(d, &mut default_locals, label));
                    merged = Some(match merged {
                        Some(m) => merge_scopes(&m, &default_locals),
                        None => default_locals,
                    });
                }
                if let Some(m) = merged {
                    *locals = m;
                }
                // Same "don't let an unknown arm compose into a partial
                // Union that looks more precise than it is" rule Ternary
                // applies.
                if result_types.contains(&Type::Mixed) {
                    Type::Mixed
                } else {
                    let mut deduped: Vec<Type> = Vec::new();
                    for t in result_types {
                        if !deduped.contains(&t) {
                            deduped.push(t);
                        }
                    }
                    match deduped.len() {
                        0 => Type::Never,
                        1 => deduped.into_iter().next().unwrap(),
                        _ => Type::Union(deduped),
                    }
                }
            }
            ExprKind::Closure {
                params,
                uses,
                body,
                is_static,
                return_type,
                ..
            } => {
                // Unlike an arrow function, a plain closure does NOT
                // automatically see the enclosing scope - only variables
                // explicitly named in `use (...)` are captured (by value
                // or by reference; either way, the type it starts with is
                // whatever the outer scope currently has it as). `$this`
                // is the one implicit exception: a non-static closure
                // declared inside a method still auto-binds it, the same
                // as real PHP.
                let mut closure_locals: Locals = Locals::default();
                for u in uses {
                    let ty = locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    closure_locals.insert(u.name.clone(), ty);
                }
                if !is_static && let Some(this_ty) = locals.get("this") {
                    closure_locals.insert("this".to_string(), this_ty.clone());
                }
                for p in params {
                    let ty = self.resolve_type_hint(p.type_hint.as_deref());
                    let ty = if p.variadic {
                        Type::Array(Box::new(ty))
                    } else {
                        ty
                    };
                    closure_locals.insert(p.name.clone(), ty);
                }
                let declared_return = Some(self.resolve_type_hint(return_type.as_deref()));
                let closure_label = format!("{label} (closure)");
                // Finding 3: a generator-returning closure (`function ():
                // Generator { if ($done) { return; } yield $x; }`, a
                // plausible callback shape) needs the same
                // current_function_is_generator save/set/restore
                // check_function does for a named function/method -
                // check_block never routes through check_function at all.
                let previous_generator_flag = self.current_function_is_generator;
                self.current_function_is_generator = body_contains_yield(body);
                self.check_block(
                    body,
                    &mut closure_locals,
                    declared_return.as_ref(),
                    &closure_label,
                );
                self.current_function_is_generator = previous_generator_flag;
                Type::Object("Closure".to_string(), Vec::new())
            }
            ExprKind::ArrowFn {
                params,
                body,
                return_type,
                ..
            } => {
                // An arrow function DOES automatically capture the whole
                // enclosing scope by value (implicit, no `use (...)`
                // needed) - a plain clone of `locals` is exactly that.
                let mut arrow_locals = locals.clone();
                for p in params {
                    let ty = self.resolve_type_hint(p.type_hint.as_deref());
                    let ty = if p.variadic {
                        Type::Array(Box::new(ty))
                    } else {
                        ty
                    };
                    arrow_locals.insert(p.name.clone(), ty);
                }
                let body_ty = self.check_expr(body, &mut arrow_locals, label);
                // Its body is a single implicit-return expression, not a
                // StmtKind::Return - the equivalent of check_stmt's own
                // Return-vs-declared_return check, kept here since
                // there's no StmtKind::Return for it to share.
                if return_type.is_some() {
                    let expected = self.resolve_type_hint(return_type.as_deref());
                    if body_ty != Type::Mixed && !self.registry.is_assignable(&body_ty, &expected) {
                        self.report(
                            label.to_string(),
                            format!("return type mismatch: expected {expected}, got {body_ty}"),
                            body.span,
                            3,
                        );
                    }
                }
                Type::Object("Closure".to_string(), Vec::new())
            }
            // Finding 28: `foo(...)`/`$obj->method(...)`/`Class::method(...)`
            // (PHP 8.1 first-class-callable syntax) always wraps a
            // Call/MethodCall/StaticCall/DynamicMethodCall with an empty
            // argument list (this project's own parser convention) - not
            // dispatching to that inner node's own ordinary arm directly:
            // its own `check_args` would misread "no arguments were
            // written" as "zero arguments were passed to this call,"
            // producing a false "expects at least N argument(s)"
            // diagnostic for any real function/method with required
            // parameters. So this checks only existence - the one thing
            // that doesn't depend on argument count at all - mirroring
            // each inner arm's own existence-check logic without its
            // argument handling.
            ExprKind::FirstClassCallable(inner) => {
                match &inner.kind {
                    ExprKind::Call(callee, _) => {
                        if let ExprKind::Name(name) = &callee.kind {
                            if !is_language_construct(name) {
                                let (primary, fallback) =
                                    self.name_ctx.resolve_function_candidates(name);
                                let found = self.registry.lookup_function(&primary).is_some()
                                    || fallback.as_deref().is_some_and(|g| {
                                        self.registry.lookup_function(g).is_some()
                                    });
                                if !found {
                                    self.report(
                                        label.to_string(),
                                        format!("call to undefined function {primary}()"),
                                        e.span,
                                        0,
                                    );
                                }
                            }
                        } else {
                            self.check_expr(callee, locals, label);
                        }
                    }
                    ExprKind::MethodCall(recv, method, _, _) => {
                        let recv_ty = self.check_expr(recv, locals, label);
                        let recv_class = match &recv_ty {
                            Type::Object(class, _) => Some(class.clone()),
                            Type::Union(members) | Type::Intersection(members) => {
                                members.iter().find_map(|m| match m {
                                    Type::Object(class, _) => Some(class.clone()),
                                    _ => None,
                                })
                            }
                            _ => None,
                        };
                        if let Some(class) = &recv_class
                            && self.registry.classes.contains_key(class)
                            && self.member_confirmed_missing(&recv_ty, |c| {
                                self.registry.lookup_method(c, method).is_some()
                                    || self.registry.lookup_method(c, "__call").is_some()
                            })
                        {
                            self.report(
                                label.to_string(),
                                format!("call to undefined method {recv_ty}::{method}()"),
                                e.span,
                                0,
                            );
                        }
                    }
                    ExprKind::StaticCall(class_name, method, _) => {
                        if let Some(class) = self.resolve_static_class_name(class_name)
                            && self.registry.classes.contains_key(&class)
                            && self.registry.lookup_method(&class, method).is_none()
                            && self
                                .registry
                                .lookup_method(&class, "__callStatic")
                                .is_none()
                        {
                            self.report(
                                label.to_string(),
                                format!("call to undefined static method {class}::{method}()"),
                                e.span,
                                0,
                            );
                        }
                    }
                    ExprKind::DynamicMethodCall(recv, name_expr, _) => {
                        self.check_expr(recv, locals, label);
                        self.check_expr(name_expr, locals, label);
                    }
                    _ => {
                        self.check_expr(inner, locals, label);
                    }
                }
                Type::Object("Closure".to_string(), Vec::new())
            }
            ExprKind::Literal(lit) => match lit {
                Literal::Int(n) => Type::IntLiteral(*n),
                Literal::Float(_) => Type::Float,
                Literal::String(s) => Type::StringLiteral(s.clone()),
                Literal::Bool(true) => Type::True,
                Literal::Bool(false) => Type::False,
                Literal::Null => Type::Null,
            },
            ExprKind::Name(_) => Type::Mixed,
        }
    }

    pub(crate) fn infer_expr(&self, e: &Expr, locals: &Locals) -> Type {
        match &e.kind {
            ExprKind::Literal(lit) => match lit {
                Literal::Int(n) => Type::IntLiteral(*n),
                Literal::Float(_) => Type::Float,
                Literal::String(s) => Type::StringLiteral(s.clone()),
                Literal::Bool(true) => Type::True,
                Literal::Bool(false) => Type::False,
                Literal::Null => Type::Null,
            },
            ExprKind::Variable(name) => locals.get(name).cloned().unwrap_or(Type::Mixed),
            ExprKind::Grouping(inner) | ExprKind::Clone(inner, _) => self.infer_expr(inner, locals),
            // A closure/arrow-fn literal, and first-class-callable syntax
            // (`strlen(...)`, `$obj->method(...)`), all produce a real
            // `Closure` instance at runtime in PHP - infer the same
            // `Object("Closure", ...)` the native `Closure` type hint
            // itself now resolves to (rather than the broader
            // `Type::Callable`, which `callable`-hinted parameters
            // accept via `is_assignable`'s own separate leniency), so a
            // strictly `Closure`-typed parameter correctly accepts them.
            ExprKind::FirstClassCallable(_)
            | ExprKind::Closure { .. }
            | ExprKind::ArrowFn { .. } => Type::Object("Closure".to_string(), Vec::new()),
            ExprKind::Match {
                subject,
                arms,
                default,
            } => {
                // Same "don't let an unknown branch compose into a
                // partial Union that looks more precise than it is" rule
                // Ternary already applies, generalized to N arms plus an
                // optional default - and the same per-arm narrowing
                // `check_expr`'s own Match handling applies, so a
                // `default` arm returning the match's own subject infers
                // its real, narrowed type rather than the subject's
                // unnarrowed original type.
                let match_true = matches!(&subject.kind, ExprKind::Literal(Literal::Bool(true)));
                let mut accumulated = locals.clone();
                let mut result_types: Vec<Type> = Vec::new();
                for (conds, result) in arms {
                    let mut arm_locals = accumulated.clone();
                    if let [only_cond] = conds.as_slice() {
                        if match_true {
                            self.apply_narrowing(only_cond, &mut arm_locals, true);
                        } else {
                            self.apply_equality_narrowing(
                                subject,
                                only_cond,
                                &mut arm_locals,
                                true,
                            );
                        }
                    }
                    result_types.push(self.infer_expr(result, &arm_locals));
                    if let [only_cond] = conds.as_slice() {
                        if match_true {
                            self.apply_narrowing(only_cond, &mut accumulated, false);
                        } else {
                            self.apply_equality_narrowing(
                                subject,
                                only_cond,
                                &mut accumulated,
                                false,
                            );
                        }
                    }
                }
                if let Some(d) = default {
                    result_types.push(self.infer_expr(d, &accumulated));
                }
                if result_types.contains(&Type::Mixed) {
                    Type::Mixed
                } else {
                    let mut deduped: Vec<Type> = Vec::new();
                    for t in result_types {
                        if !deduped.contains(&t) {
                            deduped.push(t);
                        }
                    }
                    match deduped.len() {
                        0 => Type::Never,
                        1 => deduped.into_iter().next().unwrap(),
                        _ => Type::Union(deduped),
                    }
                }
            }
            // A throw expression never actually produces a value - Never
            // is exactly the right type for it, the same way it's used for
            // a function that always throws or exits.
            ExprKind::Throw(_) => Type::Never,
            // Never given a stable nominal type, since multiple such
            // expressions in the same file could share the placeholder
            // name - Mixed is the honest answer, not an oversight.
            ExprKind::NewAnonymousClass(..) => Type::Mixed,
            ExprKind::Cast(ty, _) => match ty.as_str() {
                "int" => Type::Int,
                "float" => Type::Float,
                "string" => Type::String,
                "bool" => Type::Bool,
                "array" => Type::Array(Box::new(Type::Mixed)),
                "object" => Type::Object("object".to_string(), Vec::new()),
                "unset" => Type::Null,
                _ => Type::Mixed,
            },
            ExprKind::Unary(UnaryOp::Not, _) => Type::Bool,
            // Real PHP's unary minus/plus convert a numeric string
            // operand to a number (`-"5"` is `int(-5)`, not `"-5"`) -
            // the same implicit-numeric-coercion rule arithmetic
            // operators apply generally.
            ExprKind::Unary(UnaryOp::Neg | UnaryOp::Plus, inner) => {
                match self.infer_expr(inner, locals) {
                    Type::String | Type::StringLiteral(_) => Type::Int,
                    other => other,
                }
            }
            ExprKind::Unary(_, inner) => self.infer_expr(inner, locals),
            ExprKind::Binary(op, l, r) => self.infer_binary(op, l, r, locals),
            ExprKind::Assign(_, rhs) => self.infer_expr(rhs, locals),
            // `self`/`parent`/`static` aren't lexed as keywords (see the
            // parser: they're indistinguishable from any other identifier),
            // so a literal `ExprKind::Name("self")` has to be special-cased
            // here rather than showing up as an Object of that name.
            ExprKind::New(class_expr, _) => match &class_expr.kind {
                ExprKind::Name(name) => match name.as_str() {
                    "self" | "parent" | "static" => {
                        let raw = match name.as_str() {
                            "self" => Type::SelfRef,
                            "parent" => Type::ParentRef,
                            _ => Type::StaticRef,
                        };
                        match &self.current_class {
                            Some(c) => self.resolve_pseudo(&raw, c),
                            None => raw,
                        }
                    }
                    _ => Type::Object(self.name_ctx.resolve(name), Vec::new()),
                },
                _ => Type::Mixed,
            },
            ExprKind::Call(callee, args) => match &callee.kind {
                ExprKind::Name(name) if !is_language_construct(name) => {
                    let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
                    self.registry
                        .lookup_function(&primary)
                        .map(|sig| (primary.as_str(), sig))
                        .or_else(|| {
                            fallback
                                .as_deref()
                                .and_then(|g| self.registry.lookup_function(g).map(|sig| (g, sig)))
                        })
                        .map(|(resolved_name, sig)| {
                            self.infer_builtin_call_return_type(resolved_name, sig, args, locals)
                        })
                        .unwrap_or_else(|| call_fallback_return_type(callee))
                }
                _ => call_fallback_return_type(callee),
            },
            ExprKind::MethodCall(recv, method, args, nullsafe) => {
                // A nullsafe receiver is virtually always nullable
                // (that's the entire reason to write `?->`), so its own
                // inferred type is virtually always `Union([Object(...),
                // Null])`, not a bare `Object` - strip `Null` out before
                // matching, the same real class the method is actually
                // being looked up on either way.
                let recv_ty = narrow_type_excluding(&self.infer_expr(recv, locals), &Type::Null);
                let result = match recv_ty {
                    Type::Object(class, type_args) => self
                        .registry
                        .lookup_method_with_owner(&class, method)
                        .map(|(owner, sig)| {
                            // Class-level substitution (Collection<User>'s
                            // own `T`, or an inherited method's
                            // owner-level `T` via Phase 10.5) runs first,
                            // then infer_call_return_type's existing
                            // per-call-argument binding handles any
                            // template the *method itself* separately
                            // declares - the two are independent
                            // parameter scopes that can both appear on
                            // the same signature.
                            let sig =
                                self.substitute_class_templates(sig, &class, &type_args, &owner);
                            let resolved_args = resolve_call_args(args, &sig.params);
                            let resolved =
                                self.infer_call_return_type(&sig, &resolved_args, locals);
                            self.resolve_pseudo(&resolved, &class)
                        })
                        .unwrap_or(Type::Mixed),
                    _ => Type::Mixed,
                };
                // `$x?->method()` short-circuits to `null` when `$x` is
                // null, on top of whatever the method's own return type
                // is - real, implicit nullability a plain `->` doesn't
                // carry.
                if *nullsafe && result != Type::Mixed {
                    union_types(result, Type::Null)
                } else {
                    result
                }
            }
            ExprKind::StaticCall(class_name, method, args) => self
                .resolve_static_class_name(class_name)
                .and_then(|class| {
                    self.registry.lookup_method(&class, method).map(|sig| {
                        let resolved_args = resolve_call_args(args, &sig.params);
                        let resolved = self.infer_call_return_type(sig, &resolved_args, locals);
                        self.resolve_pseudo(&resolved, &class)
                    })
                })
                .unwrap_or(Type::Mixed),
            ExprKind::StaticAccess(class_name, member) => {
                if member == "class" {
                    return match self.resolve_static_class_name(class_name) {
                        Some(class) => Type::ClassString(Box::new(Type::Object(class, Vec::new()))),
                        None => Type::String,
                    };
                }
                self.resolve_static_class_name(class_name)
                    .and_then(|class| match member.strip_prefix('$') {
                        Some(prop_name) => self
                            .registry
                            .lookup_property(&class, prop_name)
                            .map(|p| self.resolve_pseudo(&p.ty, &class)),
                        None => self
                            .registry
                            .lookup_const(&class, member)
                            .map(|ty| self.resolve_pseudo(ty, &class)),
                    })
                    .unwrap_or(Type::Mixed)
            }
            ExprKind::Ternary(cond, then_, else_) => {
                let mut else_locals = locals.clone();
                self.apply_narrowing(cond, &mut else_locals, false);
                let else_ty = self.infer_expr(else_, &else_locals);
                match then_ {
                    Some(t) => {
                        let mut then_locals = locals.clone();
                        self.apply_narrowing(cond, &mut then_locals, true);
                        let then_ty = self.infer_expr(t, &then_locals);
                        // Phase 23: see the parallel arm in `check_expr`
                        // for why this must be `union_types`, not a raw
                        // `Type::Union(vec![...])`.
                        union_types(then_ty, else_ty)
                    }
                    None => Type::Mixed,
                }
            }
            ExprKind::ArrayLiteral(items) => self.infer_array_literal_type(items, locals),
            ExprKind::Interpolated(_) => Type::String,
            ExprKind::PropertyAccess(base, prop, nullsafe) => {
                if let Some(base_key) = narrowing_key(base)
                    && let Some(narrowed) = locals.get(&format!("{base_key}->{prop}"))
                {
                    return narrowed.clone();
                }
                let base_ty = narrow_type_excluding(&self.infer_expr(base, locals), &Type::Null);
                let result = match base_ty {
                    Type::Object(class, type_args) => self
                        .registry
                        .lookup_property_with_owner(&class, prop)
                        .map(|(owner, p)| {
                            let bindings = self.class_template_bindings(&class, &type_args, &owner);
                            let resolved = types::substitute_template(&p.ty, &bindings);
                            self.resolve_pseudo(&resolved, &class)
                        })
                        .unwrap_or(Type::Mixed),
                    _ => Type::Mixed,
                };
                // Same implicit nullability as `?->method()` just above,
                // for `$x?->prop`.
                if *nullsafe && result != Type::Mixed {
                    union_types(result, Type::Null)
                } else {
                    result
                }
            }
            // A homogeneous array's element type applies regardless of
            // which key is accessed; optimistically assumes the access is
            // in bounds/present rather than widening to `V|null` for a
            // possibly-missing element - consistent with this checker's
            // general preference for fewer false positives over catching
            // every possible runtime-undefined access.
            ExprKind::Index(base, Some(idx)) => {
                // Phase 22: the `Index` counterpart of `PropertyAccess`'s
                // own "check the narrowed key first" fast path just above
                // - without this, `apply_narrowing` faithfully *wrote* a
                // narrowed type under `narrowing_key`'s `"base[key]"`
                // format (e.g. after `is_string($info['code'])`), but
                // nothing ever *read* it back: this arm went straight to
                // re-deriving the type from `$info`'s own declared/
                // inferred shape, silently discarding the narrowing.
                if let Some(key) = narrowing_key(e)
                    && let Some(narrowed) = locals.get(&key)
                {
                    return narrowed.clone();
                }
                let key = literal_string_key(idx);
                index_result_type(self.infer_expr(base, locals), key.as_deref())
            }
            _ => Type::Mixed,
        }
    }

    /// Finding 48: an array literal's own element types were never
    /// inferred precisely - every literal read as `Array(Mixed)`
    /// regardless of its actual contents, which silently defeated
    /// `strict_mixed_arrays` (Phase 11 Stage 5) for its own headline use
    /// case (checking a hand-written literal against a declared shape)
    /// and made any destructuring from an inline literal lose all
    /// per-position precision (Finding 42's own gap, from the same root
    /// cause). Builds a real `Type::ArrayShape`, keyed by each literal
    /// key (or, for a positional element, PHP's own real auto-index
    /// rule: one past the highest explicit integer key seen so far, not
    /// just a running count - `[5 => 'a', 'b']`'s `'b'` really is key
    /// `6` in real PHP). A spread element (`...$other`) or a computed,
    /// non-literal key both defeat precise inference for the *whole*
    /// literal - real PHP inserts a variable number of the spread
    /// value's own elements at that point, shifting every subsequent
    /// positional key by an amount this checker has no way to know
    /// statically, and a non-literal key has no fixed name to key a
    /// field by at all - degrading to the honest `Array(Mixed)` for
    /// those specific cases, matching the status quo everywhere else.
    pub(crate) fn infer_array_literal_type(&self, items: &[ArrayItem], locals: &Locals) -> Type {
        let mut fields: Vec<(String, Type, bool)> = Vec::new();
        let mut next_index: i64 = 0;
        for item in items {
            if item.spread {
                return Type::Array(Box::new(Type::Mixed));
            }
            let key = match &item.key {
                None => {
                    let k = next_index.to_string();
                    next_index += 1;
                    k
                }
                Some(k) => match &k.kind {
                    ExprKind::Literal(Literal::String(s)) => s.clone(),
                    ExprKind::Literal(Literal::Int(n)) => {
                        next_index = next_index.max(n + 1);
                        n.to_string()
                    }
                    _ => return Type::Array(Box::new(Type::Mixed)),
                },
            };
            let value_ty = self.infer_expr(&item.value, locals);
            // A later duplicate key overwrites an earlier one, matching
            // real PHP's own array-literal semantics.
            match fields.iter_mut().find(|(k, _, _)| *k == key) {
                Some(field) => field.1 = value_ty,
                None => fields.push((key, value_ty, false)),
            }
        }
        Type::ArrayShape(fields)
    }

    /// A generic function's return type may mention its own `@template`
    /// parameters (`TemplateParam`) - this binds each one against the
    /// actual argument at this call site (`identity(5)` binds `T` to
    /// `int`) and substitutes the result into the return type. A signature
    /// with no templates at all is untouched (the common case, cheap).
    pub(crate) fn infer_call_return_type(
        &self,
        sig: &FunctionSig,
        resolved: &[Option<&Expr>],
        locals: &Locals,
    ) -> Type {
        let mut bindings: TemplateBindings = TemplateBindings::default();
        for (param, slot) in sig.params.iter().zip(resolved.iter()) {
            let Some(arg) = slot else { continue };
            let arg_ty = self.infer_expr(arg, locals);
            types::bind_template_param(&param.ty, &arg_ty, &mut bindings);
        }
        types::substitute_template(&sig.return_type, &bindings)
    }

    /// Finding 48 made a literal array's own inferred type a real
    /// `Type::ArrayShape` instead of always `Array(Mixed)` - every one of
    /// Phase 12's own special cases below that used to pattern-match
    /// `Type::Array(elem_ty)` directly needs this instead, so
    /// `array_map(fn($x) => ..., [1, 2, 3])` (a literal, now a shape) keeps
    /// working the same way it already did for a plain `array<int>`-typed
    /// variable. A shape's own "effective element type" is the union of
    /// every field's type (`Mixed` for an empty literal - nothing to infer
    /// a narrower type from).
    pub(crate) fn array_element_type(ty: &Type) -> Option<Type> {
        match ty {
            Type::Array(elem) => Some((**elem).clone()),
            Type::ArrayShape(fields) => Some(
                fields
                    .iter()
                    .map(|(_, t, _)| t.clone())
                    .reduce(union_types)
                    .unwrap_or(Type::Mixed),
            ),
            _ => None,
        }
    }

    /// Phase 12's bespoke per-function return-type extensions (real
    /// PHPStan's own `src/Type/Php/` is 190 files of exactly this kind of
    /// special-casing, not generic machinery) - `array_map`/`array_filter`
    /// always return a bare, structureless `array` today (element type
    /// `Mixed`), discarding real information already available: the
    /// input array's own element type, and (for `array_map`) the
    /// callback's own return type. Falls through to the ordinary
    /// generic-template path (`infer_call_return_type`) for every other
    /// function, and for any shape of these two this doesn't specially
    /// handle.
    /// Deliberately stays on the raw, positional `args` (not Item 1's own
    /// per-parameter resolution) for every special case below - these are
    /// a small, fixed set of well-known standard-library functions
    /// nobody realistically calls with named arguments, so resolving
    /// them by name would add risk (a resolved slice isn't sized the same
    /// way a raw argument count is - see `check_args`'s own note on this
    /// exact distinction) for no real benefit. Only the generic fallback
    /// at the end needs the real resolution, computed here from the same
    /// raw `args` this function already has.
    pub(crate) fn infer_builtin_call_return_type(
        &self,
        name: &str,
        sig: &FunctionSig,
        args: &[Arg],
        locals: &Locals,
    ) -> Type {
        match name {
            // Scoped to the common 2-arg single-array form - the real
            // function is variadic (multiple arrays); a 3+-arg call falls
            // through to the generic path below, unchanged.
            "array_map" if args.len() == 2 => {
                if let Some(elem_ty) =
                    Self::array_element_type(&self.infer_expr(&args[1].value, locals))
                    && let Some(mapped) = self.infer_closure_return_with_param_overrides(
                        &args[0].value,
                        &[elem_ty],
                        locals,
                    )
                {
                    return Type::Array(Box::new(mapped));
                }
            }
            // Filtering never transforms elements, so the result is
            // simply whatever the input array's own type already is - no
            // closure introspection needed at all. Real PHP requires at
            // least 1 argument, but a syntactically-valid call passing
            // zero (invalid at runtime, real code in the vendor/ corpus
            // this project verifies against - a genuine, previously
            // unguarded panic here, found via --check-project against
            // the *full* corpus, not just first-party src/) still parses
            // fine and must not be indexed into blindly.
            "array_filter" if !args.is_empty() => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if matches!(arr_ty, Type::Array(_) | Type::ArrayShape(_)) {
                    return arr_ty;
                }
            }
            // array_reduce(array $array, callable $callback, mixed
            // $initial = null): mixed - a fold, so the result is
            // ultimately whatever the callback's own return type is
            // (each call's return becomes the next call's $carry). Binds
            // the callback's two params: $carry to $initial's own type
            // (Null when omitted, matching real PHP's own default), $item
            // to the array's element type.
            "array_reduce" if args.len() == 2 || args.len() == 3 => {
                if let Some(item_ty) =
                    Self::array_element_type(&self.infer_expr(&args[0].value, locals))
                {
                    let carry_ty = args
                        .get(2)
                        .map(|initial| self.infer_expr(&initial.value, locals))
                        .unwrap_or(Type::Null);
                    if let Some(reduced) = self.infer_closure_return_with_param_overrides(
                        &args[1].value,
                        &[carry_ty, item_ty],
                        locals,
                    ) {
                        return reduced;
                    }
                }
            }
            // Combining never changes the values' own type - $keys
            // becomes what our Type::Array can't represent at all (key
            // types aren't tracked), so the result is simply $values'
            // own type verbatim, the same "no transformation" shape as
            // array_filter above.
            "array_combine" if args.len() == 2 => {
                let values_ty = self.infer_expr(&args[1].value, locals);
                if matches!(values_ty, Type::Array(_) | Type::ArrayShape(_)) {
                    return values_ty;
                }
            }
            // Both fill every slot with the same, already-known $value -
            // no closure or element-type binding needed at all.
            "array_fill" if args.len() == 3 => {
                return Type::Array(Box::new(self.infer_expr(&args[2].value, locals)));
            }
            "array_fill_keys" if args.len() == 2 => {
                return Type::Array(Box::new(self.infer_expr(&args[1].value, locals)));
            }
            // Splits one array into several same-shaped ones - the
            // result is an array *of* arrays, each holding the original
            // element type.
            "array_chunk" if args.len() == 2 || args.len() == 3 => {
                if let Some(elem_ty) =
                    Self::array_element_type(&self.infer_expr(&args[0].value, locals))
                {
                    return Type::Array(Box::new(Type::Array(Box::new(elem_ty))));
                }
            }
            // Extracts one named field from an array of known shapes
            // (e.g. `array<array{id: int, name: string}>`) - only fires
            // when the element type is a real, known ArrayShape and the
            // column key is a compile-time-known literal, mirroring
            // check_shape_key_access's own "only a literal, never a
            // dynamic value" precedent.
            // Phase 31: the real stub's own `@return int[]|string[]` -
            // not a single homogeneous key type but a whole-array
            // either-or, since a real array's keys could be all-int,
            // all-string, or (rarer) a mix - printed as the confusing
            // `array<int>|array<string>` seen in this corpus's own
            // diagnostics. Scoped to the one-argument form (no
            // `$filter_value`, which changes which keys survive in a way
            // this doesn't try to predict) with a known `ArrayShape`
            // argument, where every key is knowable up front: PHP itself
            // casts a canonical non-negative-integer string key to a real
            // int at the point the array was built, so this mirrors that
            // per-key, folding into a real `int|string` union only when
            // the shape's own keys are genuinely mixed.
            "array_keys" if args.len() == 1 => {
                if let Type::ArrayShape(fields) = self.infer_expr(&args[0].value, locals) {
                    let key_ty = fields
                        .iter()
                        .map(|(k, _, _)| {
                            if is_canonical_int_key(k) {
                                Type::Int
                            } else {
                                Type::String
                            }
                        })
                        .reduce(union_types)
                        .unwrap_or(Type::String);
                    return Type::Array(Box::new(key_ty));
                }
            }
            "array_column" if args.len() == 2 || args.len() == 3 => {
                if let Some(Type::ArrayShape(fields)) =
                    Self::array_element_type(&self.infer_expr(&args[0].value, locals))
                    && let Some(key) = literal_string_key(&args[1].value)
                    && let Some((_, field_ty, _)) = fields.into_iter().find(|(n, _, _)| *n == key)
                {
                    return Type::Array(Box::new(field_ty));
                }
            }
            // Phase 32 Finding D: the real stub declares `microtime(bool
            // $as_float = false): string|float` unconditionally - real
            // PHPStan's own `MicrotimeFunctionReturnTypeExtension` narrows
            // a literal `true` argument to plain `float`, a literal
            // `false` (or no argument) to a string. `is_subtype`, not
            // `==`, since a `Union` that happens to be all-`True` or
            // all-`False` (e.g. narrowed from a wider type) should narrow
            // the same way a bare `True`/`False` would.
            "microtime" => match args.first().map(|a| self.infer_expr(&a.value, locals)) {
                None => return Type::String,
                Some(ty) if types::is_subtype(&ty, &Type::True) => return Type::Float,
                Some(ty) if types::is_subtype(&ty, &Type::False) => return Type::String,
                _ => {}
            },
            // Phase 23: the real stubs declare these unconditionally
            // unioned with `array` (`str_replace(...): array|string`),
            // even though the function only ever returns an array when
            // `$subject` itself is one - real PHPStan special-cases this
            // via its own hand-written dynamic return type extension, not
            // from the stub signature alone. Scoped to "subject is
            // exactly `Type::String`" - anything else (`Mixed`, a union, a
            // real array) correctly keeps today's existing behavior.
            "str_replace" | "str_ireplace" if args.len() == 3 || args.len() == 4 => {
                if self.infer_expr(&args[2].value, locals) == Type::String {
                    return Type::String;
                }
            }
            // Same subject-dependent shape as above, but these can also
            // return `null` on a regex error (already correctly present
            // in the real stub's own declared type) - computed from the
            // ordinary generic path first and then stripped of any
            // `array` member, rather than hardcoding the nullability this
            // project doesn't otherwise know for certain.
            "preg_replace" | "preg_replace_callback" | "preg_filter"
                if args.len() >= 3 && self.infer_expr(&args[2].value, locals) == Type::String =>
            {
                let resolved = resolve_call_args(args, &sig.params);
                let generic = self.infer_call_return_type(sig, &resolved, locals);
                return strip_array_member(generic);
            }
            // Phase 32 Finding E: the same subject-dependent-return-type
            // family as the two arms above, missed by Phase 23/31 - the
            // real stub is `substr_replace(array|string $string, ...):
            // array|string`, and unlike `str_replace`'s subject at index
            // 2, `substr_replace`'s own subject is index 0. Never returns
            // `null`, so a plain early return suffices - no
            // `strip_array_member` dance needed.
            "substr_replace" if args.len() == 3 || args.len() == 4 => {
                if types::is_subtype(&self.infer_expr(&args[0].value, locals), &Type::String) {
                    return Type::String;
                }
            }
            // Same family, subject at index 1 - not evidenced directly in
            // this corpus, but identical mechanism and real stub shape
            // (`preg_replace_callback_array(array $pattern, array|string
            // $subject, ...): array|string|null`), so kept alongside its
            // siblings rather than left out for want of a diagnostic.
            "preg_replace_callback_array"
                if args.len() >= 2
                    && types::is_subtype(
                        &self.infer_expr(&args[1].value, locals),
                        &Type::String,
                    ) =>
            {
                let resolved = resolve_call_args(args, &sig.params);
                let generic = self.infer_call_return_type(sig, &resolved, locals);
                return strip_array_member(generic);
            }
            // `mb_convert_encoding()` has its own real dynamic extension,
            // considerably more involved (it also reasons about
            // `$from_encoding` to decide whether `false` stays possible) -
            // only the array-stripping half is evidenced here
            // (`CharsetHelper::convertCharset`'s `mb_convert_encoding($str,
            // ...)` with `$str: string`), so that's all this implements.
            "mb_convert_encoding"
                if args.len() >= 2
                    && types::is_subtype(
                        &self.infer_expr(&args[0].value, locals),
                        &Type::String,
                    ) =>
            {
                let resolved = resolve_call_args(args, &sig.params);
                let generic = self.infer_call_return_type(sig, &resolved, locals);
                return strip_array_member(generic);
            }
            // Phase 32 Finding J: json_encode(...): string|false -
            // passing JSON_THROW_ON_ERROR makes a real failure throw
            // instead of returning false. Real PHPStan's own
            // JsonThrowOnErrorDynamicReturnTypeExtension recognizes this
            // via a bitwise-OR-aware constant check on the flags
            // argument - `contains_json_throw_on_error` walks an
            // arbitrary-depth `|`-chain the same way (real corpus shape:
            // `JSON_HEX_TAG | JSON_HEX_AMP | JSON_HEX_APOS | JSON_HEX_QUOT
            // | JSON_THROW_ON_ERROR`), not just a bare constant.
            "json_encode" if args.len() >= 2 && contains_json_throw_on_error(&args[1].value) => {
                let resolved = resolve_call_args(args, &sig.params);
                let generic = self.infer_call_return_type(sig, &resolved, locals);
                return strip_false_member(generic);
            }
            // Phase 34 Finding A: the real stub declares `curl_getinfo`
            // unconditionally `mixed` - real PHPStan's own
            // CurlGetinfoFunctionDynamicReturnTypeExtension maps a
            // literal CURLINFO_* option constant to a concrete type
            // instead. Scoped to the 2-arg form actually used anywhere
            // in real code (a bare 1-arg `curl_getinfo($ch)` call
            // returns a much bigger "all components" array shape this
            // project doesn't model - falls through to today's `mixed`
            // unchanged).
            "curl_getinfo" if args.len() == 2 => {
                if let ExprKind::Name(option_name) = &args[1].value.kind
                    && let Some(ty) =
                        curl_getinfo_component_type(option_name.trim_start_matches('\\'))
                {
                    return ty;
                }
            }
            // Phase 35 Finding E: the real stub declares `get_class`
            // unconditionally `string` - real PHPStan's own
            // GetClassDynamicReturnTypeExtension resolves the return type
            // from the argument's own class instead, the same
            // "the correct type lives in a dynamic extension, not the
            // stub" shape `curl_getinfo`/`json_encode` above already
            // establish. Scoped to the 1-arg form - the 0-arg form ("the
            // calling object's own class") was removed in PHP 8. An
            // argument whose own type isn't a concrete `Object` falls
            // through to today's plain `string`, unchanged - matching
            // real PHPStan's own fallback for the same case.
            "get_class" if args.len() == 1 => {
                if let Type::Object(class, _) = self.infer_expr(&args[0].value, locals) {
                    return Type::ClassString(Box::new(Type::Object(class, Vec::new())));
                }
            }
            _ => {}
        }
        let resolved = resolve_call_args(args, &sig.params);
        self.infer_call_return_type(sig, &resolved, locals)
    }

    /// Infers a closure or arrow-fn's own return type *as if* its
    /// parameters were bound to `param_overrides` (positionally),
    /// instead of the `Mixed` they'd otherwise get standalone (a
    /// closure/arrow-fn's declared param, with no further context, has
    /// no type unless natively hinted) - this is what lets
    /// `array_map(fn($x) => $x * 2, $ints)` infer a real `array<int>`
    /// return instead of `array<mixed>`, and `array_reduce` bind both its
    /// callback's `$carry`/`$item` params at once. Only the common
    /// exact-arity, non-variadic, non-by-ref shape is handled - a
    /// mismatched param count, or any variadic/by-ref param, safely
    /// returns `None`, falling back to the caller's own generic
    /// (imprecise but correct) behavior. Mirrors the closure-capture-
    /// scope construction `check_expr`'s own `ExprKind::Closure`/
    /// `ExprKind::ArrowFn` arms already do, but on this separate,
    /// read-only inference pass rather than the diagnostic-producing one.
    pub(crate) fn infer_closure_return_with_param_overrides(
        &self,
        callback: &Expr,
        param_overrides: &[Type],
        locals: &Locals,
    ) -> Option<Type> {
        match &callback.kind {
            ExprKind::ArrowFn {
                params,
                body,
                return_type,
                ..
            } => {
                if return_type.is_some() {
                    return Some(self.resolve_type_hint(return_type.as_deref()));
                }
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return None;
                }
                let mut arrow_locals = locals.clone();
                for (p, ty) in params.iter().zip(param_overrides) {
                    arrow_locals.insert(p.name.clone(), ty.clone());
                }
                Some(self.infer_expr(body, &arrow_locals))
            }
            ExprKind::Closure {
                params,
                uses,
                body,
                is_static,
                return_type,
                ..
            } => {
                if return_type.is_some() {
                    return Some(self.resolve_type_hint(return_type.as_deref()));
                }
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return None;
                }
                let mut closure_locals: Locals = Locals::default();
                for u in uses {
                    let ty = locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    closure_locals.insert(u.name.clone(), ty);
                }
                if !*is_static && let Some(this_ty) = locals.get("this") {
                    closure_locals.insert("this".to_string(), this_ty.clone());
                }
                for (p, ty) in params.iter().zip(param_overrides) {
                    closure_locals.insert(p.name.clone(), ty.clone());
                }
                // Only the common single-implicit-return-statement body
                // shape is handled (mirrors check_shape_key_access's own
                // "only handle the shape that actually comes up, don't
                // guess at the rest" precedent) - a closure with a more
                // complex body safely falls back to None too.
                match body.as_slice() {
                    [
                        Stmt {
                            kind: StmtKind::Return(Some(e)),
                            ..
                        },
                    ] => Some(self.infer_expr(e, &closure_locals)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Phase 22 Finding 4: `??`'s result is *usually* just `r`'s own
    /// type (`ExprKind::Binary(BinOp::Coalesce, ...)` and this function's
    /// own `BinOp::Coalesce` arm both start from `r_ty` alone) - widening
    /// away from that is deliberately narrow, not a blanket "always union
    /// `non_null(l)` with `r`" fix: that would pollute the extremely
    /// common and currently-precise `$x ?? 'literal'` pattern (where
    /// `$x`'s real type is often genuinely `Mixed`) with unwanted `Mixed`
    /// noise across a much broader, currently-well-served set of call
    /// sites. Only widens when `l`'s real non-null type is `Type::Array`
    /// (generic/unshaped) and `r`'s own type is a narrower
    /// `Type::ArrayShape` - `$details = $row->details ?? [];` where
    /// `$row->details: ?array` genuinely holds arbitrary keys at runtime
    /// collapsed to the literal empty `[]` shape from the fallback alone,
    /// so every later `$details['key']` read looked like a typo.
    /// Deliberately not covering the sibling cases disclosed in
    /// ROADMAP.md's own Phase 22 write-up (`l`'s non-null type being a
    /// *different* non-empty `ArrayShape`, or `r` being a `Union`
    /// containing one) - no corpus evidence either occurs, same class of
    /// bug, left for a future pass if one turns up.
    fn widen_coalesce_result(&self, l: &Expr, r_ty: Type, locals: &Locals) -> Type {
        let l_ty = narrow_type_excluding(&self.infer_expr(l, locals), &Type::Null);
        match (&l_ty, &r_ty) {
            (Type::Array(_), Type::ArrayShape(_)) => l_ty,
            _ => r_ty,
        }
    }

    pub(crate) fn infer_binary(&self, op: &BinOp, l: &Expr, r: &Expr, locals: &Locals) -> Type {
        match op {
            BinOp::Concat => Type::String,
            BinOp::Eq
            | BinOp::Identical
            | BinOp::NotEq
            | BinOp::NotIdentical
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::LtEq
            | BinOp::GtEq
            | BinOp::And
            | BinOp::Or
            | BinOp::LogicalXor => Type::Bool,
            // The spaceship operator returns -1/0/1 (an int), not a bool -
            // its single most common real use is returning a value
            // directly from a comparator (`usort`/`uasort`/...).
            BinOp::Spaceship => Type::Int,
            BinOp::Add => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                // `+` on two arrays is PHP's array-union operator, not
                // arithmetic - it keeps every key from the left operand,
                // filling in any key only present on the right. The real
                // per-key result type isn't something `Type::Array`/
                // `Type::ArrayShape` can express precisely today, so
                // `Array(Mixed)` is the honest answer.
                if matches!(lt, Type::Array(_) | Type::ArrayShape(_))
                    && matches!(rt, Type::Array(_) | Type::ArrayShape(_))
                {
                    return Type::Array(Box::new(Type::Mixed));
                }
                if lt == Type::Float || rt == Type::Float {
                    Type::Float
                } else {
                    Type::Int
                }
            }
            BinOp::Sub | BinOp::Mul | BinOp::Pow => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if lt == Type::Float || rt == Type::Float {
                    Type::Float
                } else {
                    Type::Int
                }
            }
            // `%` and the bitwise/shift operators always cast both
            // operands to int before operating, regardless of whether
            // either side was a float - unlike Add/Sub/Mul/Pow, there is
            // no float-promotion rule for these.
            BinOp::Mod | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                Type::Int
            }
            // Real PHP division returns int or float depending on the
            // runtime values, not just their static types - not modeled.
            BinOp::Div => Type::Mixed,
            BinOp::Coalesce => {
                let r_ty = self.infer_expr(r, locals);
                self.widen_coalesce_result(l, r_ty, locals)
            }
            // `$value |> callable` calls callable($value) - its return
            // type depends on resolving whatever expression `callable` is
            // (a string, a closure, a first-class callable reference...)
            // to a real signature, which this checker doesn't attempt.
            BinOp::Pipe => Type::Mixed,
        }
    }
}

/// Phase 22 Finding 6: after checking `&&`/`||`'s right operand against a
/// cloned, possibly-narrowed `Locals`, merges back only the keys that are
/// both new (absent from `locals` before checking the right operand) and
/// a real PHP variable name (`is_real_variable_key`) - a genuine
/// definitional side effect (a by-ref out-param like `preg_match`'s
/// `$matches`, or a plain assignment) written as the right operand of
/// `&&`/`and`/`||`/`or` should persist regardless of the whole
/// expression's truth value, matching this codebase's "assume it might
/// be defined" philosophy elsewhere (`predefine_byref_or_unknown_args`).
///
/// Narrowing-derived synthetic keys (`narrowing_key`'s own
/// `"base->prop"`/`"base[key]"` shapes) must **not** be merged this way,
/// even though they're just as "new" by the same test - they're
/// conditional on the right operand actually having run (true only when
/// the left operand was truthy), not a real, unconditional definition.
/// Merging them would leak conditional narrowing straight past the whole
/// `&&`/`||` expression: for `if ($arg->value instanceof Variable &&
/// $arg->value->name) {...} else {...}`, it would insert `$arg->value`'s
/// narrowed type into the real `locals` *before* the `if`'s own
/// then/else split even runs, contaminating the `else` branch (where the
/// condition was false, so `$arg->value` is not known to be a
/// `Variable`) and anything after the whole `if` statement.
/// `is_real_variable_key` is exactly the discriminator that excludes
/// them, and composes correctly for a chained `A and B and C`
/// (left-associative `And(And(A,B),C)`): the inner node's own filtered
/// merge already lands B's new plain-variable defs in the outer node's
/// real `locals` before the outer node clones it for `C`.
fn merge_new_variable_definitions(locals: &mut Locals, narrowed: &Locals) {
    for (key, ty) in narrowed {
        if is_real_variable_key(key) && !locals.contains_key(key) {
            locals.insert(key.clone(), ty.clone());
        }
    }
}

/// The type of `container[key]` for a known container type - shared by
/// `infer_expr`'s own `Index` arm and its own recursion into a
/// `Type::Union`'s members. Phase 12 Finding E: `preg_match`'s own
/// `Union` of `ArrayShape`s (the matched shape, unioned with an empty one
/// for the no-match case) needs this same per-key resolution combined
/// across every member via `union_types`, not silently dropped to
/// `Mixed` for the whole type the moment it's a `Union` at all - though
/// for that *specific* pair of members, the combined result still lands
/// on `Mixed` anyway (the empty-shape member never has any real group's
/// key, so its own fallback below is always `Mixed`, which `union_types`
/// unconditionally absorbs into) - real, general infrastructure for a
/// `Union` whose members *agree* on a key's type, not what recovers
/// precision for `preg_match`'s own common case (that's `apply_narrowing`'s
/// own new `preg_match_condition` recognizer, `checker/narrowing.rs`).
fn index_result_type(container_ty: Type, key: Option<&str>) -> Type {
    match container_ty {
        Type::Array(inner) => *inner,
        Type::ArrayShape(fields) => key
            .and_then(|k| fields.into_iter().find(|(name, _, _)| name == k))
            .map(|(_, ty, _)| ty)
            .unwrap_or(Type::Mixed),
        Type::Union(members) => members
            .into_iter()
            .map(|m| index_result_type(m, key))
            .reduce(union_types)
            .unwrap_or(Type::Mixed),
        _ => Type::Mixed,
    }
}

/// A defensive cap on how many distinct members `union_types` will ever
/// materialize into a real `Type::Union`, mirroring the `MAX_GROUP_DEPTH`/
/// `MAX_EXPR_DEPTH` precedent (Phase 20). Collapsing to `Mixed` past this
/// point is a stable fixed point (`union_types(Mixed, _) == Mixed` is this
/// function's own first special case below), so a variable that blows past
/// the cap once stays collapsed rather than being recomputed on every
/// subsequent merge. Found necessary by a real hang, not guessed: `gdb`
/// caught a `check-project` run spinning inside `Type::eq`, called from
/// `union_types`, called from `merge_scopes` (`checker/control_flow.rs`,
/// once per `if`/`else`) and from `write_index_into`'s own `Union` arm
/// (`checker/assignment.rs`, Phase 12 Finding E). Once Finding E made
/// `Type::Union` "stick" across reads/writes instead of immediately
/// collapsing to `Mixed`, a real corpus function with many sequential and
/// nested branches assigning genuinely distinct array shapes to the same
/// variable grew that variable's own union member count combinatorially -
/// each subsequent `contains`-based dedup scan below (and each member's own
/// recursive `PartialEq`) growing right along with it.
const MAX_UNION_MEMBERS: usize = 12;

/// Union of two types for a scope-merge join point - flattens nested
/// unions and keeps `Mixed` "unknown" rather than let it compose into a
/// falsely-precise-looking Union (same principle as the ternary inference).
pub(crate) fn union_types(a: Type, b: Type) -> Type {
    if a == b {
        return a;
    }
    if a == Type::Mixed || b == Type::Mixed {
        return Type::Mixed;
    }
    let mut members = Vec::new();
    match a {
        Type::Union(m) => members.extend(m),
        other => members.push(other),
    }
    match b {
        Type::Union(m) => {
            for x in m {
                if !members.contains(&x) {
                    members.push(x);
                }
            }
        }
        other => {
            if !members.contains(&other) {
                members.push(other);
            }
        }
    }
    // Phase 32 Finding L: drop a literal member whenever its own exact
    // base type is *also* present in the same union - a ternary's two
    // branches (`is_string($x) ? $x : ''`, `check_expr`'s own `Ternary`
    // arm calling this) build exactly this shape,
    // `union_types(String, StringLiteral(""))`, and keeping both was
    // this project's actual root cause for `SectionInitializer::parse()`'s
    // `str_replace()` subject never matching `== Type::String` again
    // after the ternary. Snapshotting which base types are present
    // first, rather than checking `members` from inside its own
    // `retain` closure (which Rust's borrow checker won't allow).
    let present_bases: Vec<Type> = members
        .iter()
        .filter(|m| matches!(m, Type::Int | Type::String | Type::Bool))
        .cloned()
        .collect();
    members.retain(|m| match literal_base_type(m) {
        Some(base) => !present_bases.contains(&base),
        None => true,
    });
    if members.len() > MAX_UNION_MEMBERS {
        return Type::Mixed;
    }
    if members.len() == 1 {
        members.into_iter().next().unwrap()
    } else {
        Type::Union(members)
    }
}

/// The "lossless absorption" half of `union_types`' own literal-dedup
/// fix - an `IntLiteral`/`StringLiteral`/`True`/`False` member is
/// redundant once its own *exact* base type is already present in the
/// same union (an `Int` member already includes every possible
/// `IntLiteral`'s value, etc.), mirroring `is_subtype`'s own first four
/// match arms specifically (`types.rs` lines 498-500) rather than
/// calling `is_subtype` itself - `is_subtype`'s int-to-float widening
/// rule is a real, load-bearing *assignability* fact (an int argument is
/// accepted wherever a float is declared), not a "this value's
/// representation is a subset of that value's representation" one, so
/// reusing it here would also silently drop an `IntLiteral` sitting next
/// to an unrelated `Float` member, discarding real information for a
/// relation that only holds for compatibility, not equivalence.
fn literal_base_type(t: &Type) -> Option<Type> {
    match t {
        Type::IntLiteral(_) => Some(Type::Int),
        Type::StringLiteral(_) => Some(Type::String),
        Type::True | Type::False => Some(Type::Bool),
        _ => None,
    }
}

/// The type of a call this project can't look up a real signature for -
/// `exit`/`die` are real PHP language constructs (`is_language_construct`)
/// with no stub of their own, but unlike the others in that list
/// (`isset`/`empty`/`unset`/`eval`/`include*`/`print`), PHP itself declares
/// them `never`: control flow can't continue past one. `always_terminates`
/// (`checker/control_flow.rs`) recognizes a terminating expression
/// statement via exactly `infer_expr(e, locals) == Type::Never` - without
/// this, a guard clause built on `exit()`/`die()` instead of `return`/
/// `throw` (extremely common in this codebase's CLI tools and test
/// fixtures) was never recognized as terminating, leaking its own
/// never-actually-reached branch's narrowing into the surrounding scope.
fn call_fallback_return_type(callee: &Expr) -> Type {
    match &callee.kind {
        ExprKind::Name(name) if matches!(name.to_ascii_lowercase().as_str(), "exit" | "die") => {
            Type::Never
        }
        _ => Type::Mixed,
    }
}

/// Whether a `Type::ArrayShape` key string is one PHP itself would have
/// already cast to a real integer at the point the array was built -
/// PHP's own rule: a decimal string with no leading zero (unless it's
/// exactly `"0"`) and no leading `+`. Deliberately doesn't handle a
/// leading `-` (a negative-integer key string, real but rare enough as a
/// literal shape key that no evidence calls for it) - misclassifying one
/// as a string key here is the same safe-by-omission default this
/// project uses throughout rather than a wrong answer.
fn is_canonical_int_key(key: &str) -> bool {
    !key.is_empty()
        && key.bytes().all(|b| b.is_ascii_digit())
        && (key == "0" || !key.starts_with('0'))
}

/// Removes any `Array`/`ArrayShape` member from `ty` - the array-ish
/// counterpart of `narrow_type_excluding`, which matches a specific value
/// rather than a whole type *kind*. Used for the `preg_replace` family,
/// where the real return type includes `array` only when `$subject` is
/// itself an array - once that's confirmed not the case, this drops the
/// always-possible-per-the-stub `array` member while leaving everything
/// else (a real declared `null`, say) untouched.
fn strip_array_member(ty: Type) -> Type {
    match ty {
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .into_iter()
                .filter(|m| !matches!(m, Type::Array(_) | Type::ArrayShape(_)))
                .collect();
            match kept.len() {
                0 => Type::Mixed,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        Type::Array(_) | Type::ArrayShape(_) => Type::Mixed,
        other => other,
    }
}

/// The `json_encode`/Finding J counterpart of `strip_array_member` above -
/// identical shape, filtering `Type::False` instead of `Array`/`ArrayShape`.
fn strip_false_member(ty: Type) -> Type {
    match ty {
        Type::Union(members) => {
            let kept: Vec<Type> = members.into_iter().filter(|m| *m != Type::False).collect();
            match kept.len() {
                0 => Type::Mixed,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        Type::False => Type::Mixed,
        other => other,
    }
}

/// Whether `e` is (or bitwise-ORs together, to any depth) a `Name`
/// resolving to `JSON_THROW_ON_ERROR` - real corpus shapes include both a
/// bare `JSON_THROW_ON_ERROR` and a multi-flag chain
/// (`JSON_HEX_TAG | JSON_HEX_AMP | ... | JSON_THROW_ON_ERROR`). Tolerates
/// a leading-backslash global name (`\JSON_THROW_ON_ERROR`), seen as-is
/// in the corpus.
fn contains_json_throw_on_error(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Name(name) => name.trim_start_matches('\\') == "JSON_THROW_ON_ERROR",
        ExprKind::Binary(BinOp::BitOr, l, r) => {
            contains_json_throw_on_error(l) || contains_json_throw_on_error(r)
        }
        ExprKind::Grouping(inner) => contains_json_throw_on_error(inner),
        _ => false,
    }
}

/// The type of one matched group's "leaf" value in `preg_match`'s own
/// `$matches` shape - plain `string` normally, or the real, positional
/// (not named-field) `[string, int]` tuple `PREG_OFFSET_CAPTURE`
/// actually produces at runtime (the match text and its byte offset) -
/// confirmed directly against a live PHP interpreter, not assumed from
/// PHPStan's own different naming convention for the same idea.
fn preg_leaf_type(offset_capture: bool) -> Type {
    if offset_capture {
        Type::ArrayShape(vec![
            ("0".to_string(), Type::String, false),
            ("1".to_string(), Type::Int, false),
        ])
    } else {
        Type::String
    }
}

/// Builds `preg_match`'s own `$matches` shape from a pattern's parsed
/// capture groups (`regex_groups::parse_capture_groups`): field `"0"` is
/// always present (the whole match); each group gets a numbered field
/// with `optional: group.optional`; a named group gets an *additional*
/// field under its own name with identical type/optionality (real PHP
/// places both the numeric and named key in `$matches`). Skips adding a
/// named field whose name is already present - real PCRE allows the same
/// group name on two different alternation branches without the `J`
/// modifier (`(?<x>a)|(?<x>b)` is ordinary, valid PCRE), which would
/// otherwise produce a shape with a duplicated field.
///
/// `unmatched_as_null`: `PREG_UNMATCHED_AS_NULL` changes a
/// non-participating optional group from "entirely absent from the
/// array" to "always present, as `null`" - confirmed directly against a
/// live PHP interpreter. A group that's never optional at all is
/// unaffected either way, since it can never have a non-participating
/// slot to turn nullable.
pub(crate) fn preg_match_matches_shape(
    groups: &[regex_groups::RegexCaptureGroup],
    leaf: &Type,
    unmatched_as_null: bool,
) -> Type {
    let mut fields = vec![("0".to_string(), leaf.clone(), false)];
    for g in groups {
        let (ty, optional) = if g.optional && unmatched_as_null {
            (union_types(leaf.clone(), Type::Null), false)
        } else {
            (leaf.clone(), g.optional)
        };
        fields.push((g.number.to_string(), ty.clone(), optional));
        if let Some(name) = &g.name
            && !fields.iter().any(|(n, _, _)| n == name)
        {
            fields.push((name.clone(), ty, optional));
        }
    }
    Type::ArrayShape(fields)
}

/// Builds `preg_match_all`'s own `$matches` shape for its default
/// `PREG_PATTERN_ORDER` mode - structurally different from
/// `preg_match_matches_shape`'s single-match shape: confirmed directly
/// against a live PHP interpreter that *every* key (`0`, each group
/// number, each name) is always present, as an *array* of the leaf type,
/// regardless of match count (a genuine zero-match call still populates
/// every key with an empty array, not the whole thing collapsing to
/// `[]`), and a group that doesn't participate in one particular match
/// still gets a slot in that key's own array (`''` normally, or `null`
/// under `PREG_UNMATCHED_AS_NULL`) rather than an entirely missing
/// element. So no field is ever `optional` at the top level here, and a
/// group's own `optional` flag only ever affects the *element* type
/// nested inside its array (when combined with `PREG_UNMATCHED_AS_NULL`),
/// never whether the top-level key itself is present.
fn preg_match_all_pattern_order_shape(
    groups: &[regex_groups::RegexCaptureGroup],
    leaf: &Type,
    unmatched_as_null: bool,
) -> Type {
    let element_ty = |optional: bool| {
        if optional && unmatched_as_null {
            union_types(leaf.clone(), Type::Null)
        } else {
            leaf.clone()
        }
    };
    let mut fields = vec![(
        "0".to_string(),
        Type::Array(Box::new(element_ty(false))),
        false,
    )];
    for g in groups {
        let arr_ty = Type::Array(Box::new(element_ty(g.optional)));
        fields.push((g.number.to_string(), arr_ty.clone(), false));
        if let Some(name) = &g.name
            && !fields.iter().any(|(n, _, _)| n == name)
        {
            fields.push((name.clone(), arr_ty, false));
        }
    }
    Type::ArrayShape(fields)
}

/// A bitwise-OR-chain flag collector for `preg_match`/`preg_match_all`'s
/// own `$flags` argument, mirroring `contains_json_throw_on_error`'s own
/// recursive shape but collecting a small flag struct instead of a single
/// boolean.
#[derive(Default, Clone, Copy)]
struct PregFlags {
    offset_capture: bool,
    unmatched_as_null: bool,
    // Only meaningful for `preg_match_all`; `false` = `PREG_PATTERN_ORDER`
    // (the real default).
    set_order: bool,
}

/// Walks `e` as a `Name` or a `BitOr`/`Grouping` chain of `Name`s,
/// recognizing exactly `PREG_OFFSET_CAPTURE`/`PREG_UNMATCHED_AS_NULL`
/// (and, when `allow_order` is set, `PREG_PATTERN_ORDER`/
/// `PREG_SET_ORDER`). `None` if any leaf name isn't one of these, or if
/// both `PATTERN_ORDER` and `SET_ORDER` appear together - never guessed.
fn parse_preg_flags(e: &Expr, allow_order: bool) -> Option<PregFlags> {
    fn collect<'a>(e: &'a Expr, out: &mut Vec<&'a str>) -> bool {
        match &e.kind {
            ExprKind::Name(name) => {
                out.push(name.trim_start_matches('\\'));
                true
            }
            ExprKind::Binary(BinOp::BitOr, l, r) => collect(l, out) && collect(r, out),
            ExprKind::Grouping(inner) => collect(inner, out),
            _ => false,
        }
    }
    let mut names = Vec::new();
    if !collect(e, &mut names) {
        return None;
    }
    let mut flags = PregFlags::default();
    let mut order_seen = 0;
    for name in names {
        match name {
            "PREG_OFFSET_CAPTURE" => flags.offset_capture = true,
            "PREG_UNMATCHED_AS_NULL" => flags.unmatched_as_null = true,
            "PREG_PATTERN_ORDER" if allow_order => order_seen += 1,
            "PREG_SET_ORDER" if allow_order => {
                flags.set_order = true;
                order_seen += 1;
            }
            _ => return None,
        }
    }
    if order_seen > 1 {
        return None;
    }
    Some(flags)
}

/// `curl_getinfo($ch, CONSTANT)`'s own real per-option return type - a
/// curated subset of real PHPStan's own ~70-entry
/// `CurlGetinfoFunctionDynamicReturnTypeExtension` table, covering the
/// commonly-used constants real code actually reaches for (every real
/// call site in this project's own verification corpus uses one of
/// these) rather than transcribing the full table, including its RTSP/
/// proxy/microsecond/bignum variants, for entries nothing here gives any
/// evidence about. `None` for anything else falls through to this
/// project's existing generic (`mixed`) behavior, unchanged.
fn curl_getinfo_component_type(name: &str) -> Option<Type> {
    let string_false = Type::Union(vec![Type::String, Type::False]);
    Some(match name {
        "CURLINFO_EFFECTIVE_URL"
        | "CURLINFO_PRIMARY_IP"
        | "CURLINFO_LOCAL_IP"
        | "CURLINFO_SCHEME"
        | "CURLINFO_EFFECTIVE_METHOD" => Type::String,
        "CURLINFO_REDIRECT_URL" | "CURLINFO_HEADER_OUT" | "CURLINFO_CONTENT_TYPE" => string_false,
        "CURLINFO_TOTAL_TIME"
        | "CURLINFO_NAMELOOKUP_TIME"
        | "CURLINFO_CONNECT_TIME"
        | "CURLINFO_PRETRANSFER_TIME"
        | "CURLINFO_STARTTRANSFER_TIME"
        | "CURLINFO_REDIRECT_TIME"
        | "CURLINFO_SIZE_UPLOAD"
        | "CURLINFO_SIZE_DOWNLOAD"
        | "CURLINFO_SPEED_DOWNLOAD"
        | "CURLINFO_SPEED_UPLOAD"
        | "CURLINFO_CONTENT_LENGTH_DOWNLOAD"
        | "CURLINFO_CONTENT_LENGTH_UPLOAD" => Type::Float,
        "CURLINFO_FILETIME"
        | "CURLINFO_REDIRECT_COUNT"
        | "CURLINFO_PRIMARY_PORT"
        | "CURLINFO_LOCAL_PORT"
        | "CURLINFO_HEADER_SIZE"
        | "CURLINFO_REQUEST_SIZE"
        | "CURLINFO_SSL_VERIFYRESULT"
        | "CURLINFO_RESPONSE_CODE"
        | "CURLINFO_HTTP_CODE"
        | "CURLINFO_HTTP_CONNECTCODE"
        | "CURLINFO_OS_ERRNO"
        | "CURLINFO_NUM_CONNECTS"
        | "CURLINFO_PROTOCOL"
        | "CURLINFO_HTTP_VERSION" => Type::Int,
        "CURLINFO_PRIVATE" => Type::Mixed,
        _ => return None,
    })
}

/// Peels through any `(...)`-grouping wrapper(s) to the real expression
/// underneath - `ExprKind::Grouping` is a genuine, distinct AST node
/// (not something parens collapse away during parsing), so a
/// closure-literal-shaped check like Finding 49's own `Closure::bind`/
/// `->bindTo()` special-casing needs to see through
/// `(function () {...})` the same as a bare `function () {...}`.
pub(crate) fn unwrap_grouping(e: &Expr) -> &Expr {
    match &e.kind {
        ExprKind::Grouping(inner) => unwrap_grouping(inner),
        _ => e,
    }
}
