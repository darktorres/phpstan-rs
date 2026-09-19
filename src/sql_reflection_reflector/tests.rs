use super::*;

fn column(
    column_type: ColumnType,
    flags: ColumnFlags,
    org_table: &str,
    org_name: &str,
) -> mysql::Column {
    mysql::Column::new(column_type)
        .with_flags(flags)
        .with_name(org_name.as_bytes())
        .with_org_name(org_name.as_bytes())
        .with_table(org_table.as_bytes())
        .with_org_table(org_table.as_bytes())
}

fn not_null(column_type: ColumnType) -> mysql::Column {
    column(column_type, ColumnFlags::NOT_NULL_FLAG, "t", "c")
}

fn nullable(column_type: ColumnType) -> mysql::Column {
    column(column_type, ColumnFlags::empty(), "t", "c")
}

#[test]
fn integer_family_is_always_int_never_bool_even_for_tinyint_one() {
    for ty in [
        ColumnType::MYSQL_TYPE_TINY,
        ColumnType::MYSQL_TYPE_SHORT,
        ColumnType::MYSQL_TYPE_INT24,
        ColumnType::MYSQL_TYPE_LONG,
        ColumnType::MYSQL_TYPE_LONGLONG,
    ] {
        assert_eq!(mysql_column_to_type(&not_null(ty), None), Type::Int);
    }
}

#[test]
fn decimal_is_string_but_float_double_are_the_native_type() {
    assert_eq!(
        mysql_column_to_type(&not_null(ColumnType::MYSQL_TYPE_NEWDECIMAL), None),
        Type::String
    );
    assert_eq!(
        mysql_column_to_type(&not_null(ColumnType::MYSQL_TYPE_FLOAT), None),
        Type::Float
    );
    assert_eq!(
        mysql_column_to_type(&not_null(ColumnType::MYSQL_TYPE_DOUBLE), None),
        Type::Float
    );
}

#[test]
fn json_and_date_time_family_are_string() {
    assert_eq!(
        mysql_column_to_type(&not_null(ColumnType::MYSQL_TYPE_JSON), None),
        Type::String
    );
    for ty in [
        ColumnType::MYSQL_TYPE_DATE,
        ColumnType::MYSQL_TYPE_DATETIME,
        ColumnType::MYSQL_TYPE_TIMESTAMP,
        ColumnType::MYSQL_TYPE_TIME,
        ColumnType::MYSQL_TYPE_YEAR,
    ] {
        assert_eq!(mysql_column_to_type(&not_null(ty), None), Type::String);
    }
}

#[test]
fn a_nullable_column_wraps_the_base_type_in_a_union_with_null() {
    assert_eq!(
        mysql_column_to_type(&nullable(ColumnType::MYSQL_TYPE_LONG), None),
        Type::Union(vec![Type::Int, Type::Null])
    );
}

#[test]
fn an_enum_column_falls_back_to_string_with_no_schema_configured() {
    assert_eq!(
        mysql_column_to_type(&not_null(ColumnType::MYSQL_TYPE_ENUM), None),
        Type::String
    );
}

#[test]
fn an_enum_column_resolves_its_real_literal_union_from_the_static_schema_catalog() {
    let schema = crate::sql_reflection_schema::parse_ddl(
        "CREATE TABLE users (status enum('webmaster','admin','normal') NOT NULL);",
        SqlDriver::Mysql,
    );
    let col = column(
        ColumnType::MYSQL_TYPE_ENUM,
        ColumnFlags::NOT_NULL_FLAG,
        "users",
        "status",
    );
    assert_eq!(
        mysql_column_to_type(&col, Some(&schema)),
        Type::Union(vec![
            Type::StringLiteral("webmaster".to_string()),
            Type::StringLiteral("admin".to_string()),
            Type::StringLiteral("normal".to_string()),
        ])
    );
}

#[test]
fn an_enum_column_not_found_in_the_schema_catalog_falls_back_to_string() {
    let schema = crate::sql_reflection_schema::parse_ddl(
        "CREATE TABLE users (status enum('a','b') NOT NULL);",
        SqlDriver::Mysql,
    );
    let col = column(
        ColumnType::MYSQL_TYPE_ENUM,
        ColumnFlags::NOT_NULL_FLAG,
        "some_other_table",
        "status",
    );
    assert_eq!(mysql_column_to_type(&col, Some(&schema)), Type::String);
}

#[test]
fn a_reflector_handle_with_no_config_never_builds_a_reflector() {
    let handle = SqlReflectorHandle::default();
    assert!(handle.get(None).is_none());
}

#[test]
fn a_reflector_handle_with_a_schema_file_only_config_never_attempts_a_live_connection() {
    let config = SqlReflectionConfig {
        driver: SqlDriver::Mysql,
        host: None,
        port: None,
        user: None,
        password: None,
        password_env: None,
        database: None,
        path: None,
        schema_file: Some(std::path::PathBuf::from(
            "install/piwigo_structure-mysql.sql",
        )),
    };
    let handle = SqlReflectorHandle::default();
    assert!(handle.get(Some(&config)).is_none());
}

/// Real connection details, mirroring Piwigo's own `.env.test`
/// convention (`PIWIGO_DB_HOST`/`PIWIGO_DB_USER`/`PIWIGO_DB_PASSWORD`/
/// `PIWIGO_DB_BASE`/`PIWIGO_DB_PORT`) - a plain early `return` (not
/// `#[ignore]`) when any is unset, so default `cargo test` stays green
/// with no DB present. This env-var-gated-skip mechanism is new to this
/// crate (confirmed zero existing precedent) - borrowed by analogy from
/// Piwigo's own PHP-side convention, not an existing internal one.
fn live_mysql_config() -> Option<SqlReflectionConfig> {
    Some(SqlReflectionConfig {
        driver: SqlDriver::Mysql,
        host: Some(std::env::var("PIWIGO_DB_HOST").ok()?),
        port: std::env::var("PIWIGO_DB_PORT").ok()?.parse().ok(),
        user: Some(std::env::var("PIWIGO_DB_USER").ok()?),
        password: Some(std::env::var("PIWIGO_DB_PASSWORD").ok()?),
        password_env: None,
        database: Some(std::env::var("PIWIGO_DB_BASE").ok()?),
        path: None,
        schema_file: None,
    })
}

#[test]
fn a_real_live_query_against_piwigo_test_reflects_the_real_column_types() {
    let Some(config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    let reflector = MysqlReflector::connect(&config).expect("connect to live MySQL");
    let row_type = reflector
        .reflect("SELECT id, name FROM categories WHERE id = ?")
        .expect("reflect a real SELECT");
    let Type::ArrayShape(fields) = row_type else {
        panic!("expected an ArrayShape, got {row_type:?}");
    };
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].0, "id");
    assert_eq!(fields[1].0, "name");
    // `categories.id` is a real NOT NULL primary key int, `name` a real
    // NOT NULL varchar - both confirmed against the live schema, not
    // assumed.
    assert_eq!(fields[0].1, Type::Int);
    assert_eq!(fields[1].1, Type::String);
}

#[test]
fn a_real_live_prepare_of_an_invalid_query_degrades_to_none_not_a_panic() {
    let Some(config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    let reflector = MysqlReflector::connect(&config).expect("connect to live MySQL");
    assert!(
        reflector
            .reflect("SELECT * FROM this_table_does_not_exist")
            .is_none()
    );
}

#[test]
fn a_reflector_handle_lazily_builds_a_real_live_reflector_end_to_end() {
    let Some(config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    let handle = SqlReflectorHandle::default();
    let reflector = handle.get(Some(&config)).expect("a live reflector");
    assert!(reflector.reflect("SELECT id FROM categories").is_some());
}

#[test]
fn postgres_bool_int_float_are_native_but_numeric_is_string() {
    use postgres::types::Type as PgType;
    assert_eq!(postgres_column_to_type(&PgType::BOOL, false), Type::Bool);
    assert_eq!(postgres_column_to_type(&PgType::INT4, false), Type::Int);
    assert_eq!(postgres_column_to_type(&PgType::INT8, false), Type::Int);
    assert_eq!(postgres_column_to_type(&PgType::FLOAT4, false), Type::Float);
    assert_eq!(postgres_column_to_type(&PgType::FLOAT8, false), Type::Float);
    assert_eq!(
        postgres_column_to_type(&PgType::NUMERIC, false),
        Type::String
    );
}

#[test]
fn postgres_json_and_date_time_family_are_string() {
    use postgres::types::Type as PgType;
    for ty in [PgType::JSON, PgType::JSONB] {
        assert_eq!(postgres_column_to_type(&ty, false), Type::String);
    }
    for ty in [
        PgType::DATE,
        PgType::TIME,
        PgType::TIMETZ,
        PgType::TIMESTAMP,
        PgType::TIMESTAMPTZ,
    ] {
        assert_eq!(postgres_column_to_type(&ty, false), Type::String);
    }
}

#[test]
fn postgres_a_nullable_column_wraps_the_base_type_in_a_union_with_null() {
    assert_eq!(
        postgres_column_to_type(&postgres::types::Type::INT4, true),
        Type::Union(vec![Type::Int, Type::Null])
    );
}

#[test]
fn postgres_an_unrecognized_type_degrades_to_mixed() {
    assert_eq!(
        postgres_column_to_type(&postgres::types::Type::INT4_ARRAY, false),
        Type::Mixed
    );
}

/// A dedicated, throwaway Postgres role+database (`phpstan_rs_test`) -
/// this project's own, not Piwigo's - Postgres has no live corpus-
/// schema target the way MySQL's `piwigo_test` does (confirmed: no
/// `piwigo_test`-equivalent Postgres database exists), so this env-var
/// set is separate from `PIWIGO_DB_*` and names this project's own
/// fixture instead.
fn live_postgres_config() -> Option<SqlReflectionConfig> {
    Some(SqlReflectionConfig {
        driver: SqlDriver::Pgsql,
        host: Some(std::env::var("PHPSTAN_RS_TEST_PG_HOST").ok()?),
        port: std::env::var("PHPSTAN_RS_TEST_PG_PORT").ok()?.parse().ok(),
        user: Some(std::env::var("PHPSTAN_RS_TEST_PG_USER").ok()?),
        password: Some(std::env::var("PHPSTAN_RS_TEST_PG_PASSWORD").ok()?),
        password_env: None,
        database: Some(std::env::var("PHPSTAN_RS_TEST_PG_DATABASE").ok()?),
        path: None,
        schema_file: None,
    })
}

#[test]
fn a_real_live_postgres_query_reflects_the_real_column_types_and_nullability() {
    let Some(config) = live_postgres_config() else {
        eprintln!("skipping: PHPSTAN_RS_TEST_PG_* env vars not set");
        return;
    };
    let reflector = PostgresReflector::connect(&config).expect("connect to live Postgres");
    {
        let mut conn = reflector.pool.get().expect("get a pooled connection");
        conn.batch_execute(
            "DROP TABLE IF EXISTS sql_reflection_probe;
             CREATE TABLE sql_reflection_probe (
                 id integer NOT NULL,
                 name text,
                 price numeric(10,2) NOT NULL
             );",
        )
        .expect("set up the fixture table");
    }
    let row_type = reflector
        .reflect("SELECT id, name, price FROM sql_reflection_probe WHERE id = $1")
        .expect("reflect a real SELECT");
    let Type::ArrayShape(fields) = row_type else {
        panic!("expected an ArrayShape, got {row_type:?}");
    };
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0], ("id".to_string(), Type::Int, false));
    assert_eq!(
        fields[1],
        (
            "name".to_string(),
            Type::Union(vec![Type::String, Type::Null]),
            false
        )
    );
    assert_eq!(fields[2], ("price".to_string(), Type::String, false));
}

#[test]
fn a_real_live_postgres_prepare_of_an_invalid_query_degrades_to_none_not_a_panic() {
    let Some(config) = live_postgres_config() else {
        eprintln!("skipping: PHPSTAN_RS_TEST_PG_* env vars not set");
        return;
    };
    let reflector = PostgresReflector::connect(&config).expect("connect to live Postgres");
    assert!(
        reflector
            .reflect("SELECT * FROM this_table_does_not_exist")
            .is_none()
    );
}
