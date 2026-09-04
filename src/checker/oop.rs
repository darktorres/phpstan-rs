use std::collections::HashMap;

use crate::ast::*;
use crate::checker::{Checker, DiagnosticBuilder};
use crate::docblock;
use crate::registry::{self, FunctionSig, PropertySig};
use crate::token::Span;
use crate::types::Type;

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
    /// Phase 37: the existence half of real PHPStan's own `AttributesCheck`
    /// (`vendor/phpstan-src/src/Rules/AttributesCheck.php`) - every
    /// `#[Name(...)]` group's `Name` must resolve to a real, known class,
    /// the same `attribute.notFound` identifier real PHPStan's own rule
    /// uses. Wired at every declaration site an `Attribute` list can now
    /// appear on (class/enum, method/function, property, const, enum
    /// case, param, closure/arrow-fn - see each call site).
    ///
    /// Deliberately narrower than real PHPStan's own check: whether the
    /// resolved class is actually usable as an attribute at all (declares
    /// its own `#[Attribute(...)]`, i.e. `attribute.notAttribute`),
    /// whether its declared `Attribute::TARGET_*` flags match where this
    /// one is actually used (`attribute.target`), and constructor-
    /// argument validation (`attribute.noConstructor`/parameter-count/
    /// type checks - genuinely needs Phase 49's call-argument-binding
    /// generalization, named explicitly in this phase's own audit notes)
    /// are all left for a later pass - this project's own registry
    /// doesn't yet track which classes are attribute classes or what
    /// targets they declare, and guessing wrong here (silently assuming
    /// every class qualifies) would be strictly worse than not checking
    /// at all.
    pub(crate) fn check_attributes(&mut self, attrs: &[Attribute], label: &str) {
        for attr in attrs {
            let fqn = self.name_ctx.resolve(&attr.name);
            if !self.registry.classes.contains_key(&fqn) {
                self.report(
                    label.to_string(),
                    format!("attribute class {fqn} does not exist"),
                    attr.span,
                    0,
                    "attribute.notFound",
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
    pub(crate) fn check_method_overrides(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for m in &c.methods {
            // Finding 46: overriding a `final` method is a real,
            // unconditional fatal error in PHP - including for
            // __construct, unlike the signature-compatibility checks
            // below, so this runs before that exemption.
            if let Some(ancestor) = self
                .registry
                .find_final_ancestor_method(fqn, &m.function.name)
            {
                self.report(
                    fqn.to_string(),
                    format!(
                        "{fqn}::{}() cannot override final method {ancestor}::{}()",
                        m.function.name, m.function.name
                    ),
                    m.span,
                    0,
                    "method.overrideFinal",
                );
            }
            // Real PHP deliberately exempts constructors from override-
            // compatibility checking - a subclass's own __construct is
            // allowed an entirely different, otherwise-incompatible
            // signature from its parent's, unlike every other method.
            if m.function.name == "__construct" {
                continue;
            }
            let Some(own_sig) = info.methods.get(&m.function.name) else {
                continue;
            };
            let Some((ancestor_owner, ancestor_sig)) = self
                .registry
                .lookup_ancestor_method_with_owner(fqn, &m.function.name)
            else {
                continue;
            };
            // A `private` ancestor method isn't a real override target at
            // all - real PHP's own method dispatch is never virtual for
            // `private` (a same-named method on a subclass is an
            // unrelated declaration, not a polymorphic override), so
            // nothing about compatibility applies. `lookup_ancestor_
            // method_with_owner` itself doesn't filter by visibility (it
            // answers "is there a same-named declaration up the chain",
            // the right question for the visibility-narrowing check just
            // below, which needs to see a `private` ancestor to *know*
            // it's not a real override) - this exemption belongs here,
            // not there.
            if ancestor_sig.visibility == Visibility::Private {
                continue;
            }
            // Phase 64: narrowing a method's own visibility on override
            // is a real, unconditional PHP fatal error (confirmed
            // directly: `Access level to Child::foo() must be public (as
            // in class Base)`) - matches real PHPStan's own
            // `MethodVisibilityComparisonHelper` exactly (same
            // identifier, same two-way split: an ancestor's `public`
            // demands `public`; an ancestor's `protected` only forbids
            // narrowing all the way to `private`).
            let narrows_visibility = match ancestor_sig.visibility {
                Visibility::Public => m.visibility != Visibility::Public,
                Visibility::Protected => m.visibility == Visibility::Private,
                Visibility::Private => false,
            };
            if narrows_visibility {
                self.report(
                    fqn.to_string(),
                    format!(
                        "{fqn}::{}() overriding {} method {ancestor_owner}::{}() must not reduce visibility",
                        m.function.name,
                        visibility_word(&ancestor_sig.visibility),
                        m.function.name
                    ),
                    m.span,
                    0,
                    "method.visibility",
                );
            }
            for (i, ancestor_param) in ancestor_sig.params.iter().enumerate() {
                let Some(own_param) = own_sig.params.get(i) else {
                    // Real PHP fatal error - the override doesn't declare
                    // this many parameters at all. Skipped for a
                    // variadic ancestor tail (`...$args`, matched at any
                    // position, not just the last - real PHP itself
                    // requires it to be the last declared parameter, so
                    // an earlier one is already a parse-time error this
                    // project's own parser presumably rejects too): real
                    // PHPStan's own handling of that shape is
                    // considerably more involved (contravariance across
                    // every trailing ancestor parameter the variadic
                    // swallows) than this project attempts here.
                    if !ancestor_param.variadic {
                        self.report(
                            fqn.to_string(),
                            format!(
                                "{fqn}::{}() overrides {ancestor_owner}::{}() but misses parameter #{} (${})",
                                m.function.name,
                                m.function.name,
                                i + 1,
                                ancestor_param.name
                            ),
                            m.span,
                            0,
                            "parameter.missing",
                        );
                    }
                    continue;
                };
                // A parameter the ancestor made optional (has a default)
                // can never become *required* on override - a caller
                // holding the ancestor's own type could always omit it.
                // Real, unconditional PHP fatal error (confirmed
                // directly). Not gated on either side's type being
                // `Mixed` the way the narrowing check below is - this is
                // about arity/callability, not type compatibility.
                if ancestor_param.has_default && !own_param.has_default && !own_param.variadic {
                    self.report(
                        fqn.to_string(),
                        format!(
                            "Parameter #{} (${}) of {fqn}::{}() is required but parameter #{} (${}) of {ancestor_owner}::{}() is optional",
                            i + 1,
                            own_param.name,
                            m.function.name,
                            i + 1,
                            ancestor_param.name,
                            m.function.name
                        ),
                        m.span,
                        0,
                        "parameter.notOptional",
                    );
                }
                // `Type::Mixed` is overloaded throughout this project to
                // mean both real PHP `mixed` *and* "no type declared at
                // all, native or docblock" (see this file's own opening
                // comment) - and real PHP explicitly does NOT enforce
                // override compatibility for a parameter/return position
                // that isn't typed at all (only an explicitly *narrower*
                // declared type is a real fatal error). Since an
                // intentional, explicit `mixed` on an override is rare
                // next to "just wasn't declared", skipping either side
                // being Mixed avoids flagging the overwhelmingly common
                // case (confirmed via vendor/phpstorm-stubs: dozens of
                // real stub methods simply don't redeclare an inherited
                // native return type) at the cost of missing the rare
                // genuine `mixed`-narrows-a-real-type case - the same
                // "don't guess wrong on Mixed" tradeoff `is_assignable`
                // already makes elsewhere in this project.
                if own_param.ty == Type::Mixed || ancestor_param.ty == Type::Mixed {
                    continue;
                }
                // A bare `self`/`static` in either signature means "an
                // instance of the class that actually declared it", not
                // some shared, comparable-by-name sentinel - `self` in
                // `fqn`'s own override and `self` in `ancestor_owner`'s
                // declaration usually name two *different* concrete
                // classes. `is_assignable`/`is_subtype` have no rule at
                // all for a concrete `Object` satisfying a still-bare
                // `SelfRef` (only the reverse - `self` satisfying a
                // concrete class - is handled), so a real, correct
                // override typing its own parameter `self` (a `final`
                // class implementing an interface that (redundantly, but
                // validly) re-declares the same concrete class by name)
                // was flagged as narrowing what's actually the exact
                // same type. Resolving both sides first, same as any
                // other pseudo-type use, fixes this without weakening
                // the check for a genuine narrowing.
                let own_ty = self.resolve_pseudo(&own_param.ty, fqn);
                let ancestor_ty = self.resolve_pseudo(&ancestor_param.ty, &ancestor_owner);
                if !self.registry.is_assignable(&ancestor_ty, &own_ty) {
                    self.report(fqn.to_string(), format!(
                            "{fqn}::{}() parameter #{} (${}) narrows the inherited parameter type: expects {}, ancestor expects {}",
                            m.function.name,
                            i + 1,
                            own_param.name,
                            own_ty,
                            ancestor_ty
                        ), m.span, 0, "method.childParameterType");
                }
            }
            // An ancestor's own `static`-typed return (`self.registry`
            // has no way to resolve what `static` meant relative to the
            // *ancestor's* declaring class, only the checker's own
            // current-class-aware `resolve_pseudo`, which isn't
            // available here) is real PHP's own explicit "late static
            // binding" covariance escape hatch - a subclass narrowing it
            // to its own concrete class name (the overwhelmingly common
            // real shape, confirmed via vendor/phpstorm-stubs'
            // `Ds\Collection::copy(): static` and its own subclasses'
            // `copy(): Vector`/`copy(): Stack`/...) is always exactly
            // what real PHP allows, never a real incompatibility.
            // Checked at the *native* level only (ignoring any `@return`
            // docblock refinement on either side) - real PHP itself only
            // ever enforces override-compatibility on the native
            // signature, and by default so does real PHPStan
            // (`checkPhpDocMethodSignatures` is off unless a project
            // explicitly opts in - confirmed neither Piwigo's own corpus
            // config nor this project's own does). Using the merged,
            // docblock-aware `return_type` here wrongly flagged every
            // override that faithfully keeps the ancestor's own native
            // `array` while the ancestor's *interface* had separately
            // narrowed it to `list<...>` via docblock alone - a real
            // corpus false positive (several `HtmlRenderingInterface`
            // test fakes each declaring `getCatBreadcrumb(): array`,
            // exactly matching the interface's own native signature).
            if own_sig.return_type_native != Type::Mixed
                && ancestor_sig.return_type_native != Type::Mixed
                && !ancestor_return_uses_static(&ancestor_sig.return_type_native)
                && !self.registry.is_assignable(
                    &own_sig.return_type_native,
                    &ancestor_sig.return_type_native,
                )
            {
                self.report(fqn.to_string(), format!(
                        "{fqn}::{}() return type {} is not compatible with the inherited return type {}",
                        m.function.name, own_sig.return_type, ancestor_sig.return_type
                    ), m.span, 0, "method.childReturnType");
            }
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
