use syntax::name::NameMap;

use crate::checker::{
    Checker, ITERABLE_GENERIC_CLASS_NAMES, TemplateBindings, class_kind_identifier,
};
use phpdoc::docblock;
use syntax::ast::*;
use syntax::span::Span;
use types::{self, TemplateParam, Type};

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
    /// `location` names where `ty` was written, as the messages quote it
    /// (`PHPDoc tag @param for parameter $x`).
    pub(crate) fn check_generic_bounds(
        &mut self,
        ty: &Type,
        location: &str,
        label: &str,
        span: Span,
    ) {
        stack::guard(|| {
            self.check_generic_bounds_and_arity(ty, location, label, span, true);
        })
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
        location: &str,
        label: &str,
        span: Span,
        check_arity: bool,
    ) {
        stack::guard(|| {
            if check_arity {
                self.check_generic_reference_arity(ty, location, span);
            }
            if check_arity && let Some((class, args)) = ty.as_object() {
                self.check_call_site_variance(class, args, location, span);
            }
            match ty.as_object() {
                Some((class, args)) => {
                    if !args.is_empty()
                        && let Some(info) = self.registry.classes.get(class.as_str())
                        && info.templates().iter().any(|t| !t.bound.is_mixed())
                    {
                        // Bound-checking cares about the real referenced
                        // type, not a call-site variance marker wrapping it
                        // (`Collection<covariant Foo>` still needs `Foo`
                        // itself checked against the bound -
                        // `accepts`'s own variance handling is
                        // separate) - `Type::without_call_site_variance` unwraps for
                        // both the substitution bindings and the bound
                        // comparison; the *displayed* `arg` below stays the
                        // original, marker-and-all, so a violation message
                        // still shows `covariant Foo`.
                        let bindings: TemplateBindings = info
                            .template_names()
                            .map(str::to_string)
                            .zip(args.iter().map(|a| {
                                a.without_call_site_variance()
                                    .cloned()
                                    .unwrap_or(Type::mixed())
                            }))
                            .collect();
                        for (template, arg) in info.templates().iter().zip(args.iter()) {
                            let name = template.name.as_str();
                            if template.bound.is_mixed() {
                                continue;
                            }
                            let bound = template
                                .bound
                                .substitute_templates(&bindings, &self.relation());
                            let Some(checked_arg) = arg.without_call_site_variance() else {
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
                            // Real PHPStan's own `GenericObjectTypeCheck`
                            // calls `$boundType->isSuperTypeOf(...)` directly
                            // (`vendor/phpstan-src/src/Rules/Generics/
                            // GenericObjectTypeCheck.php`), never
                            // `RuleLevelHelper::accepts()` - `checkExplicitMixed`/
                            // `checkImplicitMixed` never applies to a generic
                            // bound check in real PHPStan either, so this
                            // stays on plain `accepts`, not
                            // `rule_level_accepts` (see that
                            // method's own doc comment, which already
                            // disclosed this exact exclusion).
                            if !checked_arg.is_mixed()
                                && self.is_type_fully_known(checked_arg)
                                && self.is_type_fully_known(&bound)
                                && !self.accepts(&bound, checked_arg).yes()
                            {
                                let kind = class_kind_identifier(&info.kind());
                                self.report(format!(
                                    "Type {arg} in generic type {ty} in {location} is not subtype of template type {name} of {kind} {class}."
                                ),
                                span,
                                6,
                                "generics.notSubtype",
                            );
                            }
                        }
                    }
                    for arg in args {
                        self.check_generic_bounds(arg, location, label, span);
                    }
                }
                None => {
                    ty.for_each_child(&mut |t| self.check_generic_bounds(t, location, label, span))
                }
            }
        })
    }

    /// `false` if `ty` mentions any class this project has no real data
    /// for at all - an uninstalled dependency, or a PHAR-bundled one
    /// (real `phpstan/phpstan` ships as a compiled PHAR with no loose
    /// source files, the same gap undefined-symbol checks
    /// already document). `check_generic_bounds` uses this to skip a
    /// bound it has no way to actually verify, rather than guess wrong.
    pub(crate) fn is_type_fully_known(&self, ty: &Type) -> bool {
        stack::guard(|| {
            ty.as_object()
                .is_none_or(|(name, _)| self.registry.classes.contains_key(name.as_str()))
                && !ty.any_child(|t| !self.is_type_fully_known(t))
        })
    }

    /// `check_generic_bounds` for everything a class declares outside
    /// its own methods (which `check_function` already covers
    /// per-method): every property's type, plus its
    /// `@extends`/`@implements` arguments - both are real places a
    /// generic bound violation can be written
    /// (`class Bad extends Collection<NotAShape> {}`) without ever
    /// showing up in a method signature.
    pub(crate) fn check_class_generic_bounds(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.class_info(fqn) else {
            return;
        };
        // Each property gets its own real span (declaration order, not a
        // sorted HashMap walk - the AST's `Vec<PropertyDecl>` is already
        // deterministic, unlike the registry's `HashMap`-backed lookup).
        for p in &c.def.properties {
            if let Some(prop_sig) = info.properties().get(p.name.as_ref()) {
                self.check_generic_bounds(
                    prop_sig.ty(),
                    &format!("PHPDoc tag @var for property {fqn}::${}", p.name),
                    fqn,
                    p.span,
                );
                // `ExistingClassesInPropertiesRule`'s own check -
                // reuses this exact loop (same resolved `PropertyReflection::ty`,
                // same real per-property span) rather than a second walk
                // over `c.properties` elsewhere.
                // The own PHPDoc type too: a member the native hint
                // excludes no longer reaches `ty`.
                let checked = match prop_sig.doc_ty() {
                    Some(doc) => self
                        .relation()
                        .union_of(vec![prop_sig.ty().clone(), doc.clone()]),
                    None => prop_sig.ty().clone(),
                };
                self.check_type_class_names_exist(
                    &checked,
                    format!("property {fqn}::${}", p.name),
                    p.span,
                );
                // `property.trait` - see `check_type_trait_refs`'s
                // own doc comment.
                self.check_type_trait_refs(
                    prop_sig.ty(),
                    format!("property {fqn}::${}", p.name),
                    p.span,
                    "property.trait",
                );
                // `class.nameCase` for this PHPDoc-typed property position.
                self.check_type_class_name_case(prop_sig.case_refs(), p.span);
                self.check_type_internal_usage(prop_sig.case_refs(), p.span);
            }
        }
        self.check_property_tag_types(fqn, &c.def);
        self.check_property_phpdoc_types(fqn, &c.def);
        self.check_class_ancestors(fqn, &c.def);
        self.check_ancestor_generic_bounds(fqn, c.def.span);
        // `TemplateTypeCheck`'s own declaration-validity family,
        // class-level (real PHPStan's per-kind rule split -
        // `ClassTemplateTypeRule`/`InterfaceTemplateTypeRule`/
        // `TraitTemplateTypeRule` all funnel into the identical shared
        // check); an enum's own separate `check_enum_template_tag`/
        // `enum.generic` check is `EnumTemplateTypeRule`'s real
        // counterpart, since real PHPStan excludes enums from
        // `ClassTemplateTypeRule` entirely.
        if !info.templates().is_empty() {
            let display_name = format!("{} {fqn}", class_kind_identifier(&c.def.kind));
            let templates = info.templates().to_vec();
            let type_aliases = info.type_aliases().clone();
            self.check_template_declaration_validity(
                &templates,
                &type_aliases,
                TemplateSubject::Declared {
                    display_name: &display_name,
                },
                c.def.span,
            );
        }
    }

    /// `@property`/`@property-read`/`@property-write`'s *validation*
    /// half (real PHPStan's own `PropertyTagCheck`) - parsing and
    /// inference already happen in `ClassReflection::from_decl` (which
    /// synthesizes every tag into a real, resolved `PropertyReflection`); this
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
    fn check_property_tag_types(&mut self, fqn: &str, c: &ClassDef) {
        let Some(raw) = &c.doc_comment else {
            return;
        };
        let Some(info) = self.class_info(fqn) else {
            return;
        };
        let block = docblock::parse_shared(raw);
        for tag in &block.properties {
            let Some(prop_sig) = info.properties().get(tag.name.as_str()) else {
                continue;
            };
            let tag_name = match (tag.readable, tag.writable) {
                (true, true) => "@property",
                (true, false) => "@property-read",
                (false, true) => "@property-write",
                (false, false) => "@property",
            };
            self.check_generic_bounds(
                prop_sig.ty(),
                &format!("PHPDoc tag {tag_name} for property {fqn}::${}", tag.name),
                fqn,
                c.span,
            );
            self.check_type_class_names_exist(
                prop_sig.ty(),
                format!("PHPDoc tag {tag_name} for property {fqn}::${}", tag.name),
                c.span,
            );
            // `class.nameCase` for this `@property`-tag target.
            self.check_type_class_name_case(prop_sig.case_refs(), c.span);
            self.check_type_internal_usage(prop_sig.case_refs(), c.span);
        }
    }

    /// `generics.interfaceConflict` for a class - see
    /// `checker::generics::interfaces`'s own module doc comment.
    pub(crate) fn check_ancestor_generic_bounds(&mut self, fqn: &str, span: Span) {
        self.check_cross_path_interface_conflicts(fqn, span);
    }

    /// `GenericObjectTypeCheck` for an ancestor tag: arity and bounds, but
    /// not call-site variance, which an ancestor tag may not carry at all.
    pub(crate) fn check_ancestor_generic_object(
        &mut self,
        ty: &Type,
        location: &str,
        label: &str,
        span: Span,
    ) {
        self.check_generic_reference_arity(ty, location, span);
        self.check_generic_bounds_and_arity(ty, location, label, span, false);
    }

    /// `GenericObjectTypeCheck`'s own arity/well-formedness half
    /// (`generics.lessTypes`/`.moreTypes`/`.notGeneric`) - a genuinely
    /// different rule than `check_generic_bounds`'s bound-checking, which
    /// silently truncates via `zip` (an extra argument beyond
    /// `info.templates.len()` is never looked at) rather than flagging
    /// the mismatch. Called pervasively from
    /// `check_generic_bounds_and_arity`, matching real PHPStan's
    /// `IncompatiblePhpDocTypeCheck`. Every template with its own
    /// `@template T = Default` (`TemplateParam::default`) is optional - only
    /// a template with *no* default is "required" for `lessTypes`
    /// purposes.
    ///
    /// `generics.notGeneric` was previously left unbuilt here over a
    /// worry that "genuinely not generic" and "this project's own
    /// `@template`-tag parsing has a gap for this class" are
    /// indistinguishable - concretely, a project's own custom stub file
    /// retroactively granting templates onto a real builtin with none of
    /// its own (real shape: Piwigo's own `phpstan-stubs/
    /// LatteCachingIterator.stub.php`, whose own `@extends
    /// \CachingIterator<TKey, TValue, Iterator<TKey, TValue>>` supplies 3
    /// type arguments to real PHP's own `\CachingIterator`, which has no
    /// `@template` tag anywhere). That worry doesn't actually apply:
    /// confirmed directly (`Session::listing`, where a configured stub
    /// directory is listed as a stub, never analysed) that a
    /// project's own configured stub file is always loaded as *context*
    /// (`analyzed: false`) - its own class declarations, `@extends`
    /// clause included, are never run through this checker's own
    /// diagnostic pass at all, exactly mirroring real PHPStan's own
    /// `stubFiles` semantics (also reflection-only, never analysed). The
    /// Piwigo stub above is the concrete confirming case: real,
    /// currently-installed `vendor/bin/phpstan` (Piwigo's own
    /// `phpstan.neon`, `stubFiles: [...LatteCachingIterator.stub.php]`)
    /// never runs any rule against that file's own content either - only
    /// ordinary application code, whose registry state already reflects
    /// every stub-granted template correctly, ever reaches this check.
    fn check_generic_reference_arity(&mut self, ty: &Type, location: &str, span: Span) {
        let Some((class, args)) = ty.as_object() else {
            return;
        };
        if args.is_empty() {
            return;
        }
        let Some(info) = self.registry.classes.get(class.as_str()) else {
            return;
        };
        let kind = class_kind_identifier(&info.kind());
        if info.templates().is_empty() {
            self.report(
                format!("{location} contains generic type {ty} but {kind} {class} is not generic."),
                span,
                6,
                "generics.notGeneric",
            );
            return;
        }
        // Real PHPStan hardcodes a parse-time special case
        // (`PhpDoc\TypeNodeResolver::resolveGenericTypeNode`) for four
        // built-in names: `Traversable`/`Iterator`/`IteratorAggregate<V>`
        // pads a single given argument into `<mixed, V>` before this
        // check ever runs, and `Generator<V>`/`Generator<K, V>` pads to
        // the full `<K, V, mixed, mixed>` shape - neither ever reaches
        // `lessTypes`, even though none of their four templates declares
        // a `@template ... = Default`. This project doesn't model that resolution-time padding
        // itself, so this narrowly special-cases the exact same argument
        // counts real PHPStan's hardcoded list treats as pre-padded
        // (real `phpstan/phpstan` flags neither `@return
        // \Generator<int, string>` nor a bare `Traversable<V>`, but does
        // flag `Generator<int, string, bool>` - three args isn't one of
        // its padded shapes).
        let is_pre_padded_short_form = (args.len() == 1
            && ITERABLE_GENERIC_CLASS_NAMES
                .iter()
                .any(|name| class.eq_ignore_ascii_case(name)))
            || (args.len() == 2 && class.eq_ignore_ascii_case("Generator"));
        if is_pre_padded_short_form {
            return;
        }
        let required = info
            .templates()
            .iter()
            .filter(|template| template.default.is_none())
            .count();
        let total = info.templates().len();
        let list = info.template_names().collect::<Vec<_>>().join(", ");
        if required > args.len() {
            let list = if required == total {
                list
            } else {
                format!("{list} ({required}-{total} required).")
            };
            self.report(format!(
                    "Generic type {ty} in {location} does not specify all template types of {kind} {class}: {list}"
                ),
                span,
                6,
                "generics.lessTypes",
            );
        } else if total < args.len() {
            let list = if required == total {
                list
            } else {
                format!("{list} ({required}-{total} required)")
            };
            self.report(format!(
                    "Generic type {ty} in {location} specifies {} template types, but {kind} {class} supports only {total}: {list}",
                    args.len()
                ),
                span,
                6,
                "generics.moreTypes",
            );
        }
    }

    /// `GenericObjectTypeCheck`'s own call-site-variance half
    /// (`generics.callSiteVarianceConflict`/`.callSiteVarianceRedundant`).
    /// A call-site variance marker (`Collection<covariant Foo>`) is
    /// redundant when it names the exact same variance `Collection`'s
    /// `@template` already declares for that slot, and conflicting when
    /// it names the *opposite* real variance (`contravariant` written on
    /// a slot the class declared `@template-covariant`, or vice versa) -
    /// ported directly from real PHPStan's
    /// `TemplateTypeVariance::validPosition`: a call-site annotation is
    /// only ever unsound against a slot the class declares as the
    /// *other* non-invariant direction. Writing `covariant`/
    /// `contravariant` on a slot the class declares plain invariant is
    /// exactly call-site variance's intended escape hatch (never
    /// flagged), and `*` (bivariant) is never flagged at all.
    ///
    /// Skips real PHPStan's narrow `ReflectionClass<covariant T>`
    /// carve-out (a compat allowance for PHP 8.3 vs. 8.4's differing
    /// core stub): this project's vendored
    /// `phpstorm-stubs/Reflection/ReflectionClass.php` declares its `T`
    /// with a plain `@template T of object`, not `@template-covariant`,
    /// so the carve-out's gating condition can never be reached against
    /// this project's actual data.
    fn check_call_site_variance(&mut self, class: &str, args: &[Type], location: &str, span: Span) {
        let Some(info) = self.class_info(class) else {
            return;
        };
        let kind = class_kind_identifier(&info.kind());
        let ty = Type::object(class.into(), args.to_vec());
        for (i, arg) in args.iter().enumerate() {
            let call_site = match arg.call_site_variance() {
                Some(types::CallSiteVariance::Covariant) => types::Variance::Covariant,
                Some(types::CallSiteVariance::Contravariant) => types::Variance::Contravariant,
                // `*` (bivariant) is never flagged - see this function's
                // own doc comment.
                _ => continue,
            };
            let Some(template) = info.templates().get(i) else {
                continue;
            };
            let declared = template.variance;
            let template_name = &template.name;
            if declared == call_site {
                self.report(format!(
                        "Call-site variance of {arg} in generic type {ty} in {location} is redundant, template type {template_name} of {kind} {class} has the same variance."
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
                self.report(format!(
                        "Call-site variance of {arg} in generic type {ty} in {location} is in conflict with {declared_word} template type {template_name} of {kind} {class}."
                    ),
                    span,
                    6,
                    "generics.callSiteVarianceConflict",
                );
            }
        }
    }

    /// `TemplateTypeCheck`'s own declaration-validity family -
    /// validates a `@template` tag's own declaration, not a usage site.
    /// Shared between a class/interface/trait's own templates
    /// (read straight off its `ClassReflection`) and a method's own
    /// (re-parsed fresh from its own doc comment, since `FunctionReflection`
    /// has no persisted raw template-bound/-default map - see
    /// `Checker::current_function_template_params`) - `type_aliases` is
    /// always the *enclosing class's* map either way (a type alias is
    /// class-scoped).
    ///
    /// Deliberately narrower than real PHPStan's full check in two
    /// disclosed ways: the bound's/default's own further
    /// `GenericObjectTypeCheck` isn't reused here - a genuinely separate
    /// primitive this project doesn't have yet, left as a narrower cut
    /// rather than building a second bound/arity-checking engine on top
    /// of `check_generic_bounds`/`check_generic_reference_arity`. And
    /// unlike real PHPStan, a bound's/default's own referenced class is
    /// never existence-checked here at all: this project's registry is
    /// inherently partial, so a bound naming a real class this project
    /// has no data for (e.g. a PHAR-bundled dependency) is
    /// indistinguishable from a genuine typo, and this project's
    /// established convention is "don't know, don't guess" for exactly
    /// that ambiguity.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_template_declaration_validity(
        &mut self,
        templates: &[TemplateParam],
        type_aliases: &NameMap<Type>,
        subject: TemplateSubject<'_>,
        span: Span,
    ) {
        let mut template_with_default: Option<&str> = None;
        for template in templates {
            let name = template.name.as_str();
            let bound = &*template.bound;
            if self.class_name_exists(name) {
                self.report(
                    subject.existing_class(name),
                    span,
                    6,
                    "generics.existingClass",
                );
            }
            if type_aliases.contains_key(name) {
                self.report(
                    subject.existing_type_alias(name),
                    span,
                    6,
                    "generics.existingTypeAlias",
                );
            }
            if !bound.can_bound_a_template() {
                self.report(
                    subject.unsupported_bound(name, bound),
                    span,
                    6,
                    "generics.notSupportedBound",
                );
            }
            match template.default.as_deref() {
                None => {
                    if let Some(prev) = template_with_default {
                        self.report(
                            subject.required_after_optional(name, prev),
                            span,
                            6,
                            "generics.requiredTypeAfterOptional",
                        );
                    }
                }
                Some(default) => {
                    template_with_default = Some(name);
                    if self.is_type_fully_known(default)
                        && self.is_type_fully_known(bound)
                        && !self.accepts(bound, default).yes()
                    {
                        self.report(
                            subject.default_out_of_bounds(name, default, bound),
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

/// Whose templates `check_template_declaration_validity` checks, which
/// decides its messages' wording.
#[derive(Clone, Copy)]
pub(crate) enum TemplateSubject<'s> {
    /// A class's, function's or method's `@template` tags.
    Declared { display_name: &'s str },
    /// A callable type's own templates, in the tag at `location`.
    Callable {
        location: &'s str,
        description: &'s str,
    },
    /// A `@method` tag's own templates.
    MethodTag { class: &'s str, method: &'s str },
}

impl TemplateSubject<'_> {
    fn existing_class(&self, name: &str) -> String {
        match self {
            TemplateSubject::Declared { display_name } => format!(
                "PHPDoc tag @template for {display_name} cannot have existing class {name} as its name."
            ),
            TemplateSubject::Callable {
                location,
                description,
            } => format!(
                "PHPDoc tag {location} template of {description} cannot have existing class {name} as its name."
            ),
            TemplateSubject::MethodTag { class, method } => format!(
                "PHPDoc tag @method template for method {class}::{method}() cannot have existing class {name} as its name."
            ),
        }
    }

    fn existing_type_alias(&self, name: &str) -> String {
        match self {
            TemplateSubject::Declared { display_name } => format!(
                "PHPDoc tag @template for {display_name} cannot have existing type alias {name} as its name."
            ),
            TemplateSubject::Callable {
                location,
                description,
            } => format!(
                "PHPDoc tag {location} template of {description} cannot have existing type alias {name} as its name."
            ),
            TemplateSubject::MethodTag { class, method } => format!(
                "PHPDoc tag @method template for method {class}::{method}() cannot have existing type alias {name} as its name."
            ),
        }
    }

    fn unsupported_bound(&self, name: &str, bound: &Type) -> String {
        match self {
            TemplateSubject::Declared { display_name } => format!(
                "PHPDoc tag @template {name} for {display_name} with bound type {bound} is not supported."
            ),
            TemplateSubject::Callable {
                location,
                description,
            } => format!(
                "PHPDoc tag {location} template {name} of {description} with bound type {bound} is not supported."
            ),
            TemplateSubject::MethodTag { class, method } => format!(
                "PHPDoc tag @method template {name} for method {class}::{method}() with bound type {bound} is not supported."
            ),
        }
    }

    fn required_after_optional(&self, name: &str, prev: &str) -> String {
        match self {
            TemplateSubject::Declared { display_name } => format!(
                "PHPDoc tag @template {name} for {display_name} does not have a default type but follows an optional @template {prev}."
            ),
            TemplateSubject::Callable {
                location,
                description,
            } => format!(
                "PHPDoc tag {location} template {name} of {description} does not have a default type but follows an optional template {prev}."
            ),
            TemplateSubject::MethodTag { class, method } => format!(
                "PHPDoc tag @template {name} for method {class}::{method}() does not have a default type but follows an optional @template {prev}."
            ),
        }
    }

    fn default_out_of_bounds(&self, name: &str, default: &Type, bound: &Type) -> String {
        match self {
            TemplateSubject::Declared { display_name } => format!(
                "Default type {default} in PHPDoc tag @template {name} for {display_name} is not subtype of bound type {bound}."
            ),
            TemplateSubject::Callable {
                location,
                description,
            } => format!(
                "Default type {default} in PHPDoc tag {location} template {name} of {description} is not subtype of bound type {bound}."
            ),
            TemplateSubject::MethodTag { class, method } => format!(
                "Default type {default} in PHPDoc tag @method template {name} for method {class}::{method}() is not subtype of bound type {bound}."
            ),
        }
    }
}
