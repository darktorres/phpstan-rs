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
    d.retain(|x| !x.identifier.starts_with("missingType."));
    d
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
