use super::super::*;
use diagnostics::DiagnosticBuilder;
use syntax::token::{Position, Span};

pub(super) fn diag(identifier: &str, message: &str) -> Diagnostic {
    DiagnosticBuilder::new(
        message.to_string(),
        Span {
            start: Position { line: 1, col: 1 },
            end: Position { line: 1, col: 1 },
        },
        0,
        identifier.to_string(),
    )
    .build()
}

pub(super) fn non_ignorable_diag(identifier: &str, message: &str) -> Diagnostic {
    DiagnosticBuilder::new(
        message.to_string(),
        Span {
            start: Position { line: 1, col: 1 },
            end: Position { line: 1, col: 1 },
        },
        0,
        identifier.to_string(),
    )
    .non_ignorable()
    .build()
}
