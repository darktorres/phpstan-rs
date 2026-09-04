use super::super::*;
use crate::checker::DiagnosticBuilder;
use crate::token::{Position, Span};

pub(super) fn diag(location: &str, message: &str) -> Diagnostic {
    DiagnosticBuilder::new(
        location.to_string(),
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
