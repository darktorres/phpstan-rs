use std::collections::HashSet;

use crate::ast::*;
use crate::checker::{Checker, TemplateBindings};
use crate::docblock;
use crate::registry::{AssertInfo, FunctionSig, ParamSig};
use crate::token::Span;
use crate::types::{self, Type};

impl<'a> Checker<'a> {
    /// Walks `ty` for every `Object(class, args)` naming a class with its
    /// own `@template T of Bound` (Phase 10.5's bound-enforcement half),
    /// flagging any argument that doesn't satisfy its bound - e.g.
    /// `Collection<Foo>` where `Collection`'s own `@template T of Shape`
    /// and `Foo` doesn't implement `Shape`. Recurses into every argument
    /// too (so `Collection<Box<Foo>>` still checks `Box`'s own bound), and
    /// into `Union`/`Intersection`/`Array`/`ArrayShape` members (a bound
    /// violation doesn't stop being real just because it's wrapped in a
    /// union or is one array's element type).
    ///
    /// Bindings are computed once, positionally, before any bound is
    /// checked - not just so multiple arguments against multiple bounds
    /// each see the right value, but because a bound can itself reference
    /// another of the *same* class's own templates (`@template A`
    /// `@template B of A`), which only resolves correctly once `A`'s own
    /// concrete argument is known.
    ///
    /// Called once per declared type (a function's params/return type, a
    /// class's own properties, its `@extends`/`@implements` tags) rather
    /// than at every call/access site - a generic type only needs
    /// flagging once, where it's *written*, the same way real PHPStan's
    /// own `GenericObjectTypeCheck` documents itself.
    pub(crate) fn check_generic_bounds(&mut self, ty: &Type, label: &str, span: Span) {
        match ty {
            Type::Object(class, args) => {
                if !args.is_empty()
                    && let Some(info) = self.registry.classes.get(class)
                    && !info.template_bounds.is_empty()
                {
                    // Phase 54: bound-checking cares about the real
                    // referenced type, not a call-site variance marker
                    // wrapping it (`Collection<covariant Foo>` still
                    // needs `Foo` itself checked against the bound,
                    // `is_assignable`'s own variance handling is a
                    // separate concern) - `strip_call_site_variance`
                    // unwraps for both the substitution bindings and the
                    // actual bound comparison; the *displayed* `arg`
                    // below stays the original, marker-and-all, so a
                    // real violation message still shows `covariant Foo`
                    // rather than silently dropping the annotation.
                    let bindings: TemplateBindings = info
                        .templates
                        .iter()
                        .cloned()
                        .zip(args.iter().map(|a| {
                            types::strip_call_site_variance(a)
                                .cloned()
                                .unwrap_or(Type::Mixed)
                        }))
                        .collect();
                    for (name, arg) in info.templates.iter().zip(args.iter()) {
                        let Some(bound) = info.template_bounds.get(name) else {
                            continue;
                        };
                        let bound = types::substitute_template(bound, &bindings);
                        let Some(checked_arg) = types::strip_call_site_variance(arg) else {
                            continue;
                        };
                        // Same "don't know, don't guess" leniency every
                        // other undefined-symbol-adjacent check in this
                        // project already applies to a class the registry
                        // has no real data for (an uninstalled dependency,
                        // or - the real case found verifying this against
                        // Piwigo's corpus - a PHAR-bundled one like
                        // `phpstan/phpstan` itself, which ships with no
                        // loose source files to parse at all): without it,
                        // every reference to one of its own real classes,
                        // e.g. `PHPStan\Type\ArrayType implements
                        // PHPStan\Type\Type`, reads as violating a bound
                        // this project simply has no way to verify either
                        // way, not one it's actually confirmed broken.
                        if *checked_arg != Type::Mixed
                            && self.is_type_fully_known(checked_arg)
                            && self.is_type_fully_known(&bound)
                            && !self.registry.is_assignable(checked_arg, &bound)
                        {
                            self.report(
                                label.to_string(),
                                format!("{class}'s @template {name} requires {bound}, got {arg}"),
                                span,
                                6,
                                "generics.templateBound",
                            );
                        }
                    }
                }
                for arg in args {
                    self.check_generic_bounds(arg, label, span);
                }
            }
            Type::Array(key, inner) => {
                self.check_generic_bounds(key, label, span);
                self.check_generic_bounds(inner, label, span);
            }
            Type::ClassString(inner) => self.check_generic_bounds(inner, label, span),
            Type::Union(members) | Type::Intersection(members) => {
                for m in members {
                    self.check_generic_bounds(m, label, span);
                }
            }
            Type::ArrayShape(fields) => {
                for (_, t, _) in fields {
                    self.check_generic_bounds(t, label, span);
                }
            }
            // A real, previously-missing case: `object{...}`'s own field
            // types were never recursed into for bound-checking either.
            Type::ObjectShape(fields) => {
                for (_, t, _) in fields {
                    self.check_generic_bounds(t, label, span);
                }
            }
            _ => {}
        }
    }

    /// `false` if `ty` mentions any class this project has no real data
    /// for at all - an uninstalled dependency, or a PHAR-bundled one
    /// (real `phpstan/phpstan` ships as a compiled PHAR with no loose
    /// source files, the same gap Phase 9's own undefined-symbol checks
    /// already document). `check_generic_bounds` uses this to skip a
    /// bound it has no way to actually verify, rather than guess wrong.
    pub(crate) fn is_type_fully_known(&self, ty: &Type) -> bool {
        match ty {
            // Phase 54: a call-site variance sentinel's own name is
            // never a real class to look up - defer to whatever it
            // wraps (`"$$bivariant"` wraps nothing at all, trivially
            // "known" the same way this function's own catch-all
            // treats anything it doesn't otherwise recognize).
            Type::Object(name, _)
                if matches!(
                    name.as_str(),
                    "$$covariant" | "$$contravariant" | "$$bivariant"
                ) =>
            {
                types::strip_call_site_variance(ty)
                    .is_none_or(|inner| self.is_type_fully_known(inner))
            }
            Type::Object(name, args) => {
                (name == "object" || self.registry.classes.contains_key(name))
                    && args.iter().all(|a| self.is_type_fully_known(a))
            }
            Type::Array(key, inner) => {
                self.is_type_fully_known(key) && self.is_type_fully_known(inner)
            }
            Type::ClassString(inner) => self.is_type_fully_known(inner),
            Type::Union(members) | Type::Intersection(members) => {
                members.iter().all(|m| self.is_type_fully_known(m))
            }
            Type::ArrayShape(fields) => fields.iter().all(|(_, t, _)| self.is_type_fully_known(t)),
            // A real, previously-missing case: `object{...}`'s own field
            // types were never checked either.
            Type::ObjectShape(fields) => fields.iter().all(|(_, t, _)| self.is_type_fully_known(t)),
            _ => true,
        }
    }

    /// `check_generic_bounds` for everything a class declares outside its
    /// own methods (which `check_function` already covers per-method):
    /// every property's type, plus its own `@extends`/`@implements`
    /// arguments - both are real places a generic bound violation can be
    /// written (`class Bad extends Collection<NotAShape> {}`) without
    /// ever showing up in a method signature at all.
    pub(crate) fn check_class_generic_bounds(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        // Each property gets its own real span (declaration order, not a
        // sorted HashMap walk - the AST's `Vec<PropertyDecl>` is already
        // deterministic, unlike the registry's `HashMap`-backed lookup).
        for p in &c.properties {
            if let Some(prop_sig) = info.properties.get(&p.name) {
                self.check_generic_bounds(&prop_sig.ty, fqn, p.span);
                // Phase 48: `ExistingClassesInPropertiesRule`'s own check -
                // reuses this exact loop (same resolved `PropertySig::ty`,
                // same real per-property span) rather than a second walk
                // over `c.properties` elsewhere.
                self.check_type_class_names_exist(
                    &prop_sig.ty,
                    &format!("property {fqn}::${}", p.name),
                    p.span,
                    fqn,
                );
            }
        }
        self.check_property_tag_types(fqn, c);
        self.check_ancestor_generic_bounds(fqn, c.span);
    }

    /// Phase 36's own "bright spot": `@property`/`@property-read`/
    /// `@property-write`'s *validation* half (real PHPStan's own
    /// `PropertyTagCheck`) - parsing and inference already shipped back
    /// in Phase 13 Finding D (`ClassInfo::from_decl` already synthesizes
    /// every tag into a real, resolved `PropertySig`, exactly the way a
    /// magic `__get`/`__set`-backed property needs); only the diagnostic
    /// layer checking the tag's own declared type was missing. Mirrors
    /// the identical `check_generic_bounds`/`check_type_class_names_exist`
    /// pair the loop just above already runs for a class's own *real*
    /// properties, applied to every `@property`-family tag on this
    /// class's own doc comment instead. Re-parses `c.doc_comment` fresh
    /// (the same pattern `check_enum_template_tag` already uses) rather
    /// than threading a `DocBlock` through from wherever the registry
    /// last parsed one, since the checker doesn't keep it around after
    /// indexing.
    ///
    /// Deliberately narrower than real PHPStan's own `PropertyTagCheck`
    /// in one way: looks the tag's *name* up in the already-resolved
    /// `info.properties` map (which only ever holds one winning entry per
    /// name - a real, physically-declared property of the same name
    /// always wins over a tag, `ClassInfo::from_decl`'s own `or_insert_
    /// with` above) rather than independently re-resolving each tag's own
    /// type from scratch. This means a `@property` tag whose name is
    /// shadowed by a real property of the same name checks the *real*
    /// property's type instead of the tag's own - a redundant/self-
    /// contradictory declaration style with no confirmed corpus incidence,
    /// not the common case this check exists for.
    fn check_property_tag_types(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(raw) = &c.doc_comment else {
            return;
        };
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let block = docblock::parse(raw);
        for tag in &block.properties {
            let Some(prop_sig) = info.properties.get(&tag.name) else {
                continue;
            };
            let tag_name = match (tag.readable, tag.writable) {
                (true, true) => "@property",
                (true, false) => "@property-read",
                (false, true) => "@property-write",
                (false, false) => "@property",
            };
            self.check_generic_bounds(&prop_sig.ty, fqn, c.span);
            self.check_type_class_names_exist(
                &prop_sig.ty,
                &format!("PHPDoc tag {tag_name} for property {fqn}::${}", tag.name),
                c.span,
                fqn,
            );
        }
    }

    /// Phase 66: the `extends`/`implements` half of
    /// `check_class_generic_bounds`, split out so an enum declaration -
    /// which can `implements` a generic interface but has no properties
    /// and can't `extends` anything - can share it too. Previously this
    /// whole check only ran for `StmtKind::ClassDecl`, silently skipping
    /// generic-bound violations written on an enum's own `implements`
    /// clause.
    pub(crate) fn check_ancestor_generic_bounds(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        // `extends_args`/`implements_args` store just the argument list,
        // not paired with the parent/interface's own name - re-pair each
        // one into a real `Object(class, args)` here, or
        // `check_generic_bounds` has no class to look bounds up against
        // at all (it would just see the *arguments* as if they were the
        // generic reference themselves). Neither the parent name nor an
        // interface name carries its own span in the AST, so both use the
        // declaration's own span - imprecise. but still real.
        let mut types_to_check: Vec<Type> = Vec::new();
        if let Some(parent) = info.extends.first() {
            types_to_check.push(Type::Object(parent.clone(), info.extends_args.clone()));
        }
        // `implements_args` is a `HashMap` with no guaranteed iteration
        // order - sorted by key so two diagnostics from it always come out
        // in the same relative order, not one that silently varies by
        // process (Rust's default hasher is randomly seeded per run).
        let mut iface_names: Vec<&String> = info.implements_args.keys().collect();
        iface_names.sort();
        for iface in iface_names {
            types_to_check.push(Type::Object(
                iface.clone(),
                info.implements_args[iface].clone(),
            ));
        }
        for ty in types_to_check {
            self.check_generic_bounds(&ty, fqn, span);
        }
    }

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

    /// Thin delegation to `Registry::class_template_bindings` (Phase 54
    /// hoisted the real logic there, alongside `is_assignable`, since
    /// neither ever needed anything but `self.registry`/`self.classes` -
    /// kept here too since real call sites (`checker/assignment.rs`,
    /// `checker/expr.rs`) already call it as `self.class_template_
    /// bindings(...)`). See `Registry::class_template_bindings`'s own doc
    /// comment for the full binding/ancestor-walk semantics.
    pub(crate) fn class_template_bindings(
        &self,
        class: &str,
        type_args: &[Type],
        target: &str,
    ) -> TemplateBindings {
        self.registry
            .class_template_bindings(class, type_args, target)
    }

    /// Applies `class_template_bindings`' substitution to every param and
    /// the return type of a method signature - the class-level counterpart
    /// to `infer_call_return_type`'s existing per-call-argument template
    /// binding (which only ever sees a method's *own* `@template`, not one
    /// declared on the enclosing class). `owner` is whichever ancestor of
    /// `class` actually declares `sig` (from `lookup_method_with_owner`/
    /// `lookup_property_with_owner`) - `class` itself when it's not
    /// inherited.
    ///
    /// Scoped to only the names `owner`'s own class docblock actually
    /// declares as templates. `types::substitute_template`'s own fallback
    /// for a name absent from `bindings` is `Type::Mixed`, correct for a
    /// genuinely class-level template referenced with no or fewer type
    /// arguments than it declares (e.g. a bare `Collection` with no `<T>`
    /// at all), but a real Piwigo-corpus round found this wiping out every
    /// method's own separate template too the moment it was called on a
    /// non-generic class (a method's own generic identity-style function,
    /// declared entirely on its own docblock, on a class with zero
    /// templates of its own). That class has zero class-level templates,
    /// so `bindings` was always empty, and this substitution runs first
    /// (per this function's own doc comment above), replacing the
    /// method's own bare template name with `Mixed` before
    /// `infer_call_return_type`'s later, correct per-call-argument pass
    /// ever got a chance to bind it for real. `class_scope` restricts
    /// substitution to names genuinely in `owner`'s own template list,
    /// leaving any other name (a method's own, unrelated to the class)
    /// completely untouched for that later pass to resolve.
    pub(crate) fn substitute_class_templates(
        &self,
        sig: &FunctionSig,
        class: &str,
        type_args: &[Type],
        owner: &str,
    ) -> FunctionSig {
        let bindings = self.class_template_bindings(class, type_args, owner);
        let class_scope: HashSet<String> = self
            .registry
            .classes
            .get(owner)
            .map(|info| info.templates.iter().cloned().collect())
            .unwrap_or_default();
        let subst = |ty: &Type| substitute_scoped(ty, &bindings, &class_scope);
        FunctionSig {
            params: sig
                .params
                .iter()
                .map(|p| ParamSig {
                    ty: subst(&p.ty),
                    ty_native: subst(&p.ty_native),
                    ..p.clone()
                })
                .collect(),
            return_type: subst(&sig.return_type),
            return_type_native: subst(&sig.return_type_native),
            return_has_own_doc: sig.return_has_own_doc,
            return_explicit_mixed_hint: sig.return_explicit_mixed_hint,
            no_discard: sig.no_discard,
            visibility: sig.visibility.clone(),
            is_internal: sig.is_internal,
            has_override_attr: sig.has_override_attr,
            deprecated: sig.deprecated.clone(),
            throws: sig.throws.as_ref().map(subst),
            assert_if_true: substitute_assert_infos(&sig.assert_if_true, &subst),
            assert_if_false: substitute_assert_infos(&sig.assert_if_false, &subst),
            assert: substitute_assert_infos(&sig.assert, &subst),
            body: sig.body.clone(),
            purity_dependencies: sig.purity_dependencies.clone(),
            is_transitively_pure: sig.is_transitively_pure,
            is_stub: sig.is_stub,
            is_magic: sig.is_magic,
        }
    }
}

/// Applies the same class-template substitution `substitute_class_
/// templates` computes for every other declared type on this same
/// signature to a `@phpstan-assert`-family list's own asserted type -
/// only `ty` needs it; `target`/`negated` name a real parameter/`$this`
/// chain, never a type, so they're carried through unchanged.
fn substitute_assert_infos(
    infos: &[AssertInfo],
    subst: &impl Fn(&Type) -> Type,
) -> Vec<AssertInfo> {
    infos
        .iter()
        .map(|info| AssertInfo {
            negated: info.negated,
            ty: subst(&info.ty),
            target: info.target.clone(),
        })
        .collect()
}

/// Same structural walk as `types::substitute_template`, except a
/// `TemplateParam` name outside `scope` is left completely untouched
/// instead of defaulting to `Mixed` - see `substitute_class_templates`'s
/// own doc comment for why that distinction matters.
fn substitute_scoped(ty: &Type, bindings: &TemplateBindings, scope: &HashSet<String>) -> Type {
    match ty {
        Type::TemplateParam(tp) => {
            if scope.contains(&tp.name) {
                // Phase 54: mirrors `types::substitute_template`'s own
                // `@template T = Default` fallback - an "erased" class
                // reference (fewer type arguments than declared,
                // including a bare one with none at all) leaves `bindings`
                // with no entry for this name at all, same as before; the
                // only change is falling back to this template's own
                // declared default (when it has one) instead of
                // unconditionally `Mixed`.
                bindings.get(&tp.name).cloned().unwrap_or_else(|| {
                    tp.default
                        .as_ref()
                        .map(|d| substitute_scoped(d, bindings, scope))
                        .unwrap_or(Type::Mixed)
                })
            } else {
                ty.clone()
            }
        }
        // Phase 50: substitute through the key type too, not reset
        // to "unknown" every time (matches types::substitute_template's
        // own Array-arm fix).
        Type::Array(key, inner) => Type::Array(
            Box::new(substitute_scoped(key, bindings, scope)),
            Box::new(substitute_scoped(inner, bindings, scope)),
        ),
        Type::ClassString(inner) => {
            Type::ClassString(Box::new(substitute_scoped(inner, bindings, scope)))
        }
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|m| substitute_scoped(m, bindings, scope))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .iter()
                .map(|m| substitute_scoped(m, bindings, scope))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .iter()
                .map(|(k, t, opt)| (k.clone(), substitute_scoped(t, bindings, scope), *opt))
                .collect(),
        ),
        // A real, previously-missing case, found via this session's own
        // corpus diff (`php-standard-library`'s own `ContainerType<Tk as
        // array-key, Tv>`, whose own `Tk`/`Tv` reach a caller's return
        // type nested inside `object{...}` by way of an *interface's*
        // own template): `object{...}`'s own field types were never
        // substituted here either, mirroring `types::substitute_
        // template`'s own identical fix.
        Type::ObjectShape(fields) => Type::ObjectShape(
            fields
                .iter()
                .map(|(k, t, opt)| (k.clone(), substitute_scoped(t, bindings, scope), *opt))
                .collect(),
        ),
        // Phase 54: mirrors `types::substitute_template`'s own identical,
        // previously-missing fix - `Object`'s own generic argument list
        // wasn't recursed into at all, so a class-level template used
        // inside e.g. `Box<T>` never actually got substituted.
        Type::Object(name, args) => Type::Object(
            name.clone(),
            args.iter()
                .map(|a| substitute_scoped(a, bindings, scope))
                .collect(),
        ),
        other => other.clone(),
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
