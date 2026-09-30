use crate::checker::expr::type_definitely_excludes_null;
use crate::checker::{Checker, Discriminant, Locals, literal_string_key};
use crate::registry::{AssertTarget, FunctionReflection};
use syntax::ast::*;
use syntax::token::Span;
use types::{AccessoryKind, Type};
mod assertions;
mod phpunit;
mod predicates;
mod preg;

pub(crate) use predicates::{
    is_real_variable_key, key_depends_on, narrowing_key, narrowing_type_for_predicate,
    propagate_container_field_facts, purge_stale_narrowing_for,
};

/// `count($x)`'s own operand-order detection for the count-comparison
/// narrowing below - same shape as `int_comparison_operands`, but keyed
/// on "is this side a single-argument `count(...)` call" rather than
/// `narrowing_key` (a bare `count(...)` call isn't itself a narrowable
/// key). Returns the call's own argument and whether `count(...)` was
/// written on the left (`count($x) > 0`) or the right (`0 < count($x)`).
pub(crate) fn count_call_operand<'e>(l: &'e Expr, r: &'e Expr) -> Option<(&'e Expr, bool)> {
    count_call(l)
        .map(|(a, _)| (a, true))
        .or_else(|| count_call(r).map(|(a, _)| (a, false)))
}

/// `count($x)` or `sizeof($x)`, with its mode argument if written: `$x`
/// and the mode.
fn count_call(e: &Expr) -> Option<(&Expr, Option<&Expr>)> {
    let ExprKind::Call(callee, args) = &e.peeled().kind else {
        return None;
    };
    let ExprKind::Name(name) = &callee.kind else {
        return None;
    };
    if !matches!(name.trim_start_matches('\\'), "count" | "sizeof") {
        return None;
    }
    match args.as_slice() {
        [counted] => Some((&counted.value, None)),
        [counted, mode] => Some((&counted.value, Some(&mode.value))),
        _ => None,
    }
}

/// Flips a comparison operator to the operand-swapped equivalent - `0 <
/// count($x)` narrows the same way `count($x) > 0` already does, once
/// the operator itself is mirrored to match.
pub(super) fn mirror_comparison(op: &BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Gt => BinOp::Lt,
        BinOp::LtEq => BinOp::GtEq,
        BinOp::GtEq => BinOp::LtEq,
        other => *other,
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
        ExprKind::PropertyAccess(base, _, ObjectOperator::NullsafeArrow)
        | ExprKind::MethodCall(base, _, _, ObjectOperator::NullsafeArrow) => Some(base),
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
    syntax::stack::guard(|| {
        match &rhs.kind {
            ExprKind::Instanceof(..)
            // `Lt`/`Gt`/`LtEq`/`GtEq` alongside the equality-family
            // operators - needed so a numeric-comparison leaf inside a
            // larger `&&`/`||` tree (`record_if_body_flag_alias`'s own real
            // target shape: `$x === false or $x < $y or $x < $z`) doesn't
            // block the *whole* compound condition from being alias-
            // eligible, even though `apply_narrowing`'s own comparison
            // narrowing doesn't do anything false-ness-specific for this one
            // leaf - the useful narrowing still comes from whichever other
            // leaf actually proves it.
            | ExprKind::Binary(
                BinOp::Identical
                | BinOp::NotIdentical
                | BinOp::Eq
                | BinOp::NotEq
                | BinOp::Lt
                | BinOp::Gt
                | BinOp::LtEq
                | BinOp::GtEq,
                ..,
            )
            | ExprKind::Call(..)
            | ExprKind::Isset(_)
            | ExprKind::Empty(_)
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
            ExprKind::Binary(BinOp::And | BinOp::Or | BinOp::LogicalAnd | BinOp::LogicalOr, l, r) => {
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
    })
}

/// Whether `name` (a bare variable, already `is_real_variable_key`-checked
/// by the one caller) appears anywhere inside `e` as an `ExprKind::
/// Variable` - `record_branch_implied_facts`'s own guard against recording
/// a variable whose narrowing this exact branch's own condition already
/// explains (see its call site's doc comment). A generic search rather
/// than a hand-enumerated set of `ExprKind` arms - `cond` can be
/// arbitrarily shaped (a call, a chained method,
/// `instanceof`, a compound `&&`/`||`), and this only needs to answer
/// "is this name referenced anywhere in it," not narrow/instanceof-specific
/// shape recognition the way `narrowing_key`/`truthiness_discriminant_subject`
/// do elsewhere in this module.
fn cond_references_variable(e: &Expr, name: &str) -> bool {
    syntax::visit::any_expr_in_scope(
        e,
        |e| matches!(&e.kind, ExprKind::Variable(v) if v.as_ref() == name),
    )
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
    let e = e.peeled();
    let inner = match &e.kind {
        ExprKind::Cast(cast, inner) if cast.kind() == CastKind::Bool => inner,
        _ => e,
    };
    match &inner.kind {
        ExprKind::Variable(_) => narrowing_key(inner),
        _ => None,
    }
}

/// A condition recognized well enough to canonicalize as a bare-
/// truthiness discriminant, for `record_branch_implied_facts`'s own
/// same-condition-recurrence case - a bare variable (the
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
        ExprKind::Isset(operands) => {
            let [operand] = operands.as_slice() else {
                return None;
            };
            Some(format!("isset({})", narrowing_key(operand)?))
        }
        _ => None,
    }
}

impl<'a> Checker<'a> {
    /// A true `isset($a['x'][$k])` also proves every container along the
    /// chain held an offset - see `Type::to_isset_container`.
    fn narrow_isset_containers(&self, arg: &Expr, locals: &mut Locals) {
        let mut e = arg.peeled();
        loop {
            let (base, lit_key) = match &e.kind {
                ExprKind::Index(base, Some(idx)) => (base, crate::checker::literal_shape_key(idx)),
                ExprKind::PropertyAccess(base, _, _)
                | ExprKind::DynamicPropertyAccess(base, _, _) => {
                    if let Some(base_key) = narrowing_key(base) {
                        let current = self.current_narrowed_type(base, &base_key, locals);
                        locals.insert(base_key, current.without(&Type::null()));
                    }
                    e = base.peeled();
                    continue;
                }
                _ => return,
            };
            if let Some(base_key) = narrowing_key(base) {
                let current = self.current_narrowed_type(base, &base_key, locals);
                let narrowed = current.to_isset_container(lit_key.as_ref());
                if !narrowed.is_never() {
                    locals.insert(base_key, narrowed);
                }
            }
            e = base.peeled();
        }
    }

    /// A plain function call tagged `@phpstan-assert-if-true`/`-if-false`
    /// on its own docblock - see `FunctionReflection::assert_if_true`'s own doc
    /// comment for the full picture. No `$this`/receiver here, so only a
    /// `Param`-targeted assertion can ever resolve (`apply_assert_infos`
    /// silently skips the rest).
    fn apply_function_conditional_asserts(
        &self,
        name: &QualifiedName,
        args: &[Arg],
        locals: &mut Locals,
        branch_true: bool,
    ) {
        if let Some((_, sig)) = self.registry.resolve_function(name) {
            let infos = if branch_true {
                sig.assert_if_true()
            } else {
                sig.assert_if_false()
            };
            if !infos.is_empty() {
                self.apply_assert_infos(infos, sig, None, args, locals);
            }
        }
    }

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
    /// name, or `"isset(key)"`). Looked up under whichever of `true` and
    /// `false` matches `branch_true`, mirroring how the
    /// equality discriminant's own replay (`apply_equality_narrowing`)
    /// looks itself up by `(subject, value)`.
    pub(crate) fn replay_truthiness_discriminant_facts(
        &self,
        subject: &str,
        locals: &mut Locals,
        branch_true: bool,
    ) {
        let marker = if branch_true {
            Type::bool_literal(true)
        } else {
            Type::bool_literal(false)
        };
        if let Some(implied) = locals
            .branch_implied_facts()
            .get(&Discriminant::new(subject, &marker))
            .cloned()
        {
            for (implied_key, implied_ty) in implied.iter() {
                locals.insert(implied_key.clone(), implied_ty.clone());
            }
        }
    }

    /// `replay_truthiness_discriminant_facts`'s own `instanceof` sibling -
    /// `record_branch_implied_facts`'s own third subject shape (a variable
    /// that became a concrete `Object` type, not a boolean, specifically
    /// within the recording branch - `UploadService::addUploadedFile`'s
    /// own `$dup_detect_lock_conn = DbConnection::build();` inside `if
    /// (...) { ... }`, later re-tested via `if ($dup_detect_lock_conn
    /// instanceof Connection)`). True-branch only, like the equality
    /// discriminant above: `instanceof` false says nothing reusable about
    /// one fixed identity to key facts under.
    pub(crate) fn replay_object_discriminant_facts(
        &self,
        subject: &str,
        resolved_class: &Type,
        locals: &mut Locals,
    ) {
        if let Some(implied) = locals
            .branch_implied_facts()
            .get(&Discriminant::new(subject, resolved_class))
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
    /// relationship to this `if` at all. Three recognized discriminant
    /// shapes, each with its own scoping: `$subject === value` (or
    /// `==`), scoped to the *true* branch only (an equality's false
    /// branch says nothing reusable - "not equal to X" isn't one fixed
    /// identity to key facts under) and a value comparably concrete
    /// enough to make "the identical comparison" reproducible (an enum
    /// case, a literal); a bare truthiness discriminant
    /// (`truthiness_discriminant_subject`), keyed by `true`/
    /// `false` as `Discriminant`'s own "value" half;
    /// and any *other* local variable that itself became a literal
    /// `true`/`false` specifically within this branch (`AlbumsPageRenderer
    /// ::render`'s own `$order_by_date = true;` inside `if (str_starts_with
    /// (...)) { ... }`, later re-tested bare as `if ($order_by_date)`) - a
    /// real proxy discriminant for "this branch ran," the same as the
    /// tested condition itself, confirmed distinct from it only by not
    /// already being one (`before.get(name) != Some(after_ty)` - a value
    /// already `True`/`False` *before* this branch ran says nothing about
    /// *this* branch). The truthiness-shaped subjects are safe for either
    /// branch, unlike the equality shape, since truthy/falsy are the whole
    /// space of a boolean condition's own outcomes, not one value out of
    /// many. A branch can yield several such subjects at once (the tested
    /// condition plus zero or more flag variables) - the identical implied
    /// facts get recorded under every one of them, each a valid, separate
    /// way to later prove "this branch ran."
    /// The discriminant `cond` itself establishes in its `branch_true`
    /// branch, as `(subject, value)`, when it has one facts are keyed by.
    pub(crate) fn primary_discriminant(
        &self,
        cond: &Expr,
        before: &Locals,
        branch_true: bool,
    ) -> Option<(String, Type)> {
        if branch_true && let ExprKind::Binary(BinOp::Identical | BinOp::Eq, l, r) = &cond.kind {
            match (narrowing_key(l), narrowing_key(r)) {
                (Some(key), _) => Some((key, self.infer_expr(r, before))),
                (None, Some(key)) => Some((key, self.infer_expr(l, before))),
                (None, None) => None,
            }
            .filter(|(_, ty)| {
                ty.is_enum_case()
                    || ty.int_literal_value().is_some()
                    || ty.string_literal_value().is_some()
            })
        } else {
            truthiness_discriminant_subject(cond).map(|key| {
                (
                    key,
                    if branch_true {
                        Type::bool_literal(true)
                    } else {
                        Type::bool_literal(false)
                    },
                )
            })
        }
    }

    /// The key facts under `cond`'s own discriminant for its `branch_true`
    /// branch are recorded at, if any.
    pub(crate) fn primary_discriminant_key(
        &self,
        cond: &Expr,
        before: &Locals,
        branch_true: bool,
    ) -> Option<Discriminant> {
        self.primary_discriminant(cond, before, branch_true)
            .map(|(subject, value)| Discriminant::new(&subject, &value))
    }

    pub(crate) fn record_branch_implied_facts(
        &self,
        cond: &Expr,
        before: &Locals,
        after: &mut Locals,
        branch_true: bool,
    ) {
        let primary_subject = self.primary_discriminant(cond, before, branch_true);
        let mut subjects: Vec<(String, Type)> = primary_subject.into_iter().collect();
        let mut flag_subjects: Vec<(String, Type)> = Vec::new();
        for (name, after_ty) in after.iter() {
            let is_flag_proxy = after_ty.bool_literal_value().is_some();
            // A variable that became a concrete `Object` type specifically
            // within this branch is `instanceof`'s own proxy discriminant
            // (`replay_object_discriminant_facts`'s own doc comment) - the
            // object-typed counterpart of the boolean-flag-proxy case
            // above.
            let is_object_proxy =
                after_ty.as_object().is_some() || *after_ty == Type::object_without_class();
            // Same proxy shape as `is_object_proxy` above, for a variable
            // that became a concrete enum case specifically within this
            // branch (`AuthService::authKeyLogin`'s own `$valid_key =
            // ConnectedWith::ApiKey;`) - an enum case isn't an object of a
            // class here, so it needs its own check;
            // the replay side (`apply_equality_narrowing`) is already
            // generic over the discriminant value's own type and needs no
            // matching change.
            let is_enum_case_proxy = after_ty.is_enum_case();
            if (is_flag_proxy || is_object_proxy || is_enum_case_proxy)
                && before.get(name) != Some(after_ty)
                && !subjects.iter().any(|(k, _)| k == name)
            {
                subjects.push((name.clone(), after_ty.clone()));
                if is_flag_proxy {
                    flag_subjects.push((name.clone(), after_ty.clone()));
                }
            }
        }
        if subjects.is_empty() {
            return;
        }
        // A flag set in this branch stands for the whole branch having
        // run, so it also implies every variable the branch narrowed,
        // including the ones `cond` tests.
        let mut narrowed: Vec<(String, Type)> = Vec::new();
        if !flag_subjects.is_empty() {
            for (name, after_ty) in after.iter() {
                if !is_real_variable_key(name)
                    || subjects.iter().any(|(k, _)| k == name)
                    || after_ty.is_never()
                {
                    continue;
                }
                if let Some(before_ty) = before.get(name)
                    && before_ty != after_ty
                    && self
                        .registry
                        .relation()
                        .is_super_type_of(before_ty, after_ty)
                        .yes()
                {
                    narrowed.push((name.clone(), after_ty.clone()));
                }
            }
        }
        record_implied_facts(after, &flag_subjects, &narrowed);
        let mut implied: Vec<(String, Type)> = Vec::new();
        for (name, after_ty) in after.iter() {
            if subjects.iter().any(|(k, _)| k == name) {
                continue;
            }
            // A candidate whose own name is tested directly by `cond`
            // (`if ($value instanceof BackedEnum) { ...; $value =
            // get_object_vars($value); }`) has its own non-null-ness
            // already explained by `cond`'s own narrowing, not by whatever
            // *other* subject this branch also happens to establish -
            // recording it here would let a later, unrelated re-test of
            // that other subject replay a type snapshot that's stale the
            // moment this variable's own narrowing chain (freshly
            // re-derived every time, e.g. on a later loop pass) disagrees
            // with it. Real corpus regression this guards against:
            // `League\Uri\QueryString::composeRecursive`'s `$value`, whose
            // `instanceof`-narrowed type got overwritten by a stale
            // `array` fact replayed from `$composeMode`'s own unrelated
            // enum-case correlation, recorded on an earlier loop pass.
            // Only a plain variable name is checked here (`is_real_variable_key`) -
            // a synthetic per-field key (`"row[id]"`) never matches a bare
            // `ExprKind::Variable` walk and isn't this shape's own hazard.
            if is_real_variable_key(name) && cond_references_variable(cond, name) {
                continue;
            }
            if type_definitely_excludes_null(after_ty)
                && !after_ty.is_mixed()
                && !before.get(name).is_some_and(type_definitely_excludes_null)
            {
                implied.push((name.clone(), after_ty.clone()));
            }
        }
        record_implied_facts(after, &subjects, &implied);
    }
}

/// Adds `implied` to the facts recorded under each of `subjects`.
fn record_implied_facts(
    after: &mut Locals,
    subjects: &[(String, Type)],
    implied: &[(String, Type)],
) {
    if implied.is_empty() {
        return;
    }
    for (subject_key, value_ty) in subjects {
        let key = Discriminant::new(subject_key, value_ty);
        let mut combined = after
            .branch_implied_facts()
            .get(&key)
            .map(|existing| (**existing).clone())
            .unwrap_or_default();
        for (name, ty) in implied {
            crate::checker::control_flow::add_implied_fact(&mut combined, name, ty);
        }
        after
            .branch_implied_facts_mut()
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
    syntax::stack::guard(|| {
        match &rhs.kind {
            ExprKind::Instanceof(inner, _) => Some(vec![narrowing_key(inner)?]),
            // `condition_alias_shape`'s own identical comparison-operator
            // extension, mirrored here so a numeric-comparison leaf inside a
            // larger tree still contributes its own narrowable operand as a
            // real subject (not just a shape-eligibility check).
            ExprKind::Binary(
                BinOp::Identical
                | BinOp::NotIdentical
                | BinOp::Eq
                | BinOp::NotEq
                | BinOp::Lt
                | BinOp::Gt
                | BinOp::LtEq
                | BinOp::GtEq,
                l,
                r,
            ) => Some(vec![narrowing_key(l).or_else(|| narrowing_key(r))?]),
            // A stored preg call is replayed with its operands' later
            // values, so rebinding any of them drops the alias.
            ExprKind::Call(..) if let Some(call) = crate::checker::preg_call(rhs) => {
                let mut targets = vec![narrowing_key(call.matches?)?];
                targets.extend(call.subject.and_then(narrowing_key));
                for operand in [Some(call.pattern), call.offset].into_iter().flatten() {
                    if !matches!(
                        operand.peeled().kind,
                        ExprKind::Literal(_) | ExprKind::Name(_) | ExprKind::ClassConstFetch(..)
                    ) {
                        targets.push(narrowing_key(operand)?);
                    }
                }
                Some(targets)
            }
            ExprKind::Call(_, args) => match args.as_slice() {
                [single_arg] => Some(vec![narrowing_key(&single_arg.value)?]),
                _ => None,
            },
            ExprKind::Isset(operands) => operands.iter().map(narrowing_key).collect(),
            ExprKind::Empty(operand) => Some(vec![narrowing_key(operand)?]),
            ExprKind::Variable(name) => Some(vec![name.to_string()]),
            ExprKind::Binary(
                BinOp::And | BinOp::Or | BinOp::LogicalAnd | BinOp::LogicalOr,
                l,
                r,
            ) => {
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
            ExprKind::PropertyAccess(base, _, ObjectOperator::NullsafeArrow)
            | ExprKind::MethodCall(base, _, _, ObjectOperator::NullsafeArrow) => {
                Some(vec![narrowing_key(base)?])
            }
            _ => None,
        }
    })
}

impl<'a> Checker<'a> {
    /// Narrows `locals` in place based on `cond` being true (`branch_true`)
    /// or false, along one branch of an `if`/`while`. Only handles the
    /// common, safe patterns - equality against a known-type value
    /// (`$x === null`, `null !== $x`), `is_*()` guards, and `instanceof` -
    /// anything else leaves `locals` untouched rather than guess.
    pub(crate) fn apply_narrowing(&self, cond: &Expr, locals: &mut Locals, branch_true: bool) {
        syntax::stack::guard(|| self.apply_narrowing_unguarded(cond, locals, branch_true))
    }

    fn apply_narrowing_unguarded(&self, cond: &Expr, locals: &mut Locals, branch_true: bool) {
        // A preg call's result, however it's compared, narrows its
        // `$matches` and subject; the arms below still narrow a variable
        // the call was stored in.
        self.apply_preg_result_narrowing(cond, locals, branch_true);
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
            && let ExprKind::Call(callee, args) = &cond.peeled().kind
            && let ExprKind::Name(fn_name) = &callee.peeled().kind
            && fn_name
                .rsplit('\\')
                .next()
                .unwrap_or(fn_name)
                .eq_ignore_ascii_case("function_exists")
            && let [guarded_arg] = args.as_slice()
            && let Some(guarded_name) = literal_string_value(&guarded_arg.value)
        {
            locals
                .guarded_functions_mut()
                .insert(guarded_name.trim_start_matches('\\').to_ascii_lowercase());
        }
        // `class_exists('X')`'s true branch (or `interface_exists`,
        // `trait_exists`, `enum_exists`) proves the class-like `X` declared.
        if branch_true
            && let ExprKind::Call(callee, args) = &cond.peeled().kind
            && let ExprKind::Name(fn_name) = &callee.peeled().kind
            && matches!(
                fn_name
                    .rsplit('\\')
                    .next()
                    .unwrap_or(fn_name)
                    .to_ascii_lowercase()
                    .as_str(),
                "class_exists" | "interface_exists" | "trait_exists" | "enum_exists"
            )
            && let Some(guarded_arg) = args.first()
            && let Some(guarded_name) = literal_string_value(&guarded_arg.value)
        {
            locals
                .guarded_classes_mut()
                .insert(guarded_name.trim_start_matches('\\').to_ascii_lowercase());
        }
        // `defined('NAME')`'s true branch proves the global constant
        // `NAME` exists, the constant counterpart of `function_exists`.
        if branch_true
            && let ExprKind::Call(callee, args) = &cond.peeled().kind
            && let ExprKind::Name(fn_name) = &callee.peeled().kind
            && fn_name
                .rsplit('\\')
                .next()
                .unwrap_or(fn_name)
                .eq_ignore_ascii_case("defined")
            && let [guarded_arg] = args.as_slice()
            && let Some(guarded_name) = literal_string_value(&guarded_arg.value)
        {
            locals
                .guarded_constants_mut()
                .insert(guarded_name.trim_start_matches('\\').to_string());
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
            && let ExprKind::Call(callee, args) = &cond.peeled().kind
            && let ExprKind::Name(fn_name) = &callee.peeled().kind
            && let [recv_arg, name_arg] = args.as_slice()
            && let ExprKind::Variable(recv_name) = &recv_arg.value.peeled().kind
            && let Some(member_name) = literal_string_value(&name_arg.value)
        {
            let bare = fn_name.rsplit('\\').next().unwrap_or(fn_name);
            if bare.eq_ignore_ascii_case("method_exists") {
                locals
                    .guarded_methods_mut()
                    .insert((recv_name.to_string(), member_name.into_owned()));
            } else if bare.eq_ignore_ascii_case("property_exists") {
                locals
                    .guarded_properties_mut()
                    .insert((recv_name.to_string(), member_name.into_owned()));
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
            ExprKind::Binary(BinOp::And | BinOp::LogicalAnd, l, r) if branch_true => {
                self.apply_narrowing(l, locals, true);
                self.apply_narrowing(r, locals, true);
            }
            ExprKind::Binary(BinOp::Or | BinOp::LogicalOr, l, r) if !branch_true => {
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
            ExprKind::Binary(BinOp::Or | BinOp::LogicalOr, ..) if branch_true => {
                if let Some((key, target_ty, repr_expr)) =
                    self.or_chain_narrowing_target(cond, locals)
                {
                    let current = self.current_narrowed_type(repr_expr, &key, locals);
                    locals.insert(key, self.relation().narrow(&current, &target_ty));
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
            ExprKind::Binary(BinOp::And | BinOp::LogicalAnd, ..) if !branch_true => {
                if let Some((key, target_ty, repr_expr)) =
                    self.and_chain_narrowing_target(cond, locals)
                {
                    let current = self.current_narrowed_type(repr_expr, &key, locals);
                    locals.insert(key, self.relation().narrow(&current, &target_ty));
                }
            }
            ExprKind::Binary(op, l, r)
                if let Some((spaceship_op, a, b)) = spaceship_comparison(op, l, r) =>
            {
                // `($a <=> $b) < 0` is `$a < $b`; `=== 0` or `== 0` is
                // `$a == $b`.
                match spaceship_op {
                    Some(ordering) => {
                        self.apply_ordering_narrowing(a, b, ordering, branch_true, locals);
                    }
                    None => self.apply_loose_equality_narrowing(a, b, locals, branch_true),
                }
            }
            // A comparison with `count($x)` narrows `$x` by the count it
            // proves, then the other side as any comparison does.
            ExprKind::Binary(op, l, r) if comparison_count_operand(op, l, r).is_some() => {
                self.apply_count_narrowing(op, l, r, branch_true, locals);
            }
            ExprKind::Binary(op, l, r) => {
                let is_eq = matches!(op, BinOp::Identical);
                let is_neq = matches!(op, BinOp::NotIdentical);
                if is_eq || is_neq {
                    let matches_when_true = if is_eq { branch_true } else { !branch_true };
                    self.apply_equality_narrowing(l, r, locals, matches_when_true);
                } else if matches!(op, BinOp::Eq | BinOp::NotEq) {
                    // PHP's loose comparison (`comparison.rs`).
                    let equal = matches!(op, BinOp::Eq) == branch_true;
                    self.apply_loose_equality_narrowing(l, r, locals, equal);
                } else if let Some(ordering) = crate::checker::operators::ordering_op(op) {
                    self.apply_ordering_narrowing(l, r, ordering, branch_true, locals);
                }
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                self.apply_narrowing(inner, locals, !branch_true);
            }
            ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) => {
                self.apply_narrowing(inner, locals, branch_true)
            }
            // `(bool) $x`'s own truthiness is exactly `$x`'s -
            // PHP's cast doesn't change which values are truthy/falsy, it
            // just makes the check explicit (a common, idiomatic style:
            // `if (! (bool) $fp) { throw ...; }` after `fopen()`) - so
            // this narrows precisely as if the cast weren't there at all.
            // Any other cast (`(int)`, `(string)`, ...) genuinely changes
            // the value and isn't narrowable this way, so it's left alone.
            ExprKind::Cast(cast, inner) if cast.kind() == CastKind::Bool => {
                self.apply_narrowing(inner, locals, branch_true);
            }
            // `empty($x)` is PHP's falsy check: false leaves the truthy
            // values, true the falsy ones - for a defined variable, which
            // `empty()` of an undefined one can't be told apart from.
            ExprKind::Empty(operand) => {
                let defined = is_defined_variable(operand, locals);
                self.narrow_emptiness(operand, branch_true, defined, locals);
            }
            // `isset($x)`'s truthy branch means specifically "not null" -
            // it's true for `0`, `''`, `false`, an empty array - so this
            // narrows only `Null` away. The false branch says one operand
            // is null or unset: with a single operand that's a defined
            // variable, it's null.
            ExprKind::Isset(operands) => {
                if let [operand] = operands.as_slice()
                    && let Some(key) = narrowing_key(operand)
                {
                    // The identical `isset($x)` re-checked later replays
                    // what the first occurrence's matching branch learned
                    // (`truthiness_discriminant_subject`), both branches.
                    self.replay_truthiness_discriminant_facts(
                        &format!("isset({key})"),
                        locals,
                        branch_true,
                    );
                }
                if branch_true {
                    for operand in operands {
                        if let Some(key) = narrowing_key(operand) {
                            let current = self.current_narrowed_type(operand, &key, locals);
                            locals.insert(key, current.without(&Type::null()));
                        }
                        self.narrow_isset_containers(operand, locals);
                    }
                } else if let [operand] = operands.as_slice()
                    && is_defined_variable(operand, locals)
                    && let Some(key) = narrowing_key(operand)
                {
                    let current = self.current_narrowed_type(operand, &key, locals);
                    let null = if self
                        .relation()
                        .is_super_type_of(&current, &Type::null())
                        .no()
                    {
                        Type::never()
                    } else {
                        Type::null()
                    };
                    locals.insert(key, null);
                }
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
                    // The function the call reaches: `\is_array($x)` and
                    // `is_array($x)` in a namespace alike, but not a
                    // namespaced function of the same name.
                    let name = self.registry.called_function_name(name);
                    if name == "count" && branch_true {
                        // A bare `if (count($x))`/`while
                        // (count($x))` truthiness check, not the `> 0`/
                        // `>= 1` comparison form the `Binary` arm above
                        // already handles - `count()` is never negative,
                        // so a truthy result means exactly the same
                        // "provably non-empty" fact. The false branch
                        // (`count($x) === 0`) isn't narrowed here, the
                        // same asymmetry the comparison form's own arm
                        // already accepts (`Type::to_non_empty_array`
                        // has no "prove empty" counterpart).
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        locals.insert(key, current.to_non_empty_array());
                    } else if name == "array_is_list" {
                        // Passing proves list-ness. Failing proves the
                        // array non-empty, as an empty array is a list.
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        let narrowed = if branch_true {
                            current.to_list()
                        } else {
                            current.to_non_list()
                        };
                        locals.insert(key, narrowed);
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
                            let excluded = [
                                Type::bool(),
                                Type::bool_literal(true),
                                Type::bool_literal(false),
                                Type::null(),
                            ]
                            .into_iter()
                            .fold(current.into_owned(), |acc, excluded| {
                                self.exclude(&acc, &excluded)
                            });
                            excluded.with_string_accessory(AccessoryKind::NumericString)
                        } else {
                            // Int/Float are unconditionally numeric, so
                            // failing is_numeric proves the value wasn't
                            // one - nothing else can be dropped this way.
                            [Type::int(), Type::float()]
                                .into_iter()
                                .fold(current.into_owned(), |acc, excluded| {
                                    self.exclude(&acc, &excluded)
                                })
                        };
                        locals.insert(key, narrowed);
                    } else if let Some(target_ty) = narrowing_type_for_predicate(name) {
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        let narrowed = if branch_true {
                            self.relation().narrow(&current, &target_ty)
                        } else {
                            self.exclude(&current, &target_ty)
                        };
                        locals.insert(key, narrowed);
                    } else if let ExprKind::Name(raw) = &callee.kind {
                        self.apply_function_conditional_asserts(raw, args, locals, branch_true);
                    }
                } else if let ExprKind::Name(name) = &callee.kind
                    && name.trim_start_matches('\\') == "in_array"
                    && let [needle, haystack, strict] = args.as_slice()
                    // Only the strict form - a loose comparison's type
                    // juggling makes the real match set wider than the
                    // haystack's own values.
                    && matches!(&strict.value.kind, ExprKind::Literal(Literal::Bool(true)))
                {
                    self.narrow_strict_membership(
                        &needle.value,
                        &haystack.value,
                        branch_true,
                        locals,
                    );
                } else if let ExprKind::Name(name) = &callee.kind
                    && name.trim_start_matches('\\') == "in_array"
                    && let [needle, haystack, rest @ ..] = args.as_slice()
                    && !rest.iter().any(|strict| {
                        !matches!(&strict.value.kind, ExprKind::Literal(Literal::Bool(false)))
                    })
                {
                    self.narrow_loose_membership(
                        &needle.value,
                        &haystack.value,
                        branch_true,
                        locals,
                    );
                } else if let ExprKind::Name(name) = &callee.kind
                    && name.trim_start_matches('\\') == "array_key_exists"
                    && let [key_arg, arr_arg] = args.as_slice()
                {
                    self.narrow_key_presence(&key_arg.value, &arr_arg.value, branch_true, locals);
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
                    let needle_ty = self.infer_expr(&needle.value, locals);
                    if let Some(needle_lit) = needle_ty.string_literal_value()
                        && !needle_lit.is_empty()
                    {
                        let kind = if needle_lit == "0" {
                            AccessoryKind::NonEmptyString
                        } else {
                            AccessoryKind::NonFalsyString
                        };
                        let current = self.current_narrowed_type(&haystack.value, &key, locals);
                        locals.insert(key, current.with_string_accessory(kind));
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
                    // does. `Foo::class` is a `ClassNameFetch`, not a
                    // string `literal_string_value` handles.
                    let class = match &class_arg.value.kind {
                        ExprKind::ClassNameFetch(target) => {
                            target.as_name().map(|name| self.resolve_name_expr(name))
                        }
                        _ => literal_string_value(&class_arg.value)
                            .map(|name| self.class_type_named(&name)),
                    };
                    if let Some(resolved) = class {
                        let current = self.current_narrowed_type(&obj_arg.value, &key, locals);
                        if branch_true {
                            locals.insert(
                                key,
                                self.relation().narrow_to_instance(&current, &resolved),
                            );
                        } else {
                            locals.insert(key, self.exclude(&current, &resolved));
                        }
                    }
                } else if let ExprKind::Name(name) = &callee.kind {
                    self.apply_function_conditional_asserts(name, args, locals, branch_true);
                }
            }
            // `@phpstan-assert-if-true`/`-if-false` on a method's own
            // docblock - see `FunctionReflection::assert_if_true`'s own doc
            // comment for the full picture. `recv_class` picks a
            // representative member out of a `Union`/`Intersection`
            // receiver the exact same way `checker/expr.rs`'s own
            // `MethodCall` arm already does for ordinary method-call
            // checking - a variable typed as the union of several
            // concrete subclasses that all inherit the identical method
            // resolves the same `AssertInfo` any single member would.
            // Once resolved, `apply_assert_infos`/`Relation::narrow`
            // already narrow the receiver's own full current type
            // generically, Union or not.
            ExprKind::MethodCall(recv, method, args, _) => {
                let recv_ty = self.infer_expr(recv, locals);
                let class = recv_ty.object_class_names().first().map(|c| (*c).clone());
                if let Some(class) = class
                    && let Some(sig) = self.registry.lookup_method(&class, method)
                {
                    let infos = if branch_true {
                        sig.assert_if_true()
                    } else {
                        sig.assert_if_false()
                    };
                    if !infos.is_empty() {
                        self.apply_assert_infos(infos, sig, Some(recv), args, locals);
                    }
                }
            }
            // Same idea as `MethodCall` just above, for a static method
            // call - no receiver expression exists here at all, so only
            // a `Param`-targeted assertion can ever resolve.
            ExprKind::StaticCall(target, method, args) => {
                if let Some(class) = self.infer_static_target_class(target, locals)
                    && let Some(sig) = self.registry.lookup_method(&class, method)
                {
                    let infos = if branch_true {
                        sig.assert_if_true()
                    } else {
                        sig.assert_if_false()
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
                // `Type::without` already does soundly elsewhere.
                // A dynamic target (`$x instanceof $className`) still
                // doesn't name a concrete class to narrow to either way.
                if let Some(key) = narrowing_key(inner)
                    && let InstanceofTarget::Name(class_name) = target
                {
                    let resolved = self.resolve_name_expr(class_name);
                    if branch_true {
                        self.replay_object_discriminant_facts(&key, &resolved, locals);
                        let current = self.current_narrowed_type(inner, &key, locals);
                        locals.insert(key, self.relation().narrow_to_instance(&current, &resolved));
                    } else {
                        let current = self.current_narrowed_type(inner, &key, locals);
                        locals.insert(key, self.exclude(&current, &resolved));
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
            ExprKind::Assign(lhs, rhs) | ExprKind::AssignRef(lhs, rhs) => {
                let rhs_ty = self.infer_expr(rhs, locals);
                let native = match (&cond.kind, &lhs.kind) {
                    (ExprKind::Assign(..), ExprKind::Variable(name)) => {
                        Some((name.clone(), self.native_expr_type(rhs, locals)))
                    }
                    _ => None,
                };
                self.bind_assignment_target(lhs, rhs_ty, locals);
                if let Some((name, native)) = native {
                    locals.set_native(&name, native);
                }
                if branch_true
                    && let Some(key) = narrowing_key(lhs)
                    && let Some(current) = locals.get(&key).cloned()
                {
                    locals.insert(key, current.without_falsy());
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
                if let Some((_, alias)) = locals.condition_aliases().get(name.as_ref()).cloned() {
                    self.apply_narrowing(&alias, locals, branch_true);
                }
                self.replay_truthiness_discriminant_facts(name, locals, branch_true);
                if branch_true {
                    let current = self.current_narrowed_type(cond, name, locals);
                    locals.insert(name.to_string(), current.without_falsy());
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
                locals.insert(key, current.without_falsy());
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
        if target_keys.iter().any(|k| **k == **name) {
            return;
        }
        locals.set_condition_alias(name.to_string(), target_keys, std::rc::Rc::new(rhs.clone()));
    }

    /// A different shape from `record_condition_alias` above: a flag
    /// variable set unconditionally *inside* an `if`'s own body (`if (A
    /// || B) { $flag = true; }`), not via a direct `$flag = A || B;`
    /// assignment *expression*. Confirmed real corpus shape:
    /// `ImageDerivativeController`'s own `$need_generate = false; if
    /// ($derivative_mtime === false or $derivative_mtime < $src_mtime or
    /// ...) { $need_generate = true; } ...; if (! $need_generate) { ...
    /// gmdate(..., $derivative_mtime) ... }` - reaching `!$need_generate`
    /// provably means every disjunct of the original `if`'s own
    /// condition was false (including `$derivative_mtime === false`),
    /// but nothing records that without this.
    ///
    /// Storing `cond` itself (not a synthetic negation) is enough:
    /// `apply_narrowing`'s existing `ExprKind::Variable`/`Unary(Not,
    /// ...)` dispatch already replays a stored alias at whichever
    /// polarity the flag itself is tested at, so testing `!$need_generate`
    /// later recurses into replaying `cond` with `branch_true` flipped -
    /// proving the whole `A || B || ...` false exactly when
    /// `!$need_generate` holds, via the identical OR-chain narrowing
    /// mechanism a direct alias already gets.
    ///
    /// Recorded into *both* `then_locals` and `else_locals` directly
    /// (with the identical `Rc`, not just one side left to `merge_scopes`'s
    /// own "both branches must agree" `Rc::ptr_eq` filter) - unlike an
    /// ordinary top-level `record_condition_alias` call (which runs
    /// before any branching, so both forks inherit the same `Rc` for
    /// free), this alias is only ever created on one specific path
    /// through the `if`, so `else_locals` needs it written in explicitly
    /// or the merge would drop it (else_locals never independently
    /// produces a matching entry for `$need_generate` on its own). Sound
    /// in `else_locals` too: reaching it already means `cond` was false
    /// (`check_if`'s own `apply_narrowing(cond, else_locals, false)`
    /// already applied there), so replaying it again later is at worst
    /// redundant, never wrong.
    ///
    /// Scoped deliberately narrow: only a single, brace-or-bare `$flag =
    /// <literal>;` statement as the *entire* `then_branch` body, and only
    /// when the flag already had a real prior value before the `if` (the
    /// real shape this targets - an as-yet-undefined flag wouldn't
    /// survive `merge_scopes`'s own "present in only one branch" drop
    /// anyway, so aliasing it here would never be consulted). A branch
    /// that does anything else alongside the flag assignment isn't
    /// provably "this flag's value is fully determined by `cond` alone",
    /// so it's left alone rather than risking a wrong alias.
    pub(crate) fn record_if_body_flag_alias(
        &self,
        cond: &Expr,
        then_branch: &Stmt,
        then_locals: &mut Locals,
        else_locals: &mut Locals,
    ) {
        let single = match &then_branch.kind {
            StmtKind::Block(stmts) => match stmts.as_slice() {
                [only] => only,
                _ => return,
            },
            _ => then_branch,
        };
        let StmtKind::Expression(assign) = &single.kind else {
            return;
        };
        let ExprKind::Assign(lhs, rhs) = &assign.kind else {
            return;
        };
        let ExprKind::Variable(name) = &lhs.kind else {
            return;
        };
        if !matches!(rhs.kind, ExprKind::Literal(_)) {
            return;
        }
        if !else_locals.contains_key(name) {
            return;
        }
        if !condition_alias_shape(cond) {
            return;
        }
        let Some(target_keys) = condition_alias_targets(cond) else {
            return;
        };
        if target_keys.iter().any(|k| **k == **name) {
            return;
        }
        let alias = std::rc::Rc::new(cond.clone());
        then_locals.set_condition_alias(name.to_string(), target_keys.clone(), alias.clone());
        else_locals.set_condition_alias(name.to_string(), target_keys, alias);
    }

    /// `AssertRuleHelper`'s equivalent - a `@phpstan-
    /// assert`-family tag's own type is already resolved into a real
    /// `AssertInfo` (`FunctionReflection::assert`/`assert_if_true`/
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
    pub(crate) fn check_assert_tag_types(&mut self, sig: Option<&FunctionReflection>, span: Span) {
        let Some(sig) = sig else {
            return;
        };
        for (tag_name, infos) in [
            ("@phpstan-assert", sig.assert()),
            ("@phpstan-assert-if-true", sig.assert_if_true()),
            ("@phpstan-assert-if-false", sig.assert_if_false()),
        ] {
            for info in infos {
                self.check_one_assert_tag(info, tag_name, sig, span);
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
        sig: &FunctionReflection,
        span: Span,
    ) {
        let target_desc = match &info.target {
            AssertTarget::Param(name) => format!("${name}"),
            AssertTarget::ThisSelf => "$this".to_string(),
            AssertTarget::ThisProp(prop) => format!("$this->{prop}"),
            AssertTarget::NotFound(name) => {
                self.report(
                    format!("Assert references unknown parameter ${name}."),
                    span,
                    0,
                    "parameter.notFound",
                );
                return;
            }
            AssertTarget::Unknown(raw) => {
                self.report(
                    format!("Assert references unknown {raw}."),
                    span,
                    0,
                    "assert.unknownExpr",
                );
                return;
            }
        };
        let subject = format!("PHPDoc tag {tag_name} for {target_desc}");
        let class = self.current_class.clone();
        if self.type_node_unresolvable(&info.written_ty, class.as_deref()) {
            self.report(
                format!("{subject} contains unresolvable type."),
                span,
                2,
                "assert.unresolvableType",
            );
            return;
        }
        // `AssertRuleHelper`'s missing-typehint checks on the asserted
        // type itself.
        let explicit_mixed = crate::registry::mentions_mixed(&info.written_ty);
        self.report_missing_typehint_findings(
            span,
            |desc| format!("{subject} has no value type specified in {desc}."),
            |class_desc, list| {
                format!(
                    "{subject} contains generic {class_desc} but does not specify its types: {list}"
                )
            },
            &info.ty,
            explicit_mixed,
        );
        self.check_type_class_names_exist(&info.ty, &subject, span);
        self.check_type_trait_refs(&info.ty, &subject, span, "assert.trait");
        self.check_type_class_name_case(&info.case_refs, span);

        let AssertTarget::Param(name) = &info.target else {
            return;
        };
        let Some(param) = sig.params().iter().find(|p| p.name() == name) else {
            return;
        };
        let negated_prefix = if info.negated { "negated " } else { "" };
        let is_super = self.accepts(&info.ty, param.ty()).yes();
        let is_disjoint = self.relation().disjoint(&info.ty, param.ty());
        let (impossible, already_narrowed) = if info.negated {
            (is_super, is_disjoint)
        } else {
            (is_disjoint, is_super)
        };
        if impossible {
            self.report(format!(
                    "Asserted {negated_prefix}type {} for {target_desc} with type {} can never happen.",
                    info.ty, param.ty()
                ),
                span,
                0,
                "assert.impossibleType",
            );
        } else if already_narrowed {
            self.report(format!(
                    "Asserted {negated_prefix}type {} for {target_desc} with type {} does not narrow down the type.",
                    info.ty, param.ty()
                ),
                span,
                0,
                "assert.alreadyNarrowedType",
            );
        }
    }
}

/// Whether `e` is a plain variable this scope has defined.
fn is_defined_variable(e: &Expr, locals: &Locals) -> bool {
    matches!(&e.kind, ExprKind::Variable(name) if locals.contains_key(name))
}

/// `($a <=> $b) op c` as the comparison of `$a` and `$b` it encodes, for a
/// constant `c`: an ordering, or `None` for loose equality. `None` overall
/// when `cond` isn't one.
fn spaceship_comparison<'e>(
    op: &BinOp,
    l: &'e Expr,
    r: &'e Expr,
) -> Option<(Option<types::OrderingOp>, &'e Expr, &'e Expr)> {
    let (spaceship, constant, op) = match (&l.peeled().kind, &r.peeled().kind) {
        (ExprKind::Binary(BinOp::Spaceship, ..), _) => (l.peeled(), r, *op),
        (_, ExprKind::Binary(BinOp::Spaceship, ..)) => (r.peeled(), l, mirror_comparison(op)),
        _ => return None,
    };
    let ExprKind::Binary(BinOp::Spaceship, a, b) = &spaceship.kind else {
        return None;
    };
    let n = match &constant.peeled().kind {
        ExprKind::Literal(Literal::Int(n)) => *n,
        ExprKind::Unary(UnaryOp::Neg, inner) => match &inner.kind {
            ExprKind::Literal(Literal::Int(n)) => -*n,
            _ => return None,
        },
        _ => return None,
    };
    use types::OrderingOp::*;
    let encoded = match (op, n) {
        (BinOp::Lt, 0) | (BinOp::Identical | BinOp::Eq, -1) | (BinOp::LtEq, -1) => Some(Lt),
        (BinOp::Gt, 0) | (BinOp::Identical | BinOp::Eq, 1) | (BinOp::GtEq, 1) => Some(Gt),
        (BinOp::LtEq, 0) => Some(LtEq),
        (BinOp::GtEq, 0) => Some(GtEq),
        (BinOp::Identical | BinOp::Eq, 0) => None,
        _ => return None,
    };
    Some((encoded, a, b))
}

/// The `count($x)` side of a comparison: `$x`, its mode argument, and
/// whether it's on the left.
fn comparison_count_operand<'e>(
    op: &BinOp,
    l: &'e Expr,
    r: &'e Expr,
) -> Option<(&'e Expr, Option<&'e Expr>, bool)> {
    if !matches!(
        op,
        BinOp::Lt
            | BinOp::LtEq
            | BinOp::Gt
            | BinOp::GtEq
            | BinOp::Eq
            | BinOp::NotEq
            | BinOp::Identical
            | BinOp::NotIdentical
    ) {
        return None;
    }
    count_call(l)
        .map(|(a, m)| (a, m, true))
        .or_else(|| count_call(r).map(|(a, m)| (a, m, false)))
}

impl Checker<'_> {
    /// `count($x) op n` (`holds`): `$x` narrowed to the count range it
    /// proves (`Relation::narrow_count`), then the other side as the
    /// comparison narrows it, against the count's type.
    fn apply_count_narrowing(
        &self,
        op: &BinOp,
        l: &Expr,
        r: &Expr,
        holds: bool,
        locals: &mut Locals,
    ) {
        let Some((counted, mode, count_on_left)) = comparison_count_operand(op, l, r) else {
            return;
        };
        let other = if count_on_left { r } else { l };
        let op = if count_on_left {
            *op
        } else {
            mirror_comparison(op)
        };
        let recursive =
            mode.is_some_and(|m| self.infer_expr(m, locals).int_literal_value() != Some(0));
        let other_ty = self.infer_expr(other, locals);
        self.narrow_counted(counted, recursive, op, &other_ty, holds, locals);
        // The other side, as the comparison narrows it against the count.
        let (l, r) = if count_on_left {
            (l, other)
        } else {
            (other, r)
        };
        match op {
            BinOp::Identical | BinOp::NotIdentical => {
                let equal = matches!(op, BinOp::Identical) == holds;
                self.apply_equality_narrowing(l, r, locals, equal);
            }
            BinOp::Eq | BinOp::NotEq => {
                let equal = matches!(op, BinOp::Eq) == holds;
                self.apply_loose_equality_narrowing(l, r, locals, equal);
            }
            _ => {
                if let Some(ordering) = crate::checker::operators::ordering_op(&op) {
                    let ordering = if count_on_left {
                        ordering
                    } else {
                        crate::checker::operators::ordering_op(&mirror_comparison(&op))
                            .unwrap_or(ordering)
                    };
                    self.apply_ordering_narrowing(l, r, ordering, holds, locals);
                }
            }
        }
    }
}

impl Checker<'_> {
    /// `empty(operand)` was `empty`: the falsy values stay when it was,
    /// the truthy ones when it wasn't. An undefined variable is empty too,
    /// so the empty side narrows only one `known_defined`.
    pub(crate) fn narrow_emptiness(
        &self,
        operand: &Expr,
        empty: bool,
        known_defined: bool,
        locals: &mut Locals,
    ) {
        let Some(key) = narrowing_key(operand) else {
            return;
        };
        let current = self.current_narrowed_type(operand, &key, locals);
        if !empty {
            locals.insert(key, current.without_falsy());
        } else if known_defined {
            locals.insert(key, current.only_falsy(&self.relation()));
        }
    }

    /// `array_key_exists(key, arr)` was `present`: a union member that
    /// can't answer so is dropped, an optional field becomes required or
    /// goes, and when present `arr[key]` reads the field's value. The key
    /// is a literal, an int one stringified as shape keys are.
    pub(crate) fn narrow_key_presence(
        &self,
        key: &Expr,
        arr: &Expr,
        present: bool,
        locals: &mut Locals,
    ) {
        let (Some(lit_key), Some(shape_key), Some(arr_key)) = (
            literal_string_key(key),
            crate::checker::literal_shape_key(key),
            narrowing_key(arr),
        ) else {
            return;
        };
        let current = self.current_narrowed_type(arr, &arr_key, locals);
        let narrowed = current.with_key_presence(&shape_key, present);
        if present && let Some(value_ty) = narrowed.offset_value_type(Some(&shape_key)) {
            locals.insert(format!("{arr_key}[{lit_key}]"), value_ty);
        }
        locals.insert(arr_key, narrowed);
    }

    /// `in_array(needle, haystack, true)` was `contained`: the needle is
    /// one of the haystack's values, or, against a haystack of constants,
    /// none of them.
    pub(crate) fn narrow_strict_membership(
        &self,
        needle: &Expr,
        haystack: &Expr,
        contained: bool,
        locals: &mut Locals,
    ) {
        let Some((target_ty, excluded_ty)) = self.in_array_narrowing(haystack, locals) else {
            return;
        };
        let Some(key) = narrowing_key(needle) else {
            return;
        };
        let current = self.current_narrowed_type(needle, &key, locals);
        let narrowed = if contained {
            Some(self.relation().narrow(&current, &target_ty))
        } else {
            excluded_ty.map(|excluded| self.exclude(&current, &excluded))
        };
        if let Some(narrowed) = narrowed {
            locals.insert(key, narrowed);
        }
    }
}

impl Checker<'_> {
    /// `count(counted) op other` was `holds`, with `other` of type
    /// `other_ty`: `counted` narrowed to the count range it proves
    /// (`Relation::narrow_count`); a recursive count proves only that it
    /// isn't empty.
    pub(crate) fn narrow_counted(
        &self,
        counted: &Expr,
        recursive: bool,
        op: BinOp,
        other_ty: &Type,
        holds: bool,
        locals: &mut Locals,
    ) {
        if let Some(key) = narrowing_key(counted)
            && other_ty.is_int().yes()
            && let Some((bl, bh)) = other_ty.int_bounds()
        {
            let (bl, bh) = (bl.unwrap_or(i64::MIN), bh);
            // `!=`/`!==` is `==`/`===` with the branches swapped.
            let (op, holds) = match op {
                BinOp::NotEq => (BinOp::Eq, !holds),
                BinOp::NotIdentical => (BinOp::Identical, !holds),
                other => (other, holds),
            };
            let range = match (op, holds) {
                (BinOp::Gt, true) => Some((bl.saturating_add(1), None)),
                (BinOp::Gt, false) => Some((0, bh)),
                (BinOp::GtEq, true) => Some((bl, None)),
                (BinOp::GtEq, false) => Some((0, bh.map(|h| h.saturating_sub(1)))),
                (BinOp::Lt, true) => Some((0, bh.map(|h| h.saturating_sub(1)))),
                (BinOp::Lt, false) => Some((bl, None)),
                (BinOp::LtEq, true) => Some((0, bh)),
                (BinOp::LtEq, false) => Some((bl.saturating_add(1), None)),
                (BinOp::Eq | BinOp::Identical, true) => Some((bl, bh)),
                _ => None,
            };
            let current = self.current_narrowed_type(counted, &key, locals);
            let narrowed = match range {
                // A recursive count proves only that the array isn't empty.
                Some((lo, _)) if recursive => (lo >= 1).then(|| current.to_non_empty_array()),
                Some((lo, hi)) => Some(self.relation().narrow_count(&current, lo.max(0), hi)),
                // `count($x) !== n` for one `n`: the members that hold
                // exactly `n` go, and `n = 0` leaves a non-empty array.
                None if !recursive && bh == Some(bl) => {
                    let kept: Vec<Type> = current
                        .union_members()
                        .iter()
                        .filter(|m| m.count_range() != Some((bl, Some(bl))))
                        .cloned()
                        .collect();
                    let kept = Type::union_of(kept);
                    Some(if bl == 0 {
                        kept.to_non_empty_array()
                    } else {
                        kept
                    })
                }
                None => None,
            };
            if let Some(narrowed) = narrowed {
                locals.insert(key, narrowed);
            }
        }
    }
}

impl Checker<'_> {
    /// `in_array(needle, haystack)` without `strict` was `contained`: the
    /// needle may loosely equal some value, or, against a haystack of
    /// constants, surely equals none of them.
    pub(crate) fn narrow_loose_membership(
        &self,
        needle: &Expr,
        haystack: &Expr,
        contained: bool,
        locals: &mut Locals,
    ) {
        let Some(key) = narrowing_key(needle) else {
            return;
        };
        let Some(values) = self.infer_expr(haystack, locals).array_value_type() else {
            return;
        };
        let current = self.current_narrowed_type(needle, &key, locals);
        let version = self.comparison_php_version();
        let narrowed = if contained {
            Some(
                self.relation()
                    .narrow_loose(&current, &values, true, version),
            )
        } else {
            let constants = values.finite_types();
            (!constants.is_empty()).then(|| {
                constants.iter().fold(current.into_owned(), |acc, c| {
                    self.relation().narrow_loose(&acc, c, false, version)
                })
            })
        };
        if let Some(narrowed) = narrowed {
            locals.insert(key, narrowed);
        }
    }
}
