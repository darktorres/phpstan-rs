//! Raw-SQL checks against a live database or a schema file.

use serde::Deserialize;

/// `[ext.sql]`: without `reflection`, no connection is attempted and
/// every raw-SQL call site keeps its fallback.
///
/// ```toml
/// [ext.sql.reflection]
/// driver = "mysql"
/// host = "127.0.0.1"
/// password_env = "PIWIGO_DB_PASSWORD"
/// database = "piwigo_test"
/// ```
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct SqlSettings {
    pub reflection: Option<sql::config::SqlReflectionConfig>,
}

impl config::Section for SqlSettings {
    const PATH: &'static [&'static str] = &["ext", "sql"];
}
