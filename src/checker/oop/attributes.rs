use std::collections::HashSet;

use crate::ast::*;
use crate::checker::{
    CheckArgsCall, Checker, DiagnosticBuilder, Locals, class_type_word, resolve_call_args,
};
use crate::php_version::PhpFeature;
use crate::registry::{is_allow_dynamic_properties_attribute_name, is_deprecated_attribute_name};
use crate::token::Span;

/// PHP's own `Attribute::TARGET_*` values (confirmed directly via
/// `php -r`, see `registry::class_info::attribute_target_constant_value`'s
/// identical table) - only the ones this project's own call sites for
/// `check_attribute_targets` need so far.
pub(crate) const ATTRIBUTE_TARGET_CLASS: u32 = 1;
pub(crate) const ATTRIBUTE_TARGET_FUNCTION: u32 = 2;
pub(crate) const ATTRIBUTE_TARGET_METHOD: u32 = 4;
pub(crate) const ATTRIBUTE_TARGET_PROPERTY: u32 = 8;
pub(crate) const ATTRIBUTE_TARGET_CLASS_CONSTANT: u32 = 16;
pub(crate) const ATTRIBUTE_TARGET_PARAMETER: u32 = 32;
pub(crate) const ATTRIBUTE_TARGET_CONSTANT: u32 = 64;
pub(crate) const ATTRIBUTE_IS_REPEATABLE: u32 = 128;

impl<'a> Checker<'a> {
    /// Phase 37: real PHPStan's own `AttributesCheck`
    /// (`vendor/phpstan-src/src/Rules/AttributesCheck.php`) - every
    /// `#[Name(...)]` group's `Name` must resolve to a real, known class,
    /// the same `attribute.notFound` identifier real PHPStan's own rule
    /// uses. Wired at every declaration site an `Attribute` list can now
    /// appear on (class/enum, method/function, property, const, enum
    /// case, param, closure/arrow-fn - see each call site).
    ///
    /// Phase 37 follow-up (this session): constructor-argument validation
    /// (`attribute.noConstructor`/`attribute.constructorNotPublic`/
    /// `attribute.abstract`/arity/type checks) - this phase's own prior
    /// write-up called this "genuinely needs Phase 49's call-argument-
    /// binding generalization," re-checked rather than trusted (this
    /// project's own now-standing practice, see ROADMAP.md's Phase 15
    /// entry for where it started): `resolve_call_args`/`check_args`
    /// (`checker/calls.rs`) already take a bare `args: &[Arg]` +
    /// `sig: &FunctionSig`, with no expr-node coupling at all - the exact
    /// same pair `ExprKind::New`'s own constructor-call handling already
    /// uses, directly reusable here with zero further generalization.
    /// Attribute arguments are real PHP constant-expressions (no local
    /// variable can ever appear in one), so a fresh, empty `Locals::
    /// default()` is exactly the right scope to check them against - not
    /// a shortcut, the correct binding for what real PHP itself allows
    /// there. `attribute.nonRepeatable`/`attribute.deprecated` aren't
    /// ported (the former needs a same-declaration-list accumulator, a
    /// genuinely different shape of check than this per-attribute loop;
    /// the latter would duplicate this project's own existing `@deprecated`
    /// family - see ROADMAP.md's Phase 37 entry for both, disclosed
    /// rather than silently dropped). Whether the resolved class is
    /// actually usable as an attribute at all, and whether its declared
    /// `Attribute::TARGET_*` flags match where this one is actually used,
    /// are checked too - see `check_attribute_targets` just below.
    pub(crate) fn check_attributes(&mut self, attrs: &[Attribute], label: &str) {
        for attr in attrs {
            let fqn = self.name_ctx.resolve(&attr.name);
            // Phase 37: `lookup_class` (Phase 48.1's case-insensitive
            // fallback), not a bare `self.registry.classes.get(&fqn)` -
            // without it, a case-mismatched attribute reference always
            // reported "does not exist" here before the new `check_class_
            // name_case` call just below ever got a chance to see it
            // (confirmed the hard way: a real, disclosed-scope-necessary
            // fix, not an unrelated improvement - `class.nameCase` on an
            // attribute reference is genuinely unreachable without this).
            // Every subsequent lookup/message in this function uses
            // `canonical` (the real declared case), not `fqn` (as-written,
            // possibly wrong-case) - correct either way, and avoids a
            // message naming the very case `check_class_name_case` is
            // simultaneously flagging as wrong.
            let Some((canonical, info)) = self.registry.lookup_class(&fqn) else {
                self.report(
                    label.to_string(),
                    format!("attribute class {fqn} does not exist"),
                    attr.span,
                    0,
                    "attribute.notFound",
                );
                continue;
            };
            let canonical = canonical.to_string();
            // Phase 37: attribute-reference `<kind>.nameCase` parity -
            // the already-shipped, already-corpus-verified helper, reused
            // as-is (real PHPStan's own `AttributesCheck` runs its
            // identical `ClassNameCheck::checkClassNames` call right here
            // too, immediately after confirming the name resolves).
            self.check_class_name_case(label, &attr.name, attr.span);
            if info.is_abstract {
                self.report(
                    label.to_string(),
                    format!("Attribute class {canonical} is abstract."),
                    attr.span,
                    0,
                    "attribute.abstract",
                );
            }
            // `lookup_method` (ancestor-chain-aware), not a bare
            // `info.methods.get("__construct")` - real corpus false
            // positive found via this session's own corpus diff:
            // PHPBench's `#[BeforeMethods]` (`class BeforeMethods extends
            // AbstractMethodsAttribute {}`) declares no `__construct` of
            // its own at all, inheriting its parent's - a bare own-class
            // lookup wrongly saw "no constructor" and misfired
            // `attribute.noConstructor` on every real, legal use.
            let ctor_sig = self.registry.lookup_method(&canonical, "__construct");
            let Some(ctor_sig) = ctor_sig else {
                if !attr.args.is_empty() {
                    self.report(
                        label.to_string(),
                        format!(
                            "Attribute class {canonical} does not have a constructor and must be instantiated without any parameters."
                        ),
                        attr.span,
                        0,
                        "attribute.noConstructor",
                    );
                }
                continue;
            };
            if ctor_sig.visibility != Visibility::Public {
                self.report(
                    label.to_string(),
                    format!("Constructor of attribute class {canonical} is not public."),
                    attr.span,
                    0,
                    "attribute.constructorNotPublic",
                );
            }
            let mut locals = Locals::default();
            for a in &attr.args {
                self.check_expr(&a.value, &mut locals, label);
            }
            let resolved = resolve_call_args(&attr.args, &ctor_sig.params);
            self.check_args(CheckArgsCall {
                callee_label: &format!("{canonical}::__construct"),
                sig: ctor_sig,
                args: &attr.args,
                resolved: &resolved,
                locals: &mut locals,
                label,
                defining_class: Some(&canonical),
                span: attr.span,
            });
        }
    }

    /// Phase 37 follow-up: the `attribute.notAttribute`/`attribute.target`
    /// halves of real PHPStan's own `AttributesCheck` - mirrors
    /// `registry::class_info::compute_attribute_targets`'s own doc
    /// comment for how a class's own allowed targets are computed. Called
    /// from every declaration kind attributes can appear on (class/enum,
    /// property, const, method/function, param, closure/arrow-fn,
    /// property hook - see each call site); an older version of this doc
    /// comment claimed several of those "still only get the plain
    /// existence check" - no longer true. A name the registry has no
    /// entry for at all is skipped here (the existence check above
    /// already reports it, and reporting *both* would be confusing
    /// double-counting for the identical typo).
    ///
    /// Also folds in `attribute.nonRepeatable` (an attribute class
    /// missing `Attribute::IS_REPEATABLE` used more than once in this
    /// exact `attrs` list - a `HashSet` scoped fresh per call, matching
    /// real PHPStan's own per-`check()` accumulator exactly) and
    /// `attribute.deprecated` (gated on `registry.check_deprecated_usage`,
    /// the missing eighth usage-shape of the already-shipped `@deprecated`
    /// family reading `info.deprecated` - not a duplicate of it, since
    /// nothing else checks a class's own deprecation at its *use as an
    /// attribute* specifically). Both only apply to an attribute that
    /// already resolved to a real Attribute class (`flags` above),
    /// matching real PHPStan's own `continue`-past-the-non-attribute-case
    /// ordering.
    pub(crate) fn check_attribute_targets(
        &mut self,
        attrs: &[Attribute],
        required_target: u32,
        target_name: &str,
        label: &str,
    ) {
        let mut seen_non_repeatable: HashSet<String> = HashSet::new();
        for attr in attrs {
            let fqn = self.name_ctx.resolve(&attr.name);
            let Some(info) = self.registry.classes.get(&fqn) else {
                continue;
            };
            let Some(flags) = info.attribute_targets else {
                self.report(
                    label.to_string(),
                    format!(
                        "{} {fqn} is not an Attribute class.",
                        class_type_word(info.kind)
                    ),
                    attr.span,
                    0,
                    "attribute.notAttribute",
                );
                continue;
            };
            if flags & required_target == 0 {
                self.report(
                    label.to_string(),
                    format!("Attribute class {fqn} does not have the {target_name} target."),
                    attr.span,
                    0,
                    "attribute.target",
                );
            }
            if flags & ATTRIBUTE_IS_REPEATABLE == 0
                && !seen_non_repeatable.insert(fqn.to_lowercase())
            {
                self.report(
                    label.to_string(),
                    format!(
                        "Attribute class {fqn} is not repeatable but is already present above the {target_name}."
                    ),
                    attr.span,
                    0,
                    "attribute.nonRepeatable",
                );
            }
            if self.registry.check_deprecated_usage
                && !self.current_scope_deprecated
                && let Some(desc) = &info.deprecated
            {
                let message = if desc.is_empty() {
                    format!("Attribute class {fqn} is deprecated.")
                } else {
                    format!("Attribute class {fqn} is deprecated: {desc}")
                };
                self.report(
                    label.to_string(),
                    message,
                    attr.span,
                    0,
                    "attribute.deprecated",
                );
            }
        }
    }

    /// Phase 37: real PHPStan's own `ConstantAttributesRule`
    /// (`vendor/phpstan-src/src/Rules/Constants/ConstantAttributesRule.php`),
    /// checking a top-level (or nested, conditionally-declared) `const A =
    /// 1, B = 2;` statement's own `#[Attr]` group(s), gated on PHP 8.5's
    /// `supportsAttributesOnGlobalConstants()` first (an unsupported
    /// version reports exactly one `constant.attributesNotSupported` and
    /// skips the rest entirely - matching real PHPStan's own early
    /// return). `consts.first()` only, deliberately: the parser clones
    /// one shared attribute group onto every `ClassConst` in the
    /// statement (same pattern as a class body's own multi-const
    /// declarator list), so looping over every entry would report the
    /// identical finding once per declared name instead of once per
    /// statement, the way real PHPStan's own whole-statement node type
    /// does. With no configured `php_version` at all, the version gate is
    /// silently skipped (this project's own "unset means unconfigured"
    /// convention - see `check_class_constant_modifiers`) but the
    /// existence/target checks below still run unconditionally, unlike
    /// real PHPStan's own early return - this project never has "no
    /// version" as a real state the way real PHPStan's runtime does, so
    /// silently skipping the always-valid checks too would just be a
    /// false-negative with no matching real-PHPStan behavior to mirror.
    pub(crate) fn check_global_const_attributes(
        &mut self,
        consts: &[ClassConst],
        stmt_span: Span,
        label: &str,
    ) {
        let Some(first) = consts.first() else {
            return;
        };
        if first.attrs.is_empty() {
            return;
        }
        if let Some(version) = self.registry.php_version
            && !crate::php_version::PhpFeature::AttributesOnGlobalConstants.is_available(version)
        {
            self.report_builder(
                DiagnosticBuilder::new(
                    label.to_string(),
                    "Attributes on global constants are supported only on PHP 8.5 and later."
                        .to_string(),
                    stmt_span,
                    0,
                    "constant.attributesNotSupported",
                )
                .non_ignorable(),
            );
            return;
        }
        self.check_attributes(&first.attrs, label);
        self.check_attribute_targets(&first.attrs, ATTRIBUTE_TARGET_CONSTANT, "constant", label);
    }

    /// Phase 37: generalizes what was originally just real PHPStan's
    /// `TraitAttributeClassRule` (`attribute.trait`) into its own full
    /// `NonClassAttributeClassRule` equivalent - a *self*-declaration-site
    /// check (is *this* declaration allowed to mark itself `#[\Attribute]`
    /// at all?), a different kind of check than target-*validity* (which
    /// governs where an *existing* attribute class may be *used*, not
    /// what may declare itself as one), so it's not folded into
    /// `check_attribute_targets` above. The attribute name is compared
    /// case-insensitively with no namespace resolution, matching real
    /// PHPStan's own `toLowerString()` (`\Attribute` and a same-named
    /// unrelated class both match - the same leniency `is_override_
    /// attribute_name`/`is_no_discard_attribute_name` already use for
    /// this exact class of marker-attribute check). Only the *first*
    /// `#[\Attribute]`-named attribute matters - real PHPStan's own rule
    /// returns after the first match too (`InClassNode`'s own outer loop
    /// `return`s as soon as one is found).
    pub(crate) fn check_self_attribute_class_declaration(
        &mut self,
        attrs: &[Attribute],
        fqn: &str,
        kind: ClassKind,
        is_abstract: bool,
        label: &str,
    ) {
        let Some(attr) = attrs
            .iter()
            .find(|a| a.name.trim_start_matches('\\').to_lowercase() == "attribute")
        else {
            return;
        };
        match kind {
            ClassKind::Trait => {
                self.report(
                    label.to_string(),
                    "Trait cannot be an Attribute class.".to_string(),
                    attr.span,
                    0,
                    "attribute.trait",
                );
                return;
            }
            ClassKind::Interface => {
                self.report(
                    label.to_string(),
                    "Interface cannot be an Attribute class.".to_string(),
                    attr.span,
                    0,
                    "attribute.interface",
                );
                return;
            }
            ClassKind::Enum => {
                self.report(
                    label.to_string(),
                    "Enum cannot be an Attribute class.".to_string(),
                    attr.span,
                    0,
                    "attribute.enum",
                );
                return;
            }
            ClassKind::Class => {}
        }
        if is_abstract {
            self.report(
                label.to_string(),
                format!("Abstract class {fqn} cannot be an Attribute class."),
                attr.span,
                0,
                "attribute.abstract",
            );
            return;
        }
        if let Some(ctor) = self.registry.lookup_method(fqn, "__construct")
            && ctor.visibility != Visibility::Public
        {
            self.report(
                label.to_string(),
                format!("Attribute class {fqn} constructor must be public."),
                attr.span,
                0,
                "attribute.constructorNotPublic",
            );
        }
    }

    /// Phase 37: PHP's native `#[\Deprecated]`/`#[\AllowDynamicProperties]`
    /// marker-attribute misuse on a class-like declaration itself - real
    /// PHPStan's own `ClassAttributesRule`/`TraitAttributesRule`.
    /// `#[\Deprecated]` on a class/interface/enum is a real, unconditional
    /// PHP fatal error (confirmed directly: `#[\Deprecated] class Foo {}`
    /// fatals "Cannot apply #[\Deprecated] to class Foo"); on a trait it's
    /// PHP-8.5-gated instead (confirmed directly: below 8.5 it's silently
    /// inert, no real deprecation warning ever fires when the trait is
    /// used - from 8.5 on it does). `#[\AllowDynamicProperties]` is
    /// pointless/wrong on a `readonly class`, `enum`, `interface`, or
    /// (unconditionally) `trait` - none of which can ever have a real
    /// dynamic property to permit; an ordinary *non*-readonly class is
    /// never flagged at all, since that's the attribute's own real
    /// intended target.
    pub(crate) fn check_class_like_marker_attributes(
        &mut self,
        attrs: &[Attribute],
        kind: ClassKind,
        is_readonly: bool,
        label: &str,
    ) {
        let type_word = class_type_word(kind).to_lowercase();
        for attr in attrs {
            if is_deprecated_attribute_name(&attr.name) {
                if kind == ClassKind::Trait {
                    if self
                        .registry
                        .php_version
                        .is_some_and(|v| !PhpFeature::DeprecatedAttributeOnTraits.is_available(v))
                    {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                label.to_string(),
                                "Attribute class Deprecated can be used with traits only on PHP 8.5 and later."
                                    .to_string(),
                                attr.span,
                                0,
                                "trait.deprecatedAttribute",
                            )
                            .non_ignorable(),
                        );
                    }
                } else {
                    self.report_builder(
                        DiagnosticBuilder::new(
                            label.to_string(),
                            format!(
                                "Attribute class Deprecated cannot be used with {type_word} {label}."
                            ),
                            attr.span,
                            0,
                            format!("{type_word}.deprecatedAttribute"),
                        )
                        .non_ignorable(),
                    );
                }
            }
            if is_allow_dynamic_properties_attribute_name(&attr.name) {
                let flagged = match kind {
                    ClassKind::Class => is_readonly,
                    ClassKind::Interface | ClassKind::Enum | ClassKind::Trait => true,
                };
                if !flagged {
                    continue;
                }
                let (identifier, subject) = if kind == ClassKind::Class {
                    (
                        "class.allowDynamicPropertiesReadonly".to_string(),
                        "readonly class".to_string(),
                    )
                } else {
                    (
                        format!("{type_word}.allowDynamicProperties"),
                        type_word.clone(),
                    )
                };
                self.report_builder(
                    DiagnosticBuilder::new(
                        label.to_string(),
                        format!(
                            "Attribute class AllowDynamicProperties cannot be used with {subject}."
                        ),
                        attr.span,
                        0,
                        identifier,
                    )
                    .non_ignorable(),
                );
            }
        }
    }
}
