use crate::ast::*;
use crate::checker::{
    ATTRIBUTE_TARGET_CLASS, ATTRIBUTE_TARGET_CLASS_CONSTANT, ATTRIBUTE_TARGET_PROPERTY, Checker,
    Locals, generator_missing_return_type, generator_template_param, merge_scopes,
    purge_stale_narrowing_for,
};
use crate::docblock;
use crate::types::Type;

mod loops;
mod switch;
mod try_catch;

impl<'a> Checker<'a> {
    pub(crate) fn check_block(
        &mut self,
        stmts: &[Stmt],
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        // Phase 15 item (1), `UnreachableStatementRule`: investigated,
        // corpus-verified, reverted, and now reinstated. The original
        // revert's root cause - a variable reassigned inside a loop kept
        // its pre-loop-entry type for the whole single-pass body check,
        // so a later guard on it could wrongly narrow to `Type::Never`,
        // read here as "this statement always terminates" - is fixed by
        // Phase 55's fixed-point loop-body convergence (`converge_loop_
        // body`). Real PHPStan's own equivalent (`UnreachableStatementRule`,
        // `deadCode.unreachable`, level 4) reports exactly once per
        // maximal unreachable run, at its first statement - matched here
        // via `reported_unreachable`, gating further reports without
        // gating further checking. Every statement is still run through
        // `check_stmt` regardless - real PHPStan still fully analyzes
        // code after an `UnreachableStatementNode` for every other rule,
        // and this project's own corpus verification confirmed why:
        // stopping early here silently dropped real, unrelated diagnostics
        // that happened to sit in a since-dead branch (amphp/http-client's
        // `Http1Parser::parse`, verified against an early, since-reverted
        // version of this change that did stop).
        let mut reported_unreachable = false;
        for (i, stmt) in stmts.iter().enumerate() {
            self.check_stmt(stmt, locals, declared_return, label);
            if !reported_unreachable
                && self.always_terminates(stmt, locals)
                && let Some(next) = stmts.get(i + 1)
                // A labeled statement `goto`-targeted somewhere else in
                // this same function is a real jump target, not dead
                // code, whatever the statement immediately before it
                // does - see `collect_goto_targets`'s own doc comment
                // for the real corpus false positive this guards
                // against.
                && !matches!(&next.kind, StmtKind::Label(name) if self.current_function_goto_targets.contains(name))
            {
                self.report(
                    label.to_string(),
                    "Unreachable statement - code above always terminates.".to_string(),
                    next.span,
                    4,
                    "deadCode.unreachable",
                );
                reported_unreachable = true;
            }
        }
    }

    #[hotpath::measure]
    pub(crate) fn check_stmt(
        &mut self,
        stmt: &Stmt,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        let inline_var = self.inline_var_annotation(stmt);
        if let Some((name, ty)) = &inline_var {
            purge_stale_narrowing_for(locals, name);
            self.check_inline_var_annotation_type(stmt, name, ty, locals, label);
            locals.insert(name.clone(), ty.clone());
        }
        match &stmt.kind {
            StmtKind::Expression(e) => {
                self.in_bare_statement_position = true;
                self.check_expr(e, locals, label);
                self.check_no_discard_statement(e, locals, label);
                // Phase 41 stage 1: NoopRule's own supported slice - see
                // `checker::noop`'s own module doc comment.
                self.check_noop_statement(e, label);
                // Phase 41/82: a bare call to a now-known-pure declaration.
                self.check_pure_call_without_effect(e, locals, label);
                // `assert($cond)` is a real, common narrowing idiom real
                // PHPStan explicitly documents support for - if execution
                // continues past this statement, `$cond` held, so it's
                // narrowed the same way a terminating `if`-guard clause's
                // surviving branch already is, just without any
                // branch/merge machinery since there's only one path
                // forward (assert either holds or the program aborts).
                // Deliberately not `instanceof`-specific: apply_narrowing
                // already handles whatever condition shapes it
                // understands and no-ops harmlessly on the rest.
                //
                // `name.trim_start_matches('\\')`: a fully-qualified call
                // (`\assert(...)`) keeps its leading backslash in
                // `ExprKind::Name`, unlike ordinary call resolution - the
                // same gap `apply_narrowing`'s own `ExprKind::Call` arm
                // already fixed for `is_string`/`is_array`/etc (see its
                // own comment). Confirmed by a real corpus false positive
                // this exact miss caused: amphp/http-client's
                // `FollowRedirects::removeDotSegments`, whose loop opens
                // every iteration with `\assert(\is_string($input));` -
                // written with the fully-qualified form throughout (a
                // common defensive-global-namespace style), so the
                // narrowing this whole block exists for never ran at all.
                //
                // Phase 55 follow-up: `assert($cond) && sideEffect();` (a
                // real, common idiom - the assertion doubling as a guard
                // clause for whatever follows) needs the identical
                // narrowing, not just a bare `assert($cond);` alone -
                // `bare_assert_condition`'s own doc comment has the
                // "which chain position is actually safe" reasoning.
                if let Some(cond) = bare_assert_condition(e) {
                    self.apply_narrowing(cond, locals, true);
                }
                // Phase 27: PHPUnit's own narrowing-relevant assertions
                // (`self::assertIsString($x)`, `$this->assertNotNull($x)`)
                // - the exact same "statement narrows unconditionally"
                // shape as `assert($cond)` above, just per-assertion-name
                // rather than a generic boolean condition.
                match &e.kind {
                    ExprKind::MethodCall(_, method, args, _)
                    | ExprKind::StaticCall(_, method, args) => {
                        self.apply_phpunit_assertion_narrowing(method, args, locals);
                    }
                    _ => {}
                }
                // A bare `@phpstan-assert Type $target` on this call's
                // own declaration - the same "narrows unconditionally,
                // no branch needed" shape `assert($cond)`/PHPUnit's own
                // assertions get just above, generalized via a real
                // `FunctionSig` lookup instead of a hardcoded name table
                // (see `FunctionSig::assert_if_true`'s own doc comment).
                match &e.kind {
                    ExprKind::MethodCall(recv, method, args, _) => {
                        // Real bug found and fixed this session: this used
                        // to require the receiver's own type to be
                        // literally `Type::Object`, silently skipping a
                        // `@phpstan-assert` narrowing opportunity for a
                        // `Union`/`Intersection` receiver.
                        let recv_ty = self.infer_expr(recv, locals);
                        if let Some((_, sig)) = self.find_method_owner(&recv_ty, method)
                            && !sig.assert.is_empty()
                        {
                            self.apply_assert_infos(&sig.assert, sig, Some(recv), args, locals);
                        }
                    }
                    ExprKind::StaticCall(class_name, method, args) => {
                        if let Some(class) = self.resolve_static_class_name(class_name)
                            && let Some(sig) = self.registry.lookup_method(&class, method)
                            && !sig.assert.is_empty()
                        {
                            self.apply_assert_infos(&sig.assert, sig, None, args, locals);
                        }
                    }
                    ExprKind::Call(callee, args) => {
                        if let ExprKind::Name(name) = &callee.kind {
                            let (primary, fallback) =
                                self.name_ctx.resolve_function_candidates(name);
                            if let Some(sig) =
                                self.registry.lookup_function(&primary).or_else(|| {
                                    fallback
                                        .as_deref()
                                        .and_then(|g| self.registry.lookup_function(g))
                                })
                                && !sig.assert.is_empty()
                            {
                                self.apply_assert_infos(&sig.assert, sig, None, args, locals);
                            }
                        }
                    }
                    _ => {}
                }
                // Phase 32 Finding K: a bare `$cond || neverReturningCall();`
                // statement - PHP's classic "guard or die" idiom
                // (`$pos !== false || $this->ierror('Missing .', 400);`).
                // `ierror(): never` is a real `never` return type; if
                // execution reaches the statement *after* this one, the
                // `||`'s right side didn't run, so (by short-circuit
                // evaluation) the left side was already true - the same
                // "statement narrows unconditionally" reasoning as
                // `assert($cond)` above, just for a condition spelled as
                // an `Or` instead of a direct call. `Or`-only, not `And` -
                // `A && B;` gives no equivalent guarantee (if `A` is
                // false, `B` never runs and the statement simply ends,
                // without proving `A` either way).
                if let ExprKind::Binary(BinOp::Or, lhs, rhs) = &e.kind
                    && self.infer_expr(rhs, locals) == Type::Never
                {
                    self.apply_narrowing(lhs, locals, true);
                }
            }
            StmtKind::Return(Some(e)) => {
                let actual = self.check_expr(e, locals, label);
                if let Some(expected) = declared_return {
                    let expected = match &self.current_class {
                        Some(c) => self.resolve_pseudo(expected, c),
                        None => expected.clone(),
                    };
                    // Phase 70.A follow-up: inside a real generator, a
                    // `return $x;` statement's own value sets `Generator::
                    // getReturn()` - it's checked against the declared
                    // `Generator<TKey, TValue, TSend, TReturn>`'s own
                    // TReturn, never against the whole `Generator<...>`
                    // type itself (that would always mismatch: no real
                    // `return` statement inside a generator body ever
                    // produces an actual `Generator` object). A pre-
                    // existing gap this project's own generator-exemption
                    // convention already correctly applies to the bare-
                    // `return;`/falls-off-the-end cases just below and in
                    // `check_function` - `Return(Some(e))` was the one
                    // case still missing it.
                    let expected = if self.current_function_is_generator {
                        generator_template_param(&expected, self.registry, "TReturn")
                    } else {
                        expected
                    };
                    // Phase 61: a `never`-typed function/method reaching
                    // *any* `return` at all - value or bare - is its own
                    // dedicated real-PHPStan identifier/message
                    // (`return.never`, "should never return but return
                    // statement found"), not an ordinary type mismatch -
                    // `never` promises the caller execution can't
                    // continue past this call, a stronger claim than
                    // "returns the wrong type." Checked before the
                    // generic path below, which would otherwise report
                    // this identically to any other type mismatch.
                    if expected == Type::Never {
                        self.report(
                            label.to_string(),
                            "should never return, but a return statement was reached".to_string(),
                            e.span,
                            3,
                            "return.never",
                        );
                    } else if expected == Type::Void && actual != Type::Void {
                        // Phase 61: same idea, one position over - a
                        // `void` function returning an actual value is
                        // real PHPStan's own dedicated `return.void`,
                        // not the generic mismatch path (`void` isn't a
                        // "wrong type" the way e.g. `string` vs `int`
                        // is - it's "no value was ever supposed to come
                        // back"). `actual != Type::Void` is a real,
                        // necessary exception, not a defensive
                        // afterthought: `return anotherVoidFn();`
                        // delegating to another void-returning call is
                        // legal, common PHP (real PHPStan doesn't flag
                        // it either - a `void` function returning
                        // *nothing new*, just handing back void-ness
                        // itself, isn't "returning a value" at all).
                        // Without this, every such delegation reported
                        // the nonsensical "expected void, got void" -
                        // a real corpus false positive this project's
                        // own registry fix for conditional-function-
                        // declaration arity tiebreaking (a top-10-bucket
                        // scoping round) unmasked by finally resolving
                        // `\RectorPrefix202608\trigger_deprecation(...)`
                        // to its own real, correctly-`void`-typed
                        // signature instead of leaving it unresolved.
                        self.report(
                            label.to_string(),
                            format!("return type mismatch: expected void, got {actual}"),
                            e.span,
                            3,
                            "return.void",
                        );
                    } else if actual != Type::Mixed
                        && !self.registry.is_assignable(&actual, &expected)
                    {
                        self.report(
                            label.to_string(),
                            format!("return type mismatch: expected {expected}, got {actual}"),
                            e.span,
                            3,
                            "return.type",
                        );
                    }
                }
            }
            // A bare `return;` always returns `null` in real PHP,
            // regardless of the function's declared return type - a
            // genuine mismatch inside a function declared to return a
            // concrete, non-nullable, non-`void` type (the same easy
            // slip as forgetting to add a return value after an
            // early-exit guard clause).
            StmtKind::Return(None) => {
                // A bare `return;` inside a generator (Finding 3) sets
                // Generator::getReturn()'s own value, not this function's
                // return value - real PHPStan never checks it against the
                // declared Generator/Iterator/Traversable return type
                // directly. Phase 61.B: it's still checked against the
                // declared TReturn specifically, same as `check_function`'s
                // own falls-off-the-end completeness check - a bare
                // `return;` sets `getReturn()` to `null`, a real mismatch
                // when TReturn requires a concrete, non-nullable type.
                let generator_treturn = self
                    .current_function_is_generator
                    .then(|| {
                        generator_missing_return_type(
                            self.current_declared_return_type.as_ref(),
                            self.registry,
                        )
                    })
                    .flatten();
                if let Some(expected) = declared_return
                    && (!self.current_function_is_generator || generator_treturn.is_some())
                {
                    let expected = if let Some(treturn) = generator_treturn {
                        treturn
                    } else {
                        match &self.current_class {
                            Some(c) => self.resolve_pseudo(expected, c),
                            None => expected.clone(),
                        }
                    };
                    // Phase 61: see the identical `return.never` special
                    // case on `Return(Some(e))` just above - a bare
                    // `return;` reaches this exact "never returns" claim
                    // just as surely as one with a value does.
                    if expected == Type::Never {
                        self.report(
                            label.to_string(),
                            "should never return, but a return statement was reached".to_string(),
                            stmt.span,
                            3,
                            "return.never",
                        );
                    } else if expected != Type::Void
                        && expected != Type::Mixed
                        && !self.registry.is_assignable(&Type::Null, &expected)
                    {
                        self.report(
                            label.to_string(),
                            format!("return type mismatch: expected {expected}, got null"),
                            stmt.span,
                            3,
                            "return.type",
                        );
                    }
                }
            }
            StmtKind::Echo(exprs) => {
                for (i, e) in exprs.iter().enumerate() {
                    let ty = self.check_expr(e, locals, label);
                    self.check_echo_validity(&ty, i, e.span, label);
                    // Phase 67/Phase 39 follow-up: `echo` implicitly
                    // invokes `__toString()` on an object argument - see
                    // `check_implicit_tostring_usage`'s own doc comment.
                    self.check_implicit_tostring_usage(&ty, e.span, label);
                }
            }
            StmtKind::Block(stmts) => self.check_block(stmts, locals, declared_return, label),
            StmtKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check_if(
                    cond,
                    then_branch,
                    else_branch,
                    locals,
                    declared_return,
                    label,
                    false,
                );
            }
            StmtKind::While { cond, body } => {
                self.check_while(cond, body, locals, declared_return, label)
            }
            StmtKind::DoWhile { body, cond } => {
                self.check_do_while(body, cond, locals, declared_return, label)
            }
            StmtKind::For {
                init,
                cond,
                step,
                body,
            } => self.check_for(init, cond, step, body, locals, declared_return, label),
            StmtKind::Foreach {
                subject,
                key,
                value,
                body,
                ..
            } => self.check_foreach(subject, key, value, body, locals, declared_return, label),
            StmtKind::Switch { subject, cases } => {
                self.check_switch(subject, cases, locals, declared_return, label)
            }
            StmtKind::Try {
                body,
                catches,
                finally,
            } => self.check_try(body, catches, finally, locals, declared_return, label),
            StmtKind::Throw(e) => self.check_throw(e, locals, label),
            // `global $x;` binds $x to whatever the global scope holds,
            // which this checker doesn't track - Mixed is the honest
            // answer, but the important part is that it's bound at all, so
            // using it afterward isn't flagged as undefined.
            StmtKind::Global(names) => {
                for name in names {
                    // Phase 65: `$this` is real PHP's own implicitly-
                    // bound reference, not a real global - "Cannot use
                    // $this as global variable." is a real,
                    // unconditional fatal error, confirmed directly.
                    if name == "this" {
                        self.report(
                            label.to_string(),
                            "cannot use $this as global variable".to_string(),
                            stmt.span,
                            0,
                            "global.this",
                        );
                        continue;
                    }
                    // Phase 17 follow-up: real, lexically-visible binding
                    // syntax - a `global $x;` declares `$x` here, even
                    // though this checker deliberately doesn't try to
                    // link it back to any specific top-level assignment
                    // (see this arm's own doc comment above) - `stmt.span`
                    // is as precise an answer as the type side already
                    // gives (`Mixed`, not a real inferred type either).
                    // `or_insert`'s own "only if not already bound"
                    // semantics carry over: `.declare` unconditionally
                    // overwrites, so it's only called when there's
                    // nothing here yet to overwrite.
                    if !locals.contains_key(name) {
                        self.probe_hit_at(stmt.span, &Type::Mixed);
                        locals.declare(name.clone(), Type::Mixed, stmt.span);
                    }
                }
            }
            StmtKind::StaticVar(vars) => {
                for (name, default) in vars {
                    // Phase 65: same idea as `global $this;` just above -
                    // "Cannot use $this as static variable.", confirmed
                    // directly.
                    if name == "this" {
                        self.report(
                            label.to_string(),
                            "cannot use $this as static variable".to_string(),
                            stmt.span,
                            0,
                            "static.this",
                        );
                        continue;
                    }
                    let ty = default
                        .as_ref()
                        .map(|e| self.check_expr(e, locals, label))
                        .unwrap_or(Type::Mixed);
                    purge_stale_narrowing_for(locals, name);
                    // Phase 17 follow-up: real declaration - no per-name
                    // span exists on `StaticVar` (only the whole
                    // statement's), same as `Global` just above.
                    self.probe_hit_at(stmt.span, &ty);
                    locals.declare(name.clone(), ty, stmt.span);
                    self.static_var_names.insert(name.clone());
                }
            }
            // `check_program`'s own top-level loop already handles a
            // top-level FunctionDecl/ClassDecl/EnumDecl by checking its
            // body - but any of the same three declared *nested* inside a
            // block (the extremely common `if (!function_exists('foo')) {
            // function foo() {} }` polyfill idiom, or a class conditionally
            // declared inside an if) used to fall into the wildcard arm
            // below and never got its own body checked at all - it was
            // indexed into the registry (so calls to it resolve), but
            // silently exempt from every diagnostic, undefined-variable
            // detection included. Mirrors check_program's handling.
            StmtKind::FunctionDecl(f) => self.check_function(f, None, false, true),
            StmtKind::ClassDecl(c) => {
                let fqn = self.name_ctx.declared_name(&c.name);
                self.check_attributes(&c.attrs, &fqn);
                self.check_attribute_targets(&c.attrs, ATTRIBUTE_TARGET_CLASS, "class", &fqn);
                if c.kind == ClassKind::Trait {
                    self.check_trait_is_not_an_attribute_class(&c.attrs, &fqn);
                }
                for p in &c.properties {
                    self.check_attributes(&p.attrs, &fqn);
                    self.check_attribute_targets(
                        &p.attrs,
                        ATTRIBUTE_TARGET_PROPERTY,
                        "property",
                        &fqn,
                    );
                }
                for cc in &c.consts {
                    self.check_attributes(&cc.attrs, &fqn);
                    self.check_attribute_targets(
                        &cc.attrs,
                        ATTRIBUTE_TARGET_CLASS_CONSTANT,
                        "class constant",
                        &fqn,
                    );
                }
                // A conditionally-declared trait (the common `if
                // (version_compare(...)) { trait Foo {...} }`/`if
                // (PHP_VERSION_ID ...) { trait Foo {...} }` shape - real
                // corpus example, symfony/cache's own `Redis63ProxyTrait`)
                // reaches this nested arm instead of `check_program`'s own
                // top-level one, which already knows to dispatch a trait
                // through `check_trait_methods` (Phase 13 Finding E) so
                // its `$this->hostOnlyMethod()` calls are checked against
                // a real using class, not standalone. This arm's own
                // unconditional per-method loop below skipped that
                // entirely, checking every trait method with `current_
                // class` set to the *trait's own* name regardless - a
                // real false positive (`$this->initializeLazyObject()`,
                // provided by a sibling trait on the real host `RedisProxy`,
                // not by `Redis63ProxyTrait` itself) invisible to a
                // top-level-only trait but real for one declared here.
                if c.kind == ClassKind::Trait {
                    self.check_trait_methods(&fqn, c);
                } else {
                    for m in &c.methods {
                        self.check_function(
                            &m.function,
                            Some(&fqn),
                            m.is_static,
                            !m.is_abstract && c.kind != ClassKind::Interface,
                        );
                    }
                }
            }
            StmtKind::EnumDecl(e) => {
                let fqn = self.name_ctx.declared_name(&e.name);
                self.check_attributes(&e.attrs, &fqn);
                self.check_attribute_targets(&e.attrs, ATTRIBUTE_TARGET_CLASS, "class", &fqn);
                for case in &e.cases {
                    self.check_attributes(&case.attrs, &fqn);
                    self.check_attribute_targets(
                        &case.attrs,
                        ATTRIBUTE_TARGET_CLASS_CONSTANT,
                        "class constant",
                        &fqn,
                    );
                }
                for cc in &e.consts {
                    self.check_attributes(&cc.attrs, &fqn);
                    self.check_attribute_targets(
                        &cc.attrs,
                        ATTRIBUTE_TARGET_CLASS_CONSTANT,
                        "class constant",
                        &fqn,
                    );
                }
                for m in &e.methods {
                    self.check_function(&m.function, Some(&fqn), m.is_static, !m.is_abstract);
                }
            }
            // Phase 29: a bare `break;` (`None`, level 1) or an explicit
            // `break N;` targets the Nth-innermost open loop/switch frame
            // - `checked_sub` naturally no-ops for an out-of-range level
            // (more levels than open loops/switches, invalid PHP anyway),
            // matching this project's "don't guess" default. The level is
            // already parse-time-resolved to a plain `i64`, no expression
            // evaluation needed.
            StmtKind::Break(level) => {
                let depth = level.unwrap_or(1).max(1) as usize;
                if let Some(idx) = self.loop_break_states.len().checked_sub(depth) {
                    self.loop_break_states[idx].push(locals.clone());
                }
            }
            // Phase 55: the `continue` counterpart of `Break` just above -
            // see `loop_continue_states`'s own doc comment on `Checker`
            // for why this exists (feeding a `continue`-reached state
            // into the *next* convergence pass, not just past-the-loop
            // exit state the way `break`'s states are used).
            StmtKind::Continue(level) => {
                let depth = level.unwrap_or(1).max(1) as usize;
                if let Some(idx) = self.loop_continue_states.len().checked_sub(depth) {
                    self.loop_continue_states[idx].push(locals.clone());
                }
            }
            // Phase 37: `ConstantAttributesRule` - a top-level (or nested,
            // conditionally-declared) `const` statement's own `#[Attr]`
            // group(s). Covers both the top-level program walk (which
            // already falls through to this same `check_stmt` catch-all
            // for anything it doesn't special-case) and any nested
            // occurrence - `GlobalConst` needs no class/FQN context the
            // way `ClassDecl`/`EnumDecl` do, so one arm here is enough.
            StmtKind::GlobalConst(consts) => {
                self.check_global_const_attributes(consts, stmt.span, label);
            }
            _ => {}
        }
        // Re-applied *after* dispatch too, but only when this exact
        // statement is itself a plain assignment to the same variable the
        // assertion names (the bare `/** @var Type */ $x = expr();` form,
        // or an explicitly-named `@var Type $x` sitting on its own
        // assignment) - the whole point of `@var` there is to override
        // whatever the assignment's own natural inference produces, which
        // `bind_assignment_target` (called from the `match` above, for
        // the `Assign` case) would otherwise silently clobber the
        // pre-applied assertion with. For every other statement shape
        // (an unrelated statement merely *reading* the variable), this
        // never matches - the single before-dispatch application above
        // already covers it, since the statement's own body needs to see
        // the narrowed type *during* its own check, not after.
        //
        // Deliberately does NOT extend this to a tag sitting directly on
        // a `foreach` header (`/** @var T $x */ foreach (...) { ... }`):
        // `bind_assignment_target` for the loop variable, and the loop
        // body's own check, both happen *inside* the `Foreach` arm above,
        // before this point ever runs - reapplying only here would fix
        // the type seen *after* the loop but leave the body itself seeing
        // the loop's own natural (usually `Mixed`) element type, a
        // half-working result worse than not supporting the shape at
        // all. The evidenced real-world case (`findCallSites`) instead
        // puts the tag on a statement *inside* the loop body, which the
        // before-dispatch application above already handles correctly
        // and completely on its own.
        if let Some((name, ty)) = inline_var
            && statement_defined_variable_names(&stmt.kind).contains(&name)
        {
            purge_stale_narrowing_for(locals, &name);
            locals.insert(name, ty);
        }
    }

    /// `StmtKind::If`'s own real body, extracted so a nested `elseif`
    /// (which the parser already desugars into a nested `If` inside
    /// `else_branch` - confirmed directly in `parser/stmt.rs::parse_if`:
    /// `self.tokens[self.pos] = Token::Keyword(Keyword::If); Some(Box::new
    /// (self.parse_if()?))`) can recurse into `check_if` itself
    /// (`is_elseif: true`) instead of the generic `check_stmt` dispatch,
    /// carrying its own chain-position context along - `check_stmt` never
    /// gains an `is_elseif` parameter, so its ~dozens of other call sites
    /// need no change at all.
    ///
    /// Phase 15 continuation, `IfConstantConditionRule`/
    /// `ElseIfConstantConditionRule`: the original type-inference-trusting
    /// design is reinstated (`inferred_constant_condition`, see its own
    /// doc comment) now that both real reasons it was reverted are fixed
    /// (Phase 55's convergence work; `widen_coalesce_result`'s own
    /// general-case bug, fixed this session). A genuine `elseif` gets its
    /// own real identifier/message (not `if.*`) and real PHPStan's own
    /// "last condition, no trailing else" exemption for `alwaysTrue`
    /// only, confirmed against `vendor/phpstan-src`'s own
    /// `reportAlwaysTrueInLastCondition: false` default - a trailing
    /// `elseif (true)` with nothing after it is a real, common "acts as
    /// an implicit else" idiom; `alwaysFalse` is never exempted this way,
    /// dead code is dead code regardless of position. A bare, non-elseif
    /// `If` has no such chain-position concept at all and is never
    /// exempted.
    #[allow(clippy::too_many_arguments)]
    fn check_if(
        &mut self,
        cond: &Expr,
        then_branch: &Stmt,
        else_branch: &Option<Box<Stmt>>,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
        is_elseif: bool,
    ) {
        let is_last_without_else = else_branch.is_none();
        self.in_condition_position = true;
        self.in_exempt_last_condition = is_elseif && is_last_without_else;
        self.check_expr(cond, locals, label);
        if let Some(value) = self.inferred_constant_condition(cond, locals) {
            if is_elseif {
                if !(value && is_last_without_else) {
                    self.report(
                        label.to_string(),
                        format!(
                            "Elseif condition is always {}.",
                            if value { "true" } else { "false" }
                        ),
                        cond.span,
                        4,
                        if value {
                            "elseif.alwaysTrue"
                        } else {
                            "elseif.alwaysFalse"
                        },
                    );
                }
            } else {
                self.report(
                    label.to_string(),
                    format!(
                        "If condition is always {}.",
                        if value { "true" } else { "false" }
                    ),
                    cond.span,
                    4,
                    if value {
                        "if.alwaysTrue"
                    } else {
                        "if.alwaysFalse"
                    },
                );
            }
        }

        let mut then_locals = locals.clone();
        self.apply_narrowing(cond, &mut then_locals, true);
        self.check_stmt(then_branch, &mut then_locals, declared_return, label);

        let mut else_locals = locals.clone();
        self.apply_narrowing(cond, &mut else_locals, false);
        if let Some(e) = else_branch {
            if let StmtKind::If {
                cond: nested_cond,
                then_branch: nested_then,
                else_branch: nested_else,
            } = &e.kind
            {
                self.check_if(
                    nested_cond,
                    nested_then,
                    nested_else,
                    &mut else_locals,
                    declared_return,
                    label,
                    true,
                );
            } else {
                self.check_stmt(e, &mut else_locals, declared_return, label);
            }
        }

        // The classic guard clause - `if ($x === null) { return; }` -
        // depends on this: a branch that unconditionally
        // returns/throws/breaks/continues never reaches the merge point,
        // so only the surviving branch's (narrowed) scope should
        // propagate past the if. Without this, the terminated branch's
        // narrowing (here, "$x is null") would incorrectly leak into the
        // code after the guard.
        let then_terminates = self.always_terminates(then_branch, &then_locals);
        let else_terminates = else_branch
            .as_deref()
            .is_some_and(|e| self.always_terminates(e, &else_locals));

        *locals = match (then_terminates, else_terminates) {
            (true, false) => else_locals,
            (false, true) => then_locals,
            _ => merge_scopes(&then_locals, &else_locals),
        };
    }

    /// Recognizes an inline `/** @var Type $name */` (or bare `/** @var
    /// Type */`) doc comment sitting on `stmt` itself, and resolves it to
    /// the `(name, type)` assertion it makes - `None` for a statement with
    /// no doc comment, one with no `@var` tag, or a bare tag whose target
    /// can't be inferred (see `statement_defined_variable_names`'s own
    /// "don't guess" scope).
    fn inline_var_annotation(&self, stmt: &Stmt) -> Option<(String, Type)> {
        let doc = stmt.doc_comment.as_deref()?;
        // A cheap substring pre-filter - `check_stmt` is already
        // `#[hotpath::measure]`-instrumented, confirming this is a real
        // hot path worth avoiding a full docblock parse on every
        // statement's own `@param`/`@return`-only declaration comment.
        if !doc.contains("@var") {
            return None;
        }
        let block = docblock::parse(doc);
        let ty_str = block.var_type?;
        // A *named* tag (`@var Type $name`) always wins when present. The
        // bare form (`@var Type`, no name) applies to whatever single
        // variable the statement it precedes assigns, inferred from that
        // statement's own left-hand side, not guessed at from thin air.
        let name = block.var_name.or_else(|| {
            match statement_defined_variable_names(&stmt.kind).as_slice() {
                [single] => Some(single.clone()),
                _ => None,
            }
        })?;
        let ty = self.resolve_type_hint(Some(&ty_str));
        // `resolve_type_hint` alone has no idea this text came from a
        // docblock rather than a native hint, so it can't tell "Options"
        // apart from a genuine, unresolvable class reference - a
        // `@phpstan-type` alias declared on the current class needs the
        // same expansion here that `ClassInfo::from_decl` already gives
        // every member's own `@param`/`@return`/`@var` (real corpus
        // shape: `league/uri-interfaces`'s own `UriString::parse()`,
        // `/** @var ComponentMap $components */`). Silently a no-op
        // outside any class, or one with no `@phpstan-type` tags at all.
        let ty = match &self.current_class {
            Some(class) => match self.registry.classes.get(class) {
                Some(info) if !info.type_aliases.is_empty() => {
                    crate::registry::substitute_type_aliases(ty, &info.type_aliases)
                }
                _ => ty,
            },
            None => ty,
        };
        Some((name, ty))
    }

    /// `@var Type $name` / bare `@var Type`'s own error-reporting half -
    /// real PHPStan's `VarTagTypeRuleHelper`/`VarTagChangedExpressionTypeRule`,
    /// scoped down to the single highest-value, most common real shape: a
    /// plain assignment statement (`/** @var Type */ $x = expr;`) whose
    /// own RHS's naturally-inferred type the declared tag can't possibly
    /// be describing (not even a supertype of it) - a real, catchable
    /// mistake (a typo'd type, a stale tag after a refactor). Real
    /// PHPStan's own version covers substantially more (list/array
    /// destructuring targets, a native-type-vs-docblock-type split with
    /// its own separate identifier and config toggle, a
    /// `containsPhpStanType`-specific "this exact type, don't widen"
    /// assumption check) - deliberately not replicated, a disclosed scope
    /// trim matching this project's own simpler type system (no native/
    /// docblock type split at this granularity) and "favor false
    /// negatives" precedent throughout. `Type::Mixed` on either side is
    /// never flagged - it means "no type declared at all" as often as
    /// real `mixed` in this project's own overloaded convention (see
    /// this file's own module doc comment), so a mismatch there is
    /// usually not a real one.
    fn check_inline_var_annotation_type(
        &mut self,
        stmt: &Stmt,
        name: &str,
        declared_ty: &Type,
        locals: &Locals,
        label: &str,
    ) {
        let StmtKind::Expression(e) = &stmt.kind else {
            return;
        };
        let ExprKind::Assign(lhs, rhs) = &e.kind else {
            return;
        };
        let ExprKind::Variable(lhs_name) = &lhs.kind else {
            return;
        };
        if lhs_name != name {
            return;
        }
        if matches!(declared_ty, Type::Mixed) {
            return;
        }
        let rhs_ty = self.infer_expr(rhs, locals);
        if matches!(rhs_ty, Type::Mixed) {
            return;
        }
        // Real corpus find: `is_assignable(&rhs_ty, declared_ty)` alone
        // flags the single most common real reason anyone writes an
        // inline `@var` in the first place - *refining* a wide, less-
        // precise inferred type (Doctrine's own bare `array`-returning
        // `getArrayResult()`, real Piwigo shape) into a narrower,
        // perfectly compatible one (`list<array<string, mixed>>`) - a
        // list is unconditionally still an array, not a genuine
        // mismatch. `declared_ty` only ever needs to be *some* plausible
        // relative of `rhs_ty` - a subtype (the refining case above) or
        // a supertype (widening, rarer but equally legitimate) - so both
        // directions have to fail before this is a real, catchable
        // mistake (a typo'd type, a stale tag after a refactor), matching
        // this function's own doc comment's "not even a supertype of it"
        // phrasing exactly (the code just didn't implement it yet).
        if !self.registry.is_assignable(&rhs_ty, declared_ty)
            && !self.registry.is_assignable(declared_ty, &rhs_ty)
        {
            self.report(
                label.to_string(),
                format!(
                    "PHPDoc tag @var with type {declared_ty} is not compatible with the assigned expression's own type {rhs_ty}."
                ),
                lhs.span,
                2,
                "varTag.type",
            );
        }
    }
}

/// Every bare-variable name `kind` itself unconditionally (re)binds via a
/// plain `$x = ...;` expression statement's own LHS. Used two ways:
/// inferring a *nameless* `@var Type` tag's own target (only when there's
/// exactly one candidate), and deciding whether an inline `@var` assertion
/// (named or not) needs re-applying *after* this statement's own dispatch,
/// because the assignment's own natural binding would otherwise silently
/// overwrite it first. Deliberately doesn't cover `foreach` - see the
/// comment in `check_stmt` on why a tag on a `foreach` header isn't
/// supported at all, rather than half-supported.
/// The condition to narrow for, if `e` is a bare `assert(...)` call
/// (`\assert` included), or a top-level `&&`-chain whose *leftmost*
/// operand is one - `None` otherwise. Only the leftmost position is
/// safe: `&&` evaluates left to right and short-circuits on the first
/// falsy operand, so the leftmost operand is unconditionally evaluated
/// the moment this whole statement is reached at all, and (being an
/// `assert()` call) throws rather than let execution continue if its
/// own condition were false - meaning reaching *past* this statement
/// proves it held, exactly like the bare, whole-statement case. Any
/// operand *after* the first has no such guarantee (if an earlier
/// operand was falsy, `&&` short-circuits before ever reaching it at
/// all, and the statement still completes normally either way) - not
/// walked past the leftmost position, even for a real, common `assert(
/// $a) && assert($b);` double-assertion, a real, disclosed scope trim
/// rather than a wrong answer.
fn bare_assert_condition(e: &Expr) -> Option<&Expr> {
    fn as_bare_assert_call(e: &Expr) -> Option<&Expr> {
        let ExprKind::Call(callee, args) = &e.kind else {
            return None;
        };
        let ExprKind::Name(name) = &callee.kind else {
            return None;
        };
        if name.trim_start_matches('\\') != "assert" {
            return None;
        }
        args.first().map(|a| &a.value)
    }
    match &e.kind {
        ExprKind::Binary(BinOp::And, l, _) => as_bare_assert_call(l),
        _ => as_bare_assert_call(e),
    }
}

fn statement_defined_variable_names(kind: &StmtKind) -> Vec<String> {
    match kind {
        StmtKind::Expression(e) => match &e.kind {
            ExprKind::Assign(lhs, _) => match &lhs.kind {
                ExprKind::Variable(name) => vec![name.clone()],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        },
        // Phase 60 corpus find: `StmtKind::StaticVar`'s own arm above
        // infers purely from the declaration's default expression
        // (`static $x = null;` types `$x` as exactly `Null`), unlike
        // every other statement shape here, which unconditionally
        // clobbered an already-applied `/** @var string|null */`
        // annotation instead of merely leaving it alone - real PHPStan's
        // own `@var` on a `static` declaration is exactly the same
        // "trust the annotation over the initializer" override every
        // other assignment already gets. Listed here so both existing
        // mechanisms (the nameless-tag target inference and the
        // post-dispatch reapplication) pick a `static` declaration up
        // for free, with no new plumbing of their own.
        StmtKind::StaticVar(vars) => vars.iter().map(|(name, _)| name.clone()).collect(),
        _ => Vec::new(),
    }
}
