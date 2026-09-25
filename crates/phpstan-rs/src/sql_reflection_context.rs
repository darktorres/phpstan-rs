//! The SQL reflection service: a project's `[sql_reflection]` config, its
//! static schema catalog, and the live reflector built from the config on
//! first use, answering what columns a query's result set has.

use std::sync::Mutex;

use rustc_hash::FxHashMap;

use crate::sql_reflection_column::Column;
use crate::sql_reflection_config::{SqlDriver, SqlReflectionConfig};
use crate::sql_reflection_reflector::SqlReflectorHandle;
use crate::sql_reflection_schema::SchemaCatalog;

#[derive(Debug, Default)]
pub struct SqlContext {
    config: Option<SqlReflectionConfig>,
    schema: Option<SchemaCatalog>,
    reflector: SqlReflectorHandle,
    // The live reflector's answer by the literal SQL text alone: one
    // schema source and one connection serve the whole run, so the text
    // is the whole key. Saves a repeated live `PREPARE` round trip for a
    // query written at several call sites.
    reflected: Mutex<FxHashMap<String, Option<Vec<Column>>>>,
}

impl SqlContext {
    pub fn new(config: Option<SqlReflectionConfig>, schema: Option<SchemaCatalog>) -> SqlContext {
        SqlContext {
            config,
            schema,
            ..SqlContext::default()
        }
    }

    pub fn config(&self) -> Option<&SqlReflectionConfig> {
        self.config.as_ref()
    }

    /// The configured driver, when a `[sql_reflection]` block is.
    pub fn driver(&self) -> Option<SqlDriver> {
        self.config.as_ref().map(|c| c.driver)
    }

    pub fn schema(&self) -> Option<&SchemaCatalog> {
        self.schema.as_ref()
    }

    /// Replaces the config, dropping the reflector built from the old one.
    pub fn set_config(&mut self, config: Option<SqlReflectionConfig>) {
        *self = SqlContext::new(config, self.schema.take());
    }

    pub fn set_schema(&mut self, schema: Option<SchemaCatalog>) {
        self.schema = schema;
    }

    /// The columns of `sql`'s result set as the live database reports
    /// them - `None` when no live connection is configured or the
    /// statement doesn't prepare into a result set.
    pub fn reflect(&self, sql: &str) -> Option<Vec<Column>> {
        if let Some(cached) = self.reflected.lock().unwrap().get(sql) {
            return cached.clone();
        }
        let columns = self
            .reflector
            .get(self.config.as_ref())
            .and_then(|r| r.reflect(sql));
        self.reflected
            .lock()
            .unwrap()
            .insert(sql.to_string(), columns.clone());
        columns
    }
}

#[cfg(test)]
mod tests;
