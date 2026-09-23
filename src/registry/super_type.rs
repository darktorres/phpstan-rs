use crate::ast::ClassKind;
use crate::registry::Registry;
use crate::types::{self, AccessoryKind, Type};

/// Real PHPStan's `TrinaryLogic` result of a `Type::isSuperTypeOf` query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trinary {
    Yes,
    Maybe,
    No,
}

impl Trinary {
    /// `TrinaryLogic::extremeIdentity`: every member agreeing decides,
    /// anything mixed is `Maybe` - a union on the sub-type side.
    fn all(results: impl Iterator<Item = Trinary>) -> Trinary {
        let mut seen_yes = false;
        let mut seen_no = false;
        let mut seen_maybe = false;
        for r in results {
            match r {
                Trinary::Yes => seen_yes = true,
                Trinary::No => seen_no = true,
                Trinary::Maybe => seen_maybe = true,
            }
        }
        match (seen_yes, seen_no, seen_maybe) {
            (true, false, false) => Trinary::Yes,
            (false, true, false) => Trinary::No,
            (false, false, false) => Trinary::Yes,
            _ => Trinary::Maybe,
        }
    }

    /// `TrinaryLogic::lazyMaxMin` over a union on the super-type side:
    /// any member accepting decides `Yes`, all refusing is `No`.
    fn any(results: impl Iterator<Item = Trinary>) -> Trinary {
        let mut best = Trinary::No;
        for r in results {
            match r {
                Trinary::Yes => return Trinary::Yes,
                Trinary::Maybe => best = Trinary::Maybe,
                Trinary::No => {}
            }
        }
        best
    }
}

impl Registry {
    /// Whether every value of `sub` is a `sup` (`Yes`), none is (`No`),
    /// or some are (`Maybe`) - real PHPStan's `isSuperTypeOf`, as the
    /// PHPDoc-vs-native rules read it. `Yes` follows `is_assignable`, so it
    /// inherits that relation's leniencies; `No` needs a proof (disjoint
    /// scalar families, distinct literals, or two unrelated classes
    /// neither of which can be extended into the other), and anything
    /// unprovable is `Maybe`, including any comparison with a class the
    /// registry doesn't know.
    pub fn is_super_type_of(&self, sup: &Type, sub: &Type) -> Trinary {
        match (sup, sub) {
            (Type::Mixed(_), _) | (_, Type::Never) => Trinary::Yes,
            // A benevolent union (`(int|string)`) is its members for
            // `isSuperTypeOf`; only `accepts` is lenient about it.
            (_, Type::Intersection(members)) if is_benevolent(members) => {
                self.is_super_type_of(sup, &benevolent_inner(members))
            }
            (Type::Intersection(members), _) if is_benevolent(members) => {
                self.is_super_type_of(&benevolent_inner(members), sub)
            }
            (_, Type::Union(members)) => {
                Trinary::all(members.iter().map(|m| self.is_super_type_of(sup, m)))
            }
            (
                _,
                Type::Conditional {
                    if_true, if_false, ..
                },
            ) => Trinary::all(
                [if_true, if_false]
                    .into_iter()
                    .map(|branch| self.is_super_type_of(sup, branch)),
            ),
            (Type::Union(members), _) => {
                Trinary::any(members.iter().map(|m| self.is_super_type_of(m, sub)))
            }
            (_, Type::TemplateParam(tp)) => self.is_super_type_of(sup, &tp.bound),
            (_, Type::Mixed(_)) | (Type::TemplateParam(_), _) => Trinary::Maybe,
            (Type::Void, Type::Void) => Trinary::Yes,
            (Type::Void, _) | (_, Type::Void) => Trinary::No,
            (Type::Intersection(members), _) => {
                let results: Vec<Trinary> = members
                    .iter()
                    .map(|m| self.is_super_type_of(m, sub))
                    .collect();
                if results.contains(&Trinary::No) {
                    Trinary::No
                } else if results.iter().all(|r| *r == Trinary::Yes) {
                    Trinary::Yes
                } else {
                    Trinary::Maybe
                }
            }
            (
                Type::Float | Type::FloatLiteral(_),
                Type::Int | Type::IntLiteral(_) | Type::IntRange(..),
            ) => Trinary::No,
            _ if self.is_assignable(sub, sup) => Trinary::Yes,
            (_, Type::Intersection(members)) => {
                let refused = members
                    .iter()
                    .filter(|m| !matches!(m, Type::Accessory(_) | Type::HasOffsetValue(..)))
                    .any(|m| self.is_super_type_of(sup, m) == Trinary::No);
                if refused { Trinary::No } else { Trinary::Maybe }
            }
            (Type::Object(a, _), Type::Object(b, _)) => self.object_super_type_of(a, b),
            _ if types::types_definitely_disjoint(sup, sub) => Trinary::No,
            (Type::Accessory(kind), _) if accessory_refuses(kind, sub) => Trinary::No,
            _ => Trinary::Maybe,
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
                && self.is_super_type_of(n, m) == Trinary::Yes
                && self.is_super_type_of(m, n) != Trinary::Yes
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

    fn object_super_type_of(&self, sup: &str, sub: &str) -> Trinary {
        if sup == sub || sup == "object" || sub == "object" {
            return Trinary::Maybe;
        }
        let (Some((_, sup_info)), Some((_, sub_info))) =
            (self.lookup_class(sup), self.lookup_class(sub))
        else {
            return Trinary::Maybe;
        };
        if self.is_subclass_of(sup, sub) {
            return Trinary::Maybe;
        }
        // A `No` needs the whole hierarchy of both sides: an ancestor the
        // registry never indexed could still connect them.
        let hierarchy_known = |class: &str| {
            self.all_ancestor_names(class)
                .iter()
                .all(|a| self.lookup_class(a).is_some())
        };
        if !hierarchy_known(sup) || !hierarchy_known(sub) {
            return Trinary::Maybe;
        }
        let is_interface = |k: &ClassKind| matches!(k, ClassKind::Interface);
        if (is_interface(&sup_info.kind) && !sub_info.is_final)
            || (is_interface(&sub_info.kind) && !sup_info.is_final)
        {
            return Trinary::Maybe;
        }
        Trinary::No
    }
}

/// An accessory refinement no value of `sub`'s scalar family can carry
/// (`non-empty-string` over an `int`, `list` over a `string`).
fn accessory_refuses(kind: &AccessoryKind, sub: &Type) -> bool {
    let is_string = matches!(
        sub,
        Type::String | Type::StringLiteral(_) | Type::ClassString(_)
    );
    let is_array = matches!(sub, Type::Array(..) | Type::ArrayShape(_));
    match kind {
        AccessoryKind::NonEmptyArray | AccessoryKind::ArrayList => {
            !is_array
                && types::types_definitely_disjoint(
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
            !is_string && types::types_definitely_disjoint(&Type::String, sub)
        }
        _ => false,
    }
}

fn is_benevolent(members: &[Type]) -> bool {
    members.contains(&Type::Accessory(AccessoryKind::Benevolent))
}

fn benevolent_inner(members: &[Type]) -> Type {
    let rest: Vec<Type> = members
        .iter()
        .filter(|m| **m != Type::Accessory(AccessoryKind::Benevolent))
        .cloned()
        .collect();
    match rest.len() {
        1 => rest.into_iter().next().unwrap(),
        _ => Type::Intersection(rest),
    }
}
