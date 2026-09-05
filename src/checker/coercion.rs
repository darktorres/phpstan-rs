//! Phase 51: `Type` coercion validity - real PHPStan's own `Type::
//! toInteger()`/`toFloat()`/`toString()` surface, condensed to what
//! `InvalidCastRule`/`EchoRule` actually need: "is this cast/context
//! legal, and if so what does it produce." Every scalar, `null`, `array`
//! (for int/float only - see below), and `Mixed` are always legal; the
//! only real illegality lives on the object side, mirroring real
//! PHPStan's own `ObjectType::toInteger()`/`toFloat()`/`toString()` and
//! `ArrayTypeTrait`'s own always-`ErrorType` `toString()`.
//!
//! Deliberately narrower than real PHPStan in one respect: `toBoolean()`
//! is never modeled here at all - real PHP has no type for which `(bool)`
//! is illegal (every value has a truthiness), so `InvalidCastRule`'s own
//! `bool` arm can never actually fire and isn't worth a coercion method
//! of its own.

use crate::checker::{Checker, union_types};
use crate::token::Span;
use crate::types::{AccessoryKind, Type};

/// The handful of built-in extension classes real PHPStan's own
/// `ObjectType::toInteger()`/`toFloat()` special-case as numeric-
/// castable (`vendor/phpstan-src/src/Type/ObjectType.php`) - every other
/// object is illegal for `(int)`/`(float)`, even one with a real
/// `__toString()` (int/float coercion has no string-cast fallback the
/// way `(string)` itself does).
const INT_CASTABLE_CLASSES: &[&str] = &["SimpleXMLElement", "GMP", "CurlHandle", "CurlMultiHandle"];
const FLOAT_CASTABLE_CLASSES: &[&str] = &["SimpleXMLElement", "GMP"];

impl<'a> Checker<'a> {
    /// `(int) $expr` / an implicit int-coercion context. `None` = PHP
    /// itself cannot legally produce an int this way. `resource` (this
    /// project's own sentinel `Object("resource", [])`, not a real
    /// class - see `parse_type_keyword_or_class`'s own comment) is
    /// always legal here, matching real PHPStan's own dedicated
    /// `ResourceType::toInteger()` - a real, common idiom (`(int)
    /// $fileHandle` for a resource's own unique id), unlike an
    /// arbitrary object.
    pub(crate) fn type_to_integer(&self, ty: &Type) -> Option<Type> {
        match ty {
            Type::Object(name, _) if name == "resource" => Some(Type::Int),
            Type::Object(name, _) if self.is_castable_object(name, INT_CASTABLE_CLASSES) => {
                Some(Type::Int)
            }
            Type::Object(..) => None,
            Type::Union(members) => union_coercion(members, |m| self.type_to_integer(m)),
            _ => Some(Type::Int),
        }
    }

    /// `(float) $expr`. Same shape as `type_to_integer` (`resource`
    /// included, matching real PHPStan's own `ResourceType::toFloat()`),
    /// real PHPStan's own numeric-castable class list is one entry
    /// narrower for objects specifically (no `CurlHandle`/
    /// `CurlMultiHandle` - `curl_getinfo()`-style resource handles have
    /// no meaningful float value).
    pub(crate) fn type_to_float(&self, ty: &Type) -> Option<Type> {
        match ty {
            Type::Object(name, _) if name == "resource" => Some(Type::Float),
            Type::Object(name, _) if self.is_castable_object(name, FLOAT_CASTABLE_CLASSES) => {
                Some(Type::Float)
            }
            Type::Object(..) => None,
            Type::Union(members) => union_coercion(members, |m| self.type_to_float(m)),
            _ => Some(Type::Float),
        }
    }

    /// `(string) $expr`. `None` = illegal - an array (PHP's own "Array
    /// to string conversion" - always triggered, regardless of the
    /// array's own content, matching real PHPStan's `ArrayTypeTrait::
    /// toString()` unconditionally returning `ErrorType`) or an object
    /// with no `__toString()` method that also isn't `GMP`/
    /// `BcMath\Number` (both real, numeric-string-castable by a native
    /// operator overload, not a real `__toString()` declaration - real
    /// PHPStan special-cases them the same way). An *unresolvable*
    /// class (not in this project's own registry at all) is left alone,
    /// matching this project's usual "don't know, don't guess" leniency
    /// since a real corpus's own stub coverage may simply not include it.
    pub(crate) fn type_to_string(&self, ty: &Type) -> Option<Type> {
        match ty {
            Type::Array(..) | Type::ArrayShape(_) => None,
            Type::Intersection(members) if members.iter().any(|m| matches!(m, Type::Array(..))) => {
                None
            }
            // Matches real PHPStan's own dedicated `ResourceType::
            // toString()`, which is `ErrorType` unconditionally -
            // unlike `(int)`/`(float)` just above, a resource-to-string
            // cast is treated as illegal even though real PHP itself
            // produces a "Resource id #N" string with no warning.
            Type::Object(name, _) if name == "resource" => None,
            Type::Object(name, _) => {
                if self.registry.is_subclass_of(name, "GMP")
                    || self.registry.is_subclass_of(name, "BcMath\\Number")
                {
                    return Some(Type::Intersection(vec![
                        Type::String,
                        Type::Accessory(AccessoryKind::NumericString),
                        Type::Accessory(AccessoryKind::NonEmptyString),
                    ]));
                }
                match self.registry.lookup_method(name, "__toString") {
                    Some(sig) => Some(sig.return_type.clone()),
                    None if self.registry.classes.contains_key(name) => None,
                    None => Some(Type::String),
                }
            }
            Type::Union(members) => union_coercion(members, |m| self.type_to_string(m)),
            _ => Some(Type::String),
        }
    }

    fn is_castable_object(&self, name: &str, classes: &[&str]) -> bool {
        classes
            .iter()
            .any(|c| self.registry.is_subclass_of(name, c))
    }

    /// Real PHPStan's `InvalidCastRule` (`cast.int`/`cast.double`/
    /// `cast.string`) - `(bool)`/`(array)`/`(object)`/`(unset)` are never
    /// checked, matching real PHPStan's own callback (only the four
    /// scalar-coercion casts it names).
    pub(crate) fn check_cast_validity(
        &mut self,
        cast_ty: &str,
        inner_ty: &Type,
        span: Span,
        label: &str,
    ) {
        let (result, identifier, target_name) = match cast_ty {
            "int" => (self.type_to_integer(inner_ty), "cast.int", "int"),
            "float" => (self.type_to_float(inner_ty), "cast.double", "float"),
            "string" => (self.type_to_string(inner_ty), "cast.string", "string"),
            _ => return,
        };
        if result.is_some() {
            return;
        }
        self.report(
            label.to_string(),
            format!("Cannot cast {inner_ty} to {target_name}."),
            span,
            2,
            identifier,
        );
    }

    /// Real PHPStan's `EchoRule` (`echo.nonString`) - `position` is
    /// 0-based, real PHPStan's own message counts from 1.
    pub(crate) fn check_echo_validity(
        &mut self,
        expr_ty: &Type,
        position: usize,
        span: Span,
        label: &str,
    ) {
        if self.type_to_string(expr_ty).is_some() {
            return;
        }
        self.report(
            label.to_string(),
            format!(
                "Parameter #{} ({expr_ty}) of echo cannot be converted to string.",
                position + 1
            ),
            span,
            2,
            "echo.nonString",
        );
    }

    /// Real PHPStan's `PrintRule` (`print.nonString`) - unlike
    /// `check_echo_validity`, `print` only ever has one operand, so real
    /// PHPStan's own message has no `#%d` position number at all
    /// (confirmed directly against `vendor/phpstan-src`'s own
    /// `PrintRule::processNode`, which interpolates the type description
    /// straight into `"Parameter %s of print..."` with no separate
    /// position argument - not a typo to "fix" into echo's shape).
    pub(crate) fn check_print_validity(&mut self, expr_ty: &Type, span: Span, label: &str) {
        if self.type_to_string(expr_ty).is_some() {
            return;
        }
        self.report(
            label.to_string(),
            format!("Parameter {expr_ty} of print cannot be converted to string."),
            span,
            2,
            "print.nonString",
        );
    }
}

/// `abs($x)` - mirrors real PHPStan's own `IntegerRangeType::
/// toAbsoluteNumber()` exactly (including its own `PHP_INT_MIN`
/// overflow special case: negating it would overflow, so its own
/// absolute value is treated as unbounded, the same as an already-
/// unbounded lower bound). Falls back to the stub's own generic
/// `int|float` return type for anything this can't narrow (a plain,
/// unbounded `Int`/`Float`, or any non-numeric type - the latter is
/// already an argument-type error the ordinary call-argument check
/// catches, not something this function needs to gate separately).
pub(crate) fn to_absolute_number(ty: &Type) -> Type {
    match ty {
        Type::IntRange(min, max) => {
            if min.is_some_and(|m| m >= 0) {
                return ty.clone();
            }
            let inversed_min = min.filter(|&m| m != i64::MIN).map(|m| -m);
            if max.is_none_or(|m| m >= 0) {
                let upper = match (inversed_min, *max) {
                    (Some(im), Some(mx)) => Some(im.max(mx)),
                    _ => None,
                };
                Type::IntRange(Some(0), upper)
            } else {
                Type::IntRange(Some(-max.expect("max < 0 is always Some")), inversed_min)
            }
        }
        Type::IntLiteral(n) => Type::IntLiteral(n.saturating_abs()),
        Type::FloatLiteral(f) => Type::FloatLiteral(f.abs()),
        Type::Union(members) => members
            .iter()
            .map(to_absolute_number)
            .reduce(union_types)
            .unwrap_or_else(|| ty.clone()),
        Type::Int | Type::Float => ty.clone(),
        _ => Type::Union(vec![Type::Int, Type::Float]),
    }
}

/// `~$x` (`UnaryOp::BitNot`) on an int-range-typed operand. Mirrors real
/// PHPStan's own `IntegerRangeType::toBitwiseNotType()`: bitwise NOT
/// reverses ordering (`~min` is the new max, `~max` is the new min),
/// since negating a larger magnitude number produces a smaller (more
/// negative) two's-complement result.
pub(crate) fn to_bitwise_not(ty: &Type) -> Option<Type> {
    match ty {
        Type::IntRange(min, max) => Some(Type::IntRange(max.map(|m| !m), min.map(|m| !m))),
        Type::IntLiteral(n) => Some(Type::IntLiteral(!n)),
        _ => None,
    }
}

/// A union's own cast validity: real PHPStan's `UnionType::toInteger()`/
/// etc. combine each member's own result via `TypeCombinator`, and
/// `InvalidCastRule` only flags when the *combined* result is exactly
/// `ErrorType` - which only happens when every single member's own cast
/// was illegal (a union with even one legal member produces a real,
/// mixed-but-not-purely-`ErrorType` result). Mirrors Phase 50's own
/// `invalid_array_key_description` union handling for the identical
/// reason: no tri-state "possibly invalid" grading to fall back on, so
/// "confidently illegal" is the only case worth flagging.
fn union_coercion(members: &[Type], f: impl Fn(&Type) -> Option<Type>) -> Option<Type> {
    let legal: Vec<Type> = members.iter().filter_map(f).collect();
    legal.into_iter().reduce(union_types)
}
