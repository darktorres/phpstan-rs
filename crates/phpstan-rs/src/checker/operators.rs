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
use syntax::ast::unwrap_grouping;
use syntax::ast::{BinOp, Expr, ExprKind, Literal, UnaryOp};
use syntax::token::Span;
use types::{AccessoryKind, Type};

impl<'a> Checker<'a> {
    /// Mirrors real PHPStan's own `Type::toNumber()` legality: every
    /// scalar except a non-numeric string, `null`, and the same
    /// `SimpleXMLElement`/`GMP`/`BcMath\Number` numeric-castable objects
    /// `coercion.rs` already special-cases for `(float)`/`(string)` -
    /// never an array, a `resource`, a `class-string`, or any other
    /// registered class. `Mixed`, an unresolvable class, and a PHPDoc
    /// type alias this project doesn't resolve (both surface as
    /// `Type::Object` with no matching registry entry - see
    /// `self.registry.classes.contains_key`) all fall through the
    /// catch-all, matching this project's usual "don't know, don't
    /// guess" leniency - this arm must tell "confidently not a
    /// number-castable object" apart from "not a real, known class at
    /// all," or a PHPDoc type alias combined with a `mixed` parameter
    /// would be wrongly flagged.
    pub(crate) fn is_number_operand(&self, ty: &Type) -> bool {
        match ty {
            Type::Array(..) | Type::ArrayShape(_) => false,
            Type::ClassString(_) | Type::ClassKindString(_) => false,
            Type::String => false,
            Type::StringLiteral(_) => types::Relation::structural()
                .subtype(ty, &Type::Accessory(AccessoryKind::NumericString)),
            Type::Intersection(members) if members.iter().any(|m| matches!(m, Type::String)) => {
                types::Relation::structural()
                    .subtype(ty, &Type::Accessory(AccessoryKind::NumericString))
            }
            Type::Resource { .. } => false,
            Type::Object(name, _) if self.registry.classes.contains_key(name.as_str()) => {
                self.registry.is_subclass_of(name, "SimpleXMLElement")
                    || self.registry.is_subclass_of(name, "GMP")
                    || self.registry.is_subclass_of(name, "BcMath\\Number")
            }
            Type::Union(members) => members.iter().any(|m| self.is_number_operand(m)),
            Type::Intersection(members) => members.iter().any(|m| self.is_number_operand(m)),
            _ => true,
        }
    }

    /// Whether `ty` is (or, inside a `Union`, could be) a real
    /// instance of `class` - `is_subclass_of`'s own reflexive "a class is
    /// always its own subclass" covers the exact-name case too.
    ///
    /// `Type::Intersection` needs handling too, unlike `Union` ("could
    /// be this member"): an `Intersection` value genuinely is every
    /// member simultaneously, so it's just as true an instance of `class`
    /// the moment any one member confirms it (the same "any" shape, for a
    /// different reason).
    fn type_is_instance_of(&self, ty: &Type, class: &str) -> bool {
        match ty {
            Type::Object(name, _) => self.registry.is_subclass_of(name, class),
            Type::Union(members) | Type::Intersection(members) => {
                members.iter().any(|m| self.type_is_instance_of(m, class))
            }
            _ => false,
        }
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
        match ty {
            Type::Int | Type::IntLiteral(_) | Type::IntRange(_, _) => true,
            Type::StringLiteral(_) => types::Relation::structural()
                .subtype(ty, &Type::Accessory(AccessoryKind::NumericString)),
            Type::Intersection(members) if members.iter().any(|m| matches!(m, Type::String)) => {
                types::Relation::structural()
                    .subtype(ty, &Type::Accessory(AccessoryKind::NumericString))
            }
            Type::Object(name, _) => self.registry.is_subclass_of(name, class),
            // Same reasoning as `type_is_instance_of` just above -
            // `Type::Intersection` needs the identical "any member" check.
            Type::Union(members) | Type::Intersection(members) => members
                .iter()
                .any(|m| self.is_valid_overload_operand(m, class)),
            _ => false,
        }
    }

    /// `GMP`/`BcMath\Number`'s own operator-overloading return
    /// type - `is_number_operand` above already establishes these two
    /// classes as legal arithmetic/comparison operands; this
    /// answers what the result actually is once legality is
    /// established, mirroring real PHPStan's own
    /// `GmpOperatorTypeSpecifyingExtension`/
    /// `BcMathNumberOperatorTypeSpecifyingExtension` (comparison operators
    /// and unary `-`/`+`/`~` are deliberately not covered here - the
    /// comparisons already produce `Type::Bool` regardless of operand via
    /// `infer_binary`'s own existing catch-all, and unary `-`/`+`/`~`
    /// already pass a non-string-shaped operand's type straight through
    /// unchanged, both already matching real PHP's actual behavior with
    /// no override needed). `None` when neither side is one of these two
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
                return Some(Type::IntRange(Some(-1), Some(1)));
            }
            let other = if l_is { r_ty } else { l_ty };
            return if self.is_valid_overload_operand(other, class) {
                Some(Type::Object(class.into(), Vec::new()))
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
        match ty {
            Type::String | Type::StringLiteral(_) => true,
            Type::Intersection(members) => members.iter().any(|m| matches!(m, Type::String)),
            Type::Union(members) => members.iter().any(|m| self.is_exactly_string_operand(m)),
            _ => false,
        }
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
    ) {
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
        // runtime.
        if matches!(l_ty, Type::Mixed(_)) || matches!(r_ty, Type::Mixed(_)) {
            return;
        }
        let Some(valid) = self.binary_operand_validity(op, l_ty, r_ty) else {
            return;
        };
        if valid {
            return;
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
    }

    /// Flags a condition whose type isn't confidently `bool` at all,
    /// regardless of its value - distinct from `inferred_constant_
    /// condition`'s always-true/false check. `Type::Mixed(false)` is always
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
        if !self.registry.check_boolean_conditions {
            return;
        }
        if matches!(ty, Type::Mixed(_)) || is_definitely_bool(ty) {
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
        if !self.registry.check_boolean_conditions {
            return;
        }
        if matches!(ty, Type::Mixed(_)) || is_definitely_bool(ty) {
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
    /// machinery. Scoped to a literal `0`/`0.0` divisor only - a
    /// provably-zero *range* (`int<0, 0>`) is a real, if much rarer,
    /// residual not attempted, and a computed (non-literal) divisor is
    /// never guessed at either way. Takes the divisor's own raw
    /// expression, not just its inferred type - a plain float literal
    /// (`0.0`) infers to bare `Type::Float` everywhere else in this
    /// checker (`Type::FloatLiteral` is only ever produced by parsing a
    /// docblock type *string*, never an ordinary literal float
    /// *expression*), so the type alone would silently miss the single
    /// most common real spelling of a literal-zero float divisor -
    /// checked directly against the AST node itself instead, the same
    /// "peek at the literal syntax directly" precedent `is_false_literal`/
    /// `is_true_literal` already establish elsewhere in this codebase.
    pub(crate) fn check_division_by_literal_zero(
        &mut self,
        op: &BinOp,
        divisor: &Expr,
        divisor_ty: &Type,
        span: Span,
    ) {
        let (op_word, identifier) = match op {
            BinOp::Div => ("Division", "division.byZero"),
            BinOp::Mod => ("Modulo", "modulo.byZero"),
            _ => return,
        };
        let is_zero = match divisor_ty {
            Type::IntLiteral(0) => true,
            Type::FloatLiteral(n) => *n == 0.0,
            _ => matches!(&divisor.kind, ExprKind::Literal(Literal::Float(f)) if *f == 0.0),
        };
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
                let l_arr = Checker::is_array_like(l_ty);
                let r_arr = Checker::is_array_like(r_ty);
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
        if matches!(l_ty, Type::Mixed(_)) || matches!(r_ty, Type::Mixed(_)) {
            return;
        }
        let l_num = self.is_strictly_number(l_ty);
        let r_num = self.is_strictly_number(r_ty);
        if l_num == r_num {
            return;
        }
        let invalid = (l_num
            && (is_possibly_nullable_object(r_ty) || is_possibly_nullable_array(r_ty)))
            || (r_num && (is_possibly_nullable_object(l_ty) || is_possibly_nullable_array(l_ty)));
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

    /// `~$gmp` is real, valid, GMP-overloaded syntax
    /// (`GmpUnaryOperatorTypeSpecifyingExtension`'s own operator set
    /// includes `~`, unlike `BcMath\Number`'s equivalent extension, which
    /// doesn't overload `~` at all) - the free `is_bitwise_not_operand`
    /// below has no registry access to special-case it (it unconditionally
    /// rejects every `Type::Object`), so this thin wrapper adds the one
    /// exception.
    fn bitnot_operand_is_valid(&self, ty: &Type) -> bool {
        is_bitwise_not_operand(ty) || self.type_is_instance_of(ty, "GMP")
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

    /// Real PHPStan's `InvalidIncDecOperationRule`. Two independent
    /// checks, matching the real rule's own two identifier families:
    /// `{name}.expr` when the target isn't an assignable l-value at all
    /// (`5++`, `foo()++`), `{name}.type` when it is one but its current
    /// type can never legally be incremented/decremented (an array, or a
    /// plain object with no native operator overload). Deliberately
    /// ignores real PHPStan's own PHP-version-gated deprecation nuance
    /// (non-numeric-string/`null`/`bool` inc-dec is deprecated, not
    /// illegal, on newer PHP versions) - this project has no per-target
    /// PHP-version concept to gate that on, so the older, always-legal
    /// behavior is assumed throughout.
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
                &format!("{name}.expr"),
            );
            return;
        }
        if self.is_incdec_operand_valid(inner_ty) {
            return;
        }
        self.report(
            format!("Cannot use {symbol} on {inner_ty}."),
            span,
            0,
            &format!("{name}.type"),
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
        types::Relation::structural().subtype(ty, &Type::Union(vec![Type::Int, Type::Float]))
            || matches!(ty, Type::Object(name, _) if
                self.registry.is_subclass_of(name, "SimpleXMLElement")
                    || self.registry.is_subclass_of(name, "GMP")
                    || self.registry.is_subclass_of(name, "BcMath\\Number"))
    }

    /// Real PHPStan's own `InvalidIncDecOperationRule` allowed-type union
    /// (`float|int|string|SimpleXMLElement|GMP|null|bool`), minus the
    /// version-gated `BcMath\Number` addition and the deprecation-driven
    /// numeric-string/non-null/non-bool narrowing - see `check_incdec_
    /// validity`'s own doc comment for why.
    fn is_incdec_operand_valid(&self, ty: &Type) -> bool {
        match ty {
            Type::Array(..)
            | Type::ArrayShape(_)
            | Type::ClassString(_)
            | Type::ClassKindString(_) => false,
            Type::Object(name, _) => {
                self.registry.is_subclass_of(name, "SimpleXMLElement")
                    || self.registry.is_subclass_of(name, "GMP")
            }
            Type::Union(members) => members.iter().any(|m| self.is_incdec_operand_valid(m)),
            Type::Intersection(members) => members.iter().any(|m| self.is_incdec_operand_valid(m)),
            _ => true,
        }
    }
}

/// Real PHPStan's own `isString()->yes() || isInteger()->yes() ||
/// isFloat()->yes()` check for `~` - unlike `is_number_operand`, `bool`,
/// `null`, and any object (including `SimpleXMLElement`/`GMP` - real
/// `ObjectType` has no `toBitwiseNotType()` override of its own, unlike
/// its `toNumber()`/`toInteger()`/`toFloat()`) are NOT legal here.
/// Everything else - `Mixed` included - falls through to `true`, the
/// same "don't guess" leniency this file applies everywhere else.
fn is_bitwise_not_operand(ty: &Type) -> bool {
    match ty {
        Type::Bool | Type::True | Type::False | Type::Null => false,
        Type::Array(..)
        | Type::ArrayShape(_)
        | Type::Object(..)
        | Type::ObjectWithoutClass
        | Type::Resource { .. }
        | Type::ClassString(_)
        | Type::ClassKindString(_) => false,
        Type::Intersection(members) => members
            .iter()
            .any(|m| matches!(m, Type::String | Type::Int | Type::Float)),
        Type::Union(members) | Type::BenevolentUnion(members) => {
            members.iter().any(is_bitwise_not_operand)
        }
        _ => true,
    }
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

fn is_object_or_null(ty: &Type) -> bool {
    match ty {
        Type::Null => true,
        Type::Object(..) | Type::ObjectWithoutClass => true,
        // A `Union` is definitely object-or-null only when every
        // member is (any other member could be the runtime value
        // instead); an `Intersection` value genuinely is every member at
        // once, so any single member proving it is object-shaped is
        // enough - the opposite quantifier from `Union`, not the same
        // "any member" shape `type_is_instance_of`/
        // `is_valid_overload_operand` just above needed.
        Type::Union(members) => members.iter().all(is_object_or_null),
        Type::Intersection(members) => members.iter().any(is_object_or_null),
        _ => false,
    }
}

fn is_possibly_nullable_object(ty: &Type) -> bool {
    !matches!(ty, Type::Null) && is_object_or_null(ty)
}

fn is_array_or_null(ty: &Type) -> bool {
    match ty {
        Type::Null => true,
        Type::Array(..) | Type::ArrayShape(_) => true,
        Type::Union(members) => members.iter().all(is_array_or_null),
        // A common shape (`list<T>`/`non-empty-array<T>`, both
        // `Intersection([Array(_), Accessory(_)])`) must not fall to
        // the `_ => false` catch-all below, or `$list < 5`-style
        // comparisons against a `list<T>`-typed value would never be
        // flagged - the identical "any member proves it" reasoning
        // `is_object_or_null` just above uses.
        Type::Intersection(members) => members.iter().any(is_array_or_null),
        _ => false,
    }
}

fn is_possibly_nullable_array(ty: &Type) -> bool {
    !matches!(ty, Type::Null) && is_array_or_null(ty)
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

fn is_definitely_bool(ty: &Type) -> bool {
    match ty {
        Type::Bool | Type::True | Type::False => true,
        Type::Union(members) => members.iter().all(is_definitely_bool),
        Type::Intersection(members) => members.iter().any(is_definitely_bool),
        _ => false,
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
    match (&unwrap_grouping(l).kind, &unwrap_grouping(r).kind) {
        (ExprKind::Variable(a), ExprKind::Variable(b)) => a == b,
        (ExprKind::PropertyAccess(la, lp, ln), ExprKind::PropertyAccess(ra, rp, rn)) => {
            ln == rn && lp.name == rp.name && is_duplicate_side_effect_free_operand(la, ra)
        }
        (ExprKind::Index(la, Some(li)), ExprKind::Index(ra, Some(ri))) => {
            is_duplicate_side_effect_free_operand(la, ra)
                && is_duplicate_side_effect_free_operand(li, ri)
        }
        _ => false,
    }
}
