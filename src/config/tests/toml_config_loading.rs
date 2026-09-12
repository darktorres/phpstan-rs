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
        stub_paths = ["phpstan-stubs/Imagick.stub.php"]
        check_duplicate_class_declarations = true
        no_composer = true
        php_version = "8.1"
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
    assert_eq!(
        config.stub_paths,
        Some(vec![PathBuf::from("phpstan-stubs/Imagick.stub.php")])
    );
    assert_eq!(config.check_duplicate_class_declarations, Some(true));
    assert_eq!(config.no_composer, Some(true));
    assert_eq!(config.php_version, Some("8.1".to_string()));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn loads_every_ignore_error_field_combination() {
    let path = temp_path("ignore_errors");
    std::fs::write(
        &path,
        r#"
        [[ignore_errors]]
        identifier = "missingType.iterableValue"

        [[ignore_errors]]
        path = "tests/Unit/Db/BatchWriterTest.php"

        [[ignore_errors]]
        identifier = "missingType.iterableValue"
        path = "_analysis/phpstan-latte/*"

        [[ignore_errors]]
        message = '^Only booleans are allowed.*$'
        path = "_analysis/phpstan-latte/*"
        count = 1
        "#,
    )
    .unwrap();

    let config = Config::load(&path).unwrap();
    let entries = config.ignore_errors.expect("ignore_errors present");
    assert_eq!(entries.len(), 4);
    assert_eq!(
        entries[0].identifier.as_deref(),
        Some("missingType.iterableValue")
    );
    assert_eq!(entries[0].path, None);
    assert_eq!(
        entries[1].path.as_deref(),
        Some("tests/Unit/Db/BatchWriterTest.php")
    );
    assert_eq!(entries[1].identifier, None);
    assert_eq!(
        entries[2].identifier.as_deref(),
        Some("missingType.iterableValue")
    );
    assert_eq!(
        entries[2].path.as_deref(),
        Some("_analysis/phpstan-latte/*")
    );
    assert_eq!(
        entries[3].message.as_deref(),
        Some("^Only booleans are allowed.*$")
    );
    assert_eq!(entries[3].count, Some(1));

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
