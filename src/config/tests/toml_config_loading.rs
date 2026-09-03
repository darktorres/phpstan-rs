//! Loading and parsing `Config` from a TOML file on disk.

use super::super::*;
use super::helpers::*;

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
