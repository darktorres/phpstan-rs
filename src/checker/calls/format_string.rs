use crate::ast::*;
use crate::checker::{Checker, Locals, literal_string_value};
use crate::php_version::{PhpFeature, PhpVersion};
use crate::token::Span;
use crate::types::Type;

impl<'a> Checker<'a> {
    /// Validates a `sprintf`/`printf`/`fprintf`/`vsprintf`/`vprintf`
    /// call's format string against the arguments actually passed - only
    /// when the format string is a
    /// literal (mirrors `check_shape_key_access`'s own "only a literal,
    /// never a dynamic value" precedent). `fprintf`'s format string is
    /// its *second* argument (`fprintf(resource $stream, string
    /// $format, mixed ...$values)`), not its first - otherwise identical
    /// to `sprintf`/`printf`. `vsprintf`/`vprintf` take their values as
    /// one array argument rather than variadically - only checked when
    /// that argument is itself a literal (or otherwise a real
    /// `ArrayShape` with sequential canonical-int keys - a plain list
    /// literal in every way that matters here), the same "don't guess
    /// past what's statically known" restriction the format string
    /// itself already has. `sprintf`/`printf`/`vsprintf`/`vprintf`
    /// require an *exact* placeholder/argument count match (real
    /// PHPStan's own `PrintfParametersRule`/`PrintfArrayParametersRule`,
    /// both unconditional at level 0); `fprintf` has no real-PHPStan
    /// rule at all backing it - its support here is this project's own
    /// extension, kept deliberately conservative at "too few only".
    pub(crate) fn check_format_string_call(
        &mut self,
        name: &str,
        args: &[Arg],
        locals: &Locals,
        label: &str,
    ) {
        let fmt_idx = usize::from(name == "fprintf");
        let Some(fmt_expr) = args.get(fmt_idx) else {
            return;
        };
        let Some(fmt) = literal_string_value(&fmt_expr.value) else {
            return;
        };
        if matches!(name, "vsprintf" | "vprintf") {
            let Some(values_arg) = args.get(fmt_idx + 1) else {
                return;
            };
            let values_ty = self.infer_expr(&values_arg.value, locals);
            let Type::ArrayShape(fields) = &values_ty else {
                return;
            };
            if !crate::types::shape_fields_are_list_like(fields) {
                return;
            }
            let provided: Vec<(Type, Span)> = fields
                .iter()
                .map(|(_, ty, _)| (ty.clone(), values_arg.value.span))
                .collect();
            self.check_format_placeholders(name, &fmt, fmt_expr.value.span, &provided, label);
            return;
        }
        let provided: Vec<(Type, Span)> = args[fmt_idx + 1..]
            .iter()
            .map(|a| (self.infer_expr(&a.value, locals), a.value.span))
            .collect();
        self.check_format_placeholders(name, &fmt, fmt_expr.value.span, &provided, label);
    }

    /// The shared placeholder-vs-argument-type engine both call shapes
    /// `check_format_string_call` recognizes feed into - `provided` is
    /// already-resolved (type, span-to-report-at) pairs, so this itself
    /// doesn't care whether they came from real positional call
    /// arguments or a `vsprintf`/`vprintf` array literal's own fields.
    /// Tries a purely-sequential parse first (`parse_format_specifiers`),
    /// then a purely-positional one (`parse_positional_format_specifiers`).
    /// A format string mixing the two styles satisfies neither and is
    /// silently skipped, the same "don't guess" leniency each parser
    /// already has on its own.
    fn check_format_placeholders(
        &mut self,
        name: &str,
        fmt: &str,
        fmt_span: Span,
        provided: &[(Type, Span)],
        label: &str,
    ) {
        let exact_match_required = name != "fprintf";
        let php_version = self.registry.php_version;
        let strict = self.registry.check_strict_printf_placeholder_types;
        if let Some(slots) = parse_format_specifiers(fmt, php_version) {
            let mismatched = if exact_match_required {
                slots.len() != provided.len()
            } else {
                slots.len() > provided.len()
            };
            if mismatched {
                self.report(
                    label.to_string(),
                    format!(
                        "{name}() format string expects {} argument(s), {} given",
                        slots.len(),
                        provided.len()
                    ),
                    fmt_span,
                    0,
                    "arguments.count",
                );
                return;
            }
            for (slot, (arg_ty, span)) in slots.iter().zip(provided) {
                match slot {
                    FormatSlot::Width | FormatSlot::Precision => {
                        if !is_strict_int(arg_ty) {
                            let which = if matches!(slot, FormatSlot::Width) {
                                "width"
                            } else {
                                "precision"
                            };
                            self.report(
                                label.to_string(),
                                format!(
                                    "{name}() format specifier's {which} (\"*\") expects int, {arg_ty} given"
                                ),
                                *span,
                                5,
                                "argument.type",
                            );
                        }
                    }
                    FormatSlot::Value(spec) => {
                        if !format_specifier_accepts(*spec, arg_ty, strict) {
                            self.report(
                                label.to_string(),
                                format!(
                                    "{name}() format specifier %{spec} does not accept {arg_ty}"
                                ),
                                *span,
                                5,
                                "argument.type",
                            );
                        }
                    }
                }
            }
            return;
        }
        let Some(by_position) = parse_positional_format_specifiers(fmt, php_version) else {
            return;
        };
        let mismatched = if exact_match_required {
            by_position.len() != provided.len()
        } else {
            by_position.len() > provided.len()
        };
        if mismatched {
            self.report(
                label.to_string(),
                format!(
                    "{name}() format string expects {} argument(s), {} given",
                    by_position.len(),
                    provided.len()
                ),
                fmt_span,
                0,
                "arguments.count",
            );
            return;
        }
        for (specs, (arg_ty, span)) in by_position.iter().zip(provided) {
            for spec in specs {
                if !format_specifier_accepts(*spec, arg_ty, strict) {
                    self.report(
                        label.to_string(),
                        format!("{name}() format specifier %{spec} does not accept {arg_ty}"),
                        *span,
                        5,
                        "argument.type",
                    );
                }
            }
        }
    }
}

/// Parses a `sprintf`/`printf`-style format string into its ordered list
/// of conversion specifiers (`%d`, `%s`, ...), skipping `%%` (a literal
/// percent, not a placeholder). Returns `None` - "don't know, don't
/// guess" - for anything this doesn't confidently understand: a
/// positional specifier (`%1$s`, real and common but out of scope for
/// this pass, since a lone specifier count no longer maps 1:1 to argument
/// count once positions can repeat or skip), an unknown specifier
/// character, or a malformed/truncated `%` sequence. `%b`/`%c`/`%d`/`%e`/
/// `%E`/`%f`/`%F`/`%g`/`%G`/`%o`/`%s`/`%u`/`%x`/`%X` are real PHP
/// specifier characters. `%h`/`%H` (locale-independent decimal-point
/// float formatting) are real too, but only from PHP 8.0 on
/// (`PhpFeature::PrintfHhSpecifier`) - accepted unless a configured
/// target version is confidently older, matching this project's "no
/// target configured, don't restrict" convention.
/// One slot `parse_format_specifiers` consumes an argument for, in the
/// order it appears in the format string. `Width`/`Precision` come from
/// a literal `*` sub-placeholder - real, if undocumented, PHP:
/// `sprintf("%*d", 5, 42)` pads to width 5, consuming its own argument
/// before the specifier's own value argument; `sprintf("%.*f", 2, pi)`
/// does the same for precision (`%*.*f` combines both).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormatSlot {
    Width,
    Precision,
    Value(char),
}

pub(crate) fn parse_format_specifiers(
    fmt: &str,
    php_version: Option<PhpVersion>,
) -> Option<Vec<FormatSlot>> {
    let hh_allowed = php_version.is_none_or(|v| PhpFeature::PrintfHhSpecifier.is_available(v));
    let bytes = fmt.as_bytes();
    let mut specs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        let &next = bytes.get(i)?;
        if next == b'%' {
            i += 1;
            continue;
        }
        // A positional prefix (`%1$s`) is digits immediately followed by
        // `$` - checked before flags/width, since a bare width (`%5d`)
        // is also just digits and must NOT be mistaken for one.
        let digits_start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'$') {
            return None;
        }
        i = digits_start;
        while bytes
            .get(i)
            .is_some_and(|b| matches!(b, b'-' | b'+' | b' ' | b'0'))
        {
            i += 1;
        }
        if bytes.get(i) == Some(&b'\'') {
            i += 2; // custom padding char: the quote plus the pad character itself
        }
        if bytes.get(i) == Some(&b'*') {
            specs.push(FormatSlot::Width);
            i += 1;
        } else {
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        if bytes.get(i) == Some(&b'.') {
            i += 1;
            if bytes.get(i) == Some(&b'*') {
                specs.push(FormatSlot::Precision);
                i += 1;
            } else {
                while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                }
            }
        }
        let &spec = bytes.get(i)?;
        if !b"bcdeEfFgGosuxX".contains(&spec) && !(hh_allowed && matches!(spec, b'h' | b'H')) {
            return None;
        }
        specs.push(FormatSlot::Value(spec as char));
        i += 1;
    }
    Some(specs)
}

/// Whether `ty` is genuinely an `int` - used for a `*` width/precision
/// sub-placeholder, which real PHP always requires a real int for
/// (`PrintfPlaceholder`'s own `'strict-int'` acceptance, unconditional
/// regardless of the castable-vs-strict placeholder-type mode).
fn is_strict_int(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::IntLiteral(_))
}

/// Same job as `parse_format_specifiers` just above, but for a format
/// string using only positional specifiers (`%1$s`, `%2$d`, ...) - real,
/// common PHP for reordering arguments in localized message strings.
/// Returns `None` on anything not confidently understood, crucially
/// including a format string that mixes a positional specifier with an
/// ordinary, unnumbered one. Real PHP's interaction between the two is
/// genuinely quirky (`sprintf('%1$s-%s-%2$s', 'a', 'b', 'c')` produces
/// `"a-a-b"`, not `"a-b-c"` - the unnumbered specifier does not pick up
/// wherever the positional ones left off), quirky enough that guessing
/// at it risks a wrong answer worse than staying silent.
///
/// Returns one entry per 0-based argument *position* referenced by
/// *some* specifier, up to the highest position seen - a "gap" position
/// referenced only implicitly (`%3$s` alone still needs positions 0 and
/// 1 to exist as real arguments) gets an empty `Vec`: nothing to check
/// its own argument's type against, but it still counts toward the
/// minimum argument count. A position referenced by more than one
/// specifier (`%1$d %1$s`, a real if unusual idiom) collects every
/// specifier that references it - the argument there must satisfy all
/// of them.
pub(crate) fn parse_positional_format_specifiers(
    fmt: &str,
    php_version: Option<PhpVersion>,
) -> Option<Vec<Vec<char>>> {
    let hh_allowed = php_version.is_none_or(|v| PhpFeature::PrintfHhSpecifier.is_available(v));
    let bytes = fmt.as_bytes();
    let mut by_position: Vec<Vec<char>> = Vec::new();
    let mut i = 0;
    let mut saw_any = false;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        let &next = bytes.get(i)?;
        if next == b'%' {
            i += 1;
            continue;
        }
        let digits_start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == digits_start || bytes.get(i) != Some(&b'$') {
            // No digits, or digits not immediately followed by `$` - an
            // ordinary, unnumbered specifier. This format string isn't
            // uniformly positional, so don't guess at how the two
            // styles interact.
            return None;
        }
        let position: usize = fmt[digits_start..i].parse().ok()?;
        i += 1; // skip '$'
        while bytes
            .get(i)
            .is_some_and(|b| matches!(b, b'-' | b'+' | b' ' | b'0'))
        {
            i += 1;
        }
        if bytes.get(i) == Some(&b'\'') {
            i += 2; // custom padding char: the quote plus the pad character itself
        }
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'.') {
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        let &spec = bytes.get(i)?;
        if !b"bcdeEfFgGosuxX".contains(&spec) && !(hh_allowed && matches!(spec, b'h' | b'H')) {
            return None;
        }
        i += 1;
        saw_any = true;
        // Real PHP positions are 1-based; `%0$s` isn't valid PHP at all
        // (a `ValueError` at runtime) - don't guess at what it means.
        if position == 0 {
            return None;
        }
        let idx = position - 1;
        if by_position.len() <= idx {
            by_position.resize(idx + 1, Vec::new());
        }
        by_position[idx].push(spec as char);
    }
    if !saw_any {
        return None;
    }
    Some(by_position)
}

/// Whether a real PHP conversion specifier could plausibly accept
/// `arg_ty`, under either of real PHPStan's own two `PrintfPlaceholder::
/// doesArgumentTypeMatchPlaceholder` matching modes (`strict` - real
/// PHPStan's own `checkStrictPrintfPlaceholderTypes`, opt-in and default
/// `false` here too, matching real PHPStan's own stock config).
///
/// Non-strict ("castable") mode is deliberately lenient (PHP casts
/// numeric strings and floats for `%d`, and almost anything with a
/// `__toString` for `%s`), so this only flags the unambiguous cases: an
/// array (or a known array shape) for any specifier, or an object for a
/// numeric one - both produce either a fatal error or the literal,
/// useless string `"Array"` in real PHP, never a legitimate value.
///
/// Strict mode requires the int/float bucket's own real target type (or,
/// for the float bucket, an int - PHP implicitly widens int to float, and
/// real PHPStan's own `FloatType::accepts` agrees), plus a numeric-string
/// *literal* carve-out mirroring real PHPStan's `AccessoryDecimalInteger
/// StringType`/`AccessoryNumericStringType` - a non-literal `string`-typed
/// argument doesn't carry that accessory type in this project's own
/// (coarser) type system, so it's rejected here the same "don't guess"
/// way an unresolved template is. The string/mixed bucket only tightens
/// by rejecting `bool` outright (real PHPStan's own "likely a mistake"
/// carve-out) - `null` stays accepted even in strict mode, matching real
/// PHPStan's own explicit "consistency with phpstan-strict-rules" carve-out
/// there.
pub(crate) fn format_specifier_accepts(spec: char, arg_ty: &Type, strict: bool) -> bool {
    // `Checker::is_array_like` also recognizes a `list<T>`/
    // `non-empty-array<T>`-typed argument (an `Intersection`).
    match spec {
        'b' | 'c' | 'd' | 'o' | 'u' | 'x' | 'X' => {
            if strict {
                is_strict_int(arg_ty) || is_decimal_int_string_literal(arg_ty)
            } else {
                !Checker::is_array_like(arg_ty) && !matches!(arg_ty, Type::Object(_, _))
            }
        }
        'e' | 'E' | 'f' | 'F' | 'g' | 'G' | 'h' | 'H' => {
            if strict {
                matches!(arg_ty, Type::Float)
                    || is_strict_int(arg_ty)
                    || is_numeric_string_type(arg_ty)
            } else {
                !Checker::is_array_like(arg_ty) && !matches!(arg_ty, Type::Object(_, _))
            }
        }
        's' => {
            if strict {
                !matches!(arg_ty, Type::Bool | Type::True | Type::False)
                    && !Checker::is_array_like(arg_ty)
            } else {
                !Checker::is_array_like(arg_ty)
            }
        }
        _ => true,
    }
}

/// Strict-mode `%d`-bucket carve-out: a string *literal* whose value is
/// itself a plain base-10 integer (real PHPStan's `AccessoryDecimalInteger
/// StringType`) - reuses `canonical_int_key`'s own "no leading zero except
/// bare `0`, no `-0`" rules, which already encode exactly this shape.
fn is_decimal_int_string_literal(ty: &Type) -> bool {
    matches!(ty, Type::StringLiteral(s) if crate::types::canonical_int_key(s).is_some())
}

/// Strict-mode `%f`-bucket carve-out: a string literal that parses as any
/// real PHP numeric string (real PHPStan's `AccessoryNumericStringType`).
fn is_numeric_string_type(ty: &Type) -> bool {
    matches!(ty, Type::StringLiteral(s) if crate::types::is_numeric_string_literal(s))
}
