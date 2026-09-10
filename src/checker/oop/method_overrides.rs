use crate::ast::*;
use crate::checker::Checker;
use crate::checker::oop::visibility_word;
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
        // Finding 46: overriding a `final` method is a real,
        // unconditional fatal error in PHP - including for __construct,
        // unlike the signature-compatibility checks below, so this runs
        // before that exemption.
        if let Some(ancestor) = self.registry.find_final_ancestor_method(fqn, method_name) {
            self.report(
                fqn.to_string(),
                format!(
                    "{fqn}::{method_name}() cannot override final method {ancestor}::{method_name}()"
                ),
                span,
                0,
                "method.overrideFinal",
            );
        }
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let Some(own_sig) = info.methods.get(method_name) else {
            return;
        };
        let ancestor = self
            .registry
            .lookup_ancestor_method_with_owner(fqn, method_name);
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
            .is_some_and(|(_, sig)| sig.visibility != Visibility::Private)
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
            self.registry.find_consistent_constructor_ancestor(fqn)
        } else {
            ancestor
        };
        let Some((ancestor_owner, ancestor_sig)) = compatibility_ancestor else {
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
        // to `private`).
        let narrows_visibility = match ancestor_sig.visibility {
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
            let own_ty = self.resolve_pseudo(&own_param.ty_native, fqn);
            let ancestor_ty = self.resolve_pseudo(&ancestor_param.ty_native, &ancestor_owner);
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
