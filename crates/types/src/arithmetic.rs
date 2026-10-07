//! Integer and comparison arithmetic on types: PHPStan's
//! `IntegerRangeType` operations and `InitializerExprTypeResolver`'s
//! arithmetic, and the outcome of comparing ranges and literals.

use crate::UnionAll as _;
use crate::{AccessoryKind, PhpString, Repr, TrinaryLogic, Type};

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
    let result = Type::int_range(min_bound, max_bound);
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
    let result = Type::int_range(min, max);
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
        Type::int_range(min, max)
    } else {
        Type::new(Repr::Union(vec![Type::int(), Type::float()]))
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
            (Ok(min), Ok(max)) => Type::int_range(min, max),
            _ => Type::int(),
        }
    } else {
        let amount = amount.min(63);
        Type::int_range(l_min.map(|v| v >> amount), l_max.map(|v| v >> amount))
    }
}

impl Type {
    /// The provable outcome of a loose `==` comparison (`!=` is its
    /// negation), or `None` when it isn't decided: the comparison model's
    /// answer (`Relation::loose_equal`) without class knowledge.
    /// `php_version` `None` means the answer must hold before and after
    /// PHP 8.0.
    pub fn loosely_equals(
        &self,
        r: &Type,
        php_version: Option<crate::php_version::PhpVersion>,
    ) -> Option<bool> {
        match crate::Relation::structural().loose_equal(self, r, php_version) {
            crate::TrinaryLogic::Yes => Some(true),
            crate::TrinaryLogic::No => Some(false),
            crate::TrinaryLogic::Maybe => None,
        }
    }
}

/// `l % r` over int ranges. PHP's remainder takes the dividend's sign,
/// is smaller in magnitude than the divisor, and is no larger in
/// magnitude than the dividend: `int<0, max> % -3` is `int<0, 2>`, and
/// `int<0, 2> % 5` stays `int<0, 2>`. A divisor that can only be `0`
/// throws, so there is no result.
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
    // The largest divisor magnitude, less one; unbounded when the
    // divisor is.
    let divisor_magnitude = match (r_min, r_max) {
        (Some(lo), Some(hi)) => Some(i128::from(lo).abs().max(i128::from(hi).abs())),
        _ => None,
    };
    if divisor_magnitude == Some(0) {
        return Type::never();
    }
    let bound = divisor_magnitude.map(|m| m - 1);
    let clamp = |dividend_end: i128| match bound {
        Some(b) => dividend_end.clamp(-b, b),
        None => dividend_end,
    };
    let min = match l_min {
        Some(v) if v >= 0 => Some(0),
        Some(v) => Some(clamp(i128::from(v))),
        None => bound.map(|b| -b),
    };
    let max = match l_max {
        Some(v) if v <= 0 => Some(0),
        Some(v) => Some(clamp(i128::from(v))),
        None => bound,
    };
    let narrow =
        |v: Option<i128>| v.map(|v| i64::try_from(v).expect("within an i64 operand's range"));
    Type::int_range(narrow(min), narrow(max))
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
/// `ExprNode\BinaryOp\Div` branch, plus the shared tail every arm in that
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
            return Type::new(Repr::Union(vec![Type::int(), Type::float()]));
        }
        let int_half = Type::int_range(min_i, max_i);
        match int_half.repr() {
            // The rounding step (`ceil`ing the min, `floor`ing the max
            // independently) can invert an otherwise-valid range into one
            // with no real int members at all - `normalize_int_range`'s own
            // doc comment has an example (`10 / 4`, always
            // `2.5`, never any int). `Type::union` has no general `Never`-
            // absorption rule of its own (nothing elsewhere in this project
            // ever produces a `Never` member to union away), so this handles
            // it explicitly rather than teach that absorption to every other
            // `Type::union` caller for the sake of this one.
            Repr::Never => Type::float(),
            _ => Type::union(int_half, Type::float()),
        }
    })
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
        match self.repr() {
            Repr::IntLiteral(n) => match n.checked_neg() {
                Some(v) => Type::int_literal(v),
                // `-PHP_INT_MIN` is past `PHP_INT_MAX`: a float.
                None => Type::float_literal(-(*n as f64)),
            },
            Repr::FloatLiteral(f) => Type::float_literal(-f),
            Repr::IntRange(min, max) => {
                let negated = Type::int_range(
                    max.map(|m| m.checked_neg().unwrap_or(i64::MAX)),
                    min.map(|m| m.checked_neg().unwrap_or(i64::MAX)),
                );
                if *min == Some(i64::MIN) {
                    Type::union(negated, Type::float_literal(-(i64::MIN as f64)))
                } else {
                    negated
                }
            }
            _ => self,
        }
    }
}

/// An operator `Type::binary_operation` computes: an arithmetic, bitwise
/// or shift operator, or `<=>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithmeticOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Spaceship,
}

/// An ordering comparison: `<`, `<=`, `>` or `>=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderingOp {
    Lt,
    LtEq,
    Gt,
    GtEq,
}

impl Type {
    /// The result of `l op r` for an arithmetic, bitwise or shift operator
    /// or `<=>` over operands of these types, operator overloading aside
    /// (PHPStan's `InitializerExprTypeResolver`).
    pub fn binary_operation(op: ArithmeticOp, lt: &Type, rt: &Type) -> Type {
        if let Some(folded) = fold_float_constants(op, lt, rt) {
            return folded;
        }
        let either_float = lt.is_float().yes() || rt.is_float().yes();
        match op {
            ArithmeticOp::Spaceship => Type::int_range(Some(-1), Some(1)),
            ArithmeticOp::Add => {
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
                    let result = Type::int_range(min, max);
                    return if min_overflow || max_overflow {
                        Type::union(result, Type::float())
                    } else {
                        result
                    };
                }
                Type::int()
            }
            ArithmeticOp::Sub => {
                if either_float {
                    return Type::float();
                }
                if let (Some((l_min, l_max)), Some((Some(c), Some(rc)))) =
                    (lt.int_bounds(), rt.int_bounds())
                    && c == rc
                {
                    let (min, min_overflow) = checked_bound_op(l_min, Some(c), i64::checked_sub);
                    let (max, max_overflow) = checked_bound_op(l_max, Some(c), i64::checked_sub);
                    let result = Type::int_range(min, max);
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
            ArithmeticOp::Mul => {
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
            ArithmeticOp::Pow => {
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
            ArithmeticOp::BitAnd | ArithmeticOp::BitOr | ArithmeticOp::BitXor => {
                let as_ints = || fold_bitwise_int_literals(op, lt, rt).unwrap_or(Type::int());
                match lt.is_string().and(rt.is_string()) {
                    TrinaryLogic::Yes => string_bitwise(op, lt, rt),
                    TrinaryLogic::No => as_ints(),
                    TrinaryLogic::Maybe => Type::union(Type::int(), string_bitwise(op, lt, rt)),
                }
            }
            ArithmeticOp::Shl | ArithmeticOp::Shr => match (lt.int_bounds(), rt.int_bounds()) {
                (Some((l_min, l_max)), Some((Some(amount), Some(r_max)))) if amount == r_max => {
                    shift_int_range(l_min, l_max, amount, matches!(op, ArithmeticOp::Shl))
                }
                _ => Type::int(),
            },
            ArithmeticOp::Mod => match (lt.int_bounds(), rt.int_bounds()) {
                (Some((l_min, l_max)), Some((r_min, r_max))) => {
                    mod_int_range(l_min, l_max, r_min, r_max)
                }
                _ => Type::int(),
            },
            ArithmeticOp::Div => {
                if either_float {
                    return Type::float();
                }
                match (lt.int_bounds(), rt.int_bounds()) {
                    (Some((l_min, l_max)), Some((r_min, r_max))) => {
                        div_int_range(l_min, l_max, r_min, r_max)
                    }
                    _ => Type::new(Repr::Union(vec![Type::int(), Type::float()])),
                }
            }
        }
    }
}

/// An int or float value.
#[derive(Clone, Copy)]
enum Number {
    Int(i64),
    Float(f64),
}

impl Number {
    fn as_f64(self) -> f64 {
        match self {
            Number::Int(n) => n as f64,
            Number::Float(f) => f,
        }
    }

    /// `(int)` of it, as `%` reads its operands.
    fn as_i64(self) -> i64 {
        match self {
            Number::Int(n) => n,
            Number::Float(f) => crate::coercion::php_float_to_int(f),
        }
    }

    fn into_type(self) -> Type {
        match self {
            Number::Int(n) => Type::int_literal(n),
            Number::Float(f) => Type::float_literal(f),
        }
    }
}

/// `l op r` computed value by value when both sides are a few int or
/// float literals and a float takes part, or the operator is `/`. A pair
/// that throws (a division or modulo by zero) contributes nothing.
fn fold_float_constants(op: ArithmeticOp, lt: &Type, rt: &Type) -> Option<Type> {
    const LIMIT: usize = 64;
    fn numbers(ty: &Type) -> Option<Vec<Number>> {
        ty.union_members()
            .iter()
            .map(|t| match t.repr() {
                Repr::IntLiteral(n) => Some(Number::Int(*n)),
                Repr::FloatLiteral(f) => Some(Number::Float(*f)),
                _ => None,
            })
            .collect()
    }
    if !matches!(
        op,
        ArithmeticOp::Add
            | ArithmeticOp::Sub
            | ArithmeticOp::Mul
            | ArithmeticOp::Div
            | ArithmeticOp::Mod
            | ArithmeticOp::Pow
            | ArithmeticOp::Spaceship
    ) {
        return None;
    }
    let (ls, rs) = (numbers(lt)?, numbers(rt)?);
    let has_float = |ns: &[Number]| ns.iter().any(|n| matches!(n, Number::Float(_)));
    // Int-only operands are the range arithmetic's, but a division of
    // literals is exact here where a range can only say `int|float`.
    let takes_part = has_float(&ls) || has_float(&rs) || matches!(op, ArithmeticOp::Div);
    if !takes_part || ls.len() * rs.len() > LIMIT {
        return None;
    }
    let apply = |a: Number, b: Number| -> Option<Number> {
        use Number::{Float, Int};
        let checked = |f: fn(i64, i64) -> Option<i64>, g: fn(f64, f64) -> f64| match (a, b) {
            (Int(x), Int(y)) => Some(f(x, y).map_or_else(|| Float(g(x as f64, y as f64)), Int)),
            _ => Some(Float(g(a.as_f64(), b.as_f64()))),
        };
        match op {
            ArithmeticOp::Add => checked(i64::checked_add, |x, y| x + y),
            ArithmeticOp::Sub => checked(i64::checked_sub, |x, y| x - y),
            ArithmeticOp::Mul => checked(i64::checked_mul, |x, y| x * y),
            ArithmeticOp::Div => match (a, b) {
                (_, Int(0)) => None,
                (_, Float(0.0)) => None,
                (Int(x), Int(y)) if x.checked_rem(y) == Some(0) => {
                    Some(x.checked_div(y).map_or(Float(x as f64 / y as f64), Int))
                }
                _ => Some(Float(a.as_f64() / b.as_f64())),
            },
            ArithmeticOp::Mod => {
                let divisor = b.as_i64();
                (divisor != 0).then(|| Int(a.as_i64().checked_rem(divisor).unwrap_or(0)))
            }
            ArithmeticOp::Pow => match (a, b) {
                (Int(x), Int(y)) if y >= 0 => Some(
                    u32::try_from(y)
                        .ok()
                        .and_then(|y| x.checked_pow(y))
                        .map_or(Float((x as f64).powf(y as f64)), Int),
                ),
                _ => Some(Float(a.as_f64().powf(b.as_f64()))),
            },
            _ => Some(Int(match a.as_f64().partial_cmp(&b.as_f64()) {
                Some(std::cmp::Ordering::Less) => -1,
                Some(std::cmp::Ordering::Equal) => 0,
                _ => 1,
            })),
        }
    };
    Some(
        ls.iter()
            .flat_map(|&a| rs.iter().filter_map(move |&b| apply(a, b)))
            .map(Number::into_type)
            .union_all()
            .unwrap_or_else(Type::never),
    )
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

/// `l & r`, `l | r` or `l ^ r` over two strings: byte by byte, as long as
/// the shorter operand for `&` and `^`, and as the longer for `|` (whose
/// extra bytes are copied). Folded over up to 64 pairs of literals.
fn string_bitwise(op: ArithmeticOp, l: &Type, r: &Type) -> Type {
    fn literals(ty: &Type) -> Option<Vec<PhpString>> {
        let finite = ty.finite_types();
        if finite.is_empty() {
            return None;
        }
        finite
            .into_iter()
            .map(|t| match t.repr() {
                Repr::StringLiteral(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }
    let apply = |a: &[u8], b: &[u8]| -> Vec<u8> {
        match op {
            ArithmeticOp::BitOr => {
                let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
                let mut out = long.to_vec();
                for (o, s) in out.iter_mut().zip(short) {
                    *o |= s;
                }
                out
            }
            ArithmeticOp::BitAnd => a.iter().zip(b).map(|(x, y)| x & y).collect(),
            _ => a.iter().zip(b).map(|(x, y)| x ^ y).collect(),
        }
    };
    if let (Some(ls), Some(rs)) = (literals(l), literals(r))
        && ls.len() * rs.len() <= 64
    {
        return ls
            .iter()
            .flat_map(|a| {
                rs.iter().map(move |b| {
                    Type::string_literal(PhpString::from(apply(a.as_bytes(), b.as_bytes())))
                })
            })
            .union_all()
            .unwrap_or_else(Type::never);
    }
    let non_empty = |t: &Type| t.is_refined_string(AccessoryKind::NonEmptyString).yes();
    let result_non_empty = match op {
        ArithmeticOp::BitOr => non_empty(l) || non_empty(r),
        _ => non_empty(l) && non_empty(r),
    };
    if result_non_empty {
        Type::string().with_string_accessory(AccessoryKind::NonEmptyString)
    } else {
        Type::string()
    }
}

/// A bitwise `&`/`|`/`^` over int literals, folded to its exact values
/// (`PREG_SPLIT_NO_EMPTY | PREG_SPLIT_DELIM_CAPTURE` is `3`), at most 64
/// combinations. `$x & k` with a non-negative literal `k` is `int<0, k>`
/// whatever `$x` is.
fn fold_bitwise_int_literals(op: ArithmeticOp, l: &Type, r: &Type) -> Option<Type> {
    fn literals(ty: &Type) -> Option<Vec<i64>> {
        match ty.repr() {
            Repr::IntLiteral(n) => Some(vec![*n]),
            Repr::True => Some(vec![1]),
            Repr::False | Repr::Null => Some(vec![0]),
            Repr::Union(members) => members.iter().try_fold(Vec::new(), |mut acc, m| {
                acc.extend(literals(m)?);
                Some(acc)
            }),
            _ => None,
        }
    }
    let apply = |a: i64, b: i64| match op {
        ArithmeticOp::BitAnd => a & b,
        ArithmeticOp::BitOr => a | b,
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
            if matches!(op, ArithmeticOp::BitAnd) && ks.len() == 1 && ks[0] >= 0 =>
        {
            Some(Type::int_range(Some(0), Some(ks[0])))
        }
        _ => None,
    }
}
