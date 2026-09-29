use crate::checker::expr::ranges::{NullProbeVerdictFn, expr_reads_a_property};
use crate::checker::operators::ordering_op;
use crate::checker::{Checker, Locals, literal_string_key, narrowing_type_for_predicate};
use syntax::ast::*;
use syntax::token::Span;
use types::Type;
use types::{OrderingOp, compare_range_outcome};

impl<'a> Checker<'a> {
    /// Looks up a bare global-constant reference (`ExprKind::Name`,
    /// already excluding `true`/`false`/`null` - see `bare_name_literal_
    /// type`'s own doc comment) through the `ConstantResolver`, against
    /// `Registry::global_constants`.
    /// Constants share the identical "check the current namespace first,
    /// fall back to the global namespace for an unqualified reference"
    /// resolution rule real PHP functions do (unlike class names, which
    /// never fall back), matched against `use const` imports -
    /// `QualifiedName::candidates`. `None` for anything not in the table (an engine-
    /// provided constant like `PHP_EOL`, or a `const`/`define()` whose
    /// own value wasn't a plain literal - see `Registry::global_
    /// constants`'s own doc comment for the exact scope), leaving
    /// `bare_name_literal_type`'s existing `Mixed` fallback in place.
    pub(crate) fn global_constant_type(&self, name: &QualifiedName) -> Option<Type> {
        let (primary, fallback) = name.candidates();
        self.settings
            .constants
            .global_constant_type(self.registry, primary, fallback)
    }

    /// Checks an `isset()`/`??`-style "quiet" left-hand-side chain: walks
    /// down through `Index`/`PropertyAccess`/`DynamicPropertyAccess`,
    /// checking each level's own sub-expressions (an index's key, a
    /// dynamic property's name) normally, but suppressing the root
    /// variable's own "undefined variable" check and `PropertyAccess`'s
    /// own "access to undefined property" check at every level along the
    /// way - exactly what `isset()`/`??` themselves suppress in real PHP,
    /// and nothing more. `ClassConstFetch`/`StaticPropertyFetch` is never part of this walk (an
    /// undeclared *static* property is a real, non-suppressible PHP
    /// fatal error), and anything past a function/method call is checked
    /// normally, since `isset()`/`??` don't evaluate quietly past one.
    pub(crate) fn check_expr_quiet_root(
        &mut self,
        e: &Expr,
        locals: &mut Locals,
        label: &str,
    ) -> Type {
        syntax::stack::guard(|| {
            // Every arm here is consumed only for its side effects - neither
            // `??` nor `??=` (its only two call sites) depend on the
            // quiet-checked left side's own type, so the exact value
            // returned doesn't matter beyond satisfying the type system
            // post-unification.
            match &e.kind {
                ExprKind::Variable(_) => {
                    // Deliberately not `check_expr` - a bare variable here is
                    // exactly the "might not be defined" case `??` exists to
                    // handle safely.
                    Type::mixed()
                }
                ExprKind::Index(base, idx) => {
                    self.check_expr_quiet_root(base, locals, label);
                    if let Some(i) = idx {
                        self.check_expr(i, locals, label);
                    }
                    Type::mixed()
                }
                ExprKind::PropertyAccess(base, prop, _) => {
                    self.check_expr_quiet_root(base, locals, label);
                    // Opt-in `checkDynamicProperties` strict mode - real
                    // PHPStan's
                    // lenient default silently allows `isset($x->undefined)`
                    // (the whole point of `isset`/`??` is to probe for
                    // exactly that), but the stricter mode still flags it as
                    // a likely typo. Deliberately skips the ordinary path's
                    // `guarded_properties`/Pest-hook/visibility exemptions -
                    // this is a narrower, opt-in-only re-check, not a full
                    // duplicate of the real property-access rule.
                    if self.settings.rules.check_dynamic_properties {
                        let base_ty = self.infer_expr(base, locals);
                        if self.member_confirmed_missing(&base_ty, |c| {
                            c == "stdClass"
                                || self.registry.lookup_property(c, prop).is_some()
                                || self.registry.lookup_method(c, "__get").is_some()
                        }) {
                            self.report(
                                format!("access to undefined property {base_ty}::${prop}"),
                                e.span,
                                2,
                                "property.notFound",
                            );
                        }
                    }
                    Type::mixed()
                }
                ExprKind::DynamicPropertyAccess(base, name_expr) => {
                    self.check_expr_quiet_root(base, locals, label);
                    self.check_expr(name_expr, locals, label);
                    Type::mixed()
                }
                _ => self.check_expr(e, locals, label),
            }
        })
    }

    /// `isset($x)`/`empty($x)`/`$x ?? ...`/`$x ??= ...` where
    /// `$x` is a bare, already-guaranteed-defined local - `locals`
    /// itself is this project's own "definitely bound on every path"
    /// signal (`merge_scopes` drops a variable entirely rather than
    /// carry it forward unsoundly when only one branch defines it) - and
    /// whose tracked type proves the null-probe can never change the
    /// outcome. Deliberately scoped to a bare `Variable` root only: a
    /// deeper chain (`isset($x['key'])`, `isset($x->prop)`) needs
    /// offset-existence/property-initialization dataflow this project
    /// doesn't model yet (real PHPStan gates the equivalent behind its
    /// own `checkAdvancedIsset` config flag) - left alone rather than
    /// risk a false positive from an unmodeled "might not exist" case.
    pub(crate) fn check_pointless_null_probe(
        &mut self,
        e: &Expr,
        locals: &Locals,
        identifier_prefix: &str,
        operator_description: &str,
        verdict_fn: NullProbeVerdictFn,
    ) {
        let ExprKind::Variable(name) = &e.kind else {
            return;
        };
        if is_superglobal(name) {
            return;
        }
        // Inside a loop body, `locals`'s own tracked type only reflects
        // a single, non-iterated pass through it (see
        // `Checker::loop_depth`'s own doc comment) - a variable
        // initialized to a narrow type once before the loop, then
        // self-referentially widened only via the very statement being
        // checked here (`$err ??= $e;` inside a loop's own `catch`)
        // looks "always null" on this first pass even though a real
        // second-or-later iteration would see otherwise.
        if self.loop_depth > 0 {
            return;
        }
        // A `static` local can carry a value left over from an earlier
        // call - see `Checker::static_var_names`'s own doc comment
        // (e.g. `static $curlVersion = null; $curlVersion ??=
        // curl_version();`, genuinely silent in real PHPStan too).
        if self.static_var_names.contains(name.as_ref()) {
            return;
        }
        let Some(ty) = locals.get(name) else {
            return;
        };
        let Some(verdict) = verdict_fn(ty) else {
            return;
        };
        self.report(
            format!("Variable ${name} {operator_description} always exists and {verdict}."),
            e.span,
            0,
            format!("{identifier_prefix}.variable"),
        );
    }

    /// The offset/property-chain counterpart of
    /// `check_pointless_null_probe` just above - mirrors real PHPStan's
    /// own recursive `IssetabilityDescriptor::resolve`/`IssetCheck::doCheck`,
    /// walking
    /// `e`'s own `Index`/`PropertyAccess` chain base-first (the same
    /// recursion shape `check_expr_quiet_root` already uses, for a
    /// different reason). Returns the resolved leaf's own value type once
    /// *every* link in the chain is confirmed unconditionally present -
    /// `None` for anything not provably present end-to-end (an ordinary
    /// variable/property, a `Call`/`MethodCall` anywhere in the chain -
    /// real `isset()`/`??` never evaluate past one - a `Union` whose
    /// members disagree, ...). A provably *absent* array offset is a
    /// real, separate diagnostic reported directly as a side effect here
    /// (`isset.offset`/`{identifier_prefix}.offset`, "does not exist") -
    /// not folded into the `None` return, so it isn't silently lost
    /// partway through a longer chain the way a merely-ambiguous link
    /// would be.
    fn resolve_always_exists(
        &mut self,
        e: &Expr,
        locals: &Locals,
        identifier_prefix: &str,
        operator_description: &str,
    ) -> Option<Type> {
        syntax::stack::guard(|| {
            match &e.kind {
                ExprKind::Variable(name) => {
                    if is_superglobal(name)
                        || self.loop_depth > 0
                        || self.static_var_names.contains(name.as_ref())
                    {
                        return None;
                    }
                    locals.get(name).cloned()
                }
                ExprKind::Index(base, Some(idx)) => {
                    let key = literal_string_key(idx)?;
                    let shape_key = crate::checker::literal_shape_key(idx)?;
                    let base_ty = self.resolve_always_exists(
                        base,
                        locals,
                        identifier_prefix,
                        operator_description,
                    )?;
                    // See this function's own doc comment for why these two
                    // calls, not a bespoke tri-state helper: assuming
                    // "present" is impossible means the key can never be
                    // there; assuming "absent" is impossible means it's
                    // unconditionally there - `Type::with_key_presence`
                    // already answers both questions correctly (including
                    // through a `Union`, recursively), this just asks it
                    // twice rather than duplicating its own logic.
                    if base_ty.with_key_presence(&shape_key, true).is_never() {
                        self.report(
                            format!(
                                "Offset '{key}' on {base_ty} {operator_description} does not exist."
                            ),
                            e.span,
                            0,
                            format!("{identifier_prefix}.offset"),
                        );
                        return None;
                    }
                    if base_ty.with_key_presence(&shape_key, false).is_never() {
                        return base_ty.offset_value_type(Some(&shape_key));
                    }
                    None
                }
                ExprKind::PropertyAccess(base, prop, _) => {
                    let base_ty = self.resolve_always_exists(
                        base,
                        locals,
                        identifier_prefix,
                        operator_description,
                    )?;
                    let (_, sig) = self.find_property_owner(&base_ty, prop)?;
                    // A hooked property's own getter can compute anything,
                    // unrelated to its own init-state - matches every other
                    // `is_virtual`-gated check in this codebase. `!has_default`
                    // is Design ##2's own deliberately safe-direction
                    // exemption (see `PropertyReflection::has_default`'s own doc
                    // comment): a property with no default *might* genuinely
                    // still be uninitialized, so `isset()`/`??` checking it is
                    // never treated as redundant here, even when its own
                    // declared type is non-nullable.
                    if sig.is_virtual() || !sig.has_default() {
                        return None;
                    }
                    Some(sig.ty().clone())
                }
                _ => None,
            }
        })
    }

    /// `isset($x->prop)`/`isset($x['key'])`/`$x->prop ?? ...`/
    /// `$x['key'] ?? ...` where every link in the chain is provably
    /// always-present - the entry point into `resolve_always_exists`
    /// just above, called alongside (never instead of)
    /// `check_pointless_null_probe` at each of that function's own three
    /// call sites. Scoped to `e` itself being an `Index`/`PropertyAccess`,
    /// since a bare variable is `check_pointless_null_probe`'s own job
    /// already, and re-deriving the identical verdict for it here would
    /// just double-report.
    pub(crate) fn check_pointless_offset_or_property_probe(
        &mut self,
        e: &Expr,
        locals: &Locals,
        identifier_prefix: &str,
        operator_description: &str,
        verdict_fn: NullProbeVerdictFn,
    ) {
        if !matches!(&e.kind, ExprKind::Index(..) | ExprKind::PropertyAccess(..)) {
            return;
        }
        let Some(leaf_ty) =
            self.resolve_always_exists(e, locals, identifier_prefix, operator_description)
        else {
            return;
        };
        let Some(verdict) = verdict_fn(&leaf_ty) else {
            return;
        };
        match &e.kind {
            ExprKind::Index(base, Some(idx)) => {
                // `resolve_always_exists` only ever returns `Some` here
                // once `literal_string_key(idx)` already succeeded once -
                // safe to redo, `idx` itself hasn't changed.
                let Some(key) = literal_string_key(idx) else {
                    return;
                };
                let base_ty = self.infer_expr(base, locals);
                self.report(format!(
                        "Offset '{key}' on {base_ty} {operator_description} always exists and {verdict}."
                    ),
                    e.span,
                    0,
                    format!("{identifier_prefix}.offset"),
                );
            }
            ExprKind::PropertyAccess(base, prop, _) => {
                let base_ty = self.infer_expr(base, locals);
                // Real PHPStan's own `PropertyDescriptor::describeProperty`
                // also distinguishes "fetched on a narrower type than the
                // property's own declaring class" - simplified here to
                // always name the resolved owner directly, a real,
                // disclosed precision gap only ever visible in the
                // message text itself, never in whether something gets
                // flagged.
                let owner = self
                    .find_property_owner(&base_ty, prop)
                    .map(|(owner, _)| owner)
                    .unwrap_or_else(|| base_ty.to_string().into());
                self.report(
                    format!(
                        "Property {owner}::${prop} ({leaf_ty}) {operator_description} {verdict}."
                    ),
                    e.span,
                    0,
                    format!("{identifier_prefix}.property"),
                );
            }
            _ => {}
        }
    }

    /// Real PHPStan's own `ConstantLooseComparisonRule`
    /// (level 4): flags a `==`/`!=` whose outcome is provably constant
    /// from both operands' own literal types - `Type::loosely_equals`'s
    /// own doc comment has the full scope/version-gating reasoning.
    /// Fires independently of `infer_binary`'s identical constant-folding
    /// (consumed separately by the `if`/`while`/ternary/... constant-
    /// condition family) since real PHPStan's own rule fires on *every*
    /// occurrence of the comparison, not just ones in condition position,
    /// matching its own dedicated identifiers/wording exactly. Guarded
    /// on `self.loop_depth == 0` for the identical reason `check_
    /// constant_numeric_comparison`'s own doc comment (right below) is:
    /// a counting `for ($j = 0; $j !== 3; $j++)` idiom is exactly as
    /// vulnerable to this project's own bounded-pass convergence
    /// approximation as the numeric-range case. Also guarded on neither
    /// operand reading a property (`expr_reads_a_property`'s own doc
    /// comment).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_constant_loose_comparison(
        &mut self,
        op: &BinOp,
        l: &Expr,
        r: &Expr,
        l_ty: &Type,
        r_ty: &Type,
        span: Span,
    ) {
        if self.loop_depth > 0 || expr_reads_a_property(l) || expr_reads_a_property(r) {
            return;
        }
        if !matches!(op, BinOp::Eq | BinOp::NotEq) {
            return;
        }
        let Some(equal) = self
            .registry
            .php_version_consensus(|v| l_ty.loosely_equals(r_ty, v))
        else {
            return;
        };
        let outcome = if matches!(op, BinOp::Eq) {
            equal
        } else {
            !equal
        };
        let sigil = if matches!(op, BinOp::Eq) { "==" } else { "!=" };
        let rule_name = if matches!(op, BinOp::Eq) {
            "equal"
        } else {
            "notEqual"
        };
        self.report(format!(
                "Loose comparison using {sigil} between {l_ty} and {r_ty} will always evaluate to {}.",
                if outcome { "true" } else { "false" }
            ),
            span,
            4,
            format!("{rule_name}.always{}", if outcome { "True" } else { "False" }),
        );
    }

    /// Real PHPStan's own `Rules/Comparison/StrictComparisonOfDifferentTypesRule.php`:
    /// a `===`/`!==` comparison whose outcome is provably constant
    /// from both operands' own types: `identical.alwaysFalse`/
    /// `.alwaysTrue`, `notIdentical.alwaysFalse`/`.alwaysTrue`. The real
    /// type-relation reasoning (`RicherScopeGetTypeHelper::
    /// getIdenticalResult`/`InitializerExprTypeResolver::
    /// resolveIdenticalType`) lives in `registry::identical_result`
    /// itself, not here - shared with `checker/expr.rs`'s own `match`
    /// per-arm exhaustiveness reasoning, which reaches this exact same
    /// question for the exact same reason real PHPStan's own
    /// `MatchExpressionRule` does (each arm condition is itself checked
    /// as `$matchCondition === $rawCondition`). `!==`'s own outcome is
    /// always exactly `===`'s negated answer - mirrors real PHPStan's
    /// `getNotIdenticalResult` computing it from `getIdenticalResult`
    /// directly, never independently.
    ///
    /// Guarded on `loop_depth`/`expr_reads_a_property` the same way the
    /// sibling `check_constant_loose_comparison` right above is - the
    /// same "one pass looks tighter than a real fixed point" unsoundness
    /// risk applies here too (a loop counter's own `IntLiteral` snapshot
    /// on a single pass, e.g. `$j !== 3` right after `$j = 0`, is
    /// exactly the shape the finite-value fast path above would compare
    /// directly).
    ///
    /// Honors the same "acts as an implicit else" `elseif`-with-no-
    /// trailing-`else` exemption `elseif.alwaysTrue`/`function.
    /// alreadyNarrowedType` already have (`alwaysTrue` only, never
    /// `alwaysFalse`) - confirmed present in this exact real rule's own
    /// source (`$isLast === true && !$this->reportAlwaysTrueInLastCondition`),
    /// not assumed by analogy the way it would be for an unverified
    /// sibling.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_strict_comparison_of_different_types(
        &mut self,
        op: &BinOp,
        l: &Expr,
        r: &Expr,
        l_ty: &Type,
        r_ty: &Type,
        span: Span,
        is_exempt_last_condition: bool,
    ) {
        if self.loop_depth > 0
            || expr_reads_a_mutable_property(l)
            || expr_reads_a_mutable_property(r)
        {
            return;
        }
        // A `never` operand is never evaluated (every case of an enum
        // already handled).
        if !matches!(op, BinOp::Identical | BinOp::NotIdentical)
            || l_ty.is_never()
            || r_ty.is_never()
        {
            return;
        }
        let Some(identical_outcome) = self.registry.identical_result(l_ty, r_ty) else {
            return;
        };
        let is_identical = matches!(op, BinOp::Identical);
        let outcome = if is_identical {
            identical_outcome
        } else {
            !identical_outcome
        };
        if outcome && is_exempt_last_condition {
            return;
        }
        let sigil = if is_identical { "===" } else { "!==" };
        let rule_name = if is_identical {
            "identical"
        } else {
            "notIdentical"
        };
        self.report(format!(
                "Strict comparison using {sigil} between {l_ty} and {r_ty} will always evaluate to {}.",
                if outcome { "true" } else { "false" }
            ),
            span,
            4,
            format!("{rule_name}.always{}", if outcome { "True" } else { "False" }),
        );
    }

    /// Real PHPStan's own
    /// `NumberComparisonOperatorsConstantConditionRule` (level 4): flags a
    /// `</>/<=/>=` comparison whose outcome is provably constant from
    /// both operands' known int ranges - `Type::int_bounds`/
    /// `compare_range_outcome`'s own doc comments have the range-
    /// arithmetic reasoning; this is purely the reporting half, mirroring
    /// `check_constant_loose_comparison` just above's identical shape
    /// (its own standalone rule, fired on every occurrence, not folded
    /// into `if`/`while`'s own generic constant-condition family). Real
    /// identifiers/wording (`smaller`/`greater`/`smallerOrEqual`/
    /// `greaterOrEqual`) are PHP's own operator names, not this
    /// project's own `Lt`/`Gt`/... spelling.
    ///
    /// Guarded on `self.loop_depth == 0` for the identical reason
    /// `check_pointless_null_probe` already is: this project's own loop
    /// convergence (`converge_loop_body`) is a bounded, fixed-pass
    /// approximation of a real fixed point, not a true one - a counting
    /// `for ($j = 0; $j < 3; $j++)`'s own condition, checked on any one
    /// pass, sees `$j` as some specific (if widening) literal/range
    /// rather than the fully general `int` real PHPStan's own genuine
    /// fixed-point solver would settle on, so "provably true/false"
    /// here inside a loop would be systematically unsound - the single
    /// most common loop idiom in PHP (`0 < 3` "always true") would
    /// otherwise misfire on every ordinary counting loop. Also guarded
    /// on neither operand reading a property
    /// (`expr_reads_a_property`'s own doc comment) - an intervening
    /// method call between two reads of the same property is not
    /// modeled as potentially repopulating it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_constant_numeric_comparison(
        &mut self,
        op: &BinOp,
        l: &Expr,
        r: &Expr,
        l_ty: &Type,
        r_ty: &Type,
        span: Span,
    ) {
        if self.loop_depth > 0 || expr_reads_a_property(l) || expr_reads_a_property(r) {
            return;
        }
        let Some(ordering) = ordering_op(op) else {
            return;
        };
        let rule_name = match ordering {
            OrderingOp::Lt => "smaller",
            OrderingOp::Gt => "greater",
            OrderingOp::LtEq => "smallerOrEqual",
            OrderingOp::GtEq => "greaterOrEqual",
        };
        let Some(l_bounds) = l_ty.int_bounds() else {
            return;
        };
        let Some(r_bounds) = r_ty.int_bounds() else {
            return;
        };
        let Some(outcome) = compare_range_outcome(ordering, l_bounds, r_bounds) else {
            return;
        };
        let sigil = match ordering {
            OrderingOp::Lt => "<",
            OrderingOp::Gt => ">",
            OrderingOp::LtEq => "<=",
            OrderingOp::GtEq => ">=",
        };
        self.report(
            format!(
                "Comparison operation \"{sigil}\" between {l_ty} and {r_ty} is always {}.",
                if outcome { "true" } else { "false" }
            ),
            span,
            4,
            format!(
                "{rule_name}.always{}",
                if outcome { "True" } else { "False" }
            ),
        );
    }

    /// Real PHPStan's own `Rules/Comparison/ImpossibleCheckTypeFunctionCallRule.php`
    /// (`ImpossibleCheckTypeHelper::getSpecifiedType`) - a single-argument
    /// type-check call (`is_string($x)`, `is_int($x)`, ...) whose outcome
    /// is already statically decided by its argument's own current type.
    /// Deliberately scoped to exactly the `narrowing_type_for_predicate`
    /// table this project's own narrowing engine already trusts for
    /// these same functions (`checker/narrowing.rs`'s own `is_string`/
    /// `is_int`/... dispatch) - real PHPStan's own helper additionally
    /// reasons about `in_array()`, `method_exists()`, `assert()`, and a
    /// handful of others via its own generic `TypeSpecifier` machinery
    /// this project has no equivalent of; not attempted here, favoring a
    /// false negative on those shapes over a wrong guess. Reuses the
    /// exact same disjointness/subtyping primitives `assert.
    /// impossibleType`/`.alreadyNarrowedType` already established for
    /// the PHPDoc-tag version of this same idea (`Relation::disjoint`,
    /// `registry.accepts`) - disjointness needs a proof (different scalar
    /// families, distinct literals, or classes neither of which can be
    /// extended into the other), so an unprovable case stays silent
    /// rather than risking a wrong "always false".
    ///
    /// Deliberately not guarded on `loop_depth` the way
    /// `check_constant_numeric_comparison` above is - that guard exists
    /// specifically because a numeric *bound* read on one loop pass can
    /// look tighter than it really is across later iterations (`$j < 3`
    /// right after `$j = 0`). A type-*family* fact doesn't have that
    /// failure mode: `Locals` convergence already unions every loop
    /// body path's own facts before a post-loop condition ever sees
    /// them, so a variable that's sometimes `string` and sometimes `int`
    /// across iterations already reads back as `string|int` here, which
    /// neither `Relation::disjoint` nor `accepts` treats as
    /// decided.
    pub(crate) fn check_impossible_type_check_call(
        &mut self,
        callee: &Expr,
        args: &[Arg],
        locals: &Locals,
        is_exempt_last_condition: bool,
        span: Span,
    ) {
        let ExprKind::Name(name) = &callee.kind else {
            return;
        };
        let name = name.trim_start_matches('\\');
        let Some(outcome) = self.type_check_call_outcome(name, args, locals) else {
            return;
        };
        let arg_ty = self.infer_expr(&args[0].value, locals);
        if !outcome {
            self.report(
                format!("Call to function {name}() with {arg_ty} will always evaluate to false."),
                span,
                4,
                "function.impossibleType",
            );
        } else if !is_exempt_last_condition {
            self.report(
                format!("Call to function {name}() with {arg_ty} will always evaluate to true."),
                span,
                4,
                "function.alreadyNarrowedType",
            );
        }
    }

    /// A type-check call's (`is_int($x)`, `is_numeric($x)`, ...) known
    /// answer: `true` when every value its argument can have passes,
    /// `false` when none does, `None` otherwise.
    pub(crate) fn type_check_call_outcome(
        &self,
        name: &str,
        args: &[Arg],
        locals: &Locals,
    ) -> Option<bool> {
        let target_ty = match name.to_ascii_lowercase().as_str() {
            "is_numeric" => types::parse_type_string("int|float|numeric-string"),
            other => narrowing_type_for_predicate(other)?,
        };
        let [single_arg] = args else {
            return None;
        };
        if single_arg.spread {
            return None;
        }
        // Deliberately plain `infer_expr` on the whole argument
        // expression, not `narrowing_key`/`current_narrowed_type` (the
        // mechanism `apply_narrowing`'s own predicate dispatch above
        // uses) - that pair answers "what does the underlying container
        // *key* alone currently narrow to," which is the wrong question
        // here for a wrapping expression like `$page['k'] ?? null`: the
        // key's own narrowed type is just the shape field's value type
        // (`list<string>`, no `null` - an unsealed shape's "key absent"
        // state isn't itself part of that `Type`), while the call's real
        // argument value is the whole `?? null` expression, correctly
        // `list<string>|null` under plain `infer_expr` - conflating the
        // two produced a real false `function.alreadyNarrowedType` on
        // exactly this shape during this check's own corpus-style
        // regression pass.
        let arg_ty = self.infer_expr(&single_arg.value, locals);
        if arg_ty.is_mixed() || arg_ty.is_never() {
            return None;
        }
        if self.relation().disjoint(&target_ty, &arg_ty) {
            Some(false)
        } else if self.registry.is_super_type_of(&target_ty, &arg_ty) == types::TrinaryLogic::Yes {
            Some(true)
        } else {
            None
        }
    }
}

/// `check_strict_comparison_of_different_types`'s own narrower cousin
/// of `expr_reads_a_property` - a real static *property* read
/// (`Foo::$bar`) carries the same "could be repopulated between two
/// reads" risk `expr_reads_a_property` already guards `check_constant_
/// loose_comparison`/`check_constant_numeric_comparison` against, but a
/// class *constant*/enum-case reference (`Foo::BAR`, `Status::A`) is
/// exactly as immutable as a bare literal - conflating the two (as
/// `expr_reads_a_property`'s own static-fetch `=> true` arm
/// does, correctly for those two siblings' own narrower needs) would
/// silently exempt the single most valuable real-world shape this check
/// exists for (`$status === Status::A`) from ever firing at all -
/// caught by this check's own manual verification pass, not a test
/// written in advance of knowing this gap existed.
fn expr_reads_a_mutable_property(e: &Expr) -> bool {
    let e = e.peeled();
    syntax::stack::guard(|| match &e.kind {
        ExprKind::PropertyAccess(..) | ExprKind::DynamicPropertyAccess(..) => true,
        ExprKind::StaticPropertyFetch(..) => true,
        ExprKind::Call(callee, args) => {
            expr_reads_a_mutable_property(callee)
                || args.iter().any(|a| expr_reads_a_mutable_property(&a.value))
        }
        ExprKind::MethodCall(base, _, args, _) => {
            expr_reads_a_mutable_property(base)
                || args.iter().any(|a| expr_reads_a_mutable_property(&a.value))
        }
        ExprKind::Binary(_, l, r) => {
            expr_reads_a_mutable_property(l) || expr_reads_a_mutable_property(r)
        }
        ExprKind::Unary(_, inner) => expr_reads_a_mutable_property(inner),
        ExprKind::Index(base, idx) => {
            expr_reads_a_mutable_property(base)
                || idx.as_deref().is_some_and(expr_reads_a_mutable_property)
        }
        _ => false,
    })
}
