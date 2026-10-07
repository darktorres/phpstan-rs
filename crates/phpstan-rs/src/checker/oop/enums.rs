use rustc_hash::FxHashMap;

use crate::checker::Checker;
use crate::registry::{self};
use diagnostics::DiagnosticBuilder;
use phpdoc::docblock;
use syntax::ast::*;

/// The full set of PHP magic-method names (`nikic/php-parser`'s own
/// `ClassMethod::isMagic()` list) - used only by `check_enum_sanity`
/// below to tell a real magic method apart from an ordinary one that
/// merely starts with `__`.
pub(crate) fn is_magic_method_name(lower: &str) -> bool {
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

/// Whether `name` is a method PHP gives `e` natively when `e` is an enum:
/// `cases()`, and a backed enum's `from()` and `tryFrom()`.
pub(crate) fn is_native_enum_method(e: &ClassDef, name: &str) -> bool {
    e.kind == ClassKind::Enum
        && (name.eq_ignore_ascii_case("cases")
            || (e.backing_type.is_some()
                && (name.eq_ignore_ascii_case("from") || name.eq_ignore_ascii_case("tryFrom"))))
}

impl<'a> Checker<'a> {
    /// `EnumCaseOutsideEnumRule`: a `case` in a class, interface, trait
    /// or anonymous class body - PHP parses it, then refuses to compile it.
    pub(crate) fn check_enum_cases_outside_enum(&mut self, cases: &[EnumCase]) {
        for case in cases {
            self.report_builder(
                DiagnosticBuilder::new(
                    "Enum case can only be used in enums.".to_string(),
                    case.span,
                    0,
                    "enum.caseOutsideOfEnum",
                )
                .non_ignorable(),
            );
        }
    }

    /// Every case of a backed enum must have a unique backing
    /// value - a real, unconditional PHP fatal error at declaration time
    /// otherwise ("Duplicate value in enum ... for case ..."), not merely
    /// a design smell. Only compares cases whose value is a literal
    /// int/string (the overwhelming majority in practice) - a computed or
    /// otherwise non-literal case value isn't evaluated, matching this
    /// project's usual "don't guess" leniency.
    pub(crate) fn check_enum_case_values(&mut self, fqn: &str, e: &ClassDef) {
        let Some(owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = owner.arenas();
        let mut seen: FxHashMap<(bool, String), &str> = FxHashMap::default();
        for case in e.enum_cases(arenas) {
            let Some(value_id) = case.value else {
                continue;
            };
            let Some(value) = arenas.expr(value_id) else {
                continue;
            };
            let key = match &value.kind {
                ExprKind::Literal(Literal::Int(n)) => (true, n.to_string()),
                ExprKind::Literal(Literal::String(s)) => {
                    let Some(bytes) = arenas.string_bytes(s) else {
                        continue;
                    };
                    (false, String::from_utf8_lossy(bytes).into_owned())
                }
                _ => continue,
            };
            if let Some(first_case) = seen.get(&key) {
                self.report(format!(
                        "duplicate value in enum {fqn} for case {} (already used by case {first_case})",
                        case.name.as_ref()
                    ), case.span, 0, "enum.duplicateValue");
            } else {
                seen.insert(key, case.name.as_ref());
            }
        }
    }

    /// `EnumTemplateTypeRule` - enums can't be generic in real
    /// PHP (there's no `enum Foo<T>` syntax at all), so a stray
    /// `@template` tag on one's own doc comment is always a mistake,
    /// never a real declaration the way it would be on a class.
    pub(crate) fn check_enum_template_tag(&mut self, fqn: &str, e: &ClassDef) {
        let source = self.syntax_source_owner();
        let Some(raw) = e
            .doc_comment
            .as_ref()
            .and_then(|ranges| source.as_ref().and_then(|source| ranges.text(source)))
        else {
            return;
        };
        let block = docblock::parse_shared(raw.as_ref());
        if block.templates.is_empty() {
            return;
        }
        let plural = if block.templates.len() == 1 { "" } else { "s" };
        self.report(
            format!("Enum {fqn} has PHPDoc @template tag{plural} but enums cannot be generic."),
            e.span,
            2,
            "enum.generic",
        );
    }

    /// The rest of real PHPStan's own `EnumSanityRule`
    /// (`vendor/phpstan-src/src/Rules/Classes/EnumSanityRule.php`) -
    /// `check_enum_case_values` above already covers its hardest sub-
    /// check (duplicate backing values). Every one of these is a real,
    /// unconditional PHP fatal error at declaration time: a magic
    /// method other than `__call`/
    /// `__callStatic`/`__invoke`, redeclaring the native `cases()`/
    /// `from()`/`tryFrom()`, a backing type that isn't literally `int`/
    /// `string`, a value on a non-backed enum's case, a missing value on
    /// a backed one's, or a value whose own type doesn't match the
    /// declared backing type. `Serializable` is the one exception - PHP
    /// itself doesn't reject it at parse time, but real PHPStan still
    /// flags it as a design-level error (an enum has no identity to
    /// serialize/unserialize into). A property is a compile error too.
    pub(crate) fn check_enum_sanity(&mut self, fqn: &str, e: &ClassDecl) {
        let Some(owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = owner.arenas();
        let Some(def) = arenas.class(e.def) else {
            return;
        };
        if let Some(property) = def.properties(arenas).first() {
            self.report_builder(
                DiagnosticBuilder::new(
                    format!("Enum {fqn} cannot include properties."),
                    property.span,
                    0,
                    "enum.property",
                )
                .non_ignorable(),
            );
        }
        for m in arenas.methods(def.methods.clone()) {
            let lower = m.function.name.as_ref().to_ascii_lowercase();
            if is_magic_method_name(&lower) {
                if lower == "__construct" {
                    self.report_builder(
                        DiagnosticBuilder::new(
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
                            format!("Enum {fqn} contains magic method {}().", m.function.name),
                            m.span,
                            0,
                            "enum.magicMethod",
                        )
                        .non_ignorable(),
                    );
                }
            }
            if is_native_enum_method(def, m.function.name.as_ref()) {
                self.report_builder(
                    DiagnosticBuilder::new(
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

        if let Some(backing) = &def.backing_type {
            let expects_int = matches!(backing.kind, TypeHintKind::Keyword(NativeType::Int, _));
            let expects_string =
                matches!(backing.kind, TypeHintKind::Keyword(NativeType::String, _));
            if !expects_int && !expects_string {
                self.report_builder(
                    DiagnosticBuilder::new(
                        format!("Backed enum {fqn} can have only \"int\" or \"string\" type."),
                        def.span,
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
                    format!("Enum {fqn} cannot implement the Serializable interface."),
                    def.span,
                    0,
                    "enum.serializable",
                )
                .non_ignorable(),
            );
        }

        let backing_type_word = def.backing_type.as_ref().map(|b| {
            if matches!(b.kind, TypeHintKind::Keyword(NativeType::Int, _)) {
                "int"
            } else {
                "string"
            }
        });
        for case in def.enum_cases(arenas) {
            match (&def.backing_type, &case.value) {
                (None, Some(value)) => {
                    let ty = registry::infer_const_type(arenas, *value);
                    self.report_builder(
                        DiagnosticBuilder::new(
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
                        DiagnosticBuilder::new(format!(
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
                    let ty = registry::infer_const_type(arenas, *value);
                    // `Mixed` means the value wasn't a literal this
                    // project's own const-value inference recognizes
                    // (e.g. `self::SOME_CONST`, a concatenation) -
                    // skipped rather than guessed, same "don't guess"
                    // leniency `check_enum_case_values` above already
                    // applies to duplicate-value detection.
                    let matches_backing = if ty.int_literal_value().is_some() {
                        backing_type_word == Some("int")
                    } else if ty.string_literal_value().is_some() {
                        backing_type_word == Some("string")
                    } else {
                        ty.is_mixed()
                    };
                    if !matches_backing {
                        self.report_builder(
                            DiagnosticBuilder::new(format!(
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
}
