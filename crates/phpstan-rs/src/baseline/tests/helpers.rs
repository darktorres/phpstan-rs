use super::super::*;
use diagnostics::DiagnosticBuilder;
use syntax::span::{LineCol, Range, Span};

pub(super) fn diag(message: &str) -> Diagnostic {
    DiagnosticBuilder::new(message.to_string(), Span::default(), 0, "test.diagnostic").build_at(
        Range {
            start: LineCol { line: 1, col: 1 },
            end: LineCol { line: 1, col: 1 },
        },
    )
}

/// `generate` with every file of `results` checked, as a run that analysed
/// them all passes.
pub(super) fn generate_all(
    path: &Path,
    results: &FxHashMap<PathBuf, Vec<Diagnostic>>,
) -> io::Result<usize> {
    let checked = results.keys().cloned().collect();
    generate(path, results, &checked)
}

pub(super) fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "phpstan_rs_baseline_test_{label}_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

pub(super) fn diag_with(message: &str, identifier: &'static str) -> Diagnostic {
    DiagnosticBuilder::new(message.to_string(), Span::default(), 0, identifier).build_at(Range {
        start: LineCol { line: 1, col: 1 },
        end: LineCol { line: 1, col: 1 },
    })
}
