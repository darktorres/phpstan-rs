use crate::ast::*;
use crate::checker::Checker;
use crate::registry::FunctionSig;
use crate::token::Span;
use crate::types::{self, Type};

impl<'a> Checker<'a> {
    /// Phase 10.5's variance-enforcement half, rewritten in Phase 54 to
    /// really compose variance through nested generics instead of the
    /// original flat "found anywhere in an input/output type at all"
    /// check: a class's own `@template-covariant T` may only appear in
    /// one of its methods' *output* positions (covariance is only sound
    /// if nothing can ever hand a `T` back *into* the generic container,
    /// the way `Collection<Cat>` being usable where `Collection<Animal>`
    /// is expected relies on only ever reading a value out, never
    /// writing one in); `@template-contravariant T` is the mirror image,
    /// restricted to *input* positions only. A plain, un-annotated
    /// `@template T` (invariant, the only kind Phase 10 modeled at all)
    /// is unrestricted, same as always.
    ///
    /// "Position" is no longer just "which half of the signature" - a
    /// nested generic argument slot composes with (and can flip) the
    /// position it's found in, based on *that* class's own declared
    /// variance for its own template at that position: a `T` found as
    /// `Collection<T>`'s own argument stays in the same position
    /// `Collection<T>` itself was found in when `Collection`'s own
    /// template is covariant (reading a `T` back out of a covariant
    /// container is exactly as safe as reading the container itself),
    /// flips when it's contravariant, and becomes doubly-restricted
    /// (`Position::Inv`, flagged regardless of which direction this
    /// class's own template varies) when it's invariant - unrelated
    /// generic containers don't let subtyping flow through their own
    /// invariant slot at all, so neither direction is sound there
    /// either. See `walk_variance`/`Position::compose` below.
    ///
    /// Checks method params/return types only - a property's own single
    /// type needs different position logic entirely (`Position::Out` when
    /// write access is restricted, `Position::Inv` otherwise, not a
    /// param/return split), see `check_property_variances` below (Phase
    /// 43). This doc comment used to say properties weren't checked
    /// because "this project doesn't track `readonly` at all yet" - stale
    /// since Phase 57 added `PropertySig::is_readonly`/`write_visibility`;
    /// corrected as part of building that check rather than left
    /// contradicted by the current code.
    pub(crate) fn check_variance(
        &mut self,
        sig: &FunctionSig,
        class_name: &str,
        label: &str,
        span: Span,
    ) {
        let Some(info) = self.registry.classes.get(class_name) else {
            return;
        };
        if info.covariant_templates.is_empty() && info.contravariant_templates.is_empty() {
            return;
        }
        let mut violations: Vec<String> = Vec::new();
        for param in &sig.params {
            self.walk_variance(&param.ty, Position::In, class_name, &mut violations);
        }
        self.walk_variance(&sig.return_type, Position::Out, class_name, &mut violations);

        // Sorted-and-deduped before reporting: the same `T` can now
        // legitimately be reached more than once (once per parameter, or
        // more than once within a single nested type), and this project
        // has already been bitten once this phase by a `HashMap`/
        // `HashSet` iteration order silently varying a diagnostic's
        // position between runs (see `check_class_generic_bounds`'s own
        // fix) - a plain `Vec` built via one deterministic left-to-right
        // walk sidesteps that class of bug entirely, so sorting here is
        // only about de-duplicating repeat visits, not fixing ordering.
        violations.sort();
        violations.dedup();
        for message in violations {
            self.report(label.to_string(), message, span, 6, "generics.variance");
        }
    }

    /// Phase 43: `PropertyVarianceRule`, ported directly from
    /// `vendor/phpstan-src/src/Rules/Generics/PropertyVarianceRule.php` -
    /// a property has one type, not a param/return pair, so this is a new
    /// sibling of `check_variance` above rather than a caller of it,
    /// reusing `walk_variance` directly with property-specific position
    /// logic: `Position::Out` (real PHPStan's "covariant" - the property
    /// can only ever be read back out, not written into, so a covariant
    /// template there is sound) when write access is restricted -
    /// `readonly`, private-set (`write_visibility == Private`, only
    /// reachable here for a property whose *read* visibility isn't
    /// itself private - already filtered below, same as real PHPStan's
    /// own `isPrivate()` early return - so this can only mean a genuine
    /// asymmetric `private(set)`, not a plain fully-private property), or
    /// a property with hooks where none of them is `set` (get-only, so
    /// nothing can ever write through it either) - `Position::Inv`
    /// otherwise. `readonly`-by-PHPDoc isn't checked (this project has no
    /// such tracking at all - see ROADMAP.md's Phase 36 entry, a
    /// pre-existing, disclosed scope boundary this doesn't newly narrow).
    ///
    /// One real, disclosed wording imprecision: `walk_variance`'s own
    /// violation messages are worded for the method-parameter case
    /// ("cannot be used in an input (parameter) position") since they're
    /// shared verbatim with `check_variance` above - a property violation
    /// reuses the identical wording rather than real PHPStan's own
    /// property-specific phrasing ("...but occurs in ... position in
    /// property Foo::$bar"). Still fires on exactly the right condition,
    /// same identifier/level - a text nuance, not a correctness gap;
    /// splitting `walk_variance`'s hardcoded strings by call site wasn't
    /// judged worth it for how rarely a property is both generic-
    /// template-typed and hits this specific violation.
    pub(crate) fn check_property_variances(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        if info.covariant_templates.is_empty() && info.contravariant_templates.is_empty() {
            return;
        }
        for p in &c.properties {
            let Some(prop_sig) = info.properties.get(&p.name) else {
                continue;
            };
            if prop_sig.visibility == Visibility::Private {
                continue;
            }
            let has_restricted_write_access = prop_sig.is_readonly
                || prop_sig.write_visibility == Visibility::Private
                || p.hooks
                    .as_ref()
                    .is_some_and(|h| h.set.is_none() && h.get.is_some());
            let pos = if has_restricted_write_access {
                Position::Out
            } else {
                Position::Inv
            };
            let mut violations: Vec<String> = Vec::new();
            self.walk_variance(&prop_sig.ty.clone(), pos, fqn, &mut violations);
            violations.sort();
            violations.dedup();
            for message in violations {
                self.report(fqn.to_string(), message, p.span, 6, "generics.variance");
            }
        }
    }

    /// Walks `ty` composing `pos` through every nested generic argument
    /// slot (see `check_variance`'s own doc comment for the composition
    /// rules), flagging a `TemplateParam` belonging to `class_name` itself
    /// (`TemplateParam::scope` - not some unrelated method's own template,
    /// or an ancestor's, reached through a chain this walk doesn't itself
    /// resolve) whose declared variance disagrees with the position it
    /// was found in. `Array`/`ClassString`/`Union`/`Intersection`/
    /// `ArrayShape` are structural wrappers that don't themselves carry a
    /// declared variance to compose with, so they pass `pos` through
    /// unchanged - only an `Object`'s own type-argument slots can flip or
    /// force it, via `Registry::effective_slot_variance`.
    ///
    /// Phase 54: uses `effective_slot_variance`, not the class's bare
    /// declared variance alone - a call-site variance marker written on
    /// this exact argument (`Collection<covariant T>`) overrides
    /// `Collection`'s own declared slot variance for this composition,
    /// exactly mirroring real PHPStan's `VarianceCheck`/
    /// `getReferencedTemplateTypes`, which consult the identical
    /// call-site-overrides-declared rule this project's own
    /// `is_assignable` now also uses (`registry.rs`) - both are real
    /// consumers of the same effective-variance concept, not just one.
    fn walk_variance(
        &self,
        ty: &Type,
        pos: Position,
        class_name: &str,
        violations: &mut Vec<String>,
    ) {
        match ty {
            Type::TemplateParam(tp) => {
                if tp.scope.as_deref() != Some(class_name) {
                    return;
                }
                match tp.variance {
                    types::Variance::Covariant if pos != Position::Out => {
                        violations.push(format!(
                            "{class_name}'s @template-covariant {} cannot be used in an input (parameter) position",
                            tp.name
                        ));
                    }
                    types::Variance::Contravariant if pos != Position::In => {
                        violations.push(format!(
                            "{class_name}'s @template-contravariant {} cannot be used in an output (return) position",
                            tp.name
                        ));
                    }
                    _ => {}
                }
            }
            // Phase 54: a call-site variance sentinel isn't a real nested
            // class with its own slot to look up - the *enclosing* call
            // (the arm just below, one level up) already read the
            // override and composed `pos` accordingly before ever
            // recursing into this node. Without this dedicated arm, the
            // generic `Object(class, args)` case below would instead try
            // `effective_slot_variance("$$covariant", 0, ...)`, which
            // always returns `None` (`"$$covariant"` is never a
            // registered class) - silently stopping the walk right here,
            // so a real violation nested inside the wrapped type would
            // never be found, for the wrong reason. Confirmed the hard
            // way via this project's own revert-verification: a
            // deliberately broken wiring test still passed, because this
            // exact silent-stop already produced the same "no violation
            // found" result independent of whether the override was
            // ever consulted at all.
            Type::Object(class, _)
                if matches!(
                    class.as_str(),
                    "$$covariant" | "$$contravariant" | "$$bivariant"
                ) =>
            {
                if let Some(inner) = types::strip_call_site_variance(ty) {
                    self.walk_variance(inner, pos, class_name, violations);
                }
            }
            Type::Object(class, args) => {
                for (i, arg) in args.iter().enumerate() {
                    if let Some(slot) = self.registry.effective_slot_variance(class, i, arg) {
                        self.walk_variance(arg, pos.compose(slot), class_name, violations);
                    }
                }
            }
            Type::Array(key, inner) => {
                self.walk_variance(key, pos, class_name, violations);
                self.walk_variance(inner, pos, class_name, violations);
            }
            Type::ClassString(inner) => self.walk_variance(inner, pos, class_name, violations),
            Type::Union(members) | Type::Intersection(members) => {
                for m in members {
                    self.walk_variance(m, pos, class_name, violations);
                }
            }
            Type::ArrayShape(fields) => {
                for (_, t, _) in fields {
                    self.walk_variance(t, pos, class_name, violations);
                }
            }
            // A real, previously-missing case: `object{...}`'s own field
            // types were never walked for declaration-soundness either -
            // a covariant/contravariant template nested inside one
            // (`object{value: T}`) went unchecked.
            Type::ObjectShape(fields) => {
                for (_, t, _) in fields {
                    self.walk_variance(t, pos, class_name, violations);
                }
            }
            _ => {}
        }
    }
}

/// Phase 54: a position within a type expression, for `Checker::
/// walk_variance`'s own real per-position variance composition -
/// `Position::In` for a parameter type (real PHPStan's "negative"/input
/// polarity), `Position::Out` for a return type ("positive"/output
/// polarity), and `Position::Inv` once composition has passed through any
/// nested generic argument slot this project knows to be invariant
/// (`slot_variance` returning `Invariant`, or simply unresolved to any of
/// the three - see `Position::compose`) - an invariant slot doesn't let
/// subtyping flow through it in *either* direction, so from here on a
/// covariant and a contravariant template are equally unsound to find,
/// and `walk_variance`'s own `pos != Out`/`pos != In` checks flag both
/// without needing a third, explicit case per variance kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Position {
    In,
    Out,
    Inv,
}

impl Position {
    /// Composes this position with a nested generic argument slot's own
    /// declared variance, the same left-to-right composition real
    /// PHPStan's own `TemplateTypeVariance::compose()` performs: a
    /// covariant slot leaves the position unchanged (reading a value back
    /// out of a covariant container is exactly as safe as reading the
    /// container itself), a contravariant slot flips it, and an
    /// invariant slot forces `Inv` outright, absorbing whatever this
    /// position already was - once `self` is already `Inv`, every
    /// further composition (through any slot variance) stays `Inv`, the
    /// same "no further flipping/relaxing can undo becoming invariant"
    /// invariant `compose`'s own match arms preserve without needing a
    /// separate `self == Inv` guard: `Covariant` returns `self` (already
    /// `Inv`) as-is, and `Contravariant`'s `flip()` maps `Inv` to `Inv`.
    fn compose(self, slot: types::Variance) -> Position {
        match slot {
            types::Variance::Invariant => Position::Inv,
            types::Variance::Covariant => self,
            types::Variance::Contravariant => self.flip(),
        }
    }

    fn flip(self) -> Position {
        match self {
            Position::In => Position::Out,
            Position::Out => Position::In,
            Position::Inv => Position::Inv,
        }
    }
}
