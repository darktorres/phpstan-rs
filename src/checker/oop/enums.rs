use rustc_hash::FxHashMap;

use crate::ast::*;
use crate::checker::{Checker, DiagnosticBuilder};
use crate::docblock;
use crate::registry::{self};
use crate::types::Type;

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

impl<'a> Checker<'a> {
    /// Every case of a backed enum must have a unique backing
    /// value - a real, unconditional PHP fatal error at declaration time
    /// otherwise ("Duplicate value in enum ... for case ..."), not merely
    /// a design smell. Only compares cases whose value is a literal
    /// int/string (the overwhelming majority in practice) - a computed or
    /// otherwise non-literal case value isn't evaluated, matching this
    /// project's usual "don't guess" leniency.
    pub(crate) fn check_enum_case_values(&mut self, fqn: &str, e: &EnumDecl) {
        let mut seen: FxHashMap<(bool, String), &str> = FxHashMap::default();
        for case in &e.cases {
            let Some(value) = &case.value else {
                continue;
            };
            let key = match &value.kind {
                ExprKind::Literal(Literal::Int(n)) => (true, n.to_string()),
                ExprKind::Literal(Literal::String(s)) => (false, s.to_string()),
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

    /// `EnumTemplateTypeRule` - enums can't be generic in real
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
                        Type::Mixed(_) => true,
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
}
