use std::collections::HashMap;

use crate::ast::*;
use crate::checker::{
    CheckArgsCall, Checker, DiagnosticBuilder, Locals, body_contains_yield, class_kind_identifier,
    class_type_word, collect_goto_targets, expr_contains_yield, resolve_call_args,
};
use crate::docblock;
use crate::registry::{self, FunctionSig, PropertySig};
use crate::token::Span;
use crate::types::Type;

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

/// The full set of PHP magic-method names (`nikic/php-parser`'s own
/// `ClassMethod::isMagic()` list) - used only by `check_enum_sanity`
/// below to tell a real magic method apart from an ordinary one that
/// merely starts with `__`.
fn is_magic_method_name(lower: &str) -> bool {
    matches!(
        lower,
        "__construct"
            | "__destruct"
            | "__call"
            | "__callstatic"
            | "__get"
            | "__set"
            | "__isset"
            | "__unset"
            | "__sleep"
            | "__wakeup"
            | "__serialize"
            | "__unserialize"
            | "__tostring"
            | "__invoke"
            | "__set_state"
            | "__clone"
            | "__debuginfo"
    )
}

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

    /// Finding 46's class-level half: extending a class declared `final`
    /// is a real, unconditional fatal error in PHP at class-declaration
    /// time, regardless of whether the subclass overrides anything at
    /// all. The method-level half (overriding a `final` *method*) lives
    /// in `check_method_overrides`, sharing that function's own
    /// per-method ancestor walk instead of a second one here.
    pub(crate) fn check_final_class_extension(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let Some(parent) = info.extends.first() else {
            return;
        };
        let Some(parent_info) = self.registry.classes.get(parent) else {
            return;
        };
        if parent_info.is_final {
            self.report(
                fqn.to_string(),
                format!("class {fqn} cannot extend final class {parent}"),
                span,
                0,
                "class.extendsFinal",
            );
        }
    }

    /// Phase 48: the class/enum-declaration-level half of the "does this
    /// name actually resolve" existence pipeline - every `extends`
    /// target (a class's single parent, or an interface's own possibly-
    /// multiple parent interfaces), every `implements` target, and every
    /// `use`d trait. Unlike `check_final_class_extension`'s identical
    /// `info.extends.get(0)`-only walk (which only needs the *first*
    /// parent to check finality, since a class can only ever have one),
    /// this checks every entry - an interface's own `extends A, B;` is
    /// real, valid PHP with more than one target. Operates purely on the
    /// already-indexed, already name-resolved `ClassInfo` (`info.extends`/
    /// `implements`/`uses` are fully-qualified names by the time
    /// `Registry::index` builds them - see `class_info.rs`'s own
    /// `ctx.resolve(s)` calls), so this needs no raw `ClassDecl`/
    /// `EnumDecl` at all and covers both uniformly from one call site.
    /// Deliberately existence-only, not kind-compatibility (a class
    /// `implements`-ing another class instead of an interface is a real,
    /// separate PHPStan rule this phase doesn't attempt) - matches the
    /// scope every other `Existing*Rule` this phase's own roadmap entry
    /// lists shares.
    pub(crate) fn check_class_references_exist(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for parent in &info.extends {
            if !self.class_name_exists(parent) {
                let verb = if info.kind == ClassKind::Interface {
                    "extends unknown interface"
                } else {
                    "extends unknown class"
                };
                self.report(
                    fqn.to_string(),
                    format!("{fqn} {verb} {parent}"),
                    span,
                    0,
                    "class.notFound",
                );
            }
        }
        for iface in &info.implements {
            if !self.class_name_exists(iface) {
                self.report(
                    fqn.to_string(),
                    format!("{fqn} implements unknown interface {iface}"),
                    span,
                    0,
                    // Real PHPStan's own `ExistingClassesInClassImplementsRule`
                    // identifier for this exact message shape - confirmed
                    // directly against `vendor/phpstan-src`, not guessed
                    // (unlike `extends`/trait-use just above/below, which
                    // really do share the generic `class.notFound`).
                    "interface.notFound",
                );
            }
        }
        for used_trait in &info.uses {
            if !self.class_name_exists(used_trait) {
                self.report(
                    fqn.to_string(),
                    format!("{fqn} uses unknown trait {used_trait}"),
                    span,
                    0,
                    // Real PHPStan's own `ExistingClassInTraitUseRule`
                    // identifier - confirmed against `vendor/phpstan-src`.
                    "trait.notFound",
                );
            }
        }
    }

    /// Finding 58: two traits used by the same class declaring the same
    /// property (with a genuinely different type) or method (with a
    /// genuinely different signature) is a real, unconditional PHP fatal
    /// error at class-declaration time unless the class explicitly
    /// resolves the conflict (`insteadof`/`as` - Finding 30's own
    /// `resolved_trait_conflicts`, tracked regardless of whether this
    /// project models exactly which trait `insteadof` picked). Compares
    /// only pairs of *different* traits - a name declared by just one
    /// used trait is never a conflict, no matter what it looks like.
    pub(crate) fn check_trait_conflicts(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        if info.uses.len() < 2 {
            return;
        }

        let mut seen_props: HashMap<&str, (&str, &PropertySig)> = HashMap::new();
        let mut seen_methods: HashMap<&str, (&str, &FunctionSig)> = HashMap::new();
        for trait_name in &info.uses {
            let Some(trait_info) = self.registry.classes.get(trait_name) else {
                continue;
            };
            for (prop_name, sig) in &trait_info.properties {
                if info.resolved_trait_conflicts.contains(prop_name) {
                    continue;
                }
                match seen_props.get(prop_name.as_str()) {
                    Some((first_trait, first_sig)) if first_sig.ty != sig.ty => {
                        self.report(fqn.to_string(), format!(
                                "{first_trait} and {trait_name} define the same property (${prop_name}) with incompatible types in the composition of {fqn}"
                            ), span, 0, "trait.propertyConflict");
                    }
                    Some(_) => {}
                    None => {
                        seen_props.insert(prop_name, (trait_name, sig));
                    }
                }
            }
            for (method_name, sig) in &trait_info.methods {
                if info.resolved_trait_conflicts.contains(method_name) {
                    continue;
                }
                match seen_methods.get(method_name.as_str()) {
                    Some((first_trait, first_sig)) if !signatures_compatible(first_sig, sig) => {
                        self.report(fqn.to_string(), format!(
                                "{first_trait} and {trait_name} define the same method ({method_name}) with an incompatible signature in the composition of {fqn}"
                            ), span, 0, "trait.methodConflict");
                    }
                    Some(_) => {}
                    None => {
                        seen_methods.insert(method_name, (trait_name, sig));
                    }
                }
            }
        }
    }

    /// Phase 65: `class` is reserved for `ClassName::class` name-fetching,
    /// so declaring a class constant by that name is a real,
    /// unconditional PHP fatal error, confirmed directly against real
    /// PHP's own message: a class constant must not be called `class`,
    /// since it's reserved for class name fetching. Shared between
    /// `check_method_declaration_sanity`'s own two call sites' const
    /// lists (a `ClassDecl`'s `consts` and an `EnumDecl`'s) rather than
    /// living inside that function itself - unrelated to methods, and
    /// enums route through a params-less call there already.
    pub(crate) fn check_reserved_class_constant_name(&mut self, fqn: &str, consts: &[ClassConst]) {
        for c in consts {
            if c.name == "class" {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        "a class constant must not be called 'class'; it is reserved for class name fetching".to_string(),
                        c.span,
                        0,
                        "classConstant.class",
                    )
                    .non_ignorable(),
                );
            }
        }
    }

    /// `InvalidPHPStanDocTagRule` (`phpDoc.phpstanTag`) - a `@phpstan-*`
    /// tag real PHPStan doesn't recognize at all (a real, common way to
    /// catch a typo, e.g. `@phpstan-parm`, silently doing nothing instead
    /// of what was intended). Scoped to a class's own docblock plus
    /// every one of its methods'/properties'/consts' - real PHPStan's own
    /// rule additionally covers arbitrary statement-level and function-
    /// level docblocks (`check_unknown_phpstan_tags_on` is called
    /// separately for top-level functions, `check_function`'s own call
    /// site) - a bare statement's own inline docblock isn't covered, a
    /// disclosed scope trim (lower value: `@phpstan-*` tags are
    /// overwhelmingly declaration-level in practice).
    pub(crate) fn check_unknown_phpstan_tags_in_class(&mut self, fqn: &str, c: &ClassDecl) {
        self.check_unknown_phpstan_tags_on(fqn, c.doc_comment.as_deref(), c.name_span);
        for m in &c.methods {
            self.check_unknown_phpstan_tags_on(
                fqn,
                m.function.doc_comment.as_deref(),
                m.function.name_span,
            );
        }
        for p in &c.properties {
            self.check_unknown_phpstan_tags_on(fqn, p.doc_comment.as_deref(), p.span);
        }
        for cc in &c.consts {
            self.check_unknown_phpstan_tags_on(fqn, cc.doc_comment.as_deref(), cc.name_span);
        }
    }

    /// See `check_unknown_phpstan_tags_in_class`'s own doc comment - the
    /// one-declaration-at-a-time worker both that function and
    /// `check_function`'s own top-level-function call site share.
    pub(crate) fn check_unknown_phpstan_tags_on(
        &mut self,
        label: &str,
        doc_comment: Option<&str>,
        span: Span,
    ) {
        let Some(doc) = doc_comment else {
            return;
        };
        for tag in docblock::unknown_phpstan_tags(doc) {
            self.report(
                label.to_string(),
                format!("Unknown PHPDoc tag: @phpstan-{tag}"),
                span,
                2,
                "phpDoc.phpstanTag",
            );
        }
    }

    /// `ClassConstantRule`'s own typed-constant half: PHP 8.3's
    /// `const TYPE NAME = value;` syntax declares a real, unconditional
    /// native type for the constant - checked here the same "does the
    /// literal value actually satisfy the declared type" way a typed
    /// property's own default value already is (see `check_missing_
    /// property_typehints`'s sibling checks), matching real PHPStan's
    /// own `classConstant.value` identifier exactly. Untyped (`const
    /// NAME = value;`, the overwhelming majority) constants have nothing
    /// to check here - their own `@var`-doc-refined type (a *separate*,
    /// already-existing mechanism, `ConstSig::ty`'s own construction in
    /// `class_info.rs`) is never enforced this strictly against real
    /// PHPStan's own convention either (`classConstant.phpDocType`, not
    /// attempted here). `self.infer_expr`, not the registry's own
    /// already-resolved `ConstSig.ty`, so a value referencing another
    /// constant/enum case gets this checker's full expression inference
    /// rather than only ever seeing `infer_const_type`'s narrower
    /// literal-only view.
    pub(crate) fn check_typed_class_constant_values(&mut self, fqn: &str, consts: &[ClassConst]) {
        if !consts.iter().any(|c| c.type_hint.is_some()) {
            return;
        }
        // `self::OTHER_CONST` inside a const's own value (a real, common
        // idiom - confirmed live: without this, `resolve_static_class_
        // name("self")` had no defining class to resolve against here,
        // silently reading every `self::`-qualified value as `Mixed` and
        // producing a real false positive against `int DERIVED =
        // self::BASE;`) needs `current_class` set the same way `check_
        // function` sets it for a method body - this call site runs
        // outside any method, so it's otherwise left at whatever it was
        // (typically `None` here).
        let previous_class = self.current_class.take();
        self.current_class = Some(fqn.to_string());
        for c in consts {
            let Some(raw_hint) = &c.type_hint else {
                continue;
            };
            let declared_ty = self.resolve_type_hint(Some(raw_hint));
            let value_ty = self.infer_expr(&c.value, &Locals::default());
            // `Type::Mixed` here almost always means "this checker
            // couldn't resolve it" (an unrecognized global constant -
            // this project has no such table at all, confirmed live:
            // `DEBUG_BACKTRACE_IGNORE_ARGS`, a real PHP builtin, reads as
            // `Mixed`), not a genuinely `mixed`-typed value - the same
            // "don't guess wrong on Mixed" leniency `check_method_
            // overrides`'s own parameter-narrowing check already applies
            // for the identical reason.
            if value_ty == Type::Mixed {
                continue;
            }
            if !self.registry.is_assignable(&value_ty, &declared_ty) {
                self.report(
                    fqn.to_string(),
                    format!(
                        "constant {fqn}::{} ({declared_ty}) does not accept value {value_ty}",
                        c.name
                    ),
                    c.span,
                    0,
                    "classConstant.value",
                );
            }
        }
        self.current_class = previous_class;
    }

    /// Phase 65 follow-up: three small, independent PHP-version/modifier-
    /// consistency checks on a class constant's own declaration, all
    /// confirmed directly via `php -l`/`php -r` and matching real
    /// PHPStan's own identifiers exactly:
    ///
    /// `classConstant.finalNotSupported` (`FinalConstantRule`) - `final
    /// const` needs PHP 8.1+; `classConstant.finalPrivate`
    /// (`FinalPrivateConstantRule`) - a `private` constant can never be
    /// `final` too, since nothing outside its own declaring class could
    /// ever see it to override in the first place (real PHP's own exact
    /// wording: "Private constant Foo::X cannot be final as it is not
    /// visible to other classes") - unconditional, no PHP-version gate at
    /// all; `classConstant.nativeTypeNotSupported`
    /// (`NativeTypedClassConstantRule`) - a typed constant (`const TYPE
    /// NAME = value;`) needs PHP 8.3+. All three reuse `PhpFeature`
    /// variants (`FinalClassConstants`/`NativeTypedClassConstants`)
    /// Phase 38 already defined but never wired to anything.
    pub(crate) fn check_class_constant_modifiers(&mut self, fqn: &str, consts: &[ClassConst]) {
        let version = self.registry.php_version;
        for c in consts {
            if c.is_final
                && let Some(version) = version
                && !crate::php_version::PhpFeature::FinalClassConstants.is_available(version)
            {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        "Final class constants are supported only on PHP 8.1 and later."
                            .to_string(),
                        c.span,
                        0,
                        "classConstant.finalNotSupported",
                    )
                    .non_ignorable(),
                );
            }
            if c.is_final && c.visibility == Visibility::Private {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
                            "Private constant {fqn}::{} cannot be final as it is never overridden by other classes.",
                            c.name
                        ),
                        c.span,
                        0,
                        "classConstant.finalPrivate",
                    )
                    .non_ignorable(),
                );
            }
            if c.type_hint.is_some()
                && let Some(version) = version
                && !crate::php_version::PhpFeature::NativeTypedClassConstants.is_available(version)
            {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        "Class constants with native types are supported only on PHP 8.3 and later.".to_string(),
                        c.span,
                        0,
                        "classConstant.nativeTypeNotSupported",
                    )
                    .non_ignorable(),
                );
            }
        }
    }

    /// Phase 65: a batch of small, independent method-declaration
    /// boolean-flag-combination checks real PHP enforces unconditionally
    /// at compile time, all confirmed directly against real `php -l` -
    /// this project's own parser is deliberately more lenient than the
    /// real engine (it has to tolerate parsing code that might never
    /// actually run), so nothing else would catch any of these before
    /// that code runs and fatals for real.
    ///
    /// `kind`/`is_class_abstract` describe the *declaring* class/enum
    /// (never `Trait` for `is_class_abstract` - a trait itself is never
    /// `abstract`/non-abstract in real PHP's own sense, and traits are
    /// deliberately exempt from the "abstract method needs an abstract
    /// container" check below the same way `check_class_completeness`
    /// already exempts them from "must implement every abstract
    /// method" - a trait's whole point is supplying a method the
    /// *using* class must complete).
    pub(crate) fn check_method_declaration_sanity(
        &mut self,
        fqn: &str,
        kind: ClassKind,
        is_class_abstract: bool,
        methods: &[MethodDecl],
    ) {
        for m in methods {
            // `method.abstract`: an abstract method inside a non-
            // abstract, non-interface, non-trait class/enum - real PHP:
            // "Class Foo declares abstract method bar() and must
            // therefore be declared abstract." An enum's own built-in
            // `cases()`/`from()`/`tryFrom()` are exempt, matching real
            // PHPStan's own identical exemption (they're the one shape
            // an enum's abstract method declaration legitimately
            // re-states rather than violates).
            if m.is_abstract
                && !is_class_abstract
                && matches!(kind, ClassKind::Class | ClassKind::Enum)
            {
                let lower = m.function.name.to_ascii_lowercase();
                let enum_builtin = kind == ClassKind::Enum
                    && matches!(lower.as_str(), "cases" | "from" | "tryfrom");
                if !enum_builtin {
                    let description = if kind == ClassKind::Enum {
                        "Enum"
                    } else {
                        "Non-abstract class"
                    };
                    self.report_builder(
                        DiagnosticBuilder::new(
                            fqn.to_string(),
                            format!(
                                "{description} {fqn} contains abstract method {}()",
                                m.function.name
                            ),
                            m.span,
                            0,
                            "method.abstract",
                        )
                        .non_ignorable(),
                    );
                }
            }
            // `method.abstractPrivate`: an abstract method can never be
            // `private` - nothing could ever provide the required
            // implementation, since a `private` method isn't visible to
            // (and so can't be overridden by) a subclass at all. Only
            // meaningful where an abstract method is legal to declare
            // in the first place (an abstract class or an interface);
            // elsewhere it's already `method.abstract`'s own concern,
            // and traits are exempt for the same reason as above. Every
            // interface method is implicitly abstract regardless of the
            // literal `abstract` keyword - real PHP actually *forbids*
            // writing that keyword on one at all ("Interface method
            // Foo::bar() must not be abstract", confirmed directly), so
            // `m.is_abstract` alone would never observe this shape;
            // `kind == ClassKind::Interface` stands in for it here.
            if (m.is_abstract || kind == ClassKind::Interface)
                && m.visibility == Visibility::Private
                && kind != ClassKind::Trait
                && (is_class_abstract || kind == ClassKind::Interface)
            {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
                            "private method {fqn}::{}() cannot be abstract",
                            m.function.name
                        ),
                        m.span,
                        0,
                        "method.abstractPrivate",
                    )
                    .non_ignorable(),
                );
            }
            // `method.finalPrivate`: real PHP only *warns* here (not a
            // fatal error, confirmed directly), since a `final` on a
            // `private` method is pointless rather than contradictory -
            // a private method is never overridden by anything, `final`
            // or not. `__construct` is deliberately exempt, matching
            // real PHPStan's own identical exemption - a private final
            // constructor is a real, common, legitimate singleton-
            // pattern idiom, not the "forgot final does nothing here"
            // mistake this check otherwise exists to catch.
            if m.is_final && m.visibility == Visibility::Private && m.function.name != "__construct"
            {
                self.report(
                    fqn.to_string(),
                    format!(
                        "private method {fqn}::{}() cannot be final as it is never overridden by other classes",
                        m.function.name
                    ),
                    m.span,
                    1,
                    "method.finalPrivate",
                );
            }
            // `method.visibility`: every interface method is implicitly
            // `public` - real PHP: "Access type for interface method
            // Foo::bar() must be public." Reuses the exact same
            // identifier Phase 64's own override-visibility-narrowing
            // check uses, matching real PHPStan's own deliberate
            // choice to share it between both shapes.
            if kind == ClassKind::Interface && m.visibility != Visibility::Public {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
                            "method {fqn}::{}() cannot use non-public visibility in interface",
                            m.function.name
                        ),
                        m.span,
                        0,
                        "method.visibility",
                    )
                    .non_ignorable(),
                );
            }
            // `constructor.returnType`: real PHP: "Method Foo::
            // __construct() cannot declare a return type." - a
            // constructor's own return value is never meaningful (it
            // always yields the new instance), so a native return type
            // is always a mistake, never legitimate.
            if m.function.name.eq_ignore_ascii_case("__construct")
                && m.function.return_type.is_some()
            {
                self.report(
                    fqn.to_string(),
                    format!("constructor of class {fqn} has a return type"),
                    m.span,
                    0,
                    "constructor.returnType",
                );
            }
        }
    }

    /// Finding 59: every case of a backed enum must have a unique backing
    /// value - a real, unconditional PHP fatal error at declaration time
    /// otherwise ("Duplicate value in enum ... for case ..."), not merely
    /// a design smell. Only compares cases whose value is a literal
    /// int/string (the overwhelming majority in practice) - a computed or
    /// otherwise non-literal case value isn't evaluated, matching this
    /// project's usual "don't guess" leniency.
    pub(crate) fn check_enum_case_values(&mut self, fqn: &str, e: &EnumDecl) {
        let mut seen: HashMap<(bool, String), &str> = HashMap::new();
        for case in &e.cases {
            let Some(value) = &case.value else {
                continue;
            };
            let key = match &value.kind {
                ExprKind::Literal(Literal::Int(n)) => (true, n.to_string()),
                ExprKind::Literal(Literal::String(s)) => (false, s.clone()),
                _ => continue,
            };
            if let Some(first_case) = seen.get(&key) {
                self.report(fqn.to_string(), format!(
                        "duplicate value in enum {fqn} for case {} (already used by case {first_case})",
                        case.name
                    ), case.span, 0, "enum.duplicateValue");
            } else {
                seen.insert(key, &case.name);
            }
        }
    }

    /// Phase 66: `EnumTemplateTypeRule` - enums can't be generic in real
    /// PHP (there's no `enum Foo<T>` syntax at all), so a stray
    /// `@template` tag on one's own doc comment is always a mistake,
    /// never a real declaration the way it would be on a class.
    pub(crate) fn check_enum_template_tag(&mut self, fqn: &str, e: &EnumDecl) {
        let Some(raw) = &e.doc_comment else {
            return;
        };
        let block = docblock::parse(raw);
        if block.templates.is_empty() {
            return;
        }
        let plural = if block.templates.len() == 1 { "" } else { "s" };
        self.report(
            fqn.to_string(),
            format!("Enum {fqn} has PHPDoc @template tag{plural} but enums cannot be generic."),
            e.span,
            2,
            "enum.generic",
        );
    }

    /// Phase 66: the rest of real PHPStan's own `EnumSanityRule`
    /// (`vendor/phpstan-src/src/Rules/Classes/EnumSanityRule.php`) -
    /// `check_enum_case_values` above already covers its hardest sub-
    /// check (duplicate backing values). Every one of these is a real,
    /// unconditional PHP fatal error at declaration time (confirmed
    /// directly via `php -l`): a magic method other than `__call`/
    /// `__callStatic`/`__invoke`, redeclaring the native `cases()`/
    /// `from()`/`tryFrom()`, a backing type that isn't literally `int`/
    /// `string`, a value on a non-backed enum's case, a missing value on
    /// a backed one's, or a value whose own type doesn't match the
    /// declared backing type. `Serializable` is the one exception - PHP
    /// itself doesn't reject it at parse time, but real PHPStan still
    /// flags it as a design-level error (an enum has no identity to
    /// serialize/unserialize into).
    pub(crate) fn check_enum_sanity(&mut self, fqn: &str, e: &EnumDecl) {
        for m in &e.methods {
            let lower = m.function.name.to_ascii_lowercase();
            if is_magic_method_name(&lower) {
                if lower == "__construct" {
                    self.report_builder(
                        DiagnosticBuilder::new(
                            fqn.to_string(),
                            format!("Enum {fqn} contains constructor."),
                            m.span,
                            0,
                            "enum.constructor",
                        )
                        .non_ignorable(),
                    );
                } else if lower == "__destruct" {
                    self.report_builder(
                        DiagnosticBuilder::new(
                            fqn.to_string(),
                            format!("Enum {fqn} contains destructor."),
                            m.span,
                            0,
                            "enum.destructor",
                        )
                        .non_ignorable(),
                    );
                } else if !matches!(lower.as_str(), "__call" | "__callstatic" | "__invoke") {
                    self.report_builder(
                        DiagnosticBuilder::new(
                            fqn.to_string(),
                            format!("Enum {fqn} contains magic method {}().", m.function.name),
                            m.span,
                            0,
                            "enum.magicMethod",
                        )
                        .non_ignorable(),
                    );
                }
            }
            let redeclares_native = lower == "cases"
                || (e.backing_type.is_some() && matches!(lower.as_str(), "from" | "tryfrom"));
            if redeclares_native {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
                            "Enum {fqn} cannot redeclare native method {}().",
                            m.function.name
                        ),
                        m.span,
                        0,
                        "enum.methodRedeclaration",
                    )
                    .non_ignorable(),
                );
            }
        }

        if let Some(backing) = &e.backing_type {
            let expects_int = backing.eq_ignore_ascii_case("int");
            let expects_string = backing.eq_ignore_ascii_case("string");
            if !expects_int && !expects_string {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!("Backed enum {fqn} can have only \"int\" or \"string\" type."),
                        e.span,
                        0,
                        "enum.backingType",
                    )
                    .non_ignorable(),
                );
            }
        }

        if self.registry.is_subclass_of(fqn, "Serializable") {
            self.report_builder(
                DiagnosticBuilder::new(
                    fqn.to_string(),
                    format!("Enum {fqn} cannot implement the Serializable interface."),
                    e.span,
                    0,
                    "enum.serializable",
                )
                .non_ignorable(),
            );
        }

        let backing_type_word = e.backing_type.as_deref().map(|b| {
            if b.eq_ignore_ascii_case("int") {
                "int"
            } else {
                "string"
            }
        });
        for case in &e.cases {
            match (&e.backing_type, &case.value) {
                (None, Some(value)) => {
                    let ty = registry::infer_const_type(value);
                    self.report_builder(
                        DiagnosticBuilder::new(
                            fqn.to_string(),
                            format!(
                                "Enum {fqn} is not backed, but case {} has value {ty}.",
                                case.name
                            ),
                            case.span,
                            0,
                            "enum.caseWithValue",
                        )
                        .non_ignorable(),
                    );
                }
                (Some(_), None) => {
                    self.report_builder(
                        DiagnosticBuilder::new(
                            fqn.to_string(),
                            format!(
                                "Enum case {fqn}::{} does not have a value but the enum is backed with the \"{}\" type.",
                                case.name,
                                backing_type_word.unwrap_or("string")
                            ),
                            case.span,
                            0,
                            "enum.missingCase",
                        )
                        .non_ignorable(),
                    );
                }
                (Some(_), Some(value)) => {
                    let ty = registry::infer_const_type(value);
                    // `Mixed` means the value wasn't a literal this
                    // project's own const-value inference recognizes
                    // (e.g. `self::SOME_CONST`, a concatenation) -
                    // skipped rather than guessed, same "don't guess"
                    // leniency `check_enum_case_values` above already
                    // applies to duplicate-value detection.
                    let matches_backing = match ty {
                        Type::IntLiteral(_) => backing_type_word == Some("int"),
                        Type::StringLiteral(_) => backing_type_word == Some("string"),
                        Type::Mixed => true,
                        _ => false,
                    };
                    if !matches_backing {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                fqn.to_string(),
                                format!(
                                    "Enum case {fqn}::{} value {ty} does not match the \"{}\" type.",
                                    case.name,
                                    backing_type_word.unwrap_or("string")
                                ),
                                case.span,
                                0,
                                "enum.caseType",
                            )
                            .non_ignorable(),
                        );
                    }
                }
                (None, None) => {}
            }
        }
    }

    /// Finding 45: a concrete class must provide a real implementation of
    /// every abstract method it inherits (from a parent class, an
    /// implemented interface, or a directly-`use`d trait) - real PHP
    /// raises a fatal error at declaration time otherwise, independent of
    /// whether anything ever calls the missing method. Interfaces,
    /// traits, and abstract classes are themselves exempt (a class that
    /// legitimately leaves a method unimplemented is required to declare
    /// itself `abstract`, and one that does is not itself required to be
    /// complete).
    pub(crate) fn check_class_completeness(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        if info.kind != ClassKind::Class && info.kind != ClassKind::Enum {
            return;
        }
        if info.is_abstract {
            return;
        }
        let required = self.registry.required_abstract_methods(fqn);
        let missing: Vec<&String> = required
            .iter()
            .filter(|m| !self.registry.has_concrete_implementation(fqn, m))
            .collect();
        if missing.is_empty() {
            return;
        }
        let shown: Vec<&str> = missing.iter().take(3).map(|s| s.as_str()).collect();
        let mut message = format!(
            "class {fqn} does not implement abstract method(s): {}",
            shown.join(", ")
        );
        if missing.len() > shown.len() {
            message.push_str(&format!(", and {} more", missing.len() - shown.len()));
        }
        self.report_builder(
            DiagnosticBuilder::new(
                fqn.to_string(),
                message,
                span,
                0,
                "class.abstractMethodNotImplemented",
            )
            .tip(
                "either implement every method listed above, or declare this class itself abstract",
            ),
        );
    }

    /// Finding 50: an overriding method's signature must stay
    /// Liskov-compatible with the method it overrides - a parameter type
    /// may only be widened or kept the same (accepting more is safe),
    /// never narrowed; a return type may only be narrowed or kept the
    /// same (promising more is safe), never widened. Real PHP enforces
    /// this unconditionally at class-declaration time. Deliberately
    /// scoped to concrete parameter/return type mismatches only, not
    /// generic type-argument variance within an overridden signature -
    /// this project's own `is_assignable` is already lenient there by
    /// design (guessing wrong in either direction is worse than not
    /// checking), and that leniency is the right call here too.
    ///
    /// Also covers Finding 46's method-level half: overriding a method
    /// declared `final` anywhere in the ancestor chain, sharing this same
    /// per-method loop since both checks need the same "does this
    /// override collide with an ancestor's declaration" walk.
    /// `@phpstan-require-extends`/`@phpstan-require-implements`'s own
    /// consumer half - real PHPStan's `RequireExtendsRule`/
    /// `RequireImplementsRule`, transcribed (confirmed directly). For
    /// every interface/trait `fqn` transitively pulls in at all
    /// (`Registry::all_ancestor_names`, both `extends`/`implements` for
    /// interfaces and `uses` for traits - both real rules are exactly
    /// this transitive) that declares one of these tags, `fqn` itself
    /// must satisfy it (`is_subclass_of`, since both "extend" and
    /// "implement" here mean "be a subtype of," not "declare directly").
    pub(crate) fn check_require_tags(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        // Real PHPStan registers both consuming rules for `InClassNode`
        // only - a *trait* declaration gets its own separate `InTraitNode`
        // instead (confirmed directly, real PHPStan's own architecture),
        // so a trait is never itself "the checked class" here, only ever
        // an ancestor consulted for classes that use it. Without this, a
        // trait using another require-tagged trait would always wrongly
        // fail its own check - a trait can't `extends`/`implements`
        // anything at all, so `is_subclass_of`/membership would never be
        // satisfiable for one regardless of what any real using class
        // eventually does.
        if info.kind == ClassKind::Trait {
            return;
        }
        // Real PHP: a checked class that is itself an interface is
        // exempt from require-extends (an interface can't `extends` a
        // concrete class the way a class does) - confirmed directly,
        // `RequireExtendsRule`'s own top-level guard. Interfaces can't
        // `use` traits at all, so require-implements never has anything
        // to check for one either way - no separate guard needed for it.
        let is_interface = info.kind == ClassKind::Interface;
        for ancestor_fqn in self.registry.all_ancestor_names(fqn) {
            let Some(ancestor_info) = self.registry.classes.get(&ancestor_fqn) else {
                continue;
            };
            if !is_interface {
                for required in ancestor_info.require_extends.clone() {
                    if self.registry.is_subclass_of(fqn, &required) {
                        continue;
                    }
                    let (kind_word, relation_word) = if ancestor_info.kind == ClassKind::Trait {
                        ("Trait", "using")
                    } else {
                        ("Interface", "implementing")
                    };
                    self.report(
                        fqn.to_string(),
                        format!(
                            "{kind_word} {ancestor_fqn} requires {relation_word} class to extend {required}, but {fqn} does not."
                        ),
                        span,
                        0,
                        "class.missingExtends",
                    );
                }
            }
            for required in ancestor_info.require_implements.clone() {
                if self.registry.is_subclass_of(fqn, &required) {
                    continue;
                }
                self.report(
                    fqn.to_string(),
                    format!(
                        "Trait {ancestor_fqn} requires using class to implement {required}, but {fqn} does not."
                    ),
                    span,
                    0,
                    "class.missingImplements",
                );
            }
        }
    }

    /// `@phpstan-sealed`'s own consumer half - real PHPStan's
    /// `AllowedSubTypesRule`, transcribed (confirmed directly: this is
    /// the actual enforcement mechanism, not - as the tag's own name
    /// might suggest - a standalone "illegal extends" rule of its own;
    /// `@phpstan-sealed`'s own value feeds a generic `AllowedSubTypes
    /// ClassReflectionExtension` extension point this project doesn't
    /// otherwise need). Checked against *immediate* parents only (unlike
    /// `check_require_tags` above) - a sealed hierarchy's own allowlist
    /// enumerates the complete, direct child set, not every transitive
    /// descendant.
    ///
    /// Real PHPStan's own `getAllowedSubTypes` additionally allows a
    /// concrete (non-abstract) sealed class its own name for free, with
    /// no need to list itself - not modeled here, since that self-entry
    /// can only ever matter to this rule if the checked class were
    /// literally its own parent (impossible), confirmed directly by
    /// tracing through `AllowedSubTypesRule`'s own loop: the self-entry
    /// exists for `getAllowedSubTypes`'s *other* consumers (type-
    /// narrowing exhaustiveness over a sealed hierarchy, `ObjectType`/
    /// `StaticType`), a separate, much larger feature this project
    /// doesn't attempt.
    pub(crate) fn check_sealed_hierarchy(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        // Same `InClassNode`-only registration as `check_require_tags`
        // above - a trait is never itself the checked class. Real
        // PHPStan's own `AllowedSubTypesRule` also never consults `uses`
        // at all (only `extends`/immediate `implements`), so a trait
        // being sealed is never actually enforced there either way - not
        // a gap this exemption introduces on its own.
        if info.kind == ClassKind::Trait {
            return;
        }
        let kind = info.kind;
        for parent_fqn in info.extends.iter().chain(info.implements.iter()) {
            let Some(parent_info) = self.registry.classes.get(parent_fqn) else {
                continue;
            };
            if parent_info.sealed.is_empty() || parent_info.sealed.iter().any(|s| s == fqn) {
                continue;
            }
            self.report(
                fqn.to_string(),
                format!("Type {fqn} is not allowed to be a subtype of {parent_fqn}."),
                span,
                0,
                &format!("{}.disallowedSubtype", class_kind_identifier(&kind)),
            );
        }
    }

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

    /// Phase 64's own still-open property-override half - real PHPStan's
    /// `OverridingPropertyRule` equivalent, scoped to the three mismatch
    /// kinds answerable with fields `PropertyDecl`/`PropertySig` already
    /// carry (`is_static`/`is_readonly`/`visibility` - all three
    /// confirmed directly via `php -r` as real, unconditional fatal
    /// errors, in whichever direction is checked below; the *other*
    /// direction of each is real, legal PHP and deliberately left
    /// unflagged). Native-type-variance (`property.nativeType` - real
    /// PHP requires a property's own type to match its ancestor's
    /// *exactly*, invariantly, unlike a method's covariant return type)
    /// is deliberately not attempted here: `PropertySig` only carries a
    /// single, already-docblock-merged `ty` (no `return_type_native`-
    /// style split the way `FunctionSig` has for return types), and
    /// comparing that merged type directly would risk the identical
    /// false-positive class `method.childReturnType`'s own doc comment
    /// above already found and fixed for methods (an ancestor's own
    /// native type re-declared identically, but with an *added* `@var`
    /// docblock refinement on just one side) - needs a `PropertySig`
    /// split first, not attempted in this pass. `#[Override]`
    /// (`property.override`/`missingOverride`) and property hooks
    /// (`notReadable`/`notWritable`/`abstractOverridingNonAbstractHook`)
    /// are separate, still-open items too. A promoted constructor
    /// property is a real property for override purposes too (confirmed
    /// directly: `class Child extends Base { public function
    /// __construct(public readonly int $x) {} }` overriding a plain
    /// `Base::$x` still fatals the identical way) - checked via the
    /// shared `check_one_property_override` helper below, once per
    /// plain declaration and once per promoted `__construct` parameter.
    /// A promoted property is never `static` (no such syntax exists),
    /// which `own_is_static: false` encodes directly rather than
    /// guessing.
    /// Phase 43: PHP 8.4 property hooks - real PHPStan's
    /// `GetNonVirtualPropertyHookReadRule`/`SetNonVirtualPropertyHookAssignRule`
    /// (does a non-virtual hook actually touch the property's own backing
    /// storage), `SetPropertyHookParameterRule`'s native-type-contravariance
    /// half (see below for why only the native half, not its docblock-
    /// merged sibling), and `ExistingClassesInPropertyHookTypehintsRule`'s
    /// existence-check half for the `set` hook's own parameter type,
    /// combined into one per-property pass. Each hook's own body is also
    /// checked as a real function-like scope here - modeled directly on
    /// `checker/expr.rs`'s existing `ExprKind::Closure`/`ExprKind::ArrowFn`
    /// handling (fresh `locals`, explicit `$this` binding, generator-flag/
    /// goto-targets save-restore), NOT on `check_function`'s registry-
    /// `FunctionSig`-lookup path: that path runs a lot of method-specific
    /// machinery (attribute-target validation for *methods*, `#[Override]`/
    /// `#[NoDiscard]`, first-class-callable eligibility) that would be
    /// actively wrong for a hook, which - unlike a real method - is never
    /// itself callable as `$obj->get()`.
    ///
    /// Only plain property declarations (`c.properties`) are covered - a
    /// promoted constructor property can also carry its own inline hooks
    /// (`Param::hooks`), but nothing in this project's registry resolves
    /// `set_ty`/`is_virtual` for that shape yet either (confirmed:
    /// `property_hook_set_type` takes a `&PropertyDecl`, structurally
    /// unable to accept a `&Param`) - a real, disclosed residual gap, not
    /// attempted here (see ROADMAP.md's Phase 43 entry).
    pub(crate) fn check_property_hooks(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for p in &c.properties {
            let Some(hooks) = &p.hooks else {
                continue;
            };
            let Some(prop_sig) = info.properties.get(&p.name).cloned() else {
                continue;
            };
            if let Some(body) = &hooks.get {
                self.check_get_hook(fqn, p, body, &prop_sig);
            }
            if let Some((param, body)) = &hooks.set {
                self.check_set_hook(fqn, p, param.as_ref(), body, &prop_sig);
            }
        }
    }

    fn check_get_hook(
        &mut self,
        fqn: &str,
        p: &PropertyDecl,
        body: &PropertyHookBody,
        prop_sig: &PropertySig,
    ) {
        let label = format!("{fqn}::${} (get hook)", p.name);
        // `GetNonVirtualPropertyHookReadRule`: a non-virtual property's
        // `get` hook that never references its own backing storage at
        // all almost certainly forgot to (real PHP still compiles it -
        // it just always returns whatever the hook body itself computes,
        // silently never reading the real stored value).
        if !prop_sig.is_virtual && !registry::hook_body_references_property(body, &p.name) {
            self.report(
                fqn.to_string(),
                format!(
                    "Get hook for non-virtual property {fqn}::${} does not read its value.",
                    p.name
                ),
                p.span,
                3,
                "propertyGetHook.noRead",
            );
        }

        let previous_class = self.current_class.take();
        self.current_class = Some(fqn.to_string());
        let mut locals = Locals::default();
        locals.insert(
            "this".to_string(),
            Type::Object(fqn.to_string(), Vec::new()),
        );
        let declared_return = Some(prop_sig.ty.clone());
        let previous_generator_flag = self.current_function_is_generator;
        let previous_goto_targets = match body {
            PropertyHookBody::Arrow(e) => {
                self.current_function_is_generator = expr_contains_yield(e);
                None
            }
            PropertyHookBody::Block(stmts) => {
                self.current_function_is_generator = body_contains_yield(stmts);
                Some(std::mem::replace(
                    &mut self.current_function_goto_targets,
                    collect_goto_targets(stmts),
                ))
            }
        };
        self.check_generator_return_type(declared_return.as_ref(), p.span, &label);
        match body {
            PropertyHookBody::Arrow(e) => {
                let body_ty = self.check_expr(e, &mut locals, &label);
                if body_ty != Type::Mixed && !self.registry.is_assignable(&body_ty, &prop_sig.ty) {
                    self.report(
                        label.clone(),
                        format!(
                            "return type mismatch: expected {}, got {body_ty}",
                            prop_sig.ty
                        ),
                        e.span,
                        3,
                        "return.type",
                    );
                }
            }
            PropertyHookBody::Block(stmts) => {
                self.check_block(stmts, &mut locals, declared_return.as_ref(), &label);
                // Same "falls off the end with no return" completeness
                // check `check_function`'s own tail performs for a named
                // function/method - a `get` hook block body is exactly as
                // eligible for `return.missing` as an ordinary typed
                // method body is (real PHPStan runs `MissingReturnRule`
                // against a property hook the same as any other function-
                // like node).
                if !stmts.is_empty()
                    && !stmts.iter().any(|s| self.always_terminates(s, &locals))
                    && !self.current_function_is_generator
                    && prop_sig.ty != Type::Mixed
                {
                    self.report(
                        label.clone(),
                        format!(
                            "should return {} but return statement is missing",
                            prop_sig.ty
                        ),
                        p.span,
                        3,
                        "return.missing",
                    );
                }
            }
        }
        self.current_function_is_generator = previous_generator_flag;
        if let Some(previous) = previous_goto_targets {
            self.current_function_goto_targets = previous;
        }
        self.current_class = previous_class;
    }

    fn check_set_hook(
        &mut self,
        fqn: &str,
        p: &PropertyDecl,
        param: Option<&Param>,
        body: &PropertyHookBody,
        prop_sig: &PropertySig,
    ) {
        let label = format!("{fqn}::${} (set hook)", p.name);
        if !prop_sig.is_virtual && !registry::hook_body_references_property(body, &p.name) {
            self.report(
                fqn.to_string(),
                format!(
                    "Set hook for non-virtual property {fqn}::${} does not assign value to it.",
                    p.name
                ),
                p.span,
                3,
                "propertySetHook.noAssign",
            );
        }

        // `SetPropertyHookParameterRule`'s native-type-contravariance
        // half only - its own docblock-merged sibling
        // (`propertySetHook.parameterType`, real PHPStan's own
        // `checkPhpDocMethodSignatures`-gated check) is deliberately
        // skipped, mirroring this project's own established default for
        // override compatibility generally: real PHPStan's own flag
        // defaults off (confirmed neither Piwigo's own `phpstan.neon` nor
        // this project's corpus config enables it - see
        // `FunctionSig::return_type_native`'s own doc comment for the
        // identical reasoning already applied to method overrides).
        let param_has_native = param.is_some_and(|p| p.type_hint.is_some());
        let prop_has_native = p.type_hint.is_some();
        match (prop_has_native, param_has_native) {
            (false, true) => {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
                            "Parameter ${} of set hook has a native type but the property {fqn}::${} does not.",
                            param.map(|p| p.name.as_str()).unwrap_or("value"),
                            p.name
                        ),
                        p.span,
                        0,
                        "propertySetHook.nativeParameterType",
                    )
                    .non_ignorable(),
                );
            }
            (true, false) => {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
                            "Parameter ${} of set hook does not have a native type but the property {fqn}::${} does.",
                            param.map(|p| p.name.as_str()).unwrap_or("value"),
                            p.name
                        ),
                        p.span,
                        0,
                        "propertySetHook.nativeParameterType",
                    )
                    .non_ignorable(),
                );
            }
            (true, true) => {
                let param_ty = self.resolve_type_hint(param.and_then(|p| p.type_hint.as_deref()));
                let prop_native_ty = self.resolve_type_hint(p.type_hint.as_deref());
                if !self.registry.is_assignable(&prop_native_ty, &param_ty) {
                    self.report_builder(
                        DiagnosticBuilder::new(
                            fqn.to_string(),
                            format!(
                                "Native type {param_ty} of set hook parameter ${} is not contravariant with native type {prop_native_ty} of property {fqn}::${}.",
                                param.map(|p| p.name.as_str()).unwrap_or("value"),
                                p.name
                            ),
                            p.span,
                            0,
                            "propertySetHook.nativeParameterType",
                        )
                        .non_ignorable(),
                    );
                }
            }
            (false, false) => {}
        }

        // `ExistingClassesInPropertyHookTypehintsRule`'s existence half -
        // `prop_sig.set_ty` is already resolved (registry-build time),
        // same as every other `check_type_class_names_exist` call site in
        // this checker (`check_function`, Closure/ArrowFn params).
        if let Some(set_ty) = &prop_sig.set_ty {
            self.check_type_class_names_exist(
                set_ty,
                &format!(
                    "Parameter ${} of set hook for property {fqn}::${}",
                    param.map(|p| p.name.as_str()).unwrap_or("value"),
                    p.name
                ),
                p.span,
                &label,
            );
        }

        let previous_class = self.current_class.take();
        self.current_class = Some(fqn.to_string());
        let mut locals = Locals::default();
        locals.insert(
            "this".to_string(),
            Type::Object(fqn.to_string(), Vec::new()),
        );
        let param_name = param
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "value".to_string());
        let param_span = param.map(|p| p.name_span).unwrap_or(p.span);
        let value_ty = prop_sig
            .set_ty
            .clone()
            .unwrap_or_else(|| prop_sig.ty.clone());
        locals.declare(param_name, value_ty, param_span);
        let declared_return = Some(Type::Void);
        let previous_generator_flag = self.current_function_is_generator;
        let previous_goto_targets = match body {
            PropertyHookBody::Arrow(e) => {
                self.current_function_is_generator = expr_contains_yield(e);
                None
            }
            PropertyHookBody::Block(stmts) => {
                self.current_function_is_generator = body_contains_yield(stmts);
                Some(std::mem::replace(
                    &mut self.current_function_goto_targets,
                    collect_goto_targets(stmts),
                ))
            }
        };
        self.check_generator_return_type(declared_return.as_ref(), p.span, &label);
        match body {
            PropertyHookBody::Arrow(e) => {
                self.check_expr(e, &mut locals, &label);
            }
            PropertyHookBody::Block(stmts) => {
                self.check_block(stmts, &mut locals, declared_return.as_ref(), &label);
            }
        }
        self.current_function_is_generator = previous_generator_flag;
        if let Some(previous) = previous_goto_targets {
            self.current_function_goto_targets = previous;
        }
        self.current_class = previous_class;
    }

    pub(crate) fn check_property_overrides(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for p in &c.properties {
            let Some(own_sig) = info.properties.get(&p.name) else {
                continue;
            };
            self.check_one_property_override(
                fqn,
                &p.name,
                own_sig.is_static,
                own_sig.is_readonly,
                &p.visibility,
                p.span,
            );
        }
        for m in &c.methods {
            if m.function.name != "__construct" {
                continue;
            }
            for param in &m.function.params {
                let Some(visibility) = &param.promoted_visibility else {
                    continue;
                };
                self.check_one_property_override(
                    fqn,
                    &param.name,
                    false,
                    param.promoted_readonly,
                    visibility,
                    param.span,
                );
            }
        }
    }

    fn check_one_property_override(
        &mut self,
        fqn: &str,
        name: &str,
        own_is_static: bool,
        own_is_readonly: bool,
        own_visibility: &Visibility,
        span: Span,
    ) {
        let Some((ancestor_owner, ancestor_sig)) =
            self.registry.lookup_ancestor_property_with_owner(fqn, name)
        else {
            return;
        };
        // A `private` ancestor property is an unrelated declaration, not
        // a real override target - same reasoning as `check_method_
        // overrides`'s identical exemption.
        if ancestor_sig.visibility == Visibility::Private {
            return;
        }
        if ancestor_sig.is_static && !own_is_static {
            self.report(
                fqn.to_string(),
                format!(
                    "Non-static property {fqn}::${name} overrides static property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.nonStatic",
            );
        } else if !ancestor_sig.is_static && own_is_static {
            self.report(
                fqn.to_string(),
                format!(
                    "Static property {fqn}::${name} overrides non-static property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.static",
            );
        }
        if ancestor_sig.is_readonly && !own_is_readonly {
            self.report(
                fqn.to_string(),
                format!(
                    "Readwrite property {fqn}::${name} overrides readonly property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.readWrite",
            );
        } else if !ancestor_sig.is_readonly && own_is_readonly {
            self.report(
                fqn.to_string(),
                format!(
                    "Readonly property {fqn}::${name} overrides readwrite property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.readOnly",
            );
        }
        // Same two-way visibility-narrowing split `check_method_
        // overrides` already uses, one member kind over.
        let narrows_visibility = match ancestor_sig.visibility {
            Visibility::Public => *own_visibility != Visibility::Public,
            Visibility::Protected => *own_visibility == Visibility::Private,
            Visibility::Private => false,
        };
        if narrows_visibility {
            self.report(
                fqn.to_string(),
                format!(
                    "{fqn}::${name} overriding {} property {ancestor_owner}::${name} must not reduce visibility",
                    visibility_word(&ancestor_sig.visibility)
                ),
                span,
                0,
                "property.visibility",
            );
        }
    }
}

fn visibility_word(v: &Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::Private => "private",
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

/// Finding 58's own notion of "two trait methods with the same name are
/// genuinely incompatible, not just written slightly differently" -
/// deliberately narrow (exact param count and exact per-position/return
/// type equality) rather than a real Liskov-style compatibility check:
/// this is comparing two *unrelated* trait declarations with no
/// inheritance relationship between them at all, so there's no
/// contravariance/covariance direction to apply - only "are these the
/// same signature" makes sense here.
pub(crate) fn signatures_compatible(a: &FunctionSig, b: &FunctionSig) -> bool {
    if a.params.len() != b.params.len() {
        return false;
    }
    if a.return_type != b.return_type {
        return false;
    }
    a.params
        .iter()
        .zip(&b.params)
        .all(|(pa, pb)| pa.ty == pb.ty)
}
