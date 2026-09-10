use crate::ast::*;
use crate::checker::{
    CheckArgsCall, Checker, DiagnosticBuilder, Locals, class_type_word, resolve_call_args,
};
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
            let Some(info) = self.registry.classes.get(&fqn) else {
                self.report(
                    label.to_string(),
                    format!("attribute class {fqn} does not exist"),
                    attr.span,
                    0,
                    "attribute.notFound",
                );
                continue;
            };
            if info.is_abstract {
                self.report(
                    label.to_string(),
                    format!("Attribute class {fqn} is abstract."),
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
            let ctor_sig = self.registry.lookup_method(&fqn, "__construct");
            let Some(ctor_sig) = ctor_sig else {
                if !attr.args.is_empty() {
                    self.report(
                        label.to_string(),
                        format!(
                            "Attribute class {fqn} does not have a constructor and must be instantiated without any parameters."
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
                    format!("Constructor of attribute class {fqn} is not public."),
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
                callee_label: &format!("{fqn}::__construct"),
                sig: ctor_sig,
                args: &attr.args,
                resolved: &resolved,
                locals: &mut locals,
                label,
                defining_class: Some(&fqn),
                span: attr.span,
            });
        }
    }

    /// Phase 37 follow-up: the `attribute.notAttribute`/`attribute.target`
    /// halves of real PHPStan's own `AttributesCheck` - mirrors
    /// `registry::class_info::compute_attribute_targets`'s own doc
    /// comment for how a class's own allowed targets are computed.
    /// Deliberately called only from the two highest-value declaration
    /// kinds so far (property, method/function - see each call site);
    /// class-level, param-level (including the promoted-property
    /// `parameter or property` dual-target shape), enum-case, class-
    /// constant, and closure/arrow-fn attributes still only get the
    /// plain existence check above (see ROADMAP.md's own Phase 37 entry).
    /// A name the registry has no entry for at all is skipped here (the
    /// existence check above already reports it, and reporting *both*
    /// would be confusing double-counting for the identical typo).
    pub(crate) fn check_attribute_targets(
        &mut self,
        attrs: &[Attribute],
        required_target: u32,
        target_name: &str,
        label: &str,
    ) {
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

    /// Real PHPStan's `TraitAttributeClassRule` (`attribute.trait`,
    /// level 0): a *trait* declaration can never itself be marked
    /// `#[\Attribute]` (only a real class can be an Attribute class at
    /// all) - a different kind of check than target-*validity* (which
    /// governs where an *existing* attribute class may be *used*, not
    /// what may declare itself as one), so it's not folded into
    /// `check_attribute_targets` above. The attribute name is compared
    /// case-insensitively with no namespace resolution, matching real
    /// PHPStan's own `toLowerString()` (`\Attribute` and a same-named
    /// unrelated class both match - the same leniency `is_override_
    /// attribute_name`/`is_no_discard_attribute_name` already use for
    /// this exact class of marker-attribute check).
    pub(crate) fn check_trait_is_not_an_attribute_class(
        &mut self,
        attrs: &[Attribute],
        label: &str,
    ) {
        for attr in attrs {
            if attr.name.trim_start_matches('\\').to_lowercase() == "attribute" {
                self.report(
                    label.to_string(),
                    "Trait cannot be an Attribute class.".to_string(),
                    attr.span,
                    0,
                    "attribute.trait",
                );
            }
        }
    }
}
