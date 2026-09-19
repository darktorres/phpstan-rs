//! A project's own optional `[sql_reflection]` config block - the
//! connection/schema-source surface for the live SQL query reflection
//! feature (`checker::sql_reflection`): result-set type inference for raw
//! `Doctrine\DBAL\Connection` calls, the same real-world shape
//! `staabm/phpstan-dba` uses (prepare, don't execute, and read the
//! prepared statement's own result-set column metadata).
//!
//! Config-file-only (no CLI flag) - a structured, driver-specific block
//! doesn't fit a boolean/string CLI flag, the same reasoning
//! `restricted_symbols`/`doctrine_custom_functions` already established.
//! Absent block: no connection is ever attempted and no schema is
//! loaded, so every raw-SQL call site keeps today's
//! `array<string,mixed>|false` fallback, byte-for-byte unchanged.
//!
//! ```toml
//! [sql_reflection]
//! driver = "mysql" # or "pgsql" / "sqlite"
//! host = "127.0.0.1"
//! port = 3306
//! user = "..."
//! password_env = "PIWIGO_DB_PASSWORD" # preferred over a literal `password`
//! database = "..."
//! # sqlite: `path` instead of host/port/user/password/database
//! # either engine: `schema_file` names a static DDL dump to parse instead
//! # of (or alongside, for a cross-check) a live connection.
//! ```

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum SqlDriver {
    Mysql,
    Pgsql,
    Sqlite,
}

/// One `[sql_reflection]` block. Every field beyond `driver` is optional
/// at the type level - a `Deserialize`-only struct can't enforce "`path`
/// is required for `Sqlite`, `host`/`user`/`database` for the other two",
/// that validation belongs to whatever actually attempts a connection,
/// not this struct - and a literal `password` and `schema_file` can both
/// be present alongside live-connection fields (the schema-drift
/// cross-check case).
#[derive(Clone, Deserialize, PartialEq)]
pub struct SqlReflectionConfig {
    pub driver: SqlDriver,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    /// A literal password, for quick local setup. `password_env` is the
    /// preferred form for anything committed - see `resolved_password`.
    pub password: Option<String>,
    /// The name of an environment variable to read the real password
    /// from at connect time, so a real credential never has to live in a
    /// typically-committed `phpstan-rs.toml`. Takes precedence over a
    /// literal `password` when both are somehow set.
    pub password_env: Option<String>,
    pub database: Option<String>,
    /// SQLite only: the database file path, used instead of
    /// `host`/`port`/`user`/`password`/`database`.
    pub path: Option<PathBuf>,
    /// A static DDL dump (`install/piwigo_structure-mysql.sql`-shaped) to
    /// parse into a schema catalog, used instead of (or alongside, for a
    /// cross-check) a live connection.
    pub schema_file: Option<PathBuf>,
}

impl SqlReflectionConfig {
    /// Resolves the real password to connect with: `password_env` (read
    /// from the environment) takes precedence over a literal `password`
    /// when both are set; `Err` only when `password_env` is set but the
    /// named variable isn't - a fail-fast config error, not a silent
    /// fall-through to the literal field or to no password at all, since
    /// a user who configured `password_env` clearly intended it to be the
    /// real source.
    pub fn resolved_password(&self) -> Result<Option<String>, String> {
        match &self.password_env {
            Some(var) => std::env::var(var)
                .map(Some)
                .map_err(|_| format!("sql_reflection.password_env {var:?} is not set")),
            None => Ok(self.password.clone()),
        }
    }
}

// A literal `password` must never print in plaintext via `{:?}` -
// `Config` (`config.rs`) derives a bare `#[derive(Debug)]` with no
// redaction anywhere, so this struct can't inherit that derive the way
// every other config-entry type does.
impl std::fmt::Debug for SqlReflectionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlReflectionConfig")
            .field("driver", &self.driver)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("password_env", &self.password_env)
            .field("database", &self.database)
            .field("path", &self.path)
            .field("schema_file", &self.schema_file)
            .finish()
    }
}

#[cfg(test)]
mod tests;
