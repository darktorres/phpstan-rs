use super::super::*;

pub(super) fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "phpstan_rs_config_test_{label}_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}
