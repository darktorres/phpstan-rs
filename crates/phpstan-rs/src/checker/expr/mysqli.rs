//! SQL-aware `mysqli` result typing - `mysqli::query()`/`mysqli_query()`
//! resolve their SQL through the same reflection DBAL calls use, and the
//! resulting `mysqli_result<Row>` (a synthesized generic slot, like
//! `Doctrine\DBAL\Result`'s) types every `fetch_*` method's row.

use crate::checker::{Checker, Locals};
use syntax::ast::*;
use types::UnionAll as _;

use types::{Shape, ShapeField, ShapeKey, Type, parse_type_string};

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
// Joins a few known types, far below the union cap.
#[allow(clippy::disallowed_methods)]
fn widen_for_text_protocol(ty: &Type) -> Type {
    ty.union_members()
        .iter()
        .map(|m| {
            if m.is_int().yes() || m.is_float().yes() {
                Type::union(m.clone(), parse_type_string("numeric-string"))
            } else if m.is_bool().yes() {
                Type::union(parse_type_string("0|1"), parse_type_string("'0'|'1'"))
            } else {
                m.clone()
            }
        })
        .union_all()
        .unwrap_or(Type::never())
}

impl<'a> Checker<'a> {
    /// Since PHP 8.1 mysqli's default report mode throws
    /// `mysqli_sql_exception` instead of returning `false`.
    fn mysqli_throws_exceptions(&self) -> bool {
        self.registry
            .php_version_min()
            .is_none_or(|version| version >= types::PhpVersion::new(8, 1))
    }

    /// `mysqli::query($sql)`/`mysqli_query($link, $sql)`.
    pub(crate) fn mysqli_query_type(&self, sql_arg: Option<&ExprNode>, locals: &Locals) -> Type {
        let sql = sql_arg.and_then(|e| self.foldable_sql_string(e, locals));
        let success = match &sql {
            Some(sql) if !returns_result_set(sql) => Type::bool_literal(true),
            Some(sql) => {
                let row = crate::checker::sql_reflection::resolve::resolve_sql_row_type(
                    self.doctrine_entities(),
                    self.sql,
                    sql,
                );
                match row.as_ref().and_then(Type::as_array_shape) {
                    Some(shape) => {
                        let widened = shape.map_types(&mut widen_for_text_protocol);
                        Type::object("mysqli_result".into(), vec![Type::array_shape(widened)])
                    }
                    None => Type::object("mysqli_result".into(), Vec::new()),
                }
            }
            None => self.relation().union_of(vec![
                Type::object("mysqli_result".into(), Vec::new()),
                Type::bool_literal(true),
            ]),
        };
        if self.mysqli_throws_exceptions() {
            success
        } else {
            self.relation().union(success, Type::bool_literal(false))
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
        arg: Option<&ExprNode>,
    ) -> Option<Type> {
        let shape = row?.as_array_shape()?;
        let fields = shape.fields();
        let assoc = || Type::array_shape(shape.clone());
        let numeric = || {
            Type::array_shape(Shape::sealed(
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        ShapeField::new(ShapeKey::Int(i as i64), f.ty.clone(), f.optional)
                    })
                    .collect(),
            ))
        };
        let both = || {
            let mut merged = Vec::new();
            for (i, f) in fields.iter().enumerate() {
                merged.push(ShapeField::new(
                    ShapeKey::Int(i as i64),
                    f.ty.clone(),
                    f.optional,
                ));
                merged.push(f.clone());
            }
            Type::array_shape(Shape::sealed(merged))
        };
        // `false` only reports a failure the exception mode throws for.
        let end = if self.mysqli_throws_exceptions() {
            Type::null()
        } else {
            self.relation()
                .union_of(vec![Type::null(), Type::bool_literal(false)])
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
            "fetch_assoc" => self.relation().union(assoc(), end),
            "fetch_row" => self.relation().union(numeric(), end),
            "fetch_array" => self.relation().union(by_mode(&mode("MYSQLI_BOTH")?)?, end),
            "fetch_all" => Type::list_of(by_mode(&mode("MYSQLI_NUM")?)?),
            "fetch_column" => {
                let index = match arg.map(|e| &e.kind) {
                    None => 0,
                    Some(ExprKind::Literal(Literal::Int(n))) if *n >= 0 => *n as usize,
                    _ => return None,
                };
                let field = fields.get(index)?;
                // No more rows is `false` here, not `null`.
                self.relation()
                    .union(field.ty.clone(), Type::bool_literal(false))
            }
            _ => return None,
        })
    }
}
