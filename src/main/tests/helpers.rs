use super::super::*;

// main.rs had 0% test coverage before this - check_chunk and
// format_signature are real, deterministic logic that doesn't touch
// stdout, so they're tested directly here (collect_php_files moved to
// project.rs, tested there now).
// run_check/run_check_project/run_incremental_bench and the default
// no-args mode all print straight to stdout instead of returning
// anything - those are covered by tests/cli.rs instead, which spawns
// the actual compiled binary and checks its real output, rather than
// refactoring working CLI-glue code just to make it unit-testable in
// process.

pub(super) struct TempDir(pub(super) PathBuf);
impl TempDir {
    pub(super) fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "phpstan_rs_main_test_{label}_{}_{:?}",
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
