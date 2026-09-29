//! What a type is: the questions asked of a type from outside this crate.

use std::sync::Arc;

use crate::{
    AccessoryKind, CallSiteVariance, CallableSig, ConditionalSubject, Relation, Repr, Shape,
    TrinaryLogic, Type,
};

impl Type {
    /// Whether this is `mixed`, written or not.
    pub fn is_mixed(&self) -> bool {
        matches!(self, Type(Repr::Mixed(..)))
    }

    /// Whether this is the error type: an expression already reported
    /// invalid, which nothing reports on again.
    pub fn is_error(&self) -> bool {
        matches!(self, Type(Repr::Error))
    }

    /// Whether this is a `mixed` a written type resolved to.
    pub fn is_explicit_mixed(&self) -> bool {
        matches!(self, Type(Repr::Mixed(true, _)))
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

    /// The class of every object this type refers to, at any depth:
    /// union and intersection members, array keys and values, shape
    /// fields, class-strings, generic arguments and callable signatures.
    /// Each once, in order.
    pub fn referenced_class_names(&self) -> Vec<&crate::Name> {
        let mut out: Vec<&crate::Name> = Vec::new();
        self.walk_nested(&mut |t| {
            if let Type(Repr::Object(name, _)) = t
                && !out.contains(&name)
            {
                out.push(name);
            }
        });
        out
    }

    /// Whether a `parent` stands anywhere in this type unresolved.
    pub fn has_parent_ref(&self) -> bool {
        let mut found = false;
        self.walk_nested(&mut |t| found |= matches!(t, Type(Repr::ParentRef)));
        found
    }

    /// `f` on this type and every type nested in it, outermost first.
    fn walk_nested<'a>(&'a self, f: &mut impl FnMut(&'a Type)) {
        syntax::stack::guard(|| {
            f(self);
            self.for_each_child(&mut |t| t.walk_nested(f));
        })
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
            ("mixed", Type(Repr::Mixed(..))) => true,
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

    /// Whether `@template T of <this>` can bound a template: any type
    /// that describes values, but not `void` or `never` (which describe
    /// none), an error, a conditional (which a call-site parameter
    /// decides) or a bare refinement. A union or intersection can when
    /// each of its members can.
    pub fn can_bound_a_template(&self) -> bool {
        match self {
            Type(Repr::Void)
            | Type(Repr::Never)
            | Type(Repr::Error)
            | Type(Repr::Conditional { .. }) => false,
            Type(Repr::Union(members))
            | Type(Repr::BenevolentUnion(members))
            | Type(Repr::Intersection(members)) => members
                .iter()
                .all(|m| m.is_accessory() || m.can_bound_a_template()),
            other => !other.is_accessory(),
        }
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
    pub fn as_object_shape(&self) -> Option<&Shape> {
        match self {
            Type(Repr::ObjectShape(shape)) => Some(shape),
            Type(Repr::Intersection(members)) => members.iter().find_map(Type::as_object_shape),
            _ => None,
        }
    }

    /// The array shape this type is, or an intersection refines.
    pub fn as_array_shape(&self) -> Option<&Shape> {
        match self {
            Type(Repr::ArrayShape(shape)) => Some(shape),
            Type(Repr::Intersection(members)) => members.iter().find_map(Type::as_array_shape),
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

    /// A conditional type's condition: its subject, its target, and whether
    /// it's negated (`is not`). `None` for any other type.
    pub fn as_conditional(&self) -> Option<(&ConditionalSubject, &Type, bool)> {
        match self {
            Type(Repr::Conditional {
                subject,
                target,
                negated,
                ..
            }) => Some((subject, target, *negated)),
            _ => None,
        }
    }

    /// Whether this is `void`.
    pub fn is_void(&self) -> bool {
        matches!(self, Type(Repr::Void))
    }

    /// An enum case's enum and case name; `None` for any other type.
    pub fn as_enum_case(&self) -> Option<(&crate::Name, &crate::Name)> {
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
        syntax::stack::guard(|| {
            const LIMIT: i64 = 256;
            match self {
                Type(Repr::IntLiteral(_))
                | Type(Repr::StringLiteral(_))
                | Type(Repr::FloatLiteral(_))
                | Type(Repr::True)
                | Type(Repr::False)
                | Type(Repr::Null)
                | Type(Repr::EnumCase(..)) => vec![self.clone()],
                Type(Repr::Bool) => vec![Type::bool_literal(true), Type::bool_literal(false)],
                Type(Repr::IntRange(Some(min), Some(max)))
                    if i128::from(*max) - i128::from(*min) < i128::from(LIMIT) =>
                {
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
        })
    }

    /// The loop widening of this type as it grows into `next`: an int
    /// keeps a bound that stayed put and opens the one that moved
    /// (`0` then `0|1` is `int<0, max>`); anything else is `next`
    /// generalized.
    pub fn widen_towards(&self, next: &Type) -> Type {
        /// The smallest range holding every member, when all are ints.
        fn hull(ty: &Type) -> Option<(Option<i64>, Option<i64>)> {
            ty.union_members()
                .iter()
                .map(Type::int_bounds)
                .reduce(|a, b| {
                    let ((a_min, a_max), (b_min, b_max)) = (a?, b?);
                    Some((
                        a_min.zip(b_min).map(|(x, y)| x.min(y)),
                        a_max.zip(b_max).map(|(x, y)| x.max(y)),
                    ))
                })
                .flatten()
        }
        match (hull(self), hull(next)) {
            (Some((min, max)), Some((next_min, next_max))) => Type::int_range(
                (next_min == min).then_some(min).flatten(),
                (next_max == max).then_some(max).flatten(),
            ),
            _ => next.generalize(),
        }
    }

    /// PHPStan's `generalize(templateArgument)`: `generalize`, a
    /// non-empty array no longer known non-empty - an inferred type
    /// argument (`new Box([1, 2])` is `Box<list<int>>`) must still accept
    /// an empty one.
    pub fn generalize_template_argument(&self) -> Type {
        match self.generalize().into_repr() {
            Repr::Intersection(members) => crate::intersect_types(
                members
                    .into_iter()
                    .filter(|m| !matches!(m, Type(Repr::Accessory(AccessoryKind::NonEmptyArray))))
                    .collect(),
            ),
            Repr::Union(members) => Type::union_of(
                members
                    .iter()
                    .map(Type::generalize_template_argument)
                    .collect(),
            ),
            other => Type(other),
        }
    }

    /// PHPStan's `generalize(lessSpecific)`: literals and ranges widen to
    /// their base type, an enum case to its class, a shape to the array
    /// of its generalized keys and values (still a list, still non-empty,
    /// when it was), and a union's members each.
    pub fn generalize(&self) -> Type {
        syntax::stack::guard(|| match self {
            Type(Repr::IntLiteral(_)) | Type(Repr::IntRange(..)) => Type::int(),
            Type(Repr::StringLiteral(_)) => Type::string(),
            Type(Repr::FloatLiteral(_)) => Type::float(),
            Type(Repr::True) | Type(Repr::False) => Type::bool(),
            Type(Repr::EnumCase(class, _)) => Type::object(class.clone(), Vec::new()),
            Type(Repr::ArrayShape(shape)) => {
                let key = self
                    .array_key_type()
                    .map_or(Type::unknown_array_key(), |k| k.generalize());
                let value = self
                    .array_value_type()
                    .map_or(Type::mixed(), |v| v.generalize());
                let (key, value) = if value.is_never() {
                    (Type::unknown_array_key(), Type::mixed())
                } else {
                    (key, value)
                };
                let mut members = vec![Type::array(key, value)];
                if shape.is_list() && !shape.fields.is_empty() {
                    members.push(Type::accessory(AccessoryKind::ArrayList));
                }
                if shape.fields.iter().any(|f| !f.optional) {
                    members.push(Type::accessory(AccessoryKind::NonEmptyArray));
                }
                crate::intersect_types(members)
            }
            Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members)) => members
                .iter()
                .map(Type::generalize)
                .reduce(Type::union)
                .unwrap_or(Type::never()),
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
        })
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
            Type(Repr::ClassString(inner)) => inner.as_object().map(|(class, _)| class),
            _ => None,
        }
    }

    /// A reflected member's owner type and member name (a
    /// `ReflectionMethod`/`ReflectionProperty` bound to one member);
    /// `None` for any other type.
    pub fn as_reflected_member(&self) -> Option<(&Type, &str)> {
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
        matches!(self, Type(Repr::Mixed(false, None)))
    }

    /// Whether every value of this type is one of `of` (`Yes`), none is
    /// (`No`), or some are (`Maybe`). `never` is no kind of value.
    pub(crate) fn is_within(&self, of: &Type) -> TrinaryLogic {
        if matches!(self, Type(Repr::Never)) {
            return TrinaryLogic::No;
        }
        Relation::structural().is_super_type_of(of, self)
    }

    /// PHPStan's `isNull()`.
    pub fn is_null(&self) -> TrinaryLogic {
        self.is_within(&Type::null())
    }

    /// PHPStan's `isTrue()`.
    pub fn is_true(&self) -> TrinaryLogic {
        self.is_within(&Type::bool_literal(true))
    }

    /// PHPStan's `isFalse()`.
    pub fn is_false(&self) -> TrinaryLogic {
        self.is_within(&Type::bool_literal(false))
    }

    /// PHPStan's `isBoolean()`.
    pub fn is_bool(&self) -> TrinaryLogic {
        self.is_within(&Type::bool())
    }

    /// PHPStan's `isInteger()`.
    pub fn is_int(&self) -> TrinaryLogic {
        self.is_within(&Type::int())
    }

    /// PHPStan's `isFloat()`.
    pub fn is_float(&self) -> TrinaryLogic {
        self.is_within(&Type::float())
    }

    /// PHPStan's `isString()`.
    pub fn is_string(&self) -> TrinaryLogic {
        self.is_within(&Type::string())
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
            Type::string(),
            Type::accessory(kind),
        ])))
    }

    /// This type with its strings refined by `kind`: a `string`, bare or
    /// already refined, gains it (replacing a refinement it implies, as
    /// `non-falsy` does `non-empty`), a string literal it rules out is
    /// `never`, in a union too; other members are left as they are.
    pub fn with_string_accessory(&self, kind: AccessoryKind) -> Type {
        // The refinement a stronger one implies, and the one it implies.
        let implied_by = |k: AccessoryKind| match k {
            AccessoryKind::NonFalsyString => Some(AccessoryKind::NonEmptyString),
            AccessoryKind::DecimalIntegerString => Some(AccessoryKind::NumericString),
            _ => None,
        };
        syntax::stack::guard(|| match self {
            Type(Repr::String) => {
                crate::intersect_types(vec![Type::string(), Type::accessory(kind)])
            }
            Type(Repr::StringLiteral(_)) => {
                let refined = Type::string().with_string_accessory(kind);
                if crate::Relation::structural()
                    .is_super_type_of(&refined, self)
                    .no()
                {
                    Type::never()
                } else {
                    self.clone()
                }
            }
            Type(Repr::Intersection(members)) if members.contains(&Type::string()) => {
                let already = |k: AccessoryKind| members.contains(&Type::accessory(k));
                if already(kind)
                    || [
                        AccessoryKind::NonFalsyString,
                        AccessoryKind::DecimalIntegerString,
                    ]
                    .into_iter()
                    .any(|stronger| already(stronger) && implied_by(stronger) == Some(kind))
                {
                    return self.clone();
                }
                let mut members: Vec<Type> = members
                    .iter()
                    .filter(|m| {
                        implied_by(kind).is_none_or(|weaker| **m != Type::accessory(weaker))
                    })
                    .cloned()
                    .collect();
                members.push(Type::accessory(kind));
                crate::intersect_types(members)
            }
            Type(Repr::Union(members)) => members
                .iter()
                .map(|m| m.with_string_accessory(kind))
                .reduce(Type::union)
                .unwrap_or_else(Type::never),
            other => other.clone(),
        })
    }

    /// The `(min, max)` of an integer type, `None` bounds unbounded
    /// (`int` is `int<min, max>`); `None` for any other type.
    pub fn int_bounds(&self) -> Option<(Option<i64>, Option<i64>)> {
        match self {
            Type(Repr::Int) => Some((None, None)),
            Type(Repr::IntLiteral(n)) => Some((Some(*n), Some(*n))),
            Type(Repr::IntRange(min, max)) => Some((*min, *max)),
            _ => None,
        }
    }

    /// PHPStan's `isArray()`.
    pub fn is_array(&self) -> TrinaryLogic {
        self.is_within(&Type::array(
            Type(Repr::Mixed(false, None)),
            Type(Repr::Mixed(false, None)),
        ))
    }

    /// PHPStan's `isObject()`.
    pub fn is_object(&self) -> TrinaryLogic {
        self.is_within(&Type::object_without_class())
    }

    /// Whether every value is a resource that is open (`closed` false) or
    /// has been closed (`closed` true).
    pub fn is_resource(&self, closed: bool) -> TrinaryLogic {
        self.is_within(&Type::resource(closed))
    }

    /// PHPStan's `isScalar()`: a `bool`, `int`, `float` or `string`.
    pub fn is_scalar(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::Union(vec![
            Type::bool(),
            Type::int(),
            Type::float(),
            Type::string(),
        ])))
    }

    /// PHPStan's `isList()`.
    pub fn is_list(&self) -> TrinaryLogic {
        self.is_within(&Type::list_of(Type::mixed()))
    }

    /// Whether every value is a non-empty array.
    pub fn is_non_empty_array(&self) -> TrinaryLogic {
        self.is_within(&Type(Repr::Intersection(vec![
            Type::array(
                Type(Repr::Mixed(false, None)),
                Type(Repr::Mixed(false, None)),
            ),
            Type::accessory(AccessoryKind::NonEmptyArray),
        ])))
    }

    /// Whether every value is the empty array.
    pub fn is_empty_array(&self) -> TrinaryLogic {
        self.is_within(&Type::array_shape(Shape::sealed(Vec::new())))
    }

    /// PHPStan's `getIterableValueType()` for a type every value of which
    /// is an array; `None` otherwise. `array{}` holds no value (`never`).
    pub fn array_value_type(&self) -> Option<Type> {
        syntax::stack::guard(|| match self {
            Type(Repr::Array(_, value)) => Some((**value).clone()),
            Type(Repr::ArrayShape(shape)) => Some(
                shape
                    .fields
                    .iter()
                    .map(|f| f.ty.clone())
                    .chain(shape.unsealed.as_deref().map(|tail| tail.1.clone()))
                    .reduce(Type::union)
                    .unwrap_or(Type::never()),
            ),
            Type(Repr::Intersection(members)) => members.iter().find_map(Type::array_value_type),
            Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members)) => members
                .iter()
                .map(Type::array_value_type)
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .reduce(Type::union),
            Type(Repr::TemplateParam(param)) => param.bound.array_value_type(),
            _ => None,
        })
    }

    /// PHPStan's `getIterableKeyType()` for a type every value of which
    /// is an array; `None` otherwise. `array{}` holds no key (`never`).
    pub fn array_key_type(&self) -> Option<Type> {
        syntax::stack::guard(|| {
            match self {
                Type(Repr::Array(key, _)) => Some((**key).clone()),
                Type(Repr::ArrayShape(shape)) => Some(shape.key_type()),
                // A list keys on `int<0, max>` whatever its array member says.
                Type(Repr::Intersection(members)) => {
                    let key = members.iter().find_map(Type::array_key_type)?;
                    if members.contains(&Type::accessory(AccessoryKind::ArrayList)) {
                        Some(crate::intersect_types(vec![
                            key,
                            Type::int_range(Some(0), None),
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
                    .reduce(Type::union),
                Type(Repr::TemplateParam(param)) => param.bound.array_key_type(),
                _ => None,
            }
        })
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
                Type(Repr::ArrayShape(shape)) if shape.is_sealed() => Some(shape.as_ref()),
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

    /// The class an instance of this type belongs to, with its generic
    /// arguments: an object's, or an enum case's enum (with none, an
    /// enum case being an object of its enum). `None` for any other
    /// type.
    pub fn as_object(&self) -> Option<(&crate::Name, &[Type])> {
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
        syntax::stack::guard(|| match self {
            Type(Repr::True) => Type::bool_literal(true),
            Type(Repr::False) | Type(Repr::Null) | Type(Repr::Void) => Type::bool_literal(false),
            Type(Repr::IntLiteral(n)) => Type::bool_literal(*n != 0),
            Type(Repr::FloatLiteral(n)) => Type::bool_literal(*n != 0.0),
            Type(Repr::StringLiteral(s)) => Type::bool_literal(!s.is_empty() && s != "0"),
            Type(Repr::IntRange(min, max)) => {
                if min.is_some_and(|m| m > 0) || max.is_some_and(|m| m < 0) {
                    Type::bool_literal(true)
                } else if *min == Some(0) && *max == Some(0) {
                    Type::bool_literal(false)
                } else {
                    Type::bool()
                }
            }
            // A `SimpleXMLElement` with no children is false.
            Type(Repr::Object(class, _)) if class.eq_ignore_ascii_case("SimpleXMLElement") => {
                Type::bool()
            }
            Type(Repr::Object(..))
            | Type(Repr::ObjectWithoutClass)
            | Type(Repr::ObjectShape(_))
            | Type(Repr::Callable(_))
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
            | Type(Repr::HasProperty(_)) => Type::bool_literal(true),
            Type(Repr::ArrayShape(shape)) => {
                if shape.fields.iter().any(|f| !f.optional) {
                    Type::bool_literal(true)
                } else if shape.is_empty_array() {
                    Type::bool_literal(false)
                } else {
                    Type::bool()
                }
            }
            Type(Repr::Accessory(AccessoryKind::NonFalsyString | AccessoryKind::NonEmptyArray)) => {
                Type::bool_literal(true)
            }
            Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members)) => members
                .iter()
                .map(Type::to_boolean)
                .reduce(Type::union)
                .unwrap_or(Type::never()),
            Type(Repr::Intersection(members)) => {
                crate::intersect_types(members.iter().map(Type::to_boolean).collect())
            }
            Type(Repr::TemplateParam(param)) => param.bound.to_boolean(),
            Type(Repr::Never) => Type::never(),
            _ => Type::bool(),
        })
    }
}

impl Type {
    /// `constant_strings` as text, a non-UTF-8 byte read as U+FFFD - for
    /// consumers that read a string's characters (a format, a path, a
    /// name).
    pub fn constant_strings_text(&self) -> Vec<std::borrow::Cow<'_, str>> {
        self.constant_strings()
            .into_iter()
            .map(crate::PhpString::to_str_lossy)
            .collect()
    }

    /// Every string value this type can only be: a string literal, each
    /// member's of an intersection, and a union's only when every member
    /// has some (PHPStan's `Type::getConstantStrings()`). Empty for
    /// anything else.
    pub fn constant_strings(&self) -> Vec<&crate::PhpString> {
        match self {
            Type(Repr::StringLiteral(s)) => vec![s],
            Type(Repr::Intersection(members)) => {
                members.iter().flat_map(Type::constant_strings).collect()
            }
            Type(Repr::Union(members)) => {
                let mut all = Vec::new();
                for member in members {
                    let strings = member.constant_strings();
                    if strings.is_empty() {
                        return Vec::new();
                    }
                    all.extend(strings);
                }
                all
            }
            _ => Vec::new(),
        }
    }

    /// Whether this is an accessory refinement - only ever an
    /// `Intersection` member alongside the base type it refines.
    pub fn is_accessory(&self) -> bool {
        matches!(
            self,
            Type(Repr::Accessory(_))
                | Type(Repr::HasOffsetValue(..))
                | Type(Repr::HasOffset(_))
                | Type(Repr::HasMethod(_))
                | Type(Repr::HasProperty(_))
        )
    }

    /// The signature a value of this type can be invoked with, when one
    /// is known: a shaped `Closure(...)`/`callable(...)` PHPDoc type, or a
    /// closure literal's `closure_literal` intersection.
    pub fn callable_sig(&self) -> Option<&Arc<CallableSig>> {
        match self {
            Type(Repr::Callable(Some(sig))) => Some(sig),
            Type(Repr::Intersection(members)) => members.iter().find_map(|m| match m {
                Type(Repr::Callable(Some(sig))) => Some(sig),
                _ => None,
            }),
            _ => None,
        }
    }
}
