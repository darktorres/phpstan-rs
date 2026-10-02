//! SQL result-type resolution - the single entry point the call sites
//! consult. Implements all 4 steps of the
//! plan's own resolution order: 1 (`resolve_pure_ast` - a select list of
//! only literals/a bare `COUNT`, needing no schema/connection at all),
//! 2 (`Registry::sql_reflection_cache` - a resolved-or-undecidable
//! answer by SQL text, avoiding a repeated live `PREPARE` for the same
//! query text across multiple call sites in one run), 3 (the static
//! schema catalog, A2), and 4 (the live/cached reflector,
//! `sql_reflection_reflector`).
//!
//! The static path is scoped to the dominant real shape: a single-
//! table (or derived-table/non-recursive-CTE) `SELECT`, no `JOIN`, a
//! `*`/bare-or-qualified-column/aliased-column projection. A join or a
//! computed/aggregate select item falls through to the live path
//! instead of guessing.
//!
//! **Virtual relations** (the plan's own section by that name - views,
//! CTEs, derived tables, `UNION`): live-verified across all 3 engines
//! that the *live* reflector (step 4) needs no dedicated resolution
//! logic for any of these at all - `reflect()` hands the whole query
//! text to the real engine's own `PREPARE`, which already resolves
//! every one of these as part of its own real query planning
//! (`sql_reflection_reflector`'s own tests cover this directly). The
//! *static* path here handles derived tables and non-recursive CTEs
//! (`resolve_ctes_from_static_schema`/`TableFactor::Derived`) through
//! the same column-list primitive a real table's own schema entry
//! already uses - but deliberately not `UNION`'s own per-engine
//! numeric/string coercion rules, `CREATE VIEW` parsing (the static catalog's DDL
//! parser only extracts `CREATE TABLE`), or `WITH RECURSIVE` (which
//! compounds both): each needs real per-engine behavior this project
//! only ever trusts once verified live, which a schema-file-only
//! configuration (the only case where this gap could matter at all -
//! any configured live connection already answers all of it) has no
//! way to verify against. A real, disclosed gap, not a decline.

use rustc_hash::FxHashMap;

use sqlparser::ast::{
    Expr, ObjectNamePart, Select, SelectItem, SetExpr, Statement, TableFactor, UnaryOperator, Value,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

use crate::registry::Registry;
use sql::config::SqlDriver;
use sql::context::SqlContext;
use sql::schema::SchemaCatalog;

use types::{Shape, ShapeField, Type};

/// The row type `sql` returns, its columns refined by the matching
/// Doctrine entity's fields when `entities` (the registry, while the
/// Doctrine extension is active) has one.
pub(crate) fn resolve_sql_row_type(
    entities: Option<&Registry>,
    context: &SqlContext,
    sql: &str,
) -> Option<Type> {
    let refine = |ty: Type| match entities {
        Some(registry) => cross_reference_entity_fields(registry, sql, ty),
        None => ty,
    };
    if let Some(ty) = resolve_pure_ast(sql, context.driver()) {
        return Some(refine(refine_count_columns(sql, ty)));
    }
    let row_type = match context
        .schema()
        .and_then(|schema| resolve_from_static_schema(schema, sql))
    {
        Some(ty) => Some(ty),
        None => context
            .reflect(sql)
            .map(|columns| super::row_type(&columns)),
    };
    row_type.map(|ty| refine_count_columns(sql, ty)).map(refine)
}

/// Gap #9: a raw SQL query against a table a `DoctrineEntity` already
/// maps should prefer that entity's own already-resolved PHP property
/// type over a freshly-derived DB-column-type translation, for any
/// column that's also a mapped scalar field - the entity's own mapped
/// type can be strictly more precise (a narrower non-null guarantee the
/// app enforces above the DB's own nullability, a value-object wrapper,
/// ...). Only ever *narrows*: `column_ty` must be a super type of
/// `entity_ty`, so a genuine disagreement (the entity's own type is wider, or
/// simply incompatible) keeps the column-derived answer rather than
/// silently trusting possibly-stale entity metadata over a live/static
/// schema fact. Scoped to the same single-table, no-`JOIN` shape every
/// other step in this module already restricts itself to.
fn cross_reference_entity_fields(registry: &Registry, sql: &str, row_type: Type) -> Type {
    let Some(fields) = row_type.as_array_shape() else {
        return row_type;
    };
    let Some(table) = single_table_name(sql) else {
        return row_type;
    };
    let Some(entity) = registry.doctrine_entity_for_table(&table) else {
        return row_type;
    };
    let refined = fields
        .fields()
        .iter()
        .map(|f| {
            let name = f.key.to_string();
            let property = entity
                .column_names
                .iter()
                .find(|(_, column)| column.as_ref() == name)
                .map(|(property, _)| property);
            let entity_ty = property.and_then(|p| entity.fields.get(p));
            match entity_ty {
                Some(entity_ty)
                    if types::Relation::structural()
                        .is_super_type_of(&f.ty, entity_ty)
                        .yes() =>
                {
                    ShapeField::new(f.key.clone(), entity_ty.clone(), f.optional)
                }
                _ => f.clone(),
            }
        })
        .collect();
    Type::array_shape(Shape::from_parts(
        refined,
        fields.unsealed.clone(),
        fields.next_index,
    ))
}

/// The exact `single-table, no JOIN` scope `resolve_select_from_static_
/// schema` already restricts itself to, extracted here as its own
/// standalone parse since this needs the table name independent of
/// *which* resolution step actually answered the query (pure-AST,
/// static, or live all funnel through here identically).
fn single_table_name(sql: &str) -> Option<String> {
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
    if select.from.len() != 1 || !select.from[0].joins.is_empty() {
        return None;
    }
    let TableFactor::Table { name, .. } = &select.from[0].relation else {
        return None;
    };
    name.0.last().and_then(|part| match part {
        ObjectNamePart::Identifier(ident) => Some(ident.value.clone()),
        ObjectNamePart::Function(_) => None,
    })
}

/// Resolution-order step 1: a select list of only literals and/or a
/// bare `COUNT(...)` call needs no schema/connection at all - real,
/// live-confirmed against all 3 engines (a throwaway probe against real
/// MySQL/Postgres/SQLite, not assumed): an integer literal, a string
/// literal, and `NULL` all report the identical driver-native type
/// across every engine, so those three are answered here unconditionally.
/// A decimal-literal (`1.5`) or boolean-literal (`TRUE`) select item
/// *genuinely diverges* by engine (MySQL/Postgres report a bare decimal
/// literal as `NEWDECIMAL`/`numeric` - this project's own already-
/// established "decimal is string" rule - while SQLite's own literal
/// storage class is unambiguously `REAL`/`Float`; Postgres has a native
/// `bool` while MySQL/SQLite report a bare `TRUE`/`FALSE` literal as
/// plain `int`), so those two are only answered when a driver is
/// actually configured to disambiguate - with no driver configured at
/// all, bailing (falling through to the next step, which will also find
/// nothing to connect to) is safer than guessing one engine's answer for
/// an unknown target.
fn resolve_pure_ast(sql: &str, driver: Option<SqlDriver>) -> Option<Type> {
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
    let mut fields = Vec::new();
    for item in &select.projection {
        let (expr, alias) = match item {
            SelectItem::UnnamedExpr(e) => (e, None),
            SelectItem::ExprWithAlias { expr, alias } => (expr, Some(alias.value.clone())),
            _ => return None,
        };
        let (default_name, ty) = pure_ast_expr(expr, driver)?;
        fields.push((alias.unwrap_or(default_name), ty, false));
    }
    if fields.is_empty() {
        return None;
    }
    Some(Type::array_shape(Shape::from_named(fields)))
}

/// Resolves both the unaliased column name real driver metadata reports
/// *and* the type together, structurally - deliberately not by slicing
/// the original SQL text at `Expr::span()`, which turned out to be
/// unreliable for exactly the shapes this needs (confirmed reading
/// `sqlparser`'s own `Spanned` impls): `Expr::UnaryOp`'s span is only
/// ever its *operand's* span, silently dropping the operator token
/// (`-1` would slice back out as just `"1"`), and `COUNT(*)`'s own
/// `Function` span drops the `(*)` entirely (`FunctionArgExpr::Wildcard`
/// carries no span of its own), leaving just `"COUNT"` - the same
/// "spans aren't a substitute for knowing the real shape" lesson this
/// module already learned once for `normalize_placeholders_for_driver`.
fn pure_ast_expr(expr: &Expr, driver: Option<SqlDriver>) -> Option<(String, Type)> {
    syntax::stack::guard(|| {
        match expr {
            Expr::Value(v) => Some((
                literal_name(&v.value)?,
                literal_value_type(&v.value, driver)?,
            )),
            Expr::UnaryOp {
                op: UnaryOperator::Minus,
                expr: inner,
            } => {
                let (inner_name, ty) = pure_ast_expr(inner, driver)?;
                Some((format!("-{inner_name}"), ty))
            }
            Expr::UnaryOp {
                op: UnaryOperator::Plus,
                expr: inner,
            } => pure_ast_expr(inner, driver),
            Expr::Nested(inner) => pure_ast_expr(inner, driver),
            // `COUNT(...)`'s own base type is unanimous across every engine
            // (a plain integer count) regardless of driver - `resolve_sql_
            // row_type`'s own `refine_count_columns` post-processing refines
            // this further to `int<0, max>` afterward, the same as any other
            // resolution step's raw answer.
            Expr::Function(f) => {
                let is_count = f.name.0.last().is_some_and(|part| {
                    matches!(part, ObjectNamePart::Identifier(ident) if ident.value.eq_ignore_ascii_case("COUNT"))
                });
                if !is_count {
                    return None;
                }
                let arg_name = count_argument_name(f, driver)?;
                Some((format!("COUNT({arg_name})"), Type::int()))
            }
            _ => None,
        }
    })
}

/// `COUNT(*)` or `COUNT(<a literal this module already knows how to
/// name>)` only - anything else (a column reference, `DISTINCT`, a
/// second argument, ...) isn't a "needs no schema at all" shape, so
/// bails rather than guess.
fn count_argument_name(f: &sqlparser::ast::Function, driver: Option<SqlDriver>) -> Option<String> {
    let sqlparser::ast::FunctionArguments::List(list) = &f.args else {
        return None;
    };
    let [arg] = list.args.as_slice() else {
        return None;
    };
    match arg {
        sqlparser::ast::FunctionArg::Unnamed(sqlparser::ast::FunctionArgExpr::Wildcard) => {
            Some("*".to_string())
        }
        sqlparser::ast::FunctionArg::Unnamed(sqlparser::ast::FunctionArgExpr::Expr(e)) => {
            Some(pure_ast_expr(e, driver)?.0)
        }
        _ => None,
    }
}

/// The unaliased column name real driver metadata reports for a bare
/// literal select item - confirmed live against real MySQL (`SELECT 1`
/// -> `"1"`, `SELECT 'x'` -> `"x"` (the string's own *content*, not its
/// quoted source form), `SELECT true` -> `"true"`, `SELECT NULL` ->
/// `"NULL"`). `Value::Null`/`Value::Boolean` carry no original-casing
/// text of their own to reproduce exactly (a real, disclosed, cosmetic-
/// only precision gap for an already-corpus-confirmed-hypothetical query
/// shape) - a fixed canonical casing is used instead of guessing.
fn literal_name(value: &Value) -> Option<String> {
    match value {
        Value::Number(text, _) => Some(text.clone()),
        Value::SingleQuotedString(s) | Value::DoubleQuotedString(s) => Some(s.clone()),
        Value::Boolean(b) => Some(if *b { "true" } else { "false" }.to_string()),
        Value::Null => Some("NULL".to_string()),
        _ => None,
    }
}

fn literal_value_type(value: &Value, driver: Option<SqlDriver>) -> Option<Type> {
    match value {
        Value::Number(text, _) => {
            if text.contains(['.', 'e', 'E']) {
                match driver {
                    Some(SqlDriver::Mysql | SqlDriver::Pgsql) => Some(Type::string()),
                    Some(SqlDriver::Sqlite) => Some(Type::float()),
                    None => None,
                }
            } else {
                Some(Type::int())
            }
        }
        Value::SingleQuotedString(_) | Value::DoubleQuotedString(_) => Some(Type::string()),
        Value::Boolean(_) => match driver {
            Some(SqlDriver::Pgsql) => Some(Type::bool()),
            Some(SqlDriver::Mysql | SqlDriver::Sqlite) => Some(Type::int()),
            None => None,
        },
        Value::Null => Some(Type::null()),
        _ => None,
    }
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
    let Some(mut fields) = row_type.as_array_shape().cloned() else {
        return row_type;
    };
    if let Some(positions) = count_column_positions(sql) {
        for i in positions {
            if let Some(ty) = fields.field_type_at_mut(i) {
                *ty = Type::int_range(Some(0), None);
            }
        }
    }
    Type::array_shape(fields)
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
    let ctes = resolve_ctes_from_static_schema(schema, &query)?;
    let SetExpr::Select(select) = *query.body else {
        return None;
    };
    let fields = resolve_select_from_static_schema(schema, &select, &ctes)?;
    Some(Type::array_shape(Shape::from_named(fields)))
}

/// A non-recursive `WITH cte AS (<query>) ...`'s own defining queries,
/// resolved up front (in declaration order, so a later CTE referencing
/// an earlier one already has it available) into the same column-list
/// shape a real table's own schema entry has - from that point on, a
/// `FROM cte` reference is indistinguishable from a real table
/// reference to `resolve_select_from_static_schema`. `WITH RECURSIVE`
/// bails rather than guess: real per-engine anchor/recursive-term
/// coercion rules (confirmed live for the *live* reflector path -
/// `sql_reflection_reflector`'s own tests - to need real per-engine
/// verification, not a guessed rule) aren't something a schema-file-
/// only configuration (no live connection to verify against) can
/// responsibly compute at all.
fn resolve_ctes_from_static_schema(
    schema: &SchemaCatalog,
    query: &sqlparser::ast::Query,
) -> Option<FxHashMap<String, Vec<TypedColumn>>> {
    let mut ctes = FxHashMap::default();
    if let Some(with) = &query.with {
        if with.recursive {
            return None;
        }
        for cte in &with.cte_tables {
            let SetExpr::Select(inner) = &*cte.query.body else {
                return None;
            };
            let fields = resolve_select_from_static_schema(schema, inner, &ctes)?;
            let columns = fields
                .into_iter()
                .map(|(name, ty, _)| TypedColumn {
                    name,
                    ty,
                    nullable: false,
                })
                .collect();
            ctes.insert(cte.alias.name.value.clone(), columns);
        }
    }
    Some(ctes)
}

/// A table's, derived table's or CTE's column, typed. A derived table's
/// or CTE's columns carry their nullability in `ty` already.
#[derive(Clone)]
struct TypedColumn {
    name: String,
    ty: Type,
    nullable: bool,
}

impl TypedColumn {
    fn from_column(column: &sql::column::Column) -> TypedColumn {
        TypedColumn {
            name: column.name.clone(),
            ty: super::column_php_type(&column.ty),
            nullable: column.nullable,
        }
    }
}

/// What fetching `col` returns - its type, or `null` too when the column
/// is nullable.
// Joins a few known types, far below the union cap.
#[allow(clippy::disallowed_methods)]
fn column_value_type(col: &TypedColumn) -> Type {
    if col.nullable {
        Type::union(col.ty.clone(), Type::null())
    } else {
        col.ty.clone()
    }
}

fn resolve_select_from_static_schema(
    schema: &SchemaCatalog,
    select: &Select,
    ctes: &FxHashMap<String, Vec<TypedColumn>>,
) -> Option<Vec<(String, Type, bool)>> {
    syntax::stack::guard(|| {
        if select.from.len() != 1 || !select.from[0].joins.is_empty() {
            return None;
        }
        let columns: Vec<TypedColumn> = match &select.from[0].relation {
            TableFactor::Table { name, .. } => {
                let table_name = name.0.last().and_then(|part| match part {
                    ObjectNamePart::Identifier(ident) => Some(ident.value.as_str()),
                    ObjectNamePart::Function(_) => None,
                })?;
                match ctes.get(table_name) {
                    Some(cte_columns) => cte_columns.clone(),
                    None => schema
                        .table(table_name)?
                        .iter()
                        .map(TypedColumn::from_column)
                        .collect(),
                }
            }
            // `SELECT ... FROM (<subquery>) AS t` - resolved via the exact
            // same primitive a real table's own column list already goes
            // through, so a derived table is indistinguishable from a real
            // one from this point on (mirroring the plan's own "one shared
            // mechanism, not one-off ones" framing for virtual relations).
            TableFactor::Derived { subquery, .. } => {
                let SetExpr::Select(inner) = &*subquery.body else {
                    return None;
                };
                let inner_ctes = resolve_ctes_from_static_schema(schema, subquery)?;
                let mut merged = ctes.clone();
                merged.extend(inner_ctes);
                resolve_select_from_static_schema(schema, inner, &merged)?
                    .into_iter()
                    .map(|(name, ty, _)| TypedColumn {
                        name,
                        ty,
                        nullable: false,
                    })
                    .collect()
            }
            _ => return None,
        };

        let mut fields = Vec::new();
        for item in &select.projection {
            match item {
                SelectItem::Wildcard(_) => {
                    for col in &columns {
                        fields.push((col.name.clone(), column_value_type(col), false));
                    }
                }
                SelectItem::UnnamedExpr(Expr::Identifier(ident)) => {
                    let col = columns.iter().find(|c| c.name == ident.value)?;
                    fields.push((ident.value.clone(), column_value_type(col), false));
                }
                SelectItem::UnnamedExpr(Expr::CompoundIdentifier(parts)) => {
                    let col_name = &parts.last()?.value;
                    let col = columns.iter().find(|c| &c.name == col_name)?;
                    fields.push((col_name.clone(), column_value_type(col), false));
                }
                SelectItem::ExprWithAlias {
                    expr: Expr::Identifier(ident),
                    alias,
                } => {
                    let col = columns.iter().find(|c| c.name == ident.value)?;
                    fields.push((alias.value.clone(), column_value_type(col), false));
                }
                SelectItem::ExprWithAlias {
                    expr: Expr::CompoundIdentifier(parts),
                    alias,
                } => {
                    let col_name = &parts.last()?.value;
                    let col = columns.iter().find(|c| &c.name == col_name)?;
                    fields.push((alias.value.clone(), column_value_type(col), false));
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
        Some(fields)
    })
}

#[cfg(test)]
mod tests;
