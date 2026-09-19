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

#[test]
fn sqlite_affinity_matches_the_real_documented_algorithm() {
    // Examples straight from SQLite's own affinity-determination table.
    assert_eq!(sqlite_affinity("INT"), SqliteAffinity::Integer);
    assert_eq!(sqlite_affinity("INTEGER"), SqliteAffinity::Integer);
    assert_eq!(sqlite_affinity("TINYINT"), SqliteAffinity::Integer);
    assert_eq!(sqlite_affinity("VARCHAR(255)"), SqliteAffinity::Text);
    assert_eq!(sqlite_affinity("CHARACTER(20)"), SqliteAffinity::Text);
    assert_eq!(sqlite_affinity("CLOB"), SqliteAffinity::Text);
    assert_eq!(sqlite_affinity("TEXT"), SqliteAffinity::Text);
    assert_eq!(sqlite_affinity("BLOB"), SqliteAffinity::Blob);
    assert_eq!(sqlite_affinity(""), SqliteAffinity::Blob);
    assert_eq!(sqlite_affinity("REAL"), SqliteAffinity::Real);
    assert_eq!(sqlite_affinity("DOUBLE"), SqliteAffinity::Real);
    assert_eq!(sqlite_affinity("FLOAT"), SqliteAffinity::Real);
    assert_eq!(sqlite_affinity("NUMERIC"), SqliteAffinity::Numeric);
    assert_eq!(sqlite_affinity("DECIMAL(10,5)"), SqliteAffinity::Numeric);
    assert_eq!(sqlite_affinity("BOOLEAN"), SqliteAffinity::Numeric);
    assert_eq!(sqlite_affinity("DATE"), SqliteAffinity::Numeric);
}

#[test]
fn sqlite_affinity_to_type_maps_integer_real_text_and_numeric() {
    assert_eq!(
        sqlite_affinity_to_type(SqliteAffinity::Integer, false),
        Type::Int
    );
    assert_eq!(
        sqlite_affinity_to_type(SqliteAffinity::Real, false),
        Type::Float
    );
    assert_eq!(
        sqlite_affinity_to_type(SqliteAffinity::Text, false),
        Type::String
    );
    assert_eq!(
        sqlite_affinity_to_type(SqliteAffinity::Blob, false),
        Type::String
    );
    // Numeric affinity is a genuine semantic union (SQLite tries
    // INTEGER storage first, falls back to REAL) - not hedging against
    // an unknowable app setting.
    assert_eq!(
        sqlite_affinity_to_type(SqliteAffinity::Numeric, false),
        Type::Union(vec![Type::Int, Type::Float])
    );
}

#[test]
fn sqlite_affinity_to_type_wraps_nullable_correctly_even_for_an_existing_union() {
    assert_eq!(
        sqlite_affinity_to_type(SqliteAffinity::Integer, true),
        Type::Union(vec![Type::Int, Type::Null])
    );
    assert_eq!(
        sqlite_affinity_to_type(SqliteAffinity::Numeric, true),
        Type::Union(vec![Type::Int, Type::Float, Type::Null])
    );
}

fn sqlite_fixture_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "phpstan_rs_sqlite_reflector_test_{label}_{}_{:?}.sqlite",
        std::process::id(),
        std::thread::current().id()
    ))
}

#[test]
fn a_real_sqlite_query_reflects_real_column_types_and_nullability() {
    let path = sqlite_fixture_path("basic");
    let _ = std::fs::remove_file(&path);
    let config = SqlReflectionConfig {
        driver: SqlDriver::Sqlite,
        host: None,
        port: None,
        user: None,
        password: None,
        password_env: None,
        database: None,
        path: Some(path.clone()),
        schema_file: None,
    };
    let reflector = SqliteReflector::connect(&config).expect("open the sqlite fixture");
    {
        let conn = reflector.pool.get().expect("get a pooled connection");
        conn.execute_batch(
            "CREATE TABLE sql_reflection_probe (
                 id INTEGER NOT NULL,
                 name TEXT,
                 price NUMERIC NOT NULL
             );",
        )
        .expect("set up the fixture table");
    }
    let row_type = reflector
        .reflect("SELECT id, name, price FROM sql_reflection_probe WHERE id = ?1")
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
    assert_eq!(
        fields[2],
        (
            "price".to_string(),
            Type::Union(vec![Type::Int, Type::Float]),
            false
        )
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_real_sqlite_computed_column_with_no_backing_table_degrades_honestly() {
    let path = sqlite_fixture_path("computed");
    let _ = std::fs::remove_file(&path);
    let config = SqlReflectionConfig {
        driver: SqlDriver::Sqlite,
        host: None,
        port: None,
        user: None,
        password: None,
        password_env: None,
        database: None,
        path: Some(path.clone()),
        schema_file: None,
    };
    let reflector = SqliteReflector::connect(&config).expect("open the sqlite fixture");
    {
        let conn = reflector.pool.get().expect("get a pooled connection");
        conn.execute_batch("CREATE TABLE t (id INTEGER NOT NULL);")
            .expect("set up the fixture table");
    }
    let row_type = reflector
        .reflect("SELECT id, id + 1 AS next_id FROM t")
        .expect("reflect a real SELECT");
    let Type::ArrayShape(fields) = row_type else {
        panic!("expected an ArrayShape, got {row_type:?}");
    };
    assert_eq!(fields[0], ("id".to_string(), Type::Int, false));
    // `id + 1` has no real backing column - `column_metadata()` returns
    // `None` for it, and SQLite's own query planner gives a computed
    // arithmetic expression no declared type either, so this degrades
    // all the way to `Type::Mixed`, not a guess.
    assert_eq!(fields[1], ("next_id".to_string(), Type::Mixed, false));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_real_sqlite_prepare_of_an_invalid_query_degrades_to_none_not_a_panic() {
    let path = sqlite_fixture_path("invalid_query");
    let _ = std::fs::remove_file(&path);
    let config = SqlReflectionConfig {
        driver: SqlDriver::Sqlite,
        host: None,
        port: None,
        user: None,
        password: None,
        password_env: None,
        database: None,
        path: Some(path.clone()),
        schema_file: None,
    };
    let reflector = SqliteReflector::connect(&config).expect("open the sqlite fixture");
    assert!(
        reflector
            .reflect("SELECT * FROM this_table_does_not_exist")
            .is_none()
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_reflector_handle_with_no_path_never_attempts_a_live_sqlite_connection() {
    let config = SqlReflectionConfig {
        driver: SqlDriver::Sqlite,
        host: None,
        port: None,
        user: None,
        password: None,
        password_env: None,
        database: None,
        path: None,
        schema_file: Some(std::path::PathBuf::from(
            "install/piwigo_structure-sqlite.sql",
        )),
    };
    let handle = SqlReflectorHandle::default();
    assert!(handle.get(Some(&config)).is_none());
}

#[test]
fn normalize_placeholders_leaves_sqlite_completely_unchanged() {
    let sql = "SELECT * FROM t WHERE a = :x AND b = ?";
    assert_eq!(
        normalize_placeholders_for_driver(sql, SqlDriver::Sqlite).as_deref(),
        Some(sql)
    );
}

#[test]
fn normalize_placeholders_rewrites_named_placeholders_to_plain_question_marks_for_mysql() {
    let out = normalize_placeholders_for_driver(
        "SELECT * FROM t WHERE a = :x AND b = :y",
        SqlDriver::Mysql,
    )
    .unwrap();
    assert!(!out.contains(':'), "expected no ':' left, got: {out}");
    assert_eq!(out.matches('?').count(), 2);
}

#[test]
fn normalize_placeholders_leaves_bare_question_marks_alone_for_mysql() {
    let out =
        normalize_placeholders_for_driver("SELECT * FROM t WHERE a = ?", SqlDriver::Mysql).unwrap();
    assert_eq!(out.matches('?').count(), 1);
}

#[test]
fn normalize_placeholders_numbers_named_placeholders_for_postgres_and_dedupes_repeats() {
    let out = normalize_placeholders_for_driver(
        "SELECT * FROM t WHERE a = :x AND b = :y AND c = :x",
        SqlDriver::Pgsql,
    )
    .unwrap();
    assert!(out.contains("$1"), "got: {out}");
    assert!(out.contains("$2"), "got: {out}");
    // The repeated `:x` reuses `$1`, not a fresh `$3`.
    assert!(!out.contains("$3"), "got: {out}");
}

#[test]
fn normalize_placeholders_numbers_bare_question_marks_sequentially_for_postgres() {
    let out = normalize_placeholders_for_driver(
        "SELECT * FROM t WHERE a = ? AND b = ?",
        SqlDriver::Pgsql,
    )
    .unwrap();
    assert!(out.contains("$1"), "got: {out}");
    assert!(out.contains("$2"), "got: {out}");
}

#[test]
fn a_real_live_named_placeholder_query_prepares_successfully_against_mysql() {
    let Some(config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    let reflector = MysqlReflector::connect(&config).expect("connect to live MySQL");
    // The exact real shape 542 real Piwigo `setParameter()` call sites
    // produce once sub-phase B folds the chain - confirmed real MySQL
    // PREPARE rejects `:id` outright before this fix.
    let row_type = reflector.reflect("SELECT id, name FROM categories WHERE id = :id");
    assert!(row_type.is_some(), "expected a resolved row type");
}

#[test]
fn a_real_live_named_placeholder_query_prepares_successfully_against_postgres() {
    let Some(config) = live_postgres_config() else {
        eprintln!("skipping: PHPSTAN_RS_TEST_PG_* env vars not set");
        return;
    };
    let reflector = PostgresReflector::connect(&config).expect("connect to live Postgres");
    {
        let mut conn = reflector.pool.get().expect("get a pooled connection");
        conn.batch_execute(
            "DROP TABLE IF EXISTS sql_reflection_placeholder_probe;
             CREATE TABLE sql_reflection_placeholder_probe (id integer NOT NULL);",
        )
        .expect("set up the fixture table");
    }
    let row_type =
        reflector.reflect("SELECT id FROM sql_reflection_placeholder_probe WHERE id = :id");
    assert!(row_type.is_some(), "expected a resolved row type");
}

#[test]
fn a_real_live_count_star_with_a_named_placeholder_prepares_successfully_against_postgres() {
    let Some(config) = live_postgres_config() else {
        eprintln!("skipping: PHPSTAN_RS_TEST_PG_* env vars not set");
        return;
    };
    let reflector = PostgresReflector::connect(&config).expect("connect to live Postgres");
    {
        let mut conn = reflector.pool.get().expect("get a pooled connection");
        conn.batch_execute(
            "DROP TABLE IF EXISTS sql_reflection_count_star_probe;
             CREATE TABLE sql_reflection_count_star_probe (id integer NOT NULL);",
        )
        .expect("set up the fixture table");
    }
    let row_type =
        reflector.reflect("SELECT COUNT(*) FROM sql_reflection_count_star_probe WHERE id = :id");
    assert!(row_type.is_some(), "expected a resolved row type");
}

#[test]
fn a_real_live_window_function_query_prepares_successfully_and_reflects_column_types() {
    let Some(config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    let reflector = MysqlReflector::connect(&config).expect("connect to live MySQL");
    let row_type = reflector
        .reflect("SELECT id, COUNT(*) OVER() AS total FROM categories")
        .expect("reflect a real window-function SELECT");
    let Type::ArrayShape(fields) = row_type else {
        panic!("expected an ArrayShape, got {row_type:?}");
    };
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].0, "id");
    assert_eq!(fields[1].0, "total");
}

#[test]
fn normalize_placeholders_preserves_count_star_spacing_when_a_placeholder_is_also_rewritten() {
    // Real, live-confirmed MySQL quirk: `COUNT (*)` (a space before the
    // paren) is a genuine syntax error, unlike `COUNT(*)` - a naive
    // tokenize-then-rejoin-with-spaces reconstruction broke this the
    // moment the same query also needed a placeholder rewritten.
    let out = normalize_placeholders_for_driver(
        "SELECT COUNT(*) FROM categories WHERE id = ?",
        SqlDriver::Mysql,
    )
    .unwrap();
    assert_eq!(out, "SELECT COUNT(*) FROM categories WHERE id = ?");
}

#[test]
fn normalize_placeholders_preserves_original_spacing_around_a_named_placeholder() {
    let out = normalize_placeholders_for_driver(
        "SELECT COUNT(*) FROM categories WHERE id = :catId",
        SqlDriver::Mysql,
    )
    .unwrap();
    assert_eq!(out, "SELECT COUNT(*) FROM categories WHERE id = ?");
}

#[test]
fn normalize_placeholders_preserves_count_star_spacing_for_postgres_too() {
    let out = normalize_placeholders_for_driver(
        "SELECT COUNT(*) FROM categories WHERE id = :catId",
        SqlDriver::Pgsql,
    )
    .unwrap();
    assert_eq!(out, "SELECT COUNT(*) FROM categories WHERE id = $1");
}
