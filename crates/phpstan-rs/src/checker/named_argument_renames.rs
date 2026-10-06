//! Real PHPStan's `NamedArgumentParameterMethodCallsCollector` (Phase
//! 82): the call-site half of one whole-project feature whose only
//! consumer is `MethodCallWithPossiblyRenamedNamedArgumentRule`
//! (`argument.parameterRenamedInSubtype`). A named-argument call site
//! against `Base::m(int $old)` is a real, `TypeError`-throwing runtime
//! bug the moment the *actual* receiver could be a `Sub extends Base`
//! that renamed `$old` to `$new` on override - PHP resolves a named
//! argument against whichever method is *actually* dispatched to, never
//! the statically-declared one.
//!
//! The main check pass records each analysed file's call sites here; the
//! overrides that rename a parameter are declarations, read from the
//! registry, and the cross-referencing of the two lives in
//! `project::whole_program::renamed_named_arguments`.

use rustc_hash::FxHashMap;
use types::Name;

use crate::checker::Checker;
use syntax::ast::{Arg, Visibility};
use syntax::span::Span;

/// (declaring class, method, parameter name as declared on that
/// signature) -> every named-argument call site in this one file using
/// that exact parameter name. Only ever populated for a call whose
/// resolved method is genuinely overridable (public/protected, and
/// neither the declaring class nor the method itself marked `final`) -
/// a private method, or one on/marked `final`, can never actually
/// dispatch to a different override at runtime, so no rename collision
/// is possible there at all.
pub(crate) type NamedArgumentCallFacts = FxHashMap<(Name, Name, Name), Vec<Span>>;

impl<'a> Checker<'a> {
    /// Drains this run's own `named_argument_call_facts`, which
    /// `Project::run_checker` returns with the file's other main-pass data.
    pub(crate) fn take_named_argument_call_facts(&mut self) -> NamedArgumentCallFacts {
        std::mem::take(&mut self.named_argument_call_facts)
    }

    /// Called from `ExprKind::MethodCall`'s own resolved-receiver arm
    /// (`checker/expr.rs`), once per real method call - every named
    /// argument this call uses against a
    /// genuinely overridable method is recorded, keyed by the method's
    /// real *declaring* class (`owner`, from `find_method_receiver`),
    /// not the receiver's own possibly-narrower static class - matching
    /// real PHPStan's own `calls[declaringClassName][methodName]
    /// [parameterName]` key exactly, since a rename anywhere between
    /// `owner` and the actual runtime instance is what makes the named
    /// argument ambiguous, regardless of which subtype the call site's
    /// own static receiver type happens to be.
    pub(crate) fn record_named_argument_call(
        &mut self,
        owner: &Name,
        method: &str,
        visibility: &Visibility,
        span: Span,
        args: &[Arg],
    ) {
        if *visibility == Visibility::Private {
            return;
        }
        if self
            .registry
            .class_by_name(owner.as_ref())
            .is_some_and(|info| info.is_final())
            || self.registry.method_is_final(owner.as_ref(), method)
        {
            return;
        }
        let method: Name = Name::from(method);
        for arg in args {
            let Some(name) = &arg.name else { continue };
            let name: Name = name.name.clone();
            // A call a loop's passes check again is still one call.
            let sites = self
                .named_argument_call_facts
                .entry((owner.clone(), method.clone(), name))
                .or_default();
            if !sites.contains(&span) {
                sites.push(span);
            }
        }
    }
}
