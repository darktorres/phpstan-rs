//! PHPStan's `Type::toInteger()`/`toFloat()`/`toString()`/
//! `toAbsoluteNumber()`/`toBitwiseNotType()`: what a cast or an implicit
//! coercion of a value of a type produces, `None` where PHP can't
//! legally make one. Every scalar, `null`, `array` (for int/float only)
//! and `mixed` coerce; the only real illegality lives on the object side
//! (PHPStan's `ObjectType::toInteger()`/`toFloat()`/`toString()`) and in
//! `ArrayTypeTrait`'s always-`ErrorType` `toString()`.

use crate::{AccessoryKind, ClassHierarchy, Type, union_types};

/// The built-in classes PHPStan's `ObjectType::toInteger()`/`toFloat()`
/// special-case as numeric-castable; every other object is illegal for
/// `(int)`/`(float)`, even one with a real `__toString()`.
const INT_CASTABLE_CLASSES: &[&str] = &["SimpleXMLElement", "GMP", "CurlHandle", "CurlMultiHandle"];
const FLOAT_CASTABLE_CLASSES: &[&str] = &["SimpleXMLElement", "GMP"];

impl Type {
    /// `(int) $x`. A resource is legal (`(int) $fileHandle` is its id,
    /// PHPStan's `ResourceType::toInteger()`), an arbitrary object isn't.
    pub fn to_integer(&self, hierarchy: &dyn ClassHierarchy) -> Option<Type> {
        match self {
            Type::Resource { .. } => Some(Type::Int),
            Type::Object(name, _) if castable(hierarchy, name, INT_CASTABLE_CLASSES) => {
                Some(Type::Int)
            }
            Type::Object(..) | Type::ObjectWithoutClass => None,
            // An intersection must not fall to the permissive catch-all:
            // `(int)` on `Countable&ArrayAccess` is illegal. If any
            // member offers a legal cast path, the value supports it.
            Type::Union(members) | Type::Intersection(members) => {
                union_coercion(members, |m| m.to_integer(hierarchy))
            }
            _ => Some(Type::Int),
        }
    }

    /// `(float) $x`: as `to_integer`, with PHPStan's one entry narrower
    /// numeric-castable class list (no `CurlHandle`/`CurlMultiHandle`).
    pub fn to_float(&self, hierarchy: &dyn ClassHierarchy) -> Option<Type> {
        match self {
            Type::Resource { .. } => Some(Type::Float),
            Type::Object(name, _) if castable(hierarchy, name, FLOAT_CASTABLE_CLASSES) => {
                Some(Type::Float)
            }
            Type::Object(..) | Type::ObjectWithoutClass => None,
            Type::Union(members) | Type::Intersection(members) => {
                union_coercion(members, |m| m.to_float(hierarchy))
            }
            _ => Some(Type::Float),
        }
    }

    /// `(string) $x`. Illegal for an array (PHP's "Array to string
    /// conversion", whatever it holds), a resource (PHPStan's
    /// `ResourceType::toString()`), and an object with no `__toString()`
    /// that also isn't `GMP`/`BcMath\Number` (numeric-string-castable by
    /// an operator overload). A class the hierarchy doesn't know is left
    /// alone: stub coverage may simply not include it.
    pub fn to_string_type(&self, hierarchy: &dyn ClassHierarchy) -> Option<Type> {
        match self {
            Type::Array(..) | Type::ArrayShape(_) => None,
            Type::Intersection(members) if members.iter().any(|m| matches!(m, Type::Array(..))) => {
                None
            }
            Type::Resource { .. } => None,
            Type::Object(name, _) => {
                if hierarchy.is_subclass_of(name, "GMP")
                    || hierarchy.is_subclass_of(name, "BcMath\\Number")
                {
                    return Some(Type::Intersection(vec![
                        Type::String,
                        Type::Accessory(AccessoryKind::NumericString),
                        Type::Accessory(AccessoryKind::NonEmptyString),
                    ]));
                }
                match hierarchy.method_return_type(name, "__toString") {
                    Some(ty) => Some(ty),
                    None if hierarchy.class_exactly(name).is_some() => None,
                    None => Some(Type::String),
                }
            }
            Type::Union(members) | Type::Intersection(members) => {
                union_coercion(members, |m| m.to_string_type(hierarchy))
            }
            _ => Some(Type::String),
        }
    }

    /// `abs($x)`: PHPStan's `IntegerRangeType::toAbsoluteNumber()`,
    /// including its `PHP_INT_MIN` special case (negating it would
    /// overflow, so its absolute value is unbounded, as an unbounded
    /// lower bound's is). Anything it can't narrow is `int|float`.
    pub fn to_absolute_number(&self) -> Type {
        match self {
            Type::IntRange(min, max) => {
                if min.is_some_and(|m| m >= 0) {
                    return self.clone();
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
                .map(Type::to_absolute_number)
                .reduce(union_types)
                .unwrap_or_else(|| self.clone()),
            Type::Int | Type::Float => self.clone(),
            _ => Type::Union(vec![Type::Int, Type::Float]),
        }
    }

    /// `~$x` on an int: PHPStan's `IntegerRangeType::toBitwiseNotType()`.
    /// Bitwise NOT reverses ordering (`~min` is the new max).
    pub fn to_bitwise_not(&self) -> Option<Type> {
        match self {
            Type::IntRange(min, max) => Some(Type::IntRange(max.map(|m| !m), min.map(|m| !m))),
            Type::IntLiteral(n) => Some(Type::IntLiteral(!n)),
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
    legal.into_iter().reduce(union_types)
}
