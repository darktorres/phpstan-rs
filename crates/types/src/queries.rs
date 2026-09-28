//! What a type is: the questions asked of a type from outside this crate.

use crate::{AccessoryKind, Relation, Shape, TrinaryLogic, Type, union_types};

impl Type {
    /// Whether this is `mixed`, written or not.
    pub fn is_mixed(&self) -> bool {
        matches!(self, Type::Mixed(_))
    }

    /// Whether this is a `mixed` a written type resolved to.
    pub fn is_explicit_mixed(&self) -> bool {
        matches!(self, Type::Mixed(true))
    }

    /// A union's members, or this type alone.
    pub fn union_members(&self) -> &[Type] {
        match self {
            Type::Union(members) | Type::BenevolentUnion(members) => members,
            other => std::slice::from_ref(other),
        }
    }

    /// PHPStan's `getObjectClassNames()`: the classes of the objects and
    /// enum cases among this type's union and intersection members.
    pub fn object_class_names(&self) -> Vec<&crate::Name> {
        match self {
            Type::Object(class, _) | Type::EnumCase(class, _) => vec![class],
            Type::Union(members) | Type::BenevolentUnion(members) | Type::Intersection(members) => {
                members.iter().flat_map(Type::object_class_names).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Whether this is `never`.
    pub fn is_never(&self) -> bool {
        matches!(self, Type::Never)
    }

    /// Whether this is a `@template` type.
    pub fn is_template(&self) -> bool {
        matches!(self, Type::TemplateParam(_))
    }

    /// Whether this is a benevolent union.
    pub fn is_benevolent_union(&self) -> bool {
        matches!(self, Type::BenevolentUnion(_))
    }

    /// Whether this is a `mixed` with no type information behind it.
    pub fn is_implicit_mixed(&self) -> bool {
        matches!(self, Type::Mixed(false))
    }

    /// Whether every value of this type is one of `of` (`Yes`), none is
    /// (`No`), or some are (`Maybe`). `never` is no kind of value.
    pub fn is_within(&self, of: &Type) -> TrinaryLogic {
        if matches!(self, Type::Never) {
            return TrinaryLogic::No;
        }
        Relation::structural().is_super_type_of(of, self)
    }

    /// PHPStan's `isNull()`.
    pub fn is_null(&self) -> TrinaryLogic {
        self.is_within(&Type::Null)
    }

    /// PHPStan's `isTrue()`.
    pub fn is_true(&self) -> TrinaryLogic {
        self.is_within(&Type::True)
    }

    /// PHPStan's `isFalse()`.
    pub fn is_false(&self) -> TrinaryLogic {
        self.is_within(&Type::False)
    }

    /// PHPStan's `isBoolean()`.
    pub fn is_bool(&self) -> TrinaryLogic {
        self.is_within(&Type::Bool)
    }

    /// PHPStan's `isInteger()`.
    pub fn is_int(&self) -> TrinaryLogic {
        self.is_within(&Type::Int)
    }

    /// PHPStan's `isFloat()`.
    pub fn is_float(&self) -> TrinaryLogic {
        self.is_within(&Type::Float)
    }

    /// PHPStan's `isString()`.
    pub fn is_string(&self) -> TrinaryLogic {
        self.is_within(&Type::String)
    }

    /// PHPStan's `isNumericString()`.
    pub fn is_numeric_string(&self) -> TrinaryLogic {
        self.is_refined_string(AccessoryKind::NumericString)
    }

    /// Whether every value is a string `kind` refines: PHPStan's
    /// `isNonEmptyString()`, `isNonFalsyString()`, `isLiteralString()`,
    /// `isLowercaseString()`, `isUppercaseString()`, `isNumericString()`.
    pub fn is_refined_string(&self, kind: AccessoryKind) -> TrinaryLogic {
        self.is_within(&Type::Intersection(vec![
            Type::String,
            Type::Accessory(kind),
        ]))
    }

    /// This type with its strings refined by `kind`: a `string`, bare or
    /// already refined, gains it, in a union too; other members are left
    /// as they are.
    pub fn with_string_accessory(&self, kind: AccessoryKind) -> Type {
        match self {
            Type::String => Type::Intersection(vec![Type::String, Type::Accessory(kind)]),
            Type::Intersection(members)
                if members.contains(&Type::String) && !members.contains(&Type::Accessory(kind)) =>
            {
                let mut members = members.clone();
                members.push(Type::Accessory(kind));
                Type::Intersection(members)
            }
            Type::Union(members) => Type::Union(
                members
                    .iter()
                    .map(|m| m.with_string_accessory(kind))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// The `(min, max)` of an integer literal or range, `None` bounds
    /// unbounded; `None` for any other type.
    pub fn int_range_bounds(&self) -> Option<(Option<i64>, Option<i64>)> {
        match self {
            Type::IntLiteral(n) => Some((Some(*n), Some(*n))),
            Type::IntRange(min, max) => Some((*min, *max)),
            _ => None,
        }
    }

    /// PHPStan's `isArray()`.
    pub fn is_array(&self) -> TrinaryLogic {
        self.is_within(&Type::Array(
            Box::new(Type::Mixed(false)),
            Box::new(Type::Mixed(false)),
        ))
    }

    /// PHPStan's `isObject()`.
    pub fn is_object(&self) -> TrinaryLogic {
        self.is_within(&Type::ObjectWithoutClass)
    }

    /// PHPStan's `isScalar()`: a `bool`, `int`, `float` or `string`.
    pub fn is_scalar(&self) -> TrinaryLogic {
        self.is_within(&Type::Union(vec![
            Type::Bool,
            Type::Int,
            Type::Float,
            Type::String,
        ]))
    }

    /// PHPStan's `isList()`.
    pub fn is_list(&self) -> TrinaryLogic {
        self.is_within(&Type::list_of(Type::Mixed(false)))
    }

    /// Whether every value is a non-empty array.
    pub fn is_non_empty_array(&self) -> TrinaryLogic {
        self.is_within(&Type::Intersection(vec![
            Type::Array(Box::new(Type::Mixed(false)), Box::new(Type::Mixed(false))),
            Type::Accessory(AccessoryKind::NonEmptyArray),
        ]))
    }

    /// Whether every value is the empty array.
    pub fn is_empty_array(&self) -> TrinaryLogic {
        self.is_within(&Type::ArrayShape(Shape::sealed(Vec::new())))
    }

    /// PHPStan's `getIterableValueType()` for a type every value of which
    /// is an array; `None` otherwise. `array{}` holds no value (`never`).
    pub fn array_value_type(&self) -> Option<Type> {
        match self {
            Type::Array(_, value) => Some((**value).clone()),
            Type::ArrayShape(shape) => Some(
                shape
                    .fields
                    .iter()
                    .map(|f| f.ty.clone())
                    .chain(shape.unsealed.as_deref().map(|tail| tail.1.clone()))
                    .reduce(union_types)
                    .unwrap_or(Type::Never),
            ),
            Type::Intersection(members) => members.iter().find_map(Type::array_value_type),
            Type::Union(members) | Type::BenevolentUnion(members) => members
                .iter()
                .map(Type::array_value_type)
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .reduce(union_types),
            Type::TemplateParam(param) => param.bound.array_value_type(),
            _ => None,
        }
    }

    /// PHPStan's `getIterableKeyType()` for a type every value of which
    /// is an array; `None` otherwise. `array{}` holds no key (`never`).
    pub fn array_key_type(&self) -> Option<Type> {
        match self {
            Type::Array(key, _) => Some((**key).clone()),
            Type::ArrayShape(shape) => {
                let keys = crate::shape_key_type(shape);
                Some(match shape.unsealed.as_deref() {
                    Some(tail) => union_types(keys, tail.0.clone()),
                    None => keys,
                })
            }
            // A list keys on `int<0, max>` whatever its array member says.
            Type::Intersection(members) => {
                let key = members.iter().find_map(Type::array_key_type)?;
                if members.contains(&Type::Accessory(AccessoryKind::ArrayList)) {
                    Some(crate::intersect_types(vec![
                        key,
                        Type::IntRange(Some(0), None),
                    ]))
                } else {
                    Some(key)
                }
            }
            Type::Union(members) | Type::BenevolentUnion(members) => members
                .iter()
                .map(Type::array_key_type)
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .reduce(union_types),
            Type::TemplateParam(param) => param.bound.array_key_type(),
            _ => None,
        }
    }

    /// PHPStan's `getArrays()`: the members that are arrays.
    pub fn arrays(&self) -> Vec<&Type> {
        self.union_members()
            .iter()
            .filter(|m| m.is_array().yes())
            .collect()
    }

    /// PHPStan's `getConstantArrays()`: the sealed shapes making up this
    /// type, or none unless every member is one.
    pub fn constant_arrays(&self) -> Vec<&Shape> {
        self.union_members()
            .iter()
            .map(|m| match m {
                Type::ArrayShape(shape) if shape.is_sealed() => Some(shape),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default()
    }

    /// The value of `true` or `false`; `None` for any other type.
    pub fn bool_literal_value(&self) -> Option<bool> {
        match self {
            Type::True => Some(true),
            Type::False => Some(false),
            _ => None,
        }
    }

    /// PHPStan's `toBoolean()`: `true` when every value is truthy, `false`
    /// when every value is falsy, `bool` otherwise.
    pub fn to_boolean(&self) -> Type {
        match self {
            Type::True => Type::True,
            Type::False | Type::Null | Type::Void => Type::False,
            Type::IntLiteral(n) => Type::bool_literal(*n != 0),
            Type::FloatLiteral(n) => Type::bool_literal(*n != 0.0),
            Type::StringLiteral(s) => Type::bool_literal(!s.is_empty() && s != "0"),
            Type::IntRange(min, max) => {
                if min.is_some_and(|m| m > 0) || max.is_some_and(|m| m < 0) {
                    Type::True
                } else if *min == Some(0) && *max == Some(0) {
                    Type::False
                } else {
                    Type::Bool
                }
            }
            Type::Object(..)
            | Type::ObjectWithoutClass
            | Type::ObjectShape(_)
            | Type::Resource { .. }
            | Type::EnumCase(..)
            | Type::This
            | Type::StaticRef(_)
            | Type::SelfRef(_)
            | Type::ParentRef
            | Type::ClassString(_)
            | Type::ClassKindString(_)
            | Type::HasOffset(_)
            | Type::HasOffsetValue(..)
            | Type::HasMethod(_)
            | Type::HasProperty(_) => Type::True,
            Type::ArrayShape(shape) => {
                if shape.fields.iter().any(|f| !f.optional) {
                    Type::True
                } else if shape.is_empty_array() {
                    Type::False
                } else {
                    Type::Bool
                }
            }
            Type::Accessory(AccessoryKind::NonFalsyString | AccessoryKind::NonEmptyArray) => {
                Type::True
            }
            Type::Union(members) | Type::BenevolentUnion(members) => members
                .iter()
                .map(Type::to_boolean)
                .reduce(union_types)
                .unwrap_or(Type::Never),
            Type::Intersection(members) => {
                crate::intersect_types(members.iter().map(Type::to_boolean).collect())
            }
            Type::TemplateParam(param) => param.bound.to_boolean(),
            Type::Never => Type::Never,
            _ => Type::Bool,
        }
    }
}
