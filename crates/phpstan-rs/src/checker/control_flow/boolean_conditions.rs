use crate::checker::{Checker, Locals};
use syntax::ast::*;

impl<'a> Checker<'a> {
    /// The real, original PHPStan design -
    /// `ConstantConditionRuleHelper::getBooleanType`. Trusts `e`'s own
    /// inferred type being exactly `true`/`false`, not just
    /// a syntactic literal - a bare literal already infers to
    /// `true`/`false` exactly, so inferred-type trust is a
    /// strict superset of syntactic-literal trust.
    /// `should_skip_constant_condition`
    /// excludes every shape that has - or, for a `Call`-shaped condition,
    /// deliberately doesn't yet have - its own separate, dedicated check.
    pub(crate) fn inferred_constant_condition(
        &self,
        e: &ExprNode,
        locals: &Locals,
    ) -> Option<bool> {
        let arenas = self.syntax_arenas()?;
        if should_skip_constant_condition(arenas, e) {
            return None;
        }
        self.infer_expr(e, locals).bool_literal_value()
    }

    /// `BooleanAndConstantConditionRule`/`BooleanOrConstantConditionRule` -
    /// called for both the symbol (`&&`/`||`, `BinOp::And`/`Or`) and word
    /// (`and`/`or`, `BinOp::LogicalAnd`/`LogicalOr`) spellings, `op`
    /// carrying the exact matched variant through to `kind`/`symbol`
    /// below so the identifier prefix (`booleanAnd`/`logicalAnd`) and
    /// rendered message text match real PHPStan's own per-spelling
    /// `$identifierType`/`getOperatorSigil()`.
    /// Three independent diagnostics (left side, right side, whole-expression result), the
    /// left one unconditional, the other two gated by `is_first_level`
    /// (`Checker::in_condition_position`'s own doc comment - real
    /// PHPStan's `!isInFirstLevelStatement()` guard: skipped exactly when
    /// this `&&`/`||` *is* the direct condition of an `if`/`while`/
    /// ternary, since that's already covered by that construct's own
    /// check). `whole` is the same `&ExprNode` `check_expr`'s own `ExprKind::
    /// Binary` arm already has in hand - never reconstructed. Its own
    /// diagnostic deliberately calls `infer_expr` directly rather than
    /// going through `inferred_constant_condition` (unlike left/right,
    /// above): `whole` is itself a `Binary(And|Or, ..)` node, exactly the
    /// shape `should_skip_constant_condition` excludes so a *condition*
    /// shaped like `&&`/`||` defers to this very function instead of
    /// being double-reported by the generic if/while/ternary check -
    /// routing this function's own result computation back through that
    /// same skip-list would make it permanently unreachable.
    ///
    /// Real PHPStan's own `isLast`/`reportAlwaysTrueInLastCondition`
    /// exemption (elseif-chain-position, `check_if`'s own equivalent) is
    /// deliberately not replicated here: it comes from `LastConditionVisitor`,
    /// which only ever tags nodes reachable from an `if`/`elseif` chain's
    /// own condition list, never a bare `&&`/`||` reached as an ordinary
    /// sub-expression - there is no "chain position" concept for one to
    /// have, so nothing is actually being skipped by omitting it.
    ///
    /// `locals` here is deliberately the scope before `l`'s own
    /// truthy/falsey narrowing, not the caller's already-narrowed one -
    /// otherwise a bare-variable left operand (`$same && true`) would be
    /// judged against a scope where `$same` had already been narrowed
    /// truthy on the assumption the left side holds, making it look
    /// constantly true by pure circularity regardless of `$same`'s real,
    /// unnarrowed type. Only the right-side check genuinely wants the
    /// left-narrowed scope (matching real PHPStan's own
    /// `getRightScope()`) - narrowed internally here, rather than by the
    /// caller, so the left check can never see it by mistake. The
    /// whole-result check also takes the unnarrowed
    /// `locals`: `infer_expr`/`infer_binary`'s own `BinOp::And`/`Or` arms
    /// already do their own internal left-narrowing before inferring the
    /// right side's type, so handing them an already-narrowed scope would
    /// double-apply it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_boolean_and_or(
        &mut self,
        op: &BinOp,
        l: &ExprNode,
        r: &ExprNode,
        whole: &ExprNode,
        locals: &Locals,
        is_first_level: bool,
    ) {
        let is_and = matches!(op, BinOp::And | BinOp::LogicalAnd);
        let kind = match op {
            BinOp::And => "booleanAnd",
            BinOp::LogicalAnd => "logicalAnd",
            BinOp::Or => "booleanOr",
            BinOp::LogicalOr => "logicalOr",
            _ => unreachable!(
                "check_boolean_and_or is only ever called with And/Or/LogicalAnd/LogicalOr"
            ),
        };
        let symbol = crate::checker::binop_sigil(op);
        if let Some(value) = self.inferred_constant_condition(l, locals) {
            report!(
                self,
                format!(
                    "Left side of {symbol} is always {}.",
                    if value { "true" } else { "false" }
                ),
                self.expr_span(l.id),
                4,
                format!("{kind}.leftAlways{}", if value { "True" } else { "False" }),
            );
        }
        if is_first_level {
            return;
        }
        let mut narrowed = locals.clone();
        self.apply_narrowing(l, &mut narrowed, is_and);
        if let Some(value) = self.inferred_constant_condition(r, &narrowed) {
            report!(
                self,
                format!(
                    "Right side of {symbol} is always {}.",
                    if value { "true" } else { "false" }
                ),
                self.expr_span(r.id),
                4,
                format!("{kind}.rightAlways{}", if value { "True" } else { "False" }),
            );
        }
        // Deliberately bypasses `inferred_constant_condition` here (unlike
        // the left/right checks above): `whole` is itself a `Binary(And|
        // Or, ..)` node, exactly the shape `should_skip_constant_condition`
        // excludes - that exclusion exists so the generic if/while/ternary
        // check defers a condition shaped like `&&`/`||` to this very
        // function instead of double-reporting, not to gate this
        // function's own whole-result computation against itself. Calling
        // `inferred_constant_condition(whole, ..)` here would always
        // return `None`.
        let whole_value = self.infer_expr(whole, locals).bool_literal_value();
        if let Some(value) = whole_value {
            report!(
                self,
                format!(
                    "Result of {symbol} is always {}.",
                    if value { "true" } else { "false" }
                ),
                self.expr_span(whole.id),
                4,
                format!("{kind}.always{}", if value { "True" } else { "False" }),
            );
        }
    }

    /// `LogicalXorConstantConditionRule` - left/right only, no whole-expression-result
    /// diagnostic and no `is_first_level` gating at all (unlike `&&`/
    /// `||`) - `xor` doesn't short-circuit, so there's no "already
    /// covered by the enclosing if/while's own check" overlap to avoid,
    /// and no real rule for it exists to model that overlap regardless.
    pub(crate) fn check_logical_xor(&mut self, l: &ExprNode, r: &ExprNode, locals: &Locals) {
        if let Some(value) = self.inferred_constant_condition(l, locals) {
            report!(
                self,
                format!(
                    "Left side of xor is always {}.",
                    if value { "true" } else { "false" }
                ),
                self.expr_span(l.id),
                4,
                format!(
                    "logicalXor.leftAlways{}",
                    if value { "True" } else { "False" }
                ),
            );
        }
        if let Some(value) = self.inferred_constant_condition(r, locals) {
            report!(
                self,
                format!(
                    "Right side of xor is always {}.",
                    if value { "true" } else { "false" }
                ),
                self.expr_span(r.id),
                4,
                format!(
                    "logicalXor.rightAlways{}",
                    if value { "True" } else { "False" }
                ),
            );
        }
    }
}

/// Real PHPStan's own `ConstantConditionRuleHelper::shouldSkip` - every
/// one of these shapes has its own separate, dedicated rule (an
/// equality/identity/ordering comparison, `instanceof`, `!`, `&&`/`||`,
/// a ternary). Excluding them here means `inferred_constant_condition`
/// only ever answers for the shapes that genuinely have nowhere else to
/// be caught.
///
/// A type-check call (`is_int($x)`) is excluded too: the impossible-
/// type-check rule reports it (`function.alreadyNarrowedType`/
/// `function.impossibleType`). Any other call condition is answered like
/// any other shape.
fn should_skip_constant_condition(arenas: &NodeArenas, e: &ExprNode) -> bool {
    // Parentheses don't change what reports the condition.
    let Some(e) = arenas.peeled(e.id).and_then(|id| arenas.expr(id)) else {
        return true;
    };
    // A type-check call (`is_int($x)`) reports itself
    // (`function.alreadyNarrowedType`/`function.impossibleType`).
    if let ExprKind::Call(callee, _) = &e.kind
        && let Some(callee) = arenas.expr(*callee)
        && let ExprKind::Name(name) = &callee.kind
        && let Some(name) = arenas.qualified_name(name)
    {
        let name = name.trim_start_matches('\\').to_ascii_lowercase();
        if name == "is_numeric" || crate::checker::narrowing_type_for_predicate(&name).is_some() {
            return true;
        }
    }
    matches!(
        &e.kind,
        ExprKind::Binary(
            BinOp::Eq
                | BinOp::NotEq
                | BinOp::Identical
                | BinOp::NotIdentical
                | BinOp::Gt
                | BinOp::GtEq
                | BinOp::Lt
                | BinOp::LtEq,
            _,
            _,
        ) | ExprKind::Instanceof(_, _)
            | ExprKind::Unary(UnaryOp::Not, _)
            | ExprKind::Binary(
                BinOp::And | BinOp::Or | BinOp::LogicalAnd | BinOp::LogicalOr,
                _,
                _,
            )
            | ExprKind::Ternary(_, _, _)
    )
}

/// Whether `identifier` belongs to this whole "condition provably
/// always true/false" family - every real one this checker ever reports
/// (`if.alwaysTrue`, `booleanAnd.leftAlwaysFalse`, `logicalXor.
/// rightAlwaysFalse`, ...) ends with `true`/`false` case-insensitively
/// and contains `always` somewhere before that, regardless of whether
/// that word starts the identifier's own tag half (`if.alwaysTrue`) or
/// sits mid-camelCase (`booleanAnd.leftAlwaysTrue`) - checked via a
/// lowercased substring/suffix test rather than enumerating every real
/// spelling, so a new sibling rule joining this family later doesn't
/// need a matching update here. Used by `check_trait_methods`'s own
/// divergent-per-host suppression (`function_decl.rs`) - a call-
/// conditioned member of this family is the only shape whose truth can
/// ever actually depend on which host uses a trait; a literal condition
/// (`if (true)`) can never diverge across hosts at all, so restricting
/// to "this family, landed at an identical span with a different
/// message across two host passes" already scopes correctly to the
/// call-conditioned case with no separate call-shape bookkeeping
/// needed. Deliberately excludes non-boolean-truth families that
/// happen to share the word `always` (`arrayFilter.alwaysEmpty`) by
/// also requiring the `true`/`false` suffix.
pub(crate) fn is_constant_condition_family_identifier(identifier: &str) -> bool {
    let lower = identifier.to_ascii_lowercase();
    lower.contains("always") && (lower.ends_with("true") || lower.ends_with("false"))
}
