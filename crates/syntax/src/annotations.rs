//! Facts about where a node sits in its file, computed once after parsing
//! and looked up by [`NodeId`] - what a checker would otherwise track in
//! mutable state while it walks: the node's parent, whether an expression
//! is a condition or a statement of its own, the `catch` types around it,
//! which labels a `goto` jumps to, whether a `declare` opens the file, and
//! `PHP_VERSION_ID` comparisons.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::ast::*;
use crate::visit::{Visitor, walk_expr, walk_stmt, walk_stmts};

const CONDITION: u8 = 1;
const LAST_CONDITION: u8 = 2;
const STATEMENT_EXPRESSION: u8 = 4;
const GOTO_TARGET: u8 = 8;
const FIRST_STATEMENT: u8 = 16;
const NO_CATCH: u32 = u32::MAX;

/// A comparison of `PHP_VERSION_ID` with an integer literal, written as
/// `PHP_VERSION_ID <op> version` (a literal on the left is turned around).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionCheck {
    pub op: BinOp,
    pub version: i64,
}

/// The side table for one file.
#[derive(Debug, Default)]
pub struct Annotations {
    parents: Vec<Option<NodeId>>,
    flags: Vec<u8>,
    catch_scope: Vec<u32>,
    catch_scopes: Vec<Vec<QualifiedName>>,
    version_checks: FxHashMap<NodeId, VersionCheck>,
}

impl Annotations {
    fn flag(&self, id: NodeId, flag: u8) -> bool {
        self.flags.get(id.index()).is_some_and(|f| f & flag != 0)
    }

    /// The statement or expression `id` sits directly in.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.parents.get(id.index()).copied().flatten()
    }

    /// Whether an expression is the condition of an `if`, `elseif`,
    /// `while`, `do-while` or ternary.
    pub fn is_condition(&self, id: NodeId) -> bool {
        self.flag(id, CONDITION)
    }

    /// Whether an expression is the condition of an `elseif` with no
    /// `else` after it, which acts as the chain's implicit `else`.
    pub fn is_last_condition(&self, id: NodeId) -> bool {
        self.flag(id, LAST_CONDITION)
    }

    /// Whether an expression is a statement on its own (`foo();`), so its
    /// value is discarded.
    pub fn is_statement_expression(&self, id: NodeId) -> bool {
        self.flag(id, STATEMENT_EXPRESSION)
    }

    /// Whether a label statement is the target of a `goto` in the same
    /// function body.
    pub fn is_goto_target(&self, id: NodeId) -> bool {
        self.flag(id, GOTO_TARGET)
    }

    /// Whether a statement is the file's first (nested first statements
    /// of a leading block included), where `declare(strict_types=1)` must
    /// be.
    pub fn is_first_statement(&self, id: NodeId) -> bool {
        self.flag(id, FIRST_STATEMENT)
    }

    /// The types the enclosing `try` blocks of this function body catch,
    /// innermost `try` last.
    pub fn catch_types(&self, id: NodeId) -> &[QualifiedName] {
        match self.catch_scope.get(id.index()) {
            Some(&scope) if scope != NO_CATCH => &self.catch_scopes[scope as usize],
            _ => &[],
        }
    }

    /// A condition comparing `PHP_VERSION_ID` with an integer literal.
    pub fn version_check(&self, id: NodeId) -> Option<VersionCheck> {
        self.version_checks.get(&id).copied()
    }
}

/// Computes the annotations of a parsed file.
pub fn annotate(stmts: &[Stmt]) -> Annotations {
    let mut builder = Builder {
        out: Annotations::default(),
        path: Vec::new(),
        catch_stack: Vec::new(),
        current_catch: NO_CATCH,
        gotos: FxHashSet::default(),
        labels: Vec::new(),
        elseifs: FxHashSet::default(),
    };
    mark_first_statements(stmts, &mut builder.out);
    builder.body(stmts);
    builder.out
}

struct Builder {
    out: Annotations,
    /// The statements and expressions enclosing the current node.
    path: Vec<NodeId>,
    /// The catch types of each enclosing `try` in this function body.
    catch_stack: Vec<Vec<QualifiedName>>,
    current_catch: u32,
    /// The current function body's `goto` targets and labels.
    gotos: FxHashSet<crate::name::Name>,
    labels: Vec<(crate::name::Name, NodeId)>,
    /// `if` statements that are an `elseif` of a chain already marked.
    elseifs: FxHashSet<NodeId>,
}

impl Builder {
    fn slot<T: Default + Clone>(v: &mut Vec<T>, id: NodeId) -> &mut T {
        let i = id.index();
        if v.len() <= i {
            v.resize(i + 1, T::default());
        }
        &mut v[i]
    }

    fn set(&mut self, id: NodeId, flag: u8) {
        if id != NodeId::SYNTHETIC {
            *Self::slot(&mut self.out.flags, id) |= flag;
        }
    }

    /// Records a node's parent and catch scope before walking into it.
    fn enter(&mut self, id: NodeId) {
        if id == NodeId::SYNTHETIC {
            return;
        }
        *Self::slot(&mut self.out.parents, id) = self.path.last().copied();
        if self.current_catch != NO_CATCH {
            let len = self.out.catch_scope.len();
            if len <= id.index() {
                self.out.catch_scope.resize(id.index() + 1, NO_CATCH);
            }
            self.out.catch_scope[id.index()] = self.current_catch;
        }
    }

    /// Walks one function body: `goto` targets and `catch` types do not
    /// cross it.
    fn body(&mut self, stmts: &[Stmt]) {
        let gotos = std::mem::take(&mut self.gotos);
        let labels = std::mem::take(&mut self.labels);
        let catch_stack = std::mem::take(&mut self.catch_stack);
        let current_catch = std::mem::replace(&mut self.current_catch, NO_CATCH);
        walk_stmts(self, stmts);
        for (name, id) in std::mem::take(&mut self.labels) {
            if self.gotos.contains(&name) {
                self.set(id, GOTO_TARGET);
            }
        }
        self.gotos = gotos;
        self.labels = labels;
        self.catch_stack = catch_stack;
        self.current_catch = current_catch;
    }

    fn condition(&mut self, cond: &Expr, last: bool) {
        self.set(cond.id, CONDITION);
        if last {
            self.set(cond.id, LAST_CONDITION);
        }
        if let Some(check) = version_check(cond) {
            self.out.version_checks.insert(cond.id, check);
        }
    }

    /// Conditions of an `if` and its `elseif` chain.
    fn if_chain(&mut self, stmt: &Stmt, is_elseif: bool) {
        if let StmtKind::If {
            cond, else_branch, ..
        } = &stmt.kind
        {
            self.condition(cond, is_elseif && else_branch.is_none());
            if let Some(else_branch) = else_branch
                && matches!(else_branch.kind, StmtKind::If { .. })
            {
                self.elseifs.insert(else_branch.id);
                self.if_chain(else_branch, true);
            }
        }
    }
}

fn version_check(cond: &Expr) -> Option<VersionCheck> {
    let ExprKind::Binary(op, l, r) = &cond.kind else {
        return None;
    };
    let is_version_id = |e: &Expr| matches!(&e.kind, ExprKind::Name(n) if n.trim_start_matches('\\') == "PHP_VERSION_ID");
    let literal = |e: &Expr| match &e.kind {
        ExprKind::Literal(Literal::Int(v)) => Some(*v),
        _ => None,
    };
    let flipped = |op: BinOp| match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Gt => BinOp::Lt,
        BinOp::LtEq => BinOp::GtEq,
        BinOp::GtEq => BinOp::LtEq,
        other => other,
    };
    if !matches!(
        op,
        BinOp::Lt
            | BinOp::Gt
            | BinOp::LtEq
            | BinOp::GtEq
            | BinOp::Eq
            | BinOp::Identical
            | BinOp::NotEq
            | BinOp::NotIdentical
    ) {
        return None;
    }
    if is_version_id(l) {
        literal(r).map(|version| VersionCheck { op: *op, version })
    } else if is_version_id(r) {
        literal(l).map(|version| VersionCheck {
            op: flipped(*op),
            version,
        })
    } else {
        None
    }
}

/// The file's first statement, descending into a leading block.
fn mark_first_statements(stmts: &[Stmt], out: &mut Annotations) {
    let mut current = stmts.first();
    while let Some(stmt) = current {
        *Builder::slot(&mut out.flags, stmt.id) |= FIRST_STATEMENT;
        current = match &stmt.kind {
            StmtKind::Block(inner) => inner.first(),
            _ => None,
        };
    }
}

impl<'ast> Visitor<'ast> for Builder {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        self.enter(stmt.id);
        match &stmt.kind {
            StmtKind::Expression(e) => self.set(e.id, STATEMENT_EXPRESSION),
            // An `elseif` is marked by the `if` that heads its chain.
            StmtKind::If { .. } if !self.elseifs.contains(&stmt.id) => {
                self.if_chain(stmt, false);
            }
            StmtKind::While { cond, .. } | StmtKind::DoWhile { cond, .. } => {
                self.condition(cond, false)
            }
            StmtKind::Goto(name) => {
                self.gotos.insert(name.name.clone());
            }
            StmtKind::Label(name) => self.labels.push((name.name.clone(), stmt.id)),
            _ => {}
        }
        self.path.push(stmt.id);
        match &stmt.kind {
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                let caught: Vec<QualifiedName> = catches
                    .iter()
                    .flat_map(|c| c.types.iter().cloned())
                    .collect();
                self.catch_stack.push(caught);
                let outer = self.current_catch;
                self.current_catch = self.out.catch_scopes.len() as u32;
                self.out
                    .catch_scopes
                    .push(self.catch_stack.iter().flatten().cloned().collect());
                walk_stmts(self, body);
                self.catch_stack.pop();
                self.current_catch = outer;
                for clause in catches {
                    self.visit_catch_clause(clause);
                }
                if let Some(finally) = finally {
                    walk_stmts(self, finally);
                }
            }
            _ => walk_stmt(self, stmt),
        }
        self.path.pop();
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        self.enter(expr.id);
        if let ExprKind::Ternary(cond, ..) = &expr.kind {
            self.condition(cond, false);
        }
        self.path.push(expr.id);
        match &expr.kind {
            ExprKind::Closure { params, body, .. } => {
                for param in params {
                    self.visit_param(param);
                }
                self.body(body);
            }
            _ => walk_expr(self, expr),
        }
        self.path.pop();
    }

    fn visit_function_decl(&mut self, f: &'ast FunctionDecl) {
        for attr in &f.attrs {
            self.visit_attribute(attr);
        }
        for param in &f.params {
            self.visit_param(param);
        }
        self.body(&f.body);
    }

    fn visit_property_hooks(&mut self, hooks: &'ast PropertyHooks) {
        for attr in hooks.get_attrs.iter().chain(&hooks.set_attrs) {
            self.visit_attribute(attr);
        }
        if let Some((Some(param), _)) = &hooks.set {
            self.visit_param(param);
        }
        for body in [hooks.get.as_ref(), hooks.set.as_ref().map(|(_, b)| b)]
            .into_iter()
            .flatten()
        {
            match body {
                PropertyHookBody::Arrow(e) => self.visit_expr(e),
                PropertyHookBody::Block(stmts) => self.body(stmts),
                PropertyHookBody::Abstract => {}
            }
        }
    }
}

#[cfg(test)]
mod tests;
