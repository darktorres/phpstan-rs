//! PHPStan's `Type::toInteger()`/`toFloat()`/`toString()`/
//! `toAbsoluteNumber()`/`toBitwiseNotType()`: what a cast or an implicit
//! coercion of a value of a type produces, `None` where PHP can't
//! legally make one. Every scalar, `null`, `array` (for int/float only)
//! and `mixed` coerce; the only real illegality lives on the object side
//! (PHPStan's `ObjectType::toInteger()`/`toFloat()`/`toString()`) and in
//! `ArrayTypeTrait`'s always-`ErrorType` `toString()`.

use crate::UnionAll as _;
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
        syntax::stack::guard(|| {
            match self.repr() {
                Repr::Int | Repr::IntLiteral(_) | Repr::IntRange(..) => Some(self.clone()),
                Repr::True => Some(Type::int_literal(1)),
                Repr::False | Repr::Null => Some(Type::int_literal(0)),
                Repr::Bool => Some(Type::int_range(Some(0), Some(1))),
                Repr::FloatLiteral(f) => Some(Type::int_literal(php_float_to_int(*f))),
                Repr::Resource { .. } => Some(Type::int()),
                Repr::Object(name, _) if castable(hierarchy, name, INT_CASTABLE_CLASSES) => {
                    Some(Type::int())
                }
                Repr::Object(..)
                | Repr::ObjectWithoutClass
                | Repr::ObjectShape(_)
                | Repr::EnumCase(..) => None,
                // An intersection must not fall to the permissive catch-all:
                // `(int)` on `Countable&ArrayAccess` is illegal. If any
                // member offers a legal cast path, the value supports it.
                Repr::Union(members) | Repr::Intersection(members) => {
                    union_coercion(members, |m| m.to_integer(hierarchy))
                }
                _ => Some(Type::int()),
            }
        })
    }

    /// `(float) $x`: a float as it is, an int, bool or `null` literal as its
    /// value; otherwise as `to_integer`, with PHPStan's one entry narrower
    /// numeric-castable class list (no `CurlHandle`/`CurlMultiHandle`).
    pub fn to_float(&self, hierarchy: &dyn ClassHierarchy) -> Option<Type> {
        syntax::stack::guard(|| match self.repr() {
            Repr::Float | Repr::FloatLiteral(_) => Some(self.clone()),
            Repr::IntLiteral(n) => Some(Type::float_literal(*n as f64)),
            Repr::True => Some(Type::float_literal(1.0)),
            Repr::False | Repr::Null => Some(Type::float_literal(0.0)),
            Repr::Resource { .. } => Some(Type::float()),
            Repr::Object(name, _) if castable(hierarchy, name, FLOAT_CASTABLE_CLASSES) => {
                Some(Type::float())
            }
            Repr::Object(..)
            | Repr::ObjectWithoutClass
            | Repr::ObjectShape(_)
            | Repr::EnumCase(..) => None,
            Repr::Union(members) | Repr::Intersection(members) => {
                union_coercion(members, |m| m.to_float(hierarchy))
            }
            _ => Some(Type::float()),
        })
    }

    /// PHPStan's `toNumber()`: what arithmetic reads a value of this type
    /// as - an int or float as it is, a bool or `null` as `0`/`1`,
    /// anything else that coerces as `int|float`. `None` for an array, an
    /// object `to_float` refuses, or - when `non_numeric_throws` (PHP 8's
    /// `NonNumericStringArithmeticThrows`) - a string literal that isn't
    /// numeric (`'abc'`, `'5 apples'`).
    pub fn to_number(
        &self,
        hierarchy: &dyn ClassHierarchy,
        non_numeric_throws: bool,
    ) -> Option<Type> {
        syntax::stack::guard(|| {
            if self.is_array().yes() {
                return None;
            }
            match self.repr() {
                Repr::Int
                | Repr::IntLiteral(_)
                | Repr::IntRange(..)
                | Repr::Float
                | Repr::FloatLiteral(_) => Some(self.clone()),
                Repr::True | Repr::False | Repr::Null | Repr::Bool => self.to_integer(hierarchy),
                Repr::StringLiteral(s)
                    if non_numeric_throws
                        && !s
                            .as_str()
                            .is_some_and(crate::subtype::is_numeric_string_literal) =>
                {
                    None
                }
                Repr::Union(members) | Repr::Intersection(members) => {
                    union_coercion(members, |m| m.to_number(hierarchy, non_numeric_throws))
                }
                _ => self
                    .to_float(hierarchy)
                    .map(|_| Type::union(Type::int(), Type::float())),
            }
        })
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
        match hierarchy.generation() {
            Some(generation) => crate::relation::memo::string_type(generation, self, || {
                self.to_string_type_uncached(hierarchy)
            }),
            None => self.to_string_type_uncached(hierarchy),
        }
    }

    fn to_string_type_uncached(&self, hierarchy: &dyn ClassHierarchy) -> Option<Type> {
        syntax::stack::guard(|| {
            let decimal_int_string = || {
                Type::new(Repr::Intersection(vec![
                    Type::string(),
                    Type::accessory(AccessoryKind::DecimalIntegerString),
                ]))
            };
            if !matches!(self.repr(), Repr::Mixed(..) | Repr::Never)
                && crate::Relation::structural()
                    .is_super_type_of(&Type::string(), self)
                    .yes()
            {
                return Some(self.clone());
            }
            match self.repr() {
                Repr::IntLiteral(n) => Some(Type::string_literal(n.to_string().into())),
                Repr::Int | Repr::IntRange(..) => Some(decimal_int_string()),
                Repr::FloatLiteral(f) => Some(Type::string_literal(format_php_float(*f).into())),
                // `INF`, `-INF` and `NAN` aren't numeric.
                Repr::Float => Some(Type::new(Repr::Intersection(vec![
                    Type::string(),
                    Type::accessory(AccessoryKind::UppercaseString),
                    Type::accessory(AccessoryKind::NonEmptyString),
                ]))),
                Repr::True => Some(Type::string_literal("1".into())),
                Repr::False | Repr::Null => Some(Type::string_literal(PhpString::default())),
                Repr::Bool => Some(Type::new(Repr::Union(vec![
                    Type::string_literal("1".into()),
                    Type::string_literal(PhpString::default()),
                ]))),
                Repr::Array(..) | Repr::ArrayShape(_) => None,
                Repr::Intersection(members)
                    if members.iter().any(|m| matches!(m.repr(), Repr::Array(..))) =>
                {
                    None
                }
                Repr::Resource { .. } => Some(Type::new(Repr::Intersection(vec![
                    Type::string(),
                    Type::accessory(AccessoryKind::NonEmptyString),
                ]))),
                Repr::Object(name, _) => {
                    if hierarchy.is_subclass_of(name, "GMP")
                        || hierarchy.is_subclass_of(name, "BcMath\\Number")
                    {
                        return Some(Type::new(Repr::Intersection(vec![
                            Type::string(),
                            Type::accessory(AccessoryKind::NumericString),
                            Type::accessory(AccessoryKind::NonEmptyString),
                        ])));
                    }
                    match hierarchy.method_return_type(name, "__toString") {
                        Some(ty) => Some(ty),
                        None if hierarchy.find_class(name).is_some() => None,
                        None => Some(Type::string()),
                    }
                }
                // An enum case can't declare `__toString()`.
                Repr::EnumCase(..) => None,
                Repr::Union(members) | Repr::Intersection(members) => {
                    union_coercion(members, |m| m.to_string_type(hierarchy))
                }
                _ => Some(Type::string()),
            }
        })
    }

    /// The key a value becomes where PHP converts it to a string first
    /// (`array_fill_keys`, `array_combine`): `1.7` gives `'1.7'`, `true`
    /// gives `1`, `null` gives `''`.
    pub fn to_string_array_key(&self, hierarchy: &dyn ClassHierarchy) -> Type {
        self.to_string_type(hierarchy)
            .unwrap_or_else(Type::string)
            .to_array_key()
    }

    /// PHPStan's `toArrayKey()`: the key a value of this type becomes in
    /// an array - a float truncated to an int, `true`/`false` as `1`/`0`,
    /// `null` as `''`, a canonical integer string as that int. A string
    /// not known to be one may still be, so it is an int or that string
    /// (benevolently); a class name never is. Any other key type is kept.
    pub fn to_array_key(&self) -> Type {
        syntax::stack::guard(|| match self.repr() {
            Repr::FloatLiteral(f) => Type::int_literal(php_float_to_int(*f)),
            Repr::Float => Type::int(),
            Repr::True => Type::int_literal(1),
            Repr::False => Type::int_literal(0),
            Repr::Bool => Type::new(Repr::Union(vec![
                Type::int_literal(0),
                Type::int_literal(1),
            ])),
            Repr::Null => Type::string_literal(PhpString::default()),
            Repr::StringLiteral(s) => match crate::canonical_int_key(&s.to_str_lossy()) {
                Some(n) => Type::int_literal(n),
                None => self.clone(),
            },
            Repr::Union(members) => members
                .iter()
                .map(Type::to_array_key)
                .union_all()
                .unwrap_or(Type::never()),
            Repr::BenevolentUnion(members) => {
                Type::benevolent_union_of(members.iter().map(Type::to_array_key).collect())
            }
            Repr::ClassString(_) => self.clone(),
            _ if self.is_string().yes() => {
                if self
                    .is_refined_string(AccessoryKind::DecimalIntegerString)
                    .yes()
                {
                    Type::int()
                } else if self
                    .is_refined_string(AccessoryKind::NonDecimalIntegerString)
                    .yes()
                {
                    crate::strip_key_refinement(self)
                } else {
                    Type::benevolent_union_of(vec![Type::int(), self.clone()])
                }
            }
            _ => self.clone(),
        })
    }

    /// `abs($x)`: PHPStan's `IntegerRangeType::toAbsoluteNumber()`,
    /// including its `PHP_INT_MIN` special case (negating it would
    /// overflow, so its absolute value is unbounded, as an unbounded
    /// lower bound's is). Anything it can't narrow is `int|float`.
    pub fn to_absolute_number(&self) -> Type {
        match self.repr() {
            Repr::IntRange(min, max) if *min == Some(i64::MIN) => Type::union(
                Type::int_range(Some(i64::MIN + 1), *max).to_absolute_number(),
                Type::float_literal(-(i64::MIN as f64)),
            ),
            Repr::IntRange(min, max) => {
                if min.is_some_and(|m| m >= 0) {
                    return self.clone();
                }
                let inversed_min = min.filter(|&m| m != i64::MIN).map(|m| -m);
                if max.is_none_or(|m| m >= 0) {
                    let upper = match (inversed_min, *max) {
                        (Some(im), Some(mx)) => Some(im.max(mx)),
                        _ => None,
                    };
                    Type::int_range(Some(0), upper)
                } else {
                    let max = max.expect("max < 0 is always Some");
                    Type::int_range(Some(max.checked_neg().unwrap_or(i64::MAX)), inversed_min)
                }
            }
            // `abs(PHP_INT_MIN)` is past `PHP_INT_MAX`: a float.
            Repr::IntLiteral(n) => match n.checked_abs() {
                Some(v) => Type::int_literal(v),
                None => Type::float_literal(-(*n as f64)),
            },
            Repr::FloatLiteral(f) => Type::float_literal(f.abs()),
            Repr::Union(members) => members
                .iter()
                .map(Type::to_absolute_number)
                .union_all()
                .unwrap_or_else(|| self.clone()),
            Repr::Int | Repr::Float => self.clone(),
            _ => Type::new(Repr::Union(vec![Type::int(), Type::float()])),
        }
    }

    /// `~$x`: an int's, or a float's truncated, complement as an int, and
    /// a string's byte-wise complement as a string of the same length (so
    /// a non-empty one stays non-empty). Any other operand throws.
    pub fn to_bitwise_not(&self) -> Type {
        syntax::stack::guard(|| match self.repr() {
            Repr::IntRange(min, max) => Type::int_range(max.map(|m| !m), min.map(|m| !m)),
            Repr::IntLiteral(n) => Type::int_literal(!n),
            Repr::Int | Repr::Float => Type::int(),
            Repr::FloatLiteral(f) => Type::int_literal(!php_float_to_int(*f)),
            Repr::StringLiteral(s) => Type::string_literal(PhpString::from(
                s.as_bytes().iter().map(|b| !b).collect::<Vec<u8>>(),
            )),
            Repr::Union(members) | Repr::BenevolentUnion(members) => members
                .iter()
                .map(Type::to_bitwise_not)
                .union_all()
                .unwrap_or_else(Type::never),
            Repr::TemplateParam(param) => param.bound.to_bitwise_not(),
            Repr::Mixed(..) => Type::union(Type::int(), Type::string()),
            _ if self.is_string().yes() => {
                if self.is_refined_string(AccessoryKind::NonEmptyString).yes() {
                    Type::string().with_string_accessory(AccessoryKind::NonEmptyString)
                } else {
                    Type::string()
                }
            }
            _ => Type::error(),
        })
    }
}

/// `(int) $f` as PHP computes it: truncated toward zero, wrapped modulo
/// 2^64 when out of range, and 0 for NaN and the infinities.
pub(crate) fn php_float_to_int(f: f64) -> i64 {
    const TWO_POW_64: f64 = 18_446_744_073_709_551_616.0;
    if !f.is_finite() {
        return 0;
    }
    let t = f.trunc();
    if t >= i64::MIN as f64 && t < -(i64::MIN as f64) {
        t as i64
    } else {
        t.rem_euclid(TWO_POW_64) as u64 as i64
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
    legal.into_iter().union_all()
}
