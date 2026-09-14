use std::collections::HashMap;

use crate::ast::*;
use crate::checker::{
    Checker, ITERABLE_GENERIC_CLASS_NAMES, TemplateBindings, class_kind_identifier,
};
use crate::docblock;
use crate::token::Span;
use crate::types::{self, Type};

impl<'a> Checker<'a> {
    /// Walks `ty` for every `Object(class, args)` naming a class with
    /// its own `@template T of Bound`, flagging any argument that
    /// doesn't satisfy its bound - e.g. `Collection<Foo>` where
    /// `Collection`'s own `@template T of Shape` and `Foo` doesn't
    /// implement `Shape`. Recurses into every argument (so
    /// `Collection<Box<Foo>>` still checks `Box`'s bound) and into
    /// `Union`/`Intersection`/`Array`/`ArrayShape` members.
    ///
    /// Bindings are computed once, positionally, before any bound is
    /// checked, since a bound can itself reference another of the
    /// *same* class's templates (`@template A` `@template B of A`),
    /// which only resolves correctly once `A`'s concrete argument is
    /// known.
    ///
    /// Called once per declared type (a function's params/return type, a
    /// class's properties, its `@extends`/`@implements` tags) rather
    /// than at every call/access site - a generic type only needs
    /// flagging once, where it's *written*.
    pub(crate) fn check_generic_bounds(&mut self, ty: &Type, label: &str, span: Span) {
        self.check_generic_bounds_and_arity(ty, label, span, true);
    }

    /// `check_arity` gates both `check_generic_reference_arity` and
    /// `check_call_site_variance` - `false` only for
    /// `check_ancestor_generic_bounds`'s known ill-formed
    /// multi-`extends`-conflated parent type (see `arity_check_types`),
    /// whose misattributed args list is exactly as untrustworthy for
    /// "does argument `i`'s call-site variance match slot `i`'s
    /// declared one" as it is for a plain count comparison. Every other
    /// caller always keeps both pervasive checks, matching real
    /// PHPStan's `GenericObjectTypeCheck` running both at *every*
    /// declared generic-type position, not just a class's
    /// `extends`/`implements`.
    fn check_generic_bounds_and_arity(
        &mut self,
        ty: &Type,
        label: &str,
        span: Span,
        check_arity: bool,
    ) {
        if check_arity {
            self.check_generic_reference_arity(ty, label, span);
        }
        if check_arity && let Type::Object(class, args) = ty {
            self.check_call_site_variance(class, args, label, span);
        }
        match ty {
            Type::Object(class, args) => {
                if !args.is_empty()
                    && let Some(info) = self.registry.classes.get(class)
                    && !info.template_bounds.is_empty()
                {
                    // Bound-checking cares about the real referenced
                    // type, not a call-site variance marker wrapping it
                    // (`Collection<covariant Foo>` still needs `Foo`
                    // itself checked against the bound -
                    // `is_assignable`'s own variance handling is
                    // separate) - `strip_call_site_variance` unwraps for
                    // both the substitution bindings and the bound
                    // comparison; the *displayed* `arg` below stays the
                    // original, marker-and-all, so a violation message
                    // still shows `covariant Foo`.
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
                        // Same "don't know, don't guess" leniency this
                        // project applies to a class the registry has no
                        // real data for (an uninstalled dependency, or a
                        // PHAR-bundled one like `phpstan/phpstan` itself,
                        // which ships with no loose source files to
                        // parse) - without it, every reference to one of
                        // its own real classes, e.g.
                        // `PHPStan\Type\ArrayType implements
                        // PHPStan\Type\Type`, would read as violating a
                        // bound this project has no way to verify either
                        // way.
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
            // `object{...}`'s own field types also need recursing into
            // for bound-checking.
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
    /// source files, the same gap undefined-symbol checks
    /// already document). `check_generic_bounds` uses this to skip a
    /// bound it has no way to actually verify, rather than guess wrong.
    pub(crate) fn is_type_fully_known(&self, ty: &Type) -> bool {
        match ty {
            // A call-site variance sentinel's own name is
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
            // `object{...}`'s own field types need checking too.
            Type::ObjectShape(fields) => fields.iter().all(|(_, t, _)| self.is_type_fully_known(t)),
            _ => true,
        }
    }

    /// `check_generic_bounds` for everything a class declares outside
    /// its own methods (which `check_function` already covers
    /// per-method): every property's type, plus its
    /// `@extends`/`@implements` arguments - both are real places a
    /// generic bound violation can be written
    /// (`class Bad extends Collection<NotAShape> {}`) without ever
    /// showing up in a method signature.
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
                // `ExistingClassesInPropertiesRule`'s own check -
                // reuses this exact loop (same resolved `PropertySig::ty`,
                // same real per-property span) rather than a second walk
                // over `c.properties` elsewhere.
                self.check_type_class_names_exist(
                    &prop_sig.ty,
                    &format!("property {fqn}::${}", p.name),
                    p.span,
                    fqn,
                );
                // `property.trait` - see `check_type_trait_refs`'s
                // own doc comment.
                self.check_type_trait_refs(
                    &prop_sig.ty,
                    &format!("property {fqn}::${}", p.name),
                    p.span,
                    fqn,
                    "property.trait",
                );
                // `class.nameCase` for this PHPDoc-typed property position.
                self.check_type_class_name_case(&prop_sig.case_refs, fqn, p.span);
                self.check_type_internal_usage(&prop_sig.case_refs, fqn, p.span);
            }
        }
        self.check_property_tag_types(fqn, c);
        self.check_ancestor_generic_bounds(fqn, c.span);
        // `TemplateTypeCheck`'s own declaration-validity family,
        // class-level (real PHPStan's per-kind rule split -
        // `ClassTemplateTypeRule`/`InterfaceTemplateTypeRule`/
        // `TraitTemplateTypeRule` all funnel into the identical shared
        // check); an enum's own separate `check_enum_template_tag`/
        // `enum.generic` check is `EnumTemplateTypeRule`'s real
        // counterpart, since real PHPStan excludes enums from
        // `ClassTemplateTypeRule` entirely.
        if !info.templates.is_empty() {
            let display_name = format!("{} {fqn}", class_kind_identifier(&c.kind));
            let templates = info.templates.clone();
            let bounds = info.template_bounds.clone();
            let defaults = info.template_defaults.clone();
            let type_aliases = info.type_aliases.clone();
            self.check_template_declaration_validity(
                &templates,
                &bounds,
                &defaults,
                &type_aliases,
                &display_name,
                fqn,
                c.span,
            );
        }
    }

    /// `@property`/`@property-read`/`@property-write`'s *validation*
    /// half (real PHPStan's own `PropertyTagCheck`) - parsing and
    /// inference already happen in `ClassInfo::from_decl` (which
    /// synthesizes every tag into a real, resolved `PropertySig`); this
    /// adds the diagnostic layer checking the tag's declared type.
    /// Mirrors the `check_generic_bounds`/`check_type_class_names_exist`
    /// pair the loop above runs for a class's *real* properties, applied
    /// to every `@property`-family tag instead. Re-parses
    /// `c.doc_comment` fresh (same pattern `check_enum_template_tag`
    /// uses), since the checker doesn't keep a `DocBlock` around after
    /// indexing.
    ///
    /// Deliberately narrower than real PHPStan's `PropertyTagCheck` in
    /// one way: looks the tag's *name* up in the already-resolved
    /// `info.properties` map (which only holds one winning entry per
    /// name - a real, physically-declared property always wins over a
    /// tag) rather than independently re-resolving each tag's type from
    /// scratch, so a `@property` tag shadowed by a real property of the
    /// same name checks the *real* property's type instead of the tag's
    /// own.
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
            // `class.nameCase` for this `@property`-tag target.
            self.check_type_class_name_case(&prop_sig.case_refs, fqn, c.span);
            self.check_type_internal_usage(&prop_sig.case_refs, fqn, c.span);
        }
    }

    /// The `extends`/`implements` half of
    /// `check_class_generic_bounds`, split out so an enum declaration -
    /// which can `implements` a generic interface but has no properties
    /// and can't `extends` anything - can share it too. Previously this
    /// whole check only ran for `StmtKind::ClassDecl`, silently skipping
    /// generic-bound violations written on an enum's own `implements`
    /// clause.
    ///
    /// Also covers `@use Trait<Arg, ...>` (a `use`
    /// clause's own generic arguments) - real PHPStan's own
    /// `UsedTraitsRule` is a separate rule (triggered per `TraitUse`
    /// statement node, with its own trait-name-mismatch diagnostics this
    /// project doesn't replicate), but its actual bound/arity/call-site-
    /// variance checking is the identical `GenericObjectTypeCheck`/
    /// `GenericAncestorsCheck` machinery this function already runs for
    /// `extends`/`implements` - folding it in here, the same way this function
    /// already unifies `extends`+`implements` for enums, avoids a third
    /// near-duplicate call site for what's ultimately the same check.
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
        // Each entry pairs a type to check with whether it's also safe to
        // arity-check - see the `extends` case just below for the one
        // case where it isn't.
        let mut types_to_check: Vec<(Type, bool)> = Vec::new();
        // Real corpus false positive (Symfony's own `ServiceCollectionInterface`,
        // which `extends ServiceProviderInterface, \Countable,
        // \IteratorAggregate` with *two* of those three carrying their own
        // `@extends ...<Args>` tag): `extends_args` is a flat, un-keyed
        // `Vec<Type>` (unlike `implements_args`, already a real `HashMap`)
        // - real PHP interfaces can `extends` more than one parent, each
        // with its own independent argument list, but this field has no
        // way to keep more than one ancestor's own arguments apart, so
        // multiple `@extends<...>` tags parse into one conflated list.
        // Harmless for `check_generic_bounds`'s own tolerant, `zip`-based
        // bound-checking (an extra/misattributed argument is simply never
        // looked at), but `check_generic_reference_arity`'s own
        // exact-count comparison is far more sensitive to it - only
        // trustworthy when there's exactly one `extends` target to begin
        // with, the one case `extends_args` is actually guaranteed to
        // belong to. `check_generic_bounds_and_arity`'s own `check_arity`
        // flag is how this one call site opts out of the arity check its
        // own pervasive default would otherwise now also run.
        if let Some(parent) = info.extends.first() {
            let parent_ty = Type::Object(parent.clone(), info.extends_args.clone());
            types_to_check.push((parent_ty, info.extends.len() == 1));
        }
        // `implements_args` is a `HashMap` with no guaranteed iteration
        // order - sorted by key so two diagnostics from it always come out
        // in the same relative order, not one that silently varies by
        // process (Rust's default hasher is randomly seeded per run).
        let mut iface_names: Vec<&String> = info.implements_args.keys().collect();
        iface_names.sort();
        for iface in iface_names {
            let iface_ty = Type::Object(iface.clone(), info.implements_args[iface].clone());
            types_to_check.push((iface_ty, true));
        }
        // `@use Trait<Arg, ...>` - a `HashMap` just
        // like `implements_args`, each trait keyed independently (no
        // `extends_args`-style conflation risk: a class can `use` more
        // than one generic trait, but PHP requires one `@use` tag per
        // trait, each naming its own trait explicitly), so every entry is
        // as safe to arity-check as an interface is.
        let mut trait_names: Vec<&String> = info.uses_args.keys().collect();
        trait_names.sort();
        for trait_name in trait_names {
            let trait_ty = Type::Object(trait_name.clone(), info.uses_args[trait_name].clone());
            types_to_check.push((trait_ty, true));
        }
        for (ty, check_arity) in &types_to_check {
            self.check_generic_bounds_and_arity(ty, fqn, span, *check_arity);
        }
        // `generics.interfaceConflict` - see
        // `checker::generics::interfaces`'s own module doc comment.
        self.check_cross_path_interface_conflicts(fqn, span);
    }

    /// `GenericObjectTypeCheck`'s own arity/well-
    /// formedness half (`generics.lessTypes`/`.moreTypes`, confirmed
    /// directly against `vendor/phpstan-src`) - a genuinely different
    /// rule than `check_generic_bounds`'s own bound-checking, which
    /// silently truncates via `zip` (an extra argument beyond `info.
    /// templates.len()` is simply never looked at) rather than flagging
    /// the mismatch itself. Called pervasively from
    /// `check_generic_bounds_and_arity` - every generic type reference
    /// anywhere (a property type, a param/return hint, an `extends`/
    /// `implements` argument, ...) - matching real PHPStan's own
    /// `IncompatiblePhpDocTypeCheck`, the shared engine behind every one
    /// of those positions, which calls `GenericObjectTypeCheck` the same
    /// way. Every template with its own `@template T = Default`
    /// (`template_defaults`) is optional - only a template with *no*
    /// default is "required" for `lessTypes` purposes. Real PHPStan's own
    /// third case,
    /// `generics.notGeneric` (a reference supplying args to a class
    /// with *no* templates at all), is deliberately not built here: this
    /// project can't always tell "genuinely not generic" apart from
    /// "this project's own template-tag parsing has a gap for this
    /// class," and misfiring on the latter across *every* `extends`/
    /// `implements` in the corpus is a real, broad false-positive risk
    /// the narrower arity-only checks below don't share (`0 < args.len()`
    /// on a class with zero known templates would already be caught as
    /// `moreTypes` instead, at the same, more conservative confidence
    /// level as any other class here).
    fn check_generic_reference_arity(&mut self, ty: &Type, label: &str, span: Span) {
        let Type::Object(class, args) = ty else {
            return;
        };
        if args.is_empty() {
            return;
        }
        let Some(info) = self.registry.classes.get(class) else {
            return;
        };
        // Real corpus false positive (Piwigo's own custom `phpstan-stubs/
        // LatteCachingIterator.stub.php`, its own extensive comment
        // explains why: Latte's `CachingIterator` retroactively "restores"
        // generics onto real PHP's own `\CachingIterator`, whose actual
        // phpstorm-stubs declaration carries zero `@template` tags at
        // all, via `@extends \CachingIterator<TKey, TValue, Iterator<TKey,
        // TValue>>` alone): a referenced class with *no* declared
        // templates at all is real PHPStan's own `generics.notGeneric`
        // territory - a different check, already deliberately not built
        // (see this file's own `check_generic_reference_arity` doc
        // comment) precisely because this project's own `@template`-tag
        // parsing can have a real gap for a class whose only template
        // declaration lives on the *referencing* side, not this one -
        // "genuinely non-generic" and "this project just doesn't see its
        // templates" are indistinguishable from here, so this stays
        // silent rather than guess.
        if info.templates.is_empty() {
            return;
        }
        // Real, hardcoded parse-time special case (`PhpDoc\TypeNodeResolver
        // ::resolveGenericTypeNode`, confirmed directly against
        // `vendor/phpstan-src`) for four built-in names specifically:
        // `Traversable`/`Iterator`/`IteratorAggregate<V>` pads a single
        // given argument into `<mixed, V>` before this check ever runs,
        // and `Generator<V>`/`Generator<K, V>` pads to the full `<K, V,
        // mixed, mixed>` shape - neither ever reaches `lessTypes`, even
        // though this project's own `template_defaults` has (correctly)
        // recorded no real `@template ... = Default` for any of their
        // four template names. This project doesn't model that
        // resolution-time padding itself (it would need touching every
        // consumer of a short-form reference to these names, not just
        // this one check), so this narrowly special-cases the exact same
        // argument counts real PHPStan's own hardcoded list treats as
        // pre-padded, rather than risk a real, common false positive on
        // `@return \Generator<int, string>` (verified directly: a real,
        // installed `phpstan/phpstan` flags neither this nor a bare
        // `Traversable<V>`, but does still flag `Generator<int, string,
        // bool>` - three args isn't one of its own padded shapes either).
        let is_pre_padded_short_form = (args.len() == 1
            && ITERABLE_GENERIC_CLASS_NAMES.contains(&class.as_str()))
            || (args.len() == 2 && class == "Generator");
        if is_pre_padded_short_form {
            return;
        }
        let required = info
            .templates
            .iter()
            .filter(|name| !info.template_defaults.contains_key(*name))
            .count();
        if required > args.len() {
            self.report(
                label.to_string(),
                format!(
                    "Generic type {ty} requires at least {required} type argument(s), {} given.",
                    args.len()
                ),
                span,
                6,
                "generics.lessTypes",
            );
        } else if info.templates.len() < args.len() {
            self.report(
                label.to_string(),
                format!(
                    "Generic type {ty} accepts at most {} type argument(s), {} given.",
                    info.templates.len(),
                    args.len()
                ),
                span,
                6,
                "generics.moreTypes",
            );
        }
    }

    /// `GenericObjectTypeCheck`'s own call-site-
    /// variance half (`generics.callSiteVarianceConflict`/
    /// `.callSiteVarianceRedundant`, confirmed directly against
    /// `vendor/phpstan-src`) - a call-site variance marker
    /// (`Collection<covariant Foo>`) is redundant when it names the exact
    /// same variance `Collection`'s own `@template` already declares for
    /// that slot (nothing is being overridden), and conflicting when it
    /// names the *opposite* real variance (`contravariant` written on a
    /// slot the class itself declared `@template-covariant`, or vice
    /// versa) - real PHPStan's own `TemplateTypeVariance::validPosition`,
    /// ported directly: a call-site annotation is only ever unsound
    /// against a slot the class declares as the *other* non-invariant
    /// direction. Writing `covariant`/`contravariant` on a slot the class
    /// declares plain invariant is exactly call-site variance's own
    /// intended escape hatch (never flagged, either way), and `*`
    /// (bivariant) is never flagged at all (real PHPStan's own
    /// `validPosition` always accepts it, `this.bivariant()`).
    ///
    /// Skips real PHPStan's own narrow `ReflectionClass<covariant T>`
    /// carve-out (a compat allowance for PHP 8.3 vs. 8.4's differing core
    /// stub) - confirmed by direct read that this project's own vendored
    /// `phpstorm-stubs/Reflection/ReflectionClass.php` declares its `T`
    /// with a plain `@template T of object`, not `@template-covariant`,
    /// so the carve-out's own gating condition (declared variance must
    /// already be covariant for the redundant case to fire at all) can
    /// never be reached against this project's actual data - dead code
    /// against a scenario that can't happen, not a real gap.
    fn check_call_site_variance(&mut self, class: &str, args: &[Type], label: &str, span: Span) {
        let Some(info) = self.registry.classes.get(class) else {
            return;
        };
        let ty = Type::Object(class.to_string(), args.to_vec());
        for (i, arg) in args.iter().enumerate() {
            let call_site = match arg {
                Type::Object(name, _) if name == "$$covariant" => types::Variance::Covariant,
                Type::Object(name, _) if name == "$$contravariant" => {
                    types::Variance::Contravariant
                }
                // `*` (bivariant) is never flagged - see this function's
                // own doc comment.
                _ => continue,
            };
            let Some(declared) = self.registry.slot_variance(class, i) else {
                continue;
            };
            let Some(template_name) = info.templates.get(i) else {
                continue;
            };
            if declared == call_site {
                self.report(
                    label.to_string(),
                    format!(
                        "Call-site variance of {arg} in generic type {ty} is redundant, template type {template_name} of class {class} has the same variance."
                    ),
                    span,
                    6,
                    "generics.callSiteVarianceRedundant",
                );
            } else if declared == types::Variance::Covariant
                || declared == types::Variance::Contravariant
            {
                let declared_word = if declared == types::Variance::Covariant {
                    "covariant"
                } else {
                    "contravariant"
                };
                self.report(
                    label.to_string(),
                    format!(
                        "Call-site variance of {arg} in generic type {ty} is in conflict with {declared_word} template type {template_name} of class {class}."
                    ),
                    span,
                    6,
                    "generics.callSiteVarianceConflict",
                );
            }
        }
    }

    /// `TemplateTypeCheck`'s own declaration-validity family
    /// (`vendor/phpstan-src`) - validates a
    /// `@template` tag's own declaration, not a usage site. Shared
    /// between a class/interface/trait's own templates (`templates`/
    /// `bounds`/`defaults` read straight off its `ClassInfo`) and a
    /// method's own (re-parsed fresh from its own doc comment, since
    /// `FunctionSig` has no persisted raw template-bound/-default map of
    /// its own - see `Checker::current_function_template_params`'s own
    /// doc comment for the identical reasoning) - `type_aliases` is
    /// always the *enclosing class's* own map either way (a type alias is
    /// class-scoped, real PHPStan's own `hasTypeAlias(name, className)`
    /// call takes the class regardless of whether the template itself is
    /// class- or method-level).
    ///
    /// Deliberately narrower than real PHPStan's own full check in two
    /// disclosed ways: the bound's/default's own further `GenericObjectTypeCheck`
    /// (does *that* type, if itself generic, specify all of *its own*
    /// required template arguments) isn't reused here - a genuinely
    /// separate primitive this project doesn't have yet, not just a
    /// missing wiring - left as a narrower, disclosed cut for this pass
    /// rather than building a whole second bound/arity-checking engine on
    /// top of the one `check_generic_bounds`/`check_generic_reference_arity`
    /// already are for usage sites. And unlike real PHPStan, a bound's/
    /// default's own referenced class is never existence-checked
    /// (`class.notFound`/`generics.traitBound`) here at all - this
    /// project's own registry is inherently partial (a real corpus false
    /// positive already found and fixed once for `check_generic_bounds`'s
    /// own bound-*violation* check applies identically here: a bound
    /// naming a real class this project simply has no data for, e.g. a
    /// PHAR-bundled dependency, is indistinguishable from a genuine typo,
    /// and this project's own established convention is "don't know,
    /// don't guess" for exactly that ambiguity - confirmed by a real,
    /// already-existing test for this same class of bound).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_template_declaration_validity(
        &mut self,
        templates: &[String],
        bounds: &HashMap<String, Type>,
        defaults: &HashMap<String, Type>,
        type_aliases: &HashMap<String, Type>,
        display_name: &str,
        label: &str,
        span: Span,
    ) {
        let mut template_with_default: Option<&str> = None;
        for name in templates {
            if self.class_name_exists(name) {
                self.report(
                    label.to_string(),
                    format!(
                        "PHPDoc tag @template for {display_name} cannot have existing class {name} as its name."
                    ),
                    span,
                    6,
                    "generics.existingClass",
                );
            }
            if type_aliases.contains_key(name) {
                self.report(
                    label.to_string(),
                    format!(
                        "PHPDoc tag @template for {display_name} cannot have existing type alias {name} as its name."
                    ),
                    span,
                    6,
                    "generics.existingTypeAlias",
                );
            }
            if let Some(bound) = bounds.get(name)
                && !is_supported_generic_bound(bound)
            {
                self.report(
                    label.to_string(),
                    format!(
                        "PHPDoc tag @template {name} for {display_name} with bound type {bound} is not supported."
                    ),
                    span,
                    6,
                    "generics.notSupportedBound",
                );
            }
            match defaults.get(name) {
                None => {
                    if let Some(prev) = template_with_default {
                        self.report(
                            label.to_string(),
                            format!(
                                "PHPDoc tag @template {name} for {display_name} does not have a default type but follows an optional @template {prev}."
                            ),
                            span,
                            6,
                            "generics.requiredTypeAfterOptional",
                        );
                    }
                }
                Some(default) => {
                    template_with_default = Some(name);
                    if let Some(bound) = bounds.get(name)
                        && self.is_type_fully_known(default)
                        && self.is_type_fully_known(bound)
                        && !self.registry.is_assignable(default, bound)
                    {
                        self.report(
                            label.to_string(),
                            format!(
                                "Default type {default} in PHPDoc tag @template {name} for {display_name} is not subtype of bound type {bound}."
                            ),
                            span,
                            6,
                            "generics.templateDefaultOutOfBounds",
                        );
                    }
                }
            }
        }
    }
}

/// Mirrors real PHPStan's own `TemplateTypeCheck`-internal hardcoded
/// whitelist of ~15 supported `@template ... of Bound` shapes -
/// deliberately conservative, mapped onto
/// this project's own `Type` variants rather than PHPStan's type-class
/// list: anything not explicitly recognized here is treated as
/// unsupported, since a false "supported" would silently let a genuinely
/// nonsensical bound (e.g. `@template T of callable`) through uncaught,
/// while a false "unsupported" only ever costs a single extra, real
/// diagnostic on an already-unusual bound choice.
fn is_supported_generic_bound(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Mixed
            | Type::Null
            | Type::Array(_, _)
            | Type::ArrayShape(_)
            | Type::String
            | Type::StringLiteral(_)
            | Type::Int
            | Type::IntLiteral(_)
            | Type::Float
            | Type::FloatLiteral(_)
            | Type::Bool
            | Type::True
            | Type::False
            | Type::Object(_, _)
            | Type::ObjectShape(_)
            | Type::Union(_)
            | Type::Intersection(_)
            | Type::TemplateParam(_)
    )
}
