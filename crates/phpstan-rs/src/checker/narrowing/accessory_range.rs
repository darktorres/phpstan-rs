use crate::checker::narrowing::predicates::narrowing_key;
use syntax::ast::*;
use types::{self, AccessoryKind, Type};

pub(crate) fn upgrade_array_to_non_empty(ty: &Type) -> Type {
    match ty {
        Type::Array(key, inner) => Type::intersection_of(vec![
            Type::array((**key).clone(), (**inner).clone()),
            Type::accessory(AccessoryKind::NonEmptyArray),
        ]),
        Type::Intersection(members)
            if members.iter().any(|m| matches!(m, Type::Array(_, _)))
                && !members.contains(&Type::Accessory(AccessoryKind::NonEmptyArray)) =>
        {
            let mut members = members.clone();
            members.push(Type::accessory(AccessoryKind::NonEmptyArray));
            Type::intersection_of(members)
        }
        // A genuinely empty `ArrayShape` is a real contradiction once
        // `count() > 0`/truthy is known - collapses to `Never` (the same
        // "this branch is impossible" signal `narrow_type_excluding_
        // falsy`'s own identical arm already uses) rather than staying
        // unchanged, so a `Union` member that's provably empty gets
        // dropped below instead of surviving as a spurious alternative.
        Type::ArrayShape(shape) if shape.is_empty_array() => Type::never(),
        // A shape whose keys are all optional can't be empty any more.
        Type::ArrayShape(_) if !ty.is_non_empty_array().yes() => types::intersect_types(vec![
            ty.clone(),
            Type::accessory(AccessoryKind::NonEmptyArray),
        ]),
        // A loop-built array whose own entry might still be its
        // pre-loop `[]` is a real `Union` including that empty-shape
        // alternative, which a `count($x) > 0` guard immediately
        // afterward should disprove entirely (dropped via the `Never`
        // arm just above), not merely leave untouched by falling to
        // `_ => ty.clone()`.
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(upgrade_array_to_non_empty)
                .filter(|m| *m != Type::Never)
                .collect();
            match kept.len() {
                0 => Type::never(),
                1 => kept.into_iter().next().unwrap(),
                _ => Type::union_of(kept),
            }
        }
        _ => ty.clone(),
    }
}

/// `<`/`>`/`<=`/`>=`'s own two-operand shape: which side is narrowable
/// (`narrowing_key` succeeds), which side is the comparison bound, and
/// whether the narrowable side was written on the left (`$x < N`) or the
/// right (`N < $x`) - `comparison_narrowed_bounds` needs to know the
/// latter to interpret the operator correctly (`N < $x` means `$x > N`,
/// not `$x < N`). `None` when neither side has a narrowing key at all.
pub(crate) fn int_comparison_operands<'e>(
    l: &'e Expr,
    r: &'e Expr,
) -> Option<(&'e Expr, &'e Expr, bool)> {
    if narrowing_key(l).is_some() {
        Some((l, r, true))
    } else if narrowing_key(r).is_some() {
        Some((r, l, false))
    } else {
        None
    }
}

/// `int_range_bounds`'s own "isn't range-representable at all" `None`
/// widened to also accept a bare `Type::Int` as the fully-unbounded range
/// `(None, None)` - the common starting point for a plain `int` parameter
/// before any comparison has narrowed it (`int_range_bounds` itself stays
/// strict, since `infer_binary`'s own arithmetic call sites need to tell
/// "unbounded `int`" apart from "not an int at all" for their own,
/// different reasons).
pub(crate) fn numeric_bounds_for_narrowing(ty: &Type) -> Option<(Option<i64>, Option<i64>)> {
    match ty {
        Type::Int => Some((None, None)),
        _ => ty.int_range_bounds(),
    }
}

/// A comparison's own truth value, canonicalized to "the narrowed side
/// is smaller/smaller-or-equal/greater/greater-or-equal than the bound" -
/// `<`/`<=`/`>`/`>=` all four collapse to Smaller/SmallerOrEqual here (a
/// negated comparison mirrors real PHPStan's own identical `Greater(l,
/// r)` -> `Smaller(r, l)` rewrite, `BinaryOpHandler.php`), so
/// `comparison_narrowed_bounds` only ever has to reason
/// about two shapes, not four.
enum ComparisonOutcome {
    Smaller,
    SmallerOrEqual,
    Greater,
    GreaterOrEqual,
}

impl ComparisonOutcome {
    fn negate(self) -> Self {
        match self {
            ComparisonOutcome::Smaller => ComparisonOutcome::GreaterOrEqual,
            ComparisonOutcome::SmallerOrEqual => ComparisonOutcome::Greater,
            ComparisonOutcome::Greater => ComparisonOutcome::SmallerOrEqual,
            ComparisonOutcome::GreaterOrEqual => ComparisonOutcome::Smaller,
        }
    }
}

/// The new `(min, max)` bound pair for the narrowed side, once `op`
/// (written as `l op r`, `keyed_on_left` saying which of `l`/`r` is the
/// narrowed side) is known true (`branch_true`) or false - `None` only on
/// `i64` overflow computing an exclusive bound's own off-by-one (`bound -
/// 1`/`bound + 1`), an edge real code never hits (a literal compared
/// against `i64::MIN`/`i64::MAX`), where leaving `locals` untouched is the
/// same "give up gracefully" choice every other range-arithmetic helper
/// in this project already makes rather than risk a wrong bound.
pub(crate) fn comparison_narrowed_bounds(
    op: &BinOp,
    keyed_on_left: bool,
    branch_true: bool,
    cur_min: Option<i64>,
    cur_max: Option<i64>,
    bound: i64,
) -> Option<(Option<i64>, Option<i64>)> {
    let outcome = match (op, keyed_on_left) {
        (BinOp::Lt, true) | (BinOp::Gt, false) => ComparisonOutcome::Smaller,
        (BinOp::LtEq, true) | (BinOp::GtEq, false) => ComparisonOutcome::SmallerOrEqual,
        (BinOp::Gt, true) | (BinOp::Lt, false) => ComparisonOutcome::Greater,
        (BinOp::GtEq, true) | (BinOp::LtEq, false) => ComparisonOutcome::GreaterOrEqual,
        _ => return None,
    };
    let outcome = if branch_true {
        outcome
    } else {
        outcome.negate()
    };
    match outcome {
        ComparisonOutcome::Smaller => {
            let candidate = bound.checked_sub(1)?;
            Some((
                cur_min,
                Some(cur_max.map_or(candidate, |m| m.min(candidate))),
            ))
        }
        ComparisonOutcome::SmallerOrEqual => {
            Some((cur_min, Some(cur_max.map_or(bound, |m| m.min(bound)))))
        }
        ComparisonOutcome::Greater => {
            let candidate = bound.checked_add(1)?;
            Some((
                Some(cur_min.map_or(candidate, |m| m.max(candidate))),
                cur_max,
            ))
        }
        ComparisonOutcome::GreaterOrEqual => {
            Some((Some(cur_min.map_or(bound, |m| m.max(bound))), cur_max))
        }
    }
}

/// `array_is_list($x)`'s own true-branch narrowing - mirrors `upgrade_
/// array_to_non_empty` exactly, just adding `ArrayList` instead of
/// `NonEmptyArray`. Doesn't force the key type itself to `Type::Int` -
/// every other list-producing call site in this project (`preg_split`,
/// `array_values`, ...) leaves the stored key as `unknown_array_key()`
/// too, relying on `describe_accessory_intersection`'s own display-layer
/// guard (a list's key is always `int` by definition, so showing it
/// would be redundant) rather than encoding it twice. A docblock-parsed
/// `list<T>` still uses a literal `Type::Int` key instead (needed
/// elsewhere - see `parse_type_string`'s own `"list"` arm), so `types::
/// subtype`'s dedicated list-vs-list `Intersection` arm and `types::
/// types_equal_for_union` both compare the two representations ignoring
/// key entirely, rather than this project settling on one encoding
/// everywhere.
pub(crate) fn upgrade_array_to_list(ty: &Type) -> Type {
    match ty {
        Type::Array(key, inner) => Type::intersection_of(vec![
            Type::array((**key).clone(), (**inner).clone()),
            Type::accessory(AccessoryKind::ArrayList),
        ]),
        Type::Intersection(members)
            if members.iter().any(|m| matches!(m, Type::Array(_, _)))
                && !members.contains(&Type::Accessory(AccessoryKind::ArrayList)) =>
        {
            let mut members = members.clone();
            members.push(Type::accessory(AccessoryKind::ArrayList));
            Type::intersection_of(members)
        }
        Type::Union(members) => Type::union_of(members.iter().map(upgrade_array_to_list).collect()),
        _ => ty.clone(),
    }
}

/// A failing `array_is_list($x)`: no list alternative survives, and what
/// does is non-empty, as an empty array is a list.
pub(crate) fn narrow_array_to_non_list(ty: &Type) -> Type {
    match ty {
        Type::Intersection(members)
            if members.contains(&Type::Accessory(AccessoryKind::ArrayList)) =>
        {
            Type::never()
        }
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(narrow_array_to_non_list)
                .filter(|m| *m != Type::Never)
                .collect();
            match kept.len() {
                0 => Type::never(),
                1 => kept.into_iter().next().unwrap(),
                _ => Type::union_of(kept),
            }
        }
        _ => upgrade_array_to_non_empty(ty),
    }
}
