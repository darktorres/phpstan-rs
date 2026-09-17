//! Symbolic Doctrine `QueryBuilder` fluent-chain resolution (PHASE_BACKLOG
//! Phase 168 168b) - given the receiver expression of a `->getQuery()`
//! call, walks the fluent `MethodCall` chain backward collecting every
//! call that can affect the query's own result-row *shape*
//! (`select`/`addSelect`/`from`/`join`/`innerJoin`/`leftJoin`), purely
//! syntactically. No live `QueryBuilder` object, no execution, unlike
//! real PHPStan's own `phpstan-doctrine` extension, which actually
//! replays the chain onto a real `QueryBuilder` it gets from a real
//! `EntityManager` instance.
//!
//! `where`/`andWhere`/`having`/`groupBy`/`orderBy`/`setParameter`/etc.
//! are dropped entirely, not merely tolerated when unresolvable - they
//! filter/order rows, never add or remove a result column, so this
//! module never even attempts to resolve their own arguments.

use crate::ast::{Arg, Expr, ExprKind};
use crate::checker::Checker;
use crate::checker::assignment::literal_string_value;
use crate::types::Type;

/// One `join`/`innerJoin`/`leftJoin` call's own target - either a real
/// entity class (`join(Entity::class, alias)`) or a dotted association
/// path off an already-known alias (`join('t.assoc', alias)`, the far
/// more common real shape) - 168d resolves either against the already-
/// collected `from`/prior-join alias map plus 168a's association
/// metadata; this module only collects the raw text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinClause {
    pub(crate) target: String,
    pub(crate) alias: String,
}

/// One `->getQuery()` call's own resolved `QueryBuilder` chain - `None`
/// fields mean "never set," not "set to nothing," since a query missing
/// its own `from` (or, for methods 168c/168d actually resolve into row
/// shapes, its own select) is real, valid-to-detect-but-undecidable DQL,
/// not this module's own failure to look hard enough.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct QueryBuilderChain {
    pub(crate) from: Option<(String, String)>,
    pub(crate) joins: Vec<JoinClause>,
    /// Every `select()`/`addSelect()` argument's own folded string, in
    /// call order - `select()` resets this to just its own arguments;
    /// `addSelect()` appends to whatever's already here. 168c parses
    /// each entry as one DQL select item.
    pub(crate) select_args: Vec<String>,
}

/// One collected fluent-chain call, in AST (outermost-first) order -
/// `resolve_query_builder_chain` reverses this once the chain root is
/// identified, since a fluent chain reads outer-to-inner in the AST but
/// executes root-to-outer chronologically.
struct ChainCall<'a> {
    method: &'a str,
    args: &'a [Arg],
}

impl<'a> Checker<'a> {
    /// The receiver expression of a `->getQuery()` call - resolves its
    /// own fluent `QueryBuilder` chain, `None` when any part of it is
    /// undecidable (a dynamic argument, an unrecognized chain root) -
    /// the identical "don't guess" fallback real PHPStan's own extension
    /// takes at the same point (`return null`).
    pub(crate) fn resolve_query_builder_chain(
        &self,
        recv: &Expr,
        locals: &crate::checker::Locals,
    ) -> Option<QueryBuilderChain> {
        let mut calls: Vec<ChainCall<'_>> = Vec::new();
        let mut cursor = recv;
        loop {
            match &cursor.kind {
                // `createQueryBuilder(...)` is the chain root itself
                // (`resolve_chain_root`'s own job to interpret), not
                // just another call to collect and descend past -
                // stopping here without descending is what makes `cursor`
                // still point *at* this call node afterward.
                ExprKind::MethodCall(_, method, _, _) if method == "createQueryBuilder" => break,
                ExprKind::MethodCall(inner, method, args, _) => {
                    calls.push(ChainCall { method, args });
                    cursor = inner;
                }
                ExprKind::Grouping(inner) => cursor = inner,
                _ => break,
            }
        }
        // `calls` was pushed outermost-first while walking backward -
        // reverse it once, here, so the rest of this function (and every
        // caller) can just read it in real chronological call order.
        calls.reverse();

        let mut chain = self.resolve_chain_root(cursor, locals)?;
        for call in calls {
            self.apply_chain_call(&mut chain, call.method, call.args, locals)?;
        }
        Some(chain)
    }

    /// The two real chain-root shapes, checked directly against
    /// `EntityRepository::createQueryBuilder()`'s own source
    /// (`vendor/doctrine-orm/src/EntityRepository.php:53-58`) - anything
    /// else is undecidable.
    fn resolve_chain_root(
        &self,
        root: &Expr,
        locals: &crate::checker::Locals,
    ) -> Option<QueryBuilderChain> {
        let ExprKind::MethodCall(target, method, args, _) = &root.kind else {
            return None;
        };
        if method != "createQueryBuilder" {
            return None;
        }
        // `$this->createQueryBuilder($alias)` - real Doctrine defines
        // this as pure sugar for `$em->createQueryBuilder()->
        // select($alias)->from($entityName, $alias)`, so this chain root
        // must synthesize *both* an implicit `select`/`from` - the
        // corpus-dominant shape (100+ real call sites across 18
        // repository files), likely more common than the explicit-
        // `from()` form below.
        if matches!(&target.kind, ExprKind::Variable(v) if v == "this")
            && let Some(alias) = args
                .first()
                .and_then(|a| self.foldable_arg_string(a, locals))
            && let Some(current_class) = &self.current_class
            && let Some(entity) = self.registry.repository_entity(current_class)
        {
            return Some(QueryBuilderChain {
                from: Some((entity.to_string(), alias.clone())),
                joins: Vec::new(),
                select_args: vec![alias],
            });
        }
        // `$em->createQueryBuilder()` (or `$this->getEntityManager()->
        // createQueryBuilder()`) - no implicit select/from at all;
        // resolvable regardless of what the receiver itself is, since a
        // wrong-typed receiver would already have failed to resolve
        // `getQuery()` as a real Doctrine call further up the chain.
        if args.is_empty() {
            return Some(QueryBuilderChain::default());
        }
        None
    }

    /// Applies one chain call's own effect on `chain`, in place. Returns
    /// `None` (propagated by `?` at the call site) only when a call this
    /// module *does* care about has an unresolvable argument - an
    /// unrecognized method name is simply ignored, matching this
    /// project's own `resolve_query_builder_chain`-level "only select/
    /// from/join shape the result" scope.
    fn apply_chain_call(
        &self,
        chain: &mut QueryBuilderChain,
        method: &str,
        args: &[Arg],
        locals: &crate::checker::Locals,
    ) -> Option<()> {
        match method {
            "select" => {
                chain.select_args = self.foldable_arg_strings(args, locals)?;
            }
            "addSelect" => {
                chain
                    .select_args
                    .extend(self.foldable_arg_strings(args, locals)?);
            }
            "from" => {
                let entity = args
                    .first()
                    .and_then(|a| self.foldable_class_name(a, locals))?;
                let alias = args
                    .get(1)
                    .and_then(|a| self.foldable_arg_string(a, locals))?;
                chain.from = Some((entity, alias));
            }
            "join" | "innerJoin" | "leftJoin" => {
                let target = args
                    .first()
                    .and_then(|a| self.foldable_class_name(a, locals))
                    .or_else(|| {
                        args.first()
                            .and_then(|a| self.foldable_arg_string(a, locals))
                    })?;
                let alias = args
                    .get(1)
                    .and_then(|a| self.foldable_arg_string(a, locals))?;
                chain.joins.push(JoinClause { target, alias });
            }
            _ => {}
        }
        Some(())
    }

    /// A `from`/`join` first-argument's own class name, when it's a
    /// `SomeEntity::class` expression - resolved through this file's own
    /// `use` imports via `self.name_ctx`, matching how a real DQL
    /// `FROM`/`JOIN` clause names its own target class.
    fn foldable_class_name(&self, arg: &Arg, _locals: &crate::checker::Locals) -> Option<String> {
        match &arg.value.kind {
            ExprKind::StaticAccess(class_name, member) if member.eq_ignore_ascii_case("class") => {
                Some(self.name_ctx.resolve(class_name))
            }
            _ => None,
        }
    }

    fn foldable_arg_string(&self, arg: &Arg, locals: &crate::checker::Locals) -> Option<String> {
        if let Some(s) = literal_string_value(&arg.value) {
            return Some(s.into_owned());
        }
        match self.infer_expr(&arg.value, locals) {
            Type::StringLiteral(s) => Some(s),
            _ => None,
        }
    }

    fn foldable_arg_strings(
        &self,
        args: &[Arg],
        locals: &crate::checker::Locals,
    ) -> Option<Vec<String>> {
        args.iter()
            .map(|a| self.foldable_arg_string(a, locals))
            .collect()
    }
}
