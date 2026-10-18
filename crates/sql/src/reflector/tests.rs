use super::*;

/// Each column as `(name, type, nullable)`.
fn named_fields(columns: Vec<Column>) -> Vec<(String, ColumnType, bool)> {
    columns
        .into_iter()
        .map(|c| (c.name, c.ty, c.nullable))
        .collect()
}

fn column(
    column_type: MysqlType,
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

fn not_null(column_type: MysqlType) -> mysql::Column {
    column(column_type, ColumnFlags::NOT_NULL_FLAG, "t", "c")
}

fn nullable(column_type: MysqlType) -> mysql::Column {
    column(column_type, ColumnFlags::empty(), "t", "c")
}

#[test]
fn integer_family_is_always_int_never_bool_even_for_tinyint_one() {
    for ty in [
        MysqlType::MYSQL_TYPE_TINY,
        MysqlType::MYSQL_TYPE_SHORT,
        MysqlType::MYSQL_TYPE_INT24,
        MysqlType::MYSQL_TYPE_LONG,
        MysqlType::MYSQL_TYPE_LONGLONG,
    ] {
        assert_eq!(mysql_column(&not_null(ty), None).ty, ColumnType::Int);
    }
}

#[test]
fn decimal_is_string_but_float_double_are_the_native_type() {
    assert_eq!(
        mysql_column(&not_null(MysqlType::MYSQL_TYPE_NEWDECIMAL), None).ty,
        ColumnType::String
    );
    assert_eq!(
        mysql_column(&not_null(MysqlType::MYSQL_TYPE_FLOAT), None).ty,
        ColumnType::Float
    );
    assert_eq!(
        mysql_column(&not_null(MysqlType::MYSQL_TYPE_DOUBLE), None).ty,
        ColumnType::Float
    );
}

#[test]
fn json_and_date_time_family_are_string() {
    assert_eq!(
        mysql_column(&not_null(MysqlType::MYSQL_TYPE_JSON), None).ty,
        ColumnType::String
    );
    for ty in [
        MysqlType::MYSQL_TYPE_DATE,
        MysqlType::MYSQL_TYPE_DATETIME,
        MysqlType::MYSQL_TYPE_TIMESTAMP,
        MysqlType::MYSQL_TYPE_TIME,
        MysqlType::MYSQL_TYPE_YEAR,
    ] {
        assert_eq!(mysql_column(&not_null(ty), None).ty, ColumnType::String);
    }
}

#[test]
fn a_nullable_column_is_reported_nullable() {
    let column = mysql_column(&nullable(MysqlType::MYSQL_TYPE_LONG), None);
    assert_eq!(column.ty, ColumnType::Int);
    assert!(column.nullable);
    assert!(!mysql_column(&not_null(MysqlType::MYSQL_TYPE_LONG), None).nullable);
}

#[test]
fn an_enum_column_falls_back_to_string_with_no_schema_configured() {
    assert_eq!(
        mysql_column(&not_null(MysqlType::MYSQL_TYPE_ENUM), None).ty,
        ColumnType::String
    );
}

#[test]
fn an_enum_column_resolves_its_real_literal_union_from_the_static_schema_catalog() {
    let schema = crate::schema::parse_ddl(
        "CREATE TABLE users (status enum('webmaster','admin','normal') NOT NULL);",
        SqlDriver::Mysql,
    );
    let col = column(
        MysqlType::MYSQL_TYPE_ENUM,
        ColumnFlags::NOT_NULL_FLAG,
        "users",
        "status",
    );
    assert_eq!(
        mysql_column(&col, Some(&schema)).ty,
        ColumnType::Enum(vec![
            "webmaster".to_string(),
            "admin".to_string(),
            "normal".to_string(),
        ])
    );
}

#[test]
fn an_enum_column_not_found_in_the_schema_catalog_falls_back_to_string() {
    let schema = crate::schema::parse_ddl(
        "CREATE TABLE users (status enum('a','b') NOT NULL);",
        SqlDriver::Mysql,
    );
    let col = column(
        MysqlType::MYSQL_TYPE_ENUM,
        ColumnFlags::NOT_NULL_FLAG,
        "some_other_table",
        "status",
    );
    assert_eq!(mysql_column(&col, Some(&schema)).ty, ColumnType::String);
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
        session_init_sql: None,
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
        session_init_sql: None,
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
    let fields = named_fields(row_type);
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].0, "id");
    assert_eq!(fields[1].0, "name");
    // `categories.id` is a real NOT NULL primary key int, `name` a real
    // NOT NULL varchar - both confirmed against the live schema, not
    // assumed.
    assert_eq!(fields[0].1, ColumnType::Int);
    assert_eq!(fields[1].1, ColumnType::String);
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
    assert_eq!(postgres_column_type(&PgType::BOOL), ColumnType::Bool);
    assert_eq!(postgres_column_type(&PgType::INT4), ColumnType::Int);
    assert_eq!(postgres_column_type(&PgType::INT8), ColumnType::Int);
    assert_eq!(postgres_column_type(&PgType::FLOAT4), ColumnType::Float);
    assert_eq!(postgres_column_type(&PgType::FLOAT8), ColumnType::Float);
    assert_eq!(postgres_column_type(&PgType::NUMERIC), ColumnType::String);
}

#[test]
fn postgres_json_and_date_time_family_are_string() {
    use postgres::types::Type as PgType;
    for ty in [PgType::JSON, PgType::JSONB] {
        assert_eq!(postgres_column_type(&ty), ColumnType::String);
    }
    for ty in [
        PgType::DATE,
        PgType::TIME,
        PgType::TIMETZ,
        PgType::TIMESTAMP,
        PgType::TIMESTAMPTZ,
    ] {
        assert_eq!(postgres_column_type(&ty), ColumnType::String);
    }
}

#[test]
fn postgres_an_unrecognized_type_degrades_to_mixed() {
    assert_eq!(
        postgres_column_type(&postgres::types::Type::INT4_ARRAY),
        ColumnType::Unknown
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
        session_init_sql: None,
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
    let fields = named_fields(row_type);
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0], ("id".to_string(), ColumnType::Int, false));
    assert_eq!(fields[1], ("name".to_string(), ColumnType::String, true));
    assert_eq!(fields[2], ("price".to_string(), ColumnType::String, false));
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
fn sqlite_affinity_type_maps_integer_real_text_and_numeric() {
    assert_eq!(
        sqlite_affinity_type(SqliteAffinity::Integer),
        ColumnType::Int
    );
    assert_eq!(
        sqlite_affinity_type(SqliteAffinity::Real),
        ColumnType::Float
    );
    assert_eq!(
        sqlite_affinity_type(SqliteAffinity::Text),
        ColumnType::String
    );
    assert_eq!(
        sqlite_affinity_type(SqliteAffinity::Blob),
        ColumnType::String
    );
    // Numeric affinity is a genuine semantic union (SQLite tries
    // INTEGER storage first, falls back to REAL) - not hedging against
    // an unknowable app setting.
    assert_eq!(
        sqlite_affinity_type(SqliteAffinity::Numeric),
        ColumnType::Number
    );
}

fn sqlite_fixture_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "phpstan_rs_sqlite_reflector_test_{label}_{}_{:?}.sqlite",
        std::process::id(),
        std::thread::current().id()
    ))
}

/// A database file holding `ddl`, made through a connection of its own: the
/// reflector opens read-only, so a test sets its database up first.
fn sqlite_fixture(label: &str, ddl: &str) -> std::path::PathBuf {
    let path = sqlite_fixture_path(label);
    let _ = std::fs::remove_file(&path);
    rusqlite::Connection::open(&path)
        .expect("create the sqlite fixture")
        .execute_batch(ddl)
        .expect("set up the fixture");
    path
}

fn sqlite_config(path: &std::path::Path) -> SqlReflectionConfig {
    SqlReflectionConfig {
        driver: SqlDriver::Sqlite,
        host: None,
        port: None,
        user: None,
        password: None,
        password_env: None,
        database: None,
        path: Some(path.to_path_buf()),
        schema_file: None,
        session_init_sql: None,
    }
}

#[test]
fn a_real_sqlite_query_reflects_real_column_types_and_nullability() {
    let path = sqlite_fixture(
        "basic",
        "CREATE TABLE sql_reflection_probe (
             id INTEGER NOT NULL,
             name TEXT,
             price NUMERIC NOT NULL
         );",
    );
    let reflector =
        SqliteReflector::connect(&sqlite_config(&path)).expect("open the sqlite fixture");
    let row_type = reflector
        .reflect("SELECT id, name, price FROM sql_reflection_probe WHERE id = ?1")
        .expect("reflect a real SELECT");
    let fields = named_fields(row_type);
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0], ("id".to_string(), ColumnType::Int, false));
    assert_eq!(fields[1], ("name".to_string(), ColumnType::String, true));
    assert_eq!(fields[2], ("price".to_string(), ColumnType::Number, false));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_real_sqlite_computed_column_with_no_backing_table_degrades_honestly() {
    let path = sqlite_fixture("computed", "CREATE TABLE t (id INTEGER NOT NULL);");
    let reflector =
        SqliteReflector::connect(&sqlite_config(&path)).expect("open the sqlite fixture");
    let row_type = reflector
        .reflect("SELECT id, id + 1 AS next_id FROM t")
        .expect("reflect a real SELECT");
    let fields = named_fields(row_type);
    assert_eq!(fields[0], ("id".to_string(), ColumnType::Int, false));
    // `id + 1` has no real backing column - `column_metadata()` returns
    // `None` for it, and SQLite's own query planner gives a computed
    // arithmetic expression no declared type either, so this degrades
    // all the way to `ColumnType::Unknown`, not a guess.
    assert_eq!(
        fields[1],
        ("next_id".to_string(), ColumnType::Unknown, false)
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_real_sqlite_prepare_of_an_invalid_query_degrades_to_none_not_a_panic() {
    let path = sqlite_fixture("invalid_query", "CREATE TABLE unrelated (id INTEGER);");
    let reflector =
        SqliteReflector::connect(&sqlite_config(&path)).expect("open the sqlite fixture");
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
        session_init_sql: None,
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
    // produce once the `QueryBuilder` chain is folded - confirmed real MySQL
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
    let fields = named_fields(row_type);
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

#[test]
fn without_session_init_sql_a_group_by_violating_only_full_group_by_fails_to_prepare() {
    let Some(mut config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    config.session_init_sql = None;
    let reflector = MysqlReflector::connect(&config).expect("connect to live MySQL");
    assert!(
        reflector
            .reflect("SELECT id, name FROM categories GROUP BY id_uppercat")
            .is_none()
    );
}

#[test]
fn session_init_sql_runs_on_every_new_connection_and_can_relax_sql_mode() {
    let Some(mut config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    config.session_init_sql = Some("SET SESSION sql_mode=''".to_string());
    let reflector = MysqlReflector::connect(&config).expect("connect to live MySQL");
    assert!(
        reflector
            .reflect("SELECT id, name FROM categories GROUP BY id_uppercat")
            .is_some(),
        "expected the relaxed sql_mode to let this GROUP BY prepare successfully"
    );
}

#[test]
fn sqlite_session_init_sql_runs_on_every_new_connection() {
    let path = sqlite_fixture("session_init", "CREATE TABLE unrelated (id INTEGER);");
    // A temporary table is what a read-only connection can still create.
    let config = SqlReflectionConfig {
        session_init_sql: Some(
            "CREATE TEMP TABLE session_init_probe (id INTEGER NOT NULL);".to_string(),
        ),
        ..sqlite_config(&path)
    };
    let reflector = SqliteReflector::connect(&config).expect("open the sqlite fixture");
    let row_type = reflector.reflect("SELECT id FROM session_init_probe");
    let _ = std::fs::remove_file(&path);
    assert!(
        row_type.is_some(),
        "expected session_init_sql to have created the table before this query prepared"
    );
}

#[test]
fn postgres_session_init_sql_runs_before_every_reflect_call() {
    let Some(mut config) = live_postgres_config() else {
        eprintln!("skipping: PHPSTAN_RS_TEST_PG_* env vars not set");
        return;
    };
    config.session_init_sql = Some(
        "CREATE TEMP TABLE IF NOT EXISTS session_init_probe (id integer NOT NULL);".to_string(),
    );
    let reflector = PostgresReflector::connect(&config).expect("connect to live Postgres");
    let row_type = reflector.reflect("SELECT id FROM session_init_probe");
    assert!(
        row_type.is_some(),
        "expected session_init_sql to have created the temp table before this query prepared"
    );
}

/// Gap #7 of the live SQL query reflection phase ("virtual relations").
/// Live-verified across all 3 engines that the *live* reflection path
/// needs no dedicated view/CTE/derived-table/UNION resolution logic of
/// its own at all: `reflect()` hands the whole query text to the real
/// engine's own `PREPARE`, and that engine already resolves every one
/// of these constructs as part of its own real query planning - the
/// same real column metadata comes back whether the query is a plain
/// `SELECT` or a view/CTE/derived-table/UNION wrapped around one.
/// `UNION`'s own per-branch coercion (`int UNION varchar` -> the whole
/// column reported as `string`, not a PHP-level union) is a
/// real engine behavior these tests confirm directly, not a rule this
/// project computes itself. Only the *static*-schema-only path
/// (`checker::sql_reflection::resolve`, no live connection at all)
/// needed real new code - see its own `resolve_ctes_from_static_schema`/
/// `TableFactor::Derived` handling.
#[test]
fn a_real_live_derived_table_prepares_and_reflects_via_mysql() {
    let Some(config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    let reflector = MysqlReflector::connect(&config).expect("connect to live MySQL");
    let row_type = reflector
        .reflect("SELECT t.id FROM (SELECT id FROM categories) AS t")
        .expect("reflect a derived table");
    let fields = named_fields(row_type);
    assert_eq!(fields, vec![("id".to_string(), ColumnType::Int, false)]);
}

#[test]
fn a_real_live_non_recursive_cte_prepares_and_reflects_via_mysql() {
    let Some(config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    let reflector = MysqlReflector::connect(&config).expect("connect to live MySQL");
    let row_type = reflector
        .reflect("WITH t AS (SELECT id FROM categories) SELECT id FROM t")
        .expect("reflect a CTE");
    let fields = named_fields(row_type);
    assert_eq!(fields, vec![("id".to_string(), ColumnType::Int, false)]);
}

#[test]
fn a_real_live_union_of_matching_types_stays_that_type_via_mysql() {
    let Some(config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    let reflector = MysqlReflector::connect(&config).expect("connect to live MySQL");
    let row_type = reflector
        .reflect("SELECT id FROM categories UNION SELECT id FROM images")
        .expect("reflect a UNION");
    let fields = named_fields(row_type);
    assert_eq!(fields, vec![("id".to_string(), ColumnType::Int, false)]);
}

#[test]
fn a_real_live_union_of_mismatched_types_coerces_to_a_single_string_column_via_mysql() {
    let Some(config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    let reflector = MysqlReflector::connect(&config).expect("connect to live MySQL");
    let row_type = reflector
        .reflect("SELECT id FROM categories UNION SELECT name FROM categories")
        .expect("reflect a UNION of mismatched types");
    let fields = named_fields(row_type);
    // Real MySQL: a UNION's result set has one column definition, not
    // one per branch - `int UNION varchar` reports the whole column as
    // `string`, never a per-row union.
    assert_eq!(fields, vec![("id".to_string(), ColumnType::String, false)]);
}

#[test]
fn a_real_live_view_prepares_and_reflects_the_underlying_columns_via_mysql() {
    let Some(config) = live_mysql_config() else {
        eprintln!("skipping: PIWIGO_DB_* env vars not set");
        return;
    };
    let reflector = MysqlReflector::connect(&config).expect("connect to live MySQL");
    {
        let mut conn = reflector.pool.get_conn().expect("get a pooled connection");
        conn.query_drop(
            "CREATE OR REPLACE VIEW sql_reflection_virtual_relation_probe AS
             SELECT id, name FROM categories",
        )
        .expect("set up the fixture view");
    }
    let row_type = reflector
        .reflect("SELECT id, name FROM sql_reflection_virtual_relation_probe")
        .expect("reflect a view");
    let fields = named_fields(row_type);
    assert_eq!(
        fields,
        vec![
            ("id".to_string(), ColumnType::Int, false),
            ("name".to_string(), ColumnType::String, false),
        ]
    );
}

#[test]
fn a_real_live_derived_table_cte_and_view_prepare_and_reflect_via_postgres() {
    let Some(config) = live_postgres_config() else {
        eprintln!("skipping: PHPSTAN_RS_TEST_PG_* env vars not set");
        return;
    };
    let reflector = PostgresReflector::connect(&config).expect("connect to live Postgres");
    {
        let mut conn = reflector.pool.get().expect("get a pooled connection");
        conn.batch_execute(
            "DROP TABLE IF EXISTS sql_reflection_vr_probe CASCADE;
             CREATE TABLE sql_reflection_vr_probe (id integer NOT NULL);
             CREATE OR REPLACE VIEW sql_reflection_vr_probe_view AS
                 SELECT id FROM sql_reflection_vr_probe;",
        )
        .expect("set up the fixture table and view");
    }
    for sql in [
        "SELECT t.id FROM (SELECT id FROM sql_reflection_vr_probe) AS t",
        "WITH t AS (SELECT id FROM sql_reflection_vr_probe) SELECT id FROM t",
        "SELECT id FROM sql_reflection_vr_probe UNION SELECT id FROM sql_reflection_vr_probe",
        "SELECT id FROM sql_reflection_vr_probe_view",
    ] {
        assert!(
            reflector.reflect(sql).is_some(),
            "expected a resolved row type for: {sql}"
        );
    }
}

#[test]
fn a_real_live_derived_table_cte_and_view_prepare_and_reflect_via_sqlite() {
    let path = sqlite_fixture(
        "virtual_relations",
        "CREATE TABLE vr_probe (id INTEGER NOT NULL, name TEXT);
         CREATE VIEW vr_probe_view AS SELECT id, name FROM vr_probe;",
    );
    let reflector =
        SqliteReflector::connect(&sqlite_config(&path)).expect("open the sqlite fixture");
    for sql in [
        "SELECT t.id FROM (SELECT id FROM vr_probe) AS t",
        "WITH t AS (SELECT id FROM vr_probe) SELECT id FROM t",
        "SELECT id FROM vr_probe UNION SELECT id FROM vr_probe",
        "SELECT id, name FROM vr_probe_view",
    ] {
        assert!(
            reflector.reflect(sql).is_some(),
            "expected a resolved row type for: {sql}"
        );
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_sqlite_path_that_names_no_file_is_an_error_naming_it_and_creates_nothing() {
    let path = sqlite_fixture_path("missing");
    let _ = std::fs::remove_file(&path);
    let error = SqliteReflector::connect(&sqlite_config(&path))
        .err()
        .expect("a missing file is not an empty database");
    assert!(error.contains(&path.display().to_string()), "{error}");
    assert!(!path.exists(), "opening must not create the file");
}

#[test]
fn a_sqlite_connection_only_reads_but_still_reflects_what_it_prepares() {
    let path = sqlite_fixture("read_only", "CREATE TABLE t (id INTEGER NOT NULL);");
    let reflector =
        SqliteReflector::connect(&sqlite_config(&path)).expect("open the sqlite fixture");
    {
        let conn = reflector.pool.get().expect("get a pooled connection");
        assert!(
            conn.execute("INSERT INTO t (id) VALUES (1)", []).is_err(),
            "the connection is read-only"
        );
        assert!(
            conn.prepare("INSERT INTO t (id) VALUES (?1)").is_ok(),
            "a read-only connection still prepares a write, which is all reflection does"
        );
    }
    assert!(reflector.reflect("SELECT id FROM t").is_some());
    let rows: i64 = rusqlite::Connection::open(&path)
        .expect("reopen the fixture")
        .query_row("SELECT COUNT(*) FROM t", [], |row| row.get(0))
        .expect("count the rows");
    assert_eq!(rows, 0, "reflection changed nothing");
    let _ = std::fs::remove_file(&path);
}
