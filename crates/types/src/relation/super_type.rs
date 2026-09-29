use crate::relation::Relation;
use crate::{AccessoryKind, Repr, Shape, TrinaryLogic, Type};
use syntax::ast::ClassKind;

impl Relation<'_> {
    /// Whether every value of `ty` (a template read through its bound) is
    /// an enum case: an enum, `UnitEnum` or anything extending it.
    fn is_enum_like(&self, ty: &Type) -> bool {
        match crate::late::upper_bound(ty).repr() {
            Repr::Object(name, _) => {
                let name = name.as_str().trim_start_matches('\\');
                name.eq_ignore_ascii_case("UnitEnum")
                    || self
                        .hierarchy
                        .find_class(name)
                        .is_some_and(|facts| facts.kind == ClassKind::Enum)
                    || self.hierarchy.is_subclass_of(name, "UnitEnum")
            }
            Repr::EnumCase(..) => true,
            _ => false,
        }
    }

    /// Whether no value is both an `a` and a `b`: `is_super_type_of`
    /// answers no one way round or the other.
    pub fn disjoint(&self, a: &Type, b: &Type) -> bool {
        self.is_super_type_of(a, b).no() || self.is_super_type_of(b, a).no()
    }

    /// Whether every value of `sub` is a `sup` (`Yes`), none is (`No`),
    /// or some are (`Maybe`) - real PHPStan's `isSuperTypeOf`, as the
    /// PHPDoc-vs-native rules read it. `Yes` needs a structural proof;
    /// `No` needs a proof (disjoint
    /// scalar families, distinct literals, or two unrelated classes
    /// neither of which can be extended into the other), and anything
    /// unprovable is `Maybe`, including any comparison with a class the
    /// registry doesn't know.
    pub fn is_super_type_of(&self, sup: &Type, sub: &Type) -> TrinaryLogic {
        // A literal, `true`, `false`, `null` or an enum case is one value:
        // one holds another only when they are the same.
        if is_scalar_value(sup) && is_scalar_value(sub) {
            return TrinaryLogic::from_bool(sup == sub);
        }
        syntax::stack::guard(|| {
            match (sup.repr(), sub.repr()) {
                (Repr::Error, _) => TrinaryLogic::Yes,
                (_, Repr::Error) => self.is_super_type_of(sup, &Type::mixed()),
                // `mixed~S` holds everything but `S`.
                (Repr::Mixed(_, Some(subtracted)), _) if !sub.is_never() => {
                    if self.is_super_type_of(subtracted, sub).yes() {
                        TrinaryLogic::No
                    } else if self.disjoint(subtracted, sub) {
                        TrinaryLogic::Yes
                    } else {
                        TrinaryLogic::Maybe
                    }
                }
                (Repr::Mixed(..), _) | (_, Repr::Never) => TrinaryLogic::Yes,
                // `NAN` is itself, as `ConstantFloatType::equals()` has it.
                (Repr::FloatLiteral(a), Repr::FloatLiteral(b)) if a.is_nan() && b.is_nan() => {
                    TrinaryLogic::Yes
                }
                _ if sup == sub => TrinaryLogic::Yes,
                // `A & B` is within whatever either member is.
                (_, Repr::Intersection(members))
                    if members.iter().any(|m| self.is_super_type_of(sup, m).yes()) =>
                {
                    TrinaryLogic::Yes
                }
                (Repr::Never, _) => TrinaryLogic::No,
                (Repr::KeyOf(_) | Repr::OffsetAccess(..), _) => {
                    self.is_super_type_of(&sup.resolve_late_to_bounds(), sub)
                }
                (Repr::Extension(a), Repr::Extension(b)) => a
                    .is_super_type_of(&**b, self.hierarchy)
                    .unwrap_or_else(|| self.is_super_type_of(&a.base(), &b.base())),
                // A plain base value may or may not carry the payload.
                (Repr::Extension(a), _) => match self.is_super_type_of(&a.base(), sub) {
                    TrinaryLogic::No => TrinaryLogic::No,
                    _ => TrinaryLogic::Maybe,
                },
                (_, Repr::Extension(b)) => self.is_super_type_of(sup, &b.base()),
                (_, Repr::KeyOf(_) | Repr::OffsetAccess(..)) => {
                    self.is_super_type_of(sup, &sub.resolve_late_to_bounds())
                }
                // Only enums have case values, and each is an int or a
                // string, whichever enum an enum-bounded template becomes.
                (_, Repr::ValueOf(inner)) if self.is_enum_like(inner) => {
                    if self
                        .is_super_type_of(sup, &Type::union_of(vec![Type::int(), Type::string()]))
                        .yes()
                    {
                        TrinaryLogic::Yes
                    } else {
                        TrinaryLogic::Maybe
                    }
                }
                // A benevolent union (`(int|string)`) is its members for
                // `isSuperTypeOf`; only `accepts` is lenient about it.
                (_, Repr::Union(members) | Repr::BenevolentUnion(members)) => {
                    TrinaryLogic::extreme_identity(
                        members.iter().map(|m| self.is_super_type_of(sup, m)),
                    )
                }
                (_, Repr::TemplateParam(tp)) => self.is_super_type_of(sup, &tp.bound),
                (
                    _,
                    Repr::Conditional {
                        if_true, if_false, ..
                    },
                ) => TrinaryLogic::extreme_identity(
                    [if_true, if_false]
                        .into_iter()
                        .map(|branch| self.is_super_type_of(sup, branch)),
                ),
                // A single value is in a union that lists it; otherwise
                // only a member holding more than one value can hold it.
                (Repr::Union(members) | Repr::BenevolentUnion(members), _)
                    if is_scalar_value(sub) =>
                {
                    if members.iter().any(|m| m == sub) {
                        TrinaryLogic::Yes
                    } else {
                        TrinaryLogic::max_min(
                            members
                                .iter()
                                .filter(|m| !is_scalar_value(m))
                                .map(|m| self.is_super_type_of(m, sub)),
                        )
                    }
                }
                (Repr::Union(members) | Repr::BenevolentUnion(members), _) => {
                    let by_member = TrinaryLogic::max_min(
                        members.iter().map(|m| self.is_super_type_of(m, sub)),
                    );
                    // `A & B` is within a union one of its own members is.
                    match sub.repr() {
                        Repr::Intersection(sub_members)
                            if by_member != TrinaryLogic::Yes
                                && sub_members
                                    .iter()
                                    .any(|m| self.is_super_type_of(sup, m).yes()) =>
                        {
                            TrinaryLogic::Yes
                        }
                        _ => by_member,
                    }
                }
                // No value of `mixed~S` is one of `S`'s.
                (_, Repr::Mixed(_, Some(subtracted)))
                    if self.is_super_type_of(subtracted, sup).yes() =>
                {
                    TrinaryLogic::No
                }
                (_, Repr::Mixed(..)) | (Repr::TemplateParam(_), _) => TrinaryLogic::Maybe,
                (Repr::Void, Repr::Void) => TrinaryLogic::Yes,
                (Repr::Void, _) | (_, Repr::Void) => TrinaryLogic::No,
                // An object whose class declares `__invoke()` is callable
                // (`ObjectType::isCallable()`); its signature is not compared
                // with a callable's.
                (Repr::Callable(sig), Repr::Object(class, _))
                    if self.hierarchy.has_method(class, "__invoke") =>
                {
                    match sig {
                        None => TrinaryLogic::Yes,
                        Some(_) => TrinaryLogic::Maybe,
                    }
                }
                (Repr::Intersection(members), _) => {
                    let results: Vec<TrinaryLogic> = members
                        .iter()
                        .map(|m| self.is_super_type_of(m, sub))
                        .collect();
                    if results.contains(&TrinaryLogic::No) {
                        TrinaryLogic::No
                    } else if results.iter().all(|r| *r == TrinaryLogic::Yes) {
                        TrinaryLogic::Yes
                    } else {
                        TrinaryLogic::Maybe
                    }
                }
                (
                    Repr::Float | Repr::FloatLiteral(_),
                    Repr::Int | Repr::IntLiteral(_) | Repr::IntRange(..),
                ) => TrinaryLogic::No,
                // A plain string may or may not name a class.
                (
                    Repr::ClassString(_) | Repr::ClassKindString(_),
                    Repr::String | Repr::StringLiteral(_),
                ) => TrinaryLogic::Maybe,
                (Repr::ClassString(_) | Repr::ClassKindString(_), Repr::Intersection(ms))
                    if !ms.iter().any(|m| {
                        matches!(m.repr(), Repr::ClassString(_) | Repr::ClassKindString(_))
                    }) =>
                {
                    TrinaryLogic::Maybe
                }
                // An unresolved constant reference proves nothing.
                (Repr::ValueOf(_) | Repr::ClassConstant { .. }, _)
                | (_, Repr::ValueOf(_) | Repr::ClassConstant { .. }) => TrinaryLogic::Maybe,
                (Repr::Object(a, a_args), Repr::Object(b, b_args))
                    if a.eq_ignore_case(b) && a_args.is_empty() && b_args.is_empty() =>
                {
                    TrinaryLogic::Yes
                }
                (Repr::Object(a, _), Repr::Object(b, _))
                    if [a, b]
                        .iter()
                        .any(|n| self.hierarchy.find_class(n).is_none()) =>
                {
                    TrinaryLogic::Maybe
                }
                (Repr::ArrayShape(sup_shape), Repr::ArrayShape(sub_shape)) => {
                    self.shape_super_type_of(sup_shape, sub_shape)
                }
                // Arrays compare structurally: `accepts` reads an
                // `array<mixed>` value as "unknown" and lets it fit any array,
                // right for accepting an argument but not for "always". An
                // empty array fits every array type, so this is never `No`.
                (Repr::Array(sk, sv), Repr::Array(k, v)) => {
                    // Every PHP array key is an `int|string`.
                    let key = if self.is_super_type_of(sk, &Type::unknown_array_key())
                        == TrinaryLogic::Yes
                    {
                        TrinaryLogic::Yes
                    } else {
                        self.is_super_type_of(sk, k)
                    };
                    match TrinaryLogic::extreme_identity(
                        [key, self.is_super_type_of(sv, v)].into_iter(),
                    ) {
                        TrinaryLogic::Yes => TrinaryLogic::Yes,
                        _ => TrinaryLogic::Maybe,
                    }
                }
                // A field the shape requires that the array's keys or values
                // can't hold keeps any value from being both.
                (Repr::ArrayShape(shape), Repr::Array(k, v))
                | (Repr::Array(k, v), Repr::ArrayShape(shape))
                    if shape.fields.iter().any(|f| {
                        !f.optional
                            && (self.disjoint(k, &f.key.to_type()) || self.disjoint(v, &f.ty))
                    }) =>
                {
                    TrinaryLogic::No
                }
                // A general array may or may not carry a shape's keys.
                (Repr::ArrayShape(_), Repr::Array(..)) => TrinaryLogic::Maybe,
                _ if self.proves_super_type(sup, sub) => TrinaryLogic::Yes,
                (_, Repr::Intersection(members)) => {
                    let refused = members
                        .iter()
                        .filter(|m| !m.is_accessory())
                        .any(|m| self.is_super_type_of(sup, m) == TrinaryLogic::No);
                    if refused {
                        TrinaryLogic::No
                    } else {
                        TrinaryLogic::Maybe
                    }
                }
                // `proves_super_type` has already answered the generic-aware
                // `Yes`; the classes being related is only a `Maybe` here.
                (Repr::Object(a, _), Repr::Object(b, _)) => match self.object_super_type_of(a, b) {
                    TrinaryLogic::No => TrinaryLogic::No,
                    _ => TrinaryLogic::Maybe,
                },
                // A case is an instance of its enum: within a class its enum
                // is within, when the class takes no type arguments.
                (Repr::Object(a, args), Repr::EnumCase(b, _)) => {
                    match self.object_super_type_of(a, b) {
                        TrinaryLogic::Yes if args.is_empty() => TrinaryLogic::Yes,
                        TrinaryLogic::No => TrinaryLogic::No,
                        _ => TrinaryLogic::Maybe,
                    }
                }
                // An object of `a` may be the case when the classes overlap.
                (Repr::EnumCase(b, _), Repr::Object(a, _)) => {
                    match self.object_super_type_of(a, b) {
                        TrinaryLogic::No => TrinaryLogic::No,
                        _ => TrinaryLogic::Maybe,
                    }
                }
                (Repr::EnumCase(a, _), Repr::EnumCase(b, _)) if !a.eq_ignore_ascii_case(b) => {
                    TrinaryLogic::No
                }
                _ if scalars_disjoint(sup, sub) => TrinaryLogic::No,
                (Repr::Accessory(kind), _) if accessory_refuses(kind, sub) => TrinaryLogic::No,
                _ => TrinaryLogic::Maybe,
            }
        })
    }

    /// `ConstantArrayType::isSuperTypeOf` between two shapes: every key
    /// `sup` requires must be one `sub` always has, with a value `sup`
    /// accepts; a key `sup` doesn't declare is fine only when `sup` is
    /// unsealed.
    fn shape_super_type_of(&self, sup: &Shape, sub: &Shape) -> TrinaryLogic {
        let mut results = Vec::new();
        for sup_field in &sup.fields {
            match sub.field(&sup_field.key) {
                Some(sub_field) => {
                    let value = self.is_super_type_of(&sup_field.ty, &sub_field.ty);
                    results.push(if sub_field.optional && !sup_field.optional {
                        // Present only sometimes: never always.
                        match value {
                            TrinaryLogic::No => TrinaryLogic::No,
                            _ => TrinaryLogic::Maybe,
                        }
                    } else {
                        value
                    });
                }
                None if sup_field.optional => {}
                None if sub.is_sealed() => results.push(TrinaryLogic::No),
                None => results.push(TrinaryLogic::Maybe),
            }
        }
        if sup.is_sealed() {
            for sub_field in &sub.fields {
                if sup.field(&sub_field.key).is_none() {
                    results.push(if sub_field.optional {
                        TrinaryLogic::Maybe
                    } else {
                        TrinaryLogic::No
                    });
                }
            }
        }
        if results.contains(&TrinaryLogic::No) {
            TrinaryLogic::No
        } else if results.contains(&TrinaryLogic::Maybe) {
            TrinaryLogic::Maybe
        } else {
            TrinaryLogic::Yes
        }
    }

    /// `TypeCombinator::union`'s absorption for display: a member some
    /// other member strictly contains is dropped (`object|Foo` is
    /// `object`, `int|1` is `int`).
    pub fn absorb_union_subtypes(&self, ty: &Type) -> Type {
        let Repr::Union(members) = ty.repr() else {
            return ty.clone();
        };
        let strictly_within = |m: &Type, n: &Type| {
            m != n
                && self.is_super_type_of(n, m) == TrinaryLogic::Yes
                && self.is_super_type_of(m, n) != TrinaryLogic::Yes
        };
        let kept: Vec<Type> = members
            .iter()
            .filter(|m| !members.iter().any(|n| strictly_within(m, n)))
            .cloned()
            .collect();
        match kept.len() {
            1 => kept.into_iter().next().unwrap(),
            _ => Type::new(Repr::Union(kept)),
        }
    }

    /// How the class `sup` relates to the class `sub`, generic arguments
    /// aside: `Yes` when `sub` is `sup` or extends or implements it, `No`
    /// when no class can be both, `Maybe` otherwise (an unknown class, a
    /// subclass of `sub` that could extend `sup`).
    pub(crate) fn object_super_type_of(&self, sup: &str, sub: &str) -> TrinaryLogic {
        if sup.eq_ignore_ascii_case(sub) || self.hierarchy.is_subclass_of(sub, sup) {
            return TrinaryLogic::Yes;
        }
        let (Some(sup_info), Some(sub_info)) = (
            self.hierarchy.find_class(sup),
            self.hierarchy.find_class(sub),
        ) else {
            return TrinaryLogic::Maybe;
        };
        if self.hierarchy.is_subclass_of(sup, sub) {
            return TrinaryLogic::Maybe;
        }
        // A `No` needs the whole hierarchy of both sides: an ancestor the
        // registry never indexed could still connect them.
        let hierarchy_known = |class: &str| {
            self.hierarchy
                .ancestor_names(class)
                .iter()
                .all(|a| self.hierarchy.find_class(a).is_some())
        };
        if !hierarchy_known(sup) || !hierarchy_known(sub) {
            return TrinaryLogic::Maybe;
        }
        let is_interface = |k: &ClassKind| matches!(k, ClassKind::Interface);
        // An enum can't be extended either.
        let is_final =
            |facts: &crate::ClassFacts<'_>| facts.is_final || facts.kind == ClassKind::Enum;
        if (is_interface(&sup_info.kind) && !is_final(&sub_info))
            || (is_interface(&sub_info.kind) && !is_final(&sup_info))
        {
            return TrinaryLogic::Maybe;
        }
        TrinaryLogic::No
    }
}

/// An accessory refinement no value of `sub` can carry: one of another
/// scalar family (`non-empty-string` over an `int`, `list` over a
/// `string`), or a literal or shape the refinement rules out
/// (`non-empty-string` over `''`, `numeric-string` over `'abc'`,
/// `non-empty-array` over `array{}`, `list` over `array{a: int}`).
fn accessory_refuses(kind: &AccessoryKind, sub: &Type) -> bool {
    if let Repr::StringLiteral(value) = sub.repr() {
        let bytes = value.as_bytes();
        let has = match kind {
            AccessoryKind::NonEmptyString => !bytes.is_empty(),
            AccessoryKind::NonFalsyString => !bytes.is_empty() && bytes != b"0",
            AccessoryKind::NumericString => crate::numeric_value(bytes).is_some(),
            AccessoryKind::LiteralString => true,
            AccessoryKind::LowercaseString => !bytes.iter().any(u8::is_ascii_uppercase),
            AccessoryKind::UppercaseString => !bytes.iter().any(u8::is_ascii_lowercase),
            AccessoryKind::DecimalIntegerString => value
                .as_str()
                .is_some_and(crate::subtype::is_decimal_integer_string_literal),
            AccessoryKind::NonEmptyArray | AccessoryKind::ArrayList => false,
        };
        return !has;
    }
    if let Repr::ArrayShape(shape) = sub.repr() {
        match kind {
            AccessoryKind::NonEmptyArray if shape.is_empty_array() => return true,
            AccessoryKind::ArrayList
                if shape
                    .fields
                    .iter()
                    .any(|f| !f.optional && matches!(f.key, crate::ShapeKey::Str(_))) =>
            {
                return true;
            }
            _ => {}
        }
    }
    let is_string = matches!(
        sub.repr(),
        Repr::String | Repr::StringLiteral(_) | Repr::ClassString(_) | Repr::ClassKindString(_)
    );
    let is_array = matches!(sub.repr(), Repr::Array(..) | Repr::ArrayShape(_));
    match kind {
        AccessoryKind::NonEmptyArray | AccessoryKind::ArrayList => {
            !is_array
                && scalars_disjoint(
                    &Type::array(
                        Type::new(Repr::Mixed(false, None)),
                        Type::new(Repr::Mixed(false, None)),
                    ),
                    sub,
                )
        }
        AccessoryKind::NonEmptyString
        | AccessoryKind::NonFalsyString
        | AccessoryKind::NumericString
        | AccessoryKind::LiteralString
        | AccessoryKind::LowercaseString
        | AccessoryKind::UppercaseString
        | AccessoryKind::DecimalIntegerString => {
            !is_string && scalars_disjoint(&Type::string(), sub)
        }
    }
}

/// Two different values of one literal kind (`'a'`/`'b'`, `1`/`2`,
/// `true`/`false`, two cases of one enum, an int outside a range), or two
/// types of different scalar families (`string`/`int`, `array`/`object`).
/// Anything else is not proven disjoint.
fn scalars_disjoint(a: &Type, b: &Type) -> bool {
    syntax::stack::guard(|| {
        match (a.repr(), b.repr()) {
            (Repr::Union(members) | Repr::BenevolentUnion(members), _) => {
                members.iter().all(|m| scalars_disjoint(m, b))
            }
            (_, Repr::Union(members) | Repr::BenevolentUnion(members)) => {
                members.iter().all(|m| scalars_disjoint(a, m))
            }
            // Two different values of the same kind can never overlap - real
            // PHPStan's `ConstantStringType('send')->isSuperTypeOf(
            // 'list_to_send')` is a definite no, not a maybe.
            (Repr::StringLiteral(x), Repr::StringLiteral(y)) => x != y,
            (Repr::IntLiteral(x), Repr::IntLiteral(y)) => x != y,
            (Repr::True, Repr::False) | (Repr::False, Repr::True) => true,
            // An open and a closed resource are never the same value.
            (Repr::Resource { closed: a }, Repr::Resource { closed: b }) => a != b,
            (Repr::EnumCase(class_a, case_a), Repr::EnumCase(class_b, case_b)) => {
                class_a == class_b && case_a != case_b
            }
            (Repr::IntLiteral(n), Repr::IntRange(min, max))
            | (Repr::IntRange(min, max), Repr::IntLiteral(n)) => {
                min.is_some_and(|m| *n < m) || max.is_some_and(|m| *n > m)
            }
            (Repr::IntRange(a_min, a_max), Repr::IntRange(b_min, b_max)) => {
                a_max.zip(*b_min).is_some_and(|(a, b)| a < b)
                    || b_max.zip(*a_min).is_some_and(|(b, a)| b < a)
            }
            // `NAN` is itself (see `is_super_type_of`).
            (Repr::FloatLiteral(x), Repr::FloatLiteral(y)) => x != y && !(x.is_nan() && y.is_nan()),
            _ => match (scalar_family(a), scalar_family(b)) {
                (Some(fa), Some(fb)) => fa != fb,
                _ => false,
            },
        }
    })
}

#[derive(PartialEq)]
enum ScalarFamily {
    Int,
    Float,
    String,
    Bool,
    Null,
    Array,
    Object,
    Resource,
}

fn scalar_family(ty: &Type) -> Option<ScalarFamily> {
    match ty.repr() {
        Repr::Int | Repr::IntLiteral(_) | Repr::IntRange(_, _) => Some(ScalarFamily::Int),
        Repr::Float | Repr::FloatLiteral(_) => Some(ScalarFamily::Float),
        Repr::String | Repr::StringLiteral(_) | Repr::ClassString(_) | Repr::ClassKindString(_) => {
            Some(ScalarFamily::String)
        }
        Repr::Bool | Repr::True | Repr::False => Some(ScalarFamily::Bool),
        Repr::Null => Some(ScalarFamily::Null),
        Repr::Array(_, _) | Repr::ArrayShape(_) => Some(ScalarFamily::Array),
        Repr::Object(_, _)
        | Repr::ObjectWithoutClass
        | Repr::EnumCase(_, _)
        | Repr::ObjectShape(_) => Some(ScalarFamily::Object),
        Repr::Resource { .. } => Some(ScalarFamily::Resource),
        _ => None,
    }
}

/// Whether `ty` is one scalar value or enum case: a literal (`NAN`
/// excepted, being no value it equals), `true`, `false`, `null`, a case.
fn is_scalar_value(ty: &Type) -> bool {
    match ty.repr() {
        Repr::IntLiteral(_)
        | Repr::StringLiteral(_)
        | Repr::True
        | Repr::False
        | Repr::Null
        | Repr::EnumCase(..) => true,
        Repr::FloatLiteral(f) => !f.is_nan(),
        _ => false,
    }
}
