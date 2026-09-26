//! Opt-in `check_current_time_dependence` (`slam/phpstan-extensions`'
//! "not now" rules): code reading the wall clock implicitly instead of
//! taking the time from an injected clock.
//!
//! - `currentTime.time`: `time()`/`microtime()`;
//! - `currentTime.defaultTimestamp`: `date()`, `gmdate()`, `idate()`,
//!   `getdate()`, `localtime()`, `strftime()`, `gmstrftime()` or
//!   `unixtojd()` without their timestamp (or with `null`), and
//!   `mktime()`/`gmmktime()` leaving a date or time component out;
//! - `currentTime.relative`: `strtotime()` without a base timestamp,
//!   `date_create()`/`date_create_immutable()` or `new` of a
//!   `DateTimeInterface` class, given no date or one relative to now
//!   (`"now"`, `"+1 day"`, `"next monday"`, a time of day alone);
//! - `currentTime.createFromFormat`: `createFromFormat()` whose format
//!   neither resets the unparsed fields (`!`, `|`) nor reads a timestamp
//!   (`U`), so they come from the current time;
//! - `currentTime.requestTime`: `$_SERVER['REQUEST_TIME']` and
//!   `REQUEST_TIME_FLOAT`.

use crate::checker::{Checker, Locals};
use crate::php_date_parser::depends_on_current_time;
use syntax::ast::*;
use syntax::token::Span;
use types::Type;

const CLOCK_TIP: &str = "rely on an injected clock (PSR-20 ClockInterface) instead";

impl<'a> Checker<'a> {
    /// Whether the date argument `arg` resolves against the current time:
    /// absent, `null`, or a string literal relative to now.
    fn date_argument_is_relative(&self, arg: Option<&Arg>, locals: &Locals) -> Option<String> {
        let Some(arg) = arg else {
            return Some("no date".to_string());
        };
        match self.infer_expr(&arg.value, locals) {
            Type::Null => Some("null".to_string()),
            Type::StringLiteral(s) if depends_on_current_time(&s) == Some(true) => {
                Some(format!("\"{s}\""))
            }
            _ => None,
        }
    }

    fn report_current_time(&mut self, message: String, span: Span, id: &'static str) {
        self.report(message, span, 0, id);
    }

    /// `function` is the resolved, lowercase function name.
    pub(crate) fn check_current_time_call(
        &mut self,
        function: &str,
        args: &[Arg],
        locals: &Locals,
        span: Span,
    ) {
        if args.iter().any(|a| a.spread || a.name.is_some()) {
            return;
        }
        let timestamp_at = match function {
            "time" | "microtime" => {
                self.report_current_time(
                    format!("Calling {function}() reads the current time; {CLOCK_TIP}."),
                    span,
                    "currentTime.time",
                );
                return;
            }
            "date" | "gmdate" | "idate" | "strftime" | "gmstrftime" => 1,
            "getdate" | "localtime" | "unixtojd" => 0,
            "mktime" | "gmmktime" => {
                if args.len() < 6 {
                    self.report_current_time(
                        format!("Calling {function}() with fewer than 6 arguments takes the missing components from the current time; {CLOCK_TIP}."),
                        span,
                        "currentTime.defaultTimestamp",
                    );
                }
                return;
            }
            "strtotime" => {
                if args.len() == 1
                    && let Some(relative) = self.date_argument_is_relative(args.first(), locals)
                {
                    self.report_current_time(
                        format!("Calling strtotime() with {relative} and no base timestamp reads the current time; {CLOCK_TIP}."),
                        span,
                        "currentTime.relative",
                    );
                }
                return;
            }
            "date_create" | "date_create_immutable" => {
                if let Some(relative) = self.date_argument_is_relative(args.first(), locals) {
                    self.report_current_time(
                        format!("Calling {function}() with {relative} reads the current time; {CLOCK_TIP}."),
                        span,
                        "currentTime.relative",
                    );
                }
                return;
            }
            "date_create_from_format" | "date_create_immutable_from_format" => {
                self.check_create_from_format(args.first(), locals, span);
                return;
            }
            _ => return,
        };
        let omitted = match args.get(timestamp_at) {
            None => true,
            Some(arg) => self.infer_expr(&arg.value, locals) == Type::Null,
        };
        if omitted {
            self.report_current_time(
                format!(
                    "Calling {function}() without a timestamp reads the current time; {CLOCK_TIP}."
                ),
                span,
                "currentTime.defaultTimestamp",
            );
        }
    }

    fn check_create_from_format(&mut self, format: Option<&Arg>, locals: &Locals, span: Span) {
        let Some(format) = format else {
            return;
        };
        let Type::StringLiteral(text) = self.infer_expr(&format.value, locals) else {
            return;
        };
        let mut escaped = false;
        let mut resets = false;
        for c in text.chars() {
            if escaped {
                escaped = false;
                continue;
            }
            match c {
                '\\' => escaped = true,
                '!' | '|' | 'U' => resets = true,
                _ => {}
            }
        }
        if !resets {
            self.report_current_time(
                format!("createFromFormat() with format \"{text}\" takes every field it doesn't parse from the current time; start the format with ! or end it with |."),
                span,
                "currentTime.createFromFormat",
            );
        }
    }

    pub(crate) fn check_current_time_expression(&mut self, e: &Expr, locals: &Locals) {
        match &e.kind {
            ExprKind::New(class, args) => {
                let ExprKind::Name(name) = &class.kind else {
                    return;
                };
                let Some(class) = self.resolve_static_class_name(name) else {
                    return;
                };
                if !self.registry.is_subclass_of(&class, "DateTimeInterface")
                    || args.iter().any(|a| a.spread || a.name.is_some())
                {
                    return;
                }
                if let Some(relative) = self.date_argument_is_relative(args.first(), locals) {
                    self.report_current_time(
                        format!("Instantiating {class} with {relative} reads the current time; {CLOCK_TIP}."),
                        e.span,
                        "currentTime.relative",
                    );
                }
            }
            ExprKind::StaticCall(StaticTarget::Name(class, _), method, args)
                if method.eq_ignore_ascii_case("createFromFormat") =>
            {
                let Some(class) = self.resolve_static_class_name(class) else {
                    return;
                };
                if self.registry.is_subclass_of(&class, "DateTimeInterface") {
                    self.check_create_from_format(args.first(), locals, e.span);
                }
            }
            ExprKind::Index(base, Some(key)) => {
                if matches!(&base.kind, ExprKind::Variable(name) if name.as_ref() == "_SERVER")
                    && let ExprKind::Literal(Literal::String(key)) = &key.kind
                    && matches!(key.as_ref(), "REQUEST_TIME" | "REQUEST_TIME_FLOAT")
                {
                    self.report_current_time(
                        format!("Reading $_SERVER['{key}'] depends on the current request's time; {CLOCK_TIP}."),
                        e.span,
                        "currentTime.requestTime",
                    );
                }
            }
            _ => {}
        }
    }
}
