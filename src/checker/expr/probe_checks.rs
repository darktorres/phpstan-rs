use crate::ast::*;
use crate::checker::expr::ranges::{
    NullProbeVerdictFn, compare_range_outcome, expr_reads_a_property, loose_compare_literals,
    numeric_comparison_bounds,
};
use crate::checker::{
    Checker, Locals, array_value_type_for_key, is_superglobal, literal_string_key,
    narrow_array_shape_by_key_presence,
};
use crate::token::Span;
use crate::types::Type;

impl<'a> Checker<'a> {
    /// Looks up a bare global-constant reference (`ExprKind::Name`,
    /// already excluding `true`/`false`/`null` - see `bare_name_literal_
    /// type`'s own doc comment) against `Registry::global_constants`.
    /// Constants share the identical "check the current namespace first,
    /// fall back to the global namespace for an unqualified reference"
    /// resolution rule real PHP functions do (unlike class names, which
    /// never fall back) - `resolve_function_candidates` already
    /// implements exactly that rule, so it's reused here rather than
    /// duplicated. `None` for anything not in the table (an engine-
    /// provided constant like `PHP_EOL`, or a `const`/`define()` whose
    /// own value wasn't a plain literal - see `Registry::global_
    /// constants`'s own doc comment for the exact scope), leaving
    /// `bare_name_literal_type`'s existing `Mixed` fallback in place.
    pub(crate) fn global_constant_type(&self, name: &str) -> Option<Type> {
        let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
        // `PHP_VERSION_ID`'s own vendored `phpstorm-stubs` declaration
        // (`Core/Core_d.php`) is a literal `define('PHP_VERSION_ID',
        // 50306)` - phpstorm-stubs' own generation environment's actual
        // PHP version at the time it was captured, baked in as if it
        // were a real, permanent constant value. Trusting that as this
        // constant's own type would make `PHP_VERSION_ID >= 80300`-style
        // portability checks (common throughout the real vendor
        // ecosystem) look provably, unconditionally false everywhere,
        // the moment anything consumed the exact literal value for a
        // "provably constant" purpose - this project's own
        // `--php-version`/`php_version` config key (`php_version.rs`)
        // exists for exactly this ambiguity, and is the right source of
        // truth here instead: an `IntRange` spanning every possible
        // patch digit (0-99, real PHP's own patch component, never
        // modeled beyond major.minor) when configured, or a fully
        // unbounded `Type::Int` when it isn't.
        if primary == "PHP_VERSION_ID" || fallback.as_deref() == Some("PHP_VERSION_ID") {
            return Some(match self.registry.php_version {
                Some((major, minor)) => {
                    let base = major as i64 * 10000 + minor as i64 * 100;
                    Type::IntRange(Some(base), Some(base + 99))
                }
                None => Type::Int,
            });
        }
        self.registry
            .global_constants
            .get(&primary)
            .or_else(|| {
                fallback
                    .as_ref()
                    .and_then(|f| self.registry.global_constants.get(f))
            })
            .cloned()
    }

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
        label: &str,
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
        if self.static_var_names.contains(name) {
            return;
        }
        let Some(ty) = locals.get(name) else {
            return;
        };
        let Some(verdict) = verdict_fn(ty) else {
            return;
        };
        self.report(
            label.to_string(),
            format!("Variable ${name} {operator_description} always exists and {verdict}."),
            e.span,
            0,
            &format!("{identifier_prefix}.variable"),
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
        label: &str,
        identifier_prefix: &str,
        operator_description: &str,
    ) -> Option<Type> {
        match &e.kind {
            ExprKind::Variable(name) => {
                if is_superglobal(name)
                    || self.loop_depth > 0
                    || self.static_var_names.contains(name)
                {
                    return None;
                }
                locals.get(name).cloned()
            }
            ExprKind::Index(base, Some(idx)) => {
                let key = literal_string_key(idx)?;
                let base_ty = self.resolve_always_exists(
                    base,
                    locals,
                    label,
                    identifier_prefix,
                    operator_description,
                )?;
                // See this function's own doc comment for why these two
                // calls, not a bespoke tri-state helper: assuming
                // "present" is impossible means the key can never be
                // there; assuming "absent" is impossible means it's
                // unconditionally there - `narrow_array_shape_by_key_presence`
                // already answers both questions correctly (including
                // through a `Union`, recursively), this just asks it
                // twice rather than duplicating its own logic.
                if narrow_array_shape_by_key_presence(&base_ty, &key, true) == Type::Never {
                    self.report(
                        label.to_string(),
                        format!(
                            "Offset '{key}' on {base_ty} {operator_description} does not exist."
                        ),
                        e.span,
                        0,
                        &format!("{identifier_prefix}.offset"),
                    );
                    return None;
                }
                if narrow_array_shape_by_key_presence(&base_ty, &key, false) == Type::Never {
                    return array_value_type_for_key(&base_ty, &key);
                }
                None
            }
            ExprKind::PropertyAccess(base, prop, _) => {
                let base_ty = self.resolve_always_exists(
                    base,
                    locals,
                    label,
                    identifier_prefix,
                    operator_description,
                )?;
                let (_, sig) = self.find_property_owner(&base_ty, prop)?;
                // A hooked property's own getter can compute anything,
                // unrelated to its own init-state - matches every other
                // `is_virtual`-gated check in this codebase. `!has_default`
                // is Design ##2's own deliberately safe-direction
                // exemption (see `PropertySig::has_default`'s own doc
                // comment): a property with no default *might* genuinely
                // still be uninitialized, so `isset()`/`??` checking it is
                // never treated as redundant here, even when its own
                // declared type is non-nullable.
                if sig.is_virtual || !sig.has_default {
                    return None;
                }
                Some(sig.ty.clone())
            }
            _ => None,
        }
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
        label: &str,
        identifier_prefix: &str,
        operator_description: &str,
        verdict_fn: NullProbeVerdictFn,
    ) {
        if !matches!(&e.kind, ExprKind::Index(..) | ExprKind::PropertyAccess(..)) {
            return;
        }
        let Some(leaf_ty) =
            self.resolve_always_exists(e, locals, label, identifier_prefix, operator_description)
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
                self.report(
                    label.to_string(),
                    format!(
                        "Offset '{key}' on {base_ty} {operator_description} always exists and {verdict}."
                    ),
                    e.span,
                    0,
                    &format!("{identifier_prefix}.offset"),
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
                    .unwrap_or_else(|| base_ty.to_string());
                self.report(
                    label.to_string(),
                    format!(
                        "Property {owner}::${prop} ({leaf_ty}) {operator_description} {verdict}."
                    ),
                    e.span,
                    0,
                    &format!("{identifier_prefix}.property"),
                );
            }
            _ => {}
        }
    }

    /// Real PHPStan's own `ConstantLooseComparisonRule`
    /// (level 4): flags a `==`/`!=` whose outcome is provably constant
    /// from both operands' own literal types - `loose_compare_literals`'s
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
        label: &str,
    ) {
        if self.loop_depth > 0 || expr_reads_a_property(l) || expr_reads_a_property(r) {
            return;
        }
        if !matches!(op, BinOp::Eq | BinOp::NotEq) {
            return;
        }
        let Some(equal) = loose_compare_literals(l_ty, r_ty, self.registry.php_version) else {
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
        self.report(
            label.to_string(),
            format!(
                "Loose comparison using {sigil} between {l_ty} and {r_ty} will always evaluate to {}.",
                if outcome { "true" } else { "false" }
            ),
            span,
            4,
            &format!("{rule_name}.always{}", if outcome { "True" } else { "False" }),
        );
    }

    /// Real PHPStan's own
    /// `NumberComparisonOperatorsConstantConditionRule` (level 4): flags a
    /// `</>/<=/>=` comparison whose outcome is provably constant from
    /// both operands' known int ranges - `numeric_comparison_bounds`/
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
        label: &str,
    ) {
        if self.loop_depth > 0 || expr_reads_a_property(l) || expr_reads_a_property(r) {
            return;
        }
        let rule_name = match op {
            BinOp::Lt => "smaller",
            BinOp::Gt => "greater",
            BinOp::LtEq => "smallerOrEqual",
            BinOp::GtEq => "greaterOrEqual",
            _ => return,
        };
        let Some(l_bounds) = numeric_comparison_bounds(l_ty) else {
            return;
        };
        let Some(r_bounds) = numeric_comparison_bounds(r_ty) else {
            return;
        };
        let Some(outcome) = compare_range_outcome(op, l_bounds, r_bounds) else {
            return;
        };
        let sigil = match op {
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::LtEq => "<=",
            BinOp::GtEq => ">=",
            _ => unreachable!(),
        };
        self.report(
            label.to_string(),
            format!(
                "Comparison operation \"{sigil}\" between {l_ty} and {r_ty} is always {}.",
                if outcome { "true" } else { "false" }
            ),
            span,
            4,
            &format!(
                "{rule_name}.always{}",
                if outcome { "True" } else { "False" }
            ),
        );
    }
}
