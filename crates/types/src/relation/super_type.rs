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

    /// `disjoint(ty, &key.to_type())`, without making the key's type for
    /// the key types that answer on their form alone: another scalar
    /// family shares no value with the key, and a plain `int`, `string`,
    /// `mixed` or union of the two holds the keys of its families.
    fn key_disjoint(&self, ty: &Type, key: &crate::ShapeKey) -> bool {
        stack::guard(|| {
            let key_family = match key {
                crate::ShapeKey::Int(_) => ScalarFamily::Int,
                crate::ShapeKey::Str(_) => ScalarFamily::String,
            };
            let holds_family = |t: &Type| match t.repr() {
                Repr::Int => key_family == ScalarFamily::Int,
                Repr::String => key_family == ScalarFamily::String,
                _ => false,
            };
            match ty.repr() {
                Repr::Mixed(_, None) => false,
                Repr::Union(members)
                    if members
                        .iter()
                        .all(|m| matches!(m.repr(), Repr::Int | Repr::String))
                        && members.iter().any(holds_family) =>
                {
                    false
                }
                _ => match scalar_family(ty) {
                    Some(family) if family != key_family => true,
                    _ if holds_family(ty) => false,
                    _ => self.disjoint(ty, &key.to_type()),
                },
            }
        })
    }

    /// Whether no value is both an `a` and a `b`: `is_super_type_of`
    /// answers no one way round or the other.
    pub fn disjoint(&self, a: &Type, b: &Type) -> bool {
        stack::guard(|| self.is_super_type_of(a, b).no() || self.is_super_type_of(b, a).no())
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
        stack::guard(|| {
            // One node against itself: every value of a type is a value of it.
            if std::sync::Arc::ptr_eq(&sup.0, &sub.0) {
                return TrinaryLogic::Yes;
            }
            // Each shortcut reads the node's kind, set when the node was made.
            let (sup_flags, sub_flags) = (sup.0.flags(), sub.0.flags());
            // A literal, `true`, `false`, `null` or an enum case is one value:
            // one holds another only when they are the same.
            if sup_flags & sub_flags & crate::IS_SCALAR_VALUE != 0 {
                return TrinaryLogic::from_bool(sup == sub);
            }
            let (sup_kind, sub_kind) = (crate::kind_of(sup_flags), crate::kind_of(sub_flags));
            // Plain `mixed` holds everything and `never` is held by everything.
            if sup_kind == KIND_MIXED || sub_kind == KIND_NEVER {
                return TrinaryLogic::Yes;
            }
            // Two scalar families share no value.
            if is_family_kind(sup_kind) && is_family_kind(sub_kind) && sup_kind != sub_kind {
                return TrinaryLogic::No;
            }
            if self.hierarchy.is_structural() {
                return structural_memo::answer(sup, sub, || {
                    self.is_super_type_of_by_form(sup, sub)
                });
            }
            self.is_super_type_of_by_form(sup, sub)
        })
    }

    /// `is_super_type_of` read off the two types' forms, without the
    /// shortcuts that answer before it.
    pub(crate) fn is_super_type_of_by_form(&self, sup: &Type, sub: &Type) -> TrinaryLogic {
        stack::guard(|| {
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
                // `bool` is `true|false` for a union to hold.
                (Repr::Union(_) | Repr::BenevolentUnion(_), Repr::Bool) => {
                    TrinaryLogic::extreme_identity(
                        [Type::bool_literal(true), Type::bool_literal(false)]
                            .iter()
                            .map(|b| self.is_super_type_of(sup, b)),
                    )
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
                // `No` as soon as one member refuses, `Yes` when every
                // member holds, else `Maybe`: asked in order, and no
                // further once one says `No`.
                (Repr::Intersection(members), _) => {
                    let mut every_member_holds = true;
                    for member in members.iter() {
                        match self.is_super_type_of(member, sub) {
                            TrinaryLogic::No => return TrinaryLogic::No,
                            TrinaryLogic::Yes => {}
                            TrinaryLogic::Maybe => every_member_holds = false,
                        }
                    }
                    if every_member_holds {
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
                    if shape.fields().iter().any(|f| {
                        !f.optional && (self.key_disjoint(k, &f.key) || self.disjoint(v, &f.ty))
                    }) =>
                {
                    TrinaryLogic::No
                }
                // A general array may or may not carry a shape's keys.
                (Repr::ArrayShape(_), Repr::Array(..)) => TrinaryLogic::Maybe,
                _ if self.proves_super_type(sup, sub) => TrinaryLogic::Yes,
                (_, Repr::Intersection(members)) => {
                    let refused = members.iter().any(|m| match (sup.repr(), m.repr()) {
                        (Repr::Accessory(a), Repr::Accessory(b)) => accessories_contradict(a, b),
                        _ => !m.is_accessory() && self.is_super_type_of(sup, m) == TrinaryLogic::No,
                    });
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
        stack::guard(|| {
            // The answer is the weakest of the fields' answers, `No` weakest, so
            // the keys, which cost no type comparison, come first and the first
            // `No` ends it.
            let mut verdict = TrinaryLogic::Yes;
            for (at, sup_field) in sup.fields().iter().enumerate() {
                if !sup_field.optional && sub.field_near(&sup_field.key, at).is_none() {
                    if sub.is_sealed() {
                        return TrinaryLogic::No;
                    }
                    verdict = TrinaryLogic::Maybe;
                }
            }
            if sup.is_sealed() {
                for (at, sub_field) in sub.fields().iter().enumerate() {
                    if sup.field_near(&sub_field.key, at).is_none() {
                        if !sub_field.optional {
                            return TrinaryLogic::No;
                        }
                        verdict = TrinaryLogic::Maybe;
                    }
                }
            }
            for (at, sup_field) in sup.fields().iter().enumerate() {
                let Some(sub_field) = sub.field_near(&sup_field.key, at) else {
                    continue;
                };
                match self.is_super_type_of(&sup_field.ty, &sub_field.ty) {
                    TrinaryLogic::No => return TrinaryLogic::No,
                    // Present only sometimes: never always.
                    TrinaryLogic::Yes if !sub_field.optional || sup_field.optional => {}
                    _ => verdict = TrinaryLogic::Maybe,
                }
            }
            verdict
        })
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
    if let Repr::Accessory(other) = sub.repr()
        && accessories_contradict(kind, other)
    {
        return true;
    }
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
            AccessoryKind::NonDecimalIntegerString => !value
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
                    .fields()
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
        | AccessoryKind::DecimalIntegerString
        | AccessoryKind::NonDecimalIntegerString => {
            !is_string && scalars_disjoint(&Type::string(), sub)
        }
    }
}

/// Two refinements no string has together: a decimal-int string and a
/// non-decimal-int one.
fn accessories_contradict(a: &AccessoryKind, b: &AccessoryKind) -> bool {
    matches!(
        (a, b),
        (
            AccessoryKind::DecimalIntegerString,
            AccessoryKind::NonDecimalIntegerString
        ) | (
            AccessoryKind::NonDecimalIntegerString,
            AccessoryKind::DecimalIntegerString
        )
    )
}

/// Two different values of one literal kind (`'a'`/`'b'`, `1`/`2`,
/// `true`/`false`, two cases of one enum, an int outside a range), or two
/// types of different scalar families (`string`/`int`, `array`/`object`).
/// Anything else is not proven disjoint.
fn scalars_disjoint(a: &Type, b: &Type) -> bool {
    stack::guard(|| {
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

/// The answers `Relation::structural()` has given, by the pair of types:
/// the same pairs are asked again and again, by a union's absorption and
/// its merge of shapes and by the comparison of their fields' values.
mod structural_memo {
    use std::cell::RefCell;

    use rustc_hash::FxHashMap;

    use crate::{TrinaryLogic, Type};

    /// Past this many pairs a thread's memo is emptied.
    const LIMIT: usize = 1 << 17;

    /// The pair's nodes are held with the answer, which keeps their
    /// addresses from being taken by other types while the entry stands.
    type Entries = FxHashMap<(usize, usize), (Type, Type, TrinaryLogic)>;

    thread_local! {
        static ANSWERS: RefCell<Entries> = RefCell::new(FxHashMap::default());
    }

    pub(super) fn answer(
        sup: &Type,
        sub: &Type,
        compute: impl FnOnce() -> TrinaryLogic,
    ) -> TrinaryLogic {
        let key = (sup.node_address(), sub.node_address());
        if let Some(known) = ANSWERS.with(|a| a.borrow().get(&key).map(|e| e.2)) {
            return known;
        }
        let answer = compute();
        ANSWERS.with(|a| {
            let mut entries = a.borrow_mut();
            if entries.len() >= LIMIT {
                entries.clear();
            }
            entries.insert(key, (sup.clone(), sub.clone(), answer));
        });
        answer
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ScalarFamily {
    Int,
    Float,
    String,
    Bool,
    Null,
    Array,
    Object,
    Resource,
}

/// The kind a node has when it is plain `mixed`, and when it is `never`:
/// the two forms the first shortcuts of `is_super_type_of` ask for. The
/// kinds 1 to 8 are the `ScalarFamily` values, in order, and 0 is a form
/// that is none of these.
const KIND_MIXED: u16 = 9;
const KIND_NEVER: u16 = 10;

const FAMILIES: [ScalarFamily; 8] = [
    ScalarFamily::Int,
    ScalarFamily::Float,
    ScalarFamily::String,
    ScalarFamily::Bool,
    ScalarFamily::Null,
    ScalarFamily::Array,
    ScalarFamily::Object,
    ScalarFamily::Resource,
];

fn is_family_kind(kind: u16) -> bool {
    (1..=8).contains(&kind)
}

/// The kind code and the scalar-value bit of `repr`, as set in its node's
/// flags (`crate::reference_flags`): what `scalar_family`, `is_scalar_value`
/// and the `mixed` and `never` tests answer, read once when the node is made.
pub(crate) fn kind_flags(repr: &Repr) -> u16 {
    let kind = match family_of(repr) {
        Some(family) => family as u16 + 1,
        None => match repr {
            Repr::Mixed(_, None) => KIND_MIXED,
            Repr::Never => KIND_NEVER,
            _ => 0,
        },
    };
    let mut flags = kind << crate::KIND_SHIFT;
    if is_scalar_value_repr(repr) {
        flags |= crate::IS_SCALAR_VALUE;
    }
    flags
}

pub(crate) fn scalar_family(ty: &Type) -> Option<ScalarFamily> {
    let kind = crate::kind_of(ty.0.flags());
    is_family_kind(kind).then(|| FAMILIES[usize::from(kind) - 1])
}

fn family_of(repr: &Repr) -> Option<ScalarFamily> {
    match repr {
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
    ty.0.flags() & crate::IS_SCALAR_VALUE != 0
}

fn is_scalar_value_repr(repr: &Repr) -> bool {
    match repr {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConditionalSubject;

    fn samples() -> Vec<Type> {
        let object = |name: &str| Type::object(name.into(), Vec::new());
        vec![
            Type::mixed(),
            Type::explicit_mixed(),
            Type::new(Repr::Mixed(false, Some(Type::int()))),
            Type::never(),
            Type::void(),
            Type::error(),
            Type::null(),
            Type::bool(),
            Type::bool_literal(true),
            Type::bool_literal(false),
            Type::int(),
            Type::int_literal(3),
            Type::int_range(Some(1), Some(5)),
            Type::float(),
            Type::string(),
            Type::string_literal("a".into()),
            Type::new(Repr::ClassString(object("Foo"))),
            Type::array(Type::mixed(), Type::mixed()),
            Type::array(Type::int(), Type::string()),
            Type::list_of(Type::int()),
            Type::array_shape(Shape::sealed(Vec::new())),
            Type::object_without_class(),
            object("Foo"),
            object("Countable"),
            Type::enum_case("Suit".into(), "Hearts".into()),
            Type::resource(false),
            Type::resource(true),
            Type::callable(None),
            Type::union_of(vec![Type::int(), Type::string()]),
            Type::union_of(vec![Type::null(), object("Foo")]),
            Type::new(Repr::Intersection(vec![
                Type::string(),
                Type::accessory(AccessoryKind::NonEmptyString),
            ])),
            Type::template_param(crate::TemplateParam::unbounded("T".to_string())),
            Type::new(Repr::Conditional {
                subject: ConditionalSubject::Template("T".into()),
                target: Type::int(),
                if_true: Type::string(),
                if_false: Type::null(),
                negated: false,
            }),
        ]
    }

    #[test]
    fn a_nodes_kind_is_what_its_own_form_says() {
        let mut types = samples();
        types.extend([
            Type::float_literal(f64::NAN),
            Type::float_literal(1.5),
            Type::new(Repr::ClassKindString(crate::ClassKind::Interface)),
            Type::new(Repr::Mixed(false, Some(Type::int()))),
            Type::array(Type::int(), Type::int_literal(1)),
            Type::union_of(vec![Type::int_literal(1), Type::int_literal(2)]),
            Type::union_of(vec![Type::never(), Type::null()]),
            Type::array(Type::mixed(), Type::never()),
        ]);
        for ty in &types {
            let repr = ty.repr();
            assert_eq!(
                scalar_family(ty).map(|f| f as u16),
                family_of(repr).map(|f| f as u16)
            );
            assert_eq!(is_scalar_value(ty), is_scalar_value_repr(repr), "{ty}");
            let kind = crate::kind_of(ty.0.flags());
            assert_eq!(
                kind == KIND_MIXED,
                matches!(repr, Repr::Mixed(_, None)),
                "{ty}"
            );
            assert_eq!(kind == KIND_NEVER, matches!(repr, Repr::Never), "{ty}");
            assert_eq!(
                kind == 0,
                family_of(repr).is_none() && !matches!(repr, Repr::Mixed(_, None) | Repr::Never),
                "{ty}"
            );
        }
        let family = |ty: &Type| scalar_family(ty).map(|f| f as u16);
        assert_eq!(family(&Type::int_literal(3)), Some(0));
        assert_eq!(family(&Type::string_literal("a".into())), Some(2));
        assert_eq!(family(&Type::array(Type::int(), Type::int())), Some(5));
        assert_eq!(
            family(&Type::union_of(vec![Type::int(), Type::string()])),
            None
        );
        assert!(!is_scalar_value(&Type::float_literal(f64::NAN)));
        assert!(is_scalar_value(&Type::float_literal(1.5)));
    }

    #[test]
    fn the_shortcuts_answer_as_the_forms_do() {
        let relation = Relation::structural();
        let samples = samples();
        for sup in &samples {
            for sub in &samples {
                let by_form = if sup == sub {
                    TrinaryLogic::Yes
                } else if is_scalar_value(sup) && is_scalar_value(sub) {
                    TrinaryLogic::from_bool(sup == sub)
                } else {
                    relation.is_super_type_of_by_form(sup, sub)
                };
                assert_eq!(
                    relation.is_super_type_of(sup, sub),
                    by_form,
                    "{sup} over {sub}"
                );
            }
        }
    }

    #[test]
    fn a_type_is_a_super_type_of_itself() {
        let mut types = samples();
        types.extend([
            Type::float_literal(f64::NAN),
            Type::new(Repr::Mixed(true, Some(Type::string()))),
            Type::union_of(vec![Type::int(), Type::null()]),
        ]);
        for relation in [Relation::structural()] {
            for ty in &types {
                assert_eq!(relation.is_super_type_of(ty, ty), TrinaryLogic::Yes, "{ty}");
                assert_eq!(
                    relation.is_super_type_of(ty, &ty.clone()),
                    TrinaryLogic::Yes,
                    "{ty}"
                );
            }
        }
    }

    #[test]
    fn a_pair_asked_again_is_answered_as_it_was() {
        let relation = Relation::structural();
        let samples = samples();
        for _ in 0..3 {
            for sup in &samples {
                for sub in &samples {
                    let expected = if sup == sub {
                        TrinaryLogic::Yes
                    } else if is_scalar_value(sup) && is_scalar_value(sub) {
                        TrinaryLogic::from_bool(sup == sub)
                    } else {
                        relation.is_super_type_of_by_form(sup, sub)
                    };
                    assert_eq!(relation.is_super_type_of(sup, sub), expected, "{sup} {sub}");
                }
            }
        }
    }

    #[test]
    fn a_key_is_disjoint_from_a_type_as_its_literal_type_is() {
        let relation = Relation::structural();
        let mut types = samples();
        types.extend([
            Type::int_range(None, Some(0)),
            Type::union_of(vec![Type::int(), Type::null()]),
            Type::union_of(vec![Type::string(), Type::float()]),
            Type::int_literal(7),
            Type::string_literal("k".into()),
        ]);
        let keys = [
            crate::ShapeKey::Int(3),
            crate::ShapeKey::Int(-1),
            crate::ShapeKey::from_name("a"),
            crate::ShapeKey::from_name("k"),
        ];
        for ty in &types {
            for key in &keys {
                assert_eq!(
                    relation.key_disjoint(ty, key),
                    relation.disjoint(ty, &key.to_type()),
                    "{ty} and {key}"
                );
            }
        }
    }
}
