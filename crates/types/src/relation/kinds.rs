//! What a type can be used as, over the class hierarchy: an instance of a
//! class, iterable, offset-accessible, destructurable, callable.

use syntax::ast::ClassKind;

use crate::{Relation, Repr, TrinaryLogic, Type};

impl Relation<'_> {
    /// PHPStan's `isInstanceOf()`: `Yes` for an object of `class` or a
    /// subclass, `No` for any other registered class that isn't an
    /// interface and for a non-object, `Maybe` for an interface, an
    /// unregistered class, or anything unknown.
    pub fn is_instance_of(&self, ty: &Type, class: &str) -> TrinaryLogic {
        self.answer(ty, &mut |leaf| match leaf {
            Type(Repr::Object(name, _)) | Type(Repr::EnumCase(name, _)) => {
                self.class_is(name, class)
            }
            _ if leaf.is_object().yes() || leaf.is_mixed() => TrinaryLogic::Maybe,
            _ if leaf.is_object().no() => TrinaryLogic::No,
            _ => TrinaryLogic::Maybe,
        })
    }

    /// PHPStan's `isIterable()`: arrays, and objects that are
    /// `Traversable`.
    pub fn is_iterable(&self, ty: &Type) -> TrinaryLogic {
        self.answer(ty, &mut |leaf| {
            if leaf.is_array().yes() || matches!(leaf, Type(Repr::HasOffsetValue(..))) {
                TrinaryLogic::Yes
            } else if is_scalar_or_null(leaf) {
                TrinaryLogic::No
            } else if let Type(Repr::Object(name, _)) | Type(Repr::EnumCase(name, _)) = leaf {
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
                || matches!(leaf, Type(Repr::HasOffsetValue(..)))
                || leaf.is_string().yes()
                || leaf.is_null().yes()
            {
                TrinaryLogic::Yes
            } else if is_scalar_or_null(leaf) {
                TrinaryLogic::No
            } else if let Type(Repr::Object(name, _)) | Type(Repr::EnumCase(name, _)) = leaf {
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
            if leaf.is_array().yes() || matches!(leaf, Type(Repr::HasOffsetValue(..))) {
                TrinaryLogic::Yes
            } else if is_scalar_or_null(leaf) {
                TrinaryLogic::No
            } else if let Type(Repr::Object(name, _)) | Type(Repr::EnumCase(name, _)) = leaf {
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
            Type(Repr::Null)
            | Type(Repr::Void)
            | Type(Repr::Bool)
            | Type(Repr::True)
            | Type(Repr::False)
            | Type(Repr::Int)
            | Type(Repr::IntLiteral(_))
            | Type(Repr::IntRange(..))
            | Type(Repr::Float)
            | Type(Repr::FloatLiteral(_))
            | Type(Repr::Resource { .. }) => TrinaryLogic::No,
            Type(Repr::Object(name, _)) | Type(Repr::EnumCase(name, _)) => {
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
            Type(Repr::Callable(_)) => TrinaryLogic::Yes,
            _ => TrinaryLogic::Maybe,
        })
    }

    /// PHPStan's `canAccessProperties()`: objects can, booleans,
    /// numbers, strings, `null` and arrays can't.
    pub fn can_access_properties(&self, ty: &Type) -> TrinaryLogic {
        self.answer(ty, &mut |leaf| object_like(leaf))
    }

    /// PHPStan's `canCallMethods()`: as `can_access_properties`, except
    /// that a `stdClass` has no methods to call.
    pub fn can_call_methods(&self, ty: &Type) -> TrinaryLogic {
        self.answer(ty, &mut |leaf| match leaf {
            Type(Repr::Object(class, _)) if class.as_str() == "stdClass" => TrinaryLogic::No,
            _ => object_like(leaf),
        })
    }

    /// Whether a value of `ty` can name a class for `new $x` or `$x::m()`:
    /// an object or a string can, a boolean, number, `null`, `void` or
    /// array can't.
    pub fn can_name_a_class(&self, ty: &Type) -> TrinaryLogic {
        self.answer(ty, &mut |leaf| {
            if leaf.is_object().yes() || leaf.is_string().yes() {
                TrinaryLogic::Yes
            } else if is_scalar_or_null(leaf)
                || leaf.is_array().yes()
                || matches!(leaf, Type(Repr::HasOffsetValue(..)))
            {
                TrinaryLogic::No
            } else {
                TrinaryLogic::Maybe
            }
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

    /// A question asked of each leaf of `ty`, answered for the whole as
    /// PHPStan does: a union's (and a conditional's branches') answers
    /// agree or give `Maybe`, an intersection needs one member's `Yes`,
    /// a template answers for its bound.
    fn answer(&self, ty: &Type, leaf: &mut dyn FnMut(&Type) -> TrinaryLogic) -> TrinaryLogic {
        match ty {
            Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members)) => {
                let answers: Vec<TrinaryLogic> =
                    members.iter().map(|m| self.answer(m, leaf)).collect();
                TrinaryLogic::extreme_identity(answers.into_iter())
            }
            Type(Repr::Intersection(members)) => members
                .iter()
                .map(|m| self.answer(m, leaf))
                .reduce(TrinaryLogic::or)
                .unwrap_or(TrinaryLogic::Maybe),
            Type(Repr::TemplateParam(param)) => self.answer(&param.bound, leaf),
            Type(Repr::Conditional {
                if_true, if_false, ..
            }) => {
                let answers = [self.answer(if_true, leaf), self.answer(if_false, leaf)];
                TrinaryLogic::extreme_identity(answers.into_iter())
            }
            other => leaf(other),
        }
    }
}

/// A boolean, number, string, `null` or `void`.
fn is_scalar_or_null(ty: &Type) -> bool {
    ty.is_scalar().yes() || ty.is_null().yes() || matches!(ty, Type(Repr::Void))
}

/// `Yes` for an object, `No` for a value that is never one.
fn object_like(ty: &Type) -> TrinaryLogic {
    if ty.is_object().yes() {
        TrinaryLogic::Yes
    } else if is_scalar_or_null(ty)
        || ty.is_array().yes()
        || matches!(ty, Type(Repr::HasOffsetValue(..)))
    {
        TrinaryLogic::No
    } else {
        TrinaryLogic::Maybe
    }
}
