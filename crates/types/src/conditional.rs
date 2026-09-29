use rustc_hash::FxHashMap;

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
    /// resolved against `param_bindings`, the types a call passes for its
    /// parameters by name. A subject bound to a subtype of the target
    /// picks the true branch, one disjoint from it the false branch; an
    /// unbound or `mixed` subject, or anything in between, keeps both.
    pub fn resolve_conditional(
        &self,
        param_bindings: &FxHashMap<String, Type>,
        relation: &Relation,
    ) -> Type {
        syntax::stack::guard(|| match self {
            Type(Repr::Conditional {
                subject,
                target,
                if_true,
                if_false,
                negated,
            }) => match match subject {
                ConditionalSubject::Parameter(param) => param_bindings.get(param.as_str()),
                ConditionalSubject::Template(_) => None,
            } {
                Some(bound) if *bound != Type::mixed() => {
                    match conditional_branch(relation, bound, target, *negated) {
                        Some(true) => if_true.resolve_conditional(param_bindings, relation),
                        Some(false) => if_false.resolve_conditional(param_bindings, relation),
                        None => Type::union(
                            if_true.resolve_conditional(param_bindings, relation),
                            if_false.resolve_conditional(param_bindings, relation),
                        ),
                    }
                }
                _ => Type::union(
                    if_true.resolve_conditional(param_bindings, relation),
                    if_false.resolve_conditional(param_bindings, relation),
                ),
            },
            _ => self.map_children(&mut |t| t.resolve_conditional(param_bindings, relation)),
        })
    }
}
