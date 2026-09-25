//! Array and object shapes: `array{name: string, 0: int, ...}`,
//! `object{name: string}`.

use std::fmt;

use crate::types::{Type, parse::canonical_int_key};

/// An array shape key, canonicalized the way PHP canonicalizes an array
/// key: a decimal integer string is an int key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ShapeKey {
    Int(i64),
    Str(String),
}

impl ShapeKey {
    /// The key `name` is as a PHP array key.
    pub fn from_name(name: &str) -> ShapeKey {
        match canonical_int_key(name) {
            Some(n) => ShapeKey::Int(n),
            None => ShapeKey::Str(name.to_string()),
        }
    }

    /// The key as its literal type (`'name'`, `0`).
    pub fn to_type(&self) -> Type {
        match self {
            ShapeKey::Int(n) => Type::IntLiteral(*n),
            ShapeKey::Str(s) => Type::StringLiteral(s.clone()),
        }
    }

    /// The key a literal key type names, if it names one.
    pub fn from_type(ty: &Type) -> Option<ShapeKey> {
        match ty {
            Type::IntLiteral(n) => Some(ShapeKey::Int(*n)),
            Type::StringLiteral(s) => Some(ShapeKey::from_name(s)),
            _ => None,
        }
    }

    /// Whether this key is the string `name` (or the int it spells).
    pub fn is(&self, name: &str) -> bool {
        *self == ShapeKey::from_name(name)
    }
}

/// The key's own text: the digits of an int key, a string key verbatim.
impl fmt::Display for ShapeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShapeKey::Int(n) => write!(f, "{n}"),
            ShapeKey::Str(s) => write!(f, "{s}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapeField {
    pub key: ShapeKey,
    pub ty: Type,
    pub optional: bool,
}

impl ShapeField {
    pub fn new(key: ShapeKey, ty: Type, optional: bool) -> ShapeField {
        ShapeField { key, ty, optional }
    }
}

/// A shape's fields, in order, and - when the shape is unsealed
/// (`array{a: int, ...}`: other keys may exist too) - the key and value
/// types those other entries have.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shape {
    pub fields: Vec<ShapeField>,
    pub unsealed: Option<Box<(Type, Type)>>,
}

impl Shape {
    /// A sealed shape of exactly `fields`.
    pub fn sealed(fields: Vec<ShapeField>) -> Shape {
        Shape {
            fields,
            unsealed: None,
        }
    }

    /// A sealed shape of `(name, type, optional)` fields, each name read
    /// as a PHP array key.
    pub fn from_named<K: AsRef<str>>(fields: impl IntoIterator<Item = (K, Type, bool)>) -> Shape {
        Shape::sealed(
            fields
                .into_iter()
                .map(|(name, ty, optional)| {
                    ShapeField::new(ShapeKey::from_name(name.as_ref()), ty, optional)
                })
                .collect(),
        )
    }

    /// An unsealed shape (`...`) whose other entries are unknown.
    pub fn unsealed(fields: Vec<ShapeField>) -> Shape {
        Shape {
            fields,
            unsealed: Some(Box::new((Type::unknown_array_key(), Type::Mixed(false)))),
        }
    }

    pub fn is_sealed(&self) -> bool {
        self.unsealed.is_none()
    }

    /// Whether this is `array{}`: sealed, with no fields.
    pub fn is_empty_array(&self) -> bool {
        self.is_sealed() && self.fields.is_empty()
    }

    pub fn field(&self, key: &ShapeKey) -> Option<&ShapeField> {
        self.fields.iter().find(|f| f.key == *key)
    }

    /// Sets `key`'s field, replacing one already there in place
    /// (last-one-wins, PHPStan's `ConstantArrayTypeBuilder::
    /// setOffsetValueType`).
    pub fn set(&mut self, field: ShapeField) {
        match self.fields.iter_mut().find(|f| f.key == field.key) {
            Some(existing) => *existing = field,
            None => self.fields.push(field),
        }
    }

    /// The shape with `f` applied to each field's type and to the
    /// unsealed tail's value type.
    pub fn map_types(&self, f: &mut impl FnMut(&Type) -> Type) -> Shape {
        Shape {
            fields: self
                .fields
                .iter()
                .map(|field| ShapeField::new(field.key.clone(), f(&field.ty), field.optional))
                .collect(),
            unsealed: self
                .unsealed
                .as_ref()
                .map(|tail| Box::new((tail.0.clone(), f(&tail.1)))),
        }
    }

    /// The fields as `(key text, type, optional)` tuples - tests compare
    /// against written-out field lists this way.
    #[cfg(test)]
    pub fn named_fields(&self) -> Vec<(String, Type, bool)> {
        self.fields
            .iter()
            .map(|f| (f.key.to_string(), f.ty.clone(), f.optional))
            .collect()
    }

    /// Each field's type, then the unsealed tail's value type.
    pub fn types(&self) -> impl Iterator<Item = &Type> {
        self.fields
            .iter()
            .map(|field| &field.ty)
            .chain(self.unsealed.as_deref().map(|tail| &tail.1))
    }
}

/// A sealed shape of `(name, type, optional)` fields, as
/// `Shape::from_named`.
impl<K: AsRef<str>> FromIterator<(K, Type, bool)> for Shape {
    fn from_iter<I: IntoIterator<Item = (K, Type, bool)>>(fields: I) -> Shape {
        Shape::from_named(fields)
    }
}
