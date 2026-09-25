//! Calls taking a printf- or scanf-style format string: PHPStan's
//! `PrintfHelper` and the rules built on it. `PrintfParametersRule` and
//! `PrintfArrayParametersRule` count the format's placeholders against
//! the values passed; `PrintfParameterTypeRule` checks each value against
//! its placeholders. `fprintf` and `vfprintf`, which PHPStan's count rules
//! leave out, are counted the same way under `argument.fprintf` and
//! `argument.vfprintf`.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::checker::{Checker, Locals};
use crate::php_version::{PhpFeature, PhpVersion};
use crate::types::{AccessoryKind, Type};
use syntax::ast::*;

/// What a placeholder accepts (`PrintfPlaceholder::$acceptingType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Accepting {
    /// A `*` width or precision: only an int.
    StrictInt,
    Int,
    Float,
    String,
    Mixed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Placeholder {
    /// How messages quote it: `"%5d"`, `"%*d" (width)`, `"%*d" (value)`.
    pub label: String,
    /// The 0-based value it consumes (`%0$s` makes it -1, as in PHPStan).
    pub parameter_index: i64,
    /// 1-based, in format-string order.
    pub number: usize,
    pub accepting: Accepting,
}

/// PHPStan's placeholder pattern, with `h`/`H` (PHP 8.0+) and without.
fn placeholder_pattern(hh: bool) -> Regex {
    let specifiers = if hh { "bshH" } else { "bs" };
    Regex::new(&format!(
        r"(?P<before>%*)%(?:(?P<position>\d+)\$)?[-+]?(?:[ 0]|(?:'[^%]))?(?P<width>\*)?-?\d*(?:\.(?:\d+|(?P<precision>\*))?)?(?P<specifier>[{specifiers}]|l?[cdeEgfFGouxX])?"
    ))
    .expect("the placeholder pattern is valid")
}

static PLACEHOLDER_WITH_HH: LazyLock<Regex> = LazyLock::new(|| placeholder_pattern(true));
static PLACEHOLDER_WITHOUT_HH: LazyLock<Regex> = LazyLock::new(|| placeholder_pattern(false));

/// `PrintfHelper::getPrintfPlaceholders`: the placeholders of a printf
/// format by the value they consume, or `None` when one is invalid.
pub(crate) fn printf_placeholders(
    format: &str,
    php_version: Option<PhpVersion>,
) -> Option<BTreeMap<i64, Vec<Placeholder>>> {
    let hh = php_version.is_none_or(|v| PhpFeature::PrintfHhSpecifier.is_available(v));
    let pattern = if hh {
        &*PLACEHOLDER_WITH_HH
    } else {
        &*PLACEHOLDER_WITHOUT_HH
    };
    let mut parsed = Vec::new();
    let mut next_index = 0i64;
    let mut number = 0usize;
    for m in pattern.captures_iter(format) {
        // An odd run of `%` before this one escapes it (`%%d`).
        if m["before"].len() % 2 != 0 {
            continue;
        }
        number += 1;
        let whole = &m[0];
        let mut show_value_suffix = false;
        if m.name("width").is_some() {
            parsed.push(Placeholder {
                label: format!("\"{whole}\" (width)"),
                parameter_index: next_index,
                number,
                accepting: Accepting::StrictInt,
            });
            next_index += 1;
            show_value_suffix = true;
        }
        if m.name("precision").is_some() {
            parsed.push(Placeholder {
                label: format!("\"{whole}\" (precision)"),
                parameter_index: next_index,
                number,
                accepting: Accepting::StrictInt,
            });
            next_index += 1;
            show_value_suffix = true;
        }
        let specifier = m.name("specifier")?.as_str();
        let parameter_index = match m.name("position") {
            Some(position) => position.as_str().parse::<i64>().ok()? - 1,
            None => {
                next_index += 1;
                next_index - 1
            }
        };
        parsed.push(Placeholder {
            label: format!(
                "\"{whole}\"{}",
                if show_value_suffix { " (value)" } else { "" }
            ),
            parameter_index,
            number,
            accepting: accepting_by_specifier(specifier),
        });
    }
    let mut by_index: BTreeMap<i64, Vec<Placeholder>> = BTreeMap::new();
    for placeholder in parsed {
        by_index
            .entry(placeholder.parameter_index)
            .or_default()
            .push(placeholder);
    }
    Some(by_index)
}

fn accepting_by_specifier(specifier: &str) -> Accepting {
    match specifier.trim_start_matches('l') {
        "s" => Accepting::String,
        "d" | "u" | "c" | "o" | "x" | "X" | "b" => Accepting::Int,
        "e" | "E" | "f" | "F" | "g" | "G" | "h" | "H" => Accepting::Float,
        _ => Accepting::Mixed,
    }
}

/// `PrintfHelper::getPrintfPlaceholdersCount`: how many values a printf
/// format consumes, or `None` when a placeholder is invalid.
pub(crate) fn printf_placeholder_count(
    format: &str,
    php_version: Option<PhpVersion>,
) -> Option<usize> {
    let by_index = printf_placeholders(format, php_version)?;
    Some(
        by_index
            .keys()
            .next_back()
            .map_or(0, |max| usize::try_from(max + 1).unwrap_or(0)),
    )
}

/// The most variables a positional scanf conversion may name.
const SCAN_MAX_ARGS: u64 = 0xFF;

/// `PrintfHelper::getScanfPlaceholdersCount`: how many values
/// `sscanf('', '%*n' . $format)` returns, or `None` where PHP throws a
/// `ValueError`.
pub(crate) fn scanf_placeholder_count(format: &str) -> Option<usize> {
    validate_scanf_format(&format!("%*n{format}"))
}

/// A port of `ValidateFormat` in PHP's `ext/standard/scanf.c`, with no
/// variables passed: the number of values `format` assigns.
fn validate_scanf_format(format: &str) -> Option<usize> {
    let bytes = format.as_bytes();
    let at = |i: usize| bytes.get(i).copied().unwrap_or(0);
    let mut i = 0;
    let mut got_xpg = false;
    let mut got_sequential = false;
    let mut xpg_size = 0usize;
    let mut index = 0usize;
    let mut assigned: Vec<u32> = Vec::new();
    while i < bytes.len() {
        let c = at(i);
        i += 1;
        if c != b'%' {
            continue;
        }
        let mut ch = at(i);
        i += 1;
        if ch == b'%' {
            continue;
        }
        let mut suppress = false;
        if ch == b'*' {
            suppress = true;
            ch = at(i);
            i += 1;
        } else {
            let mut sequential = true;
            if ch.is_ascii_digit() {
                let start = i - 1;
                let mut end = start;
                while at(end).is_ascii_digit() {
                    end += 1;
                }
                if at(end) == b'$' {
                    let value: u64 = format[start..end].parse().unwrap_or(u64::MAX);
                    i = end + 1;
                    ch = at(i);
                    i += 1;
                    got_xpg = true;
                    if got_sequential {
                        return None;
                    }
                    if value == 0 || value > SCAN_MAX_ARGS {
                        return None;
                    }
                    index = usize::try_from(value - 1).ok()?;
                    xpg_size = xpg_size.max(index + 1);
                    sequential = false;
                }
            }
            if sequential {
                got_sequential = true;
                if got_xpg {
                    return None;
                }
            }
        }
        if ch.is_ascii_digit() {
            while at(i).is_ascii_digit() {
                i += 1;
            }
            ch = at(i);
            i += 1;
        }
        if matches!(ch, b'l' | b'L' | b'h') {
            ch = at(i);
            i += 1;
        }
        match ch {
            b'n' | b'd' | b'D' | b'i' | b'o' | b'x' | b'X' | b'u' | b'f' | b'e' | b'E' | b'g'
            | b's' | b'c' => {}
            b'[' => {
                if i >= bytes.len() {
                    return None;
                }
                ch = at(i);
                i += 1;
                if ch == b'^' {
                    if i >= bytes.len() {
                        return None;
                    }
                    ch = at(i);
                    i += 1;
                }
                if ch == b']' {
                    if i >= bytes.len() {
                        return None;
                    }
                    ch = at(i);
                    i += 1;
                }
                while ch != b']' {
                    if i >= bytes.len() {
                        return None;
                    }
                    ch = at(i);
                    i += 1;
                }
            }
            _ => return None,
        }
        if !suppress {
            if assigned.len() <= index {
                assigned.resize(index + 1, 0);
            }
            assigned[index] += 1;
            index += 1;
        }
    }
    let total = if xpg_size > 0 { xpg_size } else { index };
    for slot in 0..total {
        let count = assigned.get(slot).copied().unwrap_or(0);
        if count > 1 || (xpg_size == 0 && count == 0) {
            return None;
        }
    }
    Some(total)
}

/// A count of values: one number, or an inclusive range of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Count {
    min: usize,
    max: usize,
}

impl Count {
    fn exact(n: usize) -> Count {
        Count { min: n, max: n }
    }

    fn is_exact(self) -> bool {
        self.min == self.max
    }

    fn describe(self) -> String {
        if self.is_exact() {
            self.min.to_string()
        } else {
            format!("{}-{}", self.min, self.max)
        }
    }
}

/// The printf-family function a call is, with where its format sits.
struct FormatFunction {
    /// Position of the format argument.
    format: usize,
    /// Fewer arguments than this is `arguments.count`'s to report.
    minimum: usize,
    kind: FormatKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FormatKind {
    /// Values passed one argument each (`sprintf`).
    Printf,
    /// Values passed as one array (`vsprintf`).
    PrintfArray,
    /// Variables to assign passed one argument each (`sscanf`).
    Scanf,
}

fn format_function(name: &str) -> Option<FormatFunction> {
    let (format, minimum, kind) = match name {
        "printf" | "sprintf" => (0, 1, FormatKind::Printf),
        "fprintf" => (1, 2, FormatKind::Printf),
        "vprintf" | "vsprintf" => (0, 1, FormatKind::PrintfArray),
        "vfprintf" => (1, 2, FormatKind::PrintfArray),
        "sscanf" | "fscanf" => (1, 3, FormatKind::Scanf),
        _ => return None,
    };
    Some(FormatFunction {
        format,
        minimum,
        kind,
    })
}

impl<'a> Checker<'a> {
    /// A call to a printf- or scanf-family function (`name` resolved, in
    /// lower case): its format's placeholders against the arguments.
    pub(crate) fn check_format_string_call(
        &mut self,
        name: &str,
        args: &[Arg],
        locals: &Locals,
        label: &str,
        span: syntax::token::Span,
    ) {
        let Some(function) = format_function(name) else {
            return;
        };
        match function.kind {
            FormatKind::PrintfArray => {
                self.check_format_array_count(name, &function, args, locals, label, span);
                self.check_format_values_array_type(name, &function, args, locals, label);
            }
            FormatKind::Printf | FormatKind::Scanf => {
                self.check_format_count(name, &function, args, locals, label, span);
            }
        }
        if function.kind == FormatKind::Printf {
            self.check_format_value_types(name, &function, args, locals, label, span);
        }
    }

    /// The format's constant strings, reporting an invalid placeholder in
    /// any of them; `None` when there is nothing (more) to check.
    fn format_counts(
        &mut self,
        name: &str,
        function: &FormatFunction,
        args: &[Arg],
        locals: &Locals,
        label: &str,
        span: syntax::token::Span,
    ) -> Option<Vec<usize>> {
        let format_ty = self.infer_expr(&args.get(function.format)?.value, locals);
        let php_version = self.registry.php_version;
        let mut counts = Vec::new();
        for format in format_ty.constant_strings() {
            let count = if function.kind == FormatKind::Scanf {
                scanf_placeholder_count(format)
            } else {
                printf_placeholder_count(format, php_version)
            };
            let Some(count) = count else {
                self.report(
                    label.to_string(),
                    format!("Call to {name} contains an invalid placeholder."),
                    span,
                    0,
                    &format!("argument.{name}"),
                );
                return None;
            };
            counts.push(count);
        }
        (!counts.is_empty()).then_some(counts)
    }

    /// `PrintfParametersRule`: the most placeholders any of the format's
    /// constant strings has against the values passed.
    fn check_format_count(
        &mut self,
        name: &str,
        function: &FormatFunction,
        args: &[Arg],
        locals: &Locals,
        label: &str,
        span: syntax::token::Span,
    ) {
        if args.iter().any(|a| a.spread) || args.len() < function.minimum {
            return;
        }
        let Some(counts) = self.format_counts(name, function, args, locals, label, span) else {
            return;
        };
        let placeholders = counts.into_iter().max().unwrap_or(0);
        let values = args.len() - function.format - 1;
        if values == placeholders {
            return;
        }
        self.report(
            label.to_string(),
            count_message(name, Count::exact(placeholders), Count::exact(values)),
            span,
            0,
            &format!("argument.{name}"),
        );
    }

    /// `PrintfArrayParametersRule`: every placeholder count the format's
    /// constant strings can have against every size the values array can.
    fn check_format_array_count(
        &mut self,
        name: &str,
        function: &FormatFunction,
        args: &[Arg],
        locals: &Locals,
        label: &str,
        span: syntax::token::Span,
    ) {
        if args.len() < function.minimum {
            return;
        }
        let Some(counts) = self.format_counts(name, function, args, locals, label, span) else {
            return;
        };
        let placeholders = Count {
            min: counts.iter().copied().min().unwrap_or(0),
            max: counts.iter().copied().max().unwrap_or(0),
        };
        let values = match args.get(function.format + 1) {
            None => Count::exact(0),
            Some(values) => {
                let ty = self.infer_expr(&values.value, locals);
                let Some(sizes) = constant_array_sizes(&ty) else {
                    return;
                };
                let Some(values) = merge_counts(sizes) else {
                    return;
                };
                values
            }
        };
        if placeholders_match_values(placeholders, values) {
            return;
        }
        self.report(
            label.to_string(),
            count_message(name, placeholders, values),
            span,
            0,
            &format!("argument.{name}"),
        );
    }

    /// The values array of `vsprintf` and its siblings against their
    /// `functionMap.php` parameter type, `array<bool|float|int|string|null>`
    /// (`CallToFunctionParametersRule`).
    fn check_format_values_array_type(
        &mut self,
        name: &str,
        function: &FormatFunction,
        args: &[Arg],
        locals: &Locals,
        label: &str,
    ) {
        let position = function.format + 1;
        let Some(values) = args.get(position) else {
            return;
        };
        if values.spread {
            return;
        }
        let values_ty = self.infer_expr(&values.value, locals);
        let expected = Type::Array(
            Box::new(Type::Union(vec![Type::Int, Type::String])),
            Box::new(format_value_type()),
        );
        if self
            .registry
            .accepts_at_configured_level(&values_ty, &expected)
        {
            return;
        }
        self.report(
            label.to_string(),
            format!(
                "Parameter #{} $values of function {name} expects array<bool|float|int|string|null>, {values_ty} given.",
                position + 1
            ),
            values.value.span,
            5,
            "argument.type",
        );
    }

    /// `PrintfParameterTypeRule`: each value against the placeholders
    /// consuming it, for a format with one constant string. A value the
    /// function's own parameter type rejects gets that parameter's error
    /// instead (`CallToFunctionParametersRule` with PHPStan's
    /// `functionMap.php` signature, `bool|float|int|string|null` values).
    fn check_format_value_types(
        &mut self,
        name: &str,
        function: &FormatFunction,
        args: &[Arg],
        locals: &Locals,
        label: &str,
        span: syntax::token::Span,
    ) {
        if args.iter().any(|a| a.spread) || args.len() < function.minimum {
            return;
        }
        let format_ty = self.infer_expr(&args[function.format].value, locals);
        let [format] = format_ty.constant_strings()[..] else {
            return;
        };
        let Some(by_index) = printf_placeholders(format, self.registry.php_version) else {
            return;
        };
        let strict = self.registry.check_strict_printf_placeholder_types;
        let allowed = format_value_type();
        for (i, arg) in args.iter().enumerate().skip(function.format + 1) {
            let arg_ty = self.infer_expr(&arg.value, locals);
            if !self.registry.accepts_at_configured_level(&arg_ty, &allowed) {
                self.report(
                    label.to_string(),
                    format!(
                        "Parameter #{} ...$values of function {name} expects bool|float|int|string|null, {arg_ty} given.",
                        i + 1
                    ),
                    arg.value.span,
                    5,
                    "argument.type",
                );
                continue;
            }
            let value_index = i64::try_from(i - function.format - 1).unwrap_or(i64::MAX);
            for placeholder in by_index.get(&value_index).into_iter().flatten() {
                let Some(checked) = self.type_to_check(&arg_ty, |t, checker| {
                    checker.matches_placeholder(t, placeholder.accepting, strict)
                }) else {
                    continue;
                };
                if self.matches_placeholder(&checked, placeholder.accepting, strict) {
                    continue;
                }
                let expected = match (placeholder.accepting, strict) {
                    (Accepting::StrictInt, _) | (Accepting::Int, true) => "int",
                    (Accepting::Float, true) => "float",
                    (Accepting::String | Accepting::Mixed, true) => "string",
                    (Accepting::Int, false) => "castable to int",
                    (Accepting::Float, false) => "castable to float",
                    (Accepting::String | Accepting::Mixed, false) => "castable to string",
                };
                self.report(
                    label.to_string(),
                    format!(
                        "Parameter #{} of function {name} is expected to be {expected} by placeholder #{} ({}), {} given.",
                        i + 1,
                        placeholder.number,
                        placeholder.label,
                        checked.describe_type_only()
                    ),
                    span,
                    5,
                    "argument.type",
                );
            }
        }
    }

    /// `RuleLevelHelper::findTypeToCheck` for a value argument: `None`
    /// for `mixed` (never checked), `null` dropped from a union below
    /// level 8 (`checkNullables`), and below level 7 (`checkUnionTypes`)
    /// only the members `matches` accepts kept when any does. An
    /// unconfigured level checks everything.
    fn type_to_check(&self, ty: &Type, matches: impl Fn(&Type, &Self) -> bool) -> Option<Type> {
        if matches!(ty, Type::Mixed(_)) {
            return None;
        }
        let level = self.registry.configured_level;
        let Type::Union(members) = ty else {
            return Some(ty.clone());
        };
        let mut members: Vec<Type> = members.clone();
        if level.is_some_and(|l| l < 8) {
            members.retain(|m| !matches!(m, Type::Null));
        }
        if level.is_some_and(|l| l < 7) {
            let accepted: Vec<Type> = members
                .iter()
                .filter(|m| matches(m, self))
                .cloned()
                .collect();
            if !accepted.is_empty() {
                members = accepted;
            }
        }
        Some(match members.len() {
            1 => members.remove(0),
            _ => Type::Union(members),
        })
    }

    /// `PrintfPlaceholder::doesArgumentTypeMatchPlaceholder`.
    fn matches_placeholder(&self, ty: &Type, accepting: Accepting, strict: bool) -> bool {
        match accepting {
            Accepting::StrictInt => self.registry.is_assignable(ty, &Type::Int),
            Accepting::Int if strict => self.registry.is_assignable(
                ty,
                &Type::Union(vec![
                    Type::Int,
                    Type::Intersection(vec![
                        Type::String,
                        Type::Accessory(AccessoryKind::DecimalIntegerString),
                    ]),
                ]),
            ),
            Accepting::Float if strict => self.registry.is_assignable(
                ty,
                &Type::Union(vec![
                    Type::Float,
                    Type::Int,
                    Type::Intersection(vec![
                        Type::String,
                        Type::Accessory(AccessoryKind::NumericString),
                    ]),
                ]),
            ),
            Accepting::Int => self.casts_to_number(ty, true),
            Accepting::Float => self.casts_to_number(ty, false),
            Accepting::String | Accepting::Mixed => {
                !strict
                    || self.registry.is_assignable(
                        ty,
                        &Type::Union(vec![
                            Type::String,
                            Type::Object("Stringable".to_string(), Vec::new()),
                            Type::Float,
                            Type::Int,
                            Type::Null,
                        ]),
                    )
            }
        }
    }

    /// Whether `(int)`/`(float)` of `ty` is defined (PHPStan's
    /// `toInteger()`/`toFloat()` not being an error): every union member
    /// must cast, one intersection member suffices, and of objects only
    /// `SimpleXMLElement` and `GMP` cast (plus the curl handles to int).
    fn casts_to_number(&self, ty: &Type, to_int: bool) -> bool {
        match ty {
            Type::Union(members) => members.iter().all(|m| self.casts_to_number(m, to_int)),
            Type::Intersection(members) => members.iter().any(|m| self.casts_to_number(m, to_int)),
            Type::Object(class, _) => {
                self.registry.is_subclass_of(class, "SimpleXMLElement")
                    || self.registry.is_subclass_of(class, "GMP")
                    || (to_int && matches!(class.as_str(), "CurlHandle" | "CurlMultiHandle"))
            }
            Type::EnumCase(..) => false,
            _ => true,
        }
    }
}

/// A printf value's `functionMap.php` type, `__stringAndStringable|int|
/// float|null|bool`.
fn format_value_type() -> Type {
    Type::Union(vec![
        Type::String,
        Type::Object("Stringable".to_string(), Vec::new()),
        Type::Int,
        Type::Float,
        Type::Bool,
        Type::Null,
    ])
}

/// `ConstantArrayType::getArraySize` of each constant array `ty` can be,
/// or `None` when it can be any other array.
fn constant_array_sizes(ty: &Type) -> Option<Vec<Count>> {
    match ty {
        Type::ArrayShape(fields) => {
            let required = fields.iter().filter(|(_, _, optional)| !optional).count();
            Some(vec![Count {
                min: required,
                max: fields.len(),
            }])
        }
        Type::Union(members) => {
            let mut sizes = Vec::new();
            for member in members {
                sizes.extend(constant_array_sizes(member)?);
            }
            Some(sizes)
        }
        _ => None,
    }
}

/// `TypeCombinator::union` of array sizes, which PHPStan only checks
/// when it is one number or one unbroken range: distinct numbers stay a
/// union, and a range absorbs the numbers and ranges it overlaps or
/// touches.
fn merge_counts(mut counts: Vec<Count>) -> Option<Count> {
    counts.sort_by_key(|c| (c.min, c.max));
    counts.dedup();
    let mut merged = *counts.first()?;
    for c in &counts[1..] {
        let touches = c.min <= merged.max + 1;
        if !touches || (merged.is_exact() && c.is_exact()) {
            return None;
        }
        merged.max = merged.max.max(c.max);
    }
    Some(merged)
}

/// `PrintfArrayParametersRule::placeholdersMatchesArgsCount`.
fn placeholders_match_values(placeholders: Count, values: Count) -> bool {
    if placeholders.is_exact() {
        if values.is_exact() {
            return placeholders.min == values.min;
        }
        return placeholders.min == 0;
    }
    placeholders.min >= 1
        && !values.is_exact()
        && placeholders.min <= values.min
        && values.max <= placeholders.max
}

fn count_message(name: &str, placeholders: Count, values: Count) -> String {
    let placeholders_text = if placeholders.is_exact() && placeholders.min == 1 {
        "1 placeholder".to_string()
    } else {
        format!("{} placeholders", placeholders.describe())
    };
    let values_text = if values.is_exact() && values.min == 1 {
        "1 value given".to_string()
    } else {
        format!("{} values given", values.describe())
    };
    format!("Call to {name} contains {placeholders_text}, {values_text}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each format against what `count(@sscanf('', '%*n' . $format))`
    /// gives in PHP 8.5, `None` for a `ValueError`.
    #[test]
    fn scanf_counts_match_php() {
        let cases: &[(&str, Option<usize>)] = &[
            ("%d", Some(1)),
            ("%d %s", Some(2)),
            ("%*d %s", Some(1)),
            ("%s%%%d", Some(2)),
            ("%1$s %2$d", Some(2)),
            ("%2$s %1$d", Some(2)),
            ("%2$s", Some(2)),
            ("%1$s %s", None),
            ("%s %1$s", None),
            ("%*1$s", None),
            ("%5d", Some(1)),
            ("%ld", Some(1)),
            ("%Ld", Some(1)),
            ("%hd", Some(1)),
            ("%c", Some(1)),
            ("%5c", Some(1)),
            ("%[a-z]", Some(1)),
            ("%[^a-z]", Some(1)),
            ("%[]a]", Some(1)),
            ("%[", None),
            ("%[a", None),
            ("%q", None),
            ("%", None),
            ("abc", Some(0)),
            ("", Some(0)),
            ("%n", Some(1)),
            ("%*n", Some(0)),
            ("%D", Some(1)),
            ("%i", Some(1)),
            ("%u", Some(1)),
            ("%x%X%o%f%e%E%g", Some(7)),
            ("%1$s %1$s", None),
            ("%0$s", None),
            ("%256$s", None),
            ("%255$s", Some(255)),
            ("%*5s%d", Some(1)),
            ("%5*d", None),
            ("%l", None),
            ("%1$*s", None),
            ("%10$s %3$d", Some(10)),
        ];
        for (format, expected) in cases {
            assert_eq!(scanf_placeholder_count(format), *expected, "{format}");
        }
    }
}
