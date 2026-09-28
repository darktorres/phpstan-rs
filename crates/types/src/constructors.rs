//! Building a `Type`: one constructor per kind of type, taking its parts.

use std::sync::Arc;

use crate::{
    AccessoryKind, CallSiteVariance, CallableSig, ClassKind, ExtensionTypeData, Name, PhpString,
    Shape, ShapeKey, TemplateParam, Type,
};

impl Type {
    /// `mixed` with no type information behind it.
    pub const fn mixed() -> Type {
        Type::Mixed(false)
    }

    /// `mixed` as a type written somewhere resolved to it.
    pub const fn explicit_mixed() -> Type {
        Type::Mixed(true)
    }

    /// `mixed`, explicit or not.
    pub const fn mixed_with(explicit: bool) -> Type {
        Type::Mixed(explicit)
    }

    pub const fn error() -> Type {
        Type::Error
    }

    pub const fn never() -> Type {
        Type::Never
    }

    pub const fn void() -> Type {
        Type::Void
    }

    pub const fn null() -> Type {
        Type::Null
    }

    pub const fn bool() -> Type {
        Type::Bool
    }

    /// `true` or `false`.
    pub const fn bool_literal(value: bool) -> Type {
        if value { Type::True } else { Type::False }
    }

    pub const fn int() -> Type {
        Type::Int
    }

    pub const fn int_literal(value: i64) -> Type {
        Type::IntLiteral(value)
    }

    /// `int<min, max>`, `None` unbounded.
    pub const fn int_range(min: Option<i64>, max: Option<i64>) -> Type {
        Type::IntRange(min, max)
    }

    /// The integers from `min` to `max` as the narrowest type: `int` when
    /// unbounded both ways, a literal when they meet, `never` when they
    /// cross (PHPStan's `IntegerRangeType::fromInterval`).
    pub fn int_interval(min: Option<i64>, max: Option<i64>) -> Type {
        match (min, max) {
            (None, None) => Type::Int,
            (Some(a), Some(b)) if a > b => Type::Never,
            (Some(a), Some(b)) if a == b => Type::IntLiteral(a),
            (min, max) => Type::IntRange(min, max),
        }
    }

    pub const fn float() -> Type {
        Type::Float
    }

    pub const fn float_literal(value: f64) -> Type {
        Type::FloatLiteral(value)
    }

    pub const fn string() -> Type {
        Type::String
    }

    pub fn string_literal(value: PhpString) -> Type {
        Type::StringLiteral(value)
    }

    /// `class-string<inner>`.
    pub fn class_string(inner: Type) -> Type {
        Type::ClassString(Box::new(inner))
    }

    /// `interface-string`, `trait-string` or `enum-string`.
    pub const fn class_kind_string(kind: ClassKind) -> Type {
        Type::ClassKindString(kind)
    }

    /// `array<key, value>`.
    pub fn array(key: Type, value: Type) -> Type {
        Type::Array(Box::new(key), Box::new(value))
    }

    pub fn array_shape(shape: Shape) -> Type {
        Type::ArrayShape(shape)
    }

    pub fn object_shape(shape: Shape) -> Type {
        Type::ObjectShape(shape)
    }

    /// An instance of `class`, with its generic arguments.
    pub fn object(class: Name, args: Vec<Type>) -> Type {
        Type::Object(class, args)
    }

    /// `object`.
    pub const fn object_without_class() -> Type {
        Type::ObjectWithoutClass
    }

    pub const fn resource(closed: bool) -> Type {
        Type::Resource { closed }
    }

    /// The case `case` of the enum `class`.
    pub fn enum_case(class: Name, case: Name) -> Type {
        Type::EnumCase(class, case)
    }

    /// `callable`, or a callable with a known signature.
    pub fn callable(sig: Option<Arc<CallableSig>>) -> Type {
        Type::Callable(sig)
    }

    pub fn self_ref(args: Vec<Type>) -> Type {
        Type::SelfRef(args)
    }

    pub const fn parent_ref() -> Type {
        Type::ParentRef
    }

    pub fn static_ref(args: Vec<Type>) -> Type {
        Type::StaticRef(args)
    }

    /// `$this`.
    pub const fn this() -> Type {
        Type::This
    }

    pub fn template_param(param: TemplateParam) -> Type {
        Type::TemplateParam(param)
    }

    /// A generic argument written with a call-site variance.
    pub fn variance_arg(variance: CallSiteVariance, arg: Type) -> Type {
        Type::VarianceArg(variance, Box::new(arg))
    }

    /// `members` as one union, taken as they are; `union_types` normalizes.
    pub fn union_of(members: Vec<Type>) -> Type {
        Type::Union(members)
    }

    /// `members` as one benevolent union, taken as they are.
    pub fn benevolent_union_of(members: Vec<Type>) -> Type {
        Type::BenevolentUnion(members)
    }

    /// `members` as one intersection, taken as they are;
    /// `Relation::intersect` reduces.
    pub fn intersection_of(members: Vec<Type>) -> Type {
        Type::Intersection(members)
    }

    pub const fn accessory(kind: AccessoryKind) -> Type {
        Type::Accessory(kind)
    }

    /// `hasOffsetValue(key, value)`.
    pub fn has_offset_value(key: ShapeKey, value: Type) -> Type {
        Type::HasOffsetValue(key, Box::new(value))
    }

    /// `hasOffset(key)`.
    pub fn has_offset(key: ShapeKey) -> Type {
        Type::HasOffset(key)
    }

    pub fn has_method(method: Name) -> Type {
        Type::HasMethod(method)
    }

    pub fn has_property(property: Name) -> Type {
        Type::HasProperty(property)
    }

    /// The member `member` of `owner`'s reflected class.
    pub fn reflected_member(owner: Type, member: String) -> Type {
        Type::ReflectedMember(Box::new(owner), member)
    }

    /// `value-of<of>`.
    pub fn value_of(of: Type) -> Type {
        Type::ValueOf(Box::new(of))
    }

    /// `key-of<of>`.
    pub fn key_of(of: Type) -> Type {
        Type::KeyOf(Box::new(of))
    }

    /// `container[offset]`.
    pub fn offset_access(container: Type, offset: Type) -> Type {
        Type::OffsetAccess(Box::new(container), Box::new(offset))
    }

    /// The constants of `class` matching `pattern` (`Foo::BAR_*`).
    pub fn class_constant(class: Type, pattern: String) -> Type {
        Type::ClassConstant {
            class: Box::new(class),
            pattern,
        }
    }

    pub fn extension(data: Arc<dyn ExtensionTypeData>) -> Type {
        Type::Extension(data)
    }

    /// `(subject is target ? if_true : if_false)`, `is not` when
    /// `negated`; `subject` names a template when `template_subject`,
    /// a parameter otherwise.
    pub fn conditional(
        subject: String,
        target: Type,
        if_true: Type,
        if_false: Type,
        negated: bool,
        template_subject: bool,
    ) -> Type {
        Type::Conditional {
            subject,
            target: Box::new(target),
            if_true: Box::new(if_true),
            if_false: Box::new(if_false),
            negated,
            template_subject,
        }
    }
}
