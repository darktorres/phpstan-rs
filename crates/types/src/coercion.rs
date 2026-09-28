//! PHPStan's `Type::toInteger()`/`toFloat()`/`toString()`/
//! `toAbsoluteNumber()`/`toBitwiseNotType()`: what a cast or an implicit
//! coercion of a value of a type produces, `None` where PHP can't
//! legally make one. Every scalar, `null`, `array` (for int/float only)
//! and `mixed` coerce; the only real illegality lives on the object side
//! (PHPStan's `ObjectType::toInteger()`/`toFloat()`/`toString()`) and in
//! `ArrayTypeTrait`'s always-`ErrorType` `toString()`.

use crate::{AccessoryKind, ClassHierarchy, PhpString, Repr, Type, format_php_float};

/// The built-in classes PHPStan's `ObjectType::toInteger()`/`toFloat()`
/// special-case as numeric-castable; every other object is illegal for
/// `(int)`/`(float)`, even one with a real `__toString()`.
const INT_CASTABLE_CLASSES: &[&str] = &["SimpleXMLElement", "GMP", "CurlHandle", "CurlMultiHandle"];
const FLOAT_CASTABLE_CLASSES: &[&str] = &["SimpleXMLElement", "GMP"];

impl Type {
    /// `(int) $x`: an int as it is, a bool or `null` as `1`/`0`, a finite
    /// float literal truncated. A resource is legal (`(int) $fileHandle`
    /// is its id), an arbitrary object isn't.
    pub fn to_integer(&self, hierarchy: &dyn ClassHierarchy) -> Option<Type> {
        match self {
            Type(Repr::Int) | Type(Repr::IntLiteral(_)) | Type(Repr::IntRange(..)) => {
                Some(self.clone())
            }
            Type(Repr::True) => Some(Type(Repr::IntLiteral(1))),
            Type(Repr::False) | Type(Repr::Null) => Some(Type(Repr::IntLiteral(0))),
            Type(Repr::Bool) => Some(Type(Repr::IntRange(Some(0), Some(1)))),
            Type(Repr::FloatLiteral(f)) if f.is_finite() && f.trunc().abs() < i64::MAX as f64 => {
                Some(Type(Repr::IntLiteral(f.trunc() as i64)))
            }
            Type(Repr::Resource { .. }) => Some(Type(Repr::Int)),
            Type(Repr::Object(name, _)) if castable(hierarchy, name, INT_CASTABLE_CLASSES) => {
                Some(Type(Repr::Int))
            }
            Type(Repr::Object(..))
            | Type(Repr::ObjectWithoutClass)
            | Type(Repr::ObjectShape(_))
            | Type(Repr::EnumCase(..)) => None,
            // An intersection must not fall to the permissive catch-all:
            // `(int)` on `Countable&ArrayAccess` is illegal. If any
            // member offers a legal cast path, the value supports it.
            Type(Repr::Union(members)) | Type(Repr::Intersection(members)) => {
                union_coercion(members, |m| m.to_integer(hierarchy))
            }
            _ => Some(Type(Repr::Int)),
        }
    }

    /// `(float) $x`: a float as it is, an int, bool or `null` literal as its
    /// value; otherwise as `to_integer`, with PHPStan's one entry narrower
    /// numeric-castable class list (no `CurlHandle`/`CurlMultiHandle`).
    pub fn to_float(&self, hierarchy: &dyn ClassHierarchy) -> Option<Type> {
        match self {
            Type(Repr::Float) | Type(Repr::FloatLiteral(_)) => Some(self.clone()),
            Type(Repr::IntLiteral(n)) => Some(Type(Repr::FloatLiteral(*n as f64))),
            Type(Repr::True) => Some(Type(Repr::FloatLiteral(1.0))),
            Type(Repr::False) | Type(Repr::Null) => Some(Type(Repr::FloatLiteral(0.0))),
            Type(Repr::Resource { .. }) => Some(Type(Repr::Float)),
            Type(Repr::Object(name, _)) if castable(hierarchy, name, FLOAT_CASTABLE_CLASSES) => {
                Some(Type(Repr::Float))
            }
            Type(Repr::Object(..))
            | Type(Repr::ObjectWithoutClass)
            | Type(Repr::ObjectShape(_))
            | Type(Repr::EnumCase(..)) => None,
            Type(Repr::Union(members)) | Type(Repr::Intersection(members)) => {
                union_coercion(members, |m| m.to_float(hierarchy))
            }
            _ => Some(Type(Repr::Float)),
        }
    }

    /// PHPStan's `toNumber()`: what arithmetic reads a value of this type
    /// as - an int or float as it is, a bool or `null` as `0`/`1`,
    /// anything else that coerces as `int|float`. `None` for an array, or
    /// an object `to_float` refuses.
    pub fn to_number(&self, hierarchy: &dyn ClassHierarchy) -> Option<Type> {
        if self.is_array().yes() {
            return None;
        }
        match self {
            Type(Repr::Int)
            | Type(Repr::IntLiteral(_))
            | Type(Repr::IntRange(..))
            | Type(Repr::Float)
            | Type(Repr::FloatLiteral(_)) => Some(self.clone()),
            Type(Repr::True) | Type(Repr::False) | Type(Repr::Null) | Type(Repr::Bool) => {
                self.to_integer(hierarchy)
            }
            Type(Repr::Union(members)) | Type(Repr::Intersection(members)) => {
                union_coercion(members, |m| m.to_number(hierarchy))
            }
            _ => self
                .to_float(hierarchy)
                .map(|_| Type::union(Type(Repr::Int), Type(Repr::Float))),
        }
    }

    /// `(string) $x`: a string as it is, an int as its decimal digits, a bool or
    /// `null` as `'1'`/`''`, a float as non-empty text (numeric when
    /// finite, `INF`/`NAN` otherwise), a resource as its `Resource id #N`
    /// text. Illegal for an array (PHP's "Array to string conversion",
    /// whatever it holds) and an object with no `__toString()` that also
    /// isn't `GMP`/`BcMath\Number` (numeric-string-castable by an operator
    /// overload). A class the hierarchy doesn't know is left alone: stub
    /// coverage may simply not include it.
    pub fn to_string_type(&self, hierarchy: &dyn ClassHierarchy) -> Option<Type> {
        let decimal_int_string = || {
            Type(Repr::Intersection(vec![
                Type(Repr::String),
                Type(Repr::Accessory(AccessoryKind::DecimalIntegerString)),
            ]))
        };
        if !matches!(self, Type(Repr::Mixed(_)) | Type(Repr::Never))
            && crate::Relation::structural()
                .is_super_type_of(&Type(Repr::String), self)
                .yes()
        {
            return Some(self.clone());
        }
        match self {
            Type(Repr::IntLiteral(n)) => Some(Type(Repr::StringLiteral(n.to_string().into()))),
            Type(Repr::Int) | Type(Repr::IntRange(..)) => Some(decimal_int_string()),
            Type(Repr::FloatLiteral(f)) => {
                Some(Type(Repr::StringLiteral(format_php_float(*f).into())))
            }
            // `INF`, `-INF` and `NAN` aren't numeric.
            Type(Repr::Float) => Some(Type(Repr::Intersection(vec![
                Type(Repr::String),
                Type(Repr::Accessory(AccessoryKind::UppercaseString)),
                Type(Repr::Accessory(AccessoryKind::NonEmptyString)),
            ]))),
            Type(Repr::True) => Some(Type(Repr::StringLiteral("1".into()))),
            Type(Repr::False) | Type(Repr::Null) => {
                Some(Type(Repr::StringLiteral(PhpString::default())))
            }
            Type(Repr::Bool) => Some(Type(Repr::Union(vec![
                Type(Repr::StringLiteral("1".into())),
                Type(Repr::StringLiteral(PhpString::default())),
            ]))),
            Type(Repr::Array(..)) | Type(Repr::ArrayShape(_)) => None,
            Type(Repr::Intersection(members))
                if members.iter().any(|m| matches!(m, Type(Repr::Array(..)))) =>
            {
                None
            }
            Type(Repr::Resource { .. }) => Some(Type(Repr::Intersection(vec![
                Type(Repr::String),
                Type(Repr::Accessory(AccessoryKind::NonEmptyString)),
            ]))),
            Type(Repr::Object(name, _)) => {
                if hierarchy.is_subclass_of(name, "GMP")
                    || hierarchy.is_subclass_of(name, "BcMath\\Number")
                {
                    return Some(Type(Repr::Intersection(vec![
                        Type(Repr::String),
                        Type(Repr::Accessory(AccessoryKind::NumericString)),
                        Type(Repr::Accessory(AccessoryKind::NonEmptyString)),
                    ])));
                }
                match hierarchy.method_return_type(name, "__toString") {
                    Some(ty) => Some(ty),
                    None if hierarchy.find_class(name).is_some() => None,
                    None => Some(Type(Repr::String)),
                }
            }
            // An enum case can't declare `__toString()`.
            Type(Repr::EnumCase(..)) => None,
            Type(Repr::Union(members)) | Type(Repr::Intersection(members)) => {
                union_coercion(members, |m| m.to_string_type(hierarchy))
            }
            _ => Some(Type(Repr::String)),
        }
    }

    /// PHPStan's `toArrayKey()`: the key a value of this type becomes in
    /// an array - a float truncated to an int, `true`/`false` as `1`/`0`,
    /// `null` as `''`, a canonical integer string as that int; any other
    /// key type as it is.
    pub fn to_array_key(&self) -> Type {
        match self {
            Type(Repr::FloatLiteral(f)) => Type(Repr::IntLiteral(*f as i64)),
            Type(Repr::Float) => Type(Repr::Int),
            Type(Repr::True) => Type(Repr::IntLiteral(1)),
            Type(Repr::False) => Type(Repr::IntLiteral(0)),
            Type(Repr::Bool) => Type(Repr::Union(vec![
                Type(Repr::IntLiteral(0)),
                Type(Repr::IntLiteral(1)),
            ])),
            Type(Repr::Null) => Type(Repr::StringLiteral(PhpString::default())),
            Type(Repr::StringLiteral(s)) => match crate::canonical_int_key(&s.to_str_lossy()) {
                Some(n) => Type(Repr::IntLiteral(n)),
                None => self.clone(),
            },
            Type(Repr::Union(members)) => members
                .iter()
                .map(Type::to_array_key)
                .reduce(Type::union)
                .unwrap_or(Type(Repr::Never)),
            other => other.clone(),
        }
    }

    /// `abs($x)`: PHPStan's `IntegerRangeType::toAbsoluteNumber()`,
    /// including its `PHP_INT_MIN` special case (negating it would
    /// overflow, so its absolute value is unbounded, as an unbounded
    /// lower bound's is). Anything it can't narrow is `int|float`.
    pub fn to_absolute_number(&self) -> Type {
        match self {
            Type(Repr::IntRange(min, max)) => {
                if min.is_some_and(|m| m >= 0) {
                    return self.clone();
                }
                let inversed_min = min.filter(|&m| m != i64::MIN).map(|m| -m);
                if max.is_none_or(|m| m >= 0) {
                    let upper = match (inversed_min, *max) {
                        (Some(im), Some(mx)) => Some(im.max(mx)),
                        _ => None,
                    };
                    Type(Repr::IntRange(Some(0), upper))
                } else {
                    Type(Repr::IntRange(
                        Some(-max.expect("max < 0 is always Some")),
                        inversed_min,
                    ))
                }
            }
            Type(Repr::IntLiteral(n)) => Type(Repr::IntLiteral(n.saturating_abs())),
            Type(Repr::FloatLiteral(f)) => Type(Repr::FloatLiteral(f.abs())),
            Type(Repr::Union(members)) => members
                .iter()
                .map(Type::to_absolute_number)
                .reduce(Type::union)
                .unwrap_or_else(|| self.clone()),
            Type(Repr::Int) | Type(Repr::Float) => self.clone(),
            _ => Type(Repr::Union(vec![Type(Repr::Int), Type(Repr::Float)])),
        }
    }

    /// `~$x` on an int: PHPStan's `IntegerRangeType::toBitwiseNotType()`.
    /// Bitwise NOT reverses ordering (`~min` is the new max).
    pub fn to_bitwise_not(&self) -> Option<Type> {
        match self {
            Type(Repr::IntRange(min, max)) => {
                Some(Type(Repr::IntRange(max.map(|m| !m), min.map(|m| !m))))
            }
            Type(Repr::IntLiteral(n)) => Some(Type(Repr::IntLiteral(!n))),
            _ => None,
        }
    }
}

fn castable(hierarchy: &dyn ClassHierarchy, name: &str, classes: &[&str]) -> bool {
    classes.iter().any(|c| hierarchy.is_subclass_of(name, c))
}

/// A union's coercion: PHPStan's `UnionType::toInteger()`/etc. combine
/// each member's result, and only a result that is purely `ErrorType` -
/// every member illegal - is illegal.
fn union_coercion(members: &[Type], f: impl Fn(&Type) -> Option<Type>) -> Option<Type> {
    let legal: Vec<Type> = members.iter().filter_map(f).collect();
    legal.into_iter().reduce(Type::union)
}
