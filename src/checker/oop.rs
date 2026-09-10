use std::collections::HashMap;

use crate::ast::*;
use crate::checker::{Checker, DiagnosticBuilder, Locals, class_kind_identifier};
use crate::docblock;
use crate::registry::{FunctionSig, PropertySig};
use crate::token::Span;
use crate::types::Type;

mod attributes;
mod enums;
mod method_overrides;
mod property_hooks;
mod property_overrides;

pub(crate) use attributes::{
    ATTRIBUTE_TARGET_CLASS, ATTRIBUTE_TARGET_CLASS_CONSTANT, ATTRIBUTE_TARGET_FUNCTION,
    ATTRIBUTE_TARGET_METHOD, ATTRIBUTE_TARGET_PARAMETER, ATTRIBUTE_TARGET_PROPERTY,
};

impl<'a> Checker<'a> {
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
}

pub(crate) fn visibility_word(v: &Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::Private => "private",
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
