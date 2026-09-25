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
fn absent_sql_reflection_block_parses_as_none() {
    let path = temp_path("sql_reflection_absent");
    std::fs::write(&path, "level = 5\n").unwrap();

    let config = Config::load(&path).unwrap();
    assert_eq!(config.sql_reflection, None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn loads_a_live_connection_sql_reflection_block() {
    let path = temp_path("sql_reflection_live");
    std::fs::write(
        &path,
        r#"
        [sql_reflection]
        driver = "mysql"
        host = "127.0.0.1"
        port = 3306
        user = "piwigo_test"
        password_env = "PIWIGO_DB_PASSWORD"
        database = "piwigo_test"
        "#,
    )
    .unwrap();

    let config = Config::load(&path).unwrap();
    let sql_reflection = config.sql_reflection.expect("sql_reflection present");
    assert_eq!(sql_reflection.driver, sql::config::SqlDriver::Mysql);
    assert_eq!(sql_reflection.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(sql_reflection.port, Some(3306));
    assert_eq!(sql_reflection.user.as_deref(), Some("piwigo_test"));
    assert_eq!(
        sql_reflection.password_env.as_deref(),
        Some("PIWIGO_DB_PASSWORD")
    );
    assert_eq!(sql_reflection.password, None);
    assert_eq!(sql_reflection.database.as_deref(), Some("piwigo_test"));
    assert_eq!(sql_reflection.path, None);
    assert_eq!(sql_reflection.schema_file, None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn loads_a_static_schema_only_sql_reflection_block() {
    let path = temp_path("sql_reflection_static");
    std::fs::write(
        &path,
        r#"
        [sql_reflection]
        driver = "sqlite"
        schema_file = "install/piwigo_structure-sqlite.sql"
        "#,
    )
    .unwrap();

    let config = Config::load(&path).unwrap();
    let sql_reflection = config.sql_reflection.expect("sql_reflection present");
    assert_eq!(sql_reflection.driver, sql::config::SqlDriver::Sqlite);
    assert_eq!(
        sql_reflection.schema_file,
        Some(PathBuf::from("install/piwigo_structure-sqlite.sql"))
    );
    assert_eq!(sql_reflection.host, None);

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

#[test]
fn dynamic_constant_names_accept_a_list_a_typed_table_or_mixed_entries() {
    let list: crate::config::Config =
        toml::from_str(r#"dynamic_constant_names = ["DEBUG", "App\\Settings::LEVEL"]"#).unwrap();
    assert_eq!(
        list.dynamic_constant_names.unwrap().entries(),
        vec![
            ("DEBUG".to_string(), None),
            ("App\\Settings::LEVEL".to_string(), None),
        ]
    );
    let typed: crate::config::Config = toml::from_str(
        r#"
            [dynamic_constant_names]
            DEBUG = "bool"
        "#,
    )
    .unwrap();
    assert_eq!(
        typed.dynamic_constant_names.unwrap().entries(),
        vec![("DEBUG".to_string(), Some("bool".to_string()))]
    );
    let mixed: crate::config::Config = toml::from_str(
        r#"dynamic_constant_names = [{ name = "DEBUG" }, { name = "MODE", type = "'dev'|'prod'" }]"#,
    )
    .unwrap();
    assert_eq!(
        mixed.dynamic_constant_names.unwrap().entries(),
        vec![
            ("DEBUG".to_string(), None),
            ("MODE".to_string(), Some("'dev'|'prod'".to_string())),
        ]
    );
}
