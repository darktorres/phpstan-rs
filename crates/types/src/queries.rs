//! What a type is: the questions asked of a type from outside this crate.

use crate::Repr;
use crate::{AccessoryKind, CallSiteVariance, Relation, Shape, TrinaryLogic, Type, union_types};

impl Type {
    /// Whether this is `mixed`, written or not.
    pub fn is_mixed(&self) -> bool {
        matches!(self, Type(Repr::Mixed(_)))
    }

    /// Whether this is a `mixed` a written type resolved to.
    pub fn is_explicit_mixed(&self) -> bool {
        matches!(self, Type(Repr::Mixed(true)))
    }

    /// A union's members, or this type alone.
    pub fn union_members(&self) -> &[Type] {
        match self {
            Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members)) => members,
            other => std::slice::from_ref(other),
        }
    }

    /// PHPStan's `getObjectClassNames()`: the classes of the objects and
    /// enum cases among this type's union and intersection members.
    pub fn object_class_names(&self) -> Vec<&crate::Name> {
        match self {
            Type(Repr::Object(class, _)) | Type(Repr::EnumCase(class, _)) => vec![class],
            Type(Repr::Union(members))
            | Type(Repr::BenevolentUnion(members))
            | Type(Repr::Intersection(members)) => {
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
            if let Type(Repr::Object(name, _)) = t
                && !out.contains(&name)
            {
                out.push(name);
            }
        });
        out
    }

    /// The class of every object at any depth: generic arguments,
    /// array and shape values, callable signatures. In order, with
    /// repeats.
    pub fn nested_class_names(&self) -> Vec<&crate::Name> {
        let mut out = Vec::new();
        self.collect_nested_class_names(&mut out);
        out
    }

    fn collect_nested_class_names<'t>(&'t self, out: &mut Vec<&'t crate::Name>) {
        if let Type(Repr::Object(name, _)) = self {
            out.push(name);
        }
        self.for_each_child(&mut |t| t.collect_nested_class_names(out));
    }

    /// Whether a `parent` stands in a class position (see
    /// `referenced_class_names`) unresolved.
    pub fn has_parent_ref(&self) -> bool {
        let mut found = false;
        self.walk_class_positions(&mut |t| found |= matches!(t, Type(Repr::ParentRef)));
        found
    }

    fn walk_class_positions<'a>(&'a self, f: &mut impl FnMut(&'a Type)) {
        match self {
            Type(Repr::Array(key, value)) => {
                key.walk_class_positions(f);
                value.walk_class_positions(f);
            }
            Type(Repr::ClassString(inner)) => inner.walk_class_positions(f),
            Type(Repr::Union(members))
            | Type(Repr::BenevolentUnion(members))
            | Type(Repr::Intersection(members)) => {
                for m in members {
                    m.walk_class_positions(f);
                }
            }
            Type(Repr::ArrayShape(shape)) | Type(Repr::ObjectShape(shape)) => {
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
            ("float", Type(Repr::Float) | Type(Repr::FloatLiteral(_))) => true,
            ("int", Type(Repr::Int) | Type(Repr::IntLiteral(_)) | Type(Repr::IntRange(..))) => true,
            (
                "string",
                Type(Repr::String)
                | Type(Repr::StringLiteral(_))
                | Type(Repr::ClassString(_))
                | Type(Repr::ClassKindString(_)),
            ) => true,
            ("bool", Type(Repr::Bool) | Type(Repr::True) | Type(Repr::False)) => true,
            ("array", Type(Repr::Array(..)) | Type(Repr::ArrayShape(_))) => true,
            ("mixed", Type(Repr::Mixed(_))) => true,
            ("object", Type(Repr::ObjectWithoutClass)) => true,
            ("callable", Type(Repr::Callable(_))) => true,
            ("iterable", Type(Repr::Object(n, _))) if n.eq_ignore_ascii_case("Traversable") => true,
            (class, Type(Repr::Object(n, _))) => {
                n.trim_start_matches('\\').eq_ignore_ascii_case(class)
            }
            _ => false,
        };
        direct || self.any_child(|t| t.mentions(name))
    }

    /// The variance a generic argument is annotated with at its use site
    /// (`covariant Foo`, `*`); `None` for a plain argument.
    pub fn call_site_variance(&self) -> Option<CallSiteVariance> {
        match self {
            Type(Repr::VarianceArg(variance, _)) => Some(*variance),
            _ => None,
        }
    }

    /// The type a generic argument names under its call-site variance
    /// (`covariant Foo` is `Foo`); `None` for `*`, which names no type.
    pub fn without_call_site_variance(&self) -> Option<&Type> {
        match self {
            Type(Repr::VarianceArg(CallSiteVariance::Bivariant, _)) => None,
            Type(Repr::VarianceArg(_, inner)) => Some(inner),
            other => Some(other),
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
            Type(Repr::Mixed(_))
                | Type(Repr::Null)
                | Type(Repr::TemplateParam(_))
                | Type(Repr::Array(..))
                | Type(Repr::ArrayShape(_))
                | Type(Repr::String)
                | Type(Repr::StringLiteral(_))
                | Type(Repr::Int)
                | Type(Repr::IntLiteral(_))
                | Type(Repr::IntRange(..))
                | Type(Repr::Float)
                | Type(Repr::FloatLiteral(_))
                | Type(Repr::Bool)
                | Type(Repr::True)
                | Type(Repr::False)
                | Type(Repr::Object(..))
                | Type(Repr::EnumCase(..))
                | Type(Repr::ObjectWithoutClass)
                | Type(Repr::ObjectShape(_))
                | Type(Repr::KeyOf(_))
                | Type(Repr::Union(_))
                | Type(Repr::BenevolentUnion(_))
                | Type(Repr::Intersection(_))
        )
    }

    /// Whether a `value-of`, `key-of`, offset access or class-constant
    /// reference, left for the registry to resolve, stands anywhere in
    /// this type.
    pub fn contains_deferred(&self) -> bool {
        matches!(
            self,
            Type(Repr::ValueOf(_))
                | Type(Repr::KeyOf(_))
                | Type(Repr::OffsetAccess(..))
                | Type(Repr::ClassConstant { .. })
        ) || self.any_child(Type::contains_deferred)
    }

    /// The object shape this type is, or an intersection refines.
    pub fn object_shape_of(&self) -> Option<&Shape> {
        match self {
            Type(Repr::ObjectShape(shape)) => Some(shape),
            Type(Repr::Intersection(members)) => members.iter().find_map(Type::object_shape_of),
            _ => None,
        }
    }

    /// The array shape this type is, or an intersection refines.
    pub fn shape(&self) -> Option<&Shape> {
        match self {
            Type(Repr::ArrayShape(shape)) => Some(shape),
            Type(Repr::Intersection(members)) => members.iter().find_map(Type::shape),
            _ => None,
        }
    }

    /// Whether this is `static` or `$this`: the class a member is called
    /// on.
    pub fn is_late_static_reference(&self) -> bool {
        matches!(self, Type(Repr::StaticRef(_)) | Type(Repr::This))
    }

    /// Whether this names the class it is written in: `self`, `static`
    /// or `$this`.
    pub fn is_self_reference(&self) -> bool {
        matches!(
            self,
            Type(Repr::SelfRef(_)) | Type(Repr::StaticRef(_)) | Type(Repr::This)
        )
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
            Type(Repr::TemplateParam(param)) => out.push(param),
            _ => self.for_each_child(&mut |t| t.collect_nested_templates(out)),
        }
    }

    /// Whether this type has exactly one value: a literal (a float one
    /// but `NAN`, never identical to itself), `null`, an enum case, or a
    /// sealed array shape of required single-valued fields.
    pub fn is_single_value(&self) -> bool {
        match self {
            Type(Repr::IntLiteral(_))
            | Type(Repr::StringLiteral(_))
            | Type(Repr::True)
            | Type(Repr::False)
            | Type(Repr::Null)
            | Type(Repr::EnumCase(..)) => true,
            Type(Repr::FloatLiteral(f)) => !f.is_nan(),
            Type(Repr::ArrayShape(shape)) => {
                shape.is_sealed()
                    && shape
                        .fields
                        .iter()
                        .all(|f| !f.optional && f.ty.is_single_value())
            }
            _ => false,
        }
    }

    /// PHPStan's `isConstantValue()`, with enum cases counted: whether
    /// every value is one known value, each member single-valued.
    pub fn is_constant_value(&self) -> TrinaryLogic {
        if self.is_mixed() || self.is_template() {
            return TrinaryLogic::Maybe;
        }
        TrinaryLogic::from_bool(self.union_members().iter().all(Type::is_single_value))
    }

    /// Every integer value this type can only be: an integer literal's,
    /// and a union's only when every member has one. Empty for anything
    /// else - `constant_strings`' integer counterpart.
    pub fn constant_ints(&self) -> Vec<i64> {
        self.union_members()
            .iter()
            .map(Type::int_literal_value)
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default()
    }

    /// Whether some part of this type - a union member, or a member of
    /// an intersection among them - is `mixed`, `never` or a template:
    /// a part that says nothing checkable about a value.
    pub fn has_unknown_part(&self) -> bool {
        self.union_members().iter().any(|m| {
            m.intersection_members()
                .iter()
                .any(|p| p.is_mixed() || p.is_never() || p.is_template())
        })
    }

    /// A conditional type's condition: its subject (a parameter or
    /// template name), its target, whether it's negated (`is not`), and
    /// whether the subject names a template. `None` for any other type.
    pub fn conditional_condition(&self) -> Option<(&str, &Type, bool, bool)> {
        match self {
            Type(Repr::Conditional {
                subject,
                target,
                negated,
                template_subject,
                ..
            }) => Some((subject, target, *negated, *template_subject)),
            _ => None,
        }
    }

    /// Whether this is `void`.
    pub fn is_void(&self) -> bool {
        matches!(self, Type(Repr::Void))
    }

    /// An enum case's enum and case name; `None` for any other type.
    pub fn enum_class_and_case(&self) -> Option<(&crate::Name, &crate::Name)> {
        match self {
            Type(Repr::EnumCase(class, case)) => Some((class, case)),
            _ => None,
        }
    }

    /// Whether this is one case of an enum.
    pub fn is_enum_case(&self) -> bool {
        matches!(self, Type(Repr::EnumCase(..)))
    }

    /// PHPStan's `getFiniteTypes()`: the values this type can only be, one
    /// type each, when there are few enough to list; none otherwise.
    pub fn finite_types(&self) -> Vec<Type> {
        const LIMIT: i64 = 256;
        match self {
            Type(Repr::IntLiteral(_))
            | Type(Repr::StringLiteral(_))
            | Type(Repr::FloatLiteral(_))
            | Type(Repr::True)
            | Type(Repr::False)
            | Type(Repr::Null)
            | Type(Repr::EnumCase(..)) => vec![self.clone()],
            Type(Repr::Bool) => vec![Type(Repr::True), Type(Repr::False)],
            Type(Repr::IntRange(Some(min), Some(max))) if max - min < LIMIT => {
                (*min..=*max).map(Type::int_literal).collect()
            }
            Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members)) => {
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
            Type(Repr::IntLiteral(_)) | Type(Repr::IntRange(..)) => Type(Repr::Int),
            Type(Repr::StringLiteral(_)) => Type(Repr::String),
            Type(Repr::FloatLiteral(_)) => Type(Repr::Float),
            Type(Repr::True) | Type(Repr::False) => Type(Repr::Bool),
            Type(Repr::EnumCase(class, _)) => Type(Repr::Object(class.clone(), Vec::new())),
            Type(Repr::ArrayShape(shape)) => {
                let key = self
                    .array_key_type()
                    .map_or(Type::unknown_array_key(), |k| k.generalize());
                let value = self
                    .array_value_type()
                    .map_or(Type(Repr::Mixed(false)), |v| v.generalize());
                let (key, value) = if value.is_never() {
                    (Type::unknown_array_key(), Type(Repr::Mixed(false)))
                } else {
                    (key, value)
                };
                let mut members = vec![Type(Repr::Array(Box::new(key), Box::new(value)))];
                if shape.is_list() && !shape.fields.is_empty() {
                    members.push(Type(Repr::Accessory(AccessoryKind::ArrayList)));
                }
                if shape.fields.iter().any(|f| !f.optional) {
                    members.push(Type(Repr::Accessory(AccessoryKind::NonEmptyArray)));
                }
                crate::intersect_types(members)
            }
            Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members)) => members
                .iter()
                .map(Type::generalize)
                .reduce(union_types)
                .unwrap_or(Type(Repr::Never)),
            Type(Repr::Intersection(members)) => crate::intersect_types(
                members
                    .iter()
                    .filter(|m| {
                        !matches!(
                            m,
                            Type(Repr::Accessory(
                                AccessoryKind::NonEmptyString
                                    | AccessoryKind::NonFalsyString
                                    | AccessoryKind::LiteralString
                                    | AccessoryKind::NumericString
                                    | AccessoryKind::LowercaseString
                                    | AccessoryKind::UppercaseString
                                    | AccessoryKind::DecimalIntegerString
                            )) | Type(Repr::HasOffsetValue(..))
                                | Type(Repr::HasOffset(_))
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
        matches!(self, Type(Repr::Never))
    }

    /// Whether this is a `@template` type.
    pub fn is_template(&self) -> bool {
        matches!(self, Type(Repr::TemplateParam(_)))
    }

    /// A `class-string<Foo>`'s class; `None` for any other type, or a
    /// class-string of anything but one named class.
    pub fn class_string_class(&self) -> Option<&crate::Name> {
        match self {
            Type(Repr::ClassString(inner)) => inner.object_class_and_args().map(|(class, _)| class),
            _ => None,
        }
    }

    /// A reflected member's owner type and member name (a
    /// `ReflectionMethod`/`ReflectionProperty` bound to one member);
    /// `None` for any other type.
    pub fn reflected_member_of(&self) -> Option<(&Type, &str)> {
        match self {
            Type(Repr::ReflectedMember(owner, name)) => Some((owner, name)),
            _ => None,
        }
    }

    /// A template's bound; `None` for any other type.
    pub fn template_bound(&self) -> Option<&Type> {
        match self {
            Type(Repr::TemplateParam(param)) => Some(&param.bound),
            _ => None,
        }
    }

    /// Whether this is a benevolent union.
    pub fn is_benevolent_union(&self) -> bool {
        matches!(self, Type(Repr::BenevolentUnion(_)))
    }

    /// An intersection's members, or this type alone.
    pub fn intersection_members(&self) -> &[Type] {
        match self {
            Type(Repr::Intersection(members)) => members,
            other => std::slice::from_ref(other),
        }
    }

    /// Whether this is a `mixed` with no type information behind it.
    pub fn is_implicit_mixed(&self) -> bool {
        matches!(self, Type(Repr::Mixed(false)))
    }

    /// Whether every value of this type is one of `of` (`Yes`), none is
    /// (`No`), or some are (`Maybe`). `never` is no kind of value.
    pub fn is_within(&self, of: &Type) -> TrinaryLogic {
        if matches!(self, Type(Repr::Never)) {
            return TrinaryLogic::No;
        }
        Relation::structural().is_super_type_of(of, self)
    }

    /// PHPStan's `isNull()`.
    pub fn is_null(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::Null))
    }

    /// PHPStan's `isTrue()`.
    pub fn is_true(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::True))
    }

    /// PHPStan's `isFalse()`.
    pub fn is_false(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::False))
    }

    /// PHPStan's `isBoolean()`.
    pub fn is_bool(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::Bool))
    }

    /// PHPStan's `isInteger()`.
    pub fn is_int(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::Int))
    }

    /// PHPStan's `isFloat()`.
    pub fn is_float(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::Float))
    }

    /// PHPStan's `isString()`.
    pub fn is_string(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::String))
    }

    /// PHPStan's `isNumericString()`.
    pub fn is_numeric_string(&self) -> TrinaryLogic {
        self.is_refined_string(AccessoryKind::NumericString)
    }

    /// Whether every value is a string `kind` refines: PHPStan's
    /// `isNonEmptyString()`, `isNonFalsyString()`, `isLiteralString()`,
    /// `isLowercaseString()`, `isUppercaseString()`, `isNumericString()`.
    pub fn is_refined_string(&self, kind: AccessoryKind) -> TrinaryLogic {
        self.is_within(&Type(Repr::Intersection(vec![
            Type(Repr::String),
            Type(Repr::Accessory(kind)),
        ])))
    }

    /// This type with its strings refined by `kind`: a `string`, bare or
    /// already refined, gains it, in a union too; other members are left
    /// as they are.
    pub fn with_string_accessory(&self, kind: AccessoryKind) -> Type {
        match self {
            Type(Repr::String) => Type(Repr::Intersection(vec![
                Type(Repr::String),
                Type(Repr::Accessory(kind)),
            ])),
            Type(Repr::Intersection(members))
                if members.contains(&Type(Repr::String))
                    && !members.contains(&Type(Repr::Accessory(kind))) =>
            {
                let mut members = members.clone();
                members.push(Type(Repr::Accessory(kind)));
                Type(Repr::Intersection(members))
            }
            Type(Repr::Union(members)) => Type(Repr::Union(
                members
                    .iter()
                    .map(|m| m.with_string_accessory(kind))
                    .collect(),
            )),
            other => other.clone(),
        }
    }

    /// The `(min, max)` of an integer literal or range, `None` bounds
    /// unbounded; `None` for any other type.
    pub fn int_range_bounds(&self) -> Option<(Option<i64>, Option<i64>)> {
        match self {
            Type(Repr::IntLiteral(n)) => Some((Some(*n), Some(*n))),
            Type(Repr::IntRange(min, max)) => Some((*min, *max)),
            _ => None,
        }
    }

    /// PHPStan's `isArray()`.
    pub fn is_array(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::Array(
            Box::new(Type(Repr::Mixed(false))),
            Box::new(Type(Repr::Mixed(false))),
        )))
    }

    /// PHPStan's `isObject()`.
    pub fn is_object(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::ObjectWithoutClass))
    }

    /// PHPStan's `isScalar()`: a `bool`, `int`, `float` or `string`.
    pub fn is_scalar(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::Union(vec![
            Type(Repr::Bool),
            Type(Repr::Int),
            Type(Repr::Float),
            Type(Repr::String),
        ])))
    }

    /// PHPStan's `isList()`.
    pub fn is_list(&self) -> TrinaryLogic {
        self.is_within(&Type::list_of(Type(Repr::Mixed(false))))
    }

    /// Whether every value is a non-empty array.
    pub fn is_non_empty_array(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::Intersection(vec![
            Type(Repr::Array(
                Box::new(Type(Repr::Mixed(false))),
                Box::new(Type(Repr::Mixed(false))),
            )),
            Type(Repr::Accessory(AccessoryKind::NonEmptyArray)),
        ])))
    }

    /// Whether every value is the empty array.
    pub fn is_empty_array(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::ArrayShape(Shape::sealed(Vec::new()))))
    }

    /// PHPStan's `getIterableValueType()` for a type every value of which
    /// is an array; `None` otherwise. `array{}` holds no value (`never`).
    pub fn array_value_type(&self) -> Option<Type> {
        match self {
            Type(Repr::Array(_, value)) => Some((**value).clone()),
            Type(Repr::ArrayShape(shape)) => Some(
                shape
                    .fields
                    .iter()
                    .map(|f| f.ty.clone())
                    .chain(shape.unsealed.as_deref().map(|tail| tail.1.clone()))
                    .reduce(union_types)
                    .unwrap_or(Type(Repr::Never)),
            ),
            Type(Repr::Intersection(members)) => members.iter().find_map(Type::array_value_type),
            Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members)) => members
                .iter()
                .map(Type::array_value_type)
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .reduce(union_types),
            Type(Repr::TemplateParam(param)) => param.bound.array_value_type(),
            _ => None,
        }
    }

    /// PHPStan's `getIterableKeyType()` for a type every value of which
    /// is an array; `None` otherwise. `array{}` holds no key (`never`).
    pub fn array_key_type(&self) -> Option<Type> {
        match self {
            Type(Repr::Array(key, _)) => Some((**key).clone()),
            Type(Repr::ArrayShape(shape)) => {
                let keys = crate::shape_key_type(shape);
                Some(match shape.unsealed.as_deref() {
                    Some(tail) => union_types(keys, tail.0.clone()),
                    None => keys,
                })
            }
            // A list keys on `int<0, max>` whatever its array member says.
            Type(Repr::Intersection(members)) => {
                let key = members.iter().find_map(Type::array_key_type)?;
                if members.contains(&Type(Repr::Accessory(AccessoryKind::ArrayList))) {
                    Some(crate::intersect_types(vec![
                        key,
                        Type(Repr::IntRange(Some(0), None)),
                    ]))
                } else {
                    Some(key)
                }
            }
            Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members)) => members
                .iter()
                .map(Type::array_key_type)
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .reduce(union_types),
            Type(Repr::TemplateParam(param)) => param.bound.array_key_type(),
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
                Type(Repr::ArrayShape(shape)) if shape.is_sealed() => Some(shape),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default()
    }

    /// The value of an integer literal; `None` for any other type.
    pub fn int_literal_value(&self) -> Option<i64> {
        match self {
            Type(Repr::IntLiteral(n)) => Some(*n),
            _ => None,
        }
    }

    /// The value of a string literal; `None` for any other type.
    pub fn string_literal_value(&self) -> Option<&crate::PhpString> {
        match self {
            Type(Repr::StringLiteral(s)) => Some(s),
            _ => None,
        }
    }

    /// The class and generic arguments of an object of a named class;
    /// `None` for any other type.
    pub fn object_class_and_args(&self) -> Option<(&crate::Name, &[Type])> {
        match self {
            Type(Repr::Object(class, args)) => Some((class, args)),
            _ => None,
        }
    }

    /// The class an instance of this type belongs to, with its generic
    /// arguments: an object's, or an enum case's enum (with none).
    /// `None` for any other type.
    pub fn instance_class_and_args(&self) -> Option<(&crate::Name, &[Type])> {
        match self {
            Type(Repr::Object(class, args)) => Some((class, args)),
            Type(Repr::EnumCase(class, _)) => Some((class, &[])),
            _ => None,
        }
    }

    /// The value of `true` or `false`; `None` for any other type.
    pub fn bool_literal_value(&self) -> Option<bool> {
        match self {
            Type(Repr::True) => Some(true),
            Type(Repr::False) => Some(false),
            _ => None,
        }
    }

    /// PHPStan's `toBoolean()`: `true` when every value is truthy, `false`
    /// when every value is falsy, `bool` otherwise.
    pub fn to_boolean(&self) -> Type {
        match self {
            Type(Repr::True) => Type(Repr::True),
            Type(Repr::False) | Type(Repr::Null) | Type(Repr::Void) => Type(Repr::False),
            Type(Repr::IntLiteral(n)) => Type::bool_literal(*n != 0),
            Type(Repr::FloatLiteral(n)) => Type::bool_literal(*n != 0.0),
            Type(Repr::StringLiteral(s)) => Type::bool_literal(!s.is_empty() && s != "0"),
            Type(Repr::IntRange(min, max)) => {
                if min.is_some_and(|m| m > 0) || max.is_some_and(|m| m < 0) {
                    Type(Repr::True)
                } else if *min == Some(0) && *max == Some(0) {
                    Type(Repr::False)
                } else {
                    Type(Repr::Bool)
                }
            }
            Type(Repr::Object(..))
            | Type(Repr::ObjectWithoutClass)
            | Type(Repr::ObjectShape(_))
            | Type(Repr::Resource { .. })
            | Type(Repr::EnumCase(..))
            | Type(Repr::This)
            | Type(Repr::StaticRef(_))
            | Type(Repr::SelfRef(_))
            | Type(Repr::ParentRef)
            | Type(Repr::ClassString(_))
            | Type(Repr::ClassKindString(_))
            | Type(Repr::HasOffset(_))
            | Type(Repr::HasOffsetValue(..))
            | Type(Repr::HasMethod(_))
            | Type(Repr::HasProperty(_)) => Type(Repr::True),
            Type(Repr::ArrayShape(shape)) => {
                if shape.fields.iter().any(|f| !f.optional) {
                    Type(Repr::True)
                } else if shape.is_empty_array() {
                    Type(Repr::False)
                } else {
                    Type(Repr::Bool)
                }
            }
            Type(Repr::Accessory(AccessoryKind::NonFalsyString | AccessoryKind::NonEmptyArray)) => {
                Type(Repr::True)
            }
            Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members)) => members
                .iter()
                .map(Type::to_boolean)
                .reduce(union_types)
                .unwrap_or(Type(Repr::Never)),
            Type(Repr::Intersection(members)) => {
                crate::intersect_types(members.iter().map(Type::to_boolean).collect())
            }
            Type(Repr::TemplateParam(param)) => param.bound.to_boolean(),
            Type(Repr::Never) => Type(Repr::Never),
            _ => Type(Repr::Bool),
        }
    }
}
