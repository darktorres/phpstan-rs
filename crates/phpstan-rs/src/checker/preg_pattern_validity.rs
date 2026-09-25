//! Wires `pcre_validate::validate_pattern` into every real PHP builtin
//! that takes a PCRE pattern literal at argument 0, mirroring real
//! PHPStan's own `RegularExpressionPatternRule::extractPatterns`
//! (`#[RegisteredRule(level: 0)]`, always-on, not opt-in).
//! `identifier: "regexp.pattern"` reuses real PHPStan's own spelling.
//!
//! `preg_grep`'s own `$pattern` is always a plain string in real PHP/
//! PHPStan - it has no array form, unlike its 3 siblings just below;
//! only `preg_replace`/`preg_replace_callback`/`preg_filter` take the
//! array-of-patterns form.

use crate::checker::Checker;
use crate::pcre_validate::validate_pattern;
use syntax::ast::{Arg, Expr, ExprKind, Literal};
use syntax::token::Span;

/// Which of the 8 real PCRE-pattern-taking builtins `name` is, and how
/// its own pattern argument(s) are shaped - `None` for anything else.
enum PatternArgShape {
    /// A plain string literal only (`preg_match`/`preg_match_all`/
    /// `preg_split`/`preg_grep`).
    Plain,
    /// A plain string literal, or an array literal of string-literal
    /// *values* (`preg_replace`/`preg_replace_callback`/`preg_filter`).
    PlainOrArrayValues,
    /// An array literal of string-literal *keys*
    /// (`preg_replace_callback_array`).
    ArrayKeys,
}

fn pattern_arg_shape(name: &str) -> Option<PatternArgShape> {
    let bare = name.rsplit('\\').next().unwrap_or(name);
    if bare.eq_ignore_ascii_case("preg_match")
        || bare.eq_ignore_ascii_case("preg_match_all")
        || bare.eq_ignore_ascii_case("preg_split")
        || bare.eq_ignore_ascii_case("preg_grep")
    {
        Some(PatternArgShape::Plain)
    } else if bare.eq_ignore_ascii_case("preg_replace")
        || bare.eq_ignore_ascii_case("preg_replace_callback")
        || bare.eq_ignore_ascii_case("preg_filter")
    {
        Some(PatternArgShape::PlainOrArrayValues)
    } else if bare.eq_ignore_ascii_case("preg_replace_callback_array") {
        Some(PatternArgShape::ArrayKeys)
    } else {
        None
    }
}

fn as_string_literal(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.as_ref()),
        _ => None,
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn check_preg_pattern_validity(&mut self, name: &str, args: &[Arg], label: &str) {
        let Some(shape) = pattern_arg_shape(name) else {
            return;
        };
        let Some(pattern_arg) = args.first() else {
            return;
        };
        if pattern_arg.name.is_some() || pattern_arg.spread {
            return;
        }
        match shape {
            PatternArgShape::Plain => {
                if let Some(s) = as_string_literal(&pattern_arg.value) {
                    self.report_if_invalid_pattern(s, pattern_arg.value.span, label);
                }
            }
            PatternArgShape::PlainOrArrayValues => {
                if let Some(s) = as_string_literal(&pattern_arg.value) {
                    self.report_if_invalid_pattern(s, pattern_arg.value.span, label);
                } else if let ExprKind::ArrayLiteral(items) = &pattern_arg.value.kind {
                    for item in items {
                        if let Some(s) = as_string_literal(&item.value) {
                            self.report_if_invalid_pattern(s, item.value.span, label);
                        }
                    }
                }
            }
            PatternArgShape::ArrayKeys => {
                if let ExprKind::ArrayLiteral(items) = &pattern_arg.value.kind {
                    for item in items {
                        if let Some(key) = &item.key
                            && let Some(s) = as_string_literal(key)
                        {
                            self.report_if_invalid_pattern(s, key.span, label);
                        }
                    }
                }
            }
        }
    }

    fn report_if_invalid_pattern(&mut self, pattern_literal: &str, span: Span, label: &str) {
        let Some(detail) = validate_pattern(pattern_literal) else {
            return;
        };
        self.report(
            label.to_string(),
            format!("Regex pattern is invalid: {detail}"),
            span,
            0,
            "regexp.pattern",
        );
    }
}
