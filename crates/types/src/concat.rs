//! Concatenation: PHPStan's `InitializerExprTypeResolver::resolveConcatType`.

use crate::{AccessoryKind, ClassHierarchy, PhpString, Repr, Type};

/// Real PHPStan's own `CALCULATE_SCALARS_LIMIT` (`InitializerExprTypeResolver
/// .php`) - bounds the constant-string cross-product below so a
/// concatenation of two wide literal unions can't blow up combinatorially.
const CALCULATE_SCALARS_LIMIT: usize = 8;

impl Type {
    /// A full port of real PHPStan's own `InitializerExprTypeResolver
    /// ::resolveConcatType` - the shared engine behind both `BinOp::Concat`
    /// and `ExprKind::Interpolated` (the latter as a left-fold over its own
    /// parts). `l`/`r` are the operands' own *original* inferred types (not
    /// yet projected to their "as a string" shape) - this function does that
    /// projection itself via `Type::to_string_type`, then also consults
    /// the originals directly wherever real PHPStan's own algorithm does
    /// (the numeric-string propagation's own `$left->isInteger()`/
    /// `$right`-range checks, which run against the un-stringified operand).
    pub fn concat(&self, r: &Type, hierarchy: &dyn ClassHierarchy) -> Type {
        stack::guard(|| {
            let as_string = |ty: &Type| ty.to_string_type(hierarchy).unwrap_or(Type::string());
            let l_str = as_string(self);
            let r_str = as_string(r);

            // Empty-string identity shortcuts.
            if let Repr::StringLiteral(s) = l_str.repr()
                && s.is_empty()
            {
                return r_str;
            }
            if let Repr::StringLiteral(s) = r_str.repr()
                && s.is_empty()
            {
                return l_str;
            }
            // Full constant fold when both sides are single literals.
            if let (Repr::StringLiteral(a), Repr::StringLiteral(b)) = (l_str.repr(), r_str.repr()) {
                return Type::string_literal(a.concat(b));
            }

            // Bounded constant-string cross product - covers a `Union` of
            // literals on either/both sides (e.g. `bool`'s own `toString()`
            // projection) without needing its own separate special case.
            let l_consts = constant_strings(&l_str);
            let r_consts = constant_strings(&r_str);
            let combined_count = l_consts.len() * r_consts.len();
            if combined_count > 0 && combined_count <= CALCULATE_SCALARS_LIMIT {
                let mut combos: Vec<PhpString> = Vec::new();
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
                        combos.push(a.concat(b));
                    }
                }
                combos.sort();
                combos.dedup();
                if !combos.is_empty() {
                    return if combos.len() == 1 {
                        Type::string_literal(combos.into_iter().next().unwrap())
                    } else {
                        Type::new(Repr::Union(
                            combos.into_iter().map(Type::string_literal).collect(),
                        ))
                    };
                }
            }

            let mut accessories: Vec<AccessoryKind> = Vec::new();
            let l_non_empty = l_str.is_refined_string(AccessoryKind::NonEmptyString).yes();
            let r_non_empty = r_str.is_refined_string(AccessoryKind::NonEmptyString).yes();
            let l_non_falsy = l_str.is_refined_string(AccessoryKind::NonFalsyString).yes();
            let r_non_falsy = r_str.is_refined_string(AccessoryKind::NonFalsyString).yes();
            if (l_non_empty && r_non_empty) || l_non_falsy || r_non_falsy {
                accessories.push(AccessoryKind::NonFalsyString);
            } else if l_non_empty || r_non_empty {
                accessories.push(AccessoryKind::NonEmptyString);
            }

            if l_str.is_refined_string(AccessoryKind::LiteralString).yes()
                && r_str.is_refined_string(AccessoryKind::LiteralString).yes()
            {
                accessories.push(AccessoryKind::LiteralString);
            }
            if l_str
                .is_refined_string(AccessoryKind::LowercaseString)
                .yes()
                && r_str
                    .is_refined_string(AccessoryKind::LowercaseString)
                    .yes()
            {
                accessories.push(AccessoryKind::LowercaseString);
            }
            if l_str
                .is_refined_string(AccessoryKind::UppercaseString)
                .yes()
                && r_str
                    .is_refined_string(AccessoryKind::UppercaseString)
                    .yes()
            {
                accessories.push(AccessoryKind::UppercaseString);
            }

            // A numeric left operand stays numeric with digits appended
            // (`'1.5' . '0'`, `'1e5' . '5'`), unless it ends in the
            // whitespace a numeric string may carry (`'1 ' . '5'` is
            // `'1 5'`). An int's text is plain digits, so after one any
            // right side that keeps `1<right>` numeric does. An always-
            // possible `''` left member is set aside first (PHPStan's
            // `TypeCombinator::remove($leftStringType, new
            // ConstantStringType(''))`).
            let l_nonempty = strip_empty_string_member(&l_str);
            if l_nonempty
                .is_refined_string(AccessoryKind::NumericString)
                .yes()
            {
                let l_is_int = matches!(
                    self.repr(),
                    Repr::Int | Repr::IntLiteral(_) | Repr::IntRange(_, _)
                );
                let l_consts = constant_strings(&l_nonempty);
                let l_no_trailing_space = l_is_int
                    || l_nonempty
                        .is_refined_string(AccessoryKind::DecimalIntegerString)
                        .yes()
                    || (!l_consts.is_empty()
                        && l_consts.iter().all(|c| {
                            c.as_bytes()
                                .last()
                                .is_none_or(|b| !b" \t\n\r\x0b\x0c".contains(b))
                        }));
                let keeps_numeric = |right: &PhpString| {
                    let right = right.as_bytes();
                    if l_is_int {
                        crate::numeric_value(&[b"1", right].concat()).is_some()
                    } else {
                        right.iter().all(u8::is_ascii_digit)
                    }
                };
                let right_is_non_negative_int_range = r
                    .int_bounds()
                    .is_some_and(|(min, _)| min.is_some_and(|m| m >= 0));
                let all_right_constants_keep_it = !r_consts.is_empty()
                    && r_consts.iter().filter(|v| !v.is_empty()).all(keeps_numeric);
                if l_no_trailing_space
                    && (all_right_constants_keep_it || right_is_non_negative_int_range)
                {
                    accessories.push(AccessoryKind::NumericString);
                }
            }

            // A byte no decimal-int string has at its position keeps the
            // result a string when used as an array key.
            let may_be_empty = |ty: &Type, consts: &[PhpString]| {
                if consts.is_empty() {
                    !ty.is_refined_string(AccessoryKind::NonEmptyString).yes()
                } else {
                    consts.iter().any(PhpString::is_empty)
                }
            };
            let class_string =
                |ty: &Type| matches!(ty.repr(), Repr::ClassString(_) | Repr::ClassKindString(_));
            let (l_consts, r_consts) = (constant_strings(&l_str), constant_strings(&r_str));
            let (l_empty, r_empty) = (
                may_be_empty(&l_str, &l_consts),
                may_be_empty(&r_str, &r_consts),
            );
            let l_breaks = class_string(&l_str)
                || (!l_consts.is_empty()
                    && l_consts.iter().all(|c| left_breaks(c.as_bytes(), r_empty)));
            let r_breaks = class_string(&r_str)
                || (!r_consts.is_empty()
                    && r_consts.iter().all(|c| right_breaks(c.as_bytes(), l_empty)));
            if l_breaks || r_breaks {
                accessories.push(AccessoryKind::NonDecimalIntegerString);
            }

            if accessories.is_empty() {
                Type::string()
            } else {
                let mut members = vec![Type::string()];
                members.extend(accessories.into_iter().map(Type::accessory));
                Type::new(Repr::Intersection(members))
            }
        })
    }
}

/// The longest decimal-int string, `-9223372036854775808`.
const MAX_DECIMAL_INT_LEN: usize = 20;

/// Whether `left . r` can't be a decimal-int string for any `r`: `left`
/// has a non-digit other than a leading `-`, starts with `-0`, or starts
/// with a `0` something follows (itself, or a right side that can't be
/// empty), or is too long.
fn left_breaks(left: &[u8], right_may_be_empty: bool) -> bool {
    let Some(&first) = left.first() else {
        return false;
    };
    left.len() > MAX_DECIMAL_INT_LEN
        || left[1..].iter().any(|b| !b.is_ascii_digit())
        || (!first.is_ascii_digit() && first != b'-')
        || left.starts_with(b"-0")
        || (first == b'0' && (left.len() > 1 || !right_may_be_empty))
}

/// Whether `l . right` can't be a decimal-int string for any `l`: `right`
/// has a non-digit, other than a `-` that may come first in the result,
/// or is too long.
fn right_breaks(right: &[u8], left_may_be_empty: bool) -> bool {
    let Some(&first) = right.first() else {
        return false;
    };
    right.len() > MAX_DECIMAL_INT_LEN
        || right[1..].iter().any(|b| !b.is_ascii_digit())
        || (!first.is_ascii_digit() && (first != b'-' || !left_may_be_empty))
}

/// Strips a `Repr::StringLiteral("")` member out of a `Union` (a no-op
/// for every other shape) - mirrors real PHPStan's own `TypeCombinator::
/// remove($leftStringType, new ConstantStringType(''))` step in
/// `resolveConcatType`'s own numeric-string propagation below, needed so
/// e.g. a `bool`'s own `toString()` projection (`Union(['', '1'])`)
/// doesn't fail `NumericString` proof-checking purely because of its own
/// always-possible empty-string member.
fn strip_empty_string_member(ty: &Type) -> Type {
    match ty.repr() {
        Repr::Union(members) => {
            let filtered: Vec<Type> = members
                .iter()
                .filter(|m| **m != Type::string_literal(PhpString::default()))
                .cloned()
                .collect();
            match filtered.len() {
                0 => ty.clone(),
                1 => filtered.into_iter().next().unwrap(),
                _ => Type::new(Repr::Union(filtered)),
            }
        }
        _ => ty.clone(),
    }
}

/// Every literal string value `ty` could concretely be, when it's a
/// `StringLiteral` or a `Union` of one - `vec![]` for anything else
/// (never guessed at for a bare `String`/accessoried `Intersection`).
fn constant_strings(ty: &Type) -> Vec<PhpString> {
    match ty.repr() {
        Repr::StringLiteral(s) => vec![s.clone()],
        Repr::Union(members) => members.iter().flat_map(constant_strings).collect(),
        _ => Vec::new(),
    }
}
