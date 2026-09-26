use crate::checker::int_range_bounds;
use crate::checker::narrowing::predicates::narrowing_key;
use syntax::ast::*;
use types::format_php_float;
use types::{self, AccessoryKind, Type};

/// Shared by `narrow_type_excluding_falsy`'s own `Array` arm above and
/// the `count($x) > 0` narrowing below, so the two "prove this array is
/// non-empty" paths can't drift apart. Handles both a bare `Type::Array`
/// and an already-`Intersection([Array(_), Accessory(ArrayList)])` value
/// (a `list<T>`) - upgrading the latter to `non-empty-list<T>` rather
/// than leaving it unchanged - and avoids adding a duplicate
/// `NonEmptyArray` member if one is already present.
/// Backs `is_numeric($x)`'s own true-branch narrowing above -
/// upgrades a kept `String`, bare or found inside a `Union`, to
/// `numeric-string`. The false branch is intentionally left alone (a
/// string that fails `is_numeric()` is still validly just `String`).
pub(crate) fn upgrade_string_to_numeric(ty: &Type) -> Type {
    upgrade_string_with_accessory(ty, AccessoryKind::NumericString)
}

/// Adds `kind` to a `String` value - bare, or already refined by some
/// other accessory kind (`non-empty-string`, `literal-string`, ... - any
/// `Intersection` whose base is `String`), recursing into a `Union`'s own
/// members the same way. Shared by `upgrade_string_to_numeric`
/// (`is_numeric($x)`) and the `str_contains`/`str_starts_with`/
/// `str_ends_with` haystack narrowing - both are "prove this string
/// satisfies one more accessory fact" upgrades that would otherwise
/// duplicate the identical bare/intersection/union recursion. The
/// "already-refined string" arm matters because e.g. a preg_match()
/// capture group (`non-empty-string`) surviving an `is_numeric()` guard
/// must not silently fall through to `other => other.clone()`
/// un-upgraded.
pub(crate) fn upgrade_string_with_accessory(ty: &Type, kind: AccessoryKind) -> Type {
    match ty {
        Type::String => Type::Intersection(vec![Type::String, Type::Accessory(kind)]),
        Type::Intersection(members)
            if members.iter().any(|m| matches!(m, Type::String))
                && !members.contains(&Type::Accessory(kind)) =>
        {
            let mut members = members.clone();
            members.push(Type::Accessory(kind));
            Type::Intersection(members)
        }
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|m| upgrade_string_with_accessory(m, kind))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Whether `ty` (a string-shaped type) is provably `kind` -
/// a reusable predicate for any accessory kind
/// (`is_non_falsy_string` is a one-line delegate to this).
pub(crate) fn string_type_has_accessory(ty: &Type, kind: AccessoryKind) -> bool {
    types::Relation::structural().subtype(
        ty,
        &Type::Intersection(vec![Type::String, Type::Accessory(kind)]),
    )
}

/// Strips a `Type::StringLiteral("")` member out of a `Union` (a no-op
/// for every other shape) - mirrors real PHPStan's own `TypeCombinator::
/// remove($leftStringType, new ConstantStringType(''))` step in
/// `resolveConcatType`'s own numeric-string propagation below, needed so
/// e.g. a `bool`'s own `toString()` projection (`Union(['', '1'])`)
/// doesn't fail `NumericString` proof-checking purely because of its own
/// always-possible empty-string member.
fn strip_empty_string_member(ty: &Type) -> Type {
    match ty {
        Type::Union(members) => {
            let filtered: Vec<Type> = members
                .iter()
                .filter(|m| **m != Type::StringLiteral(String::new()))
                .cloned()
                .collect();
            match filtered.len() {
                0 => ty.clone(),
                1 => filtered.into_iter().next().unwrap(),
                _ => Type::Union(filtered),
            }
        }
        other => other.clone(),
    }
}

/// The "as if concatenated" string projection of one concat/
/// interpolation operand's own type - mirrors real PHPStan's own
/// `Type::toString()` dispatch (each real `*Type::toString()`
/// implementation read directly, not reconstructed from memory) for
/// every operand shape this project's own inference can actually
/// produce. `ty` unchanged (a `String`/`StringLiteral`/already-
/// accessoried `Intersection`) passes straight through - it's already
/// its own "as a string" projection.
pub(crate) fn concat_operand_to_string(ty: &Type) -> Type {
    match ty {
        Type::StringLiteral(_) | Type::String => ty.clone(),
        Type::Intersection(members) if members.contains(&Type::String) => ty.clone(),
        Type::IntLiteral(n) => Type::StringLiteral(n.to_string()),
        Type::FloatLiteral(f) => Type::StringLiteral(format_php_float(*f)),
        Type::True => Type::StringLiteral("1".to_string()),
        Type::False | Type::Null => Type::StringLiteral(String::new()),
        Type::Bool => Type::Union(vec![
            Type::StringLiteral(String::new()),
            Type::StringLiteral("1".to_string()),
        ]),
        Type::Int | Type::IntRange(_, _) => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::DecimalIntegerString),
        ]),
        Type::Float => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::UppercaseString),
            Type::Accessory(AccessoryKind::NumericString),
        ]),
        Type::Union(members) => Type::Union(members.iter().map(concat_operand_to_string).collect()),
        // Every other shape (`Mixed`, an object without `__toString()` -
        // already flagged separately by this project's own implicit-
        // `__toString()`/array-to-string checks, not this function's
        // concern) has no more precise "as a string" projection than
        // plain `string`.
        _ => Type::String,
    }
}

/// Every literal string value `ty` could concretely be, when it's a
/// `StringLiteral` or a `Union` of one - `vec![]` for anything else
/// (never guessed at for a bare `String`/accessoried `Intersection`).
fn constant_strings(ty: &Type) -> Vec<String> {
    match ty {
        Type::StringLiteral(s) => vec![s.clone()],
        Type::Union(members) => members.iter().flat_map(constant_strings).collect(),
        _ => Vec::new(),
    }
}

/// Real PHPStan's own `CALCULATE_SCALARS_LIMIT` (`InitializerExprTypeResolver
/// .php`) - bounds the constant-string cross-product below so a
/// concatenation of two wide literal unions can't blow up combinatorially.
const CALCULATE_SCALARS_LIMIT: usize = 8;

/// A full port of real PHPStan's own `InitializerExprTypeResolver
/// ::resolveConcatType` - the shared engine behind both `BinOp::Concat`
/// and `ExprKind::Interpolated` (the latter as a left-fold over its own
/// parts). `l`/`r` are the operands' own *original* inferred types (not
/// yet projected to their "as a string" shape) - this function does that
/// projection itself via `concat_operand_to_string`, then also consults
/// the originals directly wherever real PHPStan's own algorithm does
/// (the numeric-string propagation's own `$left->isInteger()`/
/// `$right`-range checks, which run against the un-stringified operand).
pub(crate) fn resolve_concat_type(l: &Type, r: &Type) -> Type {
    let l_str = concat_operand_to_string(l);
    let r_str = concat_operand_to_string(r);

    // Empty-string identity shortcuts.
    if let Type::StringLiteral(s) = &l_str
        && s.is_empty()
    {
        return r_str;
    }
    if let Type::StringLiteral(s) = &r_str
        && s.is_empty()
    {
        return l_str;
    }
    // Full constant fold when both sides are single literals.
    if let (Type::StringLiteral(a), Type::StringLiteral(b)) = (&l_str, &r_str) {
        return Type::StringLiteral(format!("{a}{b}"));
    }

    // Bounded constant-string cross product - covers a `Union` of
    // literals on either/both sides (e.g. `bool`'s own `toString()`
    // projection) without needing its own separate special case.
    let l_consts = constant_strings(&l_str);
    let r_consts = constant_strings(&r_str);
    let combined_count = l_consts.len() * r_consts.len();
    if combined_count > 0 && combined_count <= CALCULATE_SCALARS_LIMIT {
        let mut combos: Vec<String> = Vec::new();
        for a in &l_consts {
            if a.is_empty() {
                combos.extend(r_consts.iter().cloned());
                continue;
            }
            for b in &r_consts {
                if b.is_empty() {
                    combos.push(a.clone());
                    continue;
                }
                combos.push(format!("{a}{b}"));
            }
        }
        combos.sort();
        combos.dedup();
        if !combos.is_empty() {
            return if combos.len() == 1 {
                Type::StringLiteral(combos.into_iter().next().unwrap())
            } else {
                Type::Union(combos.into_iter().map(Type::StringLiteral).collect())
            };
        }
    }

    let mut accessories: Vec<AccessoryKind> = Vec::new();
    let l_non_empty = string_type_has_accessory(&l_str, AccessoryKind::NonEmptyString);
    let r_non_empty = string_type_has_accessory(&r_str, AccessoryKind::NonEmptyString);
    let l_non_falsy = string_type_has_accessory(&l_str, AccessoryKind::NonFalsyString);
    let r_non_falsy = string_type_has_accessory(&r_str, AccessoryKind::NonFalsyString);
    if (l_non_empty && r_non_empty) || l_non_falsy || r_non_falsy {
        accessories.push(AccessoryKind::NonFalsyString);
    } else if l_non_empty || r_non_empty {
        accessories.push(AccessoryKind::NonEmptyString);
    }

    if string_type_has_accessory(&l_str, AccessoryKind::LiteralString)
        && string_type_has_accessory(&r_str, AccessoryKind::LiteralString)
    {
        accessories.push(AccessoryKind::LiteralString);
    }
    if string_type_has_accessory(&l_str, AccessoryKind::LowercaseString)
        && string_type_has_accessory(&r_str, AccessoryKind::LowercaseString)
    {
        accessories.push(AccessoryKind::LowercaseString);
    }
    if string_type_has_accessory(&l_str, AccessoryKind::UppercaseString)
        && string_type_has_accessory(&r_str, AccessoryKind::UppercaseString)
    {
        accessories.push(AccessoryKind::UppercaseString);
    }

    // Numeric-string propagation - only from the left operand, after
    // excluding an always-possible empty-string member, matching real
    // PHPStan's own `TypeCombinator::remove($leftStringType, new
    // ConstantStringType(''))` guard exactly.
    let l_numeric_capable = string_type_has_accessory(
        &strip_empty_string_member(&l_str),
        AccessoryKind::NumericString,
    );
    if l_numeric_capable {
        let l_is_int = matches!(l, Type::Int | Type::IntLiteral(_) | Type::IntRange(_, _));
        let validate: fn(&str) -> bool = if l_is_int {
            |v: &str| !v.starts_with('-')
        } else {
            |v: &str| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())
        };
        let right_is_non_negative_int_range =
            int_range_bounds(r).is_some_and(|(min, _)| min.is_some_and(|m| m >= 0));
        let all_right_constants_zero_or_more = !r_consts.is_empty()
            && r_consts
                .iter()
                .filter(|v| !v.is_empty())
                .all(|v| types::is_numeric_string_literal(v) && validate(v));
        if all_right_constants_zero_or_more || right_is_non_negative_int_range {
            accessories.push(AccessoryKind::NumericString);
        }
    }

    if accessories.is_empty() {
        Type::String
    } else {
        let mut members = vec![Type::String];
        members.extend(accessories.into_iter().map(Type::Accessory));
        Type::Intersection(members)
    }
}

pub(crate) fn upgrade_array_to_non_empty(ty: &Type) -> Type {
    match ty {
        Type::Array(key, inner) => Type::Intersection(vec![
            Type::Array(key.clone(), inner.clone()),
            Type::Accessory(AccessoryKind::NonEmptyArray),
        ]),
        Type::Intersection(members)
            if members.iter().any(|m| matches!(m, Type::Array(_, _)))
                && !members.contains(&Type::Accessory(AccessoryKind::NonEmptyArray)) =>
        {
            let mut members = members.clone();
            members.push(Type::Accessory(AccessoryKind::NonEmptyArray));
            Type::Intersection(members)
        }
        // A genuinely empty `ArrayShape` is a real contradiction once
        // `count() > 0`/truthy is known - collapses to `Never` (the same
        // "this branch is impossible" signal `narrow_type_excluding_
        // falsy`'s own identical arm already uses) rather than staying
        // unchanged, so a `Union` member that's provably empty gets
        // dropped below instead of surviving as a spurious alternative.
        Type::ArrayShape(shape) if shape.is_empty_array() => Type::Never,
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
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
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
        _ => int_range_bounds(ty),
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
        Type::Array(key, inner) => Type::Intersection(vec![
            Type::Array(key.clone(), inner.clone()),
            Type::Accessory(AccessoryKind::ArrayList),
        ]),
        Type::Intersection(members)
            if members.iter().any(|m| matches!(m, Type::Array(_, _)))
                && !members.contains(&Type::Accessory(AccessoryKind::ArrayList)) =>
        {
            let mut members = members.clone();
            members.push(Type::Accessory(AccessoryKind::ArrayList));
            Type::Intersection(members)
        }
        Type::Union(members) => Type::Union(members.iter().map(upgrade_array_to_list).collect()),
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
            Type::Never
        }
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(narrow_array_to_non_list)
                .filter(|m| *m != Type::Never)
                .collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        _ => upgrade_array_to_non_empty(ty),
    }
}

#[cfg(test)]
mod resolve_concat_type_tests {
    use super::*;

    #[test]
    fn a_float_literal_operand_uses_the_real_php_float_formatter() {
        // Direct coverage of `concat_operand_to_string`'s own `FloatLiteral`
        // arm - this project's own checker-level inference never actually
        // produces a `Type::FloatLiteral` for a bare float literal
        // expression (`checker/expr.rs`'s `Literal::Float` arm collapses
        // to plain `Type::Float`), so this is exercised directly rather
        // than through an end-to-end checker test.
        assert_eq!(
            concat_operand_to_string(&Type::FloatLiteral(1.5)),
            Type::StringLiteral("1.5".to_string())
        );
        let result = resolve_concat_type(
            &Type::FloatLiteral(1.5),
            &Type::StringLiteral("-suffix".to_string()),
        );
        assert_eq!(result, Type::StringLiteral("1.5-suffix".to_string()));
    }

    #[test]
    fn two_literal_strings_fully_constant_fold() {
        assert_eq!(
            resolve_concat_type(
                &Type::StringLiteral("a".to_string()),
                &Type::StringLiteral("b".to_string())
            ),
            Type::StringLiteral("ab".to_string())
        );
    }

    #[test]
    fn an_empty_string_operand_is_dropped_via_the_identity_shortcut() {
        assert_eq!(
            resolve_concat_type(&Type::StringLiteral(String::new()), &Type::Int),
            concat_operand_to_string(&Type::Int)
        );
    }

    #[test]
    fn an_int_typed_left_operand_concatenated_with_a_pure_digit_right_literal_is_numeric() {
        let result = resolve_concat_type(&Type::Int, &Type::StringLiteral("5".to_string()));
        assert!(string_type_has_accessory(
            &result,
            AccessoryKind::NumericString
        ));
    }

    #[test]
    fn an_int_typed_left_operand_concatenated_with_a_negative_signed_right_literal_is_not_numeric()
    {
        let result = resolve_concat_type(&Type::Int, &Type::StringLiteral("-5".to_string()));
        assert!(!string_type_has_accessory(
            &result,
            AccessoryKind::NumericString
        ));
    }
}
