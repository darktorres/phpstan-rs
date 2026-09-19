use super::*;

fn base_config() -> SqlReflectionConfig {
    SqlReflectionConfig {
        driver: SqlDriver::Mysql,
        host: Some("127.0.0.1".to_string()),
        port: Some(3306),
        user: Some("piwigo_test".to_string()),
        password: None,
        password_env: None,
        database: Some("piwigo_test".to_string()),
        path: None,
        schema_file: None,
    }
}

// Each test claims a distinct env var name so `cargo test`'s own
// parallel test threads (sharing one process-wide environment) never
// race each other.

#[test]
fn a_literal_password_is_used_when_no_password_env_is_set() {
    let mut config = base_config();
    config.password = Some("literal-secret".to_string());
    assert_eq!(
        config.resolved_password(),
        Ok(Some("literal-secret".to_string()))
    );
}

#[test]
fn password_env_takes_precedence_over_a_literal_password() {
    // SAFETY: test-local env var, no other thread reads this name.
    unsafe {
        std::env::set_var("PHPSTAN_RS_TEST_SQL_PW_PRECEDENCE", "from-env");
    }
    let mut config = base_config();
    config.password = Some("literal-secret".to_string());
    config.password_env = Some("PHPSTAN_RS_TEST_SQL_PW_PRECEDENCE".to_string());
    assert_eq!(config.resolved_password(), Ok(Some("from-env".to_string())));
    unsafe {
        std::env::remove_var("PHPSTAN_RS_TEST_SQL_PW_PRECEDENCE");
    }
}

#[test]
fn an_unset_password_env_is_a_fail_fast_error_not_a_silent_fallback() {
    let mut config = base_config();
    config.password = Some("literal-secret".to_string());
    config.password_env = Some("PHPSTAN_RS_TEST_SQL_PW_DEFINITELY_UNSET".to_string());
    assert_eq!(
        config.resolved_password(),
        Err(
            "sql_reflection.password_env \"PHPSTAN_RS_TEST_SQL_PW_DEFINITELY_UNSET\" is not set"
                .to_string()
        )
    );
}

#[test]
fn no_password_at_all_resolves_to_none() {
    let config = base_config();
    assert_eq!(config.resolved_password(), Ok(None));
}

#[test]
fn debug_formatting_redacts_a_literal_password() {
    let mut config = base_config();
    config.password = Some("super-secret".to_string());
    let formatted = format!("{config:?}");
    assert!(!formatted.contains("super-secret"));
    assert!(formatted.contains("<redacted>"));
}

#[test]
fn debug_formatting_never_hides_that_password_env_is_configured() {
    let mut config = base_config();
    config.password_env = Some("PIWIGO_DB_PASSWORD".to_string());
    let formatted = format!("{config:?}");
    assert!(formatted.contains("PIWIGO_DB_PASSWORD"));
}
