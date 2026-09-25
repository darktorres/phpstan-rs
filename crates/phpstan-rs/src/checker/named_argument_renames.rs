//! Real PHPStan's `NamedArgumentParameterMethodCallsCollector`/
//! `OverridingMethodRenamesParameterCollector` pair (Phase 82) - a
//! single whole-project registry-fixed-point feature, not two
//! independent ones, since both only ever feed one consuming rule
//! (`MethodCallWithPossiblyRenamedNamedArgumentRule`,
//! `argument.parameterRenamedInSubtype`): a named-argument call site
//! against `Base::m(int $old)` is a real, `TypeError`-throwing runtime
//! bug the moment the *actual* receiver could be a `Sub extends Base`
//! that renamed `$old` to `$new` on override - PHP resolves a named
//! argument against whichever method is *actually* dispatched to, never
//! the statically-declared one.
//!
//! This module only ever *records facts* (into `Checker::named_
//! argument_call_facts`/`renamed_parameter_override_facts`) - the real
//! cross-referencing (`project::whole_program::renamed_named_arguments`)
//! only makes sense once every file's own facts have been merged
//! project-wide (`Project::collect_named_argument_facts`), the same
//! two-pass shape `property_assign_facts`'s own architecture already
//! established, just gathering two independent fact kinds from the same
//! collection pass instead of one.
//!
//! Both recording sites must stay reachable in `Checker::facts_only`
//! mode (the collection pass's whole point) - `record_named_argument_
//! call` lives inside `ExprKind::MethodCall`'s own always-on body-
//! checking path already, no extra wiring needed; `record_named_
//! argument_renames` is deliberately called *outside*
//! `check_method_overrides`'s own `if !self.facts_only` gate (`checker.
//! rs`'s `ClassDecl` handling) - it duplicates that function's own two
//! iteration shapes (a class's own textually-declared methods, plus a
//! used trait's method it never re-declares) rather than reusing
//! `check_one_method_override` itself, since that function's own
//! surrounding diagnostic work is exactly what the facts-only pass
//! exists to skip.

use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::checker::Checker;
use syntax::ast::{Arg, ClassDecl, Visibility};
use syntax::token::Span;

/// (declaring class, method, parameter name as declared on that
/// signature) -> every named-argument call site in this one file using
/// that exact parameter name. Only ever populated for a call whose
/// resolved method is genuinely overridable (public/protected, and
/// neither the declaring class nor the method itself marked `final`) -
/// a private method, or one on/marked `final`, can never actually
/// dispatch to a different override at runtime, so no rename collision
/// is possible there at all.
pub(crate) type NamedArgumentCallFacts = FxHashMap<(Arc<str>, Arc<str>, Arc<str>), Vec<Span>>;

/// One override anywhere in this file that renames a parameter
/// relative to its own nearest ancestor prototype (`Registry::find_
/// method_signature_prototype`), at the same positional slot.
#[derive(Debug, Clone)]
pub(crate) struct RenamedParameterOverride {
    pub(crate) prototype_class: Arc<str>,
    pub(crate) method: Arc<str>,
    pub(crate) prototype_param: Arc<str>,
    pub(crate) overriding_class: Arc<str>,
    pub(crate) overriding_param: Arc<str>,
}

pub(crate) type RenamedParameterOverrideFacts = Vec<RenamedParameterOverride>;

impl<'a> Checker<'a> {
    /// Drains this run's own `named_argument_call_facts` - `Project::
    /// collect_named_argument_facts`'s own per-file collection loop
    /// reads this back, tagging each span with the file it came from
    /// (a `Checker` has no notion of its own file path).
    pub(crate) fn take_named_argument_call_facts(&mut self) -> NamedArgumentCallFacts {
        std::mem::take(&mut self.named_argument_call_facts)
    }

    /// Drains this run's own `renamed_parameter_override_facts` - same
    /// collection shape as `take_named_argument_call_facts`, but these
    /// facts need no per-file location of their own (the eventual
    /// diagnostic is reported at the *call site*, never at the override
    /// declaration).
    pub(crate) fn take_renamed_parameter_override_facts(
        &mut self,
    ) -> RenamedParameterOverrideFacts {
        std::mem::take(&mut self.renamed_parameter_override_facts)
    }

    /// Called from `ExprKind::MethodCall`'s own resolved-receiver arm
    /// (`checker/expr.rs`), once per real method call, regardless of
    /// `facts_only` - every named argument this call uses against a
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
        owner: &Arc<str>,
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
            .classes
            .get(owner.as_ref())
            .is_some_and(|info| info.is_final || info.final_methods.contains(method))
        {
            return;
        }
        let method: Arc<str> = syntax::intern::intern(method);
        for arg in args {
            let Some(name) = &arg.name else { continue };
            let name: Arc<str> = syntax::intern::intern(name);
            self.named_argument_call_facts
                .entry((owner.clone(), method.clone(), name))
                .or_default()
                .push(span);
        }
    }

    /// The always-on companion to `check_method_overrides` - see this
    /// module's own doc comment for why it's a deliberately separate,
    /// leaner duplicate of that function's own two iteration shapes
    /// rather than a shared call reused by both.
    pub(crate) fn record_named_argument_renames(&mut self, fqn: &str, c: &ClassDecl) {
        for m in &c.methods {
            self.record_one_named_argument_rename(fqn, &m.function.name);
        }
        if !c.uses.is_empty() {
            let Some(info) = self.registry.classes.get(fqn) else {
                return;
            };
            let own_method_names: rustc_hash::FxHashSet<&str> =
                c.methods.iter().map(|m| m.function.name.as_ref()).collect();
            let mut trait_method_names: Vec<Arc<str>> = info
                .methods
                .keys()
                .filter(|name| {
                    name.as_ref() != "__construct"
                        && !own_method_names.contains(name.as_ref())
                        && !info.abstract_methods.contains(*name)
                })
                .cloned()
                .collect();
            trait_method_names.sort();
            for name in trait_method_names {
                self.record_one_named_argument_rename(fqn, &name);
            }
        }
    }

    fn record_one_named_argument_rename(&mut self, fqn: &str, method_name: &str) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let Some(own_sig) = info.methods.get(method_name) else {
            return;
        };
        let Some((ancestor_owner, ancestor_sig, _)) = self
            .registry
            .find_method_signature_prototype(fqn, method_name)
        else {
            return;
        };
        if ancestor_sig.visibility == Visibility::Private {
            return;
        }
        for (i, ancestor_param) in ancestor_sig.params.iter().enumerate() {
            let Some(own_param) = own_sig.params.get(i) else {
                break;
            };
            if ancestor_param.name != own_param.name {
                self.renamed_parameter_override_facts
                    .push(RenamedParameterOverride {
                        prototype_class: ancestor_owner.clone(),
                        method: syntax::intern::intern(method_name),
                        prototype_param: ancestor_param.name.clone(),
                        overriding_class: syntax::intern::intern(fqn),
                        overriding_param: own_param.name.clone(),
                    });
            }
        }
    }
}
