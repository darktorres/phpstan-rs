//! What a type is: the questions asked of a type from outside this crate.

use crate::{AccessoryKind, CallSiteVariance, Relation, Shape, TrinaryLogic, Type, union_types};

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

    /// The class names this type refers to where a class stands for
    /// itself: an object, and the objects in array keys and values, a
    /// class-string's class, union and intersection members and shape
    /// fields - not in generic arguments. Each once, in order.
    pub fn referenced_class_names(&self) -> Vec<&crate::Name> {
        let mut out: Vec<&crate::Name> = Vec::new();
        self.walk_class_positions(&mut |t| {
            if let Type::Object(name, _) = t
                && !out.contains(&name)
            {
                out.push(name);
            }
        });
        out
    }

    /// Whether a `parent` stands in a class position (see
    /// `referenced_class_names`) unresolved.
    pub fn has_parent_ref(&self) -> bool {
        let mut found = false;
        self.walk_class_positions(&mut |t| found |= matches!(t, Type::ParentRef));
        found
    }

    fn walk_class_positions<'a>(&'a self, f: &mut impl FnMut(&'a Type)) {
        match self {
            Type::Array(key, value) => {
                key.walk_class_positions(f);
                value.walk_class_positions(f);
            }
            Type::ClassString(inner) => inner.walk_class_positions(f),
            Type::Union(members) | Type::BenevolentUnion(members) | Type::Intersection(members) => {
                for m in members {
                    m.walk_class_positions(f);
                }
            }
            Type::ArrayShape(shape) | Type::ObjectShape(shape) => {
                for t in shape.types() {
                    t.walk_class_positions(f);
                }
            }
            other => f(other),
        }
    }

    /// Whether this type, or one nested in it, is of the kind the keyword
    /// `name` writes (`float`, `int`, `string`, `bool`, `array`, `mixed`,
    /// `object`, `callable`, `iterable`), or is an object of class `name`.
    pub fn mentions(&self, name: &str) -> bool {
        let direct = match (name, self) {
            ("float", Type::Float | Type::FloatLiteral(_)) => true,
            ("int", Type::Int | Type::IntLiteral(_) | Type::IntRange(..)) => true,
            (
                "string",
                Type::String
                | Type::StringLiteral(_)
                | Type::ClassString(_)
                | Type::ClassKindString(_),
            ) => true,
            ("bool", Type::Bool | Type::True | Type::False) => true,
            ("array", Type::Array(..) | Type::ArrayShape(_)) => true,
            ("mixed", Type::Mixed(_)) => true,
            ("object", Type::ObjectWithoutClass) => true,
            ("callable", Type::Callable(_)) => true,
            ("iterable", Type::Object(n, _)) if n.eq_ignore_ascii_case("Traversable") => true,
            (class, Type::Object(n, _)) => n.trim_start_matches('\\').eq_ignore_ascii_case(class),
            _ => false,
        };
        direct || self.any_child(|t| t.mentions(name))
    }

    /// The variance a generic argument is annotated with at its use site
    /// (`covariant Foo`, `*`); `None` for a plain argument.
    pub fn call_site_variance(&self) -> Option<CallSiteVariance> {
        match self {
            Type::VarianceArg(variance, _) => Some(*variance),
            _ => None,
        }
    }

    /// Whether `@template T of <this>` declares a bound a template can
    /// take (PHPStan's `TemplateTypeFactory`): `mixed`, `null`, a
    /// scalar, integer range or literal, an array or array shape, an
    /// object, object shape or enum case, `key-of`, a union or an
    /// intersection, or another template. A class-string, a callable or
    /// a `resource` is not one.
    pub fn is_supported_template_bound(&self) -> bool {
        matches!(
            self,
            Type::Mixed(_)
                | Type::Null
                | Type::TemplateParam(_)
                | Type::Array(..)
                | Type::ArrayShape(_)
                | Type::String
                | Type::StringLiteral(_)
                | Type::Int
                | Type::IntLiteral(_)
                | Type::IntRange(..)
                | Type::Float
                | Type::FloatLiteral(_)
                | Type::Bool
                | Type::True
                | Type::False
                | Type::Object(..)
                | Type::EnumCase(..)
                | Type::ObjectWithoutClass
                | Type::ObjectShape(_)
                | Type::KeyOf(_)
                | Type::Union(_)
                | Type::BenevolentUnion(_)
                | Type::Intersection(_)
        )
    }

    /// Whether a `value-of`, `key-of`, offset access or class-constant
    /// reference, left for the registry to resolve, stands anywhere in
    /// this type.
    pub fn contains_deferred(&self) -> bool {
        matches!(
            self,
            Type::ValueOf(_) | Type::KeyOf(_) | Type::OffsetAccess(..) | Type::ClassConstant { .. }
        ) || self.any_child(Type::contains_deferred)
    }

    /// The array shape this type is, or an intersection refines.
    pub fn shape(&self) -> Option<&Shape> {
        match self {
            Type::ArrayShape(shape) => Some(shape),
            Type::Intersection(members) => members.iter().find_map(Type::shape),
            _ => None,
        }
    }

    /// Whether this names the class it is written in: `self`, `static`
    /// or `$this`.
    pub fn is_self_reference(&self) -> bool {
        matches!(self, Type::SelfRef(_) | Type::StaticRef(_) | Type::This)
    }

    /// Every template this type uses, at any depth (not inside a
    /// template's own bound), in order.
    pub fn nested_templates(&self) -> Vec<&crate::TemplateParam> {
        let mut out = Vec::new();
        self.collect_nested_templates(&mut out);
        out
    }

    fn collect_nested_templates<'t>(&'t self, out: &mut Vec<&'t crate::TemplateParam>) {
        match self {
            Type::TemplateParam(param) => out.push(param),
            _ => self.for_each_child(&mut |t| t.collect_nested_templates(out)),
        }
    }

    /// Whether every value is a single, known value: a literal, `null`,
    /// an enum case or a sealed array shape (PHPStan's
    /// `isConstantValue()`, with enum cases counted).
    pub fn is_constant_value(&self) -> TrinaryLogic {
        match self {
            Type::IntLiteral(_)
            | Type::StringLiteral(_)
            | Type::FloatLiteral(_)
            | Type::True
            | Type::False
            | Type::Null
            | Type::EnumCase(..) => TrinaryLogic::Yes,
            Type::ArrayShape(shape) => TrinaryLogic::from_bool(shape.is_sealed()),
            Type::Union(members) => members
                .iter()
                .map(Type::is_constant_value)
                .reduce(TrinaryLogic::and)
                .unwrap_or(TrinaryLogic::No),
            Type::Mixed(_) | Type::TemplateParam(_) => TrinaryLogic::Maybe,
            _ => TrinaryLogic::No,
        }
    }

    /// A conditional type's condition: its subject (a parameter or
    /// template name), its target, whether it's negated (`is not`), and
    /// whether the subject names a template. `None` for any other type.
    pub fn conditional_condition(&self) -> Option<(&str, &Type, bool, bool)> {
        match self {
            Type::Conditional {
                subject,
                target,
                negated,
                template_subject,
                ..
            } => Some((subject, target, *negated, *template_subject)),
            _ => None,
        }
    }

    /// Whether this is `void`.
    pub fn is_void(&self) -> bool {
        matches!(self, Type::Void)
    }

    /// Whether this is one case of an enum.
    pub fn is_enum_case(&self) -> bool {
        matches!(self, Type::EnumCase(..))
    }

    /// PHPStan's `getFiniteTypes()`: the values this type can only be, one
    /// type each, when there are few enough to list; none otherwise.
    pub fn finite_types(&self) -> Vec<Type> {
        const LIMIT: i64 = 256;
        match self {
            Type::IntLiteral(_)
            | Type::StringLiteral(_)
            | Type::FloatLiteral(_)
            | Type::True
            | Type::False
            | Type::Null
            | Type::EnumCase(..) => vec![self.clone()],
            Type::Bool => vec![Type::True, Type::False],
            Type::IntRange(Some(min), Some(max)) if max - min < LIMIT => {
                (*min..=*max).map(Type::IntLiteral).collect()
            }
            Type::Union(members) | Type::BenevolentUnion(members) => {
                let mut all = Vec::new();
                for member in members {
                    let finite = member.finite_types();
                    if finite.is_empty() {
                        return Vec::new();
                    }
                    all.extend(finite);
                }
                all
            }
            _ => Vec::new(),
        }
    }

    /// PHPStan's `generalize(lessSpecific)`: literals and ranges widen to
    /// their base type, an enum case to its class, a shape to the array
    /// of its generalized keys and values (still a list, still non-empty,
    /// when it was), and a union's members each.
    pub fn generalize(&self) -> Type {
        match self {
            Type::IntLiteral(_) | Type::IntRange(..) => Type::Int,
            Type::StringLiteral(_) => Type::String,
            Type::FloatLiteral(_) => Type::Float,
            Type::True | Type::False => Type::Bool,
            Type::EnumCase(class, _) => Type::Object(class.clone(), Vec::new()),
            Type::ArrayShape(shape) => {
                let key = self
                    .array_key_type()
                    .map_or(Type::unknown_array_key(), |k| k.generalize());
                let value = self
                    .array_value_type()
                    .map_or(Type::Mixed(false), |v| v.generalize());
                let (key, value) = if value.is_never() {
                    (Type::unknown_array_key(), Type::Mixed(false))
                } else {
                    (key, value)
                };
                let mut members = vec![Type::Array(Box::new(key), Box::new(value))];
                if shape.is_list() && !shape.fields.is_empty() {
                    members.push(Type::Accessory(AccessoryKind::ArrayList));
                }
                if shape.fields.iter().any(|f| !f.optional) {
                    members.push(Type::Accessory(AccessoryKind::NonEmptyArray));
                }
                crate::intersect_types(members)
            }
            Type::Union(members) | Type::BenevolentUnion(members) => members
                .iter()
                .map(Type::generalize)
                .reduce(union_types)
                .unwrap_or(Type::Never),
            Type::Intersection(members) => crate::intersect_types(
                members
                    .iter()
                    .filter(|m| {
                        !matches!(
                            m,
                            Type::Accessory(
                                AccessoryKind::NonEmptyString
                                    | AccessoryKind::NonFalsyString
                                    | AccessoryKind::LiteralString
                                    | AccessoryKind::NumericString
                                    | AccessoryKind::LowercaseString
                                    | AccessoryKind::UppercaseString
                                    | AccessoryKind::DecimalIntegerString
                            ) | Type::HasOffsetValue(..)
                                | Type::HasOffset(_)
                        )
                    })
                    .map(Type::generalize)
                    .collect(),
            ),
            other => other.clone(),
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

    /// An intersection's members, or this type alone.
    pub fn intersection_members(&self) -> &[Type] {
        match self {
            Type::Intersection(members) => members,
            other => std::slice::from_ref(other),
        }
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

    /// The value of an integer literal; `None` for any other type.
    pub fn int_literal_value(&self) -> Option<i64> {
        match self {
            Type::IntLiteral(n) => Some(*n),
            _ => None,
        }
    }

    /// The value of a string literal; `None` for any other type.
    pub fn string_literal_value(&self) -> Option<&crate::PhpString> {
        match self {
            Type::StringLiteral(s) => Some(s),
            _ => None,
        }
    }

    /// The class and generic arguments of an object of a named class;
    /// `None` for any other type.
    pub fn object_class_and_args(&self) -> Option<(&crate::Name, &[Type])> {
        match self {
            Type::Object(class, args) => Some((class, args)),
            _ => None,
        }
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
