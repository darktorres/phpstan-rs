//! Sub-phase C (MySQL only, so far - Postgres/SQLite are separate
//! follow-on commits): live SQL query reflection. `PREPARE`s (never
//! executes) a resolved SQL string against a live connection and reads
//! the prepared statement's own real result-set column metadata,
//! translating each column into this project's `Type` the way a raw
//! `Doctrine\DBAL\Connection::fetch*` call actually hands it to PHP -
//! the same real-world shape `staabm/phpstan-dba` uses.
//!
//! Every rule here mirrors `sql_reflection_schema`'s own static-catalog
//! translation table (A2) in *outcome*, not in code: the two inputs are
//! fundamentally different shapes (a parsed DDL `ColumnDef` there, a
//! live wire-protocol `Column`/`ColumnType` here), so this is a
//! deliberately separate translator, not a forced shared function -
//! kept consistent by hand, not by construction. One real precision
//! win only the live path can't get alone: MySQL's own prepared-
//! statement column metadata reports an `ENUM` column's type code, but
//! never its permitted-value list - `enum_literal_union` cross-
//! references a configured static schema catalog's own parsed
//! `CREATE TABLE` (via the column's own `org_table`/`org_name`, which
//! survive an alias) for the real literal union.

use std::sync::OnceLock;

use mysql::consts::{ColumnFlags, ColumnType};
use mysql::prelude::Queryable;

use crate::sql_reflection_config::{SqlDriver, SqlReflectionConfig};
use crate::sql_reflection_schema::SchemaCatalog;
use crate::types::Type;

/// One live-or-cached connection's own answer to "what row type does
/// this SQL string's own result set have?" - `None` when it can't be
/// answered (a real SQL/connection error, or a statement with no
/// result-set columns at all, e.g. `INSERT`/`UPDATE`/`DELETE`).
/// Implementations must never execute `sql`, only prepare it.
pub trait QueryReflector: Send + Sync {
    fn reflect(&self, sql: &str) -> Option<Type>;
}

pub struct MysqlReflector {
    pool: mysql::Pool,
}

impl MysqlReflector {
    fn connect(config: &SqlReflectionConfig) -> Result<Self, String> {
        let password = config.resolved_password()?;
        let opts = mysql::OptsBuilder::new()
            .ip_or_hostname(config.host.clone())
            .tcp_port(config.port.unwrap_or(3306))
            .user(config.user.clone())
            .pass(password)
            .db_name(config.database.clone());
        let pool =
            mysql::Pool::new(opts).map_err(|e| format!("failed to connect to MySQL: {e}"))?;
        Ok(Self { pool })
    }
}

impl QueryReflector for MysqlReflector {
    fn reflect(&self, sql: &str) -> Option<Type> {
        self.reflect_with_schema(sql, None)
    }
}

impl MysqlReflector {
    /// Same as `reflect`, but consults `schema` (A2's static catalog,
    /// when configured) for an `ENUM` column's own real literal-value
    /// union - kept as a separate entry point from the trait method so
    /// unit tests can exercise the enum cross-reference directly
    /// without needing a full `QueryReflector`-consuming call site.
    fn reflect_with_schema(&self, sql: &str, schema: Option<&SchemaCatalog>) -> Option<Type> {
        let mut conn = self.pool.get_conn().ok()?;
        let stmt = conn.prep(sql).ok()?;
        let columns = stmt.columns();
        if columns.is_empty() {
            return None;
        }
        let fields = columns
            .iter()
            .map(|col| {
                let name = col.name_str().into_owned();
                let ty = mysql_column_to_type(col, schema);
                (name, ty, false)
            })
            .collect();
        Some(Type::ArrayShape(fields))
    }
}

fn mysql_column_to_type(col: &mysql::Column, schema: Option<&SchemaCatalog>) -> Type {
    let nullable = !col.flags().contains(ColumnFlags::NOT_NULL_FLAG);
    let base = match col.column_type() {
        ColumnType::MYSQL_TYPE_ENUM => enum_literal_union(col, schema),
        ColumnType::MYSQL_TYPE_JSON => Type::String,
        ColumnType::MYSQL_TYPE_DATE
        | ColumnType::MYSQL_TYPE_TIME
        | ColumnType::MYSQL_TYPE_TIME2
        | ColumnType::MYSQL_TYPE_DATETIME
        | ColumnType::MYSQL_TYPE_DATETIME2
        | ColumnType::MYSQL_TYPE_TIMESTAMP
        | ColumnType::MYSQL_TYPE_TIMESTAMP2
        | ColumnType::MYSQL_TYPE_YEAR
        | ColumnType::MYSQL_TYPE_NEWDATE => Type::String,
        // mysqlnd returns DECIMAL/NEWDECIMAL as a string unconditionally
        // - confirmed live, see `sql_reflection_schema`'s own doc
        // comment for the full verification note. FLOAT/DOUBLE instead
        // switch on `PDO::ATTR_STRINGIFY_FETCHES`, which this reflector
        // has no way to see - honestly undecided, not guessed.
        ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => Type::String,
        ColumnType::MYSQL_TYPE_FLOAT | ColumnType::MYSQL_TYPE_DOUBLE => {
            Type::Union(vec![Type::Float, Type::String])
        }
        // `TINYINT(1)` is deliberately still `Int`, not `Bool` -
        // confirmed live against real PDO in both
        // `PDO::ATTR_EMULATE_PREPARES` settings (see
        // `sql_reflection_schema`'s own doc comment) - there is no
        // driver ground truth for a `Bool` mapping at all, for any
        // integer width.
        ColumnType::MYSQL_TYPE_TINY
        | ColumnType::MYSQL_TYPE_SHORT
        | ColumnType::MYSQL_TYPE_INT24
        | ColumnType::MYSQL_TYPE_LONG
        | ColumnType::MYSQL_TYPE_LONGLONG => Type::Int,
        ColumnType::MYSQL_TYPE_VARCHAR
        | ColumnType::MYSQL_TYPE_VAR_STRING
        | ColumnType::MYSQL_TYPE_STRING
        | ColumnType::MYSQL_TYPE_BIT
        | ColumnType::MYSQL_TYPE_TINY_BLOB
        | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
        | ColumnType::MYSQL_TYPE_LONG_BLOB
        | ColumnType::MYSQL_TYPE_BLOB
        | ColumnType::MYSQL_TYPE_SET
        | ColumnType::MYSQL_TYPE_GEOMETRY => Type::String,
        ColumnType::MYSQL_TYPE_NULL => Type::Null,
        // A wire-protocol type this table has no first-class
        // translation for - genuinely undecidable, matching this
        // project's own "honest, not invented" precedent (see
        // `sql_reflection_schema`'s own doc comment).
        _ => Type::Mixed,
    };
    if nullable && base != Type::Null {
        Type::Union(vec![base, Type::Null])
    } else {
        base
    }
}

/// Cross-references `schema` (A2's own static catalog) for this `ENUM`
/// column's real declared literal values, via the column's own
/// `org_table`/`org_name` (the *original* table/column, which survives
/// an alias - unlike `table`/`name`, which reflect the aliased query
/// text) - falls back to `Type::String` when no schema is configured or
/// the table/column isn't found in it (a real, honest degrade, not a
/// guess at the permitted values).
fn enum_literal_union(col: &mysql::Column, schema: Option<&SchemaCatalog>) -> Type {
    let table = col.org_table_str();
    let name = col.org_name_str();
    match schema.and_then(|s| s.column(&table, &name)) {
        Some(column) => column.ty.clone(),
        None => Type::String,
    }
}

type BoxedReflector = Box<dyn QueryReflector>;

/// The `Registry`-held, lazily-constructed live reflector - a plain
/// `OnceLock`, not a `Mutex<Connection>`: `mysql::Pool` is itself
/// internally `Sync` and designed for concurrent access from every
/// `rayon` check-phase worker at once (`check_project.rs`'s own
/// `par_iter()` calls), so a `Mutex` here would silently serialize
/// every reflection call across every worker thread. Deliberately does
/// *not* validate reachability eagerly at startup, unlike a configured
/// `schema_file` (`sql_reflection_schema`'s own eager, fail-fast
/// validation) - a temporarily-down DB should never block a run that
/// might not even need it, and (once wired into a real call site) a
/// failed `reflect()` degrades that one query to today's `Mixed`
/// fallback, not a hard process exit - see the plan's own "circuit
/// breaker, not an unbounded wait" framing.
#[derive(Default)]
pub struct SqlReflectorHandle(OnceLock<Option<BoxedReflector>>);

impl std::fmt::Debug for SqlReflectorHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SqlReflectorHandle")
    }
}

impl SqlReflectorHandle {
    pub fn get(&self, config: Option<&SqlReflectionConfig>) -> Option<&dyn QueryReflector> {
        self.0
            .get_or_init(|| build_reflector(config))
            .as_deref()
            .map(|r| r as &dyn QueryReflector)
    }
}

/// A live connection is only attempted when the config actually names a
/// target database - a `schema_file`-only block (A2, no live intent at
/// all) must never trigger a connection attempt to some default
/// `127.0.0.1:3306` with no credentials.
fn build_reflector(config: Option<&SqlReflectionConfig>) -> Option<BoxedReflector> {
    let config = config?;
    config.database.as_ref()?;
    match config.driver {
        SqlDriver::Mysql => MysqlReflector::connect(config)
            .ok()
            .map(|r| Box::new(r) as BoxedReflector),
        // Postgres/SQLite live reflectors are separate follow-on
        // commits.
        SqlDriver::Pgsql | SqlDriver::Sqlite => None,
    }
}

#[cfg(test)]
mod tests;
