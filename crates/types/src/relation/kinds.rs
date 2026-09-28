//! What a type can be used as, over the class hierarchy: an instance of a
//! class, iterable, offset-accessible, destructurable, callable.

use syntax::ast::ClassKind;

use crate::{Relation, TrinaryLogic, Type};

impl Relation<'_> {
    /// PHPStan's `isInstanceOf()`: `Yes` for an object of `class` or a
    /// subclass, `No` for any other registered class that isn't an
    /// interface and for a non-object, `Maybe` for an interface, an
    /// unregistered class, or anything unknown.
    pub fn is_instance_of(&self, ty: &Type, class: &str) -> TrinaryLogic {
        self.answer(ty, &mut |leaf| match leaf {
            Type::Object(name, _) | Type::EnumCase(name, _) => self.class_is(name, class),
            _ if leaf.is_object().yes() || leaf.is_mixed() => TrinaryLogic::Maybe,
            _ if leaf.is_object().no() => TrinaryLogic::No,
            _ => TrinaryLogic::Maybe,
        })
    }

    /// PHPStan's `isIterable()`: arrays, and objects that are
    /// `Traversable`.
    pub fn is_iterable(&self, ty: &Type) -> TrinaryLogic {
        self.answer(ty, &mut |leaf| {
            if leaf.is_array().yes() || matches!(leaf, Type::HasOffsetValue(..)) {
                TrinaryLogic::Yes
            } else if is_scalar_or_null(leaf) {
                TrinaryLogic::No
            } else if let Type::Object(name, _) | Type::EnumCase(name, _) = leaf {
                self.class_is(name, "Traversable")
            } else {
                TrinaryLogic::Maybe
            }
        })
    }

    /// PHPStan's `isOffsetAccessible()`: arrays, strings, `null` (a write
    /// creates the array), and objects that are `ArrayAccess`.
    pub fn is_offset_accessible(&self, ty: &Type) -> TrinaryLogic {
        self.answer(ty, &mut |leaf| {
            if leaf.is_array().yes()
                || matches!(leaf, Type::HasOffsetValue(..))
                || leaf.is_string().yes()
                || leaf.is_null().yes()
            {
                TrinaryLogic::Yes
            } else if is_scalar_or_null(leaf) {
                TrinaryLogic::No
            } else if let Type::Object(name, _) | Type::EnumCase(name, _) = leaf {
                self.class_is(name, "ArrayAccess")
            } else {
                TrinaryLogic::Maybe
            }
        })
    }

    /// Whether `[$a, $b] = $x` can read `$x`: an array, or an object that
    /// is `ArrayAccess` (`ArrayDestructuringRule`). A string can't be
    /// destructured.
    pub fn is_destructurable(&self, ty: &Type) -> TrinaryLogic {
        self.answer(ty, &mut |leaf| {
            if leaf.is_array().yes() || matches!(leaf, Type::HasOffsetValue(..)) {
                TrinaryLogic::Yes
            } else if is_scalar_or_null(leaf) {
                TrinaryLogic::No
            } else if let Type::Object(name, _) | Type::EnumCase(name, _) = leaf {
                self.class_is(name, "ArrayAccess")
            } else {
                TrinaryLogic::Maybe
            }
        })
    }

    /// PHPStan's `isCallable()`, as far as it can say No: never a
    /// boolean, number, `null`, `void` or resource, nor an object of a
    /// registered class that isn't an interface or `Closure` and has no
    /// `__invoke()`. A string or an array may name a callable.
    pub fn is_callable(&self, ty: &Type) -> TrinaryLogic {
        self.answer(ty, &mut |leaf| match leaf {
            Type::Null
            | Type::Void
            | Type::Bool
            | Type::True
            | Type::False
            | Type::Int
            | Type::IntLiteral(_)
            | Type::IntRange(..)
            | Type::Float
            | Type::FloatLiteral(_)
            | Type::Resource { .. } => TrinaryLogic::No,
            Type::Object(name, _) | Type::EnumCase(name, _) => {
                if name.as_str() == "Closure" || self.hierarchy.has_method(name, "__invoke") {
                    TrinaryLogic::Yes
                } else if self
                    .hierarchy
                    .find_class(name)
                    .is_some_and(|facts| facts.kind != ClassKind::Interface)
                {
                    TrinaryLogic::No
                } else {
                    TrinaryLogic::Maybe
                }
            }
            Type::Callable(_) => TrinaryLogic::Yes,
            _ => TrinaryLogic::Maybe,
        })
    }

    /// Whether `class` is `of` for these questions: `Yes` when it is or
    /// extends it, `No` when it's a registered class that isn't an
    /// interface, `Maybe` otherwise.
    fn class_is(&self, class: &str, of: &str) -> TrinaryLogic {
        if self.hierarchy.is_subclass_of(class, of) {
            TrinaryLogic::Yes
        } else if self
            .hierarchy
            .find_class(class)
            .is_some_and(|facts| facts.kind != ClassKind::Interface)
        {
            TrinaryLogic::No
        } else {
            TrinaryLogic::Maybe
        }
    }

    /// A question asked of each leaf of `ty`: a union's members all (its
    /// `null` set aside while anything else is left), an intersection's
    /// any, a template's bound, a conditional's branches all.
    fn answer(&self, ty: &Type, leaf: &mut dyn FnMut(&Type) -> TrinaryLogic) -> TrinaryLogic {
        match ty {
            Type::Union(members) | Type::BenevolentUnion(members) => {
                let non_null: Vec<&Type> = members.iter().filter(|m| **m != Type::Null).collect();
                let considered: Vec<&Type> = if non_null.is_empty() {
                    members.iter().collect()
                } else {
                    non_null
                };
                considered
                    .into_iter()
                    .map(|m| self.answer(m, leaf))
                    .reduce(TrinaryLogic::and)
                    .unwrap_or(TrinaryLogic::Maybe)
            }
            Type::Intersection(members) => members
                .iter()
                .map(|m| self.answer(m, leaf))
                .reduce(TrinaryLogic::or)
                .unwrap_or(TrinaryLogic::Maybe),
            Type::TemplateParam(param) => self.answer(&param.bound, leaf),
            Type::Conditional {
                if_true, if_false, ..
            } => self.answer(if_true, leaf).and(self.answer(if_false, leaf)),
            other => leaf(other),
        }
    }
}

/// A boolean, number, string, `null` or `void`.
fn is_scalar_or_null(ty: &Type) -> bool {
    ty.is_scalar().yes() || ty.is_null().yes() || matches!(ty, Type::Void)
}
