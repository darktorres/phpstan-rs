use crate::relation::Relation;
use crate::{AccessoryKind, Shape, TrinaryLogic, Type};
use syntax::ast::ClassKind;

impl Relation<'_> {
    /// Whether every value of `sub` is a `sup` (`Yes`), none is (`No`),
    /// or some are (`Maybe`) - real PHPStan's `isSuperTypeOf`, as the
    /// PHPDoc-vs-native rules read it. `Yes` follows `accepts`, so it
    /// inherits that relation's leniencies; `No` needs a proof (disjoint
    /// scalar families, distinct literals, or two unrelated classes
    /// neither of which can be extended into the other), and anything
    /// unprovable is `Maybe`, including any comparison with a class the
    /// registry doesn't know.
    pub fn is_super_type_of(&self, sup: &Type, sub: &Type) -> TrinaryLogic {
        match (sup, sub) {
            (Type::Mixed(_), _) | (_, Type::Never) => TrinaryLogic::Yes,
            (Type::KeyOf(_) | Type::OffsetAccess(..), _) => {
                self.is_super_type_of(&crate::resolve_late_to_bounds(sup), sub)
            }
            (Type::Extension(a), Type::Extension(b)) => a
                .is_super_type_of(&**b, self.hierarchy)
                .unwrap_or_else(|| self.is_super_type_of(&a.base(), &b.base())),
            // A plain base value may or may not carry the payload.
            (Type::Extension(a), _) => match self.is_super_type_of(&a.base(), sub) {
                TrinaryLogic::No => TrinaryLogic::No,
                _ => TrinaryLogic::Maybe,
            },
            (_, Type::Extension(b)) => self.is_super_type_of(sup, &b.base()),
            (_, Type::KeyOf(_) | Type::OffsetAccess(..)) => {
                self.is_super_type_of(sup, &crate::resolve_late_to_bounds(sub))
            }
            // A benevolent union (`(int|string)`) is its members for
            // `isSuperTypeOf`; only `accepts` is lenient about it.
            (_, Type::Union(members) | Type::BenevolentUnion(members)) => {
                TrinaryLogic::extreme_identity(
                    members.iter().map(|m| self.is_super_type_of(sup, m)),
                )
            }
            (_, Type::TemplateParam(tp)) => self.is_super_type_of(sup, &tp.bound),
            (
                _,
                Type::Conditional {
                    if_true, if_false, ..
                },
            ) => TrinaryLogic::extreme_identity(
                [if_true, if_false]
                    .into_iter()
                    .map(|branch| self.is_super_type_of(sup, branch)),
            ),
            (Type::Union(members) | Type::BenevolentUnion(members), _) => {
                TrinaryLogic::max_min(members.iter().map(|m| self.is_super_type_of(m, sub)))
            }
            (_, Type::Mixed(_)) | (Type::TemplateParam(_), _) => TrinaryLogic::Maybe,
            (Type::Void, Type::Void) => TrinaryLogic::Yes,
            (Type::Void, _) | (_, Type::Void) => TrinaryLogic::No,
            // An object whose class declares `__invoke()` is callable
            // (`ObjectType::isCallable()`); its signature is not compared
            // with a callable's.
            (Type::Callable(sig), Type::Object(class, _))
                if self.hierarchy.has_method(class, "__invoke") =>
            {
                match sig {
                    None => TrinaryLogic::Yes,
                    Some(_) => TrinaryLogic::Maybe,
                }
            }
            (Type::Intersection(members), _) => {
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
                Type::Float | Type::FloatLiteral(_),
                Type::Int | Type::IntLiteral(_) | Type::IntRange(..),
            ) => TrinaryLogic::No,
            // A plain string may or may not name a class.
            (
                Type::ClassString(_) | Type::ClassKindString(_),
                Type::String | Type::StringLiteral(_),
            ) => TrinaryLogic::Maybe,
            (Type::ClassString(_) | Type::ClassKindString(_), Type::Intersection(ms))
                if !ms
                    .iter()
                    .any(|m| matches!(m, Type::ClassString(_) | Type::ClassKindString(_))) =>
            {
                TrinaryLogic::Maybe
            }
            // An unresolved constant reference proves nothing.
            (Type::ValueOf(_) | Type::ClassConstant { .. }, _)
            | (_, Type::ValueOf(_) | Type::ClassConstant { .. }) => TrinaryLogic::Maybe,
            (Type::Object(a, a_args), Type::Object(b, b_args))
                if a == b && a_args.is_empty() && b_args.is_empty() =>
            {
                TrinaryLogic::Yes
            }
            (Type::Object(a, _), Type::Object(b, _))
                if [a, b]
                    .iter()
                    .any(|n| self.hierarchy.find_class(n).is_none()) =>
            {
                TrinaryLogic::Maybe
            }
            (Type::ArrayShape(sup_shape), Type::ArrayShape(sub_shape)) => {
                self.shape_super_type_of(sup_shape, sub_shape)
            }
            // Arrays compare structurally: `accepts` reads an
            // `array<mixed>` value as "unknown" and lets it fit any array,
            // right for accepting an argument but not for "always". An
            // empty array fits every array type, so this is never `No`.
            (Type::Array(sk, sv), Type::Array(k, v)) => {
                // Every PHP array key is an `int|string`.
                let key =
                    if self.is_super_type_of(sk, &Type::unknown_array_key()) == TrinaryLogic::Yes {
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
            // A general array may or may not carry a shape's keys.
            (Type::ArrayShape(_), Type::Array(..)) => TrinaryLogic::Maybe,
            _ if self.subtype(sub, sup) => TrinaryLogic::Yes,
            (_, Type::Intersection(members)) => {
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
            (Type::Object(a, _), Type::Object(b, _)) => self.object_super_type_of(a, b),
            _ if crate::types_definitely_disjoint(sup, sub) => TrinaryLogic::No,
            (Type::Accessory(kind), _) if accessory_refuses(kind, sub) => TrinaryLogic::No,
            _ => TrinaryLogic::Maybe,
        }
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

    /// `ObjectType::isSuperTypeOf` between two named classes, once the
    /// `sub`-extends-`sup` case has already answered `Yes`.
    /// `TypeCombinator::union`'s absorption for display: a member some
    /// other member strictly contains is dropped (`object|Foo` is
    /// `object`, `int|1` is `int`).
    pub fn absorb_union_subtypes(&self, ty: &Type) -> Type {
        let Type::Union(members) = ty else {
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
            _ => Type::Union(kept),
        }
    }

    fn object_super_type_of(&self, sup: &str, sub: &str) -> TrinaryLogic {
        if sup == sub {
            return TrinaryLogic::Maybe;
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
        if (is_interface(&sup_info.kind) && !sub_info.is_final)
            || (is_interface(&sub_info.kind) && !sup_info.is_final)
        {
            return TrinaryLogic::Maybe;
        }
        TrinaryLogic::No
    }
}

/// An accessory refinement no value of `sub`'s scalar family can carry
/// (`non-empty-string` over an `int`, `list` over a `string`).
fn accessory_refuses(kind: &AccessoryKind, sub: &Type) -> bool {
    let is_string = matches!(
        sub,
        Type::String | Type::StringLiteral(_) | Type::ClassString(_) | Type::ClassKindString(_)
    );
    let is_array = matches!(sub, Type::Array(..) | Type::ArrayShape(_));
    match kind {
        AccessoryKind::NonEmptyArray | AccessoryKind::ArrayList => {
            !is_array
                && crate::types_definitely_disjoint(
                    &Type::Array(Box::new(Type::Mixed(false)), Box::new(Type::Mixed(false))),
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
            !is_string && crate::types_definitely_disjoint(&Type::String, sub)
        }
    }
}
