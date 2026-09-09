use super::super::*;
use std::path::PathBuf;

/// Phase 58: `missingType.*` (this project's `MissingTypehintCheck`
/// family) fires on almost any declaration that isn't fully, precisely
/// typed - which several of this file's own Pest fixtures (minimal,
/// deliberately untyped `it($description, $callback) {}`-style helper
/// functions, written long before this family existed) genuinely are.
/// `checker::tests::helpers::diagnostics_for` filters this same family
/// out by default for the identical reason - see its own doc comment;
/// this is that fix's `Project::check_file`-based twin, for the handful
/// of `pest_directory_scoping` tests that call `check_file` directly
/// rather than through that helper.
pub(super) fn without_missing_typehint_diagnostics(mut d: Vec<Diagnostic>) -> Vec<Diagnostic> {
    d.retain(|x| !x.identifier.starts_with("missingType.") && !is_pure_call_result_unused(x));
    d
}

/// Phase 41/82's transitive-purity payoff (`checker::noop::
/// check_pure_call_without_effect`) fires on a bare, discarded call to
/// any declaration this project's own registry can prove effect-free -
/// which several of this file's own Pest fixtures (a trivial, empty
/// `it($description, $callback) {}`-style helper, an empty
/// `assertContract(): void {}` method, both called as bare statements)
/// genuinely are. `checker::tests::helpers::is_pure_call_result_unused_diagnostic`
/// filters this same family out for the identical reason, in the
/// sibling `checker` test suite - see its own doc comment; this
/// function's own name stays `without_missing_typehint_diagnostics`
/// (not renamed) since every existing call site already names it that
/// way and the two exclusions belong together for the same underlying
/// reason (a real, correct diagnostic family that predates, and is
/// incidental to, what these particular fixtures mean to test).
fn is_pure_call_result_unused(d: &Diagnostic) -> bool {
    matches!(
        d.identifier.as_str(),
        "function.resultUnused"
            | "method.resultUnused"
            | "staticMethod.resultUnused"
            | "new.resultUnused"
    )
}

pub(super) struct TempDir(pub(super) PathBuf);
impl TempDir {
    pub(super) fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "phpstan_rs_project_test_{label}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    pub(super) fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let path = self.0.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
