//! Placeholder/bound-parameter validation. Once a SQL
//! string is resolved (the `QueryBuilder` chain fold, or a literal/
//! foldable argument), every real placeholder it references (`?`
//! positional, `:name` named) is cross-referenced against the actual
//! bound values a real call site provides - `Doctrine\DBAL\Query\
//! QueryBuilder::setParameter`/`setParameters` for the fluent-chain
//! shape (the corpus-dominant real pattern - confirmed 542 real Piwigo
//! `setParameter('name', ...)` call sites), or `Connection`'s own
//! `$params`/`$types` array arguments for the literal-SQL-string shape.
//! Both mechanisms are built as real, general capabilities regardless
//! of which one happens to dominate any one corpus - this project
//! targets general Doctrine/DBAL usage, not just Piwigo's own.
//!
//! Three checks, all zero-coercion-risk (PHP/SQL's own loose scalar
//! type-juggling is real and this project doesn't try to out-guess it -
//! see `sql_reflection_reflector`'s own `STRINGIFY_FETCHES` research for
//! why that caution matters):
//! - `sqlReflection.missingParameter`: a placeholder the SQL text itself
//!   references has no bound value at all - unambiguous, always a real
//!   `SQLSTATE`-shaped runtime error, no coercion involved.
//! - `sqlReflection.invalidParameterType`: a bound value is a real PHP
//!   array, but the placeholder isn't declared with `ArrayParameterType::*`
//!   (real DBAL's own supported way to bind an array as one `IN (...)`
//!   list) - a plain SQL placeholder can never validly hold an array. The
//!   same identifier also covers a second, narrower source of truth: a
//!   placeholder explicitly declared `ParameterType::INTEGER`/`Types::
//!   INTEGER`/`SMALLINT`/`BIGINT` (real DBAL, a connection-independent
//!   fact already present in the call site's own PHP source - no live/
//!   static schema needed) bound to a literal, non-numeric string, which
//!   can never be a valid integer regardless of what any database says.
//!
//! Deliberately not built: comparing a bound value's own type against
//! the *prepared statement's own live-reported* expected type. SQL/PHP's
//! own scalar coercion is genuinely loose (a numeric string into an INT
//! column is completely normal, safe, everyday usage) - a real, risk-
//! based scope limit, not a corpus-popularity one. The `ParameterType`/
//! `Types::INTEGER`-style check above is different in kind, not degree:
//! it never consults a database at all, only the call site's own two
//! arguments contradicting each other.

use sqlparser::dialect::GenericDialect;
use sqlparser::tokenizer::{Token, Tokenizer};

use crate::checker::sql_reflection::query_builder::{
    ParamBinding, ParamKey, is_array_parameter_type, is_integer_parameter_type,
    param_key_from_expr, value_is_confidently_not_an_integer,
};
use crate::checker::{Checker, Locals};
use syntax::ast::{Arg, ExprKind, ExprNode, NodeArenas};
use syntax::span::Span;
use types::Type;

/// Every placeholder the SQL text itself references, in first-occurrence
/// order, deduplicated by key (a repeated `:name` is the same real bound
/// value referenced twice). `None` only on a real tokenize failure -
/// mirrors `sql_reflection_reflector::normalize_placeholders_for_driver`'s
/// own identical `:name`-detection mechanism (an adjacent `Colon` then
/// `Word`, since `sqlparser` has no single token for it), a separate,
/// purpose-built scan rather than a forced shared function - one rewrites
/// text, the other only collects keys.
fn sql_placeholder_keys(sql: &str) -> Option<Vec<ParamKey>> {
    let tokens = Tokenizer::new(&GenericDialect {}, sql).tokenize().ok()?;
    let mut keys: Vec<ParamKey> = Vec::new();
    let mut positional_index = 0i64;
    let mut i = 0;
    while i < tokens.len() {
        let is_named =
            matches!(tokens[i], Token::Colon) && matches!(tokens.get(i + 1), Some(Token::Word(_)));
        if is_named {
            let Token::Word(word) = &tokens[i + 1] else {
                unreachable!()
            };
            let key = ParamKey::Named(word.value.clone());
            if !keys.contains(&key) {
                keys.push(key);
            }
            i += 2;
            continue;
        }
        if let Token::Placeholder(text) = &tokens[i]
            && text == "?"
        {
            keys.push(ParamKey::Positional(positional_index));
            positional_index += 1;
        }
        i += 1;
    }
    Some(keys)
}

/// `Connection::fetchAssociative($sql, $params, $types)` etc.'s own
/// `$params`/`$types` array arguments - the same shape `setParameters`
/// already handles for the `QueryBuilder` side, extracted here
/// independently since there's no chain to fold through for this call
/// shape at all. `None` means "undecidable, skip the whole check" (a
/// real, common, valid shape: `$params` is a plain variable, e.g.
/// `executeStatement($sql, $newIds)` - confirmed a real false positive
/// this exact distinction fixes: treating that as "zero bindings"
/// instead of "unknown bindings" flagged every one of that call's real
/// placeholders as missing). A genuinely *absent* argument (no second
/// argument at all) is the real "zero bindings" case, kept distinct -
/// `Some(vec![])`, not `None`.
fn connection_params_from_args(
    checker: &Checker,
    arenas: &NodeArenas,
    args: &[Arg],
    locals: &Locals,
) -> Option<Vec<ParamBinding>> {
    let items = match args
        .get(1)
        .and_then(|a| arenas.expr(a.value))
        .map(|value| &value.kind)
    {
        None => return Some(Vec::new()),
        Some(ExprKind::ArrayLiteral(items)) => items,
        Some(_) => return None,
    };
    let types_by_key: Vec<(ParamKey, bool, bool)> = match args
        .get(2)
        .and_then(|a| arenas.expr(a.value))
        .map(|value| &value.kind)
    {
        Some(ExprKind::ArrayLiteral(type_items)) => arenas
            .array_items(type_items.clone())
            .iter()
            .enumerate()
            .filter_map(|(position, item)| {
                let key = match &item.key {
                    Some(k) => param_key_from_expr(checker, arenas, *k, locals)?,
                    None => ParamKey::Positional(position as i64),
                };
                let value = arenas.expr(item.value)?;
                Some((
                    key,
                    is_array_parameter_type(checker, value),
                    is_integer_parameter_type(checker, value),
                ))
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut params = Vec::new();
    for (position, item) in arenas.array_items(items.clone()).iter().enumerate() {
        let Some(key) = (match &item.key {
            Some(k) => param_key_from_expr(checker, arenas, *k, locals),
            None => Some(ParamKey::Positional(position as i64)),
        }) else {
            // An undecidable key makes only *that* binding unusable for
            // cross-referencing - the rest of the array is still real,
            // known information, not discarded wholesale.
            continue;
        };
        let value_ty = checker.infer_expr(arenas.expr(item.value)?, locals);
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
    Some(params)
}

fn describe_key(key: &ParamKey) -> String {
    match key {
        ParamKey::Named(name) => format!(":{name}"),
        ParamKey::Positional(index) => format!("?#{index}"),
    }
}

fn value_is_definitely_array(ty: &Type) -> bool {
    ty.is_array().yes()
}

impl<'a> Checker<'a> {
    /// `Doctrine\DBAL\Connection`/`Query\QueryBuilder`'s own
    /// SQL-executing methods - validates every real placeholder the
    /// resolved SQL text references has a bound value, and that no
    /// bound value is a real array where the placeholder isn't declared
    /// `ArrayParameterType::*`. Silently does nothing whenever any part
    /// of the resolution is undecidable (unfoldable SQL, no chain/array
    /// found) - the same "don't guess" leniency every other step
    /// of this feature already has.
    // Every argument here is already its own independently-meaningful
    // piece of the one real call site (`checker/expr.rs`'s own
    // `MethodCall` arm) that needs it - mirrors this project's own
    // `run_check_project`-style precedent for a single-call-site wiring
    // function, not a blanket suppression.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_dbal_parameter_binding(
        &mut self,
        arenas: &NodeArenas,
        class: &str,
        method: &str,
        recv: &ExprNode,
        args: &[Arg],
        locals: &Locals,
        span: Span,
    ) {
        if !matches!(
            method,
            "fetchAssociative"
                | "fetchAllAssociative"
                | "fetchOne"
                | "fetchFirstColumn"
                | "executeQuery"
                | "executeStatement"
        ) {
            return;
        }
        let is_connection = self
            .registry
            .is_subclass_of(class, "Doctrine\\DBAL\\Connection");
        let is_query_builder = self
            .registry
            .is_subclass_of(class, "Doctrine\\DBAL\\Query\\QueryBuilder");
        if !is_connection && !is_query_builder {
            return;
        }

        let (sql, bindings) = if is_query_builder {
            let Some(chain) = self.resolve_dbal_query_builder_chain(recv, locals) else {
                return;
            };
            let Some(sql) = chain.render() else {
                return;
            };
            (sql, chain.params)
        } else {
            let Some(sql_expr) = arenas.expr(args[0].value) else {
                return;
            };
            let Some(sql) = self.foldable_sql_string(sql_expr, locals) else {
                return;
            };
            let Some(bindings) = connection_params_from_args(self, arenas, args, locals) else {
                return;
            };
            (sql, bindings)
        };

        let Some(placeholder_keys) = sql_placeholder_keys(&sql) else {
            return;
        };
        if placeholder_keys.is_empty() {
            return;
        }

        for key in &placeholder_keys {
            if !bindings.iter().any(|b| b.key == *key) {
                report!(self, format!(
                        "{class}::{method}() call's own SQL references parameter {} with no bound value",
                        describe_key(key)
                    ),
                    span,
                    0,
                    "sqlReflection.missingParameter",
                );
            }
        }
        for binding in &bindings {
            if !binding.expects_array
                && placeholder_keys.contains(&binding.key)
                && value_is_definitely_array(&binding.value_ty)
            {
                report!(self, format!(
                        "{class}::{method}() binds an array to parameter {} without ArrayParameterType::*, which every plain SQL placeholder rejects at runtime",
                        describe_key(&binding.key)
                    ),
                    span,
                    0,
                    "sqlReflection.invalidParameterType",
                );
            }
            if binding.expects_integer
                && placeholder_keys.contains(&binding.key)
                && value_is_confidently_not_an_integer(&binding.value_ty)
            {
                report!(self, format!(
                        "{class}::{method}() binds a non-numeric string to parameter {} declared as an integer type",
                        describe_key(&binding.key)
                    ),
                    span,
                    0,
                    "sqlReflection.invalidParameterType",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
