use super::super::*;
use crate::token::{Position, Span};

pub(super) fn diag(location: &str, message: &str) -> Diagnostic {
    Diagnostic {
        location: location.to_string(),
        message: message.to_string(),
        span: Span {
            start: Position { line: 1, col: 1 },
            end: Position { line: 1, col: 1 },
        },
        level: 0,
    }
}

pub(super) fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "phpstan_rs_baseline_test_{label}_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}
