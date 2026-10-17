use super::super::session::{ErrorPolicy, Overrides, Session};
use super::super::*;
use crate::registry::stub_set::Stubs;
use config::ConfigFile;
use std::fs;
use std::path::{Path, PathBuf};

/// `missingType.*` (this project's `MissingTypehintCheck` family) fires
/// on almost any declaration that isn't fully, precisely typed - which
/// several of this file's own Pest fixtures (minimal, deliberately
/// untyped `it($description, $callback) {}`-style helper functions)
/// genuinely are. `checker::tests::helpers::diagnostics_for` filters
/// this same family out by default for the identical reason - see its
/// own doc comment; this is that filter's `Project::check_file`-based
/// twin, for the handful of `pest_directory_scoping` tests that call
/// `check_file` directly rather than through that helper.
pub(super) fn without_missing_typehint_diagnostics(mut d: Vec<Diagnostic>) -> Vec<Diagnostic> {
    d.retain(|x| !x.identifier.starts_with("missingType.") && !is_pure_call_result_unused(x));
    d
}

/// The transitive-purity payoff (`checker::noop::
/// check_pure_call_without_effect`) fires on a bare, discarded call to
/// any declaration this project's own registry can prove effect-free -
/// which several of this file's own Pest fixtures (a trivial, empty
/// `it($description, $callback) {}`-style helper, an empty
/// `assertContract(): void {}` method, both called as bare statements)
/// genuinely are. `checker::tests::helpers::is_pure_call_result_unused_diagnostic`
/// filters this same family out for the identical reason, in the
/// sibling `checker` test suite - see its own doc comment.
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

/// `dir`, resolved, so that the expected paths are the ones the session keys
/// its files by.
pub(super) fn resolved(dir: &TempDir) -> PathBuf {
    fs::canonicalize(&dir.0).unwrap()
}

/// A session over `root` with the default configuration, which analyses the
/// whole root, and `stub_paths` as its stub directories.
pub(super) fn session(root: &Path, stub_paths: Vec<PathBuf>) -> Session {
    let overrides = Overrides {
        stub_paths,
        no_composer: true,
        ..Overrides::default()
    };
    Session::resolve(
        &ConfigFile::empty(),
        None,
        root.to_path_buf(),
        overrides,
        Stubs::shared().clone(),
        ErrorPolicy::Degrade,
    )
    .unwrap()
}
