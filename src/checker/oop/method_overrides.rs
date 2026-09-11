use crate::ast::*;
use crate::checker::oop::visibility_word;
use crate::checker::{Checker, DiagnosticBuilder};
use crate::token::Span;
use crate::types::Type;

impl<'a> Checker<'a> {
    pub(crate) fn check_method_overrides(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for m in &c.methods {
            self.check_one_method_override(fqn, &m.function.name, &m.visibility, m.span);
        }
        // A method a trait supplies but the class itself never
        // textually re-declares still needs the identical override-
        // compatibility checking against any ancestor interface/parent -
        // real PHP performs Liskov compatibility checking after trait
        // flattening, not just against a class's own directly-declared
        // methods (confirmed directly: `trait T { function foo(string
        // $x) {} }` used by a class implementing `Iface { function
        // foo(int $x): void; }` is a real, unconditional fatal error even
        // though the class itself never mentions `foo` at all). No AST
        // node exists for a trait-only method within *this* class's own
        // declaration to anchor a precise span on (real PHP itself points
        // at the trait's own declaration line, in the trait's own file -
        // information `FunctionSig` doesn't carry), so `c.span` (the
        // using class's own declaration) is used instead, matching this
        // project's established fallback for class-level diagnostics with
        // no more specific member location (`check_class_completeness`,
        // `check_trait_conflicts`, ...).
        if !c.uses.is_empty() {
            let own_method_names: std::collections::HashSet<&str> =
                c.methods.iter().map(|m| m.function.name.as_str()).collect();
            let mut trait_method_names: Vec<String> = info
                .methods
                .keys()
                .filter(|name| {
                    name.as_str() != "__construct"
                        && !own_method_names.contains(name.as_str())
                        && !info.abstract_methods.contains(*name)
                })
                .cloned()
                .collect();
            trait_method_names.sort();
            for name in trait_method_names {
                let Some(own_visibility) = info.methods.get(&name).map(|s| s.visibility.clone())
                else {
                    continue;
                };
                self.check_one_method_override(fqn, &name, &own_visibility, c.span);
            }
        }
    }

    /// Phase 42: `MethodThrowTypeCovarianceRule` (`vendor/phpstan-src/
    /// src/Rules/Exceptions/MethodThrowTypeCovarianceRule.php`, confirmed
    /// by direct read) - opt-in (`Registry::check_method_throw_type_
    /// covariance`, see its own doc comment for why this project ships
    /// it opt-in unlike real PHPStan's own unconditional registration). A
    /// pure declaration-vs-declaration comparison: this method's own
    /// `@throws` (or, absent one, an implicit `Throwable` - real
    /// PHPStan's own default `exceptions.implicitThrows: true`, hardcoded
    /// here rather than separately configurable) must be covariant with
    /// every immediate parent class/transitive interface/abstract trait
    /// declaration's own `@throws` (same implicit-`Throwable` fallback).
    /// No call-graph-wide implicit-throw model is needed for this - real
    /// PHPStan's own default already treats an untagged declaration as
    /// "throws Throwable" rather than needing to trace what it actually,
    /// transitively throws, which is exactly what made the earlier
    /// decline's premise ("no such model exists") not apply here.
    fn check_method_throw_type_covariance(
        &mut self,
        fqn: &str,
        method_name: &str,
        own_visibility: &Visibility,
        span: Span,
    ) {
        if !self.registry.check_method_throw_type_covariance {
            return;
        }
        if method_name == "__construct" || *own_visibility == Visibility::Private {
            return;
        }
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let Some(own_sig) = info.methods.get(method_name) else {
            return;
        };
        let throwable = || Type::Object("Throwable".to_string(), Vec::new());
        let own_throws = own_sig.throws.clone().unwrap_or_else(throwable);
        if own_throws == Type::Void {
            return;
        }

        // `ParentMethodHelper::collectParentMethods`, transcribed: the
        // immediate parent class's own native declaration (not the full
        // `extends` chain - each class in a longer chain runs this same
        // check against only its own immediate parent, so a multi-level
        // chain still gets fully covered, one link at a time), every
        // transitive interface declaring this method, and every used
        // trait's own *abstract* declaration of it (a trait's concrete
        // contribution becomes the class's own effective implementation,
        // not something to compare the class against).
        let mut ancestors: Vec<(String, Type)> = Vec::new();
        if let Some(parent) = info.extends.first()
            && let Some(parent_sig) = self
                .registry
                .classes
                .get(parent)
                .and_then(|p| p.methods.get(method_name))
            && parent_sig.visibility != Visibility::Private
        {
            ancestors.push((
                parent.clone(),
                parent_sig.throws.clone().unwrap_or_else(throwable),
            ));
        }
        for (owner, sig) in self
            .registry
            .all_interfaces_declaring_method(fqn, method_name)
        {
            ancestors.push((owner, sig.throws.clone().unwrap_or_else(throwable)));
        }
        for used in &info.uses {
            if let Some(trait_info) = self.registry.classes.get(used)
                && trait_info.abstract_methods.contains(method_name)
                && let Some(trait_sig) = trait_info.methods.get(method_name)
            {
                ancestors.push((
                    used.clone(),
                    trait_sig.throws.clone().unwrap_or_else(throwable),
                ));
            }
        }

        for (owner, parent_throws) in ancestors {
            if parent_throws == Type::Void {
                self.report(
                    fqn.to_string(),
                    format!(
                        "Method {fqn}::{method_name}() should not throw {own_throws} because parent method {owner}::{method_name}() has PHPDoc tag @throws void."
                    ),
                    span,
                    2,
                    "throws.shouldBeVoid",
                );
                continue;
            }
            if self.registry.is_assignable(&own_throws, &parent_throws) {
                continue;
            }
            self.report(
                fqn.to_string(),
                format!(
                    "PHPDoc tag @throws type {own_throws} of method {fqn}::{method_name}() should be covariant with PHPDoc @throws type {parent_throws} of method {owner}::{method_name}()."
                ),
                span,
                2,
                "throws.notCovariant",
            );
        }
    }

    /// The full per-method override-compatibility check, shared between a
    /// class's own textually-declared methods and a trait-supplied method
    /// it never re-declares (see `check_method_overrides` above) - the
    /// two differ only in which visibility/span they can offer.
    fn check_one_method_override(
        &mut self,
        fqn: &str,
        method_name: &str,
        own_visibility: &Visibility,
        span: Span,
    ) {
        self.check_method_throw_type_covariance(fqn, method_name, own_visibility, span);
        // Finding 46: overriding a `final` method is a real,
        // unconditional fatal error in PHP - including for __construct,
        // unlike the signature-compatibility checks below, so this runs
        // before that exemption. `is_by_phpdoc` is `@final`'s own
        // docblock-only sibling (a purely static-analysis convention real
        // PHP itself never enforces) - real PHPStan's own message
        // wording/identifier/`nonIgnorable()`-ness genuinely differ
        // between the two (`method.parentMethodFinal`, unconditionally
        // non-ignorable, vs. `method.parentMethodFinalByPhpDoc`,
        // ordinarily suppressible), confirmed by direct read of
        // `OverridingMethodRule.php`.
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
                self.report(fqn.to_string(), message, span, 0, identifier);
            } else {
                self.report_builder(
                    DiagnosticBuilder::new(fqn.to_string(), message, span, 0, identifier)
                        .non_ignorable(),
                );
            }
        }
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let Some(own_sig) = info.methods.get(method_name) else {
            return;
        };
        // Phase 64 Item 4: real PHPStan's own `MethodPrototypeFinder`
        // priority order (immediately-implemented interface, then a used
        // trait's own abstract declaration, then the plain `extends`
        // chain) - not the old, wrong `extends`-before-`implements` order
        // `lookup_ancestor_method_with_owner` still uses for its other,
        // non-override-compatibility callers.
        let ancestor = self
            .registry
            .find_method_signature_prototype(fqn, method_name);
        // Phase 64 follow-up: the unconditional-fatal-error half of PHP
        // 8.3's `#[\Override]` attribute (confirmed directly via
        // `php -r`: `Child::foo() has #[\Override] attribute, but no
        // matching parent method exists`) - real PHPStan's own
        // `method.override` identifier
        // (`OverridingMethodRule::hasOverrideAttribute`/`processNode`).
        // The *other* direction (`method.missingOverride` - a real
        // override missing the attribute) is a real PHPStan opt-in rule
        // gated behind `checkMissingOverrideMethodAttribute`, off by
        // default and not enabled anywhere in this project's own corpus
        // config - not attempted here. A private ancestor method doesn't
        // count as "a matching parent method" either (confirmed
        // directly) - same `has_real_ancestor` check the signature-
        // compatibility exemption just below also needs. A *trait's own*
        // declaration is exempted entirely (`info.kind`, not just
        // "ancestor not found") - real PHP defers this whole check to
        // whichever class eventually `use`s the trait, since a trait has
        // no ancestor chain of its own to check against at all
        // (confirmed directly: the identical trait method fatals when
        // the using class lacks a real ancestor, but is completely fine
        // when it has one - and the fatal error is attributed to the
        // *using class's* own method name, not the trait's, which is
        // exactly what running this same check from both of
        // `check_method_overrides`'s own call sites above, keyed on the
        // using class's `fqn`, already produces for free). Implementing a
        // *trait's own abstract* method also counts as a real override
        // (confirmed directly, and a genuine corpus false positive:
        // `trait_declares_abstract_method` - deliberately not folded into
        // the general `ancestor` lookup above, which stays extends/
        // implements only for every other check in this function).
        // Likewise, an unresolved name anywhere in `fqn`'s own extends/
        // implements chain (`has_unresolvable_ancestor` - a dependency
        // this project's own registry has no declaration for at all)
        // means "no matching parent method" can't be confidently claimed
        // - another genuine corpus false positive, "don't guess wrong"
        // the same way elsewhere in this project.
        let has_real_ancestor = ancestor
            .as_ref()
            .is_some_and(|(_, sig, _)| sig.visibility != Visibility::Private)
            || self
                .registry
                .trait_declares_abstract_method(fqn, method_name);
        if own_sig.has_override_attr
            && !has_real_ancestor
            && info.kind != ClassKind::Trait
            && !self.registry.has_unresolvable_ancestor(fqn)
        {
            self.report(
                fqn.to_string(),
                format!(
                    "{fqn}::{method_name}() has #[\\Override] attribute but does not override any method"
                ),
                span,
                0,
                "method.override",
            );
        }
        // Phase 64: `method.nonStatic`/`method.static` - real PHPStan's
        // `OverridingMethodRule` requires an override to match its
        // ancestor's own static-ness exactly, either direction
        // (nonIgnorable, a real unconditional PHP fatal error either
        // way). Uses `ancestor` (the plain, non-`__construct`-adjusted
        // prototype), not `compatibility_ancestor` below - static-ness
        // isn't part of the parameter-signature exemption constructors
        // get, so a constructor still needs checking against its own
        // real prototype like any other method.
        if let Some((ancestor_owner, ancestor_sig, _)) = &ancestor
            && ancestor_sig.visibility != Visibility::Private
        {
            let own_is_static = info.static_methods.contains(method_name);
            let ancestor_is_static = self
                .registry
                .classes
                .get(ancestor_owner)
                .is_some_and(|i| i.static_methods.contains(method_name));
            if ancestor_is_static && !own_is_static {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
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
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
                            "Static method {fqn}::{method_name}() overrides non-static method {ancestor_owner}::{method_name}()"
                        ),
                        span,
                        0,
                        "method.static",
                    )
                    .non_ignorable(),
                );
            }
            // Phase 64: the general-case (non-`__construct`-special-path)
            // half of `method.abstractOverridingNonAbstract` - making a
            // non-abstract ancestor method abstract in the override. Real
            // PHPStan's own rule has a second, `__construct`-specific
            // branch too (triggered only when no *normal* prototype
            // resolves via the usual interface/trait/`extends` walk at
            // all) - a distinct, deliberately unbuilt code path, not a
            // narrowing of this one. An interface's own method is always
            // implicitly abstract (PHP never requires - or even allows -
            // the literal `abstract` keyword there), unlike
            // `static_methods` above, where interface methods use the
            // same explicit-keyword convention as any other class.
            let own_is_abstract = info.abstract_methods.contains(method_name);
            let ancestor_is_abstract = self.registry.classes.get(ancestor_owner).is_some_and(|i| {
                i.kind == ClassKind::Interface || i.abstract_methods.contains(method_name)
            });
            if own_is_abstract && !ancestor_is_abstract {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
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
        // *signature-compatibility* checking below - a subclass's own
        // __construct is allowed an entirely different, otherwise-
        // incompatible signature from its parent's, unlike every other
        // method. The `#[\Override]` check above is unaffected by this -
        // confirmed directly, `#[\Override]` on a `__construct()` with no
        // ancestor `__construct()` fatals exactly like any other method.
        //
        // `@phpstan-consistent-constructor` (`Registry::find_consistent_
        // constructor_ancestor`'s own doc comment) re-enables exactly this
        // exemption, but only against the nearest *tagged* ancestor's own
        // constructor, not the immediate parent's - a genuinely different
        // ancestor than `ancestor` above (which the `#[\Override]` check
        // needs to keep using regardless of any tag).
        let compatibility_ancestor = if method_name == "__construct" {
            self.registry
                .find_consistent_constructor_ancestor(fqn)
                .map(|(owner, sig)| (owner, sig, true))
        } else {
            ancestor
        };
        let Some((ancestor_owner, ancestor_sig, check_visibility)) = compatibility_ancestor else {
            return;
        };
        // A `private` ancestor method isn't a real override target at
        // all - real PHP's own method dispatch is never virtual for
        // `private` (a same-named method on a subclass is an unrelated
        // declaration, not a polymorphic override), so nothing about
        // compatibility applies. `lookup_ancestor_method_with_owner`
        // itself doesn't filter by visibility (it answers "is there a
        // same-named declaration up the chain", the right question for
        // the visibility-narrowing check just below, which needs to see
        // a `private` ancestor to *know* it's not a real override) -
        // this exemption belongs here, not there.
        if ancestor_sig.visibility == Visibility::Private {
            return;
        }
        // Phase 64: narrowing a method's own visibility on override is a
        // real, unconditional PHP fatal error (confirmed directly:
        // `Access level to Child::foo() must be public (as in class
        // Base)`) - matches real PHPStan's own
        // `MethodVisibilityComparisonHelper` exactly (same identifier,
        // same two-way split: an ancestor's `public` demands `public`;
        // an ancestor's `protected` only forbids narrowing all the way
        // to `private`). Skipped entirely when `!check_visibility`
        // (Phase 64 Item 4 follow-up: a trait-abstract-sourced prototype
        // only, per real `MethodPrototypeFinder::findPrototype`'s own
        // third tuple element) - confirmed live that real PHP itself
        // doesn't enforce this for a trait's own `abstract` declaration
        // the way it does for an interface/`extends` ancestor.
        let narrows_visibility = check_visibility
            && match ancestor_sig.visibility {
                Visibility::Public => *own_visibility != Visibility::Public,
                Visibility::Protected => *own_visibility == Visibility::Private,
                Visibility::Private => false,
            };
        if narrows_visibility {
            self.report(
                fqn.to_string(),
                format!(
                    "{fqn}::{method_name}() overriding {} method {ancestor_owner}::{method_name}() must not reduce visibility",
                    visibility_word(&ancestor_sig.visibility),
                ),
                span,
                0,
                "method.visibility",
            );
        }
        for (i, ancestor_param) in ancestor_sig.params.iter().enumerate() {
            let Some(own_param) = own_sig.params.get(i) else {
                // Real PHP fatal error - the override doesn't declare
                // this many parameters at all. Skipped for a variadic
                // ancestor tail (`...$args`, matched at any position, not
                // just the last - real PHP itself requires it to be the
                // last declared parameter, so an earlier one is already a
                // parse-time error this project's own parser presumably
                // rejects too): real PHPStan's own handling of that shape
                // is considerably more involved (contravariance across
                // every trailing ancestor parameter the variadic
                // swallows) than this project attempts here.
                if !ancestor_param.variadic {
                    self.report(
                        fqn.to_string(),
                        format!(
                            "{fqn}::{method_name}() overrides {ancestor_owner}::{method_name}() but misses parameter #{} (${})",
                            i + 1,
                            ancestor_param.name
                        ),
                        span,
                        0,
                        "parameter.missing",
                    );
                }
                continue;
            };
            // A parameter the ancestor made optional (has a default) can
            // never become *required* on override - a caller holding the
            // ancestor's own type could always omit it. Real,
            // unconditional PHP fatal error (confirmed directly). Not
            // gated on either side's type being `Mixed` the way the
            // narrowing check below is - this is about arity/callability,
            // not type compatibility.
            if ancestor_param.has_default && !own_param.has_default && !own_param.variadic {
                self.report(
                    fqn.to_string(),
                    format!(
                        "Parameter #{} (${}) of {fqn}::{method_name}() is required but parameter #{} (${}) of {ancestor_owner}::{method_name}() is optional",
                        i + 1,
                        own_param.name,
                        i + 1,
                        ancestor_param.name
                    ),
                    span,
                    0,
                    "parameter.notOptional",
                );
            }
            // Phase 64's own still-open list: by-ref/variadic-
            // contravariance mismatches - real PHPStan's own
            // `MethodParameterComparisonHelper` spends most of its own
            // ~250 lines on this cluster; only the two unconditional-
            // fatal-error shapes confirmed directly via `php -l` are
            // checked here, matching this project's own established
            // narrower scope elsewhere in this same function
            // (`parameter.missing` above explicitly skips the fuller
            // variadic-contravariance treatment too). Neither is gated on
            // either side being `Type::Mixed` - both are about calling-
            // convention compatibility, not type compatibility.
            if own_param.by_ref != ancestor_param.by_ref {
                let (message, identifier) = if own_param.by_ref {
                    (
                        format!(
                            "Parameter #{} (${}) of {fqn}::{method_name}() is passed by reference but parameter #{} (${}) of {ancestor_owner}::{method_name}() is not passed by reference",
                            i + 1,
                            own_param.name,
                            i + 1,
                            ancestor_param.name
                        ),
                        "parameter.byRef",
                    )
                } else {
                    (
                        format!(
                            "Parameter #{} (${}) of {fqn}::{method_name}() is not passed by reference but parameter #{} (${}) of {ancestor_owner}::{method_name}() is passed by reference",
                            i + 1,
                            own_param.name,
                            i + 1,
                            ancestor_param.name
                        ),
                        "parameter.notByRef",
                    )
                };
                self.report(fqn.to_string(), message, span, 0, identifier);
            }
            // The reverse direction (a non-variadic ancestor parameter
            // widened to variadic on override) is real, legal PHP -
            // confirmed directly - so only this direction is checked.
            if ancestor_param.variadic && !own_param.variadic {
                self.report(
                    fqn.to_string(),
                    format!(
                        "Parameter #{} (${}) of {fqn}::{method_name}() is not variadic but parameter #{} (${}) of {ancestor_owner}::{method_name}() is variadic",
                        i + 1, own_param.name,
                        i + 1, ancestor_param.name
                    ),
                    span,
                    0,
                    "parameter.notVariadic",
                );
            }
            // `Type::Mixed` is overloaded throughout this project to
            // mean both real PHP `mixed` *and* "no type declared at all,
            // native or docblock" (see this file's own opening comment)
            // - and real PHP explicitly does NOT enforce override
            // compatibility for a parameter/return position that isn't
            // typed at all (only an explicitly *narrower* declared type
            // is a real fatal error). Since an intentional, explicit
            // `mixed` on an override is rare next to "just wasn't
            // declared", skipping either side being Mixed avoids
            // flagging the overwhelmingly common case (confirmed via
            // vendor/phpstorm-stubs: dozens of real stub methods simply
            // don't redeclare an inherited native return type) at the
            // cost of missing the rare genuine `mixed`-narrows-a-real-
            // type case - the same "don't guess wrong on Mixed" tradeoff
            // `is_assignable` already makes elsewhere in this project.
            //
            // Checked at the *native* level only (`ty_native`, ignoring
            // any `@param` docblock refinement on either side) - real PHP
            // itself only ever enforces override-compatibility on the
            // native signature, and by default so does real PHPStan
            // (`checkPhpDocMethodSignatures` is off unless a project
            // explicitly opts in - confirmed neither Piwigo's own corpus
            // config nor this project's own does), the identical
            // reasoning the return-type check below already applies via
            // `return_type_native`. Using the merged, docblock-aware `ty`
            // here wrongly flagged the extremely common Rector/PHPStan-
            // `Collector<T>` idiom of narrowing a parameter via `@param`
            // alone while leaving a wide native hint unchanged
            // (`refactor(Node $node)`, narrowed only via `@param
            // SpecificNode $node`) as an LSP violation real PHPStan
            // doesn't, by default, check at all.
            if own_param.ty_native == Type::Mixed || ancestor_param.ty_native == Type::Mixed {
                continue;
            }
            // A bare `self`/`static` in either signature means "an
            // instance of the class that actually declared it", not some
            // shared, comparable-by-name sentinel - `self` in `fqn`'s own
            // override and `self` in `ancestor_owner`'s declaration
            // usually name two *different* concrete classes.
            // `is_assignable`/`is_subtype` have no rule at all for a
            // concrete `Object` satisfying a still-bare `SelfRef` (only
            // the reverse - `self` satisfying a concrete class - is
            // handled), so a real, correct override typing its own
            // parameter `self` (a `final` class implementing an interface
            // that (redundantly, but validly) re-declares the same
            // concrete class by name) was flagged as narrowing what's
            // actually the exact same type. Resolving both sides first,
            // same as any other pseudo-type use, fixes this without
            // weakening the check for a genuine narrowing.
            //
            // Exception: a trait-sourced ancestor's own `self` means
            // "whatever class ends up using this trait" - never the
            // trait itself, which can't be instantiated at all - so it's
            // resolved against `fqn` (the using class actually being
            // checked) too, not `ancestor_owner` (the trait's own name).
            // Confirmed directly against real PHPStan's own source:
            // `MethodPrototypeFinder::findPrototype`'s trait branch
            // builds its returned prototype via `createUserlandMethodReflection
            // ($trait, $classReflection, ...)`, passing the *using*
            // `$classReflection` as context, not the trait. Real corpus
            // false positive otherwise: Symfony's own `TransportResponseTrait
            // ::schedule(self $response, ...)`, implemented identically
            // by every one of its 4 users, each narrowing "the trait" to
            // their own concrete class - not a real narrowing at all.
            let own_ty = self.resolve_pseudo(&own_param.ty_native, fqn);
            let ancestor_pseudo_owner = if check_visibility {
                &ancestor_owner
            } else {
                fqn
            };
            let ancestor_ty = self.resolve_pseudo(&ancestor_param.ty_native, ancestor_pseudo_owner);
            if !self.registry.is_assignable(&ancestor_ty, &own_ty) {
                self.report(fqn.to_string(), format!(
                        "{fqn}::{method_name}() parameter #{} (${}) narrows the inherited parameter type: expects {}, ancestor expects {}",
                        i + 1,
                        own_param.name,
                        own_ty,
                        ancestor_ty
                    ), span, 0, "method.childParameterType");
            }
        }
        // An ancestor's own `static`-typed return (`self.registry` has
        // no way to resolve what `static` meant relative to the
        // *ancestor's* declaring class, only the checker's own current-
        // class-aware `resolve_pseudo`, which isn't available here) is
        // real PHP's own explicit "late static binding" covariance
        // escape hatch - a subclass narrowing it to its own concrete
        // class name (the overwhelmingly common real shape, confirmed
        // via vendor/phpstorm-stubs' `Ds\Collection::copy(): static` and
        // its own subclasses' `copy(): Vector`/`copy(): Stack`/...) is
        // always exactly what real PHP allows, never a real
        // incompatibility. Checked at the *native* level only (ignoring
        // any `@return` docblock refinement on either side) - real PHP
        // itself only ever enforces override-compatibility on the native
        // signature, and by default so does real PHPStan
        // (`checkPhpDocMethodSignatures` is off unless a project
        // explicitly opts in - confirmed neither Piwigo's own corpus
        // config nor this project's own does). Using the merged,
        // docblock-aware `return_type` here wrongly flagged every
        // override that faithfully keeps the ancestor's own native
        // `array` while the ancestor's *interface* had separately
        // narrowed it to `list<...>` via docblock alone - a real corpus
        // false positive (several `HtmlRenderingInterface` test fakes
        // each declaring `getCatBreadcrumb(): array`, exactly matching
        // the interface's own native signature).
        if own_sig.return_type_native != Type::Mixed
            && ancestor_sig.return_type_native != Type::Mixed
            && !ancestor_return_uses_static(&ancestor_sig.return_type_native)
            && !self.registry.is_assignable(
                &own_sig.return_type_native,
                &ancestor_sig.return_type_native,
            )
        {
            self.report(fqn.to_string(), format!(
                    "{fqn}::{method_name}() return type {} is not compatible with the inherited return type {}",
                    own_sig.return_type, ancestor_sig.return_type
                ), span, 0, "method.childReturnType");
        }
    }
}

/// Whether an ancestor's native return type invokes the `static`
/// covariance escape hatch documented above `check_method_overrides`'s own
/// return-type check - either bare (`copy(): static`) or nullable
/// (`tryFrom(): ?static`, the shape every `NumericId`-family value object's
/// interface actually uses). `resolve_type` builds `?static` as a plain
/// two-member `Union` with `StaticRef`, never its own dedicated type, so a
/// bare `!= Type::StaticRef` check misses it entirely.
fn ancestor_return_uses_static(ty: &Type) -> bool {
    match ty {
        Type::StaticRef => true,
        Type::Union(members) => members.contains(&Type::StaticRef),
        _ => false,
    }
}
