//! Live SQL query reflection for raw `Doctrine\DBAL\Connection` result
//! types - result-set type inference for `fetch*`/`executeQuery`/
//! `executeStatement`, the same real-world shape `staabm/phpstan-dba`
//! uses (prepare, don't execute, and read the prepared statement's own
//! result-set column metadata). See
//! `delegated-tumbling-dongarra.md`'s sub-phases B/C/D/E for the full
//! design. Only per-call-site, `Checker`/AST-consulting logic lives
//! under this module (this file's own `query_builder`) - both A2's
//! static schema catalog and C's live reflector are compiled/lazily-
//! built state stored directly on `Registry`, so they live top-level
//! instead, in `sql::schema`/`crate::
//! sql_reflection_reflector` (see either module's own doc comment for
//! why: `Registry` has zero existing references to `checker::` types,
//! and this project's `restricted_symbols`/`doctrine_functions` already
//! established that shape for exactly this kind of Registry-held data).

pub(crate) mod parameter_validation;
pub(crate) mod query_builder;
pub(crate) mod resolve;

use sql::column::{Column, ColumnType};
use types::{Shape, Type};

/// The PHP type of the value a raw fetch of a column returns.
pub(crate) fn column_php_type(ty: &ColumnType) -> Type {
    match ty {
        ColumnType::Int => Type::int(),
        ColumnType::Float => Type::float(),
        ColumnType::Bool => Type::bool(),
        ColumnType::String => Type::string(),
        ColumnType::Number => Type::union_of(vec![Type::int(), Type::float()]),
        ColumnType::Enum(labels) => Type::union_of(
            labels
                .iter()
                .map(|label| Type::string_literal(label.as_str().into()))
                .collect(),
        ),
        ColumnType::Null => Type::null(),
        ColumnType::Unknown => Type::mixed(),
    }
}

/// What fetching `column` returns: its type, or `null` too when the
/// column is nullable.
pub(crate) fn column_value_type(column: &Column) -> Type {
    let ty = column_php_type(&column.ty);
    if column.nullable {
        Type::union(ty, Type::null())
    } else {
        ty
    }
}

/// The row a result set with these columns fetches as.
pub(crate) fn row_type(columns: &[Column]) -> Type {
    Type::array_shape(Shape::from_named(
        columns
            .iter()
            .map(|c| (c.name.as_str(), column_value_type(c), false)),
    ))
}

#[cfg(test)]
mod tests;
