use crate::ast::*;
use crate::checker::expr::type_definitely_excludes_null;
use crate::checker::is_unsealed_shape;
use crate::checker::type_union::union_types;
use crate::types::{self, AccessoryKind, Type};

/// `ty`'s own `(min, max)` bound pair when it's
/// range-representable at all - an `IntLiteral` is a degenerate range
/// with `min == max`, matching `IntRange`'s own doc comment ("the same
/// relationship `IntLiteral` has to `Int`"). `infer_binary`'s own
/// `Add`/`Sub` arms use this to compute a real, narrow arithmetic result
/// range instead of collapsing straight to bare `Int`.
pub(crate) fn int_range_bounds(ty: &Type) -> Option<(Option<i64>, Option<i64>)> {
    match ty {
        Type::IntLiteral(n) => Some((Some(*n), Some(*n))),
        Type::IntRange(min, max) => Some((*min, *max)),
        _ => None,
    }
}

/// The inverse of `int_range_bounds`: collapses back to the most precise
/// representable type - `Type::Int` when both ends are unbounded (an
/// `IntRange(None, None)` is never constructed elsewhere in this
/// project), `Type::IntLiteral` when both ends coincide, `Type::IntRange`
/// otherwise. An inverted range (`min > max`) is genuinely empty - no
/// real value satisfies it - matching real PHPStan's own `IntegerRangeType
/// ::fromInterval`'s identical `if ($min > $max) { return new NeverType(); }`
/// guard exactly; `div_int_range`'s own rounding step (`ceil`ing the min
/// and `floor`ing the max independently, real PHPStan's own doc comment
/// there) is the one caller that can genuinely produce this shape - a
/// `Div` result with no possible integer value at all, only the `Float`
/// alternative `union_types` there still unions in (`10 / 4` is always
/// exactly `2.5`, never any int, so its own int "half" collapses to
/// `Never`, absorbed away by the union, correctly leaving bare `float`).
pub(crate) fn normalize_int_range(min: Option<i64>, max: Option<i64>) -> Type {
    match (min, max) {
        (None, None) => Type::Int,
        (Some(a), Some(b)) if a > b => Type::Never,
        (Some(a), Some(b)) if a == b => Type::IntLiteral(a),
        (min, max) => Type::IntRange(min, max),
    }
}

/// Applies `op` to two bounds, distinguishing a genuine `i64` overflow
/// (both operands were real values, but the exact result doesn't fit)
/// from a merely-unbounded operand (`None` propagating as `None` is
/// correct there, not an overflow) - `bool` is `true` only for the
/// former, the signal `Add`/`Sub`/`Mul` need to union `Type::Float`
/// into their result the same way real PHP silently promotes an
/// overflowing int operation to float at runtime.
pub(crate) fn checked_bound_op(
    a: Option<i64>,
    b: Option<i64>,
    op: impl Fn(i64, i64) -> Option<i64>,
) -> (Option<i64>, bool) {
    match (a, b) {
        (Some(x), Some(y)) => match op(x, y) {
            Some(v) => (Some(v), false),
            None => (None, true),
        },
        _ => (None, false),
    }
}

/// An unbounded end, mapped to signed infinity - `mul_int_range`'s own
/// corner-product formula needs this so a single formula covers every
/// combination of bounded/unbounded ends uniformly, matching real
/// PHPStan's own use of `-INF`/`INF` there directly (`?? -INF`/`?? INF`).
fn bound_or_inf(v: Option<i64>, inf: f64) -> f64 {
    v.map(|n| n as f64).unwrap_or(inf)
}

/// The inverse of `bound_or_inf`: a finite corner product collapses back
/// to a real `i64` bound; a non-finite one (either side was genuinely
/// unbounded, or the product's own magnitude doesn't fit any real `int`
/// PHP could ever produce) collapses to `None` - matching real PHPStan's
/// own `is_finite($min) ? ... : null` check, extended with an explicit
/// `i64`-range guard this project needs and PHP's own arbitrary-precision
/// `float` arithmetic doesn't (a product that's finite as `f64` can still
/// overflow `i64`).
fn finite_bound(v: f64) -> Option<i64> {
    if v.is_finite() && v >= i64::MIN as f64 && v <= i64::MAX as f64 {
        Some(v as i64)
    } else {
        None
    }
}

/// Real PHPStan's own `InitializerExprTypeResolver::integerRangeMath`'s
/// `Mul` arm, transcribed directly - the four "corner product" formula
/// (every combination of each side's own min/max, substituting signed
/// infinity for an unbounded end) needs no sign-based case-splitting at
/// all, unlike `Div`'s own arm, since a product's extremes are always
/// among these four corners regardless of which operand is negative.
/// `0 * (anything, including an unbounded end)` is special-cased to `0`
/// first, matching the real source exactly - naively substituting
/// infinity there would compute `0 * INF = NaN` instead.
pub(crate) fn mul_int_range(
    l_min: Option<i64>,
    l_max: Option<i64>,
    r_min: Option<i64>,
    r_max: Option<i64>,
) -> Type {
    let corner = |a: Option<i64>, b: Option<i64>, a_inf: f64, b_inf: f64| -> f64 {
        if a == Some(0) || b == Some(0) {
            0.0
        } else {
            bound_or_inf(a, a_inf) * bound_or_inf(b, b_inf)
        }
    };
    let min1 = corner(l_min, r_min, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let min2 = corner(l_min, r_max, f64::NEG_INFINITY, f64::INFINITY);
    let max1 = corner(l_max, r_min, f64::INFINITY, f64::NEG_INFINITY);
    let max2 = corner(l_max, r_max, f64::INFINITY, f64::INFINITY);
    let min = min1.min(min2).min(max1).min(max2);
    let max = min1.max(min2).max(max1).max(max2);
    let (min_bound, max_bound) = (finite_bound(min), finite_bound(max));
    let result = normalize_int_range(min_bound, max_bound);
    // With every input bound present (no `?? -INF`/`?? INF` substitution
    // ever used above), a non-finite corner can only be a genuine `i64`
    // overflow, not a propagated unbounded operand - the same real-PHP
    // int-to-float promotion `checked_bound_op` signals for `Add`/`Sub`.
    let all_bounded = l_min.is_some() && l_max.is_some() && r_min.is_some() && r_max.is_some();
    if all_bounded && (min_bound.is_none() || max_bound.is_none()) {
        union_types(result, Type::Float)
    } else {
        result
    }
}

/// Real PHPStan's own `integerRangeMath`'s `Minus` arm's own range-minus-
/// range case (the constant-right-operand shortcut is already handled
/// separately, at this project's own `Sub` arm's call site, before this
/// is ever reached) - transcribed directly, including its own sequential
/// mutation of `min`/`max` (the second block can still overwrite `min`
/// again, in exactly one sub-case) rather than "cleaned up" into
/// independent expressions, since correctness here depends on matching
/// the real source's own execution order exactly, not just its inputs
/// and outputs in isolation.
pub(crate) fn sub_int_range_full(
    l_min: Option<i64>,
    l_max: Option<i64>,
    r_min: Option<i64>,
    r_max: Option<i64>,
) -> Type {
    let (min, max) = if l_min == l_max && l_min.is_some() && (r_min.is_none() || r_max.is_none()) {
        (None, l_min)
    } else {
        // `checked_sub` throughout, not plain `-`: see `infer_binary`'s
        // own `Add` arm doc comment for why (real PHP silently promotes
        // an overflowing subtraction to `float` instead of panicking/
        // wrapping the way plain `i64::-` would on adversarial input).
        let mut min = match (l_min, r_min) {
            (_, None) => None,
            (Some(lm), Some(_)) => lm.checked_sub(r_max.or(r_min).unwrap()),
            (None, Some(_)) => None,
        };
        let max;
        match (r_max, l_max) {
            // real PHPStan's own first branch (`$operand->getMax() ===
            // null`): both ends collapse to null, overwriting whatever
            // `min` above already computed.
            (None, _) => {
                min = None;
                max = None;
            }
            (Some(rmax), Some(lmax)) => {
                if let Some(lm) = l_min
                    && r_min.is_none()
                {
                    min = lm.checked_sub(rmax);
                    max = None;
                } else if let Some(rmin) = r_min {
                    max = lmax.checked_sub(rmin);
                } else {
                    max = None;
                }
            }
            // real PHPStan's own final `else` (`$rangeMax === null`):
            // only `max` collapses to null - `min`, computed above,
            // deliberately stays untouched here.
            (Some(_), None) => {
                max = None;
            }
        }
        if let (Some(mn), Some(mx)) = (min, max)
            && mn > mx
        {
            (Some(mx), Some(mn))
        } else {
            (min, max)
        }
    };
    normalize_int_range(min, max)
}

/// `base ** exp` for two plain `i64`s, `None` whenever real PHP itself
/// would silently promote the result to `float` instead of a real `int` -
/// a negative exponent (a fractional result) or an overflowing one (PHP's
/// own general int-overflow-to-float behavior, which `pow()`/`**` share
/// with `+`/`-`/`*`) - mirroring real PHPStan's own `is_float($min)`/
/// `is_float($max)` checks in `pow_int_range`'s own two call sites below.
fn checked_i64_pow(base: i64, exp: i64) -> Option<i64> {
    let exp_u32 = u32::try_from(exp).ok()?;
    base.checked_pow(exp_u32)
}

/// Real PHPStan's own `ExponentiateHelper`/`IntegerRangeType::exponentiate`,
/// transcribed for the pure-integer case this project's own `IntRange`
/// represents - `infer_binary`'s own `Pow` arm already routes a `Float`-
/// involving operand to bare `Type::Float` before ever reaching this, the
/// same gate every one of real PHPStan's own non-numeric-exponent cases
/// (`string`/`bool`/`null`) this project's type system doesn't route
/// through this path at all. A literal base's own `(min, max)` bounds
/// coincide (`int_range_bounds`'s own doc comment), so this single
/// formula - real PHPStan's own two separate call sites, one for a
/// `ConstantScalarType` base and one for an `IntegerRangeType` base,
/// happen to collapse into exactly the same shape here - correctly
/// covers both a literal and a genuine range base with no separate case.
pub(crate) fn pow_int_range(
    l_min: Option<i64>,
    l_max: Option<i64>,
    r_min: Option<i64>,
    r_max: Option<i64>,
) -> Type {
    let min = l_min.zip(r_min).and_then(|(b, e)| checked_i64_pow(b, e));
    let max = l_max.zip(r_max).and_then(|(b, e)| checked_i64_pow(b, e));
    if min.is_some() || max.is_some() {
        normalize_int_range(min, max)
    } else {
        Type::Union(vec![Type::Int, Type::Float])
    }
}

/// Real PHPStan's own `InitializerExprTypeResolver::integerRangeMath`'s
/// `ShiftLeft`/`ShiftRight` arms, transcribed - unlike `Mul`/`Pow`, real
/// PHPStan only ever reaches this formula for a *single constant* right
/// operand (`getShiftLeftType`/`getShiftRightType` themselves bail to
/// plain `int` the moment the right side isn't a `ConstantIntegerType` -
/// no range-shifted-by-range case exists at all), so
/// `infer_binary`'s own call site only invokes this once `int_range_bounds`
/// on the right side has already collapsed to a single value. `amount`
/// outside `0..64` (negative - PHP throws `ArithmeticError` at runtime,
/// not modeled as `ErrorType` here, matching this project's existing
/// choice not to surface that family - or too wide to be a real shift
/// amount) falls back to plain `Type::Int` directly, the same "give up
/// gracefully" shape every other range-arithmetic arm here already has.
/// Each bound shifts independently and collapses to `None` (unbounded)
/// on overflow rather than aborting the whole computation - matching
/// real PHPStan's own `is_float($min) ? null : $min` guard (PHP silently
/// promotes an overflowing int to `float`, which these two functions
/// then treat as "no longer a precise int bound").
pub(crate) fn shift_int_range(
    l_min: Option<i64>,
    l_max: Option<i64>,
    amount: i64,
    left: bool,
) -> Type {
    let Ok(amount) = u32::try_from(amount) else {
        return Type::Int;
    };
    if amount >= 64 {
        return Type::Int;
    }
    let shift_bound = |v: Option<i64>| -> Option<i64> {
        v.and_then(|v| {
            if left {
                v.checked_shl(amount)
            } else {
                Some(v >> amount)
            }
        })
    };
    normalize_int_range(shift_bound(l_min), shift_bound(l_max))
}

/// Whether `l OP r` (`op` one of `Lt`/`Gt`/`LtEq`/`GtEq`) is provably
/// always true or always false, given each side's own `(min, max)`
/// bounds (`None` for an unbounded end) - `Some(true)`/`Some(false)`
/// only when every possible value pair agrees, `None` for "depends on
/// the actual runtime values" (including when either side is
/// completely unbounded in the relevant direction). Mirrors real
/// PHPStan's own `IntegerRangeType::isSmallerThan`/`isSmallerThanOrEqual`
/// corner-comparison logic -
/// deliberately simplified to the pure-numeric case only, matching this
/// project's own established "no loose-comparison modeling" precedent
/// (`checker/narrowing.rs`'s own `comparison_narrowed_bounds`, the
/// identical scope decision for the narrowing direction of this same
/// comparison family).
pub(crate) fn compare_range_outcome(
    op: &BinOp,
    l: (Option<i64>, Option<i64>),
    r: (Option<i64>, Option<i64>),
) -> Option<bool> {
    let (l_min, l_max) = l;
    let (r_min, r_max) = r;
    match op {
        BinOp::Lt => {
            if let (Some(lmax), Some(rmin)) = (l_max, r_min)
                && lmax < rmin
            {
                return Some(true);
            }
            if let (Some(lmin), Some(rmax)) = (l_min, r_max)
                && lmin >= rmax
            {
                return Some(false);
            }
            None
        }
        BinOp::LtEq => {
            if let (Some(lmax), Some(rmin)) = (l_max, r_min)
                && lmax <= rmin
            {
                return Some(true);
            }
            if let (Some(lmin), Some(rmax)) = (l_min, r_max)
                && lmin > rmax
            {
                return Some(false);
            }
            None
        }
        BinOp::Gt => {
            if let (Some(lmin), Some(rmax)) = (l_min, r_max)
                && lmin > rmax
            {
                return Some(true);
            }
            if let (Some(lmax), Some(rmin)) = (l_max, r_min)
                && lmax <= rmin
            {
                return Some(false);
            }
            None
        }
        BinOp::GtEq => {
            if let (Some(lmin), Some(rmax)) = (l_min, r_max)
                && lmin >= rmax
            {
                return Some(true);
            }
            if let (Some(lmax), Some(rmin)) = (l_max, r_min)
                && lmax < rmin
            {
                return Some(false);
            }
            None
        }
        _ => None,
    }
}

/// Whether `e` reads a property (instance or static) anywhere in its
/// own expression tree - a guard for `check_constant_
/// loose_comparison`/`check_constant_numeric_comparison`: this project
/// doesn't invalidate a property's own narrowed type across an arbitrary
/// intervening method call (real PHPStan's own `MutatingScope` does,
/// generally assuming any call might mutate any property unless
/// proven otherwise), so e.g. `$this->retrieve_list
/// = [];` followed by `$this->dispatchListCheckIntegrity();` (which, per
/// its own name, populates it) then `if (count($this->retrieve_list) >
/// 0)` would otherwise still believe `$this->retrieve_list` is the
/// empty array literal it was last assigned, making `count(...)`'s own
/// exact-`IntLiteral(0)` special case (`array_shape`'s own zero-field
/// count) look "always false" against the literal `0` it's compared to,
/// even though the intervening call almost certainly changed it. Not
/// an exhaustive AST walk (covers the shapes that actually occur in
/// call arguments/operators: `Call`/`MethodCall` args and receiver,
/// `Binary`/`Unary`/`Grouping`/`Index`) - missing a rarer shape only
/// means this guard occasionally under-protects, never over-flags,
/// matching this project's own "favor false negatives" precedent.
pub(crate) fn expr_reads_a_property(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::PropertyAccess(..)
        | ExprKind::DynamicPropertyAccess(..)
        | ExprKind::StaticAccess(..) => true,
        ExprKind::Call(callee, args) => {
            expr_reads_a_property(callee) || args.iter().any(|a| expr_reads_a_property(&a.value))
        }
        ExprKind::MethodCall(base, _, args, _) => {
            expr_reads_a_property(base) || args.iter().any(|a| expr_reads_a_property(&a.value))
        }
        ExprKind::Binary(_, l, r) => expr_reads_a_property(l) || expr_reads_a_property(r),
        ExprKind::Unary(_, inner) | ExprKind::Grouping(inner) => expr_reads_a_property(inner),
        ExprKind::Index(base, idx) => {
            expr_reads_a_property(base) || idx.as_deref().is_some_and(expr_reads_a_property)
        }
        _ => false,
    }
}

/// `null_probe_verdict`/`empty_probe_verdict`'s own shared shape - a
/// null-probe or falsiness verdict function, selected once per `isset()`/
/// `empty()` call site (`existence_probe_kind`) and threaded through to
/// both `check_pointless_null_probe` and `check_pointless_offset_or_
/// property_probe`.
pub(crate) type NullProbeVerdictFn = fn(&Type) -> Option<&'static str>;

/// Bounds for `</>/<=/>=`'s own range-outcome check - a bare `Type::Int`
/// counts as an unbounded range (matching `checker::narrowing`'s own
/// `numeric_bounds_for_narrowing`), an ordinary, not-yet-narrowed `int`
/// parameter being exactly the common real case this needs to stay
/// silent on; anything else defers to `int_range_bounds`. Shared between
/// `infer_binary`'s own constant-folding arm and `check_constant_
/// numeric_comparison`'s standalone diagnostic below - both need the
/// identical answer.
pub(crate) fn numeric_comparison_bounds(ty: &Type) -> Option<(Option<i64>, Option<i64>)> {
    match ty {
        Type::Int => Some((None, None)),
        other => int_range_bounds(other),
    }
}

/// PHP's own bool-cast truthiness for `ty`, generalized past a bare
/// literal wherever the answer is still fully unambiguous - `None`
/// whenever it genuinely isn't (a bare `int`/`string`/`array`, a
/// variable's widened type, a `Union` whose members disagree, ...).
/// Real PHP's exact falsy set: `0`, `0.0`, `""`, `"0"`, `null`, `false`,
/// `[]`; everything else (every object, unconditionally - even one with
/// no properties) is truthy. `Union`/`Intersection` recurse: a union
/// needs every member to agree; an intersection only needs one member
/// with a definite answer, since narrowing only ever adds information.
/// `ArrayList` is deliberately excluded from the "always truthy"
/// accessory set below - unlike `NonEmptyArray`/`NonFalsyString`, a bare
/// `list<T>` doesn't itself exclude the empty list.
pub(crate) fn type_truthiness(ty: &Type) -> Option<bool> {
    match ty {
        Type::True => Some(true),
        Type::False | Type::Null => Some(false),
        Type::IntLiteral(n) => Some(*n != 0),
        Type::FloatLiteral(n) => Some(*n != 0.0),
        Type::StringLiteral(s) => Some(!s.is_empty() && s != "0"),
        Type::Object(..) => Some(true),
        Type::ArrayShape(fields) => {
            let real_fields: Vec<_> = fields.iter().filter(|(name, _, _)| name != "...").collect();
            if real_fields.iter().any(|(_, _, optional)| !optional) {
                // At least one *required* field guarantees the array can
                // never be empty, sealed or not.
                Some(true)
            } else if real_fields.is_empty() && !is_unsealed_shape(fields) {
                // No fields at all, and sealed - this is exactly `[]`.
                Some(false)
            } else {
                // Every known field is optional (or the shape is
                // unsealed with no known fields at all) - genuinely
                // ambiguous, it could be `[]` or not.
                None
            }
        }
        Type::Accessory(AccessoryKind::NonFalsyString | AccessoryKind::NonEmptyArray) => Some(true),
        Type::Union(members) => {
            let mut iter = members.iter().map(type_truthiness);
            let first = iter.next()??;
            iter.all(|v| v == Some(first)).then_some(first)
        }
        Type::Intersection(members) => members.iter().find_map(type_truthiness),
        _ => None,
    }
}

/// The provable outcome of a loose `==` comparison between two literal
/// scalar types (`!=` is just its negation), or `None` when not
/// statically decidable - matches real PHPStan's own
/// `LooseComparisonHelper::compareConstantScalars`, read directly from
/// `vendor/phpstan-src` rather than assumed. Deliberately narrower than
/// that helper in one disclosed respect: PHP 8.0 changed exactly one
/// corner of the loose-comparison table - a non-numeric string compared
/// against a number now casts the *number* to a string instead of the
/// string to a number (`PhpVersion::castsNumbersToStringsOnLooseComparison`),
/// so that one branch below is only attempted once `php_version` is
/// configured at 8.0+, matching this project's existing "a version-gated
/// fact stays silent with no configured version" precedent
/// (`php_version.rs`) rather than guessing a runtime default. Every
/// other branch here is exactly as version-independent in real PHP as it
/// is below: same-kind literals, numeric-string-vs-number (always
/// numeric, regardless of version), and bool/null-vs-anything (always a
/// plain bool-cast-both-sides comparison).
pub(crate) fn loose_compare_literals(
    l: &Type,
    r: &Type,
    php_version: Option<crate::php_version::PhpVersion>,
) -> Option<bool> {
    // bool/null vs anything: PHP casts *both* sides to bool and compares
    // - version-independent, and subsumes the same-kind True/False/Null
    // combinations too (no need to special-case them separately below).
    if matches!(l, Type::True | Type::False | Type::Null)
        || matches!(r, Type::True | Type::False | Type::Null)
    {
        return Some(type_truthiness(l)? == type_truthiness(r)?);
    }
    match (l, r) {
        (Type::IntLiteral(a), Type::IntLiteral(b)) => Some(a == b),
        (Type::FloatLiteral(a), Type::FloatLiteral(b)) => Some(a == b),
        (Type::IntLiteral(a), Type::FloatLiteral(b))
        | (Type::FloatLiteral(b), Type::IntLiteral(a)) => Some(*a as f64 == *b),
        (Type::StringLiteral(a), Type::StringLiteral(b)) => {
            if types::is_numeric_string_literal(a) && types::is_numeric_string_literal(b) {
                Some(a.trim().parse::<f64>().ok()? == b.trim().parse::<f64>().ok()?)
            } else {
                Some(a == b)
            }
        }
        (Type::IntLiteral(n), Type::StringLiteral(s))
        | (Type::StringLiteral(s), Type::IntLiteral(n)) => {
            if types::is_numeric_string_literal(s) {
                Some(*n as f64 == s.trim().parse::<f64>().ok()?)
            } else if php_version.is_some_and(|v| v >= (8, 0)) {
                Some(n.to_string() == *s)
            } else {
                None
            }
        }
        (Type::FloatLiteral(n), Type::StringLiteral(s))
        | (Type::StringLiteral(s), Type::FloatLiteral(n)) => {
            if types::is_numeric_string_literal(s) {
                Some(*n == s.trim().parse::<f64>().ok()?)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Real PHPStan's own `InitializerExprTypeResolver::getModType`,
/// transcribed - unlike `Mul`/`Sub`/`Pow`, whose own corner/point
/// formulas already reduce to the exact answer for two literal operands
/// as a natural degenerate case, `%`'s own real magnitude-based range
/// fallback (below) reasons only about the *divisor*'s own bound (real
/// PHPStan's own `IntegerRangeType`-branch: `[0, |r|-1]` when the
/// dividend is provably non-negative, `[-(|r|-1), |r|-1]` otherwise) -
/// `7 % 3` would fall to the far coarser `int<0, 2>` there instead of
/// the exact literal `1` without a dedicated literal-pair fold first,
/// exactly matching real PHPStan's own two-step structure (its own
/// `getModType` does the identical cross-product fold before ever
/// reaching this same fallback).
pub(crate) fn mod_int_range(
    l_min: Option<i64>,
    l_max: Option<i64>,
    r_min: Option<i64>,
    r_max: Option<i64>,
) -> Type {
    if l_min == l_max
        && r_min == r_max
        && let (Some(l), Some(r)) = (l_min, r_min)
        && r != 0
    {
        return Type::IntLiteral(l % r);
    }
    // Real PHPStan's own explicit special case: modulo by exactly 1 is
    // always 0, regardless of the dividend.
    if r_min == Some(1) && r_max == Some(1) {
        return Type::IntLiteral(0);
    }
    let range_max = r_max.map(|m| m - 1);
    let left_is_non_negative = l_min.is_some_and(|v| v >= 0);
    let range_min = if left_is_non_negative {
        Some(0)
    } else {
        range_max.map(|m| -m)
    };
    normalize_int_range(range_min, range_max)
}

/// An unbounded end (or a `0` swapped to the nearest representable
/// non-zero int, matching `div_int_range`'s own zero-avoidance) mapped
/// to signed infinity - shares `mul_int_range`'s own `bound_or_inf`.
fn div_sign(v: f64) -> f64 {
    if v < 0.0 {
        -1.0
    } else if v > 0.0 {
        1.0
    } else {
        0.0
    }
}

/// Real PHPStan's own `integerRangeMath`'s `Div` arm, transcribed
/// directly (`InitializerExprTypeResolver::integerRangeMath`,
/// `Expr\BinaryOp\Div` branch, plus the shared tail every arm in that
/// function runs through afterward). Real division between two ints
/// isn't guaranteed to itself produce an int (`7 / 2` is `3.5`), so the
/// result is always a union of a refined int-range possibility and a
/// bare `Float` possibility, never a pure int range alone - matching
/// this project's own pre-existing `Div` fallback shape (`Union([Int,
/// Float])`) exactly, just with the int half narrowed to a real range
/// instead of bare `Int` whenever possible.
///
/// The recursive sign-splitting (when the divisor, or the dividend,
/// spans both a negative and a positive sub-range, its own extremes
/// aren't simply "the four corners" `Mul`'s own formula relies on - each
/// half needs computing independently and unioning) is transcribed as
/// real recursion, mirroring the source's own two `$this->
/// integerRangeMath(...)` self-calls per split exactly, each one
/// already returning a complete, tail-processed `Type` that's unioned
/// directly rather than combined as raw bounds.
pub(crate) fn div_int_range(
    l_min: Option<i64>,
    l_max: Option<i64>,
    r_min: Option<i64>,
    r_max: Option<i64>,
) -> Type {
    // "Avoid division by zero when looking for the min and the max by
    // using the closest int" - the real source's own comment, verbatim.
    // Only these two local values are adjusted; the "invert on negative
    // constant" check below deliberately still consults the real,
    // un-adjusted `r_min`/`r_max`.
    let operand_min = if r_min == Some(0) { Some(1) } else { r_min };
    let operand_max = if r_max == Some(0) { Some(-1) } else { r_max };

    if operand_min.is_none_or(|v| v < 0) && operand_max.is_none_or(|v| v > 0) {
        let negative = div_int_range(l_min, l_max, operand_min, Some(0));
        let positive = div_int_range(l_min, l_max, Some(0), operand_max);
        return union_types(negative, positive);
    }
    if l_min.is_none_or(|v| v < 0) && l_max.is_none_or(|v| v > 0) {
        let negative = div_int_range(l_min, Some(0), r_min, r_max);
        let positive = div_int_range(Some(0), l_max, r_min, r_max);
        return union_types(negative, positive);
    }

    let range_min_sign = div_sign(bound_or_inf(l_min, f64::NEG_INFINITY));
    let range_max_sign = div_sign(bound_or_inf(l_max, f64::INFINITY));

    let min1 = operand_min
        .map(|o| bound_or_inf(l_min, f64::NEG_INFINITY) / o as f64)
        .unwrap_or(range_min_sign * -0.1);
    let min2 = operand_max
        .map(|o| bound_or_inf(l_min, f64::NEG_INFINITY) / o as f64)
        .unwrap_or(range_min_sign * 0.1);
    let max1 = operand_min
        .map(|o| bound_or_inf(l_max, f64::INFINITY) / o as f64)
        .unwrap_or(range_max_sign * -0.1);
    let max2 = operand_max
        .map(|o| bound_or_inf(l_max, f64::INFINITY) / o as f64)
        .unwrap_or(range_max_sign * 0.1);

    let fmin = min1.min(min2).min(max1).min(max2);
    let fmax = min1.max(min2).max(max1).max(max2);
    let mut min = if fmin == f64::NEG_INFINITY {
        None
    } else {
        Some(fmin)
    };
    let mut max = if fmax == f64::INFINITY {
        None
    } else {
        Some(fmax)
    };

    if let (Some(mn), Some(mx)) = (min, max)
        && mn > mx
    {
        std::mem::swap(&mut min, &mut max);
    }
    let mut min_i = min.map(|v| v.ceil() as i64);
    let mut max_i = max.map(|v| v.floor() as i64);

    // "invert maximas on division with negative constants" - real
    // PHPStan's own tail-adjustment, using the *original*,
    // pre-zero-avoidance `l_min`/`l_max`/`r_min`/`r_max` (a literal
    // degenerates to `min == max`, matching its own `ConstantIntegerType`
    // check).
    let range_is_negative_const = l_min == l_max && l_min.is_some_and(|v| v < 0);
    let operand_is_negative_const = r_min == r_max && r_min.is_some_and(|v| v < 0);
    if (range_is_negative_const || operand_is_negative_const)
        && (min_i.is_none() || max_i.is_none())
    {
        std::mem::swap(&mut min_i, &mut max_i);
    }

    if min_i.is_none() && max_i.is_none() {
        return Type::Union(vec![Type::Int, Type::Float]);
    }
    match normalize_int_range(min_i, max_i) {
        // The rounding step (`ceil`ing the min, `floor`ing the max
        // independently) can invert an otherwise-valid range into one
        // with no real int members at all - `normalize_int_range`'s own
        // doc comment has an example (`10 / 4`, always
        // `2.5`, never any int). `union_types` has no general `Never`-
        // absorption rule of its own (nothing elsewhere in this project
        // ever produces a `Never` member to union away), so this handles
        // it explicitly rather than teach that absorption to every other
        // `union_types` caller for the sake of this one.
        Type::Never => Type::Float,
        int_half => union_types(int_half, Type::Float),
    }
}

/// Real PHPStan's own `isset()`/`empty()`/`??`/`??=` "does this
/// null-probe make sense" verdict, condensed to the one shape this
/// project scopes to (a bare, guaranteed-defined variable - see
/// `Checker::check_pointless_null_probe`'s own doc comment for why a
/// deeper chain is out of scope). `None` for a genuinely ambiguous type
/// (`Mixed`, or one that's nullable among other members) - real PHPStan's
/// `IssetCheck` draws the identical two-way split (`isNull()->yes()` vs.
/// `isNull()->no()`, `maybe()` staying silent either way).
pub(crate) fn null_probe_verdict(ty: &Type) -> Option<&'static str> {
    if *ty == Type::Null {
        return Some("is always null");
    }
    if type_definitely_excludes_null(ty) {
        Some("is not nullable")
    } else {
        None
    }
}

/// `empty()`'s own falsiness-aware verdict - real
/// PHPStan's `EmptyRule`: unlike
/// `null_probe_verdict` just above, `empty()` tests falsiness, not just
/// null-ness, so both `isNull()` and `toBoolean()->isFalse()` have to
/// be unambiguous before it says anything - by the time both are
/// resolved, the message reduces to exactly one of these two outcomes
/// (`isNull`-specific wording is provably unreachable once falsiness is
/// already known). `type_truthiness` already recurses through `Union`/
/// `Intersection`/`ArrayShape`/accessory kinds, so this is a thin
/// wrapper, not a separate reimplementation.
pub(crate) fn empty_probe_verdict(ty: &Type) -> Option<&'static str> {
    match type_truthiness(ty) {
        Some(true) => Some("is not falsy"),
        Some(false) => Some("is always falsy"),
        None => None,
    }
}
