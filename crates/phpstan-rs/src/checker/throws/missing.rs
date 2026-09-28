//! An opt-in "missing checked exception" family - not a port of any
//! real PHPStan rule (checked-exception enforcement isn't part of core
//! `phpstan/phpstan` at all), but a mirror image of
//! `TooWideFunctionThrowTypeRule`/`TooWideMethodThrowTypeRule`: instead
//! of flagging a declared `@throws` member nothing actually produces,
//! this flags a real, explicit `throw` the declaration's own `@throws`
//! tag doesn't cover.
//!
//! Gated on `ExtrasSettings::check_missing_checked_exceptions` (default
//! `false`) - see that field's own doc comment for why this is the
//! riskiest of this family, and stays opt-in.
//!
//! Deliberately narrower than the too-wide check in the direction that
//! matters for a "did I forget to document this" check: only
//! `Checker::current_function_explicit_throw_points` (a real `throw`
//! written directly in *this* declaration's own body) is consulted,
//! never a call's own implicit contribution to
//! `current_function_throw_points` - a function you call declaring its
//! own `@throws` is that function's own author's responsibility to
//! document, not evidence that *you* forgot to. Also unlike the too-wide
//! check, a class the registry has no entry for at all, or anything not
//! a plain `Type::Object` (a `Union`, `Mixed`, ...), is skipped rather
//! than guessed - "don't know, don't guess" matters more here (a false
//! "too wide" costs a stray suggestion to narrow a tag; a false
//! "missing" costs a stray demand to widen one that could easily be
//! wrong).

use crate::checker::Checker;
use types::Type;

impl<'a> Checker<'a> {
    /// `throws` is this declaration's own already-resolved `@throws` tag
    /// (`None` when there's no tag at all) - called once, after the whole
    /// body has been checked, the same choke point `check_too_wide_
    /// throw_type` runs from.
    pub(crate) fn check_missing_checked_exceptions(
        &mut self,
        throws: Option<&Type>,
        subject: &str,
    ) {
        if !self.settings.extras.check_missing_checked_exceptions {
            return;
        }
        for (thrown, span) in self.current_function_explicit_throw_points.clone() {
            let Some((class, _)) = thrown.object_class_and_args() else {
                continue;
            };
            if !self.registry.classes.contains_key(class.as_str()) {
                continue;
            }
            let covered = throws.is_some_and(|t| self.accepts(t, &thrown).yes());
            if covered {
                continue;
            }
            self.report(
                format!("{subject} throws {class} but it's not covered by any @throws tag."),
                span,
                2,
                "throws.missing",
            );
        }
    }
}
