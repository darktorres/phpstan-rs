//! Loading a whole `phpstan-rs.toml` through the application's schema.

use std::path::PathBuf;

use config::ConfigFile;

use super::config_schema;
use crate::checker::settings::EngineSettings;
use crate::ext::extras::ExtrasSettings;
use crate::ext::sql::SqlSettings;
use crate::project::session::SessionSettings;
use crate::registry::settings::ReflectionSettings;
use crate::rules_settings::RulesSettings;

fn load(text: &str) -> ConfigFile {
    let schema = config_schema();
    let file = ConfigFile::parse(text, &schema).unwrap();
    file.validate(&schema).unwrap();
    file
}

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "phpstan_rs_config_test_{label}_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

#[test]
fn every_section_loads_from_its_own_table() {
    let file = load(
        r#"
        paths = ["src", "tests"]
        level = 5
        baseline = "phpstan-baseline.json"
        scan_paths = ["vendor/some-manual-extra"]
        stub_paths = ["phpstan-stubs/Imagick.stub.php"]
        no_composer = true
        php_version = "8.1"

        [rules]
        strict_mixed_arrays = true
        check_duplicate_class_declarations = true

        [ext.extras]
        check_boolean_trap = true
        "#,
    );
    let session = file.section::<SessionSettings>().unwrap();
    assert_eq!(
        session.paths,
        Some(vec![PathBuf::from("src"), PathBuf::from("tests")])
    );
    assert_eq!(session.level, Some(5));
    assert_eq!(
        session.baseline,
        Some(PathBuf::from("phpstan-baseline.json"))
    );
    let reflection = file.section::<ReflectionSettings>().unwrap();
    assert_eq!(
        reflection.scan_paths,
        vec![PathBuf::from("vendor/some-manual-extra")]
    );
    assert_eq!(
        reflection.stub_paths,
        vec![PathBuf::from("phpstan-stubs/Imagick.stub.php")]
    );
    assert_eq!(reflection.no_composer, Some(true));
    assert_eq!(reflection.php_version, Some("8.1".to_string()));
    let rules = file.section::<RulesSettings>().unwrap();
    assert!(rules.strict_mixed_arrays);
    assert!(rules.check_duplicate_class_declarations);
    assert_eq!(
        file.section::<ExtrasSettings>().unwrap().check_boolean_trap,
        Some(true)
    );
}

#[test]
fn the_flat_layout_still_loads_with_a_warning_per_key() {
    let file = load(
        r#"
        strict_mixed_arrays = true
        check_boolean_trap = true
        "#,
    );
    assert!(file.section::<RulesSettings>().unwrap().strict_mixed_arrays);
    assert_eq!(
        file.section::<ExtrasSettings>().unwrap().check_boolean_trap,
        Some(true)
    );
    assert_eq!(file.warnings().len(), 2);
}

#[test]
fn a_key_no_section_owns_is_rejected_by_its_full_path() {
    let schema = config_schema();
    let file = ConfigFile::parse("[rules]\ncheck_bogus = true\n", &schema).unwrap();
    let error = file.validate(&schema).unwrap_err();
    assert!(
        error.contains("unknown config key `rules.check_bogus`"),
        "{error}"
    );
}

#[test]
fn loads_every_ignore_error_field_combination() {
    let file = load(
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
    );
    let entries = file.section::<SessionSettings>().unwrap().ignore_errors;
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
}

#[test]
fn absent_sql_reflection_block_parses_as_none() {
    let file = load("level = 5\n");
    assert_eq!(file.section::<SqlSettings>().unwrap().reflection, None);
}

#[test]
fn loads_a_live_connection_sql_reflection_block() {
    let file = load(
        r#"
        [ext.sql.reflection]
        driver = "mysql"
        host = "127.0.0.1"
        port = 3306
        user = "piwigo_test"
        password_env = "PIWIGO_DB_PASSWORD"
        database = "piwigo_test"
        "#,
    );
    let sql_reflection = file
        .section::<SqlSettings>()
        .unwrap()
        .reflection
        .expect("reflection present");
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
}

#[test]
fn loads_a_static_schema_only_sql_reflection_block() {
    let file = load(
        r#"
        [ext.sql.reflection]
        driver = "sqlite"
        schema_file = "install/piwigo_structure-sqlite.sql"
        "#,
    );
    let sql_reflection = file
        .section::<SqlSettings>()
        .unwrap()
        .reflection
        .expect("reflection present");
    assert_eq!(sql_reflection.driver, sql::config::SqlDriver::Sqlite);
    assert_eq!(
        sql_reflection.schema_file,
        Some(PathBuf::from("install/piwigo_structure-sqlite.sql"))
    );
    assert_eq!(sql_reflection.host, None);
}

#[test]
fn every_key_is_optional() {
    let file = load("");
    assert_eq!(
        file.section::<SessionSettings>().unwrap(),
        SessionSettings::default()
    );
    assert_eq!(
        file.section::<RulesSettings>().unwrap(),
        RulesSettings::default()
    );
}

#[test]
fn a_missing_file_is_a_real_error_not_a_panic() {
    let path = temp_path("does_not_exist");
    let err = ConfigFile::load(&path, &config_schema()).unwrap_err();
    assert!(err.contains("failed to read"), "got: {err}");
}

#[test]
fn malformed_toml_is_a_real_error_not_a_panic() {
    let path = temp_path("malformed");
    std::fs::write(&path, "this is not valid = = toml").unwrap();
    let err = ConfigFile::load(&path, &config_schema()).unwrap_err();
    assert!(err.contains("failed to parse"), "got: {err}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn dynamic_constant_names_accept_a_list_a_typed_table_or_mixed_entries() {
    let list = load(r#"dynamic_constant_names = ["DEBUG", "App\\Settings::LEVEL"]"#);
    assert_eq!(
        list.section::<EngineSettings>()
            .unwrap()
            .dynamic_constant_entries(),
        vec![
            ("DEBUG".to_string(), None),
            ("App\\Settings::LEVEL".to_string(), None),
        ]
    );
    let typed = load(
        r#"
            [dynamic_constant_names]
            DEBUG = "bool"
        "#,
    );
    assert_eq!(
        typed
            .section::<EngineSettings>()
            .unwrap()
            .dynamic_constant_entries(),
        vec![("DEBUG".to_string(), Some("bool".to_string()))]
    );
    let mixed = load(
        r#"dynamic_constant_names = [{ name = "DEBUG" }, { name = "MODE", type = "'dev'|'prod'" }]"#,
    );
    assert_eq!(
        mixed
            .section::<EngineSettings>()
            .unwrap()
            .dynamic_constant_entries(),
        vec![
            ("DEBUG".to_string(), None),
            ("MODE".to_string(), Some("'dev'|'prod'".to_string())),
        ]
    );
}
