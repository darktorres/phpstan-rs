use crate::ast::*;
use crate::checker::expr::type_definitely_excludes_null;
use crate::checker::{
    Checker, Locals, build_matched_shape, int_range_bounds, literal_string_key,
    literal_string_value, normalize_int_range, unwrap_grouping,
};
use crate::regex_groups;
use crate::registry::{AssertTarget, FunctionSig};
use crate::token::Span;
use crate::types::{AccessoryKind, Type};
mod accessory_range;
mod array_shape;
mod assertions;
mod predicates;
mod type_ops;

use accessory_range::{
    comparison_narrowed_bounds, int_comparison_operands, numeric_bounds_for_narrowing,
    upgrade_array_to_list, upgrade_array_to_non_empty, upgrade_string_to_numeric,
    upgrade_string_with_accessory,
};
pub(crate) use accessory_range::{resolve_concat_type, string_type_has_accessory};
pub(crate) use array_shape::{
    array_value_type_for_key, is_unsealed_shape, narrow_array_shape_by_key_presence,
    narrow_type_excluding_falsy,
};
use predicates::preg_match_condition;
pub(crate) use predicates::{
    is_real_variable_key, narrowing_key, narrowing_type_for_predicate, purge_stale_narrowing_for,
};
use type_ops::narrow_instanceof_true;
pub(crate) use type_ops::{narrow_type_excluding, narrow_type_to};

/// `count($x)`'s own operand-order detection for the count-comparison
/// narrowing below - same shape as `int_comparison_operands`, but keyed
/// on "is this side a single-argument `count(...)` call" rather than
/// `narrowing_key` (a bare `count(...)` call isn't itself a narrowable
/// key). Returns the call's own argument and whether `count(...)` was
/// written on the left (`count($x) > 0`) or the right (`0 < count($x)`).
fn count_call_operand<'e>(l: &'e Expr, r: &'e Expr) -> Option<(&'e Expr, bool)> {
    fn as_count_arg(e: &Expr) -> Option<&Expr> {
        if let ExprKind::Call(callee, args) = &e.kind
            && let ExprKind::Name(name) = &callee.kind
            && name.trim_start_matches('\\') == "count"
            && let [single_arg] = args.as_slice()
        {
            Some(&single_arg.value)
        } else {
            None
        }
    }
    as_count_arg(l)
        .map(|a| (a, true))
        .or_else(|| as_count_arg(r).map(|a| (a, false)))
}

/// Flips a comparison operator to the operand-swapped equivalent - `0 <
/// count($x)` narrows the same way `count($x) > 0` already does, once
/// the operator itself is mirrored to match.
fn mirror_comparison(op: &BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Gt => BinOp::Lt,
        BinOp::LtEq => BinOp::GtEq,
        BinOp::GtEq => BinOp::LtEq,
        other => other.clone(),
    }
}

/// The receiver of a nullsafe `?->` property/method chain - `$a` for
/// `$a?->b`/`$a?->b()`. Shared by `condition_alias_shape`/`target`
/// (aliasing a variable assigned from such a chain to its receiver, not
/// the chain's own key) and `apply_equality_narrowing`'s own direct-
/// testing hook (`if ($a?->b() !== null)` with no intermediate
/// variable at all) - a nullsafe chain proven non-null soundly implies
/// its receiver was non-null too, by `?->`'s own short-circuit
/// semantics. A *non*-nullsafe chain returns `None`: `$a->b()` proven
/// non-null says nothing new about `$a` (it was already known non-null,
/// or this expression could never have been reached at all).
pub(super) fn nullsafe_receiver(e: &Expr) -> Option<&Expr> {
    match &e.kind {
        ExprKind::PropertyAccess(base, _, true) | ExprKind::MethodCall(base, _, _, true) => {
            Some(base)
        }
        _ => None,
    }
}

/// The narrow slice of real PHPStan's `ConditionalExpressionHolder`
/// this project builds: whether `rhs` (an assignment's right-hand side) is a recognized
/// narrowable condition shape worth storing as a condition alias for
/// `apply_narrowing` to replay later. Deliberately closed and small -
/// the same four shapes `apply_narrowing` itself already dispatches on
/// directly (`Instanceof`, the four equality operators, a single-arg
/// predicate `Call`, and a bare `Variable` for the plain-alias case,
/// `$y = $x;`) - not attempted: compound `&&`/`||`, `isset()`/array-dim
/// correlation, `match`-arm correlation (the genuinely large part of
/// real PHPStan's own mechanism this project declines to build).
fn condition_alias_shape(rhs: &Expr) -> bool {
    match &rhs.kind {
        ExprKind::Instanceof(..)
        | ExprKind::Binary(BinOp::Identical | BinOp::NotIdentical | BinOp::Eq | BinOp::NotEq, ..)
        | ExprKind::Call(..)
        | ExprKind::Variable(_) => true,
        // A compound `&&`/`||` condition assigned to a flag variable,
        // then reused later (`$flag = $a === null || $a === 0; if (!
        // $flag) { use $a; }`, or spanning several distinct subjects
        // entirely, `$flag = ! $isUserFound || $email === null || $email
        // === '';`) - storing the whole compound expression needs no new
        // replay logic at all: `apply_narrowing`'s own existing
        // `BinOp::And if branch_true`/`BinOp::Or if !branch_true` arms
        // already recurse into each side independently (the case with a
        // simple sequential-apply equivalent - "every conjunct/negated-
        // disjunct must hold"), which is exactly the branch direction a
        // stored flag's replay needs. `condition_alias_targets` below
        // tracks every distinct subject named anywhere in the tree (not
        // just one shared target), so a rebind of *any* of them still
        // correctly invalidates the whole alias.
        ExprKind::Binary(BinOp::And | BinOp::Or, l, r) => {
            condition_alias_shape(l) && condition_alias_shape(r)
        }
        // `! $isUserFound` as one leaf of a larger `||`/`&&` tree (`!
        // $isUserFound || $email === null`) - negation doesn't add or
        // remove a subject, just flips what a `true`/`false` replay
        // means, which `apply_narrowing`'s own `Unary(Not, ...)` arm
        // already re-derives correctly by recursing with `!branch_true`
        // when the stored alias expression is replayed wholesale.
        ExprKind::Unary(UnaryOp::Not, inner) => condition_alias_shape(inner),
        // `$x = $a?->b();` - proving `$x` non-null later should also
        // narrow `$a` (`nullsafe_receiver`'s own doc comment); storing
        // the whole chain as `$x`'s alias, targeted at `$a`'s own key
        // rather than `$x->b`'s, is what `condition_alias_targets` below
        // does.
        _ if nullsafe_receiver(rhs).is_some() => true,
        _ => false,
    }
}

/// The canonical key `record_branch_implied_facts`/`apply_equality_
/// narrowing`'s own replay hook both key `Locals::branch_implied_facts`
/// by - a discriminant subject (already reduced to its own
/// `narrowing_key` string) paired with the literal/enum-case-like value
/// it's compared against. `Type`'s own `Debug` output is used rather
/// than a hand-rolled canonicalization: every value shape this applies
/// to (an enum case, a literal) already has a stable, span-free `Debug`
/// representation, and two independently-parsed occurrences of the
/// identical comparison always resolve to `Type`-equal values here.
pub(super) fn discriminant_canonical_key(subject_key: &str, value: &Type) -> String {
    format!("{subject_key}=={value:?}")
}

/// The "is this the same propositional atom" identity disjunctive-
/// syllogism narrowing needs (`Checker::pending_or_disjunctions`) -
/// deliberately restricted to a bare truthiness atom (a variable, or
/// `(bool) $variable`, the corpus-confirmed shape), never an equality
/// comparison: two equality comparisons against *different* literals
/// (`$a === 1`/`$a === 2`) would otherwise wrongly collapse to "the
/// same atom" under a naive `narrowing_key`-only comparison, since
/// `narrowing_key` only ever identifies the *subject*, not the value
/// compared against.
fn disjunctive_syllogism_atom_key(e: &Expr) -> Option<String> {
    let inner = match &e.kind {
        ExprKind::Cast(kind, inner) if kind == "bool" => inner,
        ExprKind::Grouping(inner) => inner,
        _ => e,
    };
    match &inner.kind {
        ExprKind::Variable(_) => narrowing_key(inner),
        _ => None,
    }
}

/// A condition recognized well enough to canonicalize as a bare-
/// truthiness discriminant, for `record_branch_implied_facts`'s own
/// same-condition-recurrence case (Phase 170) - a bare variable (the
/// ternary/flag-reuse shape, `$nbmSendDetailedContent ? $news : null`
/// reusing the identical `$nbmSendDetailedContent`'s own earlier `if`)
/// or a single-argument `isset(...)` call (`UploadService::
/// addUploadedFile`'s own `isset($image_id)` re-checked a second time).
/// Deliberately narrow - not every truthiness-testable shape, just the
/// two corpus-confirmed ones - matching this project's "don't guess"
/// precedent throughout `narrowing.rs` rather than a general condition
/// canonicalizer.
fn truthiness_discriminant_subject(cond: &Expr) -> Option<String> {
    match &cond.kind {
        ExprKind::Variable(_) => narrowing_key(cond),
        ExprKind::Call(callee, args) => {
            let ExprKind::Name(name) = &callee.kind else {
                return None;
            };
            let [single_arg] = args.as_slice() else {
                return None;
            };
            let key = narrowing_key(&single_arg.value)?;
            (name.trim_start_matches('\\') == "isset").then(|| format!("isset({key})"))
        }
        _ => None,
    }
}

impl<'a> Checker<'a> {
    /// Disjunctive syllogism: `if ($a || $b) { if ($b) {} else { /*
    /// here, $a must be true */ } }` - called wherever a nested `if`'s
    /// own condition is proven *false*, checks whether that condition
    /// matches one atom of any currently-pending outer `||` (`Checker::
    /// pending_or_disjunctions`, pushed by `check_if` for the duration
    /// of an outer `Binary(Or, ..)` condition's own true branch); if so,
    /// the *other* disjunct is asserted true via the identical
    /// `apply_narrowing` recursion any other proven-true condition
    /// already uses - a real, sound inference, not a new narrowing rule
    /// of its own.
    pub(crate) fn apply_disjunctive_syllogism(&self, cond: &Expr, locals: &mut Locals) {
        let Some(cond_key) = disjunctive_syllogism_atom_key(cond) else {
            return;
        };
        for (l, r) in self.pending_or_disjunctions.iter().rev() {
            if disjunctive_syllogism_atom_key(l).as_deref() == Some(cond_key.as_str()) {
                self.apply_narrowing(r, locals, true);
            } else if disjunctive_syllogism_atom_key(r).as_deref() == Some(cond_key.as_str()) {
                self.apply_narrowing(l, locals, true);
            }
        }
    }

    /// The replay half of `record_branch_implied_facts`'s bare-
    /// truthiness discriminant shape - `subject` is the same canonical
    /// string `truthiness_discriminant_subject` would have produced for
    /// the condition currently being narrowed (a bare variable's own
    /// name, or `"isset(key)"`). Looked up under whichever of `Type::
    /// True`/`Type::False` matches `branch_true`, mirroring how the
    /// equality discriminant's own replay (`apply_equality_narrowing`)
    /// looks itself up by `(subject, value)`.
    pub(crate) fn replay_truthiness_discriminant_facts(
        &self,
        subject: &str,
        locals: &mut Locals,
        branch_true: bool,
    ) {
        let marker = if branch_true { Type::True } else { Type::False };
        if let Some(implied) = locals
            .branch_implied_facts
            .get(&discriminant_canonical_key(subject, &marker))
            .cloned()
        {
            for (implied_key, implied_ty) in implied.iter() {
                locals.insert(implied_key.clone(), implied_ty.clone());
            }
        }
    }

    /// Records a same-branch discriminant-variable correlation for one
    /// branch (`branch_true` - which one `after` is the checked result
    /// of): when checking that branch caused some *other* variable to
    /// become provably non-null (from nullable/absent/`Mixed` before),
    /// stores that as a durable fact under a discriminant key - replayed
    /// the next time the identical discriminant is tested with the same
    /// polarity anywhere else in the program, even with no lexical
    /// relationship to this `if` at all. Two recognized discriminant
    /// shapes, each with its own scoping: `$subject === value` (or
    /// `==`), scoped to the *true* branch only (an equality's false
    /// branch says nothing reusable - "not equal to X" isn't one fixed
    /// identity to key facts under) and a value comparably concrete
    /// enough to make "the identical comparison" reproducible (an enum
    /// case, a literal); and a bare truthiness discriminant
    /// (`truthiness_discriminant_subject`), keyed by `Type::True`/
    /// `Type::False` as `discriminant_canonical_key`'s own "value" half.
    /// The truthiness shape is safe for either branch, unlike the
    /// equality shape, since truthy/falsy are the whole space of a
    /// boolean condition's own outcomes, not one value out of many.
    pub(crate) fn record_branch_implied_facts(
        &self,
        cond: &Expr,
        before: &Locals,
        after: &mut Locals,
        branch_true: bool,
    ) {
        let subject = if branch_true
            && let ExprKind::Binary(BinOp::Identical | BinOp::Eq, l, r) = &cond.kind
        {
            match (narrowing_key(l), narrowing_key(r)) {
                (Some(key), _) => Some((key, self.infer_expr(r, before))),
                (None, Some(key)) => Some((key, self.infer_expr(l, before))),
                (None, None) => None,
            }
            .filter(|(_, ty)| {
                matches!(
                    ty,
                    Type::EnumCase(..) | Type::IntLiteral(_) | Type::StringLiteral(_)
                )
            })
        } else {
            truthiness_discriminant_subject(cond)
                .map(|key| (key, if branch_true { Type::True } else { Type::False }))
        };
        let Some((subject_key, value_ty)) = subject else {
            return;
        };
        let mut implied: Vec<(String, Type)> = Vec::new();
        for (name, after_ty) in after.iter() {
            if *name == subject_key {
                continue;
            }
            if type_definitely_excludes_null(after_ty)
                && *after_ty != Type::Mixed
                && !before.get(name).is_some_and(type_definitely_excludes_null)
            {
                implied.push((name.clone(), after_ty.clone()));
            }
        }
        if implied.is_empty() {
            return;
        }
        let key = discriminant_canonical_key(&subject_key, &value_ty);
        let mut combined = after
            .branch_implied_facts
            .get(&key)
            .map(|existing| (**existing).clone())
            .unwrap_or_default();
        combined.extend(implied);
        after
            .branch_implied_facts
            .insert(key, std::rc::Rc::new(combined));
    }
}

/// The thing(s) a condition-alias-eligible `rhs` actually narrows -
/// reuses `narrowing_key` exactly the way `apply_equality_narrowing`
/// already does for the identical operand-order-agnostic lookup. A
/// compound `&&`/`||` tree collects every distinct subject named by any
/// leaf, not just a single shared one (`! $isUserFound || $email ===
/// null || $email === ''` targets both `isUserFound` and `email`) -
/// `Locals::declare` purges the whole alias when *any* of them is
/// rebound, so tracking every real subject, not just one, is what keeps
/// that invalidation sound. `None` for a shape `condition_alias_shape`
/// accepted but whose own "narrowed thing" isn't `narrowing_key`-
/// representable (most `Call`/`MethodCall` targets), or whose own `And`/
/// `Or` tree has a leaf that itself resolves to no target at all - a
/// real, disclosed leniency: such a condition simply never gets stored
/// as an alias, matching this project's "don't guess, don't false-
/// positive" precedent throughout `narrowing.rs`, not a false
/// conclusion.
fn condition_alias_targets(rhs: &Expr) -> Option<Vec<String>> {
    match &rhs.kind {
        ExprKind::Instanceof(inner, _) => Some(vec![narrowing_key(inner)?]),
        ExprKind::Binary(
            BinOp::Identical | BinOp::NotIdentical | BinOp::Eq | BinOp::NotEq,
            l,
            r,
        ) => Some(vec![narrowing_key(l).or_else(|| narrowing_key(r))?]),
        ExprKind::Call(_, args) => match args.as_slice() {
            [single_arg] => Some(vec![narrowing_key(&single_arg.value)?]),
            _ => None,
        },
        ExprKind::Variable(name) => Some(vec![name.clone()]),
        ExprKind::Binary(BinOp::And | BinOp::Or, l, r) => {
            let mut targets = condition_alias_targets(l)?;
            for key in condition_alias_targets(r)? {
                if !targets.contains(&key) {
                    targets.push(key);
                }
            }
            Some(targets)
        }
        ExprKind::Unary(UnaryOp::Not, inner) => condition_alias_targets(inner),
        // Targets the *receiver*'s own key, not the chain's - see
        // `nullsafe_receiver`'s own doc comment for why.
        ExprKind::PropertyAccess(base, _, true) | ExprKind::MethodCall(base, _, _, true) => {
            Some(vec![narrowing_key(base)?])
        }
        _ => None,
    }
}

impl<'a> Checker<'a> {
    /// Narrows `locals` in place based on `cond` being true (`branch_true`)
    /// or false, along one branch of an `if`/`while`. Only handles the
    /// common, safe patterns - equality against a known-type value
    /// (`$x === null`, `null !== $x`), `is_*()` guards, and `instanceof` -
    /// anything else leaves `locals` untouched rather than guess.
    pub(crate) fn apply_narrowing(&self, cond: &Expr, locals: &mut Locals, branch_true: bool) {
        // `preg_match(...)` (bare, or compared against a literal 0/1)
        // doesn't otherwise match any existing narrowing shape below,
        // so this doesn't shadow anything: the `Binary(op, l, r)` arm
        // further down does also run afterward for a comparison form,
        // dispatching to `apply_equality_narrowing`, but that function's
        // own `narrowing_key(l)`/`narrowing_key(r)` calls have no arm for
        // `ExprKind::Call` at all, so it's a genuine no-op for this
        // shape, not a silent double-narrowing.
        if let Some((pattern_arg, matches_arg, true_means_matched)) = preg_match_condition(cond)
            && let ExprKind::Variable(matches_name) = &matches_arg.kind
            && let Some(pattern_str) = literal_string_value(pattern_arg)
            && let Some(parsed) = regex_groups::parse_capture_groups(&pattern_str)
        {
            let narrowed = if branch_true == true_means_matched {
                build_matched_shape(
                    &parsed.groups,
                    &parsed.alternation_branch_counts,
                    &parsed.alternation_wrapped_in_optional_quantifier,
                    &Type::String,
                    false,
                )
            } else {
                Type::ArrayShape(vec![])
            };
            purge_stale_narrowing_for(locals, matches_name);
            locals.insert(matches_name.clone(), narrowed);
        }
        // `function_exists('foo')`'s own true branch proves `foo` is
        // callable for the rest of that branch - a project-specific
        // adaptation (this project's own registry, unlike real PHPStan's
        // live reflection, can genuinely lack a symbol that still exists
        // at runtime). Only the exact literal-string-argument shape is recognized - a
        // computed argument never populates the guard, matching this
        // project's usual "don't guess" leniency. The `&&` arm below
        // already recurses into this for free (`if (function_exists('foo')
        // && ...)`).
        if branch_true
            && let ExprKind::Call(callee, args) = &unwrap_grouping(cond).kind
            && let ExprKind::Name(fn_name) = &unwrap_grouping(callee).kind
            && fn_name
                .rsplit('\\')
                .next()
                .unwrap_or(fn_name)
                .eq_ignore_ascii_case("function_exists")
            && let [guarded_arg] = args.as_slice()
            && let Some(guarded_name) = literal_string_value(&guarded_arg.value)
        {
            locals
                .guarded_functions
                .insert(guarded_name.trim_start_matches('\\').to_ascii_lowercase());
        }
        // Real PHPStan's own `MethodExistsTypeSpecifyingExtension`/
        // `PropertyExistsTypeSpecifyingExtension` (`Accessory\
        // HasMethodType`/`HasPropertyType`) - a `method_exists($recv,
        // 'name')`/`property_exists($recv, 'name')` true branch proves
        // `$recv->name(...)`/`$recv->name` is safe for the rest of that
        // branch, regardless of `$recv`'s own static type. Mirrors
        // `guarded_functions`'s own design exactly, one level down (a
        // per-receiver fact instead of a global one): only the exact
        // (bare variable, literal string) shape is recognized.
        if branch_true
            && let ExprKind::Call(callee, args) = &unwrap_grouping(cond).kind
            && let ExprKind::Name(fn_name) = &unwrap_grouping(callee).kind
            && let [recv_arg, name_arg] = args.as_slice()
            && let ExprKind::Variable(recv_name) = &unwrap_grouping(&recv_arg.value).kind
            && let Some(member_name) = literal_string_value(&name_arg.value)
        {
            let bare = fn_name.rsplit('\\').next().unwrap_or(fn_name);
            if bare.eq_ignore_ascii_case("method_exists") {
                locals
                    .guarded_methods
                    .insert((recv_name.clone(), member_name.into_owned()));
            } else if bare.eq_ignore_ascii_case("property_exists") {
                locals
                    .guarded_properties
                    .insert((recv_name.clone(), member_name.into_owned()));
            }
        }
        match &cond.kind {
            // `&&` narrows the "both true" branch by composing each side
            // in order (the right operand's own narrowing can rely on the
            // left's, same as check_expr's short-circuit-aware handling)
            // - the false branch would need "at least one is false, but
            // not sure which", which isn't a narrowing this can express,
            // so it's left alone, same conservative choice `||` mirrors
            // for its own unrepresentable branch below.
            ExprKind::Binary(BinOp::And, l, r) if branch_true => {
                self.apply_narrowing(l, locals, true);
                self.apply_narrowing(r, locals, true);
            }
            ExprKind::Binary(BinOp::Or, l, r) if !branch_true => {
                self.apply_narrowing(l, locals, false);
                self.apply_narrowing(r, locals, false);
            }
            // "At least one is true, but not sure
            // which" isn't representable in general - but when every
            // operand is a direct predicate-table check on the identical
            // expression, it narrows precisely to the union of what each
            // operand would give alone (`is_int($x) || is_string($x)`
            // being true means `$x` is exactly `int|string`, not "give
            // up"). `or_chain_narrowing_target` is the one doing the
            // actual restriction-checking; this arm only acts when it
            // succeeds.
            ExprKind::Binary(BinOp::Or, ..) if branch_true => {
                if let Some((key, target_ty, repr_expr)) =
                    self.or_chain_narrowing_target(cond, locals)
                {
                    let current = self.current_narrowed_type(repr_expr, &key, locals);
                    locals.insert(key, narrow_type_to(self.registry, &current, &target_ty));
                }
            }
            // `if ((! is_int($a) && ! is_string($a)) || (!
            // is_int($b) && ! is_string($b)) || ...) { continue; }`
            // narrows several different variables at once through one
            // combined condition. The `Or ... if !branch_true` arm just
            // above already decomposes an arbitrarily-long `||` chain
            // into independent per-leaf `apply_narrowing` calls (De
            // Morgan on the outer `||`) - each landing here, on exactly
            // one `&&`-of-negated-predicates leaf, one variable at a
            // time. `&&`'s own false branch finishes the same De Morgan
            // step one level down: `!(!is_int($v) && !is_string($v))`
            // is `is_int($v) || is_string($v)`, which
            // `and_chain_narrowing_target` computes by reusing
            // `or_chain_narrowing_target` on each un-negated leaf
            // directly (double negation), then this arm narrows exactly
            // the way the sibling `Or ... if branch_true` arm above
            // already does for a literal `||`. Every leaf here is
            // independent (a different key each), so each And-term
            // simply lands on its own call to this same arm, mutating
            // the one shared `locals` for its own key only.
            ExprKind::Binary(BinOp::And, ..) if !branch_true => {
                if let Some((key, target_ty, repr_expr)) =
                    self.and_chain_narrowing_target(cond, locals)
                {
                    let current = self.current_narrowed_type(repr_expr, &key, locals);
                    locals.insert(key, narrow_type_to(self.registry, &current, &target_ty));
                }
            }
            // Deliberately `===`/`!==` only, not `==`/`!=` - a loose
            // comparison matches PHP's own type-juggling table (`0 ==
            // false`, `0 == null`, `"0" == 0`, ...), a much wider set of
            // values than the literal compared-to one, so narrowing a
            // variable down to exactly that value on a loose match would
            // discard every other value `==` could also have matched.
            ExprKind::Binary(op, l, r) => {
                let is_eq = matches!(op, BinOp::Identical);
                let is_neq = matches!(op, BinOp::NotIdentical);
                if is_eq || is_neq {
                    let matches_when_true = if is_eq { branch_true } else { !branch_true };
                    self.apply_equality_narrowing(l, r, locals, matches_when_true);
                } else if let Some((count_arg, count_on_left)) = count_call_operand(l, r)
                    && let Some(key) = narrowing_key(count_arg)
                    && let bound_expr = if count_on_left { r } else { l }
                    && let effective_op = if count_on_left {
                        op.clone()
                    } else {
                        mirror_comparison(op)
                    }
                    && let ExprKind::Literal(Literal::Int(n)) = &bound_expr.kind
                    && ((branch_true
                        && ((matches!(effective_op, BinOp::Gt) && *n == 0)
                            || (matches!(effective_op, BinOp::GtEq) && *n == 1)))
                        // `if (count($x) < 1) { $this->ierror(...); }`
                        // (a `never`-typed early-exit idiom) proves the
                        // exact same "`$x` is non-empty" fact on the
                        // surviving path, just phrased as the false
                        // branch of the mirrored comparison
                        // (`count($x) < 1` false, or `count($x) <= 0`
                        // false, both mean `count($x) >= 1`) - the
                        // reversed-operand form (`1 > count($x)`) is
                        // handled too, via `count_call_operand`/
                        // `mirror_comparison` above.
                        || (!branch_true
                            && ((matches!(effective_op, BinOp::Lt) && *n == 1)
                                || (matches!(effective_op, BinOp::LtEq) && *n == 0))))
                {
                    // `count($x) > 0`/`count($x) >=
                    // 1` proves `$x` non-empty - handles both operand
                    // orders (`count($x) > 0` and `0 < count($x)`), via
                    // `count_call_operand`'s own left/right detection and
                    // `mirror_comparison` canonicalizing the operator back
                    // to the `count(...)`-on-the-left shape below.
                    let current = self.current_narrowed_type(count_arg, &key, locals);
                    locals.insert(key, upgrade_array_to_non_empty(&current));
                } else if matches!(op, BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq)
                    && let Some((keyed_expr, bound_expr, keyed_on_left)) =
                        int_comparison_operands(l, r)
                    && let Some(key) = narrowing_key(keyed_expr)
                {
                    // `$x < N`/`$x >= N`/... (either
                    // operand order) narrows `$x`'s own int range - scoped
                    // directly against real PHPStan's own `IntegerRangeType
                    // ::getSmallerType`/`getGreaterType` family
                    // (`vendor/phpstan-src`), deliberately simplified: real
                    // PHPStan's version also has to subtract `true`/`null`/
                    // `false` per PHP's own loose-comparison rules for `<`/
                    // `>`, which this project doesn't model (same "no loose-
                    // comparison modeling" precedent the `===`/`!==`-only
                    // equality arm above already holds) - so this only acts
                    // when both the narrowed side's *current* type and the
                    // other side's *inferred* type are already unambiguously
                    // numeric (a bare `Type::Int` counts as an unbounded
                    // range here, unlike `int_range_bounds` itself), and the
                    // other side is a single literal value, not a genuine
                    // range (matching the `count() > 0` arm's own "exactly
                    // this shape" scoping just above, not general range-vs-
                    // range subtraction).
                    let current = self.current_narrowed_type(keyed_expr, &key, locals);
                    if let Some((cur_min, cur_max)) = numeric_bounds_for_narrowing(&current) {
                        let bound_ty = self.infer_expr(bound_expr, locals);
                        if let Some((Some(bound), Some(bound_max))) = int_range_bounds(&bound_ty)
                            && bound == bound_max
                            && let Some((new_min, new_max)) = comparison_narrowed_bounds(
                                op,
                                keyed_on_left,
                                branch_true,
                                cur_min,
                                cur_max,
                                bound,
                            )
                        {
                            locals.insert(key, normalize_int_range(new_min, new_max));
                        }
                    }
                }
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                self.apply_narrowing(inner, locals, !branch_true);
            }
            ExprKind::Grouping(inner) => self.apply_narrowing(inner, locals, branch_true),
            // `(bool) $x`'s own truthiness is exactly `$x`'s -
            // PHP's cast doesn't change which values are truthy/falsy, it
            // just makes the check explicit (a common, idiomatic style:
            // `if (! (bool) $fp) { throw ...; }` after `fopen()`) - so
            // this narrows precisely as if the cast weren't there at all.
            // Any other cast (`(int)`, `(string)`, ...) genuinely changes
            // the value and isn't narrowable this way, so it's left alone.
            ExprKind::Cast(kind, inner) if kind == "bool" => {
                self.apply_narrowing(inner, locals, branch_true);
            }
            ExprKind::Call(callee, args) => {
                // `narrowing_key`, not a bare `ExprKind::
                // Variable` match - without this, `is_string($arr['key'])`
                // (or `empty($arr['key'])`) never narrowed `$arr['key']`
                // at all, the array-index counterpart of the exact gap
                // the `instanceof` arm below already covers for property
                // chains (`$arg->value instanceof Variable`).
                if let (ExprKind::Name(name), [single_arg]) = (&callee.kind, args.as_slice())
                    && let Some(key) = narrowing_key(&single_arg.value)
                {
                    // A fully-qualified call (`\is_array($x)`,
                    // `\count($x)`, ...) keeps its leading backslash in
                    // `ExprKind::Name` - unlike ordinary call resolution
                    // (`name_ctx::resolve_function_candidates`, used for
                    // every real argument/arity check), every name
                    // comparison in this whole match arm is a bare
                    // string equality with no normalization, so a
                    // common defensive-global-namespace style (e.g.
                    // `\is_array($value)` nested inside an
                    // `is_iterable($value)` guard) needs stripping the
                    // backslash first.
                    let name = name.trim_start_matches('\\');
                    if name == "empty" {
                        // `empty($x)` is PHP's own falsy check - only the
                        // branch where it evaluated `false` (`$x` proven
                        // truthy) is safely narrowable, the same asymmetry
                        // as a bare truthiness check.
                        if !branch_true {
                            let current =
                                self.current_narrowed_type(&single_arg.value, &key, locals);
                            locals.insert(key, narrow_type_excluding_falsy(&current));
                        }
                    } else if name == "isset" {
                        // `isset($x)`'s truthy branch means
                        // specifically "not null" - it's true for `0`,
                        // `''`, `false`, an empty array, every falsy-but-
                        // non-null value - so this narrows only `Null`
                        // away, unlike `empty()`'s full falsy exclusion
                        // above. The false branch isn't safely narrowable
                        // to anything more specific, the same asymmetry
                        // `empty`/the truthy fallback below already accept.
                        //
                        // Same-condition-recurrence replay
                        // (`truthiness_discriminant_subject`'s own doc
                        // comment): the identical `isset($x)` re-checked
                        // later, even in a fresh, lexically-unrelated
                        // `if`/`else`, replays whatever the first
                        // occurrence's own matching branch already
                        // learned - both branches, unlike the narrowing
                        // just below, which is truthy-only.
                        self.replay_truthiness_discriminant_facts(
                            &format!("isset({key})"),
                            locals,
                            branch_true,
                        );
                        if branch_true {
                            let current =
                                self.current_narrowed_type(&single_arg.value, &key, locals);
                            locals.insert(key, narrow_type_excluding(&current, &Type::Null));
                        }
                    } else if name == "count" && branch_true {
                        // A bare `if (count($x))`/`while
                        // (count($x))` truthiness check, not the `> 0`/
                        // `>= 1` comparison form the `Binary` arm above
                        // already handles - `count()` is never negative,
                        // so a truthy result means exactly the same
                        // "provably non-empty" fact. The false branch
                        // (`count($x) === 0`) isn't narrowed here, the
                        // same asymmetry the comparison form's own arm
                        // already accepts (`upgrade_array_to_non_empty`
                        // has no "prove empty" counterpart).
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        locals.insert(key, upgrade_array_to_non_empty(&current));
                    } else if name == "array_is_list" && branch_true {
                        // `is_array($x) && array_is_list($x)` (a common
                        // assertion-library idiom) proves list-ness, the
                        // same shape `count($x)`'s own truthiness
                        // upgrade just above already covers for
                        // non-emptiness. Only the true branch is
                        // narrowable - failing `array_is_list()` proves
                        // nothing more specific than "not proven a list",
                        // the same asymmetry `count`/`empty` already
                        // accept above.
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        locals.insert(key, upgrade_array_to_list(&current));
                    } else if name == "is_numeric" {
                        // Deliberately its own arm,
                        // not a `narrowing_type_for_predicate` table entry
                        // - that table's single target type feeds *both*
                        // directions through the same dispatch below,
                        // which is sound for `is_scalar`/`is_string`/etc.
                        // (a string is unconditionally scalar/a-string
                        // either way) but not for `is_numeric`, where a
                        // string only *sometimes* qualifies: excluding a
                        // shared "int|float|string" target on the false
                        // branch would wrongly drop `String` entirely,
                        // even though a genuinely non-numeric string
                        // (`"hello"`) is exactly the ordinary case that
                        // makes `!is_numeric` true. The two directions
                        // need different exclusion sets instead.
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        let narrowed = if branch_true {
                            // Bool/True/False/Null can never satisfy
                            // is_numeric - unconditionally safe to drop.
                            // A kept String (bare, or inside the
                            // resulting Union) is upgraded to
                            // numeric-string, matching real PHPStan:
                            // `string $x` under is_numeric($x) narrows
                            // to plain numeric-string, `mixed $x`
                            // narrows to int|float|numeric-string.
                            let excluded = [Type::Bool, Type::True, Type::False, Type::Null]
                                .into_iter()
                                .fold(current, |acc, excluded| {
                                    narrow_type_excluding(&acc, &excluded)
                                });
                            upgrade_string_to_numeric(&excluded)
                        } else {
                            // Int/Float are unconditionally numeric, so
                            // failing is_numeric proves the value wasn't
                            // one - nothing else can be dropped this way.
                            [Type::Int, Type::Float]
                                .into_iter()
                                .fold(current, |acc, excluded| {
                                    narrow_type_excluding(&acc, &excluded)
                                })
                        };
                        locals.insert(key, narrowed);
                    } else if let Some(target_ty) = narrowing_type_for_predicate(name) {
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        let narrowed = if branch_true {
                            narrow_type_to(self.registry, &current, &target_ty)
                        } else {
                            narrow_type_excluding(&current, &target_ty)
                        };
                        locals.insert(key, narrowed);
                    }
                } else if let ExprKind::Name(name) = &callee.kind
                    && name.trim_start_matches('\\') == "in_array"
                    && let [needle, haystack, strict] = args.as_slice()
                    // Only the strict, literal-array
                    // form real PHPStan's own `InArrayFunctionTypeSpecifyingExtension`
                    // narrows - a loose comparison's type-juggling would
                    // make the real safe target wider than the literal
                    // values listed here.
                    && matches!(&strict.value.kind, ExprKind::Literal(Literal::Bool(true)))
                    && let ExprKind::ArrayLiteral(items) = &haystack.value.kind
                    && let Some(target_ty) = self.in_array_target_type(items, locals)
                    && let Some(key) = narrowing_key(&needle.value)
                {
                    let current = self.current_narrowed_type(&needle.value, &key, locals);
                    let narrowed = if branch_true {
                        narrow_type_to(self.registry, &current, &target_ty)
                    } else {
                        narrow_type_excluding(&current, &target_ty)
                    };
                    locals.insert(key, narrowed);
                } else if let ExprKind::Name(name) = &callee.kind
                    && name.trim_start_matches('\\') == "isset"
                    && branch_true
                    && args.len() > 1
                {
                    // Real PHP's `isset()` is genuinely variadic
                    // (`isset($a, $b, ...)`, equivalent to `isset($a) &&
                    // isset($b) && ...` - *every* argument must be set) -
                    // the single-arg dispatch above's own `[single_arg]`
                    // slice pattern never matches here at all, silently
                    // narrowing none of them, not just the ones past
                    // the first. Only the true branch is narrowable, the
                    // same asymmetry the single-arg case above already
                    // has (a multi-arg `isset()` being false doesn't say
                    // which argument was unset).
                    for arg in args {
                        if let Some(key) = narrowing_key(&arg.value) {
                            let current = self.current_narrowed_type(&arg.value, &key, locals);
                            locals.insert(key, narrow_type_excluding(&current, &Type::Null));
                        }
                    }
                } else if let ExprKind::Name(name) = &callee.kind
                    && name.trim_start_matches('\\') == "array_key_exists"
                    && let [key_arg, arr_arg] = args.as_slice()
                    // `literal_string_key` (not a bare `Literal::String`
                    // match) also accepts an int literal, stringifying it
                    // the same way an `ArrayShape`'s own field keys already
                    // are internally - without this, `array_key_exists(0,
                    // $arr)` (a common tuple/numeric-shape check, as
                    // common as the string-key form) would never trigger
                    // this narrowing, silently keeping a `Union` member
                    // that couldn't possibly have offset `0` at all.
                    && let Some(lit_key) = literal_string_key(&key_arg.value)
                    && let Some(arr_key) = narrowing_key(&arr_arg.value)
                {
                    // The checked expression here is
                    // the array argument, not the key - proving/disproving
                    // whether a specific offset is present lets a `Union`
                    // member that could never make the call come out this
                    // way be dropped, the same "keep/drop a Union member
                    // based on a provable fact" shape `instanceof`'s own
                    // member resolution already uses.
                    let current = self.current_narrowed_type(&arr_arg.value, &arr_key, locals);
                    let narrowed =
                        narrow_array_shape_by_key_presence(&current, &lit_key, branch_true);
                    // Also narrow the specific element
                    // access itself (`$arr['key']`), not just `$arr`'s
                    // own overall type - see `array_value_type_for_key`'s
                    // own doc comment for why this project's own
                    // composite `narrowing_key` mechanism is the right,
                    // proportionate stand-in for real PHPStan's
                    // `HasOffsetValueType`. Only the true branch: a
                    // *missing* key has no value type to narrow to at
                    // all (real PHP itself throws a warning and yields
                    // `null` on read, a fact this project doesn't model
                    // here - `isset`'s own narrowing already owns the
                    // "exclude `Null`" half of that for the false
                    // branch, a different, already-covered concern).
                    if branch_true
                        && let Some(value_ty) = array_value_type_for_key(&narrowed, &lit_key)
                    {
                        locals.insert(format!("{arr_key}[{lit_key}]"), value_ty);
                    }
                    locals.insert(arr_key, narrowed);
                } else if let ExprKind::Name(name) = &callee.kind
                    && matches!(
                        name.trim_start_matches('\\'),
                        "str_contains" | "str_starts_with" | "str_ends_with"
                    )
                    && branch_true
                    && let [haystack, needle] = args.as_slice()
                    && let Some(key) = narrowing_key(&haystack.value)
                {
                    // Real PHPStan's own
                    // `StrContainingTypeSpecifyingExtension` - a haystack
                    // that provably contains/starts/ends with a non-empty
                    // literal needle can't itself be empty. Only fires for
                    // a needle this checker can prove is a non-empty
                    // string literal (matching real PHPStan's own
                    // `isNonEmptyString()` gate) - a variable/computed
                    // needle proves nothing. Only the true branch is
                    // narrowable (mirrors `isFunctionSupported`'s own
                    // `$context->true()` restriction) - failing any of
                    // these three proves nothing about the haystack's own
                    // emptiness. `strpos`/`stripos`/`fnmatch`/`mb_*`
                    // variants (also in real PHPStan's own table) are
                    // deliberately left out - their `int|false` (or
                    // `false`-on-no-match) return makes a bare truthy
                    // `if (strpos(...))` a real, separate PHP footgun
                    // (position `0` is falsy despite being a match) this
                    // checker doesn't otherwise model, unlike these three
                    // genuinely bool-returning functions.
                    if let Type::StringLiteral(needle_lit) = self.infer_expr(&needle.value, locals)
                        && !needle_lit.is_empty()
                    {
                        let kind = if needle_lit == "0" {
                            AccessoryKind::NonEmptyString
                        } else {
                            AccessoryKind::NonFalsyString
                        };
                        let current = self.current_narrowed_type(&haystack.value, &key, locals);
                        locals.insert(key, upgrade_string_with_accessory(&current, kind));
                    }
                } else if let ExprKind::Name(name) = &callee.kind
                    && matches!(name.trim_start_matches('\\'), "is_a")
                    && let [obj_arg, class_arg, ..] = args.as_slice()
                    && let Some(key) = narrowing_key(&obj_arg.value)
                {
                    // `is_a($x, SomeClass::class)`/`is_a($x, 'SomeClass')`
                    // is semantically `$x instanceof SomeClass` with its
                    // target spelled as a runtime value instead of native
                    // syntax - reuses the identical `Instanceof` arm's own
                    // narrowing primitives below rather than a new
                    // mechanism. The optional 3rd `$allow_string` argument
                    // doesn't change narrowing at all (only whether a
                    // `class-string` first argument is also accepted), so
                    // it's matched away by the `..` above without being
                    // inspected. A dynamic class-name argument (not a
                    // literal string or `::class` constant) resolves via
                    // `literal_string_value`'s own "give up, don't guess"
                    // `None` the same way a dynamic `instanceof` target
                    // does. `Foo::class` resolves via the identical
                    // `StaticAccess(class_name, "class")` shape
                    // `phpstan_api.rs`/`visibility.rs` already extract a
                    // raw class name from elsewhere, not `literal_string_
                    // value` (which only ever handles a real string
                    // literal/concatenation).
                    let class_name = match &class_arg.value.kind {
                        ExprKind::StaticAccess(name, member) if member == "class" => {
                            Some(name.clone())
                        }
                        _ => literal_string_value(&class_arg.value).map(|s| s.into_owned()),
                    };
                    if let Some(class_name) = class_name {
                        let resolved = self.resolve_name_expr(&class_name);
                        let current = self.current_narrowed_type(&obj_arg.value, &key, locals);
                        if branch_true {
                            locals.insert(key, narrow_instanceof_true(&current, &resolved));
                        } else {
                            locals.insert(key, narrow_type_excluding(&current, &resolved));
                        }
                    }
                } else if let ExprKind::Name(name) = &callee.kind {
                    // A plain function call tagged `@phpstan-assert-if-true`/
                    // `-if-false` on its own docblock - see `FunctionSig::
                    // assert_if_true`'s own doc comment for the full
                    // picture. No `$this`/receiver here, so only a
                    // `Param`-targeted assertion can ever resolve
                    // (`apply_assert_infos` silently skips the rest).
                    let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
                    if let Some(sig) = self.registry.lookup_function(&primary).or_else(|| {
                        fallback
                            .as_deref()
                            .and_then(|g| self.registry.lookup_function(g))
                    }) {
                        let infos = if branch_true {
                            &sig.assert_if_true
                        } else {
                            &sig.assert_if_false
                        };
                        if !infos.is_empty() {
                            self.apply_assert_infos(infos, sig, None, args, locals);
                        }
                    }
                }
            }
            // `@phpstan-assert-if-true`/`-if-false` on a method's own
            // docblock - see `FunctionSig::assert_if_true`'s own doc
            // comment for the full picture. `recv_class` picks a
            // representative member out of a `Union`/`Intersection`
            // receiver the exact same way `checker/expr.rs`'s own
            // `MethodCall` arm already does for ordinary method-call
            // checking - a variable typed as the union of several
            // concrete subclasses that all inherit the identical method
            // resolves the same `AssertInfo` any single member would.
            // Once resolved, `apply_assert_infos`/`narrow_type_to`
            // already narrow the receiver's own full current type
            // generically, Union or not.
            ExprKind::MethodCall(recv, method, args, _) => {
                let recv_ty = self.infer_expr(recv, locals);
                let class = match &recv_ty {
                    Type::Object(class, _) => Some(class.clone()),
                    Type::EnumCase(fqn, _) => Some(fqn.clone()),
                    Type::Union(members) | Type::Intersection(members) => {
                        members.iter().find_map(|m| match m {
                            Type::Object(class, _) => Some(class.clone()),
                            Type::EnumCase(fqn, _) => Some(fqn.clone()),
                            _ => None,
                        })
                    }
                    _ => None,
                };
                if let Some(class) = class
                    && let Some(sig) = self.registry.lookup_method(&class, method)
                {
                    let infos = if branch_true {
                        &sig.assert_if_true
                    } else {
                        &sig.assert_if_false
                    };
                    if !infos.is_empty() {
                        self.apply_assert_infos(infos, sig, Some(recv), args, locals);
                    }
                }
            }
            // Same idea as `MethodCall` just above, for a static method
            // call - no receiver expression exists here at all, so only
            // a `Param`-targeted assertion can ever resolve.
            ExprKind::StaticCall(class_name, method, args) => {
                if let Some(class) = self.resolve_static_class_name(class_name)
                    && let Some(sig) = self.registry.lookup_method(&class, method)
                {
                    let infos = if branch_true {
                        &sig.assert_if_true
                    } else {
                        &sig.assert_if_false
                    };
                    if !infos.is_empty() {
                        self.apply_assert_infos(infos, sig, None, args, locals);
                    }
                }
            }
            ExprKind::Instanceof(inner, target) => {
                // The false branch doesn't need a
                // "not this class" type to be representable - it only
                // needs to *drop* the one member just proven impossible
                // from an already-enumerated `Union`, exactly what
                // `narrow_type_excluding` already does soundly elsewhere.
                // A dynamic target (`$x instanceof $className`) still
                // doesn't name a concrete class to narrow to either way.
                if let Some(key) = narrowing_key(inner)
                    && let InstanceofTarget::Name(class_name, _) = target
                {
                    let resolved = self.resolve_name_expr(class_name);
                    if branch_true {
                        let current = self.current_narrowed_type(inner, &key, locals);
                        locals.insert(key, narrow_instanceof_true(&current, &resolved));
                    } else {
                        let current = self.current_narrowed_type(inner, &key, locals);
                        locals.insert(key, narrow_type_excluding(&current, &resolved));
                    }
                }
            }
            // An assignment used as a boolean sub-condition (`$a &&
            // ($x = f()) && $x->m()`, `!$a || !$x = f()`) evaluates
            // unconditionally whenever control actually reaches it -
            // the assigned value becomes `$x`'s real type regardless of
            // which way the *whole* enclosing condition ultimately goes,
            // so this must run before (and instead of) the generic
            // bare-truthiness catch-all just below, which `narrowing_
            // key`'s own `Assign` case would otherwise match too:
            // without this dedicated arm, that catch-all narrowed
            // falsiness out of the *pre*-assignment type it read via
            // `current_narrowed_type` (a no-op for most non-falsy
            // reassignments, silently discarding the assignment's real
            // effect) rather than binding the assigned type at all.
            // `check_expr`'s own `Binary(And/Or, ...)` handling re-derives
            // each operand's narrowing through this same function (see
            // its own doc comment) instead of reusing whatever it
            // already computed while checking each operand for
            // diagnostics, so a later &&/|| operand referencing the
            // freshly-assigned variable needs the assignment replayed
            // here too - e.g. `$r->hasMethod('__call') && ($r =
            // $r->getMethod('__call')) && $r->isPublic()` must see that
            // `$r` became a `ReflectionMethod` two conjuncts earlier in
            // the very same condition, not the pre-reassignment type.
            //
            // The `branch_true` truthy case still needs its own extra
            // step on top of the plain bind: `if ($position = strpos($x,
            // "#")) { substr($x, 0, $position); }` relies on the
            // assignment's own result being truthy to reach the `if`
            // body at all - without also excluding falsy members from
            // the freshly-bound type here, `$position` would keep
            // `strpos()`'s raw `int<0, max>|false` inside the truthy
            // branch instead of a real non-zero `int` (`substr()`'s 3rd
            // argument expects `int|null`, not `int<0, max>|false`).
            ExprKind::Assign(lhs, rhs) => {
                let rhs_ty = self.infer_expr(rhs, locals);
                self.bind_assignment_target(lhs, rhs_ty, locals);
                if branch_true
                    && let Some(key) = narrowing_key(lhs)
                    && let Some(current) = locals.get(&key).cloned()
                {
                    locals.insert(key, narrow_type_excluding_falsy(&current));
                }
            }
            // The narrow slice of real PHPStan's `ConditionalExpressionHolder`
            // this project builds - `$ok = $x instanceof Foo; if ($ok) {...}`
            // (or the plain-alias form `$y = $x; if ($y) {...}`) replays
            // the *stored* condition through this exact same recursive
            // dispatch, correlating a *second* variable's narrowing to
            // `name`'s own truthiness with zero new type-narrowing logic
            // - only replay. Must run for both `branch_true` values
            // (`if (!$ok) {...}` needs the negated replay too), unlike
            // the bare-truthiness catch-all just below, so this can't
            // simply be folded into that arm's own `branch_true`-gated
            // guard. Runs *in addition to* that catch-all's own bare-
            // variable behavior (preserved verbatim right after), not
            // instead of it - a strict superset for a bare-`Variable`
            // condition, never a behavior change when there's no alias
            // to replay.
            ExprKind::Variable(name) => {
                if let Some((_, alias)) = locals.condition_aliases.get(name).cloned() {
                    self.apply_narrowing(&alias, locals, branch_true);
                }
                self.replay_truthiness_discriminant_facts(name, locals, branch_true);
                if branch_true {
                    let current = self.current_narrowed_type(cond, name, locals);
                    locals.insert(name.clone(), narrow_type_excluding_falsy(&current));
                }
            }
            // A bare truthiness check (`if ($x)` / `if (!$x)`, the latter
            // reaching here via the `Not` arm's recursion above) - only
            // the truthy branch is narrowable, by excluding the falsy
            // members this project can represent (`Null`/`False`); the
            // falsy branch isn't safely narrowable to anything more
            // specific than the current type.
            _ if branch_true && narrowing_key(cond).is_some() => {
                let key = narrowing_key(cond).expect("checked by guard above");
                let current = self.current_narrowed_type(cond, &key, locals);
                locals.insert(key, narrow_type_excluding_falsy(&current));
            }
            _ => {}
        }
    }

    /// The store half of the condition-alias mechanism (see `apply_
    /// narrowing`'s own `ExprKind::Variable` arm for the replay half) -
    /// called right after an ordinary assignment binds `lhs`. Cheap on
    /// the fast path: `condition_alias_shape` is a plain `matches!` with
    /// no clone, so the overwhelming majority of assignments that could
    /// never qualify (`$x = 5;`, `$x = $obj->method();` with no
    /// recognized shape) cost one match check and nothing else. Only the
    /// narrow subset that passes it pays for `rhs.clone()` (an owned
    /// `Expr` deep-clone, once), which then lives behind an `Rc` so every
    /// later branch-fork of `Locals` stays O(1).
    pub(crate) fn record_condition_alias(&self, lhs: &Expr, rhs: &Expr, locals: &mut Locals) {
        let ExprKind::Variable(name) = &lhs.kind else {
            return;
        };
        if !condition_alias_shape(rhs) {
            return;
        }
        // Real-PHPStan-confirmed self-reference guard
        // (`AssignHandler::isExprSafeToProjectThroughVariable`,
        // `$expr->name !== $variableName`) - `$x = $x instanceof Foo;`
        // must never alias to itself (the replay would just re-derive
        // `$x`'s own type from its own not-yet-narrowed self, and a
        // pathological "$x = $x;" must not loop).
        if matches!(&rhs.kind, ExprKind::Variable(n) if n == name) {
            return;
        }
        let Some(target_keys) = condition_alias_targets(rhs) else {
            return;
        };
        // The same self-reference hazard as the bare-`Variable` check
        // above, generalized: `$flag = $flag || $x === null;` naming
        // `$flag` as one of its own compound condition's targets would
        // replay `$flag`'s own not-yet-narrowed self the same way.
        if target_keys.iter().any(|k| k == name) {
            return;
        }
        locals.set_condition_alias(name.clone(), target_keys, std::rc::Rc::new(rhs.clone()));
    }

    /// `AssertRuleHelper`'s equivalent - a `@phpstan-
    /// assert`-family tag's own type is already resolved into a real
    /// `AssertInfo` (`FunctionSig::assert`/`assert_if_true`/
    /// `assert_if_false`) and consumed for narrowing (`apply_narrowing`'s
    /// own `check_assert_call` dispatch above); this is its own
    /// declaration-validity half - `class.notFound`/`assert.trait` reuse
    /// this file's sibling existence-checking helpers exactly the way
    /// `check_method_tag_types`/`check_mixin_types`/`check_type_alias_types`
    /// (`oop.rs`) already do; `parameter.notFound`/`assert.unknownExpr`
    /// surface `AssertTarget::NotFound`/`Unknown`;
    /// `assert.impossibleType`/`.alreadyNarrowedType` are new (see
    /// `check_one_assert_tag`'s own doc comment for scope). Deliberately
    /// NOT built: `assert.unresolvableType` (real PHPStan's own pure-
    /// intersection-type unsatisfiability check - a genuinely separate,
    /// larger gap spanning multiple unrelated rule families, not specific
    /// to asserts) and the `missingType.iterableValue`/`.generics`
    /// sub-checks on an asserted type (needs its own message-template
    /// plumbing change to `report_missing_typehint_findings`). Called
    /// once per declaration, alongside `check_declared_purity_tag`/
    /// `check_declared_param_callable_tags` (`function_decl.rs`).
    pub(crate) fn check_assert_tag_types(
        &mut self,
        sig: Option<&FunctionSig>,
        span: Span,
        label: &str,
    ) {
        let Some(sig) = sig else {
            return;
        };
        for (tag_name, infos) in [
            ("@phpstan-assert", &sig.assert),
            ("@phpstan-assert-if-true", &sig.assert_if_true),
            ("@phpstan-assert-if-false", &sig.assert_if_false),
        ] {
            for info in infos {
                self.check_one_assert_tag(info, tag_name, sig, span, label);
            }
        }
    }

    /// One `@phpstan-assert`-family tag's own full declaration-validity
    /// check - see `check_assert_tag_types`'s own doc comment for the
    /// full identifier list and what's deliberately not attempted.
    /// `assert.impossibleType`/`.alreadyNarrowedType` are only ever
    /// attempted for an `AssertTarget::Param` target compared against
    /// that exact parameter's own declared type - a disclosed, narrower
    /// cut versus real PHPStan's own `$this`/property-target coverage
    /// too, since those need a class-level property/receiver-type lookup
    /// this function doesn't otherwise need at all, and `Param` is by far
    /// the dominant real-corpus shape for this tag family.
    fn check_one_assert_tag(
        &mut self,
        info: &crate::registry::AssertInfo,
        tag_name: &str,
        sig: &FunctionSig,
        span: Span,
        label: &str,
    ) {
        let target_desc = match &info.target {
            AssertTarget::Param(name) => format!("${name}"),
            AssertTarget::ThisSelf => "$this".to_string(),
            AssertTarget::ThisProp(prop) => format!("$this->{prop}"),
            AssertTarget::NotFound(name) => {
                self.report(
                    label.to_string(),
                    format!("Assert references unknown parameter ${name}."),
                    span,
                    0,
                    "parameter.notFound",
                );
                return;
            }
            AssertTarget::Unknown(raw) => {
                self.report(
                    label.to_string(),
                    format!("Assert references unknown {raw}."),
                    span,
                    0,
                    "assert.unknownExpr",
                );
                return;
            }
        };
        let subject = format!("PHPDoc tag {tag_name} for {target_desc}");
        self.check_type_class_names_exist(&info.ty, &subject, span, label);
        self.check_type_trait_refs(&info.ty, &subject, span, label, "assert.trait");
        self.check_type_class_name_case(&info.case_refs, label, span);

        let AssertTarget::Param(name) = &info.target else {
            return;
        };
        let Some(param) = sig.params.iter().find(|p| &p.name == name) else {
            return;
        };
        let negated_prefix = if info.negated { "negated " } else { "" };
        let is_super = self.registry.is_assignable(&param.ty, &info.ty);
        let is_disjoint = types_definitely_disjoint(&info.ty, &param.ty);
        let (impossible, already_narrowed) = if info.negated {
            (is_super, is_disjoint)
        } else {
            (is_disjoint, is_super)
        };
        if impossible {
            self.report(
                label.to_string(),
                format!(
                    "Asserted {negated_prefix}type {} for {target_desc} with type {} can never happen.",
                    info.ty, param.ty
                ),
                span,
                0,
                "assert.impossibleType",
            );
        } else if already_narrowed {
            self.report(
                label.to_string(),
                format!(
                    "Asserted {negated_prefix}type {} for {target_desc} with type {} does not narrow down the type.",
                    info.ty, param.ty
                ),
                span,
                0,
                "assert.alreadyNarrowedType",
            );
        }
    }
}

/// `assert.impossibleType`'s own narrow, conservative type-
/// disjointness primitive. Only ever declares
/// two types disjoint when each resolves to one of a handful of coarse,
/// mutually-exclusive "scalar families" (`scalar_family` below) and those
/// families differ - e.g. `string` vs `int`, or `array` vs `object`.
/// Recurses into `Union` on either side (every member of a union side
/// must be disjoint from the other side for the whole comparison to
/// count - the same "stay on the lenient side" direction this project's
/// own union-legality check already established).
/// Returns `false` (not confidently disjoint - "don't guess") for
/// anything not covered: `Mixed`/`Never`/`Void`, two same-family types
/// (e.g. two unrelated object types - real PHPStan's own object-vs-object
/// disjointness needs a `final`-class/no-subtype-relation check this
/// primitive deliberately doesn't attempt), `Intersection`, `Callable`,
/// `Accessory`, templates, and every pseudo-type.
fn types_definitely_disjoint(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Union(members), _) => members.iter().all(|m| types_definitely_disjoint(m, b)),
        (_, Type::Union(members)) => members.iter().all(|m| types_definitely_disjoint(a, m)),
        _ => match (scalar_family(a), scalar_family(b)) {
            (Some(fa), Some(fb)) => fa != fb,
            _ => false,
        },
    }
}

#[derive(PartialEq)]
enum ScalarFamily {
    Int,
    Float,
    String,
    Bool,
    Null,
    Array,
    Object,
}

fn scalar_family(ty: &Type) -> Option<ScalarFamily> {
    match ty {
        Type::Int | Type::IntLiteral(_) | Type::IntRange(_, _) => Some(ScalarFamily::Int),
        Type::Float | Type::FloatLiteral(_) => Some(ScalarFamily::Float),
        Type::String | Type::StringLiteral(_) | Type::ClassString(_) => Some(ScalarFamily::String),
        Type::Bool | Type::True | Type::False => Some(ScalarFamily::Bool),
        Type::Null => Some(ScalarFamily::Null),
        Type::Array(_, _) | Type::ArrayShape(_) => Some(ScalarFamily::Array),
        Type::Object(_, _) | Type::EnumCase(_, _) | Type::ObjectShape(_) => {
            Some(ScalarFamily::Object)
        }
        _ => None,
    }
}
