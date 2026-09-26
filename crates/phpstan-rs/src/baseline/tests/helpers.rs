use super::super::*;
use diagnostics::DiagnosticBuilder;
use syntax::token::{Position, Span};

pub(super) fn diag(message: &str) -> Diagnostic {
    DiagnosticBuilder::new(
        message.to_string(),
        Span {
            start: Position { line: 1, col: 1 },
            end: Position { line: 1, col: 1 },
        },
        0,
        "test.diagnostic",
    )
    .build()
}

pub(super) fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "phpstan_rs_baseline_test_{label}_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

pub(super) fn diag_with(message: &str, identifier: &'static str) -> Diagnostic {
    DiagnosticBuilder::new(
        message.to_string(),
        Span {
            start: Position { line: 1, col: 1 },
            end: Position { line: 1, col: 1 },
        },
        0,
        identifier,
    )
    .build()
}
