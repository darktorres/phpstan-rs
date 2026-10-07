//! The classes, functions and constants a file declares, wherever they
//! sit. All node IDs are resolved through the owning file's arenas.

use crate::ast::*;
use crate::name_resolution::NameContext;
use crate::php_string::PhpString;
use crate::span::Span;
use crate::visit::arena::{self, Visitor};

/// What a declaration declares.
#[derive(Clone)]
pub enum Declared<'a> {
    Class(&'a ClassDecl),
    Function(&'a FunctionDecl),
    Constant(&'a ClassConst),
    Define { name: PhpString, value: ExprId },
}

/// A declaration in a file.
#[derive(Clone)]
pub struct Declaration<'a> {
    pub declared: Declared<'a>,
    pub span: Span,
    pub conditional: bool,
}

impl Declaration<'_> {
    /// The fully-qualified name it declares.
    pub fn name(&self) -> String {
        match &self.declared {
            Declared::Class(c) => c.fq_name().to_string(),
            Declared::Function(f) => f.fq_name().to_string(),
            Declared::Constant(c) => c.fq_name().to_string(),
            Declared::Define { name, .. } => name.to_str_lossy().into_owned(),
        }
    }
}

/// One thing that must hold for a conditional declaration to run.
#[derive(Clone, Copy)]
pub enum GuardTerm {
    Condition { expr: ExprId, negated: bool },
    Opaque,
}

pub fn for_each_declaration<'a>(
    arenas: &'a NodeArenas,
    stmts: &[StmtId],
    f: impl FnMut(Declaration<'a>, &NameContext, &[GuardTerm]),
) {
    for_each_declaration_in(arenas, stmts, true, f);
}

pub fn for_each_declaration_in<'a>(
    arenas: &'a NodeArenas,
    stmts: &[StmtId],
    in_expressions: bool,
    mut f: impl FnMut(Declaration<'a>, &NameContext, &[GuardTerm]),
) {
    let mut walk = Declarations {
        arenas,
        in_expressions,
        ctx: NameContext::default(),
        conditional: false,
        early: Vec::new(),
        guards: Vec::new(),
        f: &mut f,
    };
    for &stmt_id in stmts {
        walk.visit_stmt(arenas, stmt_id);
        if let Some(StmtNode {
            kind: StmtKind::If(if_stmt),
            ..
        }) = arenas.stmt(stmt_id)
            && arenas.elseifs(if_stmt.elseifs.clone()).is_empty()
            && if_stmt.else_branch.is_none()
            && ends_in_return(arenas, if_stmt.then_branch)
        {
            walk.early.push(GuardTerm::Condition {
                expr: if_stmt.cond,
                negated: true,
            });
        }
    }
}

fn ends_in_return(arenas: &NodeArenas, id: StmtId) -> bool {
    let Some(stmt) = arenas.stmt(id) else {
        return false;
    };
    match &stmt.kind {
        StmtKind::Return(_) => true,
        StmtKind::Block(stmts) => arenas
            .stmt_ids(stmts)
            .last()
            .is_some_and(|last| ends_in_return(arenas, *last)),
        _ => false,
    }
}

struct Declarations<'a, 'f, F> {
    arenas: &'a NodeArenas,
    in_expressions: bool,
    ctx: NameContext,
    conditional: bool,
    early: Vec<GuardTerm>,
    guards: Vec<GuardTerm>,
    f: &'f mut F,
}

impl<'a, F: FnMut(Declaration<'a>, &NameContext, &[GuardTerm])> Declarations<'a, '_, F> {
    fn found(&mut self, declared: Declared<'a>, span: Span) {
        let declaration = Declaration {
            declared,
            span,
            conditional: self.conditional,
        };
        if self.conditional && !self.early.is_empty() {
            let guards: Vec<_> = self.early.iter().chain(&self.guards).copied().collect();
            (self.f)(declaration, &self.ctx, &guards);
        } else {
            (self.f)(declaration, &self.ctx, &self.guards);
        }
    }

    fn conditionally(&mut self, walk: impl FnOnce(&mut Self)) {
        self.guarded(GuardTerm::Opaque, walk);
    }

    fn guarded(&mut self, term: GuardTerm, walk: impl FnOnce(&mut Self)) {
        self.guards.push(term);
        let was = std::mem::replace(&mut self.conditional, true);
        walk(self);
        self.conditional = was;
        self.guards.pop();
    }

    fn if_chain(&mut self, arenas: &NodeArenas, if_stmt: &IfStmt) {
        let was = std::mem::replace(&mut self.conditional, true);
        let mark = self.guards.len();
        let branches = std::iter::once((if_stmt.cond, if_stmt.then_branch)).chain(
            arenas
                .elseifs(if_stmt.elseifs.clone())
                .iter()
                .map(|elseif| (elseif.cond, elseif.body)),
        );
        for (expr, body) in branches {
            self.visit_expr(arenas, expr);
            self.guards.push(GuardTerm::Condition {
                expr,
                negated: false,
            });
            self.visit_stmt(arenas, body);
            self.guards.pop();
            self.guards.push(GuardTerm::Condition {
                expr,
                negated: true,
            });
        }
        if let Some(else_branch) = if_stmt.else_branch {
            self.visit_stmt(arenas, else_branch);
        }
        self.guards.truncate(mark);
        self.conditional = was;
    }
}

impl<'a, F: FnMut(Declaration<'a>, &NameContext, &[GuardTerm])> Visitor
    for Declarations<'a, '_, F>
{
    fn visit_stmt(&mut self, _arenas: &NodeArenas, id: StmtId) {
        let arenas = self.arenas;
        let Some(stmt) = arenas.stmt(id) else {
            return;
        };
        crate::stack::guard(|| match &stmt.kind {
            StmtKind::Namespace(name) => self.ctx.set_namespace(name),
            StmtKind::Use(imports) => self.ctx.add_uses(arenas.use_imports(imports)),
            StmtKind::ClassDecl(class) => {
                let class = arenas.class_decl(class.clone());
                self.found(
                    Declared::Class(class),
                    arenas
                        .stmt_span(id)
                        .expect("class declaration statement span"),
                );
                self.conditionally(|v| arena::walk_class_decl(v, arenas, class));
            }
            StmtKind::FunctionDecl(function_id) => {
                if let Some(function) = arenas.function(*function_id) {
                    self.found(
                        Declared::Function(function),
                        arenas
                            .stmt_span(id)
                            .expect("function declaration statement span"),
                    );
                    self.conditionally(|v| arena::walk_function_decl(v, arenas, function));
                }
            }
            StmtKind::GlobalConst(constants) => {
                for constant in arenas.class_consts(constants) {
                    self.found(
                        Declared::Constant(constant),
                        arenas
                            .stmt_span(id)
                            .expect("constant declaration statement span"),
                    );
                }
                arena::walk_stmt(self, arenas, id);
            }
            StmtKind::If(if_stmt) => self.if_chain(arenas, if_stmt),
            StmtKind::While { .. }
            | StmtKind::DoWhile { .. }
            | StmtKind::For { .. }
            | StmtKind::Foreach { .. }
            | StmtKind::Switch { .. }
            | StmtKind::Try { .. } => {
                self.conditionally(|v| arena::walk_stmt(v, arenas, id));
            }
            _ => arena::walk_stmt(self, arenas, id),
        });
    }

    fn visit_expr(&mut self, _arenas: &NodeArenas, id: ExprId) {
        let arenas = self.arenas;
        if !self.in_expressions {
            return;
        }
        let Some(expr) = arenas.expr(id) else {
            return;
        };
        if let Some((name, value)) = define_call(arenas, id) {
            let span = arenas
                .expr_span(id)
                .expect("declaration walk expression has a parallel span");
            self.found(Declared::Define { name, value }, span);
        }
        self.conditionally(|v| arena::walk_expr(v, arenas, id));
    }
}

fn define_call(arenas: &NodeArenas, id: ExprId) -> Option<(PhpString, ExprId)> {
    let ExprKind::Call(callee, args) = &arenas.expr(id)?.kind else {
        return None;
    };
    let args = arenas.args(args.clone());
    let ExprKind::Name(name) = &arenas.expr(*callee)?.kind else {
        return None;
    };
    let name = arenas.qualified_name(name)?;
    let (primary, fallback) = name.candidates();
    if !fallback.unwrap_or(primary).eq_ignore_ascii_case("define")
        || args.len() < 2
        || args.iter().any(|arg| arg.spread || arg.name.is_some())
    {
        return None;
    }
    let ExprKind::Literal(Literal::String(const_name)) = &arenas.expr(args[0].value)?.kind else {
        return None;
    };
    let bytes = arenas.string_bytes(const_name)?;
    Some((
        PhpString::from_bytes(std::sync::Arc::<[u8]>::from(bytes)),
        args[1].value,
    ))
}

#[cfg(test)]
mod tests;
