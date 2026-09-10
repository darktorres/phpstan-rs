use crate::ast::*;
use crate::checker::{
    Checker, Locals, body_contains_yield, is_truthy_literal, merge_scopes, stmt_contains_break,
    stmt_contains_escaping_exit, union_types,
};
use crate::types::Type;

impl<'a> Checker<'a> {
    pub(crate) fn check_while(
        &mut self,
        cond: &Expr,
        body: &Stmt,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        // Phase 15 continuation: real bug found and fixed this
        // session - the condition's own constant-condition check
        // (and, transitively, any standalone check reached while
        // checking it, like `BooleanNot`/`BooleanAnd`/`BooleanOr`)
        // used to run exactly once, against the pre-loop entry
        // `locals`, *before* `converge_loop_body` ever ran -
        // completely bypassing Phase 55's own fixed-point
        // convergence. Real corpus false positive this produced:
        // `$end = false; while (!$end) { ...; $end = true; ... }`
        // (`Piwigo\Admin\Extensions\CoreUpdateService::upgradeTo`)
        // - `$end`'s pre-loop type is exactly `false`, so `!$end`
        // looked unconditionally `true` even though the loop body
        // can reassign it. Confirmed directly against real
        // PHPStan's own `WhileHandler::processStmt`: it re-
        // evaluates the condition at the top of *every*
        // convergence pass (discarding all but the last with a
        // no-op node callback), and only the FINAL, fully-
        // converged pass's walk of the condition ever reaches the
        // real rule-invoking callback. Moving the condition check
        // inside `converge_loop_body`'s own closure reproduces
        // that exactly for free: every pass but the kept one
        // already has its diagnostics discarded via
        // `converge_loop_body`'s own `diagnostics.truncate`.
        let skip_always_true = stmt_contains_break(body)
            || stmt_contains_escaping_exit(body)
            || body_contains_yield(std::slice::from_ref(body))
            || declared_return == Some(&Type::Never);
        // Real bug found and fixed this session, on top of the
        // one above: moving the diagnostic-bearing condition
        // check into the convergence closure means it now only
        // ever runs against per-pass *copies* of the entry
        // (`pass_locals`), never mutating the real `locals`
        // directly the way a single up-front call used to. A
        // real PHP `while`'s own condition always runs at least
        // once, unconditionally, regardless of how many times the
        // body itself iterates - so a variable *the condition
        // itself* assigns (`while (($x = f()) !== null) {...}`,
        // a real, common idiom) must be treated as unconditionally
        // defined afterward, unlike anything only the *body*
        // conditionally defines. Without this, `merge_scopes`
        // dropped it entirely (present in the loop's own exit
        // state but never in the real pre-loop `locals` it gets
        // merged against). Confirmed directly against real
        // PHPStan's own `WhileHandler::processStmt`: it also does
        // a quiet, non-diagnostic evaluation of the condition
        // against the real pre-loop scope first (its own
        // `$condResult`/`$condScope`, built with a `NoopNodeCallback`),
        // strictly separate from the later diagnostic-bearing
        // walk against the converged state - replicated here by
        // running `check_expr` once for real (to bind any
        // assignment side effect into `locals`) and discarding
        // whatever it reported, since the real diagnostics come
        // from the convergence-aware walk below. Real corpus
        // false positive this fixes: `Symfony\Component\
        // DependencyInjection\Compiler\AbstractRecursivePass::
        // getConstructor`'s `while ((null === $class = $definition
        // ->getClass()) && $definition instanceof ChildDefinition)`.
        let diagnostics_before = self.diagnostics.len();
        self.check_expr(cond, locals, label);
        self.diagnostics.truncate(diagnostics_before);
        let entry = locals.clone();
        self.loop_depth += 1;
        let (body_locals, break_states) = self.converge_loop_body(
            &entry,
            declared_return,
            label,
            |this, pass_locals, declared_return, label, _pass| {
                this.in_condition_position = true;
                this.check_expr(cond, pass_locals, label);
                // Phase 15, `WhileLoopAlwaysFalseConditionRule`/
                // `WhileLoopAlwaysTrueConditionRule`: real
                // inferred-type trust (`inferred_constant_
                // condition`), matching real PHPStan's own design
                // now that both reasons the original version was
                // reverted are fixed - see `check_if`'s own doc
                // comment. `alwaysFalse` has no exclusions,
                // matching real PHPStan's own rule exactly.
                // `alwaysTrue` is gated the same way real
                // PHPStan's own `WhileLoopAlwaysTrueConditionRule`
                // is (confirmed directly against `vendor/
                // phpstan-src`): skipped when the loop is
                // breakless (`stmt_contains_break` - this
                // project's own already-existing, deliberately
                // conservative "any break anywhere" check, reused
                // rather than re-deriving real PHPStan's own more
                // precise per-level break/`continue N` analysis -
                // a narrow, disclosed simplification, see
                // ROADMAP), when the body yields (a legitimate
                // infinite generator, not dead code), or when the
                // enclosing function is declared `: never`
                // (already documents "this never returns" on its
                // own, redundant to also flag the loop).
                if this.inferred_constant_condition(cond, pass_locals) == Some(false) {
                    this.report(
                        label.to_string(),
                        "While loop condition is always false.".to_string(),
                        cond.span,
                        4,
                        "while.alwaysFalse",
                    );
                } else if this.inferred_constant_condition(cond, pass_locals) == Some(true)
                    && !skip_always_true
                {
                    this.report(
                        label.to_string(),
                        "While loop condition is always true.".to_string(),
                        cond.span,
                        4,
                        "while.alwaysTrue",
                    );
                }
                this.apply_narrowing(cond, pass_locals, true);
                this.check_stmt(body, pass_locals, declared_return, label);
            },
        );
        self.loop_depth -= 1;
        if is_truthy_literal(cond) {
            // The condition can never itself become false (a
            // literal `while (true)`/`while (1)`), so the *only*
            // way to reach code after this loop at all is via a
            // `break` somewhere in the body - neither the "may
            // run zero times" merge below nor the "reaching here
            // means cond went false" narrowing after it applies:
            // this loop always runs at least once, and never
            // exits any other way. `is_truthy_literal` is the
            // same check `always_terminates`'s own `While` arm
            // already uses for the identical reasoning, one level
            // up (a break-less `while (true) {...}` never lets
            // control reach past it at all). Real corpus false
            // positive this fixes: `while (true) { $x = ...;
            // break; } return $x;` - the old unconditional
            // zero-iteration merge below discarded `$x` from
            // every representative state that reached the
            // `break`, leaving it looking undefined afterward.
            //
            // Folds every `break`-reached state together, seeded
            // by the first rather than the (semantically
            // meaningless here) pre-loop `locals` - with none at
            // all, `locals` is left as its own pre-loop state, a
            // safe fallback for what's genuinely unreachable code
            // anyway (an infinite loop with no escape).
            let mut states = break_states.into_iter();
            if let Some(first) = states.next() {
                let mut merged = first;
                for state in states {
                    merged = merge_scopes(&merged, &state);
                }
                *locals = merged;
            }
        } else {
            // The loop may run zero times, so what's true after it
            // is "either the pre-loop state, or whatever one
            // narrowed iteration produces" - not a real fixed
            // point, but sound enough to avoid claiming more
            // precision than we have.
            *locals = merge_scopes(locals, &body_locals);
            // Reaching code after the loop *without having broken
            // out of it* always means `cond` just evaluated
            // false, regardless of how many iterations ran - real,
            // additional narrowing on top of the merge above,
            // independent of it (e.g. a "loop until no longer
            // null" idiom is fully narrowed non-null afterward).
            // Applied *before* folding in break-reached states
            // below - a `break` doesn't mean `cond` went false, so
            // those states must not be narrowed as if it had.
            self.apply_narrowing(cond, locals, false);
            // Phase 29: fold in every `break`-reached state too -
            // see `loop_break_states`'s own doc comment on
            // `Checker`.
            for state in break_states {
                *locals = merge_scopes(locals, &state);
            }
        }
    }

    pub(crate) fn check_do_while(
        &mut self,
        body: &Stmt,
        cond: &Expr,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        // The first pass has nothing to narrow going in - the
        // condition is only evaluated after the body has already
        // run once. Every *re*-convergence pass (`pass > 0`) is
        // different: reaching it at all means the previous pass's
        // own trailing `cond` evaluated true (that's the only way
        // a do-while body runs again), so the "continue" half of
        // `cond`'s own narrowing is a real, provable fact at that
        // pass's entry - the same narrowing a `while` loop's own
        // convergence closure already applies unconditionally on
        // every pass (its condition sits at the *start* of the
        // body instead), just shifted here to skip pass 0.
        //
        // Real corpus bug this fixes (`Latte\Essential\
        // CachingIterator::__construct`'s `do { $iterator =
        // $iterator->getIterator(); } while (!$iterator instanceof
        // \Iterator);`): without this, pass 2's own entry widens
        // `$iterator` all the way back to the un-narrowed
        // `Traversable` (the union of pass 1's `IteratorAggregate`
        // entry and its own post-body `Traversable` exit), and
        // `$iterator->getIterator()` isn't a real `Traversable`
        // method - a false `method.notFound`. Applying `!$iterator
        // instanceof \Iterator`'s own true-branch narrowing here
        // subtracts `Iterator` back out first (`narrow_type_
        // excluding`'s existing `Traversable`-minus-`Iterator`
        // special case), correctly re-narrowing to
        // `IteratorAggregate` instead.
        let entry = locals.clone();
        self.loop_depth += 1;
        let (body_locals, break_states) = self.converge_loop_body(
            &entry,
            declared_return,
            label,
            |this, pass_locals, declared_return, label, pass| {
                if pass > 0 {
                    this.apply_narrowing(cond, pass_locals, true);
                }
                this.check_stmt(body, pass_locals, declared_return, label);
            },
        );
        // A do-while's body always runs at least once - the
        // converged result replaces `locals` directly, the same
        // "no zero-iterations merge" choice this arm already made.
        *locals = body_locals;
        self.in_condition_position = true;
        // `loop_depth` deliberately stays incremented one
        // statement longer than While's own arm needs it to -
        // real corpus false positive this fixes (Symfony's own
        // `LoggerTest`'s `do { ...; $attempts++; } while ((...) &&
        // $attempts < 150);`): this trailing check of the loop's
        // own condition is *itself* still evaluating a loop-
        // carried variable's post-convergence type, exactly as
        // approximate as any pass inside the loop - `check_
        // constant_numeric_comparison`/`check_constant_loose_
        // comparison`'s own `self.loop_depth > 0` guard (`checker/
        // expr.rs`) needs to see it still incremented here, or
        // `$attempts`'s own convergence-bounded range (this
        // project's 3-pass approximation reaches nowhere near the
        // real 150) makes `$attempts < 150` look unconditionally,
        // wrongly provable.
        self.check_expr(cond, locals, label);
        self.loop_depth -= 1;
        // Phase 15, `DoWhileLoopConstantConditionRule`: real
        // inferred-type trust, same as `If`/`While` above. Both
        // directions are gated on breakless-ness here (confirmed
        // directly against real PHPStan's own rule): `alwaysFalse`
        // skips when the body has a `break` (`do { ...; if ($x)
        // break; ... } while (false);` is a real, common
        // "structured goto" idiom, not dead code); `alwaysTrue`
        // skips on the same `stmt_contains_break` check plus a
        // yield check (no `declared_return: never` exclusion here
        // - confirmed real PHPStan's own rule doesn't have one,
        // unlike `While`'s).
        if self.inferred_constant_condition(cond, locals) == Some(false) {
            if !stmt_contains_break(body) {
                self.report(
                    label.to_string(),
                    "Do-while loop condition is always false.".to_string(),
                    cond.span,
                    4,
                    "doWhile.alwaysFalse",
                );
            }
        } else if self.inferred_constant_condition(cond, locals) == Some(true)
            && !stmt_contains_break(body)
            && !stmt_contains_escaping_exit(body)
            && !body_contains_yield(std::slice::from_ref(body))
        {
            self.report(
                label.to_string(),
                "Do-while loop condition is always true.".to_string(),
                cond.span,
                4,
                "doWhile.alwaysTrue",
            );
        }
        // Same as `While` - reaching code after a do-while always
        // means `cond` just evaluated false. Applied *before*
        // folding in break-reached states below, same reasoning
        // as `While` - a `break` doesn't mean `cond` went false.
        self.apply_narrowing(cond, locals, false);
        // Phase 29: fold in every `break`-reached state too - see
        // `loop_break_states`'s own doc comment on `Checker`.
        for state in break_states {
            *locals = merge_scopes(locals, &state);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_for(
        &mut self,
        init: &[Expr],
        cond: &[Expr],
        step: &[Expr],
        body: &Stmt,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        for e in init.iter() {
            self.check_expr(e, locals, label);
        }
        // A real `for`'s condition slot can hold several
        // comma-separated expressions, *all* of which real PHP
        // re-evaluates on every single pass through the condition
        // (only the *last* one's truthiness actually gates the
        // loop - the others are there purely for a side effect,
        // but that side effect happens every iteration, not just
        // once). Both halves - the non-last expressions here, and
        // the last one's own check moved into the convergence
        // closure below - need the identical Phase 15 continuation
        // fix (the same real bug this session's own `While` fix
        // addresses: checking any of them once, up front, against
        // the pre-loop entry, would bypass Phase 55's fixed-point
        // convergence entirely, the exact false positive real
        // corpus code hit - `QMultiToken::parseExpression`'s
        // `for ($stop = false; !$stop && ...; $qi++)`, where
        // `$stop` is reassigned inside the body).
        match cond.last() {
            Some(c) => {
                // Same real bug and fix as `While`'s own arm
                // above: a variable *any* condition expression
                // itself assigns (`for (;; ($x = f()) !== null; )`,
                // unusual but real) must be unconditionally
                // defined afterward, since a `for`'s condition
                // always runs at least once regardless of body
                // iterations - a quiet, diagnostics-discarded
                // evaluation against the real pre-loop `locals`
                // binds every condition expression's own side
                // effect (not just the last one's) before the
                // convergence-aware, diagnostic-bearing walk runs
                // against per-pass copies.
                let diagnostics_before = self.diagnostics.len();
                for e in cond.iter() {
                    self.check_expr(e, locals, label);
                }
                self.diagnostics.truncate(diagnostics_before);
                let entry = locals.clone();
                self.loop_depth += 1;
                let (body_locals, break_states) = self.converge_loop_body(
                    &entry,
                    declared_return,
                    label,
                    |this, pass_locals, declared_return, label, _pass| {
                        for e in &cond[..cond.len() - 1] {
                            this.check_expr(e, pass_locals, label);
                        }
                        this.check_expr(c, pass_locals, label);
                        this.apply_narrowing(c, pass_locals, true);
                        this.check_stmt(body, pass_locals, declared_return, label);
                        // `step` runs after each body iteration, so
                        // it should see whatever the body just did,
                        // not the pre-loop state - and it runs on
                        // every convergence pass too, since a
                        // variable `step` mutates could itself need
                        // more than one pass to converge.
                        for e in step {
                            this.check_expr(e, pass_locals, label);
                        }
                    },
                );
                self.loop_depth -= 1;
                // Same "may run zero times" merge `While` uses,
                // for the same reason - unlike before this fix,
                // this is no longer treated as if the body always
                // executes exactly once.
                *locals = merge_scopes(locals, &body_locals);
                // Applied *before* folding in break-reached states
                // below, same reasoning as `While` - a `break`
                // doesn't mean `cond` went false.
                self.apply_narrowing(c, locals, false);
                // Phase 29: fold in every `break`-reached state
                // too - see `loop_break_states`'s own doc comment
                // on `Checker`.
                for state in break_states {
                    *locals = merge_scopes(locals, &state);
                }
            }
            None => {
                // `for (;;)` - no condition means an unconditional
                // infinite loop (barring an internal break/return/
                // etc.), so the body genuinely always executes at
                // least once - no "may run zero times" merge is
                // needed or correct here. The only way code after
                // this loop is ever reached at all is via a
                // `break` somewhere inside it (Phase 29) - folded
                // in additively on top of the direct body-check,
                // rather than trying to model "unreachable if no
                // break exists" precisely.
                let entry = locals.clone();
                self.loop_depth += 1;
                let (body_locals, break_states) = self.converge_loop_body(
                    &entry,
                    declared_return,
                    label,
                    |this, pass_locals, declared_return, label, _pass| {
                        this.check_stmt(body, pass_locals, declared_return, label);
                        for e in step {
                            this.check_expr(e, pass_locals, label);
                        }
                    },
                );
                self.loop_depth -= 1;
                *locals = body_locals;
                for state in break_states {
                    *locals = merge_scopes(locals, &state);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    /// Phase 70.C: `array_element_type`/`array_key_type`'s own parallel
    /// case for an object-shaped `foreach` subject - `Traversable`,
    /// `Iterator`, `Generator` (a real `Iterator` descendant), or
    /// `IteratorAggregate`. Returns `(key type, value type)`.
    /// Deliberately not folded into `array_element_type`/`array_key_type`
    /// themselves (both plain free functions, no registry access) -
    /// every other caller of those two genuinely never has an object
    /// subject, and this needs `&self.registry` throughout.
    ///
    /// Targets `Iterator`/`IteratorAggregate` directly rather than the
    /// plan's originally-suggested common ancestor `Traversable` - a
    /// real, verified reason, not a stylistic choice: `Traversable`'s own
    /// phpstorm-stubs declaration (`vendor/phpstorm-stubs/Core/Core_c.php`)
    /// has no real `@template` tag of its own, only a `@template-extends
    /// iterable<TKey, TValue>` referencing the `iterable` *pseudo*-type
    /// (which `types::parse_type_string` desugars straight to `array|
    /// Traversable` before any template ever attaches) - `Registry::
    /// class_template_bindings(_, _, "Traversable")` would always return
    /// an empty map, no matter what's passed in, since `ClassInfo::
    /// templates` for `Traversable` itself is empty. `Iterator`/
    /// `IteratorAggregate` both carry real, functional `@template TKey`/
    /// `@template-covariant TValue` tags of their own, and every real,
    /// concrete `Traversable` implementor is required by the PHP engine
    /// itself to implement one of the two (`Traversable` can't be
    /// implemented directly - a real fatal error) - so this covers every
    /// concrete case; a variable statically typed as the bare `Traversable`
    /// interface itself (or a raw `Traversable<K,V>` docblock reference,
    /// never actually widened to a real implementor) is the one
    /// remaining, narrow gap, and falls back to `None`/`Mixed` like any
    /// other unresolvable case in this project.
    ///
    /// `IteratorAggregate` resolves through its own `getIterator()`'s
    /// declared return type first (real, common shape: `getIterator():
    /// Iterator<K, V>` rather than the class itself declaring `@template-
    /// implements Traversable<K,V>`) - only falling back to `Iterator
    /// Aggregate`'s own template bindings directly when that return type
    /// doesn't resolve any further (e.g. a bare, non-generic `Iterator`/
    /// `Traversable` return hint) - a small, deliberate leniency beyond
    /// what real PHPStan's own method-return-type-only resolution does,
    /// since the interface's own declared generics are still a real,
    /// likely-correct answer in that case.
    fn iterable_element_type(&self, ty: &Type) -> Option<(Type, Type)> {
        match ty {
            Type::Object(name, args) => {
                if self.registry.is_subclass_of(name, "IteratorAggregate") {
                    let agg_bindings =
                        self.registry
                            .class_template_bindings(name, args, "IteratorAggregate");
                    if let Some(sig) = self.registry.lookup_method(name, "getIterator") {
                        let declared = self.resolve_pseudo(&sig.return_type, name);
                        // Avoid infinite recursion on a (real but
                        // pathological) `getIterator(): static`-shaped
                        // declaration that resolved right back to this
                        // same class.
                        let recurses_into_self =
                            matches!(&declared, Type::Object(inner, _) if inner == name);
                        if !recurses_into_self
                            && let Some(result) = self.iterable_element_type(&declared)
                        {
                            return Some(result);
                        }
                    }
                    if !agg_bindings.is_empty() {
                        return Some((
                            agg_bindings.get("TKey").cloned().unwrap_or(Type::Mixed),
                            agg_bindings.get("TValue").cloned().unwrap_or(Type::Mixed),
                        ));
                    }
                }
                let bindings = self
                    .registry
                    .class_template_bindings(name, args, "Iterator");
                if bindings.is_empty() {
                    return None;
                }
                Some((
                    bindings.get("TKey").cloned().unwrap_or(Type::Mixed),
                    bindings.get("TValue").cloned().unwrap_or(Type::Mixed),
                ))
            }
            Type::Union(members) => {
                let mut keys = Vec::new();
                let mut values = Vec::new();
                for m in members {
                    let (k, v) = self.iterable_element_type(m)?;
                    keys.push(k);
                    values.push(v);
                }
                Some((
                    keys.into_iter().reduce(union_types).unwrap_or(Type::Mixed),
                    values
                        .into_iter()
                        .reduce(union_types)
                        .unwrap_or(Type::Mixed),
                ))
            }
            _ => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_foreach(
        &mut self,
        subject: &Expr,
        key: &Option<Expr>,
        value: &Expr,
        body: &Stmt,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        let subject_ty = self.check_expr(subject, locals, label);
        // The loop variable(s) are a definition site, not a read -
        // bound (to the subject's known element type, when it's a
        // homogeneous array) before checking anything that could
        // reference them, at the start of every convergence pass
        // below (see that closure's own comment for why every
        // pass, not just once).
        //
        // Phase 12 (unplanned follow-on): a `Type::ArrayShape`
        // subject used to fall straight to the `Mixed` catch-all,
        // discarding real, known field types - a real corpus false
        // positive this surfaced: `MaintenanceActionDispatcher::
        // dispatch`'s `foreach ($versions as $key => $value) {
        // $versions[$key] = preg_replace(..., $value); }`, where
        // every field of `$versions` is actually a plain `string`.
        // With `$value` inferred as `Mixed`, `preg_replace`'s own
        // stub (which returns a broader type for a
        // statically-unknown subject) leaked an `array<string>`
        // possibility into `$versions`'s own re-widened element
        // type that stuck around once Finding E's `Union`-awareness
        // stopped silently collapsing it. Joining every field's own
        // type (the same "give the reader every field's type is at
        // least possible" idea `write_index_into`'s own dynamic-key
        // write branch already uses) is real, if narrow, precision:
        // exact when every field shares one type (the common case
        // this false positive came from), still sound but coarser
        // when they don't.
        // Phase 14: `Self::array_element_type` also sees through
        // an `Intersection`-wrapped `list<T>`/`non-empty-
        // array<T>` - without it, `foreach` over one of those
        // silently inferred `Mixed` for the loop variable, a
        // real, confirmed regression (one of the most common
        // constructs in real PHP).
        // Phase 70.C: an array-shaped subject keeps taking the pre-
        // existing, array-specific path unchanged; anything that path
        // can't handle (an object) now tries `iterable_element_type`
        // before finally giving up to `Mixed`/`Mixed` - see that
        // function's own doc comment for exactly what it covers
        // (`Traversable`/`Iterator`/`Generator`/`IteratorAggregate`).
        let (key_ty, element_ty) = match Self::array_element_type(&subject_ty) {
            Some(elem) => (
                Self::array_key_type(&subject_ty).unwrap_or(Type::Mixed),
                elem,
            ),
            None => self
                .iterable_element_type(&subject_ty)
                .unwrap_or((Type::Mixed, Type::Mixed)),
        };
        // Phase 29: `Foreach` still checks its body once directly
        // against the real `locals` (deliberately *not* also
        // adopting `While`/`For`'s own separate "may run zero
        // times" merge with the pre-loop state here - tried
        // during this fix and reverted: a common, real idiom
        // mutates the very same array a `foreach` iterates via a
        // nested write keyed by the loop variable itself
        // (`foreach ($items as $day => $data) { ...;
        // $items[$day]['derivative'] = ...; }`, confirmed in
        // `CalendarMonthly::buildMonthCalendar`) - unioning in the
        // *pre-loop* shape as an "also possible" alternative there
        // is actively wrong, since every key iterated over really
        // does get the loop's own treatment, never the stale
        // pre-loop one). What *was* missing, unconditionally: a
        // `break` reached anywhere in `body` had its own state
        // silently discarded from everything past the loop -
        // `always_terminates` correctly stops a break-ending
        // branch from leaking into the *rest of its own block*,
        // but nothing captured it for the loop's own exit state.
        // `loop_break_states` closes exactly that gap, without
        // touching the single-representative-pass model otherwise.
        //
        // Phase 55: the body itself now converges to a fixed point
        // across up to 3 passes (see `converge_loop_body`'s own doc
        // comment) - a variable reassigned inside the body is no
        // longer stuck at its pre-loop type for the whole check.
        // The *entry* state fed to every pass, and the final
        // result, are both still `locals` directly - never merged
        // with a separate pre-loop snapshot, preserving the
        // deliberate zero-iterations trade-off described above
        // exactly. Only the within/across-iteration staleness this
        // phase targets is fixed.
        //
        // The key/value binding itself is re-applied at the start
        // of *every* pass, not just once before the first - real
        // PHP rebinds a foreach's loop variable(s) fresh from the
        // subject on every actual iteration, so a pass must never
        // start from what the *previous* pass's body left the
        // loop variable mutated to. Confirmed necessary by a real
        // corpus false positive this exact ordering fixed:
        // `QNumericRangeScope::parse`'s `foreach ($range as $i =>
        // &$val) { ...; $val = floatval(...); }` - a by-reference
        // loop variable reassigned inside the body would otherwise
        // wrongly union its post-body `float` into the *next*
        // pass's entry as `string|float`, even though each real
        // iteration's `&$val` aliases a *different* array slot
        // that pass's body never touched.
        let entry = locals.clone();
        self.loop_depth += 1;
        let (body_locals, break_states) = self.converge_loop_body(
            &entry,
            declared_return,
            label,
            |this, pass_locals, declared_return, label, _pass| {
                if let Some(k) = key {
                    this.bind_simple_target(k, key_ty.clone(), pass_locals);
                    this.check_expr(k, pass_locals, label);
                }
                // A foreach value can itself be a destructuring
                // target (`foreach ($pairs as [$num, $letter])`) -
                // route it through the same fully-recursive
                // helpers a plain destructuring assignment uses,
                // not the bare-variable-only `bind_simple_target`.
                // `check_assignment_target` already covers "check
                // as a read where appropriate" for whatever isn't
                // a pure definition site, so no separate
                // `check_expr` call is needed on top of it.
                this.check_assignment_target(value, pass_locals, label);
                this.bind_assignment_target(value, element_ty.clone(), pass_locals);
                this.check_stmt(body, pass_locals, declared_return, label);
            },
        );
        self.loop_depth -= 1;
        *locals = body_locals;
        for state in break_states {
            *locals = merge_scopes(locals, &state);
        }
    }
}
