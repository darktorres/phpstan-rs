//! Sub-phase C's own resolution orchestration - the single entry point
//! sub-phase D's real call sites consult. Implements resolution-order
//! step 3 (the static schema catalog, A2) directly here, then falls
//! through to step 4 (the live/cached reflector, `sql_reflection_
//! reflector`). Steps 1 (a pure-AST answer needing no schema at all,
//! e.g. `SELECT 1`) and 2 (a resolved-answer cache) are not built yet -
//! every call re-parses and, when the static path can't answer, re-
//! prepares live - a real, disclosed performance gap, not a
//! correctness one, left for a later pass once real corpus usage shows
//! it matters.
//!
//! The static path is scoped to the dominant real shape only: a single-
//! table `SELECT`, no `JOIN`, a `*`/bare-or-qualified-column/aliased-
//! column projection. Anything else (a join, a computed/aggregate
//! select item, a subquery, a view/CTE) falls through to the live path
//! instead of guessing - full multi-table/virtual-relation resolution
//! is a disclosed, separate follow-on (the plan's own "virtual
//! relations" section).

use sqlparser::ast::{Expr, ObjectNamePart, Select, SelectItem, SetExpr, Statement, TableFactor};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

use crate::registry::Registry;
use crate::sql_reflection_schema::SchemaCatalog;
use crate::types::Type;

pub(crate) fn resolve_sql_row_type(registry: &Registry, sql: &str) -> Option<Type> {
    let row_type = if let Some(schema) = &registry.sql_reflection_schema
        && let Some(ty) = resolve_from_static_schema(schema, sql)
    {
        Some(ty)
    } else {
        registry
            .sql_reflector
            .get(registry.sql_reflection_config.as_ref())?
            .reflect(sql)
    }?;
    Some(refine_count_columns(sql, row_type))
}

/// `COUNT(...)` is always `int<0, max>` - real Doctrine's own
/// `QueryResultTypeWalker::walkAggregateExpression` gives the identical
/// answer unconditionally, confirmed reading it directly
/// (`checker::doctrine::result_shape::select_item_type`'s own `AggregateFn
/// ::Count` arm). Reused here rather than trusting the driver's own
/// raw reported column type (a real `BIGINT`/`INT` column type, honest
/// but strictly less precise than the range this project already knows
/// how to express, and some drivers report a computed column as
/// nullable at the wire-protocol level even though `COUNT` can never
/// actually return `NULL` - even a zero-row `COUNT(*)` returns `0`).
/// Deliberately does *not* attempt the same for `SUM`/`AVG`/`MIN`/`MAX`:
/// real Doctrine's own DQL-side answer for those four is only ever the
/// *operand's own PHP-side type*, an approximation this project's own
/// live/cached SQL reflection doesn't need - a real database's own
/// `SUM(tinyint_col)` genuinely can widen to a `BIGINT`, so the driver's
/// own reported type here is already more accurate than force-matching
/// 168d's simpler DQL heuristic would be.
fn refine_count_columns(sql: &str, row_type: Type) -> Type {
    let Type::ArrayShape(mut fields) = row_type else {
        return row_type;
    };
    if let Some(positions) = count_column_positions(sql) {
        for i in positions {
            if let Some(field) = fields.get_mut(i) {
                field.1 = Type::IntRange(Some(0), None);
            }
        }
    }
    Type::ArrayShape(fields)
}

fn count_column_positions(sql: &str) -> Option<Vec<usize>> {
    let stmt = Parser::parse_sql(&GenericDialect {}, sql)
        .ok()?
        .into_iter()
        .next()?;
    let Statement::Query(query) = stmt else {
        return None;
    };
    let SetExpr::Select(select) = *query.body else {
        return None;
    };
    Some(
        select
            .projection
            .iter()
            .enumerate()
            .filter_map(|(i, item)| {
                let expr = match item {
                    SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => e,
                    _ => return None,
                };
                is_count_call(expr).then_some(i)
            })
            .collect(),
    )
}

fn is_count_call(expr: &Expr) -> bool {
    match expr {
        Expr::Function(f) => f
            .name
            .0
            .last()
            .is_some_and(|part| matches!(part, ObjectNamePart::Identifier(ident) if ident.value.eq_ignore_ascii_case("COUNT"))),
        _ => false,
    }
}

fn resolve_from_static_schema(schema: &SchemaCatalog, sql: &str) -> Option<Type> {
    let stmt = Parser::parse_sql(&GenericDialect {}, sql)
        .ok()?
        .into_iter()
        .next()?;
    let Statement::Query(query) = stmt else {
        return None;
    };
    let SetExpr::Select(select) = *query.body else {
        return None;
    };
    resolve_select_from_static_schema(schema, &select)
}

fn resolve_select_from_static_schema(schema: &SchemaCatalog, select: &Select) -> Option<Type> {
    if select.from.len() != 1 || !select.from[0].joins.is_empty() {
        return None;
    }
    let TableFactor::Table { name, .. } = &select.from[0].relation else {
        return None;
    };
    let table_name = name.0.last().and_then(|part| match part {
        ObjectNamePart::Identifier(ident) => Some(ident.value.as_str()),
        ObjectNamePart::Function(_) => None,
    })?;
    let columns = schema.table(table_name)?;

    let mut fields = Vec::new();
    for item in &select.projection {
        match item {
            SelectItem::Wildcard(_) => {
                for col in columns {
                    fields.push((col.name.clone(), col.ty.clone(), false));
                }
            }
            SelectItem::UnnamedExpr(Expr::Identifier(ident)) => {
                let col = schema.column(table_name, &ident.value)?;
                fields.push((ident.value.clone(), col.ty.clone(), false));
            }
            SelectItem::UnnamedExpr(Expr::CompoundIdentifier(parts)) => {
                let col_name = &parts.last()?.value;
                let col = schema.column(table_name, col_name)?;
                fields.push((col_name.clone(), col.ty.clone(), false));
            }
            SelectItem::ExprWithAlias {
                expr: Expr::Identifier(ident),
                alias,
            } => {
                let col = schema.column(table_name, &ident.value)?;
                fields.push((alias.value.clone(), col.ty.clone(), false));
            }
            SelectItem::ExprWithAlias {
                expr: Expr::CompoundIdentifier(parts),
                alias,
            } => {
                let col_name = &parts.last()?.value;
                let col = schema.column(table_name, col_name)?;
                fields.push((alias.value.clone(), col.ty.clone(), false));
            }
            // A computed/aggregate select item, a qualified wildcard, or
            // anything else this simplified path doesn't model - can't
            // be answered from base column definitions alone, fall
            // through to the live path rather than guess.
            _ => return None,
        }
    }
    if fields.is_empty() {
        return None;
    }
    Some(Type::ArrayShape(fields))
}

#[cfg(test)]
mod tests;
