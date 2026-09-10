//! Phase 42 Sub-item C: `TooWideFunctionThrowTypeRule`/
//! `TooWideMethodThrowTypeRule` (ROADMAP's own text drops the "TooWide"
//! prefix from the second name - this project keeps both, matching real
//! PHPStan's actual class names) - direct ports of `TooWideThrowTypeCheck
//! ::check()`, sharing one implementation here since the real port is
//! identical for a function or a method, differing only in the
//! already-formatted "Function foo()"/"Method Foo::bar()" subject text
//! each caller passes in.
//!
//! Real semantics (`vendor/phpstan-src/src/Rules/Exceptions/
//! TooWideThrowTypeCheck.php`, `exceptions.implicitThrows: true` by
//! default - `vendor/phpstan-src/conf/config.neon` - so every recorded
//! throw point always counts, explicit or not): union every recorded
//! throw point into one "actually thrown" type; a declared `@throws`
//! member is flagged `throws.unusedType` when nothing in that union could
//! plausibly satisfy it. When *nothing* was thrown at all, every declared
//! member is flagged (mirrors the real check's own `NeverType` special
//! case exactly).
//!
//! Deliberately more conservative than the real check in one disclosed
//! way: `Checker::current_function_has_unannotated_call` (set whenever a
//! call resolves to no known throw information at all) suppresses this
//! whole comparison for the declaration - see that field's own doc
//! comment for why.

use crate::checker::Checker;
use crate::checker::type_union::union_types;
use crate::token::Span;
use crate::types::Type;

fn flatten_union(ty: &Type) -> Vec<&Type> {
    match ty {
        Type::Union(members) => members.iter().collect(),
        other => vec![other],
    }
}

impl<'a> Checker<'a> {
    /// `throws` is this declaration's own already-resolved `@throws` tag
    /// (the same `Some(t)` `check_throws_tag_validity`'s own caller
    /// already has in hand) - called once, after the declaration's whole
    /// body has been checked (so `current_function_throw_points`/
    /// `current_function_has_unannotated_call` are fully populated), and
    /// before either is reset for the next declaration.
    pub(crate) fn check_too_wide_throw_type(
        &mut self,
        throws: &Type,
        subject: &str,
        span: Span,
        label: &str,
    ) {
        if *throws == Type::Void || self.current_function_has_unannotated_call {
            return;
        }
        let actual = self
            .current_function_throw_points
            .iter()
            .cloned()
            .reduce(union_types);
        let members: Vec<Type> = flatten_union(throws).into_iter().cloned().collect();
        for declared in members {
            let is_used = match &actual {
                None => false,
                Some(actual) => self.registry.is_assignable(actual, &declared),
            };
            if is_used {
                continue;
            }
            self.report(
                label.to_string(),
                format!("{subject} has {declared} in PHPDoc @throws tag but it's not thrown."),
                span,
                2,
                "throws.unusedType",
            );
        }
    }
}
