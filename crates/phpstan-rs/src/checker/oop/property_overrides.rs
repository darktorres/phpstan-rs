use crate::checker::Checker;
use crate::checker::oop::visibility_word;
use crate::diagnostic::DiagnosticBuilder;
use crate::registry::PropertySig;
use syntax::ast::*;
use syntax::token::Span;
use types::php_version::PhpFeature;

impl<'a> Checker<'a> {
    /// Mirrors real PHPStan's `OverridingPropertyRule`: the mismatch
    /// kinds answerable with fields `PropertyDecl`/`PropertySig` already
    /// carry (`is_static`/`is_readonly`/`visibility` - all three real,
    /// unconditional fatal errors in whichever direction is checked
    /// below; the other direction of each is real, legal PHP and
    /// deliberately left unflagged), plus native-type-variance
    /// (`property.nativeType`/`.missingNativeType`/`.extraNativeType` -
    /// real PHP requires a property's type to match its ancestor's
    /// exactly, invariantly, unlike a method's covariant return type),
    /// checked at the native level via `PropertySig::ty_native`/
    /// `has_native_type` (mirroring `ParamSig`'s identical split - see
    /// `check_property_native_type_override`'s doc comment for why a
    /// plain, already-docblock-merged `ty` comparison would risk the
    /// identical false-positive class `method.childReturnType`'s doc
    /// comment found and fixed for methods). A promoted constructor
    /// property is a real property for override purposes too - checked
    /// via the shared `check_one_property_override` helper below, once
    /// per plain declaration and once per promoted `__construct`
    /// parameter. A promoted property is never `static` (no such syntax
    /// exists), which `own_sig.is_static: false` encodes directly.
    pub(crate) fn check_property_overrides(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for p in &c.properties {
            let Some(own_sig) = info.properties.get(p.name.as_ref()).cloned() else {
                continue;
            };
            self.check_property_override_attribute(fqn, &p.name, &p.attrs, p.span);
            self.check_one_property_override(fqn, &p.name, &own_sig, p.span);
        }
        for m in &c.methods {
            if m.function.name != "__construct".into() {
                continue;
            }
            for param in &m.function.params {
                if param.promoted_visibility.is_none() {
                    continue;
                }
                let Some(own_sig) = info.properties.get(param.name.as_ref()).cloned() else {
                    continue;
                };
                self.check_property_override_attribute(fqn, &param.name, &param.attrs, param.span);
                self.check_one_property_override(fqn, &param.name, &own_sig, param.span);
            }
        }
    }

    /// Mirrors `OverridingPropertyRule`'s `hasOverrideAttribute`/
    /// `filterOverrideAttribute` half for properties - the method-level
    /// equivalent already exists (`FunctionSig::has_override_attr`,
    /// `check_one_method_override`); a property needs no analogous
    /// registry field, since (unlike a method) it's never hoisted
    /// through a separate trait-merge lookup path before this check
    /// runs - `p.attrs`/`param.attrs` on the class's own textual
    /// declaration is already the complete, real answer. A private
    /// ancestor property doesn't count as a real override target here,
    /// mirroring `check_one_property_override`'s identical exemption
    /// below (an unrelated declaration, not something `#[Override]`
    /// could meaningfully claim to override).
    fn check_property_override_attribute(
        &mut self,
        fqn: &str,
        name: &str,
        attrs: &[Attribute],
        span: Span,
    ) {
        let Some(override_attr) = attrs
            .iter()
            .find(|a| crate::registry::is_override_attribute_name(&a.name))
        else {
            return;
        };
        // `#[\Override]` on a property is only real PHP syntax from 8.5
        // on, mirroring real PHPStan's `PropertyAttributesRule` - below
        // that, real PHPStan flags this unconditionally, independent of
        // whether the property would otherwise be a real, matching
        // override. Unlike this project's usual "no version configured,
        // don't check" convention, this still runs both checks
        // regardless - the two conditions are genuinely orthogonal (a
        // real ancestor mismatch is still a real ancestor mismatch on
        // any PHP version), matching real PHPStan's two independent
        // rules, which can both fire on the same property at once.
        if let Some(version) = self.registry.php_version
            && !PhpFeature::OverrideAttributeOnProperty.is_available(version)
        {
            self.report_builder(
                DiagnosticBuilder::new("Attribute class Override can be used with properties only on PHP 8.5 and later."
                        .to_string(),
                    span,
                    0,
                    "property.overrideAttribute",
                )
                .non_ignorable(),
            );
        }
        if self.registry.has_unresolvable_ancestor(fqn) {
            return;
        }
        let has_real_ancestor = self
            .registry
            .lookup_ancestor_property_with_owner(fqn, name)
            .is_some_and(|(_, sig)| sig.visibility != Visibility::Private);
        if !has_real_ancestor {
            self.report_builder(
                DiagnosticBuilder::new(format!(
                        "Property {fqn}::${name} has #[\\Override] attribute, but no matching parent property exists"
                    ),
                    span,
                    0,
                    "property.override",
                )
                .fix(override_attr.removal_span, "", "Remove #[\\Override]"),
            );
        }
    }

    fn check_one_property_override(
        &mut self,
        fqn: &str,
        name: &str,
        own_sig: &PropertySig,
        span: Span,
    ) {
        // Overriding a `final` property is a real, unconditional PHP
        // fatal error (PHP 8.4+) - same "runs before every other
        // exemption below" ordering `check_one_method_override`'s
        // final-ancestor check uses. `is_by_phpdoc` is `@final`'s
        // docblock-only sibling - see `check_one_method_override`'s
        // identical comment for why the two need different wording/
        // identifier/`nonIgnorable()`-ness.
        if let Some((ancestor, is_by_phpdoc)) =
            self.registry.find_final_ancestor_property(fqn, name)
        {
            let (verb, identifier) = if is_by_phpdoc {
                ("@final", "property.parentPropertyFinalByPhpDoc")
            } else {
                ("final", "property.parentPropertyFinal")
            };
            let message =
                format!("Property {fqn}::${name} overrides {verb} property {ancestor}::${name}.");
            if is_by_phpdoc {
                self.report(message, span, 0, identifier);
            } else {
                self.report_builder(
                    DiagnosticBuilder::new(message, span, 0, identifier).non_ignorable(),
                );
            }
        }
        let Some((ancestor_owner, ancestor_sig)) =
            self.registry.lookup_ancestor_property_with_owner(fqn, name)
        else {
            return;
        };
        // A private ancestor property is an unrelated declaration, not
        // a real override target - same reasoning as
        // `check_method_overrides`'s identical exemption.
        if ancestor_sig.visibility == Visibility::Private {
            return;
        }
        if ancestor_sig.is_static && !own_sig.is_static {
            self.report(format!(
                    "Non-static property {fqn}::${name} overrides static property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.nonStatic",
            );
        } else if !ancestor_sig.is_static && own_sig.is_static {
            self.report(format!(
                    "Static property {fqn}::${name} overrides non-static property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.static",
            );
        }
        if ancestor_sig.is_readonly && !own_sig.is_readonly {
            self.report(format!(
                    "Readwrite property {fqn}::${name} overrides readonly property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.readWrite",
            );
        } else if !ancestor_sig.is_readonly && own_sig.is_readonly {
            self.report(format!(
                    "Readonly property {fqn}::${name} overrides readwrite property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.readOnly",
            );
        }
        // Same two-way visibility-narrowing split `check_method_overrides`
        // uses, one member kind over.
        let narrows_visibility = match ancestor_sig.visibility {
            Visibility::Public => own_sig.visibility != Visibility::Public,
            Visibility::Protected => own_sig.visibility == Visibility::Private,
            Visibility::Private => false,
        };
        if narrows_visibility {
            self.report(format!(
                    "{fqn}::${name} overriding {} property {ancestor_owner}::${name} must not reduce visibility",
                    visibility_word(&ancestor_sig.visibility)
                ),
                span,
                0,
                "property.visibility",
            );
        }
        self.check_property_hook_override(fqn, name, own_sig, &ancestor_owner, ancestor_sig, span);
        self.check_property_native_type_override(
            fqn,
            name,
            own_sig,
            &ancestor_owner,
            ancestor_sig,
            span,
        );
    }

    /// `OverridingPropertyRule`'s native-type-variance half
    /// (`property.nativeType`/`.missingNativeType`/`.extraNativeType`),
    /// via `PropertySig::ty_native`/`has_native_type` (mirroring
    /// `ParamSig`'s identical split). Checked purely at the native
    /// level, exactly like every other override-compatibility check in
    /// this file and `method_overrides.rs`, since real PHP itself only
    /// ever enforces this invariantly against the native type, never the
    /// PHPDoc-merged one. Deliberately doesn't attempt real PHPStan's
    /// further hook-driven contravariant/covariant refinement (only
    /// reachable when a property is virtual/abstract and asymmetrically
    /// readable-xor-writable), since this project's hook-variance model
    /// doesn't track per-direction native types precisely enough to draw
    /// that distinction safely, so every mismatch here uses the same
    /// "must be exactly equal" rule real PHPStan's plain (non-hook)
    /// branch uses. Equality is checked bidirectionally through
    /// `accepts` (not raw `Type` `PartialEq`, which is sensitive
    /// to union-member order), the same shape `resolve_pseudo`'d
    /// comparisons throughout this checker use for two types that must
    /// genuinely match.
    fn check_property_native_type_override(
        &mut self,
        fqn: &str,
        name: &str,
        own_sig: &PropertySig,
        ancestor_owner: &str,
        ancestor_sig: &PropertySig,
        span: Span,
    ) {
        if !ancestor_sig.has_native_type {
            if own_sig.has_native_type {
                let own_ty = self.resolve_pseudo(&own_sig.ty_native, fqn);
                self.report_builder(
                    DiagnosticBuilder::new(format!(
                            "Property {fqn}::${name} ({own_ty}) overriding property {ancestor_owner}::${name} should not have a native type."
                        ),
                        span,
                        0,
                        "property.extraNativeType",
                    )
                    .non_ignorable(),
                );
            }
            return;
        }
        let ancestor_ty = self.resolve_pseudo(&ancestor_sig.ty_native, ancestor_owner);
        if !own_sig.has_native_type {
            self.report_builder(
                DiagnosticBuilder::new(format!(
                        "Property {fqn}::${name} overriding property {ancestor_owner}::${name} ({ancestor_ty}) should also have native type {ancestor_ty}."
                    ),
                    span,
                    0,
                    "property.missingNativeType",
                )
                .non_ignorable(),
            );
            return;
        }
        let own_ty = self.resolve_pseudo(&own_sig.ty_native, fqn);
        if self.registry.accepts(&ancestor_ty, &own_ty).yes()
            && self.registry.accepts(&own_ty, &ancestor_ty).yes()
        {
            return;
        }
        self.report_builder(
            DiagnosticBuilder::new(format!(
                    "Type {own_ty} of property {fqn}::${name} is not the same as type {ancestor_ty} of overridden property {ancestor_owner}::${name}."
                ),
                span,
                0,
                "property.nativeType",
            )
            .non_ignorable(),
        );
    }

    /// `OverridingPropertyRule`'s property-hook half -
    /// `notReadable`/`notWritable`/`abstractOverridingNonAbstractHook`.
    /// Gated on `ancestor_sig.is_virtual || own_sig.is_virtual` first -
    /// an ordinary, non-hooked property on either side is unconditionally
    /// both readable and writable, so nothing here applies to the vast
    /// majority of properties.
    fn check_property_hook_override(
        &mut self,
        fqn: &str,
        name: &str,
        own_sig: &PropertySig,
        ancestor_owner: &str,
        ancestor_sig: &PropertySig,
        span: Span,
    ) {
        if !ancestor_sig.is_virtual && !own_sig.is_virtual {
            return;
        }
        let ancestor_readable = !ancestor_sig.is_virtual || ancestor_sig.has_get_hook;
        let own_readable = !own_sig.is_virtual || own_sig.has_get_hook;
        if ancestor_readable && !own_readable {
            self.report(format!(
                    "Property {fqn}::${name} does not have `get` hook and is not readable, but is required to be readable by property {ancestor_owner}::${name}."
                ),
                span,
                0,
                "property.notReadable",
            );
        }
        let ancestor_writable = !ancestor_sig.is_virtual || ancestor_sig.has_set_hook;
        let own_writable = !own_sig.is_virtual || own_sig.has_set_hook;
        if ancestor_writable && !own_writable {
            self.report(format!(
                    "Property {fqn}::${name} does not have `set` hook and is not writable, but is required to be writable by property {ancestor_owner}::${name}."
                ),
                span,
                0,
                "property.notWritable",
            );
        }
        // A hook only makes sense to compare "abstract-ness" for when
        // both sides actually declare that same hook type at all -
        // real PHP's own rule only fires on that exact overlap.
        if own_sig.is_abstract && !ancestor_sig.is_abstract {
            if own_sig.has_get_hook && ancestor_sig.has_get_hook {
                self.report(format!(
                        "Cannot make non abstract method {ancestor_owner}::${name}::get() abstract in class {fqn}"
                    ),
                    span,
                    0,
                    "property.abstractOverridingNonAbstractHook",
                );
            }
            if own_sig.has_set_hook && ancestor_sig.has_set_hook {
                self.report(format!(
                        "Cannot make non abstract method {ancestor_owner}::${name}::set() abstract in class {fqn}"
                    ),
                    span,
                    0,
                    "property.abstractOverridingNonAbstractHook",
                );
            }
        }
    }
}
