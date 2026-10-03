use super::super::*;
use diagnostics::DiagnosticBuilder;
use syntax::span::{LineCol, Range, Span};

pub(super) fn diag(identifier: &str, message: &str) -> Diagnostic {
    DiagnosticBuilder::new(
        message.to_string(),
        Span::default(),
        0,
        identifier.to_string(),
    )
    .build_at(Range {
        start: LineCol { line: 1, col: 1 },
        end: LineCol { line: 1, col: 1 },
    })
}

pub(super) fn non_ignorable_diag(identifier: &str, message: &str) -> Diagnostic {
    DiagnosticBuilder::new(
        message.to_string(),
        Span::default(),
        0,
        identifier.to_string(),
    )
    .non_ignorable()
    .build_at(Range {
        start: LineCol { line: 1, col: 1 },
        end: LineCol { line: 1, col: 1 },
    })
}
