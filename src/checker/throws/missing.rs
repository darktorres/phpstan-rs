//! Phase 42 bonus item: an opt-in "missing checked exception" family -
//! not a port of any real PHPStan rule (checked-exception enforcement
//! isn't part of core `phpstan/phpstan` at all), but a natural mirror-
//! image of Sub-item C's own `TooWideFunctionThrowTypeRule`/
//! `TooWideMethodThrowTypeRule` port: instead of flagging a declared
//! `@throws` member nothing actually produces, this flags a real,
//! explicit `throw` the declaration's own `@throws` tag doesn't cover.
//!
//! Gated on `Registry::check_missing_checked_exceptions` (default
//! `false`) - see that field's own doc comment for why this is this
//! phase's own riskiest item, and stays opt-in.
//!
//! Deliberately narrower than Sub-item C's own comparison in the
//! direction that matters for a "did I forget to document this" check:
//! only `Checker::current_function_explicit_throw_points` (a real
//! `throw` written directly in *this* declaration's own body) is
//! consulted, never a call's own implicit contribution to `current_
//! function_throw_points` - a function you call declaring its own
//! `@throws` is that function's own author's responsibility to document,
//! not evidence that *you* forgot to. Also unlike Sub-item C, a class the
//! registry has no entry for at all, or anything not a plain `Type::
//! Object` (a `Union`, `Mixed`, ...), is skipped rather than guessed -
//! "don't know, don't guess," this project's usual leniency, matters
//! more here than in Sub-item C's direction (a false "too wide" costs a
//! stray suggestion to narrow a tag; a false "missing" costs a stray
//! demand to widen one that could easily be wrong).

use crate::checker::Checker;
use crate::types::Type;

impl<'a> Checker<'a> {
    /// `throws` is this declaration's own already-resolved `@throws` tag
    /// (`None` when there's no tag at all) - called once, after the whole
    /// body has been checked, the same choke point `check_too_wide_
    /// throw_type` runs from.
    pub(crate) fn check_missing_checked_exceptions(
        &mut self,
        throws: Option<&Type>,
        subject: &str,
        label: &str,
    ) {
        if !self.registry.check_missing_checked_exceptions {
            return;
        }
        for (thrown, span) in self.current_function_explicit_throw_points.clone() {
            let Type::Object(class, _) = &thrown else {
                continue;
            };
            if !self.registry.classes.contains_key(class) {
                continue;
            }
            let covered = throws.is_some_and(|t| self.registry.is_assignable(&thrown, t));
            if covered {
                continue;
            }
            self.report(
                label.to_string(),
                format!("{subject} throws {class} but it's not covered by any @throws tag."),
                span,
                2,
                "throws.missing",
            );
        }
    }
}
