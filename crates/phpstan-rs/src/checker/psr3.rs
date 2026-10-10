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
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use syntax::span::Span;
use types::{AccessoryKind, ShapeKey, Type};

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
    ty.union_members()
        .iter()
        .any(|m| m.is_string().yes() && !m.is_refined_string(AccessoryKind::LiteralString).yes())
}

impl<'a> Checker<'a> {
    fn is_psr3_logger(&self, ty: &Type) -> bool {
        let logger = Type::object(("Psr\\Log\\LoggerInterface".to_string()).into(), Vec::new());
        !ty.is_mixed() && self.registry.is_super_type_of(&logger, ty) == types::TrinaryLogic::Yes
    }

    pub(crate) fn check_psr3_call(&mut self, e: &ExprNode, locals: &Locals) {
        let syntax_owner = self
            .syntax_tree_owner()
            .expect("PSR-3 call arguments retain their parsed file owner");
        let arenas = syntax_owner.arenas();
        let (method, args) = match &e.kind {
            ExprKind::MethodCall(receiver, method, args, _) => {
                let Some(receiver) = arenas.expr(*receiver) else {
                    return;
                };
                let receiver_ty = self.infer_expr(receiver, locals);
                if !self.is_psr3_logger(&receiver_ty) {
                    return;
                }
                (method.to_ascii_lowercase(), arenas.args(args.clone()))
            }
            ExprKind::StaticCall(StaticTarget::Name(class), method, args)
                if self.resolve_static_class_name(class).is_some_and(|c| {
                    c.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Log")
                }) =>
            {
                (method.to_ascii_lowercase(), arenas.args(args.clone()))
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
            let Some(level) = arenas.expr(args[0].value) else {
                return;
            };
            let level_ty = self.infer_expr(level, locals);
            if self.settings.psr3.check_psr3_logging {
                self.check_psr3_level(&level_ty, self.expr_span(level.id));
            }
            let levels = level_ty
                .constant_strings_text()
                .iter()
                .map(|l| l.to_ascii_lowercase())
                .collect();
            (levels, 1)
        } else if LEVELS.contains(&method.as_str()) {
            (vec![method.clone()], 0)
        } else {
            return;
        };
        let Some(message) = arenas.expr(args[message_at].value) else {
            return;
        };
        let context = args.get(message_at + 1).and_then(|a| arenas.expr(a.value));
        let context_ty = context.map(|c| self.infer_expr(c, locals));
        if self.settings.psr3.check_psr3_logging {
            self.check_psr3_message(&method, message, context, context_ty.as_ref(), locals);
            if let Some((context, ty)) = context.zip(context_ty.as_ref()) {
                self.check_psr3_context(ty, self.expr_span(context.id));
            }
        }
        if self.settings.psr3.check_psr3_exception_context && !levels.is_empty() {
            let has_exception_key = match &context_ty {
                None => false,
                Some(ty) => ty.as_array_shape().is_none_or(|shape| {
                    shape
                        .field(&ShapeKey::from_name("exception"))
                        .is_some_and(|f| !f.optional)
                }),
            };
            if !has_exception_key
                && let Some(name) = locals
                    .variables()
                    .into_iter()
                    .filter(|(_, ty)| {
                        self.registry
                            .is_super_type_of(&Type::object("Throwable".into(), Vec::new()), ty)
                            == types::TrinaryLogic::Yes
                    })
                    .map(|(name, _)| name)
                    .min()
            {
                report!(self, format!("Parameter $context of logger method {method}() has no 'exception' key while ${name} holds a Throwable."),
                    self.expr_span(e.id),
                    0,
                    "psr3.contextMissingException",
                );
            }
        }
    }

    fn check_psr3_level(&mut self, level: &Type, span: Span) {
        if level.is_mixed() {
            return;
        }
        let levels = level.constant_strings_text();
        let valid = !levels.is_empty() && levels.iter().all(|l| LEVELS.contains(&l.as_ref()));
        if !valid {
            report!(self, format!("Parameter #1 $level of method Psr\\Log\\LoggerInterface::log() expects a PSR-3 log level, {level} given."),
                span,
                0,
                "psr3.invalidLevel",
            );
        }
    }

    fn check_psr3_message(
        &mut self,
        method: &str,
        message: &ExprNode,
        context: Option<&ExprNode>,
        context_ty: Option<&Type>,
        locals: &Locals,
    ) {
        if self.is_risky_log_expr(message, locals) {
            report!(self, "Using interpolated strings in log messages is potentially a security risk. Use PSR-3 placeholders instead.".to_string(),
                self.expr_span(message.id),
                0,
                "psr3.interpolated",
            );
        }
        let message_ty = self.infer_expr(message, locals);
        let texts = message_ty.constant_strings_text();
        if texts.is_empty() {
            if may_be_non_literal_string(&message_ty) {
                report_builder!(self, 
                    DiagnosticBuilder::new(
                        format!(
                            "Parameter $message of logger method {method}() is not a static string."
                        ),
                        self.expr_span(message.id),
                        0,
                        "psr3.messageNotStatic",
                    )
                    .tip("See https://www.php-fig.org/psr/psr-3/meta/#static-log-messages"),
                );
            }
            return;
        }
        let texts: Vec<String> = texts.into_iter().map(|t| t.into_owned()).collect();
        for text in &texts {
            let (double, invalid) = invalid_braces(text);
            if double {
                report!(self, format!("Parameter $message of logger method {method}() should not include double braces."),
                    self.expr_span(message.id),
                    0,
                    "psr3.placeholderDoubleBraces",
                );
                continue;
            }
            if !invalid.is_empty() {
                report!(self, format!(
                        "Parameter $message of logger method {method}() has braces with invalid placeholder characters - {}.",
                        invalid.join(", ")
                    ),
                    self.expr_span(message.id),
                    0,
                    "psr3.placeholderInvalidChar",
                );
            }
            let names = placeholders(text);
            if names.is_empty() {
                continue;
            }
            let Some((context, context_ty)) = context.zip(context_ty) else {
                report!(self, format!(
                        "Parameter $context of logger method {method}() is required when the message has placeholders - {}.",
                        names.iter().map(|n| format!("{{{n}}}")).collect::<Vec<_>>().join(", ")
                    ),
                    self.expr_span(message.id),
                    0,
                    "psr3.placeholderMissingContext",
                );
                continue;
            };
            let Some(shape) = context_ty.as_array_shape() else {
                continue;
            };
            let missing: Vec<String> = names
                .iter()
                .filter(|n| shape.field(&ShapeKey::from_name(n)).is_none())
                .map(|n| format!("{{{n}}}"))
                .collect();
            if !missing.is_empty() && shape.is_sealed() {
                report!(self, format!(
                        "Parameter $message of logger method {method}() has placeholders with no context key - {}.",
                        missing.join(", ")
                    ),
                    self.expr_span(context.id),
                    0,
                    "psr3.placeholderMissingKey",
                );
            }
            for field in shape.fields() {
                let (key, value) = (field.key.to_string(), &field.ty);
                if names.contains(&key.as_str()) && !self.converts_to_string(value) {
                    report!(self, format!("Context value for placeholder {{{key}}} of logger method {method}() is {value}, which has no string form."),
                        self.expr_span(context.id),
                        0,
                        "psr3.placeholderNotStringable",
                    );
                }
            }
        }
    }

    fn check_psr3_context(&mut self, context: &Type, span: Span) {
        let Some(shape) = context.as_array_shape() else {
            return;
        };
        for field in shape.fields() {
            let (key, value) = (field.key.to_string(), &field.ty);
            if key.is_empty() || key.parse::<i64>().is_ok() {
                report!(self, 
                    format!("Context key {key:?} of a logger call should be a non-empty string."),
                    span,
                    0,
                    "psr3.contextKeyNotString",
                );
            }
            if let Some(pattern) = &self.settings.psr3_context_key_pattern
                && !pattern.is_match(&key)
            {
                report!(self, 
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
                let throwable = Type::object("Throwable".into(), Vec::new());
                if !value.is_mixed()
                    && self.registry.is_super_type_of(&throwable, value) != types::TrinaryLogic::Yes
                {
                    report!(self, format!("Context key 'exception' of a logger call expects Throwable, {value} given."),
                        span,
                        0,
                        "psr3.contextType",
                    );
                }
            }
        }
    }

    fn is_risky_log_expr(&self, e: &ExprNode, locals: &Locals) -> bool {
        let Some(arenas) = self.syntax_arenas() else {
            return false;
        };
        let Some(e) = arenas.peeled(e.id).and_then(|id| arenas.expr(id)) else {
            return false;
        };
        syntax::stack::guard(|| match &e.kind {
            ExprKind::Binary(BinOp::Concat, l, r) => {
                let (Some(l), Some(r)) = (arenas.expr(*l), arenas.expr(*r)) else {
                    return false;
                };
                self.is_risky_log_expr(l, locals) || self.is_risky_log_expr(r, locals)
            }
            ExprKind::Interpolated(parts) => arenas.expr_ids(parts).iter().any(|id| {
                arenas.expr(*id).is_some_and(|part| {
                    !matches!(part.kind, ExprKind::Literal(Literal::String(_)))
                        && self.is_risky_value(part, locals)
                })
            }),
            _ => false,
        })
    }

    fn is_risky_value(&self, e: &ExprNode, locals: &Locals) -> bool {
        if matches!(e.kind, ExprKind::Literal(_)) {
            return false;
        }
        let ty = match &e.kind {
            ExprKind::Variable(name) => locals
                .native_var(name)
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
        ty.union_members().iter().all(|m| {
            m.is_int().yes()
                || m.is_float().yes()
                || m.is_bool().yes()
                || m.is_refined_string(AccessoryKind::LiteralString).yes()
                || m.is_enum_case()
                || m.as_object().is_some_and(|(class, _)| {
                    self.registry
                        .class_by_name(class.as_str())
                        .is_some_and(|info| info.kind() == ClassKind::Enum)
                })
        })
    }
}
