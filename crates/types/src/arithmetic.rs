//! Integer and comparison arithmetic on types: PHPStan's
//! `IntegerRangeType` operations and `InitializerExprTypeResolver`'s
//! arithmetic, and the outcome of comparing ranges and literals.

use crate::{Repr, Type};
use syntax::ast::BinOp;

/// Applies `op` to two bounds, distinguishing a genuine `i64` overflow
/// (both operands were real values, but the exact result doesn't fit)
/// from a merely-unbounded operand (`None` propagating as `None` is
/// correct there, not an overflow) - `bool` is `true` only for the
/// former, the signal `Add`/`Sub`/`Mul` need to union `Repr::Float`
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
    let result = Type::int_interval(min_bound, max_bound);
    // With every input bound present (no `?? -INF`/`?? INF` substitution
    // ever used above), a non-finite corner can only be a genuine `i64`
    // overflow, not a propagated unbounded operand - the same real-PHP
    // int-to-float promotion `checked_bound_op` signals for `Add`/`Sub`.
    let all_bounded = l_min.is_some() && l_max.is_some() && r_min.is_some() && r_max.is_some();
    if all_bounded && (min_bound.is_none() || max_bound.is_none()) {
        Type::union(result, Type::float())
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
    // A bound that overflows is a float in PHP, as for `+`.
    let mut overflow = false;
    let mut sub = |a: i64, b: i64| {
        let result = a.checked_sub(b);
        overflow |= result.is_none();
        result
    };
    let (min, max) = if l_min == l_max && l_min.is_some() && (r_min.is_none() || r_max.is_none()) {
        (None, l_min)
    } else {
        let mut min = match (l_min, r_min) {
            (_, None) => None,
            (Some(lm), Some(_)) => sub(lm, r_max.or(r_min).unwrap()),
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
                    min = sub(lm, rmax);
                    max = None;
                } else if let Some(rmin) = r_min {
                    max = sub(lmax, rmin);
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
    let result = Type::int_interval(min, max);
    if overflow {
        Type::union(result, Type::float())
    } else {
        result
    }
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
/// involving operand to bare `Repr::Float` before ever reaching this, the
/// same gate every one of real PHPStan's own non-numeric-exponent cases
/// (`string`/`bool`/`null`) this project's type system doesn't route
/// through this path at all. A literal base's own `(min, max)` bounds
/// coincide (`int_bounds`'s own doc comment), so this single
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
        Type::int_interval(min, max)
    } else {
        Type(Repr::Union(vec![Type::int(), Type::float()]))
    }
}

/// `l << amount` or `l >> amount` for a single constant `amount`, as PHP
/// computes it: a negative amount throws `ArithmeticError` (`never`); a
/// left shift wraps within 64 bits and one of 64 or more is 0; a right
/// shift of 64 or more leaves only the sign (`0` or `-1`). A range whose
/// bounds shift without losing bits keeps its order; one that wraps is
/// plain `int`.
pub(crate) fn shift_int_range(
    l_min: Option<i64>,
    l_max: Option<i64>,
    amount: i64,
    left: bool,
) -> Type {
    let Ok(amount) = u32::try_from(amount) else {
        return Type::never();
    };
    if left {
        if amount >= 64 {
            return Type::int_literal(0);
        }
        // One value wraps to one value.
        if let (Some(min), Some(max)) = (l_min, l_max)
            && min == max
        {
            return Type::int_literal(min.wrapping_shl(amount));
        }
        let shift = |v: Option<i64>| -> Result<Option<i64>, ()> {
            match v {
                None => Ok(None),
                Some(v) => {
                    let shifted = v.wrapping_shl(amount);
                    if shifted >> amount == v {
                        Ok(Some(shifted))
                    } else {
                        Err(())
                    }
                }
            }
        };
        match (shift(l_min), shift(l_max)) {
            (Ok(min), Ok(max)) => Type::int_interval(min, max),
            _ => Type::int(),
        }
    } else {
        let amount = amount.min(63);
        Type::int_interval(l_min.map(|v| v >> amount), l_max.map(|v| v >> amount))
    }
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
pub fn compare_range_outcome(
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

impl Type {
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
    pub fn loosely_equals(
        &self,
        r: &Type,
        php_version: Option<crate::php_version::PhpVersion>,
    ) -> Option<bool> {
        // bool/null vs anything: PHP casts *both* sides to bool and compares
        // - version-independent, and subsumes the same-kind True/False/Null
        // combinations too (no need to special-case them separately below).
        if matches!(
            self,
            Type(Repr::True) | Type(Repr::False) | Type(Repr::Null)
        ) || matches!(r, Type(Repr::True) | Type(Repr::False) | Type(Repr::Null))
        {
            return Some(
                self.to_boolean().bool_literal_value()? == r.to_boolean().bool_literal_value()?,
            );
        }
        match (self, r) {
            (Type(Repr::IntLiteral(a)), Type(Repr::IntLiteral(b))) => Some(a == b),
            (Type(Repr::FloatLiteral(a)), Type(Repr::FloatLiteral(b))) => Some(a == b),
            (Type(Repr::IntLiteral(a)), Type(Repr::FloatLiteral(b)))
            | (Type(Repr::FloatLiteral(b)), Type(Repr::IntLiteral(a))) => Some(*a as f64 == *b),
            (Type(Repr::StringLiteral(a)), Type(Repr::StringLiteral(b))) => {
                match (numeric_string_value(a), numeric_string_value(b)) {
                    (Some(x), Some(y)) => Some(x == y),
                    _ => Some(a == b),
                }
            }
            (Type(Repr::IntLiteral(n)), Type(Repr::StringLiteral(s)))
            | (Type(Repr::StringLiteral(s)), Type(Repr::IntLiteral(n))) => {
                if let Some(x) = numeric_string_value(s) {
                    Some(*n as f64 == x)
                } else if php_version.is_some_and(|v| v >= crate::PhpVersion::new(8, 0)) {
                    Some(*s == n.to_string())
                } else {
                    None
                }
            }
            (Type(Repr::FloatLiteral(n)), Type(Repr::StringLiteral(s)))
            | (Type(Repr::StringLiteral(s)), Type(Repr::FloatLiteral(n))) => {
                numeric_string_value(s).map(|x| *n == x)
            }
            _ => None,
        }
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
        // `PHP_INT_MIN % -1` is 0.
        return Type::int_literal(l.checked_rem(r).unwrap_or(0));
    }
    // Real PHPStan's own explicit special case: modulo by exactly 1 is
    // always 0, regardless of the dividend.
    if r_min == Some(1) && r_max == Some(1) {
        return Type::int_literal(0);
    }
    let range_max = r_max.and_then(|m| m.checked_sub(1));
    let left_is_non_negative = l_min.is_some_and(|v| v >= 0);
    let range_min = if left_is_non_negative {
        Some(0)
    } else {
        range_max.and_then(i64::checked_neg)
    };
    Type::int_interval(range_min, range_max)
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
    syntax::stack::guard(|| {
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
            return Type::union(negative, positive);
        }
        if l_min.is_none_or(|v| v < 0) && l_max.is_none_or(|v| v > 0) {
            let negative = div_int_range(l_min, Some(0), r_min, r_max);
            let positive = div_int_range(Some(0), l_max, r_min, r_max);
            return Type::union(negative, positive);
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
            return Type(Repr::Union(vec![Type::int(), Type::float()]));
        }
        match Type::int_interval(min_i, max_i) {
            // The rounding step (`ceil`ing the min, `floor`ing the max
            // independently) can invert an otherwise-valid range into one
            // with no real int members at all - `normalize_int_range`'s own
            // doc comment has an example (`10 / 4`, always
            // `2.5`, never any int). `Type::union` has no general `Never`-
            // absorption rule of its own (nothing elsewhere in this project
            // ever produces a `Never` member to union away), so this handles
            // it explicitly rather than teach that absorption to every other
            // `Type::union` caller for the sake of this one.
            Type(Repr::Never) => Type::float(),
            int_half => Type::union(int_half, Type::float()),
        }
    })
}

/// A numeric string literal's value, `None` for any other string.
fn numeric_string_value(s: &crate::PhpString) -> Option<f64> {
    let text = s.as_str()?;
    if !crate::is_numeric_string_literal(text) {
        return None;
    }
    text.trim().parse::<f64>().ok()
}

impl Type {
    /// Unary `-` negates a literal/range operand's actual value - `+` is a
    /// true no-op on the value (only ever coercing a numeric string), but `-`
    /// must flip it: `-32768` infers as `IntLiteral(-32768)`, not
    /// `IntLiteral(32768)`. `IntRange`'s min/max swap on negation (`-int<a,
    /// b>` is `int<-b, -a>`) mirrors real PHPStan's `IntegerRangeType`
    /// negation handling. Anything else (a bare `Repr::Int`, `Repr::Float`,
    /// ...) has no specific value to flip, so it's returned unchanged.
    pub fn negate(self) -> Type {
        match self {
            Type(Repr::IntLiteral(n)) => match n.checked_neg() {
                Some(v) => Type::int_literal(v),
                // `-PHP_INT_MIN` is past `PHP_INT_MAX`: a float.
                None => Type::float_literal(-(n as f64)),
            },
            Type(Repr::FloatLiteral(f)) => Type::float_literal(-f),
            Type(Repr::IntRange(min, max)) => {
                let negated = Type::int_interval(
                    max.map(|m| m.checked_neg().unwrap_or(i64::MAX)),
                    min.map(|m| m.checked_neg().unwrap_or(i64::MAX)),
                );
                if min == Some(i64::MIN) {
                    Type::union(negated, Type::float_literal(-(i64::MIN as f64)))
                } else {
                    negated
                }
            }
            other => other,
        }
    }
}

impl Type {
    /// The result of `l op r` for an arithmetic, bitwise or shift operator
    /// or `<=>` over operands of these types, operator overloading aside
    /// (PHPStan's `InitializerExprTypeResolver`).
    pub fn binary_operation(op: &BinOp, lt: &Type, rt: &Type) -> Type {
        let either_float = lt.is_float().yes() || rt.is_float().yes();
        match op {
            BinOp::Spaceship => Type::int_range(Some(-1), Some(1)),
            BinOp::Add => {
                if lt.is_array().yes() || rt.is_array().yes() {
                    return array_union_type(lt, rt);
                }
                if either_float {
                    return Type::float();
                }
                if let (Some((l_min, l_max)), Some((r_min, r_max))) =
                    (lt.int_bounds(), rt.int_bounds())
                {
                    let (min, min_overflow) = checked_bound_op(l_min, r_min, i64::checked_add);
                    let (max, max_overflow) = checked_bound_op(l_max, r_max, i64::checked_add);
                    let result = Type::int_interval(min, max);
                    return if min_overflow || max_overflow {
                        Type::union(result, Type::float())
                    } else {
                        result
                    };
                }
                Type::int()
            }
            BinOp::Sub => {
                if either_float {
                    return Type::float();
                }
                if let (Some((l_min, l_max)), Some((Some(c), Some(rc)))) =
                    (lt.int_bounds(), rt.int_bounds())
                    && c == rc
                {
                    let (min, min_overflow) = checked_bound_op(l_min, Some(c), i64::checked_sub);
                    let (max, max_overflow) = checked_bound_op(l_max, Some(c), i64::checked_sub);
                    let result = Type::int_interval(min, max);
                    return if min_overflow || max_overflow {
                        Type::union(result, Type::float())
                    } else {
                        result
                    };
                }
                match (lt.int_bounds(), rt.int_bounds()) {
                    (Some((l_min, l_max)), Some((r_min, r_max))) => {
                        sub_int_range_full(l_min, l_max, r_min, r_max)
                    }
                    _ => Type::int(),
                }
            }
            BinOp::Mul => {
                if either_float {
                    return Type::float();
                }
                match (lt.int_bounds(), rt.int_bounds()) {
                    (Some((l_min, l_max)), Some((r_min, r_max))) => {
                        mul_int_range(l_min, l_max, r_min, r_max)
                    }
                    _ => Type::int(),
                }
            }
            BinOp::Pow => {
                if either_float {
                    return Type::float();
                }
                match (lt.int_bounds(), rt.int_bounds()) {
                    (Some((l_min, l_max)), Some((r_min, r_max))) => {
                        pow_int_range(l_min, l_max, r_min, r_max)
                    }
                    _ => Type::int(),
                }
            }
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                fold_bitwise_int_literals(op, lt, rt).unwrap_or(Type::int())
            }
            BinOp::Shl | BinOp::Shr => match (lt.int_bounds(), rt.int_bounds()) {
                (Some((l_min, l_max)), Some((Some(amount), Some(r_max)))) if amount == r_max => {
                    shift_int_range(l_min, l_max, amount, matches!(op, BinOp::Shl))
                }
                _ => Type::int(),
            },
            BinOp::Mod => match (lt.int_bounds(), rt.int_bounds()) {
                (Some((l_min, l_max)), Some((r_min, r_max))) => {
                    mod_int_range(l_min, l_max, r_min, r_max)
                }
                _ => Type::int(),
            },
            BinOp::Div => {
                if either_float {
                    return Type::float();
                }
                match (lt.int_bounds(), rt.int_bounds()) {
                    (Some((l_min, l_max)), Some((r_min, r_max))) => {
                        div_int_range(l_min, l_max, r_min, r_max)
                    }
                    _ => Type(Repr::Union(vec![Type::int(), Type::float()])),
                }
            }
            _ => Type::mixed(),
        }
    }
}

/// `$a + $b` over arrays: the keys and values of both, a list when both
/// are, non-empty when either is.
fn array_union_type(lt: &Type, rt: &Type) -> Type {
    let both_arrays = lt.is_array().yes() && rt.is_array().yes();
    let key_of = |t: &Type| both_arrays.then(|| t.array_key_type()).flatten();
    let key_ty = match (key_of(lt), key_of(rt)) {
        _ if !both_arrays => Type::benevolent_array_key(),
        (Some(lk), Some(rk)) if lk == rk => lk,
        (Some(lk), Some(rk)) => Type::union(lk, rk),
        (Some(k), None) | (None, Some(k)) => k,
        (None, None) => Type::benevolent_array_key(),
    };
    let val_ty = Type::union(
        lt.array_value_type().unwrap_or(Type::mixed()),
        rt.array_value_type().unwrap_or(Type::mixed()),
    );
    let mut members = vec![Type::array(key_ty, val_ty)];
    if lt.is_list().yes() && rt.is_list().yes() {
        members.push(Type::accessory(crate::AccessoryKind::ArrayList));
    }
    if lt.is_non_empty_array().yes() || rt.is_non_empty_array().yes() {
        members.push(Type::accessory(crate::AccessoryKind::NonEmptyArray));
    }
    crate::intersect_types(members)
}

/// A bitwise `&`/`|`/`^` over int literals, folded to its exact values
/// (`PREG_SPLIT_NO_EMPTY | PREG_SPLIT_DELIM_CAPTURE` is `3`), at most 64
/// combinations. `$x & k` with a non-negative literal `k` is `int<0, k>`
/// whatever `$x` is.
fn fold_bitwise_int_literals(op: &BinOp, l: &Type, r: &Type) -> Option<Type> {
    fn literals(ty: &Type) -> Option<Vec<i64>> {
        match ty {
            Type(Repr::IntLiteral(n)) => Some(vec![*n]),
            Type(Repr::True) => Some(vec![1]),
            Type(Repr::False) | Type(Repr::Null) => Some(vec![0]),
            Type(Repr::Union(members)) => members.iter().try_fold(Vec::new(), |mut acc, m| {
                acc.extend(literals(m)?);
                Some(acc)
            }),
            _ => None,
        }
    }
    let apply = |a: i64, b: i64| match op {
        BinOp::BitAnd => a & b,
        BinOp::BitOr => a | b,
        _ => a ^ b,
    };
    match (literals(l), literals(r)) {
        (Some(ls), Some(rs)) if ls.len() * rs.len() <= 64 => {
            let mut values: Vec<i64> = ls
                .iter()
                .flat_map(|a| rs.iter().map(move |b| apply(*a, *b)))
                .collect();
            values.sort_unstable();
            values.dedup();
            let mut members = values.into_iter().map(Type::int_literal);
            let first = members.next()?;
            Some(members.fold(first, Type::union))
        }
        (_, Some(ks)) | (Some(ks), _)
            if matches!(op, BinOp::BitAnd) && ks.len() == 1 && ks[0] >= 0 =>
        {
            Some(Type::int_range(Some(0), Some(ks[0])))
        }
        _ => None,
    }
}
