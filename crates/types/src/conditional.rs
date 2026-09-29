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
    /// Resolves every conditional node in this type against `param_bindings`
    /// (parameter *name* -> the actual, already-inferred type bound to it at
    /// this call site - a genuinely different keyspace from `TemplateBindings`,
    /// which is keyed by template *name*, to avoid a real if rare name
    /// collision between the two). A genuinely separate mechanism from
    /// `Type::substitute_templates` (not folded into it): that function resolves
    /// `TemplateParam` nodes against a template-name-keyed map, this one
    /// resolves `Conditional` nodes against a parameter-name-keyed one - the
    /// two node kinds carry unrelated information and can appear nested inside
    /// each other (a conditional's own branch mentioning a template, or vice
    /// versa), so this recurses through the identical structural wrappers
    /// `Type::substitute_templates` already covers (`Array`/`Union`/`ClassString`/
    /// `Intersection`/`ArrayShape`/`ObjectShape`/`Object`/`Callable`) to reach
    /// every nested `Conditional`, not just a top-level one.
    ///
    /// For a `Conditional` node: `param_bindings.get(subject)` - unbound, or
    /// bound to `Mixed` (the "genuinely unknown argument type" sentinel this
    /// project uses throughout) - falls back to the safe two-branch union
    /// (also, itself, recursively resolved - a real, if rarer, nested-
    /// conditional shape). A real bound resolves trinarily: a subtype of
    /// `target` picks one branch, a provably disjoint one the other, and
    /// anything else keeps both (see the arm's own comment).
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
