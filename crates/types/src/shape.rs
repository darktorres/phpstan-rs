//! Array and object shapes: `array{name: string, 0: int, ...}`,
//! `object{name: string}`.

use std::fmt;

use crate::{PhpString, Repr, TrinaryLogic, Type, parse::canonical_int_key};

/// An array shape key, canonicalized the way PHP canonicalizes an array
/// key: a decimal integer string is an int key. A string key is bytes,
/// as PHP's are: `"\xff"` and `"\xfe"` are two keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ShapeKey {
    Int(i64),
    Str(PhpString),
}

impl ShapeKey {
    /// The key `name` is as a PHP array key.
    pub fn from_name(name: &str) -> ShapeKey {
        ShapeKey::from_bytes(name.as_bytes())
    }

    /// The key a string of these bytes is as a PHP array key.
    pub fn from_bytes(bytes: &[u8]) -> ShapeKey {
        match std::str::from_utf8(bytes).ok().and_then(canonical_int_key) {
            Some(n) => ShapeKey::Int(n),
            None => ShapeKey::Str(PhpString::from_bytes(bytes)),
        }
    }

    /// The key as its literal type (`'name'`, `0`).
    pub fn to_type(&self) -> Type {
        match self {
            ShapeKey::Int(n) => Type::int_literal(*n),
            ShapeKey::Str(s) => Type::string_literal(s.clone()),
        }
    }

    /// The key a literal key type names, if it names one.
    pub fn from_type(ty: &Type) -> Option<ShapeKey> {
        match ty.repr() {
            Repr::IntLiteral(n) => Some(ShapeKey::Int(*n)),
            Repr::StringLiteral(s) => Some(ShapeKey::from_bytes(s.as_bytes())),
            _ => None,
        }
    }

    /// Whether this key is the string `name` (or the int it spells).
    pub fn is(&self, name: &str) -> bool {
        *self == ShapeKey::from_name(name)
    }
}

/// The key's own text: the digits of an int key, a string key verbatim
/// (lossily, for display).
impl fmt::Display for ShapeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShapeKey::Int(n) => write!(f, "{n}"),
            ShapeKey::Str(s) => write!(f, "{s}"),
        }
    }
}

/// The key PHP gives the next element appended to an array being built:
/// one past the highest integer key so far, from 0. Before PHP 8.0 a
/// negative key doesn't count (`[-5 => 'a', 'b']` puts `'b'` at 0); from
/// 8.0 it does (at -4). Once `PHP_INT_MAX` is a key there is none, and
/// appending throws ("Cannot add element to the array as the next
/// element is already occupied").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NextArrayIndex {
    /// Whether the PHP 8.0 rule applies: `Maybe` when the versions the
    /// code runs on straddle it, and both rules are followed.
    negative_keys_count: TrinaryLogic,
    before_8_0: NextKey,
    since_8_0: NextKey,
}

/// One rule's next key: `None` once appending throws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NextKey {
    next: Option<i64>,
    has_int_key: bool,
}

/// The key an appended element gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendKey {
    Key(i64),
    /// The PHP versions the code runs on disagree on it.
    Unknown,
    /// Appending throws.
    Throws,
}

impl Default for NextArrayIndex {
    /// Both rules, for code whose PHP versions aren't known.
    fn default() -> Self {
        NextArrayIndex::new(TrinaryLogic::Maybe)
    }
}

impl NextArrayIndex {
    /// An empty array's, under the PHP 8.0 rule for negative keys when
    /// `negative_keys_count` says it applies.
    pub fn new(negative_keys_count: TrinaryLogic) -> NextArrayIndex {
        let empty = NextKey {
            next: Some(0),
            has_int_key: false,
        };
        NextArrayIndex {
            negative_keys_count,
            before_8_0: empty,
            since_8_0: empty,
        }
    }

    /// The key an appended element gets, moving past it.
    pub fn take(&mut self) -> AppendKey {
        let key = match self.negative_keys_count {
            TrinaryLogic::Yes => self
                .since_8_0
                .next
                .map_or(AppendKey::Throws, AppendKey::Key),
            TrinaryLogic::No => self
                .before_8_0
                .next
                .map_or(AppendKey::Throws, AppendKey::Key),
            TrinaryLogic::Maybe => match (self.before_8_0.next, self.since_8_0.next) {
                (None, None) => AppendKey::Throws,
                (Some(a), Some(b)) if a == b => AppendKey::Key(a),
                _ => AppendKey::Unknown,
            },
        };
        for rule in [&mut self.before_8_0, &mut self.since_8_0] {
            if let Some(n) = rule.next {
                rule.next = n.checked_add(1);
                rule.has_int_key = true;
            }
        }
        key
    }

    /// Records an element written at the explicit integer key `key`.
    pub fn observe(&mut self, key: i64) {
        if let Some(next) = self.before_8_0.next
            && key >= next
        {
            self.before_8_0.next = key.checked_add(1);
        }
        if let Some(next) = self.since_8_0.next
            && (key >= next || !self.since_8_0.has_int_key)
        {
            self.since_8_0.next = key.checked_add(1);
        }
        self.since_8_0.has_int_key = true;
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
#[derive(Debug, Clone)]
pub struct Shape {
    pub fields: Vec<ShapeField>,
    pub unsealed: Option<Box<(Type, Type)>>,
    /// The key an append would take, when it isn't the one the int keys
    /// give: after `unset($a[2])` on `[1, 2, 3]` it stays 3. Bookkeeping
    /// about how the array was built, not part of its type: equality and
    /// hashing ignore it.
    pub next_index: Option<NextArrayIndex>,
}

impl PartialEq for Shape {
    fn eq(&self, other: &Self) -> bool {
        self.fields == other.fields && self.unsealed == other.unsealed
    }
}

impl Eq for Shape {}

impl std::hash::Hash for Shape {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.fields.hash(state);
        self.unsealed.hash(state);
    }
}

impl Shape {
    /// A sealed shape of exactly `fields`.
    pub fn sealed(fields: Vec<ShapeField>) -> Shape {
        Shape {
            fields,
            unsealed: None,
            next_index: None,
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
            unsealed: Some(Box::new((Type::unknown_array_key(), Type::mixed()))),
            next_index: None,
        }
    }

    /// The next index an append takes (PHPStan's `nextAutoIndexes`): the
    /// one carried, else one past the highest int key. `None` when an
    /// optional int key leaves it open.
    pub fn next_append_index(&self) -> Option<NextArrayIndex> {
        if let Some(next) = self.next_index {
            return Some(next);
        }
        let mut next = NextArrayIndex::default();
        for field in &self.fields {
            if let ShapeKey::Int(key) = field.key {
                if field.optional {
                    return None;
                }
                next.observe(key);
            }
        }
        Some(next)
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
            next_index: self.next_index,
        }
    }

    /// The fields as `(key text, type, optional)` tuples - tests compare
    /// against written-out field lists this way.
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
