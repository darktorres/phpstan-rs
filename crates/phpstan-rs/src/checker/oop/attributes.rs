use rustc_hash::FxHashSet;

use crate::checker::{CheckArgsCall, Checker, Locals, class_type_word};
use crate::registry::{is_allow_dynamic_properties_attribute_name, is_deprecated_attribute_name};
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use syntax::token::Span;
use types::php_version::PhpFeature;

/// PHP's `Attribute::TARGET_*` values (see
/// `registry::class_info::attribute_target_constant_value`'s identical
/// table) - only the ones this project's call sites for
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
    /// Mirrors real PHPStan's `AttributesCheck` - every `#[Name(...)]`
    /// group's `Name` must resolve to a real, known class, the same
    /// `attribute.notFound` identifier real PHPStan's rule uses. Wired
    /// at every declaration site an `Attribute` list can appear on
    /// (class/enum, method/function, property, const, enum case, param,
    /// closure/arrow-fn - see each call site).
    ///
    /// Also validates constructor arguments
    /// (`attribute.noConstructor`/`attribute.constructorNotPublic`/
    /// `attribute.abstract`/arity/type checks): `resolve_call_args`/
    /// `check_args` (`checker/calls.rs`) already take a bare
    /// `args: &[Arg]` + `sig: &FunctionSig`, with no expr-node coupling -
    /// the exact same pair `ExprKind::New`'s constructor-call handling
    /// uses, directly reusable here. Attribute arguments are real PHP
    /// constant-expressions (no local variable can ever appear in one),
    /// so a fresh, empty `Locals::default()` is the correct scope to
    /// check them against. `attribute.nonRepeatable`/`attribute.deprecated`
    /// aren't ported here (the former needs a same-declaration-list
    /// accumulator, a different shape of check than this per-attribute
    /// loop; the latter would duplicate this project's existing
    /// `@deprecated` family). Whether the resolved class is actually
    /// usable as an attribute at all, and whether its declared
    /// `Attribute::TARGET_*` flags match where this one is used, are
    /// checked too - see `check_attribute_targets` below.
    pub(crate) fn check_attributes(&mut self, attrs: &[Attribute], label: &str) {
        for attr in attrs {
            let fqn = self.name_ctx.resolve(&attr.name);
            // `lookup_class` (case-insensitive fallback), not a bare
            // `self.registry.classes.get(&fqn)` - without it, a
            // case-mismatched attribute reference always reports "does
            // not exist" here before `check_class_name_case` below ever
            // gets a chance to see it. Every subsequent lookup/message
            // in this function uses `canonical` (the real declared
            // case), not `fqn` (as-written, possibly wrong-case) -
            // avoids a message naming the very case `check_class_name_case`
            // is simultaneously flagging as wrong.
            let Some((canonical, info)) = self.registry.lookup_class(&fqn) else {
                self.report(
                    format!("attribute class {fqn} does not exist"),
                    attr.span,
                    0,
                    "attribute.notFound",
                );
                continue;
            };
            let canonical = canonical.to_string();
            // Attribute-reference `<kind>.nameCase` parity - mirrors
            // real PHPStan's `AttributesCheck`, which runs its identical
            // `ClassNameCheck::checkClassNames` call right here too,
            // immediately after confirming the name resolves.
            self.check_class_name_case(&attr.name, attr.span);
            if info.is_abstract {
                self.report(
                    format!("Attribute class {canonical} is abstract."),
                    attr.span,
                    0,
                    "attribute.abstract",
                );
            }
            // `lookup_method` (ancestor-chain-aware), not a bare
            // `info.methods.get("__construct")` - an attribute class
            // that declares no `__construct` of its own, inheriting its
            // parent's, would otherwise have a bare own-class lookup
            // wrongly see "no constructor" and misfire
            // `attribute.noConstructor` on every real, legal use.
            let ctor_sig = self.registry.lookup_method(&canonical, "__construct");
            let Some(ctor_sig) = ctor_sig else {
                if !attr.args.is_empty() {
                    self.report(format!(
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
            self.check_args(CheckArgsCall {
                callee_label: &format!("{canonical}::__construct"),
                callee: crate::checker::calls::parameters::Callee::AttributeConstructor(&canonical),
                sig: ctor_sig,
                args: &attr.args,
                locals: &mut locals,
                defining_class: Some(&canonical),
                is_builtin: self.registry.is_builtin_class(&canonical),
                span: attr.span,
            });
        }
    }

    /// Mirrors the `attribute.notAttribute`/`attribute.target` halves of
    /// real PHPStan's `AttributesCheck` - see
    /// `registry::class_info::compute_attribute_targets`'s doc comment
    /// for how a class's allowed targets are computed. Called from every
    /// declaration kind attributes can appear on (class/enum, property,
    /// const, method/function, param, closure/arrow-fn, property hook -
    /// see each call site). A name the registry has no entry for at all
    /// is skipped here (the existence check above already reports it,
    /// and reporting both would be confusing double-counting for the
    /// identical typo).
    ///
    /// Also folds in `attribute.nonRepeatable` (an attribute class
    /// missing `Attribute::IS_REPEATABLE` used more than once in this
    /// `attrs` list - a `HashSet` scoped fresh per call, matching real
    /// PHPStan's per-`check()` accumulator) and `attribute.deprecated`
    /// (gated on `registry.check_deprecated_usage`, reading
    /// `info.deprecated` - checks a class's own deprecation at its use
    /// as an attribute specifically, not a duplicate of the existing
    /// `@deprecated` family). Both only apply to an attribute that
    /// already resolved to a real Attribute class (`flags` above),
    /// matching real PHPStan's `continue`-past-the-non-attribute-case
    /// ordering.
    pub(crate) fn check_attribute_targets(
        &mut self,
        attrs: &[Attribute],
        required_target: u32,
        target_name: &str,
    ) {
        let mut seen_non_repeatable: FxHashSet<String> = FxHashSet::default();
        for attr in attrs {
            let fqn = self.name_ctx.resolve(&attr.name);
            let Some(info) = self.registry.classes.get(fqn.as_str()) else {
                continue;
            };
            let Some(flags) = info.attribute_targets else {
                self.report(
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
                    format!("Attribute class {fqn} does not have the {target_name} target."),
                    attr.span,
                    0,
                    "attribute.target",
                );
            }
            if flags & ATTRIBUTE_IS_REPEATABLE == 0
                && !seen_non_repeatable.insert(fqn.to_lowercase())
            {
                self.report(format!(
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
                self.report(message, attr.span, 0, "attribute.deprecated");
            }
        }
    }

    /// Mirrors real PHPStan's `ConstantAttributesRule`, checking a
    /// top-level (or nested, conditionally-declared) `const A = 1,
    /// B = 2;` statement's own `#[Attr]` group(s), gated on PHP 8.5's
    /// `supportsAttributesOnGlobalConstants()` first (an unsupported
    /// version reports exactly one `constant.attributesNotSupported` and
    /// skips the rest, matching real PHPStan's early return).
    /// `consts.first()` only, deliberately: the parser clones one shared
    /// attribute group onto every `ClassConst` in the statement, so
    /// looping over every entry would report the identical finding once
    /// per declared name instead of once per statement, the way real
    /// PHPStan's whole-statement node type does. With no configured
    /// `php_version` at all, the version gate is silently skipped (this
    /// project's "unset means unconfigured" convention) but the
    /// existence/target checks below still run unconditionally, unlike
    /// real PHPStan's early return - this project never has "no
    /// version" as a real state the way real PHPStan's runtime does.
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
            && !types::php_version::PhpFeature::AttributesOnGlobalConstants.is_available(version)
        {
            self.report_builder(
                DiagnosticBuilder::new(
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
        self.check_attribute_targets(&first.attrs, ATTRIBUTE_TARGET_CONSTANT, "constant");
    }

    /// Mirrors real PHPStan's `NonClassAttributeClassRule` - a
    /// self-declaration-site check (is this declaration allowed to mark
    /// itself `#[\Attribute]` at all?), a different kind of check than
    /// target-validity (which governs where an existing attribute class
    /// may be used, not what may declare itself as one), so it's not
    /// folded into `check_attribute_targets` above. The attribute name
    /// is compared case-insensitively with no namespace resolution,
    /// mirroring real PHPStan's `toLowerString()` (`\Attribute` and a
    /// same-named unrelated class both match - the same leniency
    /// `is_override_attribute_name`/`is_no_discard_attribute_name` use
    /// for this class of marker-attribute check). Only the first
    /// `#[\Attribute]`-named attribute matters - real PHPStan's rule
    /// returns after the first match too.
    pub(crate) fn check_self_attribute_class_declaration(
        &mut self,
        attrs: &[Attribute],
        fqn: &str,
        kind: ClassKind,
        is_abstract: bool,
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
                    "Trait cannot be an Attribute class.".to_string(),
                    attr.span,
                    0,
                    "attribute.trait",
                );
                return;
            }
            ClassKind::Interface => {
                self.report(
                    "Interface cannot be an Attribute class.".to_string(),
                    attr.span,
                    0,
                    "attribute.interface",
                );
                return;
            }
            ClassKind::Enum => {
                self.report(
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
                format!("Attribute class {fqn} constructor must be public."),
                attr.span,
                0,
                "attribute.constructorNotPublic",
            );
        }
    }

    /// PHP's native `#[\Deprecated]`/`#[\AllowDynamicProperties]`
    /// marker-attribute misuse on a class-like declaration itself,
    /// mirroring real PHPStan's `ClassAttributesRule`/
    /// `TraitAttributesRule`. `#[\Deprecated]` on a class/interface/enum
    /// is a real, unconditional PHP fatal error ("Cannot apply
    /// #[\Deprecated] to class Foo"); on a trait it's PHP-8.5-gated
    /// instead (below 8.5 it's silently inert, no real deprecation
    /// warning ever fires when the trait is used - from 8.5 on it does).
    /// `#[\AllowDynamicProperties]` is pointless/wrong on a `readonly
    /// class`, `enum`, `interface`, or (unconditionally) `trait` - none
    /// of which can ever have a real dynamic property to permit; an
    /// ordinary non-readonly class is never flagged, since that's the
    /// attribute's real intended target.
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
                            DiagnosticBuilder::new("Attribute class Deprecated can be used with traits only on PHP 8.5 and later."
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
                        DiagnosticBuilder::new(format!(
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
