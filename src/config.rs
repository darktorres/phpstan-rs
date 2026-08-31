//! Phase 11 Stage 6's TOML config file - a minimal, optional standing
//! project setting (paths/level/baseline/strict-mixed-arrays), read by
//! `check-project`'s `--config` flag. Every field is optional and CLI
//! flags always take precedence over the matching config value (see
//! `main.rs::run_check_project`'s own resolution logic) - a config file
//! only fills in what the command line didn't already say.
//!
//! Example:
//!
//! ```toml
//! paths = ["src", "tests"]
//! level = 5
//! baseline = "phpstan-baseline.json"
//! strict_mixed_arrays = false
//! scan_paths = ["vendor/some-manual-extra"]
//! no_composer = false
//! ```

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct Config {
    pub paths: Option<Vec<PathBuf>>,
    pub level: Option<u8>,
    pub baseline: Option<PathBuf>,
    pub strict_mixed_arrays: Option<bool>,
    /// Extra directories loaded for type-resolution context only - never
    /// analyzed or reported on. See `composer::discover_vendor_context`
    /// for the automatic `vendor/` counterpart to this.
    pub scan_paths: Option<Vec<PathBuf>>,
    /// Disables the automatic Composer-autoloader-based `vendor/`
    /// context load (see `composer::discover_vendor_context`).
    pub no_composer: Option<bool>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("failed to parse {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "phpstan_rs_config_test_{label}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn loads_every_field_from_a_real_toml_file() {
        let path = temp_path("full");
        std::fs::write(
            &path,
            r#"
            paths = ["src", "tests"]
            level = 5
            baseline = "phpstan-baseline.json"
            strict_mixed_arrays = true
            scan_paths = ["vendor/some-manual-extra"]
            no_composer = true
            "#,
        )
        .unwrap();

        let config = Config::load(&path).unwrap();
        assert_eq!(
            config.paths,
            Some(vec![PathBuf::from("src"), PathBuf::from("tests")])
        );
        assert_eq!(config.level, Some(5));
        assert_eq!(
            config.baseline,
            Some(PathBuf::from("phpstan-baseline.json"))
        );
        assert_eq!(config.strict_mixed_arrays, Some(true));
        assert_eq!(
            config.scan_paths,
            Some(vec![PathBuf::from("vendor/some-manual-extra")])
        );
        assert_eq!(config.no_composer, Some(true));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn every_field_is_optional() {
        let path = temp_path("empty");
        std::fs::write(&path, "").unwrap();

        let config = Config::load(&path).unwrap();
        assert_eq!(config, Config::default());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_is_a_real_error_not_a_panic() {
        let path = temp_path("does_not_exist");
        let err = Config::load(&path).unwrap_err();
        assert!(err.contains("failed to read"), "got: {err}");
    }

    #[test]
    fn malformed_toml_is_a_real_error_not_a_panic() {
        let path = temp_path("malformed");
        std::fs::write(&path, "this is not valid = = toml").unwrap();

        let err = Config::load(&path).unwrap_err();
        assert!(err.contains("failed to parse"), "got: {err}");

        let _ = std::fs::remove_file(&path);
    }
}
