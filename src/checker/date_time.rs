//! `DateTime`/`DateTimeImmutable`'s own constructor-string validation.
//! Real PHPStan (`DateTimeConstructorThrowTypeExtension`) validates this
//! by constructing an actual `new DateTime($constantString)` at analysis
//! time and observing whether it throws - this project has no embedded
//! PHP runtime to do that, so `php_date_parser::validate_date_time_
//! string` (a hand-rolled port of the format families PHP's own parser
//! accepts most often, cross-checked against a real local PHP binary -
//! see that module's own doc comment) stands in instead. Mirrors
//! `check_date_interval_constructor`'s exact shape (a literal-string-only
//! direct diagnostic, not a throw-type extension, matching this
//! project's own established pattern for these two sibling checks) -
//! only fires when the port can *conclusively* prove the string
//! malformed (`Some(false)`); an unrecognized shape (`None`) stays
//! silent rather than risk a false positive against one of PHP's many
//! uncovered date formats.

use crate::ast::{Arg, ExprKind, Literal};
use crate::checker::Checker;
use crate::checker::expr::unwrap_grouping;
use crate::php_date_parser::validate_date_time_string;
use crate::token::Span;

impl<'a> Checker<'a> {
    /// `new DateTime($str)`/`new DateTimeImmutable($str)` - real PHP
    /// throws `DateMalformedStringException` (an `Exception` subclass on
    /// PHP < 8.3) when `$str` doesn't parse. Only fires for a literal
    /// string argument this checker can conclusively prove malformed - a
    /// computed string, or one whose shape isn't recognized at all, is
    /// left alone.
    pub(crate) fn check_date_time_constructor(
        &mut self,
        class: &str,
        args: &[Arg],
        span: Span,
        label: &str,
    ) {
        if class != "DateTime" && class != "DateTimeImmutable" {
            return;
        }
        let [arg] = args else { return };
        if arg.name.is_some() || arg.spread {
            return;
        }
        let ExprKind::Literal(Literal::String(spec)) = &unwrap_grouping(&arg.value).kind else {
            return;
        };
        if validate_date_time_string(spec) != Some(false) {
            return;
        }
        self.report(
            label.to_string(),
            format!(
                "{class}::__construct() date string '{spec}' is malformed and will throw at runtime"
            ),
            span,
            0,
            "argument.type",
        );
    }
}
