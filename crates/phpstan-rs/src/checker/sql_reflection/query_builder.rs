//! Folds a `Doctrine\DBAL\Query\QueryBuilder` fluent chain
//! (root: `$connection->createQueryBuilder()`) into one literal SQL
//! string the live reflector can `PREPARE` against. Same backward-
//! walk architecture as `checker::doctrine::query_builder`
//! (`resolve_query_builder_chain`), reusing its own `Locals::
//! query_builder_aliases` map (`looks_like_query_builder_chain`'s own
//! check is purely syntactic - "ends in a call named
//! `createQueryBuilder`" - so it already covers this class's chains too,
//! no separate recording path needed) - but the end product is
//! different: this class has no ORM/entity metadata at all, so the goal
//! is a syntactically valid SQL string, not a semantically faithful
//! replica of what real `QueryBuilder::getSQL()` would produce.
//! Clause order/text shape read directly from `vendor/doctrine/dbal/
//! src/Query/QueryBuilder.php`'s own `getSQLForSelect`/`getFromClauses`/
//! `getSQLForInsert`/`getSQLForUpdate`/`getSQLForDelete` as ground
//! truth, not guessed.
//!
//! `where`/`andWhere`/`orWhere`/`having`/`andHaving`/`orHaving` genuinely
//! need folding here, unlike the DQL chain-folder (which
//! drops them entirely - they never change a DQL query's own result
//! *shape*): this class's SQL text needs every clause to be a real,
//! syntactically valid `PREPARE` target, and dropping a `WHERE`/`HAVING`
//! clause could leave dangling SQL (e.g. an alias only introduced by a
//! join condition). The exact boolean-combinator structure real DBAL's
//! own `CompositeExpression` would produce isn't reproduced precisely
//! (`andWhere`/`orWhere` are both folded by joining with their own
//! keyword) - only syntactic validity matters here, never logical
//! correctness, since this text is never executed, only prepared.

use crate::checker::{Checker, Locals};
use syntax::ast::literal_string_value;
use syntax::ast::{Arg, Expr, ExprKind, StaticTarget};
use types::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JoinKind {
    Inner,
    Left,
    Right,
}

impl JoinKind {
    fn keyword(self) -> &'static str {
        match self {
            JoinKind::Inner => "JOIN",
            JoinKind::Left => "LEFT JOIN",
            JoinKind::Right => "RIGHT JOIN",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FromClause {
    table: String,
    alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JoinClause {
    kind: JoinKind,
    table: String,
    alias: String,
    condition: Option<String>,
}

/// Real DBAL: `setParameter(int|string $key, ...)` - a positional key or
/// a bare name (no leading `:`, matching `SELECT ... WHERE x = :name`'s
/// own `where()`-string convention - `setParameter('name', ...)` binds
/// `:name`, not `:'name'` or `::name`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ParamKey {
    Positional(i64),
    Named(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Dml {
    Insert {
        table: String,
        values: Vec<(String, String)>,
    },
    Update {
        table: String,
        set: Vec<(String, String)>,
    },
    Delete {
        table: String,
    },
}

/// `None` fields/empty vecs mean "never set" - a `SELECT` chain with no
/// `select()`/`from()` call at all is real, valid-to-detect-but-
/// undecidable SQL (mirrors real `QueryBuilder::getSQLForSelect()`'s own
/// `throw` on an empty `select`), not this module's own failure to look
/// hard enough.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DbalQueryBuilderChain {
    distinct: bool,
    select: Vec<String>,
    from: Vec<FromClause>,
    joins: Vec<JoinClause>,
    where_parts: Vec<(bool, String)>, // (is_or, predicate) - first entry's own bool is irrelevant
    group_by: Vec<String>,
    having_parts: Vec<(bool, String)>,
    order_by: Vec<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    dml: Option<Dml>,
    /// Every `setParameter`/`setParameters` binding collected along the
    /// chain, in call order - `setParameters` replaces the whole list
    /// (matching real DBAL: `$this->params = $params;`, not a merge),
    /// `setParameter` appends/overwrites one key. Parameter validation's own
    /// concern (`checker::sql_reflection::parameter_validation`), not
    /// consulted by `render()` at all - a placeholder's own bound value
    /// never changes the SQL *text*.
    pub(crate) params: Vec<ParamBinding>,
}

/// One collected `setParameter`/`setParameters` binding.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParamBinding {
    pub(crate) key: ParamKey,
    pub(crate) value_ty: Type,
    /// Real DBAL: `setParameter($key, $arrayValue, ArrayParameterType::
    /// INTEGER)` is the standard, common way to bind a real PHP array
    /// as a single placeholder (DBAL expands it into a real `IN (...)`
    /// list internally, e.g. `WHERE id IN (:ids)`) - a genuinely
    /// different, valid shape from every other placeholder, not a
    /// parameter-type mistake. Confirmed real and dominant, not
    /// hypothetical: 53+ real Piwigo `setParameter('ids', $ids,
    /// ArrayParameterType::INTEGER)` call sites alone.
    pub(crate) expects_array: bool,
    /// Real DBAL: `setParameter($key, $value, ParameterType::INTEGER)`
    /// (or the equivalent `Doctrine\DBAL\Types\Types::INTEGER`/
    /// `SMALLINT`/`BIGINT` type-name constant) - a connection-independent
    /// source of parameter-type truth, real and present in this exact
    /// corpus (`PermissionRepository.php`'s own `setParameter('level',
    /// ..., ParameterType::INTEGER)`). Deliberately not extended to
    /// `STRING`/`BOOLEAN`/etc - PHP/SQL's own scalar coercion makes
    /// almost every value validly bindable as those, the same zero-
    /// coercion-risk bar `parameter_validation.rs`'s own module doc
    /// already sets; `INTEGER` is the one family with a real, always-
    /// wrong value shape to check for (a literal, non-numeric string).
    pub(crate) expects_integer: bool,
}

impl DbalQueryBuilderChain {
    /// Renders the folded chain into one literal SQL string, mirroring
    /// real `QueryBuilder::getSQL()`'s own dispatch on statement kind -
    /// `None` only for a `SELECT`-shaped chain with no `select`/`from`
    /// at all (an empty chain, e.g. a bare `createQueryBuilder()` never
    /// built on further - real DBAL throws at this exact point too).
    pub(crate) fn render(&self) -> Option<String> {
        if let Some(dml) = &self.dml {
            return Some(match dml {
                Dml::Insert { table, values } => {
                    let columns = values.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>();
                    let placeholders = values.iter().map(|(_, v)| v.as_str()).collect::<Vec<_>>();
                    format!(
                        "INSERT INTO {table} ({}) VALUES({})",
                        columns.join(", "),
                        placeholders.join(", ")
                    )
                }
                Dml::Update { table, set } => {
                    let assignments = set
                        .iter()
                        .map(|(k, v)| format!("{k} = {v}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let mut sql = format!("UPDATE {table} SET {assignments}");
                    if let Some(where_sql) = self.where_sql() {
                        sql.push_str(" WHERE ");
                        sql.push_str(&where_sql);
                    }
                    sql
                }
                Dml::Delete { table } => {
                    let mut sql = format!("DELETE FROM {table}");
                    if let Some(where_sql) = self.where_sql() {
                        sql.push_str(" WHERE ");
                        sql.push_str(&where_sql);
                    }
                    sql
                }
            });
        }

        if self.select.is_empty() || self.from.is_empty() {
            return None;
        }
        let mut sql = String::from("SELECT ");
        if self.distinct {
            sql.push_str("DISTINCT ");
        }
        sql.push_str(&self.select.join(", "));
        sql.push_str(" FROM ");
        let from_clauses: Vec<String> = self
            .from
            .iter()
            .map(|f| match &f.alias {
                Some(alias) if alias != &f.table => format!("{} {alias}", f.table),
                _ => f.table.clone(),
            })
            .collect();
        sql.push_str(&from_clauses.join(", "));
        // Real DBAL associates each join with a specific `fromAlias`
        // (`getSQLForJoins`) - simplified here to "every join follows
        // every from clause, in call order," syntactically valid SQL for
        // the overwhelmingly common single-`from()` shape and harmless
        // even for a multi-`from()` query (the joined table still ends
        // up in the right `FROM` list, just not annotated onto one
        // specific from-alias's own text).
        for join in &self.joins {
            sql.push(' ');
            sql.push_str(join.kind.keyword());
            sql.push(' ');
            sql.push_str(&join.table);
            sql.push(' ');
            sql.push_str(&join.alias);
            if let Some(condition) = &join.condition {
                sql.push_str(" ON ");
                sql.push_str(condition);
            }
        }
        if let Some(where_sql) = self.where_sql() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
        }
        if !self.group_by.is_empty() {
            sql.push_str(" GROUP BY ");
            sql.push_str(&self.group_by.join(", "));
        }
        if let Some(having_sql) = combine_predicate_parts(&self.having_parts) {
            sql.push_str(" HAVING ");
            sql.push_str(&having_sql);
        }
        if !self.order_by.is_empty() {
            sql.push_str(" ORDER BY ");
            sql.push_str(&self.order_by.join(", "));
        }
        if let Some(limit) = self.limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }
        if let Some(offset) = self.offset {
            sql.push_str(&format!(" OFFSET {offset}"));
        }
        Some(sql)
    }

    fn where_sql(&self) -> Option<String> {
        combine_predicate_parts(&self.where_parts)
    }
}

fn combine_predicate_parts(parts: &[(bool, String)]) -> Option<String> {
    let mut iter = parts.iter();
    let (_, first) = iter.next()?;
    let mut out = first.clone();
    for (is_or, part) in iter {
        out.push_str(if *is_or { " OR " } else { " AND " });
        out.push_str(part);
    }
    Some(out)
}

fn looks_like_dbal_query_builder_chain(e: &Expr) -> bool {
    let e = e.peeled();
    syntax::stack::guard(|| match &e.kind {
        ExprKind::MethodCall(_, method, _, _) if **method == *"createQueryBuilder" => true,
        ExprKind::MethodCall(inner, _, _, _) => looks_like_dbal_query_builder_chain(inner),
        _ => false,
    })
}

struct ChainCall {
    method: String,
    args: Vec<Arg>,
}

enum Cursor<'a> {
    Borrowed(&'a Expr),
    Owned(std::rc::Rc<Expr>),
}

impl std::ops::Deref for Cursor<'_> {
    type Target = Expr;

    fn deref(&self) -> &Expr {
        match self {
            Cursor::Borrowed(e) => e,
            Cursor::Owned(e) => e,
        }
    }
}

impl<'a> Checker<'a> {
    /// A `fetch*`/`executeQuery`/`executeStatement` call's own receiver
    /// expression - resolves its fluent `Doctrine\DBAL\Query\
    /// QueryBuilder` chain into one literal SQL string, `None` when any
    /// part of it is undecidable (a dynamic argument, an unrecognized
    /// chain root, or a `SELECT`-shaped chain missing `select`/`from`
    /// entirely).
    pub(crate) fn resolve_dbal_query_builder_sql(
        &self,
        recv: &Expr,
        locals: &Locals,
    ) -> Option<String> {
        self.resolve_dbal_query_builder_chain(recv, locals)?
            .render()
    }

    /// Same backward-walk as `resolve_dbal_query_builder_sql`, but
    /// returns the whole resolved chain (including every collected
    /// `setParameter`/`setParameters` binding) instead of just its own
    /// rendered SQL text - parameter validation's own entry point
    /// (`checker::sql_reflection::parameter_validation`), which needs
    /// both the text (to find real placeholders) and the bindings (to
    /// cross-reference against them).
    pub(crate) fn resolve_dbal_query_builder_chain(
        &self,
        recv: &Expr,
        locals: &Locals,
    ) -> Option<DbalQueryBuilderChain> {
        if !looks_like_dbal_query_builder_chain(recv) {
            return None;
        }
        let mut calls: Vec<ChainCall> = Vec::new();
        let mut cursor = Cursor::Borrowed(recv);
        loop {
            match &cursor.peeled().kind {
                ExprKind::MethodCall(_, method, _, _) if **method == *"createQueryBuilder" => break,
                ExprKind::MethodCall(inner, method, args, _) => {
                    calls.push(ChainCall {
                        method: method.to_string(),
                        args: args.clone(),
                    });
                    cursor = Cursor::Owned(std::rc::Rc::new((**inner).clone()));
                }
                ExprKind::Variable(name) => {
                    let rhs = locals.query_builder_aliases().get(name.as_ref())?;
                    cursor = Cursor::Owned(rhs.clone());
                }
                _ => return None,
            }
        }
        calls.reverse();

        let mut chain = DbalQueryBuilderChain::default();
        for call in &calls {
            self.apply_dbal_chain_call(&mut chain, &call.method, &call.args, locals)?;
        }
        Some(chain)
    }

    /// A directly-passed SQL argument (`fetchAssociative($sql, ...)`) -
    /// the "already a plain string" shortcut, matching the DQL side's own
    /// 168g precedent for `createQuery($dql)`: a literal or constant-
    /// foldable string needs no chain-walking at all.
    pub(crate) fn foldable_sql_string(&self, expr: &Expr, locals: &Locals) -> Option<String> {
        fold_string_expr(self, expr, locals)
    }

    fn apply_dbal_chain_call(
        &self,
        chain: &mut DbalQueryBuilderChain,
        method: &str,
        args: &[Arg],
        locals: &Locals,
    ) -> Option<()> {
        match method {
            "select" => chain.select = self.foldable_sql_arg_strings(args, locals)?,
            "addSelect" => chain
                .select
                .extend(self.foldable_sql_arg_strings(args, locals)?),
            "distinct" => chain.distinct = true,
            "from" => {
                let table = args
                    .first()
                    .and_then(|a| self.foldable_sql_arg_string(a, locals))?;
                let alias = args
                    .get(1)
                    .and_then(|a| self.foldable_sql_arg_string(a, locals));
                chain.from.push(FromClause { table, alias });
            }
            "join" | "innerJoin" | "leftJoin" | "rightJoin" => {
                let kind = match method {
                    "leftJoin" => JoinKind::Left,
                    "rightJoin" => JoinKind::Right,
                    _ => JoinKind::Inner,
                };
                // args[0] is `fromAlias`, not needed for this module's
                // own simplified single-FROM-list rendering (see
                // `DbalQueryBuilderChain::render`'s own doc comment).
                let table = args
                    .get(1)
                    .and_then(|a| self.foldable_sql_arg_string(a, locals))?;
                let alias = args
                    .get(2)
                    .and_then(|a| self.foldable_sql_arg_string(a, locals))?;
                let condition = args
                    .get(3)
                    .and_then(|a| self.foldable_sql_arg_string(a, locals));
                chain.joins.push(JoinClause {
                    kind,
                    table,
                    alias,
                    condition,
                });
            }
            "where" => {
                chain.where_parts = combine_variadic_predicates(self, args, locals, false)?;
            }
            "andWhere" => {
                chain
                    .where_parts
                    .extend(combine_variadic_predicates(self, args, locals, false)?);
            }
            "orWhere" => {
                chain
                    .where_parts
                    .extend(combine_variadic_predicates(self, args, locals, true)?);
            }
            "having" => {
                chain.having_parts = combine_variadic_predicates(self, args, locals, false)?;
            }
            "andHaving" => {
                chain
                    .having_parts
                    .extend(combine_variadic_predicates(self, args, locals, false)?);
            }
            "orHaving" => {
                chain
                    .having_parts
                    .extend(combine_variadic_predicates(self, args, locals, true)?);
            }
            "groupBy" => chain.group_by = self.foldable_sql_arg_strings(args, locals)?,
            "addGroupBy" => chain
                .group_by
                .extend(self.foldable_sql_arg_strings(args, locals)?),
            "orderBy" => {
                let sort = args
                    .first()
                    .and_then(|a| self.foldable_sql_arg_string(a, locals))?;
                let order = args
                    .get(1)
                    .and_then(|a| self.foldable_sql_arg_string(a, locals));
                chain.order_by = vec![order_by_entry(sort, order)];
            }
            "addOrderBy" => {
                let sort = args
                    .first()
                    .and_then(|a| self.foldable_sql_arg_string(a, locals))?;
                let order = args
                    .get(1)
                    .and_then(|a| self.foldable_sql_arg_string(a, locals));
                chain.order_by.push(order_by_entry(sort, order));
            }
            "setMaxResults" => {
                chain.limit = args.first().and_then(|a| self.foldable_arg_int(a, locals));
            }
            "setFirstResult" => {
                chain.offset = args.first().and_then(|a| self.foldable_arg_int(a, locals));
            }
            "insert" => {
                let table = args
                    .first()
                    .and_then(|a| self.foldable_sql_arg_string(a, locals))?;
                chain.dml = Some(Dml::Insert {
                    table,
                    values: Vec::new(),
                });
            }
            "update" => {
                let table = args
                    .first()
                    .and_then(|a| self.foldable_sql_arg_string(a, locals))?;
                chain.dml = Some(Dml::Update {
                    table,
                    set: Vec::new(),
                });
            }
            "delete" => {
                let table = args
                    .first()
                    .and_then(|a| self.foldable_sql_arg_string(a, locals))?;
                chain.dml = Some(Dml::Delete { table });
            }
            "set" | "setValue" => {
                let key = args
                    .first()
                    .and_then(|a| self.foldable_sql_arg_string(a, locals))?;
                let value = args
                    .get(1)
                    .and_then(|a| self.foldable_sql_arg_string(a, locals))?;
                match &mut chain.dml {
                    Some(Dml::Update { set, .. }) => set.push((key, value)),
                    Some(Dml::Insert { values, .. }) => values.push((key, value)),
                    _ => return None,
                }
            }
            "values" => {
                let ExprKind::ArrayLiteral(items) = &args.first()?.value.kind else {
                    return None;
                };
                let mut values = Vec::new();
                for item in items {
                    let key = item
                        .key
                        .as_ref()
                        .and_then(|k| self.foldable_arg_string_expr(k, locals))?;
                    let value = self.foldable_arg_string_expr(&item.value, locals)?;
                    values.push((key, value));
                }
                match &mut chain.dml {
                    Some(Dml::Insert { values: dst, .. }) => *dst = values,
                    _ => return None,
                }
            }
            // Never change the SQL text itself (they bind a value to an
            // already-written `?`/`:name` placeholder) - collected for
            // parameter validation's own cross-reference against the resolved
            // SQL's real placeholders, not consulted by `render()`.
            "setParameter" => {
                let key = args
                    .first()
                    .and_then(|a| param_key_from_arg(self, a, locals))?;
                let value_ty = args
                    .get(1)
                    .map(|a| self.infer_expr(&a.value, locals))
                    .unwrap_or(Type::mixed());
                let expects_array = args
                    .get(2)
                    .is_some_and(|a| is_array_parameter_type(self, &a.value));
                let expects_integer = args
                    .get(2)
                    .is_some_and(|a| is_integer_parameter_type(self, &a.value));
                chain.params.retain(|p| p.key != key);
                chain.params.push(ParamBinding {
                    key,
                    value_ty,
                    expects_array,
                    expects_integer,
                });
            }
            "setParameters" => {
                let ExprKind::ArrayLiteral(items) = &args.first()?.value.kind else {
                    return None;
                };
                // The parallel `$types` array (real signature:
                // `setParameters(array $params, array $types = [])`) is
                // keyed the same way as `$params` - only consulted for
                // the `ArrayParameterType::*`/`ParameterType::INTEGER`
                // exemptions, same as `setParameter`'s own third argument.
                let types_by_key: Vec<(ParamKey, bool, bool)> =
                    match args.get(1).map(|a| &a.value.kind) {
                        Some(ExprKind::ArrayLiteral(type_items)) => type_items
                            .iter()
                            .enumerate()
                            .filter_map(|(position, item)| {
                                let key = match &item.key {
                                    Some(k) => param_key_from_expr(self, k, locals)?,
                                    None => ParamKey::Positional(position as i64),
                                };
                                Some((
                                    key,
                                    is_array_parameter_type(self, &item.value),
                                    is_integer_parameter_type(self, &item.value),
                                ))
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                let mut params = Vec::new();
                for (position, item) in items.iter().enumerate() {
                    let key = match &item.key {
                        Some(k) => param_key_from_expr(self, k, locals)?,
                        None => ParamKey::Positional(position as i64),
                    };
                    let value_ty = self.infer_expr(&item.value, locals);
                    let expects_array = types_by_key
                        .iter()
                        .any(|(k, expects_array, _)| *k == key && *expects_array);
                    let expects_integer = types_by_key
                        .iter()
                        .any(|(k, _, expects_integer)| *k == key && *expects_integer);
                    params.push(ParamBinding {
                        key,
                        value_ty,
                        expects_array,
                        expects_integer,
                    });
                }
                chain.params = params;
            }
            // Any other unrecognized method is simply ignored, matching
            // the DQL side's `apply_chain_call` precedent.
            _ => {}
        }
        Some(())
    }

    fn foldable_sql_arg_string(&self, arg: &Arg, locals: &Locals) -> Option<String> {
        self.foldable_arg_string_expr(&arg.value, locals)
    }

    fn foldable_arg_string_expr(&self, expr: &Expr, locals: &Locals) -> Option<String> {
        fold_string_expr(self, expr, locals)
    }

    fn foldable_sql_arg_strings(&self, args: &[Arg], locals: &Locals) -> Option<Vec<String>> {
        args.iter()
            .map(|a| self.foldable_sql_arg_string(a, locals))
            .collect()
    }

    fn foldable_arg_int(&self, arg: &Arg, locals: &Locals) -> Option<i64> {
        self.infer_expr(&arg.value, locals).int_literal_value()
    }
}

fn fold_string_expr(checker: &Checker, expr: &Expr, locals: &Locals) -> Option<String> {
    syntax::stack::guard(|| {
        if let Some(s) = literal_string_value(expr) {
            return Some(s.into_owned());
        }
        match &expr.kind {
            // A double-quoted/heredoc string with every interpolated part
            // itself foldable (a literal text chunk, or a variable narrowed
            // to a literal) - `check_expr`'s own `Interpolated` arm
            // deliberately widens this to plain `string` unconditionally
            // (it exists to run the real `encapsedStringPart.nonString`/
            // implicit-`__toString()` diagnostics, not to preserve a literal
            // type), so `infer_expr`'s separate literal-fold arm below is
            // never actually reachable for this case - folded here directly
            // instead, scoped to this module only, rather than changing
            // `check_expr`'s general narrowing behavior for every caller.
            ExprKind::Interpolated(parts) => {
                let mut out = String::new();
                for part in parts {
                    out.push_str(&fold_string_expr(checker, part, locals)?);
                }
                return Some(out);
            }
            ExprKind::Call(callee, args) => {
                if let ExprKind::Name(name) = &callee.kind
                    && name
                        .rsplit('\\')
                        .next_back()
                        .is_some_and(|n| n.eq_ignore_ascii_case("sprintf"))
                    && !args.iter().any(|a| a.spread)
                {
                    return fold_sprintf_call(checker, args, locals);
                }
            }
            _ => {}
        }
        checker
            .infer_expr(expr, locals)
            .string_literal_value()
            .map(|s| s.to_string())
    })
}

/// A real `sprintf()` call whose format string and every value argument
/// fold to a literal - scoped deliberately to the common, unambiguous
/// specifier shapes only (`%s`, `%d`, `%u`, `%%`, no flags/width/
/// precision/positional `$` form): those need real PHP's own padding/
/// sign/base-conversion rules faithfully reproduced to fold correctly,
/// which this module has no need to build just to resolve a SQL string -
/// bails to `None` (falls through to the live/undecided path) rather
/// than risk folding a subtly wrong SQL string. Mirrors this module's
/// own repeated "only a confidently-understood shape is folded, anything
/// else falls through rather than guesses" precedent (see `resolve.rs`'s
/// static-schema JOIN/aggregate bail-outs).
fn fold_sprintf_call(checker: &Checker, args: &[Arg], locals: &Locals) -> Option<String> {
    let fmt = fold_string_expr(checker, &args.first()?.value, locals)?;
    let mut values = args[1..].iter();
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            '%' => out.push('%'),
            's' => out.push_str(&fold_sprintf_string_arg(checker, values.next()?, locals)?),
            'd' => {
                let n = fold_sprintf_int_arg(checker, values.next()?, locals)?;
                out.push_str(&n.to_string());
            }
            'u' => {
                let n = fold_sprintf_int_arg(checker, values.next()?, locals)?;
                if n < 0 {
                    return None;
                }
                out.push_str(&n.to_string());
            }
            _ => return None,
        }
    }
    // Real `sprintf()` throws `ValueError` on a count mismatch - folding
    // a call that would itself fail at runtime is unsafe/pointless, so
    // bail rather than fold a text that could never actually be produced.
    if values.next().is_some() {
        return None;
    }
    Some(out)
}

fn fold_sprintf_string_arg(checker: &Checker, arg: &Arg, locals: &Locals) -> Option<String> {
    if let Some(s) = fold_string_expr(checker, &arg.value, locals) {
        return Some(s);
    }
    checker
        .infer_expr(&arg.value, locals)
        .int_literal_value()
        .map(|n| n.to_string())
}

fn fold_sprintf_int_arg(checker: &Checker, arg: &Arg, locals: &Locals) -> Option<i64> {
    checker.infer_expr(&arg.value, locals).int_literal_value()
}

/// `ArrayParameterType::INTEGER`/`STRING`/`ASCII`/`BINARY` (real DBAL
/// enum, `vendor/doctrine/dbal/src/ArrayParameterType.php`) - matched by
/// resolved class name only (any member/case), not the specific case,
/// since every case means the same thing for this check's own purposes
/// ("this placeholder expects an array, expand it into an `IN (...)`
/// list").
pub(crate) fn is_array_parameter_type(checker: &Checker, expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::ClassConstFetch(StaticTarget::Name(class_name), _member) => checker
            .resolve_class_name(class_name)
            .ends_with("ArrayParameterType"),
        _ => false,
    }
}

/// `Doctrine\DBAL\ParameterType::INTEGER` or the equivalent `Doctrine\
/// DBAL\Types\Types::INTEGER`/`SMALLINT`/`BIGINT` type-name constant -
/// real DBAL's own two distinct places an integer parameter type is
/// declared (`ParameterType` binds a driver-level PDO type; `Types` is
/// Doctrine's own richer type-mapping system, but both are real,
/// confirmed-present ways this corpus declares "this placeholder is an
/// integer"). Matched by resolved class name *and* member, unlike
/// `is_array_parameter_type` - every `ArrayParameterType` case means the
/// same thing for that check, but `ParameterType`/`Types` have several
/// other cases (`STRING`/`BOOLEAN`/...) this project deliberately
/// doesn't treat as integer.
pub(crate) fn is_integer_parameter_type(checker: &Checker, expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::ClassConstFetch(StaticTarget::Name(class_name), member) => {
            let resolved = checker.resolve_class_name(class_name);
            let short_name = resolved.rsplit('\\').next().unwrap_or(&resolved);
            match short_name {
                "ParameterType" => &**member == "INTEGER",
                "Types" => matches!(&**member, "INTEGER" | "SMALLINT" | "BIGINT"),
                _ => false,
            }
        }
        _ => false,
    }
}

/// A literal PHP value that can never validly represent a real DBAL
/// `Types::INTEGER`/`ParameterType::INTEGER`-declared placeholder -
/// scoped deliberately narrow (a non-numeric string literal only, real
/// PHP's own `is_numeric()` grammar) to stay inside this whole feature's
/// zero-coercion-risk bar: a numeric *string* (`'42'`) is completely
/// normal, safe, everyday DBAL usage for an integer column, not a
/// mismatch.
pub(crate) fn value_is_confidently_not_an_integer(value_ty: &Type) -> bool {
    value_ty
        .string_literal_value()
        .is_some_and(|s| !s.as_str().is_some_and(is_php_numeric_string))
}

fn is_php_numeric_string(s: &str) -> bool {
    s.trim().parse::<f64>().is_ok()
}

fn param_key_from_arg(checker: &Checker, arg: &Arg, locals: &Locals) -> Option<ParamKey> {
    param_key_from_expr(checker, &arg.value, locals)
}

/// Real `setParameter(int|string $key, ...)` - a literal/foldable
/// string becomes a named key, a literal/foldable int becomes a
/// positional one. An undecidable key (a dynamic variable) makes the
/// whole chain undecidable for parameter-validation purposes, the same
/// "don't guess" leniency every other chain call already has.
pub(crate) fn param_key_from_expr(
    checker: &Checker,
    expr: &Expr,
    locals: &Locals,
) -> Option<ParamKey> {
    if let Some(s) = fold_string_expr(checker, expr, locals) {
        return Some(ParamKey::Named(s));
    }
    checker
        .infer_expr(expr, locals)
        .int_literal_value()
        .map(ParamKey::Positional)
}

/// `where(...$predicates)`/`having(...$predicates)` take one-or-more
/// variadic predicate strings, real DBAL `AND`-combining them together
/// into one `CompositeExpression` even within a single call
/// (`where('a = 1', 'b = 2')` means `a = 1 AND b = 2`) before that
/// composite itself joins the chain's own running predicate the way
/// `is_or` says.
fn combine_variadic_predicates(
    checker: &Checker,
    args: &[Arg],
    locals: &Locals,
    is_or: bool,
) -> Option<Vec<(bool, String)>> {
    let mut parts = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        let s = fold_string_expr(checker, &arg.value, locals)?;
        parts.push((if i == 0 { is_or } else { false }, s));
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts)
}

fn order_by_entry(sort: String, order: Option<String>) -> String {
    match order {
        Some(order) => format!("{sort} {order}"),
        None => sort,
    }
}

#[cfg(test)]
mod tests;
