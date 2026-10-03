use crate::{ConditionalSubject, Relation, Repr, Type};

/// Which branch a conditional takes for a subject of type `bound`:
/// `Some(true)` the `if_true` one, `Some(false)` the other, `None` both.
/// Real PHPStan's `ConditionalType`/`ConditionalTypeForParameter` are
/// trinary: `target->isSuperTypeOf(subject)` yes picks one branch, no
/// the other, and maybe (a possibly-empty `array<T>` checked against
/// `non-empty-array<T>`) keeps both. `relation` knows the classes, so
/// `$x is Foo` picks a branch for a subclass or an unrelated final class.
pub(crate) fn conditional_branch(
    relation: &Relation,
    bound: &Type,
    target: &Type,
    negated: bool,
) -> Option<bool> {
    let matches = match relation.is_super_type_of(target, bound) {
        crate::TrinaryLogic::Yes => Some(true),
        crate::TrinaryLogic::No => Some(false),
        crate::TrinaryLogic::Maybe => None,
    };
    matches.map(|m| m != negated)
}

impl Type {
    /// This type with every parameter-subject conditional at any depth
    /// resolved against what a call passes: `argument` gives the type
    /// passed for the parameter of a name, if any. A subject bound to a
    /// subtype of the target picks the true branch, one disjoint from it
    /// the false branch; an unbound or `mixed` subject, or anything in
    /// between, keeps both.
    pub fn resolve_conditional<'a>(
        &self,
        argument: &dyn Fn(&str) -> Option<&'a Type>,
        relation: &Relation,
    ) -> Type {
        if self.0.flags & crate::HAS_CONDITIONAL == 0 && crate::shortcuts_enabled() {
            return self.clone();
        }
        syntax::stack::guard(|| match self.repr() {
            Repr::Conditional {
                subject,
                target,
                if_true,
                if_false,
                negated,
            } => match match subject {
                ConditionalSubject::Parameter(param) => argument(param.as_str()),
                ConditionalSubject::Template(_) => None,
            } {
                Some(bound) if *bound != Type::mixed() => {
                    match conditional_branch(relation, bound, target, *negated) {
                        Some(true) => if_true.resolve_conditional(argument, relation),
                        Some(false) => if_false.resolve_conditional(argument, relation),
                        None => Type::union(
                            if_true.resolve_conditional(argument, relation),
                            if_false.resolve_conditional(argument, relation),
                        ),
                    }
                }
                _ => Type::union(
                    if_true.resolve_conditional(argument, relation),
                    if_false.resolve_conditional(argument, relation),
                ),
            },
            _ => self.map_children(&mut |t| t.resolve_conditional(argument, relation)),
        })
    }
}
