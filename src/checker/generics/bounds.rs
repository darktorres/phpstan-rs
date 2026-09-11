use std::collections::HashMap;

use crate::ast::*;
use crate::checker::{Checker, TemplateBindings, class_kind_identifier};
use crate::docblock;
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
                // Phase 48.A: `property.trait` - see `check_type_trait_refs`'s
                // own doc comment.
                self.check_type_trait_refs(
                    &prop_sig.ty,
                    &format!("property {fqn}::${}", p.name),
                    p.span,
                    fqn,
                    "property.trait",
                );
            }
        }
        self.check_property_tag_types(fqn, c);
        self.check_ancestor_generic_bounds(fqn, c.span);
        // Phase 54: `TemplateTypeCheck`'s own declaration-validity family,
        // class-level (real PHPStan's own per-kind rule split -
        // `ClassTemplateTypeRule`/`InterfaceTemplateTypeRule`/
        // `TraitTemplateTypeRule` - all funnel into the identical shared
        // check, so one call site covers all three; an enum's own
        // separate, already-shipped `check_enum_template_tag`/
        // `enum.generic` check is `EnumTemplateTypeRule`'s own real
        // counterpart, confirmed by direct read that real PHPStan
        // excludes enums from `ClassTemplateTypeRule` entirely rather
        // than layering this family on top of `enum.generic` too).
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
        // bound-checking just below (an extra/misattributed argument is
        // simply never looked at), but `check_generic_reference_arity`'s
        // own exact-count comparison is far more sensitive to it - only
        // trustworthy when there's exactly one `extends` target to begin
        // with, the one case `extends_args` is actually guaranteed to
        // belong to.
        let mut arity_check_types: Vec<Type> = Vec::new();
        if let Some(parent) = info.extends.first() {
            let parent_ty = Type::Object(parent.clone(), info.extends_args.clone());
            if info.extends.len() == 1 {
                arity_check_types.push(parent_ty.clone());
            }
            types_to_check.push(parent_ty);
        }
        // `implements_args` is a `HashMap` with no guaranteed iteration
        // order - sorted by key so two diagnostics from it always come out
        // in the same relative order, not one that silently varies by
        // process (Rust's default hasher is randomly seeded per run).
        let mut iface_names: Vec<&String> = info.implements_args.keys().collect();
        iface_names.sort();
        for iface in iface_names {
            let iface_ty = Type::Object(iface.clone(), info.implements_args[iface].clone());
            arity_check_types.push(iface_ty.clone());
            types_to_check.push(iface_ty);
        }
        for ty in &types_to_check {
            self.check_generic_bounds(ty, fqn, span);
        }
        for ty in &arity_check_types {
            self.check_generic_reference_arity(ty, fqn, span);
        }
    }

    /// Phase 54 follow-up: `GenericObjectTypeCheck`'s own arity/well-
    /// formedness half (`generics.lessTypes`/`.moreTypes`, confirmed
    /// directly against `vendor/phpstan-src`) - a genuinely different
    /// rule than `check_generic_bounds`'s own bound-checking, which
    /// silently truncates via `zip` (an extra argument beyond `info.
    /// templates.len()` is simply never looked at) rather than flagging
    /// the mismatch itself. Deliberately scoped to only this function's
    /// own `extends`/`implements` call site - not `check_generic_bounds`
    /// itself, which runs pervasively over *every* generic type
    /// reference anywhere (a property type, a param/return hint, ...) -
    /// matching real PHPStan's own identically narrow scope here
    /// (`ClassAncestorsRule`/`InterfaceAncestorsRule`, not the general-
    /// purpose `GenericObjectTypeCheck` caller used for every other
    /// position, which has its own, much larger set of callers this
    /// project doesn't attempt to mirror one-for-one). Every template
    /// with its own `@template T = Default` (`template_defaults`) is
    /// optional - only a template with *no* default is "required" for
    /// `lessTypes` purposes. Real PHPStan's own third case,
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

    /// Phase 54: `TemplateTypeCheck`'s own declaration-validity family
    /// (confirmed by direct read, `vendor/phpstan-src`) - validates a
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

/// Phase 54: mirrors real PHPStan's own `TemplateTypeCheck`-internal
/// hardcoded whitelist of ~15 supported `@template ... of Bound` shapes
/// (confirmed by direct read) - deliberately conservative, mapped onto
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
