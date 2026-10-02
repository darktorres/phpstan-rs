//! Building a `Type`: one constructor per kind of type, taking its parts.

use std::sync::Arc;

use crate::{
    AccessoryKind, CallSiteVariance, CallableSig, ClassKind, ConditionalSubject, ExtensionTypeData,
    Name, PhpString, Repr, Shape, ShapeKey, TemplateParam, Type,
};

impl Type {
    /// `mixed` with no type information behind it.
    pub fn mixed() -> Type {
        Type::new(Repr::Mixed(false, None))
    }

    /// `mixed` as a type written somewhere resolved to it.
    pub fn explicit_mixed() -> Type {
        Type::new(Repr::Mixed(true, None))
    }

    /// `mixed`, explicit or not.
    pub fn mixed_with(explicit: bool) -> Type {
        Type::new(Repr::Mixed(explicit, None))
    }

    pub fn error() -> Type {
        Type::new(Repr::Error)
    }

    pub fn never() -> Type {
        Type::new(Repr::Never)
    }

    pub fn void() -> Type {
        Type::new(Repr::Void)
    }

    pub fn null() -> Type {
        Type::new(Repr::Null)
    }

    pub fn bool() -> Type {
        Type::new(Repr::Bool)
    }

    /// `true` or `false`.
    pub fn bool_literal(value: bool) -> Type {
        if value {
            Type::new(Repr::True)
        } else {
            Type::new(Repr::False)
        }
    }

    pub fn int() -> Type {
        Type::new(Repr::Int)
    }

    pub fn int_literal(value: i64) -> Type {
        Type::new(Repr::IntLiteral(value))
    }

    /// The integers from `min` to `max` (`None` unbounded) as the
    /// narrowest type: `int` when unbounded both ways, a literal when
    /// they meet, `never` when they cross (PHPStan's
    /// `IntegerRangeType::fromInterval`).
    pub fn int_range(min: Option<i64>, max: Option<i64>) -> Type {
        match (min, max) {
            (None, None) => Type::new(Repr::Int),
            (Some(a), Some(b)) if a > b => Type::new(Repr::Never),
            (Some(a), Some(b)) if a == b => Type::new(Repr::IntLiteral(a)),
            (min, max) => Type::new(Repr::IntRange(min, max)),
        }
    }

    pub fn float() -> Type {
        Type::new(Repr::Float)
    }

    pub fn float_literal(value: f64) -> Type {
        Type::new(Repr::FloatLiteral(value))
    }

    pub fn string() -> Type {
        Type::new(Repr::String)
    }

    pub fn string_literal(value: PhpString) -> Type {
        Type::new(Repr::StringLiteral(value))
    }

    /// `class-string<inner>`.
    pub fn class_string(inner: Type) -> Type {
        Type::new(Repr::ClassString(inner))
    }

    /// `interface-string`, `trait-string` or `enum-string`.
    pub fn class_kind_string(kind: ClassKind) -> Type {
        Type::new(Repr::ClassKindString(kind))
    }

    /// `array<key, value>`.
    pub fn array(key: Type, value: Type) -> Type {
        Type::new(Repr::Array(key, value))
    }

    pub fn array_shape(shape: Shape) -> Type {
        Type::new(Repr::ArrayShape(Arc::new(shape)))
    }

    pub fn object_shape(shape: Shape) -> Type {
        Type::new(Repr::ObjectShape(Arc::new(shape)))
    }

    /// An instance of `class`, with its generic arguments.
    pub fn object(class: Name, args: Vec<Type>) -> Type {
        Type::new(Repr::Object(class, args))
    }

    /// `object`.
    pub fn object_without_class() -> Type {
        Type::new(Repr::ObjectWithoutClass)
    }

    pub fn resource(closed: bool) -> Type {
        Type::new(Repr::Resource { closed })
    }

    /// The case `case` of the enum `class`.
    pub fn enum_case(class: Name, case: Name) -> Type {
        Type::new(Repr::EnumCase(class, case))
    }

    /// `callable`, or a callable with a known signature.
    pub fn callable(sig: Option<Arc<CallableSig>>) -> Type {
        Type::new(Repr::Callable(sig))
    }

    pub fn self_ref(args: Vec<Type>) -> Type {
        Type::new(Repr::SelfRef(args))
    }

    pub fn parent_ref() -> Type {
        Type::new(Repr::ParentRef)
    }

    pub fn static_ref(args: Vec<Type>) -> Type {
        Type::new(Repr::StaticRef(args))
    }

    /// `$this`.
    pub fn this() -> Type {
        Type::new(Repr::This)
    }

    pub fn template_param(param: TemplateParam) -> Type {
        Type::new(Repr::TemplateParam(param))
    }

    /// A generic argument written with a call-site variance.
    pub fn variance_arg(variance: CallSiteVariance, arg: Type) -> Type {
        Type::new(Repr::VarianceArg(variance, arg))
    }

    /// The union of `members`: nested unions flattened, members another
    /// covers absorbed; `never` for none.
    pub fn union_of(members: Vec<Type>) -> Type {
        members
            .into_iter()
            .reduce(Type::union)
            .unwrap_or(Type::new(Repr::Never))
    }

    /// The union of `members`, benevolent: a value checked against it
    /// passes when it passes against any member.
    pub fn benevolent_union_of(members: Vec<Type>) -> Type {
        match Type::union_of(members).into_repr() {
            Repr::Union(members) => Type::new(Repr::BenevolentUnion(members)),
            other => Type::new(other),
        }
    }

    /// The intersection of `members`, with no class knowledge (see
    /// `Relation::intersect`).
    pub fn intersection_of(members: Vec<Type>) -> Type {
        crate::intersect_types(members)
    }

    pub fn accessory(kind: AccessoryKind) -> Type {
        Type::new(Repr::Accessory(kind))
    }

    /// `hasOffsetValue(key, value)`.
    pub fn has_offset_value(key: ShapeKey, value: Type) -> Type {
        Type::new(Repr::HasOffsetValue(key, value))
    }

    /// `hasOffset(key)`.
    pub fn has_offset(key: ShapeKey) -> Type {
        Type::new(Repr::HasOffset(key))
    }

    pub fn has_method(method: Name) -> Type {
        Type::new(Repr::HasMethod(method))
    }

    pub fn has_property(property: Name) -> Type {
        Type::new(Repr::HasProperty(property))
    }

    /// The member `member` of `owner`'s reflected class.
    pub fn reflected_member(owner: Type, member: Name) -> Type {
        Type::new(Repr::ReflectedMember(owner, member))
    }

    /// `value-of<of>`.
    pub fn value_of(of: Type) -> Type {
        Type::new(Repr::ValueOf(of))
    }

    /// `key-of<of>`.
    pub fn key_of(of: Type) -> Type {
        Type::new(Repr::KeyOf(of))
    }

    /// `container[offset]`.
    pub fn offset_access(container: Type, offset: Type) -> Type {
        Type::new(Repr::OffsetAccess(container, offset))
    }

    /// The constants of `class` matching `pattern` (`Foo::BAR_*`).
    pub fn class_constant(class: Type, pattern: Name) -> Type {
        Type::new(Repr::ClassConstant { class, pattern })
    }

    pub fn extension(data: Arc<dyn ExtensionTypeData>) -> Type {
        Type::new(Repr::Extension(data))
    }

    /// `(subject is target ? if_true : if_false)`, `is not` when
    /// `negated`.
    pub fn conditional(
        subject: ConditionalSubject,
        target: Type,
        if_true: Type,
        if_false: Type,
        negated: bool,
    ) -> Type {
        Type::new(Repr::Conditional {
            subject,
            target,
            if_true,
            if_false,
            negated,
        })
    }
}

impl Type {
    /// The union of `members`, of the same kind as `self`: benevolent
    /// when `self` is a benevolent union, plain otherwise.
    pub fn with_members(&self, members: Vec<Type>) -> Type {
        match self.repr() {
            Repr::BenevolentUnion(_) => Type::benevolent_union_of(members),
            _ => Type::union_of(members),
        }
    }

    /// A closure/arrow-fn literal's own value: a real `Closure` instance
    /// (so `->bindTo()`/`->call()`/`instanceof Closure` keep resolving
    /// against the `Closure` class) that also carries the literal's own
    /// signature - real PHPStan's `ClosureType`, which is both at once.
    /// `Display` renders this exact pair as `Closure(params): return`.
    pub fn closure_literal(sig: CallableSig) -> Type {
        Type::new(Repr::Intersection(vec![
            Type::new(Repr::Object("Closure".into(), Vec::new())),
            Type::new(Repr::Callable(Some(Arc::new(sig)))),
        ]))
    }

    /// `list<element>`: an array whose keys are `0..n` in order.
    pub fn list_of(element: Type) -> Type {
        crate::intersect_types(vec![
            Type::array(Type::unknown_array_key(), element),
            Type::accessory(AccessoryKind::ArrayList),
        ])
    }

    /// PHP's full legal-array-key domain (`int|string`) - the default for
    /// a bare `array`/`array<V>` with no key argument given.
    pub fn unknown_array_key() -> Type {
        static KEY: std::sync::LazyLock<Type> = std::sync::LazyLock::new(|| {
            Type::new(Repr::Union(vec![
                Type::new(Repr::Int),
                Type::new(Repr::String),
            ]))
        });
        KEY.clone()
    }

    /// `Repr::Array(unknown_array_key(), value)`, for a construction site
    /// with no more precise key type available.
    pub fn array_of(value: Type) -> Type {
        Type::new(Repr::Array(Type::unknown_array_key(), value))
    }

    /// The `int|string` key type PHPDoc's `Type[]` array-shorthand
    /// (`Item[]`), the single-generic-arg `array<V>` form, and the
    /// array-union `+` operator's own "neither side had a more precise
    /// key" fallback all use - a real `BenevolentUnionType([Int,
    /// String])` marker (mirrors real PHPStan's
    /// `TypeNodeResolver::resolveArrayTypeNode`/`resolveGenericTypeNode`'s
    /// own `array<V>` arm/`InitializerExprTypeResolver::getPlusType`,
    /// confirmed reading all three directly), *not* the plain, strict
    /// `unknown_array_key()`. `RuleLevelHelper::accepts()` treats a
    /// benevolent union specially - accepted if *any* member matches,
    /// not only when *all* do - the real difference: a `@var Item[]`
    /// foreach-key passed to a `string`-typed parameter produces zero
    /// errors in real PHPStan (confirmed via `dumpType()`), unlike a
    /// plain, strict `int|string` checked the same way. Deliberately a
    /// *narrower* helper than `unknown_array_key()`, not a blanket
    /// replacement for it: real PHPStan's own benevolence for a bare
    /// `array` (no generic args at all)'s *stored* `Mixed` key type is a
    /// separate, read-time-only substitution
    /// (`ArrayType::getIterableKeyType()`) keyed on a distinction
    /// (`Mixed` vs. an already-real key type) this project's own eagerly-
    /// constructed `unknown_array_key()` has no way to preserve, and a
    /// 2-arg `array<K, V>` with an explicit key never gets the marker
    /// either - see this function's own real call sites (`types/parse.rs`'s
    /// `Type[]`-shorthand and `array<V>` parses, `checker/expr.rs`'s `+`
    /// operator fallback) for exactly which real-PHPStan-confirmed
    /// construction sites this marker belongs at, not every "no more
    /// precise key" site project-wide.
    pub fn benevolent_array_key() -> Type {
        Type::new(Repr::BenevolentUnion(vec![
            Type::new(Repr::Int),
            Type::new(Repr::String),
        ]))
    }
}
