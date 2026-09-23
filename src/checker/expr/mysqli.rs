//! SQL-aware `mysqli` result typing - `mysqli::query()`/`mysqli_query()`
//! resolve their SQL through the same reflection DBAL calls use, and the
//! resulting `mysqli_result<Row>` (a synthesized generic slot, like
//! `Doctrine\DBAL\Result`'s) types every `fetch_*` method's row.

use crate::ast::*;
use crate::checker::expr::doctrine_query_result_list_type_of;
use crate::checker::type_union::union_types;
use crate::checker::{Checker, Locals};
use crate::types::{Type, parse_type_string};

/// The statements that produce a result set - anything else (`INSERT`,
/// `UPDATE`, `SET`, ...) makes `query()` return `true`.
fn returns_result_set(sql: &str) -> bool {
    let head = sql.trim_start().trim_start_matches('(').trim_start();
    let word: String = head
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_uppercase();
    matches!(
        word.as_str(),
        "SELECT" | "SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN" | "WITH" | "TABLE" | "VALUES"
    )
}

/// `query()` runs over the text protocol, which returns every number as
/// a string unless `MYSQLI_OPT_INT_AND_FLOAT_NATIVE` is set - either form
/// is possible without knowing the connection's options.
fn widen_for_text_protocol(ty: &Type) -> Type {
    match ty {
        Type::Int
        | Type::IntLiteral(_)
        | Type::IntRange(..)
        | Type::Float
        | Type::FloatLiteral(_) => union_types(ty.clone(), parse_type_string("numeric-string")),
        Type::Bool | Type::True | Type::False => {
            union_types(parse_type_string("0|1"), parse_type_string("'0'|'1'"))
        }
        Type::Union(members) => members
            .iter()
            .map(widen_for_text_protocol)
            .reduce(union_types)
            .unwrap_or(Type::Never),
        other => other.clone(),
    }
}

impl<'a> Checker<'a> {
    /// Since PHP 8.1 mysqli's default report mode throws
    /// `mysqli_sql_exception` instead of returning `false`.
    fn mysqli_throws_exceptions(&self) -> bool {
        self.registry
            .php_version
            .is_none_or(|version| version >= (8, 1))
    }

    /// `mysqli::query($sql)`/`mysqli_query($link, $sql)`.
    pub(crate) fn mysqli_query_type(&self, sql_arg: Option<&Expr>, locals: &Locals) -> Type {
        let sql = sql_arg.and_then(|e| self.foldable_sql_string(e, locals));
        let success = match &sql {
            Some(sql) if !returns_result_set(sql) => Type::True,
            Some(sql) => {
                let row = crate::checker::sql_reflection::resolve::resolve_sql_row_type(
                    self.registry,
                    sql,
                );
                match row {
                    Some(Type::ArrayShape(fields)) => {
                        let widened = fields
                            .into_iter()
                            .map(|(k, ty, opt)| (k, widen_for_text_protocol(&ty), opt))
                            .collect();
                        Type::Object("mysqli_result".to_string(), vec![Type::ArrayShape(widened)])
                    }
                    _ => Type::Object("mysqli_result".to_string(), Vec::new()),
                }
            }
            None => Type::Union(vec![
                Type::Object("mysqli_result".to_string(), Vec::new()),
                Type::True,
            ]),
        };
        if self.mysqli_throws_exceptions() {
            success
        } else {
            union_types(success, Type::False)
        }
    }

    /// A `mysqli_result<Row>` fetch method's return type (method names
    /// as on the class; the procedural `mysqli_fetch_*` forms map onto
    /// them). `arg` is `fetch_all()`/`fetch_array()`'s mode or
    /// `fetch_column()`'s column index. `None` when the row isn't known
    /// or `arg` isn't a literal/constant this can resolve.
    pub(crate) fn mysqli_fetch_type(
        &self,
        method: &str,
        row: Option<&Type>,
        arg: Option<&Expr>,
    ) -> Option<Type> {
        let Some(Type::ArrayShape(fields)) = row else {
            return None;
        };
        let assoc = || Type::ArrayShape(fields.clone());
        let numeric = || {
            Type::ArrayShape(
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, (_, ty, opt))| (i.to_string(), ty.clone(), *opt))
                    .collect(),
            )
        };
        let both = || {
            let mut merged = Vec::new();
            for (i, (key, ty, opt)) in fields.iter().enumerate() {
                merged.push((i.to_string(), ty.clone(), *opt));
                merged.push((key.clone(), ty.clone(), *opt));
            }
            Type::ArrayShape(merged)
        };
        // `false` only reports a failure the exception mode throws for.
        let end = if self.mysqli_throws_exceptions() {
            Type::Null
        } else {
            Type::Union(vec![Type::Null, Type::False])
        };
        let mode = |default: &str| -> Option<String> {
            match arg {
                None => Some(default.to_string()),
                Some(e) => match &e.kind {
                    ExprKind::Name(name) => Some(name.trim_start_matches('\\').to_string()),
                    ExprKind::Literal(Literal::Int(1)) => Some("MYSQLI_ASSOC".to_string()),
                    ExprKind::Literal(Literal::Int(2)) => Some("MYSQLI_NUM".to_string()),
                    ExprKind::Literal(Literal::Int(3)) => Some("MYSQLI_BOTH".to_string()),
                    _ => None,
                },
            }
        };
        let by_mode = |mode: &str| match mode {
            "MYSQLI_ASSOC" => Some(assoc()),
            "MYSQLI_NUM" => Some(numeric()),
            "MYSQLI_BOTH" => Some(both()),
            _ => None,
        };
        Some(match method {
            "fetch_assoc" => union_types(assoc(), end),
            "fetch_row" => union_types(numeric(), end),
            "fetch_array" => union_types(by_mode(&mode("MYSQLI_BOTH")?)?, end),
            "fetch_all" => doctrine_query_result_list_type_of(by_mode(&mode("MYSQLI_NUM")?)?),
            "fetch_column" => {
                let index = match arg.map(|e| &e.kind) {
                    None => 0,
                    Some(ExprKind::Literal(Literal::Int(n))) if *n >= 0 => *n as usize,
                    _ => return None,
                };
                let (_, ty, _) = fields.get(index)?;
                // No more rows is `false` here, not `null`.
                union_types(ty.clone(), Type::False)
            }
            _ => return None,
        })
    }
}
