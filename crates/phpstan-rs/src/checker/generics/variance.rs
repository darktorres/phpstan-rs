use crate::checker::Checker;
use crate::registry::FunctionReflection;
use phpdoc::docblock;
use syntax::ast::*;
use syntax::span::Span;
use types::{self, PositionVariance, Type};

impl<'a> Checker<'a> {
    /// Variance-enforcement: composes variance through nested generics
    /// rather than a flat "found anywhere in an input/output type at
    /// all" check - a class's own `@template-covariant T` may only appear in
    /// one of its methods' *output* positions (covariance is only sound
    /// if nothing can ever hand a `T` back *into* the generic container,
    /// the way `Collection<Cat>` being usable where `Collection<Animal>`
    /// is expected relies on only ever reading a value out, never
    /// writing one in); `@template-contravariant T` is the mirror image,
    /// restricted to *input* positions only. A plain, un-annotated
    /// `@template T` (invariant) is unrestricted, same as always.
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
    /// (`PositionVariance::Invariant`, flagged regardless of which direction this
    /// class's own template varies) when it's invariant - unrelated
    /// generic containers don't let subtyping flow through their own
    /// invariant slot at all, so neither direction is sound there
    /// either. See `PositionVariance::compose`.
    ///
    /// Checks method params/return types only - a property's own single
    /// type needs different position logic entirely (`PositionVariance::Covariant` when
    /// write access is restricted, `PositionVariance::Invariant` otherwise, not a
    /// param/return split), see `check_property_variances` below.
    pub(crate) fn check_variance(
        &mut self,
        sig: &FunctionReflection,
        class_name: &str,
        span: Span,
    ) {
        let Some(info) = self.class_info(class_name) else {
            return;
        };
        if info.covariant_templates().is_empty() && info.contravariant_templates().is_empty() {
            return;
        }
        let mut violations: Vec<String> = Vec::new();
        for param in sig.params() {
            self.walk_variance(
                param.ty(),
                PositionVariance::Contravariant,
                class_name,
                &mut violations,
            );
        }
        self.walk_variance(
            sig.return_type(),
            PositionVariance::Covariant,
            class_name,
            &mut violations,
        );

        // Sorted-and-deduped before reporting: the same `T` can
        // legitimately be reached more than once (once per parameter, or
        // more than once within a single nested type). A plain `Vec`
        // built via one deterministic left-to-right walk avoids any
        // `HashMap`/`HashSet` iteration-order instability, so sorting
        // here is only about de-duplicating repeat visits, not fixing
        // ordering.
        violations.sort();
        violations.dedup();
        for message in violations {
            self.report(message, span, 6, "generics.variance");
        }
    }

    /// `VarianceCheck::checkParametersAcceptor`'s own
    /// declaration-validity half - a call-argument-binding function/method
    /// `@template` has no subclass relationship at all for covariance/
    /// contravariance to describe (that concept only makes sense for a
    /// class/interface's own type parameter, bound once at instantiation
    /// and held for the object's whole lifetime), so `@template-covariant`/
    /// `-contravariant` on a function or method's own tag is always an
    /// authoring mistake. Runs unconditionally, including for `__construct`
    /// and private methods (unlike `check_variance` above, whose own
    /// position-based half real PHPStan skips for both) - real
    /// PHPStan's own `$isPrivate` early return sits after this exact
    /// check in `checkParametersAcceptor`. Re-parses
    /// `f.doc_comment` fresh rather than reading back through `FunctionReflection`
    /// (which has no persisted raw template-variance list of its own, the
    /// same reason `current_function_template_params` re-derives from
    /// template nodes instead - see that function's own doc
    /// comment) - cheap, since a function/method with no `@template` tag
    /// at all (the overwhelming majority) parses to an empty `DocBlock`
    /// in one pass either way.
    pub(crate) fn check_own_template_variance_declaration(
        &mut self,
        f: &FunctionDecl,
        is_method: bool,
        label: &str,
        span: Span,
    ) {
        // Cheap substring pre-filter before the real docblock::parse_shared below
        // - this runs unconditionally on every checked function/method,
        // and the overwhelming majority declare no `@template-covariant`/
        // `-contravariant` tag at all. Matched without the leading `@`
        // so every spelling `docblock::parse_shared` treats identically
        // (`@phpstan-`/`@psalm-`-prefixed aliases included) passes.
        let has_variance_tag = f.doc_comment.as_deref().is_some_and(|d| {
            d.contains("template-covariant") || d.contains("template-contravariant")
        });
        if !has_variance_tag {
            return;
        }
        let Some(doc) = f.doc_comment.as_deref().map(docblock::parse_shared) else {
            return;
        };
        if doc.covariant_templates.is_empty() && doc.contravariant_templates.is_empty() {
            return;
        }
        let kind_word = if is_method { "method" } else { "function" };
        let identifier = if is_method {
            "method.variance"
        } else {
            "function.variance"
        };
        let mut names: Vec<&String> = doc
            .covariant_templates
            .iter()
            .chain(&doc.contravariant_templates)
            .collect();
        names.sort();
        for name in names {
            self.report(format!(
                    "Variance annotation is only allowed for type parameters of classes and interfaces, but occurs in template type {name} in {kind_word} {label}()."
                ),
                span,
                6,
                identifier,
            );
        }
    }

    /// `PropertyVarianceRule`, ported directly from
    /// `vendor/phpstan-src/src/Rules/Generics/PropertyVarianceRule.php` -
    /// a property has one type, not a param/return pair, so this is a new
    /// sibling of `check_variance` above rather than a caller of it,
    /// reusing `walk_variance` directly with property-specific position
    /// logic: `PositionVariance::Covariant` (real PHPStan's "covariant" - the property
    /// can only ever be read back out, not written into, so a covariant
    /// template there is sound) when write access is restricted -
    /// `readonly`, private-set (`write_visibility == Private`, only
    /// reachable here for a property whose *read* visibility isn't
    /// itself private - already filtered below, same as real PHPStan's
    /// own `isPrivate()` early return - so this can only mean a genuine
    /// asymmetric `private(set)`, not a plain fully-private property), or
    /// a property with hooks where none of them is `set` (get-only, so
    /// nothing can ever write through it either) - `PositionVariance::Invariant`
    /// otherwise. `readonly`-by-PHPDoc isn't checked (this project has no
    /// such tracking at all).
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
    pub(crate) fn check_property_variances(&mut self, fqn: &str, c: &ClassDef) {
        let Some(info) = self.class_info(fqn) else {
            return;
        };
        if info.covariant_templates().is_empty() && info.contravariant_templates().is_empty() {
            return;
        }
        for p in &c.properties {
            let Some(prop_sig) = self.registry.lookup_property(fqn, p.name.as_ref()) else {
                continue;
            };
            if prop_sig.visibility() == Visibility::Private {
                continue;
            }
            let has_restricted_write_access = prop_sig.is_readonly()
                || prop_sig.write_visibility() == Visibility::Private
                || p.hooks
                    .as_ref()
                    .is_some_and(|h| h.set.is_none() && h.get.is_some());
            let pos = if has_restricted_write_access {
                PositionVariance::Covariant
            } else {
                PositionVariance::Invariant
            };
            let mut violations: Vec<String> = Vec::new();
            self.walk_variance(prop_sig.ty(), pos, fqn, &mut violations);
            violations.sort();
            violations.dedup();
            for message in violations {
                self.report(message, p.span, 6, "generics.variance");
            }
        }
        // A promoted constructor property (readonly or
        // hooked or neither) must be variance-checked too -
        // real PHPStan's own `PropertyVarianceRule` runs over a
        // `ClassPropertyNode` built uniformly for plain and promoted
        // shapes, so this project's own variance walk should too.
        if let Some(ctor) = c.constructor() {
            for param in &ctor.function.params {
                if param.promotion.is_none() {
                    continue;
                }
                let Some(prop_sig) = self.registry.lookup_property(fqn, param.name.as_ref()) else {
                    continue;
                };
                if prop_sig.visibility() == Visibility::Private {
                    continue;
                }
                let has_restricted_write_access = prop_sig.is_readonly()
                    || prop_sig.write_visibility() == Visibility::Private
                    || param
                        .hooks
                        .as_deref()
                        .is_some_and(|h| h.set.is_none() && h.get.is_some());
                let pos = if has_restricted_write_access {
                    PositionVariance::Covariant
                } else {
                    PositionVariance::Invariant
                };
                let mut violations: Vec<String> = Vec::new();
                self.walk_variance(prop_sig.ty(), pos, fqn, &mut violations);
                violations.sort();
                violations.dedup();
                for message in violations {
                    self.report(message, param.span, 6, "generics.variance");
                }
            }
        }
    }

    /// A violation for each template of `class_name` that `ty`, at
    /// position `pos`, uses where its declared variance doesn't allow.
    fn walk_variance(
        &self,
        ty: &Type,
        pos: PositionVariance,
        class_name: &str,
        violations: &mut Vec<String>,
    ) {
        for (tp, position) in ty.referenced_templates(pos, self.registry) {
            if tp.scope.as_deref() != Some(class_name) || position.is_valid_for(tp.variance) {
                continue;
            }
            match tp.variance {
                types::Variance::Covariant => violations.push(format!(
                    "{class_name}'s @template-covariant {} cannot be used in an input (parameter) position",
                    tp.name
                )),
                types::Variance::Contravariant => violations.push(format!(
                    "{class_name}'s @template-contravariant {} cannot be used in an output (return) position",
                    tp.name
                )),
                types::Variance::Invariant => {}
            }
        }
    }
}
