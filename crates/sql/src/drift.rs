//! Schema drift check (the plan's own follow-on section): when both a
//! `schema_file` and a live-connection config are present, compare each
//! table the static catalog knows about against the same table's own
//! live-reflected column list, once at startup, and warn on a real
//! mismatch - catches a committed DDL dump that's silently gone stale
//! against the actual running database, rather than one source quietly
//! masking the other's staleness for the whole run.
//!
//! Reuses the existing `QueryReflector::reflect()` interface rather
//! than writing new per-engine `information_schema`/`PRAGMA` queries by
//! hand: `SELECT * FROM <table>` already gives back exactly the
//! column list every other live call site gets, via
//! the same `PREPARE`-only, never-executed mechanism.

use crate::column::{Column, ColumnType};
use crate::config::SqlDriver;
use crate::reflector::QueryReflector;
use crate::schema::SchemaCatalog;

/// One human-readable warning per table whose static and live column
/// lists disagree. A table the live connection can't reach at all
/// (dropped, renamed, or a transient connection issue) is silently
/// skipped here - resolution steps 3/4 already report that honestly
/// wherever it actually matters for a real call site, so this check
/// only concerns itself with a table both sources genuinely have their
/// own opinion about.
pub fn schema_drift_warnings(
    schema: &SchemaCatalog,
    reflector: &dyn QueryReflector,
    driver: SqlDriver,
) -> Vec<String> {
    let mut warnings = Vec::new();
    for table in schema.table_names() {
        let Some(static_columns) = schema.table(table) else {
            continue;
        };
        let Some(live_columns) = live_columns_for_table(reflector, table, driver) else {
            continue;
        };
        if !columns_are_compatible(static_columns, &live_columns) {
            warnings.push(format!(
                "sql_reflection: the static schema_file and the live database disagree on the column list for table `{table}` - the committed schema_file may be stale"
            ));
        }
    }
    warnings
}

/// Real structural equality, sorted by column name (declaration order
/// can legitimately differ between a DDL dump and a live `ALTER
/// TABLE`-mutated table without that being real drift) - with one
/// deliberate leniency: the live path's own `reflect()` never cross-
/// references a configured static catalog for an `ENUM` column's real
/// literal-value union (that cross-reference only happens through
/// `reflect_with_schema`, not exposed on the `QueryReflector` trait
/// this check consults), so a plain string on the live side against
/// an enum's labels on the static side is treated as
/// compatible rather than a false "drift".
fn columns_are_compatible(static_columns: &[Column], live_columns: &[Column]) -> bool {
    if static_columns.len() != live_columns.len() {
        return false;
    }
    let mut a: Vec<&Column> = static_columns.iter().collect();
    let mut b: Vec<&Column> = live_columns.iter().collect();
    a.sort_by(|x, y| x.name.cmp(&y.name));
    b.sort_by(|x, y| x.name.cmp(&y.name));
    a.iter().zip(b.iter()).all(|(x, y)| {
        x.name == y.name && x.nullable == y.nullable && types_are_compatible(&x.ty, &y.ty)
    })
}

fn types_are_compatible(a: &ColumnType, b: &ColumnType) -> bool {
    a == b
        || matches!(
            (a, b),
            (ColumnType::String, ColumnType::Enum(_)) | (ColumnType::Enum(_), ColumnType::String)
        )
}

fn live_columns_for_table(
    reflector: &dyn QueryReflector,
    table: &str,
    driver: SqlDriver,
) -> Option<Vec<Column>> {
    let quote = match driver {
        SqlDriver::Mysql => '`',
        SqlDriver::Pgsql | SqlDriver::Sqlite => '"',
    };
    reflector.reflect(&format!("SELECT * FROM {quote}{table}{quote}"))
}

#[cfg(test)]
mod tests;
