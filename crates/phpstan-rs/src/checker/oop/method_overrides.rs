use crate::checker::Checker;
use crate::checker::oop::visibility_word;
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use syntax::span::Span;
use types::Name;
use types::Type;

impl<'a> Checker<'a> {
    pub(crate) fn check_method_overrides(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        let Some(def) = arenas.class(c.def) else {
            return;
        };
        let methods = arenas.methods(def.methods.clone());
        // Redeclaring a native enum method is itself the error.
        for m in methods
            .iter()
            .filter(|m| !super::enums::is_native_enum_method(def, &m.function.name))
        {
            let attrs = arenas.attributes(m.function.attrs.clone());
            let decl = DeclSpans {
                name: m.function.name.span,
                override_removal: attrs
                    .iter()
                    .find(|a| crate::registry::is_override_attribute_name(&a.name))
                    .map(|a| a.removal_span),
            };
            self.check_one_method_override(
                fqn,
                &m.function.name,
                &m.visibility,
                m.span,
                Some(decl),
            );
        }
        // A method a trait supplies but the class itself never
        // textually re-declares still needs the identical override-
        // compatibility checking against any ancestor interface/parent -
        // real PHP performs Liskov compatibility checking after trait
        // flattening, not just against a class's own directly-declared
        // methods. No AST node exists for a trait-only method within
        // this class's own declaration to anchor a precise span on
        // (real PHP itself points at the trait's declaration line, in
        // the trait's own file - information `FunctionReflection` doesn't
        // carry), so `c.span` (the using class's own declaration) is
        // used instead, matching this project's established fallback
        // for class-level diagnostics with no more specific member
        // location (`check_class_completeness`, `check_trait_conflicts`,
        // ...).
        if !def.trait_uses(arenas).is_empty() {
            let own_method_names: rustc_hash::FxHashSet<&str> =
                methods.iter().map(|m| m.function.name.as_ref()).collect();
            let mut trait_method_names: Vec<Name> = self
                .registry
                .method_names(fqn)
                .filter(|name| {
                    !syntax::ast::is_constructor_name(name)
                        && !own_method_names.contains(name.as_ref())
                        && !self.registry.method_is_abstract(fqn, name)
                })
                .cloned()
                .collect();
            trait_method_names.sort();
            for name in trait_method_names {
                let Some(own_visibility) = self
                    .registry
                    .lookup_own_method(fqn, &name)
                    .map(|s| s.visibility())
                else {
                    continue;
                };
                self.check_one_method_override(fqn, &name, &own_visibility, def.span, None);
            }
        }
    }

    /// Mirrors real PHPStan's `MethodThrowTypeCovarianceRule` - opt-in
    /// (`RulesSettings::check_method_throw_type_covariance`, see its doc
    /// comment for why this project ships it opt-in unlike real
    /// PHPStan's unconditional registration). A pure declaration-vs-
    /// declaration comparison: this method's `@throws` (or, absent one,
    /// an implicit `Throwable`, hardcoded here rather than separately
    /// configurable) must be covariant with every immediate parent
    /// class/transitive interface/abstract trait declaration's own
    /// `@throws` (same implicit-`Throwable` fallback). No call-graph-
    /// wide implicit-throw model is needed for this - real PHPStan's
    /// default already treats an untagged declaration as "throws
    /// Throwable" rather than needing to trace what it actually,
    /// transitively throws.
    pub(crate) fn check_method_throw_type_covariance(
        &mut self,
        fqn: &str,
        method_name: &str,
        own_visibility: &Visibility,
        span: Span,
    ) {
        if !self.settings.rules.check_method_throw_type_covariance {
            return;
        }
        if syntax::ast::is_constructor_name(method_name) || *own_visibility == Visibility::Private {
            return;
        }
        let Some(references) = self.registry.class_references_by_name(fqn) else {
            return;
        };
        let Some(own_sig) = self.registry.lookup_own_method(fqn, method_name) else {
            return;
        };
        let throwable = || Type::object("Throwable".into(), Vec::new());
        let own_throws = own_sig.throws().clone().unwrap_or_else(throwable);
        if own_throws.is_void() {
            return;
        }

        // Mirrors real PHPStan's `ParentMethodHelper::
        // collectParentMethods`: the immediate parent class's own native
        // declaration (not the full `extends` chain - each class in a
        // longer chain runs this same check against only its own
        // immediate parent, so a multi-level chain still gets fully
        // covered, one link at a time), every transitive interface
        // declaring this method, and every used trait's own abstract
        // declaration of it (a trait's concrete contribution becomes the
        // class's effective implementation, not something to compare the
        // class against).
        let mut ancestors: Vec<(String, Type)> = Vec::new();
        if let Some(parent) = references.extends.first()
            && let Some(parent_sig) = self
                .registry
                .class_by_name(parent)
                .and_then(|_| self.registry.lookup_own_method(parent, method_name))
            && parent_sig.visibility() != Visibility::Private
        {
            ancestors.push((
                parent.to_string(),
                parent_sig.throws().clone().unwrap_or_else(throwable),
            ));
        }
        for (owner, sig) in self
            .registry
            .all_interfaces_declaring_method(fqn, method_name)
        {
            ancestors.push((
                owner.to_string(),
                sig.throws().clone().unwrap_or_else(throwable),
            ));
        }
        for used in references.uses {
            if self.registry.method_is_abstract(used, method_name)
                && let Some(trait_sig) = self.registry.lookup_own_method(used, method_name)
            {
                ancestors.push((
                    used.to_string(),
                    trait_sig.throws().clone().unwrap_or_else(throwable),
                ));
            }
        }

        for (owner, parent_throws) in ancestors {
            if parent_throws.is_void() {
                self.report(format!(
                        "Method {fqn}::{method_name}() should not throw {own_throws} because parent method {owner}::{method_name}() has PHPDoc tag @throws void."
                    ),
                    span,
                    2,
                    "throws.shouldBeVoid",
                );
                continue;
            }
            if self.accepts(&parent_throws, &own_throws).yes() {
                continue;
            }
            self.report(format!(
                    "PHPDoc tag @throws type {own_throws} of method {fqn}::{method_name}() should be covariant with PHPDoc @throws type {parent_throws} of method {owner}::{method_name}()."
                ),
                span,
                2,
                "throws.notCovariant",
            );
        }
    }

    /// The full per-method override-compatibility check, shared between
    /// a class's own textually-declared methods and a trait-supplied
    /// method it never re-declares (see `check_method_overrides` above) -
    /// the two differ only in which visibility/span they can offer.
    fn check_one_method_override(
        &mut self,
        fqn: &str,
        method_name: &str,
        own_visibility: &Visibility,
        span: Span,
        decl: Option<DeclSpans>,
    ) {
        let override_removal = decl.and_then(|d| d.override_removal);
        // Dispatched through `internal_rules`'s
        // `Vec<Box<dyn InternalRule>>` rather than called directly - see
        // that module's doc comment.
        for rule in crate::checker::internal_rules::declaration_level_rules() {
            rule.check_method(self, fqn, method_name, own_visibility, span);
        }
        // Overriding a `final` method is a real, unconditional fatal
        // error in PHP - including for __construct, unlike the
        // signature-compatibility checks below, so this runs before that
        // exemption. `is_by_phpdoc` is `@final`'s docblock-only sibling
        // (a purely static-analysis convention real PHP itself never
        // enforces) - real PHPStan's message wording/identifier/
        // `nonIgnorable()`-ness genuinely differ between the two
        // (`method.parentMethodFinal`, unconditionally non-ignorable,
        // vs. `method.parentMethodFinalByPhpDoc`, ordinarily
        // suppressible).
        if let Some((ancestor, is_by_phpdoc)) =
            self.registry.find_final_ancestor_method(fqn, method_name)
        {
            let (verb, identifier) = if is_by_phpdoc {
                ("@final", "method.parentMethodFinalByPhpDoc")
            } else {
                ("final", "method.parentMethodFinal")
            };
            let message = format!(
                "Method {fqn}::{method_name}() overrides {verb} method {ancestor}::{method_name}()."
            );
            if is_by_phpdoc {
                self.report(message, span, 0, identifier);
            } else {
                self.report_builder(
                    DiagnosticBuilder::new(message, span, 0, identifier).non_ignorable(),
                );
            }
        }
        let Some(info) = self.class_info(fqn) else {
            return;
        };
        let Some(own_sig) = self.registry.lookup_own_method(fqn, method_name) else {
            return;
        };
        if let Some((ancestor_owner, ancestor_name)) = self
            .registry
            .find_case_mismatched_ancestor_method(fqn, method_name)
        {
            let mut builder = DiagnosticBuilder::new(
                format!(
                    "Method {fqn}::{method_name}() overrides {ancestor_owner}::{ancestor_name}() with different letter casing"
                ),
                span,
                4,
                "method.nameCase",
            );
            if let Some(decl) = decl
                && let Some((span, replacement, description)) =
                    Self::name_case_fix(method_name, &ancestor_name, decl.name)
            {
                builder = builder.fix(span, replacement, description);
            }
            self.report_builder(builder);
        }
        // Mirrors real PHPStan's `MethodPrototypeFinder` priority order
        // (immediately-implemented interface, then a used trait's own
        // abstract declaration, then the plain `extends` chain) - not
        // the `extends`-before-`implements` order
        // `lookup_ancestor_method_with_owner` uses for its other,
        // non-override-compatibility callers.
        let ancestor = self
            .registry
            .find_method_signature_prototype(fqn, method_name);
        // The unconditional-fatal-error half of PHP 8.3's
        // `#[\Override]` attribute, mirroring real PHPStan's
        // `method.override` identifier
        // (`OverridingMethodRule::hasOverrideAttribute`/`processNode`).
        // The other direction (`method.missingOverride` - a real
        // override missing the attribute) is a real PHPStan opt-in rule
        // gated behind `checkMissingOverrideMethodAttribute`, off by
        // default - not attempted here. A private ancestor method
        // doesn't count as "a matching parent method" either - same
        // `has_real_ancestor` check the signature-compatibility
        // exemption below also needs. A trait's own declaration is
        // exempted entirely (`info.kind`, not just "ancestor not
        // found") - real PHP defers this whole check to whichever class
        // eventually `use`s the trait, since a trait has no ancestor
        // chain of its own to check against, and the fatal error is
        // attributed to the using class's own method name, not the
        // trait's, which is exactly what running this same check from
        // both of `check_method_overrides`'s call sites above, keyed on
        // the using class's `fqn`, already produces for free).
        // Implementing a trait's own abstract method also counts as a
        // real override (`trait_declares_abstract_method` - deliberately
        // not folded into the general `ancestor` lookup above, which
        // stays extends/implements only for every other check in this
        // function). Likewise, an unresolved name anywhere in `fqn`'s
        // extends/implements chain (`has_unresolvable_ancestor` - a
        // dependency this registry has no declaration for at all) means
        // "no matching parent method" can't be confidently claimed.
        let has_real_ancestor = ancestor
            .as_ref()
            .is_some_and(|(_, sig, _)| sig.visibility() != Visibility::Private)
            || self
                .registry
                .trait_declares_abstract_method(fqn, method_name);
        if own_sig.has_override_attr()
            && !has_real_ancestor
            && info.kind() != ClassKind::Trait
            && !self.registry.has_unresolvable_ancestor(fqn)
        {
            let mut builder = DiagnosticBuilder::new(
                format!(
                    "{fqn}::{method_name}() has #[\\Override] attribute but does not override any method"
                ),
                span,
                0,
                "method.override",
            );
            if let Some(removal) = override_removal {
                builder = builder.fix(removal, "", "Remove #[\\Override]");
            }
            self.report_builder(builder);
        }
        // `method.nonStatic`/`method.static`, mirroring real PHPStan's
        // `OverridingMethodRule`: an override must match its ancestor's
        // static-ness exactly, either direction (nonIgnorable, a real
        // unconditional PHP fatal error either way). Uses `ancestor`
        // (the plain, non-`__construct`-adjusted prototype), not
        // `compatibility_ancestor` below - static-ness isn't part of the
        // parameter-signature exemption constructors get, so a
        // constructor still needs checking against its real prototype
        // like any other method.
        if let Some((ancestor_owner, ancestor_sig, _)) = &ancestor
            && ancestor_sig.visibility() != Visibility::Private
        {
            let own_is_static = self.registry.method_is_static(fqn, method_name);
            let ancestor_is_static = self.registry.method_is_static(ancestor_owner, method_name);
            if ancestor_is_static && !own_is_static {
                self.report_builder(
                    DiagnosticBuilder::new(format!(
                            "Non-static method {fqn}::{method_name}() overrides static method {ancestor_owner}::{method_name}()"
                        ),
                        span,
                        0,
                        "method.nonStatic",
                    )
                    .non_ignorable(),
                );
            } else if !ancestor_is_static && own_is_static {
                self.report_builder(
                    DiagnosticBuilder::new(format!(
                            "Static method {fqn}::{method_name}() overrides non-static method {ancestor_owner}::{method_name}()"
                        ),
                        span,
                        0,
                        "method.static",
                    )
                    .non_ignorable(),
                );
            }
            // The general-case (non-`__construct`-special-path) half of
            // `method.abstractOverridingNonAbstract` - making a
            // non-abstract ancestor method abstract in the override.
            // Real PHPStan's rule has a second, `__construct`-specific
            // branch too (triggered only when no normal prototype
            // resolves via the usual interface/trait/`extends` walk) -
            // a distinct, deliberately unbuilt code path. An interface's
            // method is always implicitly abstract (PHP never requires -
            // or even allows - the literal `abstract` keyword there),
            // unlike `static_methods` above, where interface methods use
            // the same explicit-keyword convention as any other class.
            let own_is_abstract = self.registry.method_is_abstract(fqn, method_name);
            let ancestor_is_abstract = self
                .registry
                .class_by_name(ancestor_owner)
                .is_some_and(|i| i.kind() == ClassKind::Interface)
                || self
                    .registry
                    .method_is_abstract(ancestor_owner, method_name);
            if own_is_abstract && !ancestor_is_abstract {
                self.report_builder(
                    DiagnosticBuilder::new(format!(
                            "Cannot make non-abstract method {ancestor_owner}::{method_name}() abstract in class {fqn}"
                        ),
                        span,
                        0,
                        "method.abstractOverridingNonAbstract",
                    )
                    .non_ignorable(),
                );
            }
        }
        // Real PHP deliberately exempts constructors from the
        // signature-compatibility checking below - a subclass's own
        // __construct is allowed an entirely different, otherwise-
        // incompatible signature from its parent's, unlike every other
        // method. The `#[\Override]` check above is unaffected by this:
        // `#[\Override]` on a `__construct()` with no ancestor
        // `__construct()` fatals exactly like any other method.
        //
        // `@phpstan-consistent-constructor` (`Registry::
        // find_consistent_constructor_ancestor`'s doc comment) re-enables
        // exactly this exemption, but only against the nearest tagged
        // ancestor's own constructor, not the immediate parent's - a
        // genuinely different ancestor than `ancestor` above (which the
        // `#[\Override]` check needs to keep using regardless of any
        // tag).
        let compatibility_ancestor = if syntax::ast::is_constructor_name(method_name) {
            self.registry
                .find_consistent_constructor_ancestor(fqn)
                .map(|(owner, sig)| (owner, sig, true))
        } else {
            ancestor
        };
        let Some((ancestor_owner, ancestor_sig, check_visibility)) = compatibility_ancestor else {
            return;
        };
        // A private ancestor method isn't a real override target at
        // all - real PHP's method dispatch is never virtual for private
        // (a same-named method on a subclass is an unrelated
        // declaration, not a polymorphic override). `lookup_ancestor_
        // method_with_owner` itself doesn't filter by visibility (it
        // answers "is there a same-named declaration up the chain," the
        // right question for the visibility-narrowing check below, which
        // needs to see a private ancestor to know it's not a real
        // override) - this exemption belongs here, not there.
        if ancestor_sig.visibility() == Visibility::Private {
            return;
        }
        // Narrowing a method's visibility on override is a real,
        // unconditional PHP fatal error, mirroring real PHPStan's
        // `MethodVisibilityComparisonHelper` (same identifier, same
        // two-way split: an ancestor's `public` demands `public`; an
        // ancestor's `protected` only forbids narrowing all the way to
        // `private`). Skipped entirely when `!check_visibility` (a
        // trait-abstract-sourced prototype only, per real
        // `MethodPrototypeFinder::findPrototype`'s third tuple element) -
        // real PHP doesn't enforce this for a trait's `abstract`
        // declaration the way it does for an interface/`extends`
        // ancestor.
        let narrows_visibility = check_visibility
            && match ancestor_sig.visibility() {
                Visibility::Public => *own_visibility != Visibility::Public,
                Visibility::Protected => *own_visibility == Visibility::Private,
                Visibility::Private => false,
            };
        if narrows_visibility {
            self.report(format!(
                    "{fqn}::{method_name}() overriding {} method {ancestor_owner}::{method_name}() must not reduce visibility",
                    visibility_word(&ancestor_sig.visibility()),
                ),
                span,
                0,
                "method.visibility",
            );
        }
        for (i, ancestor_param) in ancestor_sig.params().iter().enumerate() {
            let Some(own_param) = own_sig.params().get(i) else {
                // Real PHP fatal error - the override doesn't declare
                // this many parameters at all. Skipped for a variadic
                // ancestor tail (`...$args`): real PHPStan's handling of
                // that shape is considerably more involved
                // (contravariance across every trailing ancestor
                // parameter the variadic swallows) than this project
                // attempts here.
                if !ancestor_param.variadic() {
                    self.report(format!(
                            "{fqn}::{method_name}() overrides {ancestor_owner}::{method_name}() but misses parameter #{} (${})",
                            i + 1,
                            ancestor_param.name()
                        ),
                        span,
                        0,
                        "parameter.missing",
                    );
                }
                continue;
            };
            // A parameter the ancestor made optional (has a default)
            // can never become required on override - a caller holding
            // the ancestor's type could always omit it. Real,
            // unconditional PHP fatal error. Not gated on either side's
            // type being `Mixed` the way the narrowing check below is -
            // this is about arity/callability, not type compatibility.
            if ancestor_param.has_default() && !own_param.has_default() && !own_param.variadic() {
                self.report(format!(
                        "Parameter #{} (${}) of {fqn}::{method_name}() is required but parameter #{} (${}) of {ancestor_owner}::{method_name}() is optional",
                        i + 1,
                        own_param.name(),
                        i + 1,
                        ancestor_param.name()
                    ),
                    span,
                    0,
                    "parameter.notOptional",
                );
            }
            // By-ref/variadic-contravariance mismatches - real
            // PHPStan's `MethodParameterComparisonHelper` spends most of
            // its own ~250 lines on this cluster; only the two
            // unconditional-fatal-error shapes are checked here, matching
            // this project's narrower scope elsewhere in this function
            // (`parameter.missing` above explicitly skips the fuller
            // variadic-contravariance treatment too). Neither is gated on
            // either side being `mixed` - both are about calling-
            // convention compatibility, not type compatibility.
            if own_param.by_ref() != ancestor_param.by_ref() {
                let (message, identifier) = if own_param.by_ref() {
                    (
                        format!(
                            "Parameter #{} (${}) of {fqn}::{method_name}() is passed by reference but parameter #{} (${}) of {ancestor_owner}::{method_name}() is not passed by reference",
                            i + 1,
                            own_param.name(),
                            i + 1,
                            ancestor_param.name()
                        ),
                        "parameter.byRef",
                    )
                } else {
                    (
                        format!(
                            "Parameter #{} (${}) of {fqn}::{method_name}() is not passed by reference but parameter #{} (${}) of {ancestor_owner}::{method_name}() is passed by reference",
                            i + 1,
                            own_param.name(),
                            i + 1,
                            ancestor_param.name()
                        ),
                        "parameter.notByRef",
                    )
                };
                self.report(message, span, 0, identifier);
            }
            // The reverse direction (a non-variadic ancestor parameter
            // widened to variadic on override) is real, legal PHP, so
            // only this direction is checked.
            if ancestor_param.variadic() && !own_param.variadic() {
                self.report(format!(
                        "Parameter #{} (${}) of {fqn}::{method_name}() is not variadic but parameter #{} (${}) of {ancestor_owner}::{method_name}() is variadic",
                        i + 1, own_param.name(),
                        i + 1, ancestor_param.name()
                    ),
                    span,
                    0,
                    "parameter.notVariadic",
                );
            }
            // `mixed` is overloaded throughout this project to
            // mean both real PHP `mixed` and "no type declared at all,
            // native or docblock" - and real PHP explicitly does not
            // enforce override compatibility for a parameter/return
            // position that isn't typed at all (only an explicitly
            // narrower declared type is a real fatal error). Since an
            // intentional, explicit `mixed` on an override is rare next
            // to "just wasn't declared", skipping either side being
            // Mixed avoids flagging the overwhelmingly common case at
            // the cost of missing the rare genuine `mixed`-narrows-a-
            // real-type case - the same "don't guess wrong on Mixed"
            // tradeoff `accepts` makes elsewhere in this project.
            //
            // Checked at the native level only (`ty_native`, ignoring
            // any `@param` docblock refinement on either side) - real
            // PHP itself only ever enforces override-compatibility on
            // the native signature, and by default so does real PHPStan
            // (`checkPhpDocMethodSignatures` is off unless a project
            // explicitly opts in). Using the merged, docblock-aware `ty`
            // here would wrongly flag the common Rector/PHPStan-
            // `Collector<T>` idiom of narrowing a parameter via `@param`
            // alone while leaving a wide native hint unchanged, as an
            // LSP violation real PHPStan doesn't, by default, check at
            // all.
            if own_param.ty_native().is_mixed() || ancestor_param.ty_native().is_mixed() {
                continue;
            }
            // A bare `self`/`static` in either signature means "an
            // instance of the class that actually declared it," not
            // some shared, comparable-by-name sentinel - `self` in
            // `fqn`'s override and `self` in `ancestor_owner`'s
            // declaration usually name two different concrete classes.
            // `accepts`/`is_super_type_of` have no rule for a concrete
            // `Object` satisfying a still-bare `SelfRef` (only the
            // reverse - `self` satisfying a concrete class - is
            // handled), so resolving both sides first, same as any other
            // pseudo-type use, avoids flagging a real, correct override
            // typing its own parameter `self` as narrowing what's
            // actually the exact same type.
            //
            // Exception: a trait-sourced ancestor's own `self` means
            // "whatever class ends up using this trait," never the
            // trait itself, which can't be instantiated - so it's
            // resolved against `fqn` (the using class actually being
            // checked) too, not `ancestor_owner` (the trait's own name),
            // mirroring `MethodPrototypeFinder::findPrototype`'s trait
            // branch, which builds its returned prototype via
            // `createUserlandMethodReflection($trait, $classReflection,
            // ...)`, passing the using `$classReflection` as context,
            // not the trait.
            let own_ty = self.resolve_pseudo(own_param.ty_native(), fqn);
            let ancestor_pseudo_owner = if check_visibility {
                &ancestor_owner
            } else {
                fqn
            };
            let ancestor_ty =
                self.resolve_pseudo(ancestor_param.ty_native(), ancestor_pseudo_owner);
            if !self.accepts(&own_ty, &ancestor_ty).yes() {
                self.report(format!(
                        "{fqn}::{method_name}() parameter #{} (${}) narrows the inherited parameter type: expects {}, ancestor expects {}",
                        i + 1,
                        own_param.name(),
                        own_ty,
                        ancestor_ty
                    ), span, 0, "method.childParameterType");
            }
        }
        // An ancestor's `static`-typed return (`self.registry` has no
        // way to resolve what `static` meant relative to the ancestor's
        // declaring class, only the checker's current-class-aware
        // `resolve_pseudo`, which isn't available here) is real PHP's
        // explicit "late static binding" covariance escape hatch - a
        // subclass narrowing it to its own concrete class name is always
        // exactly what real PHP allows, never a real incompatibility.
        // Checked at the native level only (ignoring any `@return`
        // docblock refinement on either side) - real PHP itself only
        // ever enforces override-compatibility on the native signature,
        // and by default so does real PHPStan
        // (`checkPhpDocMethodSignatures` is off unless a project
        // explicitly opts in). Using the merged, docblock-aware
        // `return_type` here would wrongly flag every override that
        // faithfully keeps the ancestor's native `array` while the
        // ancestor's interface had separately narrowed it to `list<...>`
        // via docblock alone.
        //
        // A built-in ancestor's `#[TentativeType]` return (PHP 8.1+)
        // replaces that check with `method.tentativeReturnType`, where a
        // missing native return type counts as `mixed`, and
        // `#[\ReturnTypeWillChange]` on either side opts out.
        let tentative = ancestor_sig.is_stub()
            && has_attr(ancestor_sig.attributes(), "TentativeType")
            && self
                .registry
                .php_supports_somewhere(types::php_version::PhpFeature::TentativeReturnTypes)
                != Some(false);
        if tentative {
            let will_change = has_attr(own_sig.attributes(), "ReturnTypeWillChange")
                || has_attr(ancestor_sig.attributes(), "ReturnTypeWillChange");
            let ancestor_ty = ancestor_sig.return_type_native();
            let own_ty = own_sig.return_type_native();
            // The child's own `self`/`static` is its own class, which a
            // built-in ancestor's class-named hint accepts.
            let compatible = ancestor_ty.is_mixed()
                || ancestor_return_uses_static(ancestor_ty)
                || (!own_ty.is_implicit_mixed()
                    && self
                        .accepts(ancestor_ty, &self.resolve_pseudo(own_ty, fqn))
                        .yes());
            if !will_change && !compatible {
                let own_display = if own_ty.is_implicit_mixed() {
                    "mixed".to_string()
                } else {
                    own_ty.to_string()
                };
                self.report_builder(
                    DiagnosticBuilder::new(format!(
                            "Return type {own_display} of method {fqn}::{method_name}() is not covariant with tentative return type {ancestor_ty} of method {ancestor_owner}::{method_name}()."
                        ),
                        span,
                        0,
                        "method.tentativeReturnType",
                    )
                    .tip("Make it covariant, or use the #[\\ReturnTypeWillChange] attribute to temporarily suppress the error.")
                    .non_ignorable(),
                );
            }
            return;
        }
        // `self` is resolved on both sides as for parameters above: an
        // interface's `: self` is that interface, which an implementation
        // returning the interface by name satisfies.
        let ancestor_pseudo_owner = if check_visibility {
            &ancestor_owner
        } else {
            fqn
        };
        if !own_sig.return_type_native().is_mixed()
            && !ancestor_sig.return_type_native().is_mixed()
            && !ancestor_return_uses_static(ancestor_sig.return_type_native())
            && !self
                .accepts(
                    &self.resolve_pseudo(ancestor_sig.return_type_native(), ancestor_pseudo_owner),
                    &self.resolve_pseudo(own_sig.return_type_native(), fqn),
                )
                .yes()
        {
            self.report(format!(
                    "{fqn}::{method_name}() return type {} is not compatible with the inherited return type {}",
                    own_sig.return_type(), ancestor_sig.return_type()
                ), span, 0, "method.childReturnType");
        }
    }
}

/// Whether `attrs` (raw, as-written attribute names) contains `name`,
/// ignoring any namespace qualification.
/// Spans of a method's own textual declaration.
#[derive(Clone, Copy)]
struct DeclSpans {
    name: Span,
    override_removal: Option<Span>,
}

fn has_attr(attributes: &[crate::registry::ReflectionAttribute], name: &str) -> bool {
    attributes
        .iter()
        .any(|a| a.short_name().eq_ignore_ascii_case(name))
}

/// Whether an ancestor's native return type invokes the `static`
/// covariance escape hatch documented above `check_method_overrides`'s
/// return-type check - either bare (`copy(): static`) or nullable
/// (`tryFrom(): ?static`). `resolve_type` builds `?static` as a plain
/// two-member `Union` with `StaticRef`, never its own dedicated type, so
/// a bare `StaticRef` check misses it entirely.
fn ancestor_return_uses_static(ty: &Type) -> bool {
    ty.union_members()
        .iter()
        .any(Type::is_late_static_reference)
}
