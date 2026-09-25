//! Symbolic Doctrine `QueryBuilder` fluent-chain resolution (ROADMAP
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

use crate::checker::{Checker, Locals};
use syntax::ast::literal_string_value;
use syntax::ast::{Arg, Expr, ExprKind};
use types::Type;

/// A cheap syntactic pre-check mirroring `resolve_query_builder_chain`'s
/// own `MethodCall`/`Grouping` walk - "does this expression's own chain
/// spine eventually reach a `createQueryBuilder()` call" - without
/// resolving anything semantically (no `Checker`/`Locals` needed).
/// Storing every assignment's RHS unconditionally would also be correct
/// (nothing ever reads an entry whose name is never tested via a later
/// `->getQuery()`), but costs a real `Expr` clone per assignment,
/// project-wide; this keeps that cost to the rare, actually-relevant one.
fn looks_like_query_builder_chain(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::MethodCall(_, method, _, _) if **method == *"createQueryBuilder" => true,
        ExprKind::MethodCall(inner, _, _, _) => looks_like_query_builder_chain(inner),
        ExprKind::Grouping(inner) => looks_like_query_builder_chain(inner),
        _ => false,
    }
}

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
    /// A `LEFT JOIN` - its alias may match no row, so everything read
    /// through it can be `null`.
    pub(crate) left: bool,
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
    /// Whether `->delete(...)`/`->update(...)` appeared anywhere in the
    /// chain. `EntityRepository::createQueryBuilder()` (`vendor/doctrine-
    /// orm/src/EntityRepository.php:53-58`) always pre-populates a
    /// `select`/`from` for its own entity, and real `QueryBuilder::
    /// delete()`/`update()` only flip an internal query-type flag without
    /// clearing it - so a `->delete()->where(...)` chain looks exactly
    /// like an ordinary `SELECT` to everything else in this module.
    /// `AbstractQuery::_doExecute()` returns a plain affected-row `int`
    /// for either statement type, bypassing hydration entirely
    /// (confirmed directly against `vendor/doctrine-orm`), so every
    /// select-shape field above is meaningless once this is `true`.
    pub(crate) dml: bool,
    /// A `GROUP BY` is present - without one, an aggregate over zero
    /// matching rows yields `null`.
    pub(crate) group_by: bool,
    /// The `INDEX BY` path (`alias.field`) the result collection is keyed
    /// by, instead of a list.
    pub(crate) index_by: Option<String>,
}

/// One collected fluent-chain call, in AST (outermost-first) order -
/// `resolve_query_builder_chain` reverses this once the chain root is
/// identified, since a fluent chain reads outer-to-inner in the AST but
/// executes root-to-outer chronologically. Owns its own data (not
/// borrowed from the walked expression) since the walk's own `cursor`
/// may switch from borrowing the original expression tree to an owned
/// `Rc<Expr>` read back from `QueryBuilderAliasMap` partway through
/// (`Cursor`'s own doc comment) - a borrow tied to one generation of
/// `cursor` can't outlive its own replacement.
struct ChainCall {
    method: String,
    args: Vec<Arg>,
}

/// `resolve_query_builder_chain`'s own walk cursor - starts borrowing the
/// real receiver expression tree (the common case, no clone needed), but
/// switches to owning a cloned `Rc<Expr>` the moment the walk reads
/// through a `$qb = <chain>;` variable alias (`QueryBuilderAliasMap`'s
/// own doc comment) - the stored expression's lifetime is independent of
/// the original receiver's, so continuing to borrow from it isn't an
/// option once that happens.
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
    /// The store half of `QueryBuilderAliasMap`'s own mechanism (see its
    /// doc comment) - called right after an ordinary assignment binds
    /// `lhs`, mirroring `record_condition_alias`'s own call site and
    /// cheap-fast-path shape (a plain syntactic check with no clone,
    /// paid only by the rare RHS that actually qualifies).
    pub(crate) fn record_query_builder_alias(&self, lhs: &Expr, rhs: &Expr, locals: &mut Locals) {
        let ExprKind::Variable(name) = &lhs.kind else {
            return;
        };
        if looks_like_query_builder_chain(rhs) {
            locals.set_query_builder_alias(name.to_string(), std::rc::Rc::new(rhs.clone()));
        }
    }

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
        let mut calls: Vec<ChainCall> = Vec::new();
        let mut cursor = Cursor::Borrowed(recv);
        loop {
            match &cursor.kind {
                // `createQueryBuilder(...)` is the chain root itself
                // (`resolve_chain_root`'s own job to interpret), not
                // just another call to collect and descend past -
                // stopping here without descending is what makes `cursor`
                // still point *at* this call node afterward.
                ExprKind::MethodCall(_, method, _, _) if **method == *"createQueryBuilder" => break,
                ExprKind::MethodCall(inner, method, args, _) => {
                    calls.push(ChainCall {
                        method: method.to_string(),
                        args: args.clone(),
                    });
                    cursor = Cursor::Owned(std::rc::Rc::new((**inner).clone()));
                }
                ExprKind::Grouping(inner) => {
                    cursor = Cursor::Owned(std::rc::Rc::new((**inner).clone()))
                }
                // `$qb = $this->createQueryBuilder(...)->...; ...; $qb->
                // getQuery()...` - a chain split into a variable before
                // its own terminal call, real and recurring in this
                // corpus (`QueryBuilderAliasMap`'s own doc comment), not
                // the "never happens" this module's own docs previously
                // claimed. Substitutes the stored assignment expression
                // and keeps walking - `$qb2 = $qb;` chains of aliases
                // resolve the same way, one substitution at a time.
                ExprKind::Variable(name) => match locals.query_builder_aliases.get(name.as_ref()) {
                    Some(rhs) => cursor = Cursor::Owned(rhs.clone()),
                    None => break,
                },
                _ => break,
            }
        }
        // `calls` was pushed outermost-first while walking backward -
        // reverse it once, here, so the rest of this function (and every
        // caller) can just read it in real chronological call order.
        calls.reverse();

        let mut chain = self.resolve_chain_root(&cursor, locals)?;
        for call in &calls {
            self.apply_chain_call(&mut chain, &call.method, &call.args, locals)?;
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
        if **method != *"createQueryBuilder" {
            return None;
        }
        // `$this->createQueryBuilder($alias)` - real Doctrine defines
        // this as pure sugar for `$em->createQueryBuilder()->
        // select($alias)->from($entityName, $alias)`, so this chain root
        // must synthesize *both* an implicit `select`/`from` - the
        // corpus-dominant shape (100+ real call sites across 18
        // repository files), likely more common than the explicit-
        // `from()` form below.
        if matches!(&target.kind, ExprKind::Variable(v) if **v == *"this")
            && let Some(alias) = args
                .first()
                .and_then(|a| self.foldable_arg_string(a, locals))
            && let Some(current_class) = &self.current_class
            && let Some(entity) = self.registry.repository_entity(current_class)
        {
            // `createQueryBuilder($alias, $indexBy)`.
            let index_by = args
                .get(1)
                .and_then(|a| self.foldable_arg_string(a, locals));
            return Some(QueryBuilderChain {
                from: Some((entity.to_string(), alias.clone())),
                joins: Vec::new(),
                select_args: vec![alias],
                dml: false,
                group_by: false,
                index_by,
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
                if let Some(index_by) = args
                    .get(2)
                    .and_then(|a| self.foldable_arg_string(a, locals))
                {
                    chain.index_by = Some(index_by);
                }
            }
            "indexBy" => {
                chain.index_by = Some(
                    args.get(1)
                        .and_then(|a| self.foldable_arg_string(a, locals))?,
                );
            }
            "groupBy" | "addGroupBy" => {
                chain.group_by = true;
            }
            "delete" | "update" => {
                chain.dml = true;
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
                if let Some(index_by) = args
                    .get(4)
                    .and_then(|a| self.foldable_arg_string(a, locals))
                {
                    chain.index_by = Some(index_by);
                }
                chain.joins.push(JoinClause {
                    target,
                    alias,
                    left: method == "leftJoin",
                });
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
            ExprKind::ClassNameFetch(target) => target.as_name().map(|n| self.name_ctx.resolve(n)),
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
