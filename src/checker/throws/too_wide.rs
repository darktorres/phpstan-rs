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

use crate::ast::Visibility;
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
    /// Phase 42 gating fix: real PHPStan's own `TooWideMethodThrowTypeRule`
    /// nested gate (`vendor/phpstan-src/src/Rules/Exceptions/
    /// TooWideMethodThrowTypeRule.php::processNode`, confirmed by direct
    /// read): a `private` method, a method on a `final` class, or a method
    /// itself declared `final` always gets checked; everything else is
    /// silent by default (a real, corpus-confirmed false-positive flood
    /// otherwise - a subclass overriding this method might genuinely need
    /// the wider declared type) unless `Registry::check_too_wide_throw_
    /// type_in_protected_and_public_methods` is on, in which case an
    /// overridable method still gets a pass the *first* time it's
    /// declared (`isFirstDeclaration` in the real rule) - only a
    /// re-declaration narrowing an ancestor's own wider `@throws` is
    /// checked. `TooWideFunctionThrowTypeRule` (a plain top-level
    /// function) has no such gate at all - this is only ever called for a
    /// method (`class_name` is a real, already-resolved declaring class).
    ///
    /// Deliberately doesn't model the real rule's separate `isThrowType
    /// Explicit`/`tooWideImplicitThrows` gate: that gate only ever matters
    /// when a `@throws` tag can be *inherited* rather than written on this
    /// exact declaration's own docblock, a distinction real PHPStan draws
    /// via `FileTypeMapper`'s own per-declaration doc resolution - this
    /// project's `FunctionSig::throws` has no such "borrowed from an
    /// ancestor" provenance bit at all (unlike `return_has_own_doc`), so
    /// every `throws` this function ever sees is already this
    /// declaration's own, explicit tag - the real gate's `true` branch,
    /// unconditionally.
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
        if info.is_final || info.final_methods.contains(method_name) {
            return true;
        }
        if !self
            .registry
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
            // A real, previously-live bug found and fixed this session:
            // this used to call `is_assignable(actual, &declared)`
            // directly - correct when only one thing is ever thrown, but
            // wrong the moment `actual` is a real `Union` of *several*
            // distinct throw points (any call whose own signature
            // documents a different, unrelated `@throws` type
            // contributes one - `ReflectionClass`'s own constructor is a
            // real corpus example, `Piwigo\Config\ConfigLoader::
            // validateRequired`). `is_assignable`'s own `Union`-in-sub-
            // position rule (correctly, for an ordinary assignability
            // check) requires *every* member to satisfy the supertype -
            // but this check's own real question is "was `declared`
            // (or something it covers) thrown *anywhere* in this body,"
            // not "is everything this body could ever throw covered by
            // `declared`" - the latter wrongly demanded the *unrelated*
            // throw point also satisfy `declared`, flagging a real,
            // genuinely-thrown declared type as unused. Flattening
            // `actual` the same way `throws` already is just above and
            // checking each of its own members independently answers the
            // real question: is there *some* actual throw point this
            // declared type covers.
            let is_used = actual
                .as_ref()
                .map(flatten_union)
                .unwrap_or_default()
                .into_iter()
                .any(|a| self.registry.is_assignable(a, &declared));
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
