//! `DateTime`/`DateTimeImmutable`'s constructor-string validation
//! (PHPStan's `DateTimeInstantiationRule`, which constructs the object at
//! analysis time). With no PHP runtime to do that,
//! `php_date_parser::validate_date_time_string` - a port of the format
//! families PHP's parser accepts most often - stands in: a string is
//! reported only when the port proves it malformed (`Some(false)`); one
//! of a shape it doesn't recognize (`None`) is left alone. The port
//! knows that a string is malformed, not timelib's wording of why, so
//! the message is this project's own.

use crate::checker::{Checker, Locals};
use crate::php_date_parser::validate_date_time_string;
use syntax::ast::Arg;
use syntax::span::Span;

impl<'a> Checker<'a> {
    /// `new DateTime($str)`/`new DateTimeImmutable($str)` - real PHP
    /// throws `DateMalformedStringException` (an `Exception` subclass on
    /// PHP < 8.3) when `$str` doesn't parse. Every constant string the
    /// first argument's type holds is checked (PHPStan's
    /// `DateTimeInstantiationRule`); one this port can conclusively prove
    /// malformed is reported, one whose shape it doesn't recognize is
    /// left alone.
    pub(crate) fn check_date_time_constructor(
        &mut self,
        class: &str,
        args: &[Arg],
        span: Span,
        locals: &Locals,
    ) {
        let identifier = match class {
            "DateTime" => "new.dateTime",
            "DateTimeImmutable" => "new.dateTimeImmutable",
            _ => return,
        };
        let Some(arg) = args.first() else { return };
        let ty = self.infer_expr(&arg.value, locals);
        for spec in ty.constant_strings_text() {
            if validate_date_time_string(&spec) != Some(false) {
                continue;
            }
            self.report(format!(
                    "{class}::__construct() date string '{spec}' is malformed and will throw at runtime"
                ),
                span,
                5,
                identifier,
            );
        }
    }
}
