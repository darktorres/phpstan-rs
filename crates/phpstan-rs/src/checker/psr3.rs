//! PSR-3 logger call hygiene (`staabm/phpstan-psr3`,
//! `struggle-for-php/sfp-phpstan-psr-log`), for a method call on a
//! `Psr\Log\LoggerInterface` and a static call on Laravel's `Log` facade.
//!
//! Opt-in `check_psr3_logging`:
//! - `psr3.interpolated`: the message concatenates or interpolates a value
//!   that isn't a number, a bool, a literal string or an enum case, in
//!   place of a placeholder;
//! - `psr3.messageNotStatic`: the message may not be a literal string;
//! - `psr3.placeholderDoubleBraces`/`psr3.placeholderInvalidChar`: a
//!   `{{...}}` or a brace pair holding anything but `A-Za-z0-9_.`;
//! - `psr3.placeholderMissingContext`/`psr3.placeholderMissingKey`: a
//!   placeholder with no context argument, or with no such key in it;
//! - `psr3.placeholderNotStringable`: a context value a placeholder
//!   interpolates that has no string form;
//! - `psr3.invalidLevel`: `log()`'s level isn't one of the eight PSR-3
//!   levels;
//! - `psr3.contextKeyNotString`: a context key that isn't a non-empty
//!   string;
//! - `psr3.contextKeyPattern`: a context key not matching
//!   `psr3_context_key_pattern`;
//! - `psr3.contextType`: an `exception` context value that isn't a
//!   `Throwable`.
//!
//! Opt-in `check_psr3_exception_context`: `psr3.contextMissingException`,
//! a Throwable variable is in scope but the context has no `exception`
//! key.

use crate::checker::{Checker, Locals};
use crate::diagnostic::DiagnosticBuilder;
use crate::types::{ShapeKey, Type};
use syntax::ast::*;
use syntax::token::Span;

const LEVELS: [&str; 8] = [
    "emergency",
    "alert",
    "critical",
    "error",
    "warning",
    "notice",
    "info",
    "debug",
];

/// `{name}` placeholders in `message`, with their brace text.
fn placeholders(message: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            break;
        };
        let name = &after[..close];
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            found.push(name);
        }
        rest = &after[close + 1..];
    }
    found
}

/// Brace pairs whose content isn't a valid placeholder name, and whether
/// the message uses `{{...}}`.
fn invalid_braces(message: &str) -> (bool, Vec<String>) {
    let double = message
        .find("{{")
        .is_some_and(|i| message[i + 2..].contains("}}"));
    let mut invalid = Vec::new();
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            break;
        };
        let name = &after[..close];
        if !name.is_empty()
            && !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            invalid.push(format!("{{{name}}}"));
        }
        rest = &after[close + 1..];
    }
    (double, invalid)
}

/// Whether some value of `ty` is a string not known to be literal.
fn may_be_non_literal_string(ty: &Type) -> bool {
    match ty {
        Type::String | Type::ClassString(_) | Type::ClassKindString(_) => true,
        Type::Union(members) => members.iter().any(may_be_non_literal_string),
        Type::Intersection(members) => {
            members.iter().any(|m| matches!(m, Type::String))
                && !members.iter().any(|m| {
                    matches!(
                        m,
                        Type::Accessory(crate::types::AccessoryKind::LiteralString)
                    )
                })
        }
        _ => false,
    }
}

fn string_literals(ty: &Type) -> Option<Vec<&str>> {
    let strings = ty.constant_strings();
    (!strings.is_empty()).then_some(strings)
}

impl<'a> Checker<'a> {
    fn is_psr3_logger(&self, ty: &Type) -> bool {
        let logger = Type::Object("Psr\\Log\\LoggerInterface".to_string(), Vec::new());
        !matches!(ty, Type::Mixed(_))
            && self.registry.is_super_type_of(&logger, ty) == crate::registry::Trinary::Yes
    }

    pub(crate) fn check_psr3_call(&mut self, e: &Expr, locals: &Locals, label: &str) {
        let (method, args) = match &e.kind {
            ExprKind::MethodCall(receiver, method, args, _) => {
                let receiver_ty = self.infer_expr(receiver, locals);
                if !self.is_psr3_logger(&receiver_ty) {
                    return;
                }
                (method.to_ascii_lowercase(), args)
            }
            ExprKind::StaticCall(StaticTarget::Name(class, _), method, args)
                if self.resolve_static_class_name(class).is_some_and(|c| {
                    c.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Log")
                }) =>
            {
                (method.to_ascii_lowercase(), args)
            }
            _ => return,
        };
        if args.is_empty() || args.iter().any(|a| a.spread || a.name.is_some()) {
            return;
        }
        let (levels, message_at): (Vec<String>, usize) = if method == "log" {
            if args.len() < 2 {
                return;
            }
            let level_ty = self.infer_expr(&args[0].value, locals);
            if self.registry.check_psr3_logging {
                self.check_psr3_level(&level_ty, args[0].value.span, label);
            }
            let levels = string_literals(&level_ty)
                .map(|ls| ls.iter().map(|l| l.to_ascii_lowercase()).collect())
                .unwrap_or_default();
            (levels, 1)
        } else if LEVELS.contains(&method.as_str()) {
            (vec![method.clone()], 0)
        } else {
            return;
        };
        let message = &args[message_at].value;
        let context = args.get(message_at + 1).map(|a| &a.value);
        let context_ty = context.map(|c| self.infer_expr(c, locals));
        if self.registry.check_psr3_logging {
            self.check_psr3_message(
                &method,
                message,
                context,
                context_ty.as_ref(),
                locals,
                label,
            );
            if let Some((context, ty)) = context.zip(context_ty.as_ref()) {
                self.check_psr3_context(ty, context.span, label);
            }
        }
        if self.registry.check_psr3_exception_context && !levels.is_empty() {
            let has_exception_key = match &context_ty {
                None => false,
                Some(Type::ArrayShape(shape)) => shape
                    .field(&ShapeKey::from_name("exception"))
                    .is_some_and(|f| !f.optional),
                Some(_) => true,
            };
            if !has_exception_key
                && let Some(name) = locals
                    .iter()
                    .filter(|(key, _)| crate::checker::narrowing::is_real_variable_key(key))
                    .filter(|(_, ty)| {
                        self.registry.is_super_type_of(
                            &Type::Object("Throwable".to_string(), Vec::new()),
                            ty,
                        ) == crate::registry::Trinary::Yes
                    })
                    .map(|(name, _)| name.clone())
                    .min()
            {
                self.report(
                    label.to_string(),
                    format!("Parameter $context of logger method {method}() has no 'exception' key while ${name} holds a Throwable."),
                    e.span,
                    0,
                    "psr3.contextMissingException",
                );
            }
        }
    }

    fn check_psr3_level(&mut self, level: &Type, span: Span, label: &str) {
        if matches!(level, Type::Mixed(_)) {
            return;
        }
        let valid = string_literals(level).is_some_and(|ls| ls.iter().all(|l| LEVELS.contains(l)));
        if !valid {
            self.report(
                label.to_string(),
                format!("Parameter #1 $level of method Psr\\Log\\LoggerInterface::log() expects a PSR-3 log level, {level} given."),
                span,
                0,
                "psr3.invalidLevel",
            );
        }
    }

    fn check_psr3_message(
        &mut self,
        method: &str,
        message: &Expr,
        context: Option<&Expr>,
        context_ty: Option<&Type>,
        locals: &Locals,
        label: &str,
    ) {
        if self.is_risky_log_expr(message, locals) {
            self.report(
                label.to_string(),
                "Using interpolated strings in log messages is potentially a security risk. Use PSR-3 placeholders instead.".to_string(),
                message.span,
                0,
                "psr3.interpolated",
            );
        }
        let message_ty = self.infer_expr(message, locals);
        let Some(texts) = string_literals(&message_ty) else {
            if may_be_non_literal_string(&message_ty) {
                self.report_builder(
                    DiagnosticBuilder::new(
                        label.to_string(),
                        format!(
                            "Parameter $message of logger method {method}() is not a static string."
                        ),
                        message.span,
                        0,
                        "psr3.messageNotStatic",
                    )
                    .tip("See https://www.php-fig.org/psr/psr-3/meta/#static-log-messages"),
                );
            }
            return;
        };
        let texts: Vec<String> = texts.into_iter().map(str::to_string).collect();
        for text in &texts {
            let (double, invalid) = invalid_braces(text);
            if double {
                self.report(
                    label.to_string(),
                    format!("Parameter $message of logger method {method}() should not include double braces."),
                    message.span,
                    0,
                    "psr3.placeholderDoubleBraces",
                );
                continue;
            }
            if !invalid.is_empty() {
                self.report(
                    label.to_string(),
                    format!(
                        "Parameter $message of logger method {method}() has braces with invalid placeholder characters - {}.",
                        invalid.join(", ")
                    ),
                    message.span,
                    0,
                    "psr3.placeholderInvalidChar",
                );
            }
            let names = placeholders(text);
            if names.is_empty() {
                continue;
            }
            let Some((context, context_ty)) = context.zip(context_ty) else {
                self.report(
                    label.to_string(),
                    format!(
                        "Parameter $context of logger method {method}() is required when the message has placeholders - {}.",
                        names.iter().map(|n| format!("{{{n}}}")).collect::<Vec<_>>().join(", ")
                    ),
                    message.span,
                    0,
                    "psr3.placeholderMissingContext",
                );
                continue;
            };
            let Type::ArrayShape(shape) = context_ty else {
                continue;
            };
            let missing: Vec<String> = names
                .iter()
                .filter(|n| shape.field(&ShapeKey::from_name(n)).is_none())
                .map(|n| format!("{{{n}}}"))
                .collect();
            if !missing.is_empty() && shape.is_sealed() {
                self.report(
                    label.to_string(),
                    format!(
                        "Parameter $message of logger method {method}() has placeholders with no context key - {}.",
                        missing.join(", ")
                    ),
                    context.span,
                    0,
                    "psr3.placeholderMissingKey",
                );
            }
            for field in &shape.fields {
                let (key, value) = (field.key.to_string(), &field.ty);
                if names.contains(&key.as_str()) && !self.converts_to_string(value) {
                    self.report(
                        label.to_string(),
                        format!("Context value for placeholder {{{key}}} of logger method {method}() is {value}, which has no string form."),
                        context.span,
                        0,
                        "psr3.placeholderNotStringable",
                    );
                }
            }
        }
    }

    fn check_psr3_context(&mut self, context: &Type, span: Span, label: &str) {
        let Type::ArrayShape(shape) = context else {
            return;
        };
        for field in &shape.fields {
            let (key, value) = (field.key.to_string(), &field.ty);
            if key.is_empty() || key.parse::<i64>().is_ok() {
                self.report(
                    label.to_string(),
                    format!("Context key {key:?} of a logger call should be a non-empty string."),
                    span,
                    0,
                    "psr3.contextKeyNotString",
                );
            }
            if let Some(pattern) = &self.registry.psr3_context_key_pattern
                && !pattern.is_match(&key)
            {
                self.report(
                    label.to_string(),
                    format!(
                        "Context key {key:?} of a logger call should match {}.",
                        pattern.as_str()
                    ),
                    span,
                    0,
                    "psr3.contextKeyPattern",
                );
            }
            if key == "exception" {
                let throwable = Type::Object("Throwable".to_string(), Vec::new());
                if !matches!(value, Type::Mixed(_))
                    && self.registry.is_super_type_of(&throwable, value)
                        != crate::registry::Trinary::Yes
                {
                    self.report(
                        label.to_string(),
                        format!("Context key 'exception' of a logger call expects Throwable, {value} given."),
                        span,
                        0,
                        "psr3.contextType",
                    );
                }
            }
        }
    }

    fn is_literal_string(&self, ty: &Type) -> bool {
        match ty {
            Type::StringLiteral(_) => true,
            Type::Intersection(members) => members.iter().any(|m| {
                matches!(
                    m,
                    Type::Accessory(crate::types::AccessoryKind::LiteralString)
                )
            }),
            Type::Union(members) => members.iter().all(|m| self.is_literal_string(m)),
            _ => false,
        }
    }

    fn is_risky_log_expr(&self, e: &Expr, locals: &Locals) -> bool {
        match &e.kind {
            ExprKind::Binary(BinOp::Concat, l, r) => {
                self.is_risky_log_expr(l, locals) || self.is_risky_log_expr(r, locals)
            }
            ExprKind::Interpolated(parts) => parts.iter().any(|part| {
                !matches!(part.kind, ExprKind::Literal(Literal::String(_)))
                    && self.is_risky_value(part, locals)
            }),
            ExprKind::Grouping(inner) => self.is_risky_log_expr(inner, locals),
            _ => false,
        }
    }

    fn is_risky_value(&self, e: &Expr, locals: &Locals) -> bool {
        if matches!(e.kind, ExprKind::Literal(_)) {
            return false;
        }
        let ty = match &e.kind {
            ExprKind::Variable(name) => locals
                .native(name)
                .cloned()
                .unwrap_or_else(|| self.infer_expr(e, locals)),
            ExprKind::Binary(BinOp::Concat, ..) | ExprKind::Interpolated(_) => {
                return self.is_risky_log_expr(e, locals);
            }
            _ => self.infer_expr(e, locals),
        };
        !self.is_safe_log_value(&ty)
    }

    fn is_safe_log_value(&self, ty: &Type) -> bool {
        match ty {
            Type::Int
            | Type::IntLiteral(_)
            | Type::IntRange(..)
            | Type::Float
            | Type::FloatLiteral(_)
            | Type::Bool
            | Type::True
            | Type::False
            | Type::StringLiteral(_)
            | Type::EnumCase(..) => true,
            Type::Object(class, _) => self
                .registry
                .classes
                .get(class.as_str())
                .is_some_and(|info| info.kind == ClassKind::Enum),
            Type::Union(members) => members.iter().all(|m| self.is_safe_log_value(m)),
            Type::Intersection(_) => self.is_literal_string(ty),
            _ => false,
        }
    }
}
