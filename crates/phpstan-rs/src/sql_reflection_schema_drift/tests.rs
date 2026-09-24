use super::*;

fn col(name: &str, ty: Type, nullable: bool) -> SchemaColumn {
    SchemaColumn {
        name: name.to_string(),
        ty,
        nullable,
    }
}

#[test]
fn identical_column_lists_are_compatible_regardless_of_order() {
    let a = vec![col("id", Type::Int, false), col("name", Type::String, true)];
    let b = vec![col("name", Type::String, true), col("id", Type::Int, false)];
    assert!(columns_are_compatible(&a, &b));
}

#[test]
fn a_missing_column_is_not_compatible() {
    let a = vec![col("id", Type::Int, false), col("name", Type::String, true)];
    let b = vec![col("id", Type::Int, false)];
    assert!(!columns_are_compatible(&a, &b));
}

#[test]
fn a_different_nullable_flag_is_not_compatible() {
    let a = vec![col("id", Type::Int, false)];
    let b = vec![col("id", Type::Int, true)];
    assert!(!columns_are_compatible(&a, &b));
}

#[test]
fn a_different_type_is_not_compatible() {
    let a = vec![col("id", Type::Int, false)];
    let b = vec![col("id", Type::String, false)];
    assert!(!columns_are_compatible(&a, &b));
}

#[test]
fn a_plain_string_against_a_literal_enum_union_is_compatible() {
    // The live path's own `reflect()` never cross-references a static
    // catalog for an ENUM's real literal-value union (that only
    // happens through `reflect_with_schema`, not exposed on the trait
    // this check consults) - so a bare `String` there against the
    // static side's real literal union must not be flagged as drift.
    let static_side = vec![col(
        "status",
        Type::Union(vec![
            Type::StringLiteral("admin".to_string()),
            Type::StringLiteral("normal".to_string()),
        ]),
        false,
    )];
    let live_side = vec![col("status", Type::String, false)];
    assert!(columns_are_compatible(&static_side, &live_side));
}

#[test]
fn a_string_against_a_non_string_literal_union_is_still_incompatible() {
    // The leniency is scoped specifically to a plain `String` against a
    // union whose *every* member is a `StringLiteral` (the ENUM shape)
    // - a union that isn't that shape must still be flagged.
    let static_side = vec![col(
        "kind",
        Type::Union(vec![Type::StringLiteral("a".to_string()), Type::Int]),
        false,
    )];
    let live_side = vec![col("kind", Type::String, false)];
    assert!(!columns_are_compatible(&static_side, &live_side));
}

fn live_mysql_config() -> Option<crate::sql_reflection_config::SqlReflectionConfig> {
    Some(crate::sql_reflection_config::SqlReflectionConfig {
        driver: SqlDriver::Mysql,
        host: Some(std::env::var("PIWIGO_DB_HOST").ok()?),
        port: std::env::var("PIWIGO_DB_PORT").ok()?.parse().ok(),
        user: Some(std::env::var("PIWIGO_DB_USER").ok()?),
        password: Some(std::env::var("PIWIGO_DB_PASSWORD").ok()?),
        password_env: None,
        database: Some(std::env::var("PIWIGO_DB_BASE").ok()?),
        path: None,
        schema_file: None,
        session_init_sql: None,
    })
}

#[test]
fn a_real_live_matching_schema_produces_no_drift_warnings() {
    let Some(config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    // The real, full DDL dump `piwigo_test` was itself created from - a
    // hand-rolled partial schema (missing real columns) would be a
    // false "mismatch" against the live table's own full column list,
    // not a meaningful test of this check. Gated on the file's own
    // presence (this local checkout's own path, not vendored into this
    // crate), same "skip gracefully, never hard-fail on missing local
    // fixtures" convention as the `PIWIGO_DB_*` env vars above.
    let ddl_path =
        std::path::Path::new("/home/torres/piwigo17-frozen/install/piwigo_structure-mysql.sql");
    if !ddl_path.exists() {
        eprintln!("skipping: {} not found", ddl_path.display());
        return;
    }
    let handle = crate::sql_reflection_reflector::SqlReflectorHandle::default();
    let reflector = handle.get(Some(&config)).expect("a live reflector");
    let schema = crate::sql_reflection_schema::parse_ddl_file(ddl_path, SqlDriver::Mysql)
        .expect("parse the real DDL dump");
    let warnings = schema_drift_warnings(&schema, reflector, SqlDriver::Mysql);
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
}

#[test]
fn a_real_live_schema_mismatch_produces_a_drift_warning() {
    let Some(config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    let handle = crate::sql_reflection_reflector::SqlReflectorHandle::default();
    let reflector = handle.get(Some(&config)).expect("a live reflector");
    // `categories` has no real `made_up_column` - a deliberate mismatch
    // against the real live table.
    let schema = crate::sql_reflection_schema::parse_ddl(
        "CREATE TABLE categories (id int NOT NULL, made_up_column int NOT NULL);",
        SqlDriver::Mysql,
    );
    let warnings = schema_drift_warnings(&schema, reflector, SqlDriver::Mysql);
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one warning: {warnings:?}"
    );
}
