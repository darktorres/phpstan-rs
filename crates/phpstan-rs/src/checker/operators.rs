//! An operand-validity matrix for binary/unary/inc-dec
//! operators - unlocks `InvalidBinaryOperationRule` (`binaryOp.invalid`/
//! `assignOp.invalid`), `InvalidComparisonOperationRule`
//! (`equal.invalid`/`notEqual.invalid`/`smaller.invalid`/
//! `smallerOrEqual.invalid`/`greater.invalid`/`greaterOrEqual.invalid`/
//! `spaceship.invalid`), `InvalidUnaryOperationRule` (`unaryOp.invalid`),
//! and `InvalidIncDecOperationRule` (`preInc.expr`/`preInc.type`/... for
//! all four inc/dec directions).
//!
//! Confidently scoped, not a full port of real PHPStan's own operator
//! type resolver: every check here answers "is this operand type ever
//! legal for this operator," reusing `coercion.rs`'s existing cast-
//! legality surface (`Type::to_string_type` for concat) and a new, analogous
//! `is_number_operand` (mirrors real PHPStan's own `Type::toNumber()`
//! legality) - not "does resolving this exact operator expression
//! produce `ErrorType`," which for a handful of real PHPStan cases
//! (division/modulo by a literal zero, in particular) depends on the
//! operator's own *value*, not just its operand *types*, and isn't
//! modeled here.
//!
//! Deliberately diverges from real PHPStan in one respect for every
//! check in this file: a `Union`'s own legality is "at least one member
//! is legal" (matching `coercion.rs::union_coercion`'s own precedent),
//! not real PHPStan's own "every member must be legal": real PHPStan
//! does flag `~$union` for a `numeric-string|int|float`-typed `$union`
//! unless
//! every member individually already passes `isString()`/`isInteger()`/
//! `isFloat()`. This project's own type inference produces much wider,
//! coarser unions than real PHPStan's (see this codebase's own
//! repeatedly-documented "don't guess" precedent), so matching that
//! stricter rule here would trade a real accuracy gain for a real,
//! elevated false-positive rate against this project's own unions -
//! not a good trade for a rule whose whole point is confidence.

use crate::checker::Checker;
use syntax::ast::{BinOp, Expr, ExprKind, UnaryOp};
use syntax::token::Span;
use types::{ArithmeticOp, OrderingOp, Type};

impl<'a> Checker<'a> {
    /// Mirrors real PHPStan's own `Type::toNumber()` legality: every
    /// scalar except a non-numeric string, `null`, and the same
    /// `SimpleXMLElement`/`GMP`/`BcMath\Number` numeric-castable objects
    /// `coercion.rs` already special-cases for `(float)`/`(string)` -
    /// never an array, a `resource`, a `class-string`, or any other
    /// registered class. `Mixed`, an unresolvable class, and a PHPDoc
    /// type alias this project doesn't resolve (both surface as an
    /// object type naming no registered class - see
    /// `self.registry.classes.contains_key`) all fall through the
    /// catch-all, matching this project's usual "don't know, don't
    /// guess" leniency - this arm must tell "confidently not a
    /// number-castable object" apart from "not a real, known class at
    /// all," or a PHPDoc type alias combined with a `mixed` parameter
    /// would be wrongly flagged.
    pub(crate) fn is_number_operand(&self, ty: &Type) -> bool {
        let not_a_number = self.relation().union_of(vec![
            Type::array(Type::mixed(), Type::mixed()),
            Type::class_string(Type::mixed()),
            Type::resource(false),
            Type::resource(true),
        ]);
        ty.union_members().iter().any(|m| {
            if self.relation().is_super_type_of(&not_a_number, m).yes() {
                false
            } else if m.is_string().yes() {
                m.is_numeric_string().yes()
            } else if m.is_object().yes()
                && m.object_class_names()
                    .iter()
                    .any(|c| self.registry.classes.contains_key(c.as_str()))
            {
                self.instance_of_any(m, &["SimpleXMLElement", "GMP", "BcMath\\Number"])
            } else {
                true
            }
        })
    }

    /// Whether `ty` is (or, inside a `Union`, could be) a real
    /// instance of `class` - `is_subclass_of`'s own reflexive "a class is
    /// always its own subclass" covers the exact-name case too.
    fn type_is_instance_of(&self, ty: &Type, class: &str) -> bool {
        ty.union_members()
            .iter()
            .any(|m| self.instance_of_any(m, &[class]))
    }

    /// `~$x`: a `GMP` stays a `GMP` (it overloads `~`, `BcMath\Number`
    /// doesn't); anything else is PHP's own complement.
    pub(crate) fn bitwise_not_type(&self, ty: &Type) -> Type {
        ty.union_members()
            .iter()
            .map(|m| {
                if self.instance_of_any(m, &["GMP"]) {
                    m.clone()
                } else {
                    m.to_bitwise_not()
                }
            })
            .reduce(|a, b| self.relation().union(a, b))
            .unwrap_or_else(Type::never)
    }

    /// The "other side" legality gate real PHPStan's own
    /// `GmpOperatorTypeSpecifyingExtension`/
    /// `BcMathNumberOperatorTypeSpecifyingExtension` share for their
    /// arithmetic operators - GMP/`BcMath\Number` can combine with an
    /// int, a numeric string, or another instance of the same
    /// overloading class, but deliberately *not* a float (both real
    /// extensions reject it - `BcMath\Number`'s own extension checks this
    /// explicitly first; GMP's own just never lists `isFloat()` among its
    /// accepted cases either). Same "any Union member legal" leniency as
    /// `is_number_operand` above, for the same reason (this project's own
    /// unions are coarser than real PHPStan's).
    fn is_valid_overload_operand(&self, ty: &Type, class: &str) -> bool {
        ty.union_members().iter().any(|m| {
            m.is_int().yes() || m.is_numeric_string().yes() || self.instance_of_any(m, &[class])
        })
    }

    /// `GMP`/`BcMath\Number`'s own operator-overloading return
    /// type - `is_number_operand` above already establishes these two
    /// classes as legal arithmetic/comparison operands; this
    /// answers what the result actually is once legality is
    /// established, mirroring real PHPStan's own
    /// `GmpOperatorTypeSpecifyingExtension`/
    /// `BcMathNumberOperatorTypeSpecifyingExtension` (comparison operators
    /// and unary `-`/`+` are deliberately not covered here - the
    /// comparisons already produce `bool` regardless of operand via
    /// `infer_binary`'s own existing catch-all, and unary `-`/`+` already
    /// pass an object operand's type straight through unchanged; `~` is
    /// `bitwise_not_type`). `None` when neither side is one of these two
    /// classes at all (the overwhelming common case, cheap to rule out
    /// first), when the class's own operator set doesn't include `op`
    /// (`BcMath\Number` doesn't overload the bitwise/shift operators at
    /// all), or when the other side isn't a valid operand - left alone
    /// rather than guessing a wrong result type, matching this project's
    /// "don't guess" precedent; `is_number_operand`'s own independent
    /// legality check still governs whether an `binaryOp.invalid`-style
    /// diagnostic fires for a genuinely bad combination.
    pub(crate) fn binary_operator_overload_result(
        &self,
        op: &BinOp,
        l_ty: &Type,
        r_ty: &Type,
    ) -> Option<Type> {
        for class in ["GMP", "BcMath\\Number"] {
            let l_is = self.type_is_instance_of(l_ty, class);
            let r_is = self.type_is_instance_of(r_ty, class);
            if !l_is && !r_is {
                continue;
            }
            let bitwise_or_shift = matches!(
                op,
                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
            );
            if bitwise_or_shift && class != "GMP" {
                return None;
            }
            if !matches!(
                op,
                BinOp::Add
                    | BinOp::Sub
                    | BinOp::Mul
                    | BinOp::Div
                    | BinOp::Pow
                    | BinOp::Mod
                    | BinOp::BitAnd
                    | BinOp::BitOr
                    | BinOp::BitXor
                    | BinOp::Shl
                    | BinOp::Shr
                    | BinOp::Spaceship
            ) {
                return None;
            }
            if *op == BinOp::Spaceship {
                return Some(Type::int_range(Some(-1), Some(1)));
            }
            let other = if l_is { r_ty } else { l_ty };
            return if self.is_valid_overload_operand(other, class) {
                Some(Type::object(class.into(), Vec::new()))
            } else {
                None
            };
        }
        None
    }

    /// A "byte-wise bitwise op" operand - real PHP's `&`/`|`/`^` accept
    /// two strings directly (a per-byte operation, no numeric coercion
    /// at all), independent of whether either string is numeric.
    fn is_exactly_string_operand(&self, ty: &Type) -> bool {
        ty.union_members().iter().any(|m| m.is_string().yes())
    }

    /// Real PHPStan's `InvalidBinaryOperationRule` (`binaryOp.invalid` for
    /// an `ExprKind::Binary`, `assignOp.invalid` for an
    /// `ExprKind::CompoundAssign`). `span` should be the left operand's
    /// own span, matching real PHPStan's `->line($left->getStartLine())`.
    pub(crate) fn check_binary_operand_validity(
        &mut self,
        op: &BinOp,
        l_ty: &Type,
        r_ty: &Type,
        is_compound_assign: bool,
        span: Span,
    ) -> bool {
        // Don't guess: a bare `Mixed` operand might be anything at
        // runtime, on either side, for any operator here - matches real
        // PHPStan's own top-level `$type instanceof MixedType => skip`
        // check in `RuleLevelHelper::findTypeToCheckImplementation`,
        // which every one of these operator-specific callbacks sits
        // behind. Without this, `$mixedFlags | self::SOME_INT_MASK`
        // would be wrongly flagged - `is_number_operand`'s own per-type
        // leniency doesn't help here, since
        // `Mixed | non-numeric-literal` fails *both* of `BitAnd`'s own
        // legal combinations even though either one might hold at
        // runtime. An error or `never` operand (a division by zero) is
        // already reported or never evaluated.
        if [l_ty, r_ty]
            .iter()
            .any(|t| t.is_mixed() || t.is_error() || t.is_never())
        {
            return false;
        }
        let Some(valid) = self.binary_operand_validity(op, l_ty, r_ty) else {
            return false;
        };
        if valid {
            return false;
        }
        let sigil = binop_sigil(op);
        let (op_str, identifier) = if is_compound_assign {
            (format!("{sigil}="), "assignOp.invalid")
        } else {
            (sigil.to_string(), "binaryOp.invalid")
        };
        self.report(
            format!("Binary operation \"{op_str}\" between {l_ty} and {r_ty} results in an error."),
            span,
            2,
            identifier,
        );
        true
    }

    /// Flags a condition whose type isn't confidently `bool` at all,
    /// regardless of its value - distinct from `inferred_constant_
    /// condition`'s always-true/false check. `mixed` is always
    /// exempted: this project has no explicit-vs-implicit-mixed
    /// provenance on an inferred expression type, so it reads as "don't
    /// know, may be bool" rather than a confirmed non-bool.
    pub(crate) fn check_non_boolean_condition(
        &mut self,
        e: &Expr,
        ty: &Type,
        context: &str,
        identifier: &'static str,
    ) {
        if !self.settings.strict_rules.check_boolean_conditions {
            return;
        }
        if ty.is_mixed() || ty.is_bool().yes() {
            return;
        }
        self.report(
            format!("Only booleans are allowed in {context}, {ty} given."),
            e.span,
            5,
            identifier,
        );
    }

    /// The `&&`/`||` operand-shaped sibling of `check_non_boolean_
    /// condition` above - real `BooleanInBooleanAndRule`/
    /// `BooleanInBooleanOrRule`'s own message shape names which side
    /// failed instead of describing the whole expression as "a
    /// condition".
    pub(crate) fn check_non_boolean_and_or_operand(
        &mut self,
        e: &Expr,
        ty: &Type,
        sigil: &str,
        side: &str,
        identifier: &'static str,
    ) {
        if !self.settings.strict_rules.check_boolean_conditions {
            return;
        }
        if ty.is_mixed() || ty.is_bool().yes() {
            return;
        }
        self.report(
            format!("Only booleans are allowed in {sigil}, {ty} given on the {side} side."),
            e.span,
            5,
            identifier,
        );
    }

    /// Flags structurally-identical operands on both sides of a
    /// comparison or `Sub` (`$x === $x`, `$a - $a`). Deliberately silent
    /// on whether the result is always true/false/zero (a float `NAN`
    /// makes `$x === $x` false) - just that comparing/subtracting an
    /// expression from itself is almost always a copy-paste mistake.
    pub(crate) fn check_duplicate_operands(&mut self, op: &BinOp, l: &Expr, r: &Expr) {
        if !matches!(
            op,
            BinOp::Identical
                | BinOp::NotIdentical
                | BinOp::Eq
                | BinOp::NotEq
                | BinOp::Lt
                | BinOp::Gt
                | BinOp::LtEq
                | BinOp::GtEq
                | BinOp::Spaceship
                | BinOp::Sub
        ) {
            return;
        }
        if !is_duplicate_side_effect_free_operand(l, r) {
            return;
        }
        self.report(
            "Both sides of this expression are the same.".to_string(),
            l.span,
            5,
            "expr.duplicateOperands",
        );
    }

    /// `$x = $x` - reuses `check_duplicate_operands`'s own operand-
    /// identity helper. A compound-assign self-target (`$x += $x`) is a
    /// real, meaningful operation (doubling), not a no-op, so it's
    /// deliberately not covered here.
    pub(crate) fn check_self_assignment(&mut self, lhs: &Expr, rhs: &Expr) {
        if !is_duplicate_side_effect_free_operand(lhs, rhs) {
            return;
        }
        self.report(
            "This assignment has no effect, both sides are the same.".to_string(),
            lhs.span,
            5,
            "expr.duplicateOperands",
        );
    }

    /// `$x / 0`/`$x % 0` - a real, unconditional PHP
    /// fatal error (`DivisionByZeroError`) whenever the divisor is
    /// statically known to be zero. Real PHPStan's own equivalent
    /// (`BinaryOpHandler`) models this as a throw point
    /// (`DivisionByZeroError` added to the expression's own inferred
    /// throw set), not an `ErrorType`/`binaryOp.invalid`-style
    /// diagnostic - this project has no general expression-level
    /// throw-point inference to feed that into, so this is a deliberate
    /// adaptation: a dedicated diagnostic instead, under this project's
    /// own identifiers (`division.byZero`/`modulo.byZero`, not real
    /// PHPStan's own naming, since this isn't really the same mechanism)
    /// rather than force-fitting it into the unrelated throw-tracking
    /// machinery. Reported when every value the divisor can have is zero
    /// (`0`, `0.0`, `int<0, 0>`).
    pub(crate) fn check_division_by_zero(&mut self, op: &BinOp, divisor_ty: &Type, span: Span) {
        let (op_word, identifier) = match op {
            BinOp::Div => ("Division", "division.byZero"),
            BinOp::Mod => ("Modulo", "modulo.byZero"),
            _ => return,
        };
        let values = divisor_ty.finite_types();
        let is_zero = !values.is_empty()
            && values
                .iter()
                .all(|v| *v == Type::int_literal(0) || *v == Type::float_literal(0.0));
        if !is_zero {
            return;
        }
        self.report(format!("{op_word} by zero."), span, 0, identifier);
    }

    /// `None` = this operator's operand legality isn't modeled here:
    /// `&&`/`||`/`xor`/`??` can never be operand-illegal in real PHP,
    /// `===`/`!==` accept any two operands, the comparison operators
    /// (`==`, `<`, ...) have their own, narrower `check_comparison_
    /// validity` below, and PHP 8.5's `|>` pipe has no legality concept
    /// this project's own coercion surface can answer without resolving
    /// an arbitrary callable.
    fn binary_operand_validity(&self, op: &BinOp, l_ty: &Type, r_ty: &Type) -> Option<bool> {
        match op {
            BinOp::Concat => Some(
                l_ty.to_string_type(self.registry).is_some()
                    && r_ty.to_string_type(self.registry).is_some(),
            ),
            // `+` is PHP's array-union operator when both sides are
            // arrays, ordinary arithmetic otherwise - one array and one
            // non-array is a hard runtime error (the caller's own
            // top-level `Mixed` guard already excused a `Mixed` operand
            // that might itself be an array at runtime).
            BinOp::Add => {
                let l_arr = l_ty.is_array().yes();
                let r_arr = r_ty.is_array().yes();
                if l_arr && r_arr {
                    Some(true)
                } else if l_arr != r_arr {
                    Some(false)
                } else {
                    Some(self.is_number_operand(l_ty) && self.is_number_operand(r_ty))
                }
            }
            BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Pow
            | BinOp::Mod
            | BinOp::Shl
            | BinOp::Shr => Some(self.is_number_operand(l_ty) && self.is_number_operand(r_ty)),
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => Some(
                (self.is_number_operand(l_ty) && self.is_number_operand(r_ty))
                    || (self.is_exactly_string_operand(l_ty)
                        && self.is_exactly_string_operand(r_ty)),
            ),
            _ => None,
        }
    }

    /// Real PHPStan's `InvalidComparisonOperationRule`: a number
    /// (strictly `int|float`, not a numeric string or bool) compared
    /// against a possibly-`null` array or object always resolves how PHP
    /// itself would (`0`/comparing-as-scalars in older PHP, a hard
    /// `TypeError` since PHP 8) - a common real bug shape (an accidental
    /// `array` where a `float` was expected reaching a `<`/`>` check).
    /// `===`/`!==` are never checked - PHP identity comparison accepts
    /// any two operands.
    pub(crate) fn check_comparison_validity(
        &mut self,
        op: &BinOp,
        l_ty: &Type,
        r_ty: &Type,
        span: Span,
    ) {
        let Some(identifier) = comparison_identifier(op) else {
            return;
        };
        if l_ty.is_mixed() || r_ty.is_mixed() {
            return;
        }
        let l_num = self.is_strictly_number(l_ty);
        let r_num = self.is_strictly_number(r_ty);
        if l_num == r_num {
            return;
        }
        // An object or array, perhaps `null`, but not only `null`.
        let nullable_object_or_array = |ty: &Type| {
            let or_null = |kind: Type| self.relation().union_of(vec![kind, Type::null()]);
            !ty.is_null().yes()
                && (self
                    .relation()
                    .is_super_type_of(&or_null(Type::object_without_class()), ty)
                    .yes()
                    || self
                        .relation()
                        .is_super_type_of(&or_null(Type::array(Type::mixed(), Type::mixed())), ty)
                        .yes())
        };
        let invalid =
            (l_num && nullable_object_or_array(r_ty)) || (r_num && nullable_object_or_array(l_ty));
        if !invalid {
            return;
        }
        self.report(
            format!(
                "Comparison operation \"{}\" between {l_ty} and {r_ty} results in an error.",
                binop_sigil(op)
            ),
            span,
            2,
            identifier,
        );
    }

    /// `~` takes an `int`, a `float` or a `string`, and a `GMP`, which
    /// overloads it (`BcMath\Number` doesn't); a type some value of which
    /// is one of those passes.
    fn bitnot_operand_is_valid(&self, ty: &Type) -> bool {
        let operand = self
            .relation()
            .union_of(vec![Type::int(), Type::float(), Type::string()]);
        !self.relation().is_super_type_of(&operand, ty).no() || self.type_is_instance_of(ty, "GMP")
    }

    /// Real PHPStan's `InvalidUnaryOperationRule` (`unaryOp.invalid`).
    /// `UnaryOp::Not` (`!`) is never checked - every PHP value has a
    /// truthiness, matching this project's own `check_cast_validity`
    /// never checking `(bool)` for the identical reason.
    pub(crate) fn check_unary_operand_validity(
        &mut self,
        op: &UnaryOp,
        inner_ty: &Type,
        span: Span,
    ) {
        let valid = match op {
            UnaryOp::Neg | UnaryOp::Plus => self.is_number_operand(inner_ty),
            UnaryOp::BitNot => self.bitnot_operand_is_valid(inner_ty),
            _ => return,
        };
        if valid {
            return;
        }
        let sigil = match op {
            UnaryOp::Neg => "-",
            UnaryOp::Plus => "+",
            _ => "~",
        };
        self.report(
            format!("Unary operation \"{sigil}\" on {inner_ty} results in an error."),
            span,
            2,
            "unaryOp.invalid",
        );
    }

    /// Real PHPStan's `InvalidIncDecOperationRule`: `{name}.expr` when the
    /// target isn't an assignable l-value at all (`5++`, `foo()++`),
    /// `{name}.type` when it is one but its current type can never legally
    /// be incremented/decremented (an array, or a plain object with no
    /// native operator overload). On PHP versions that warn about it, an
    /// operand PHP deprecates or ignores is reported too
    /// (`check_incdec_version_warnings`).
    pub(crate) fn check_incdec_validity(
        &mut self,
        op: &UnaryOp,
        target: &syntax::ast::Expr,
        inner_ty: &Type,
        span: Span,
    ) {
        use syntax::ast::ExprKind;

        let name = match op {
            UnaryOp::PreInc => "preInc",
            UnaryOp::PostInc => "postInc",
            UnaryOp::PreDec => "preDec",
            UnaryOp::PostDec => "postDec",
            _ => return,
        };
        let symbol = match op {
            UnaryOp::PreInc | UnaryOp::PostInc => "++",
            _ => "--",
        };
        if !matches!(
            target.kind,
            ExprKind::Variable(_)
                | ExprKind::Index(..)
                | ExprKind::PropertyAccess(..)
                | ExprKind::DynamicPropertyAccess(..)
                | ExprKind::ClassConstFetch(..)
                | ExprKind::ClassNameFetch(_)
                | ExprKind::StaticPropertyFetch(..)
                | ExprKind::DynamicStaticPropertyFetch(..)
        ) {
            self.report(
                format!("Cannot use {symbol} on a non-variable."),
                span,
                0,
                format!("{name}.expr"),
            );
            return;
        }
        // A `never` operand is unreachable code, not a bad operand.
        if inner_ty.is_never() {
            return;
        }
        if self.is_incdec_operand_valid(inner_ty) {
            let increment = matches!(op, UnaryOp::PreInc | UnaryOp::PostInc);
            self.check_incdec_version_warnings(increment, inner_ty, span, name);
            return;
        }
        self.report(
            format!("Cannot use {symbol} on {inner_ty}."),
            span,
            0,
            format!("{name}.type"),
        );
    }

    /// What PHP deprecates or ignores about `++`/`--` (`increment`) on
    /// `inner_ty`, reported when every configured PHP version warns about
    /// it: from PHP 8.3, `++` on a non-alphanumeric string and `--` on a
    /// non-numeric one are deprecated (`{name}.deprecated`, level 0), and
    /// `++`/`--` on a bool or `--` on `null` have no effect
    /// (`{name}.noEffect`, level 3); from PHP 8.5 `++` on any non-numeric
    /// string is deprecated. A type only some of whose values do this is
    /// reported at level 7.
    fn check_incdec_version_warnings(
        &mut self,
        increment: bool,
        inner_ty: &Type,
        span: Span,
        name: &str,
    ) {
        if !self
            .registry
            .php_supports(types::PhpFeature::IncDecDeprecations)
            .yes()
        {
            return;
        }
        let alphanumeric_deprecated = self
            .registry
            .php_supports(types::PhpFeature::AlphanumericStringIncrementDeprecated)
            .yes();
        let verb = if increment { "Increment" } else { "Decrement" };
        let rows: Vec<Option<(&'static str, String)>> = inner_ty
            .union_members()
            .iter()
            .map(|member| {
                if member.is_bool().yes() {
                    return Some(("noEffect", format!("{verb} on type bool has no effect")));
                }
                if !increment && member.is_null().yes() {
                    return Some(("noEffect", format!("{verb} on type null has no effect")));
                }
                let [s] = member.constant_strings()[..] else {
                    return None;
                };
                let bytes = s.as_bytes();
                if types::numeric_value(bytes).is_some() {
                    return None;
                }
                let message = if increment {
                    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_alphanumeric) {
                        "Increment on non-alphanumeric string is deprecated"
                    } else if alphanumeric_deprecated {
                        "Increment on non-numeric string is deprecated, use str_increment() instead"
                    } else {
                        return None;
                    }
                } else if bytes.is_empty() {
                    "Decrement on empty string is deprecated as non-numeric"
                } else {
                    "Decrement on non-numeric string has no effect and is deprecated"
                };
                Some(("deprecated", message.to_string()))
            })
            .collect();
        let Some((kind, message)) = rows.iter().flatten().next().cloned() else {
            return;
        };
        let level = if rows.iter().all(Option::is_some) {
            if kind == "deprecated" { 0 } else { 3 }
        } else {
            7
        };
        self.report(
            format!("{message} ({inner_ty} given)."),
            span,
            level,
            format!("{name}.{kind}"),
        );
    }

    /// Real PHPStan's `RuleLevelHelper::isNumberType` (as used by
    /// `InvalidComparisonOperationRule`): strictly `int|float`, plus its
    /// own real `SimpleXMLElement`/`GMP` exception (a number-castable
    /// object counts as a "number" in its own right for this
    /// comparison) - extended one class further to `BcMath\Number` too,
    /// matching this project's own existing number-castable-class
    /// precedent everywhere else in this file/`coercion.rs`.
    fn is_strictly_number(&self, ty: &Type) -> bool {
        self.relation()
            .is_super_type_of(
                &self.relation().union_of(vec![Type::int(), Type::float()]),
                ty,
            )
            .yes()
            || self.instance_of_any(ty, &["SimpleXMLElement", "GMP", "BcMath\\Number"])
    }

    /// Real PHPStan's own `InvalidIncDecOperationRule` allowed-type union
    /// (`float|int|string|SimpleXMLElement|GMP|null|bool`), minus the
    /// version-gated `BcMath\Number` addition and the deprecation-driven
    /// numeric-string/non-null/non-bool narrowing - see `check_incdec_
    /// validity`'s own doc comment for why.
    fn is_incdec_operand_valid(&self, ty: &Type) -> bool {
        let invalid = self.relation().union_of(vec![
            Type::array(Type::mixed(), Type::mixed()),
            Type::class_string(Type::mixed()),
        ]);
        ty.union_members().iter().any(|m| {
            if self.relation().is_super_type_of(&invalid, m).yes() {
                false
            } else if m.is_object().yes() {
                self.instance_of_any(m, &["SimpleXMLElement", "GMP"])
            } else {
                true
            }
        })
    }

    /// Whether every value of `ty` is an instance of one of `classes`.
    fn instance_of_any(&self, ty: &Type, classes: &[&str]) -> bool {
        let of = self.relation().union_of(
            classes
                .iter()
                .map(|c| Type::object((*c).into(), Vec::new()))
                .collect(),
        );
        self.relation().is_super_type_of(&of, ty).yes()
    }
}

/// The arithmetic, bitwise or shift operator, or `<=>`, `op` is.
pub(crate) fn arithmetic_op(op: &BinOp) -> Option<ArithmeticOp> {
    Some(match op {
        BinOp::Add => ArithmeticOp::Add,
        BinOp::Sub => ArithmeticOp::Sub,
        BinOp::Mul => ArithmeticOp::Mul,
        BinOp::Div => ArithmeticOp::Div,
        BinOp::Mod => ArithmeticOp::Mod,
        BinOp::Pow => ArithmeticOp::Pow,
        BinOp::BitAnd => ArithmeticOp::BitAnd,
        BinOp::BitOr => ArithmeticOp::BitOr,
        BinOp::BitXor => ArithmeticOp::BitXor,
        BinOp::Shl => ArithmeticOp::Shl,
        BinOp::Shr => ArithmeticOp::Shr,
        BinOp::Spaceship => ArithmeticOp::Spaceship,
        _ => return None,
    })
}

/// The ordering comparison `op` is.
pub(crate) fn ordering_op(op: &BinOp) -> Option<OrderingOp> {
    Some(match op {
        BinOp::Lt => OrderingOp::Lt,
        BinOp::LtEq => OrderingOp::LtEq,
        BinOp::Gt => OrderingOp::Gt,
        BinOp::GtEq => OrderingOp::GtEq,
        _ => return None,
    })
}

fn comparison_identifier(op: &BinOp) -> Option<&'static str> {
    Some(match op {
        BinOp::Eq => "equal.invalid",
        BinOp::NotEq => "notEqual.invalid",
        BinOp::Lt => "smaller.invalid",
        BinOp::LtEq => "smallerOrEqual.invalid",
        BinOp::Gt => "greater.invalid",
        BinOp::GtEq => "greaterOrEqual.invalid",
        BinOp::Spaceship => "spaceship.invalid",
        _ => return None,
    })
}

pub(crate) fn binop_sigil(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::Concat => ".",
        BinOp::Eq => "==",
        BinOp::Identical => "===",
        BinOp::NotEq => "!=",
        BinOp::NotIdentical => "!==",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::LtEq => "<=",
        BinOp::GtEq => ">=",
        BinOp::Spaceship => "<=>",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::LogicalAnd => "and",
        BinOp::LogicalOr => "or",
        BinOp::LogicalXor => "xor",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Coalesce => "??",
        BinOp::Pipe => "|>",
    }
}

/// Whether `l`/`r` are the exact same side-effect-free expression -
/// deliberately narrow to a bare variable, or a `PropertyAccess`/
/// `Index` chain rooted in one, recursing structurally. Never a call/
/// method-call/`new`/etc: those could have side effects or return a
/// fresh, non-identical value each time, so "looks the same" wouldn't
/// mean "IS the same" there. Never a literal either - `1 === 1` isn't
/// a copy-paste mistake the way `$x === $x` is.
fn is_duplicate_side_effect_free_operand(l: &Expr, r: &Expr) -> bool {
    syntax::stack::guard(|| match (&l.peeled().kind, &r.peeled().kind) {
        (ExprKind::Variable(a), ExprKind::Variable(b)) => a == b,
        (ExprKind::PropertyAccess(la, lp, ln), ExprKind::PropertyAccess(ra, rp, rn)) => {
            ln == rn && lp.name == rp.name && is_duplicate_side_effect_free_operand(la, ra)
        }
        (ExprKind::Index(la, Some(li)), ExprKind::Index(ra, Some(ri))) => {
            is_duplicate_side_effect_free_operand(la, ra)
                && is_duplicate_side_effect_free_operand(li, ri)
        }
        _ => false,
    })
}
