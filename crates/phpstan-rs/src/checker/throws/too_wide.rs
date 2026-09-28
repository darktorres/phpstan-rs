//! Flags a declared `@throws` member that nothing in the body actually
//! throws. Shares one implementation for a function or a method, since
//! the check is identical, differing only in the already-formatted
//! "Function foo()"/"Method Foo::bar()" subject text each caller passes
//! in.
//!
//! Every recorded throw point always counts, explicit or not: union
//! every recorded throw point into one "actually thrown" type; a
//! declared `@throws` member is flagged `throws.unusedType` when nothing
//! in that union could plausibly satisfy it. When nothing was thrown at
//! all, every declared member is flagged.
//!
//! More conservative than a full model in one way:
//! `Checker::current_function_has_unannotated_call` (set whenever a call
//! resolves to no known throw information at all) suppresses this whole
//! comparison for the declaration.

use crate::checker::Checker;
use syntax::ast::Visibility;
use syntax::token::Span;
use types::Type;

impl<'a> Checker<'a> {
    /// A `private` method, a method on a `final` class, or a method
    /// itself declared `final` always gets checked; everything else is
    /// silent by default (a subclass overriding this method might
    /// genuinely need the wider declared type) unless `RulesSettings::
    /// check_too_wide_throw_type_in_protected_and_public_methods` is on,
    /// in which case an overridable method still gets a pass the first
    /// time it's declared - only a re-declaration narrowing an
    /// ancestor's own wider `@throws` is checked. Only called for a
    /// method (`class_name` is a real, already-resolved declaring class);
    /// a plain top-level function has no such gate.
    ///
    /// Doesn't model a separate "is this `@throws` tag inherited vs.
    /// written on this exact declaration" gate: `FunctionReflection::throws` has
    /// no "borrowed from an ancestor" provenance bit, so every `throws`
    /// this function sees is already this declaration's own explicit tag.
    pub(crate) fn should_check_too_wide_method_throw_type(
        &self,
        class_name: &str,
        method_name: &str,
        visibility: Visibility,
    ) -> bool {
        if visibility == Visibility::Private {
            return true;
        }
        let Some(info) = self.registry.classes.get(class_name) else {
            return true;
        };
        if info.is_final() || info.final_methods().contains(method_name) {
            return true;
        }
        if !self
            .settings
            .rules
            .check_too_wide_throw_type_in_protected_and_public_methods
        {
            return false;
        }
        // Not the first declaration - some ancestor interface/class/trait-
        // abstract already declares this method, so this is a real
        // override narrowing an inherited (possibly wider) throw type.
        self.registry
            .find_method_signature_prototype(class_name, method_name)
            .is_some()
    }

    /// `throws` is this declaration's already-resolved `@throws` tag -
    /// called once, after the whole body has been checked (so
    /// `current_function_throw_points`/`current_function_has_
    /// unannotated_call` are fully populated), and before either resets
    /// for the next declaration.
    pub(crate) fn check_too_wide_throw_type(&mut self, throws: &Type, subject: &str, span: Span) {
        if throws.is_void() || self.current_function_has_unannotated_call {
            return;
        }
        let actual = self
            .current_function_throw_points
            .iter()
            .cloned()
            .reduce(Type::union);
        let members: Vec<Type> = throws.union_members().to_vec();
        for declared in members {
            // Calling `accepts(actual, &declared)` directly would be
            // wrong once `actual` is a `Union` of several distinct throw
            // points (any call whose signature documents a different,
            // unrelated `@throws` type contributes one):
            // `accepts`'s `Union`-in-sub-position rule requires
            // *every* member to satisfy the supertype, but the real
            // question here is "was `declared` (or something it covers)
            // thrown *anywhere* in this body," not "is everything this
            // body could throw covered by `declared`." Flattening
            // `actual` and checking each member independently answers
            // that: is there *some* actual throw point this declared type
            // covers.
            let is_used = actual
                .as_ref()
                .map(Type::union_members)
                .unwrap_or_default()
                .iter()
                .any(|a| self.accepts(&declared, a).yes());
            if is_used {
                continue;
            }
            self.report(
                format!("{subject} has {declared} in PHPDoc @throws tag but it's not thrown."),
                span,
                2,
                "throws.unusedType",
            );
        }
    }
}
