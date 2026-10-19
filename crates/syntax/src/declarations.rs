//! The classes, functions and constants a file declares, wherever they
//! sit.

use crate::ast::*;
use crate::name_resolution::NameContext;
use crate::php_string::PhpString;
use crate::span::Span;
use crate::visit::{Visitor, walk_class_decl, walk_expr, walk_function_decl, walk_stmt};

/// What a declaration declares.
#[derive(Clone, Copy)]
pub enum Declared<'a> {
    /// A class, interface, trait or enum.
    Class(&'a ClassDecl),
    Function(&'a FunctionDecl),
    /// One constant of a `const` statement.
    Constant(&'a ClassConst),
    /// `define('NAME', value)`: `name` is the constant's name as the
    /// string holds it, fully qualified.
    Define {
        name: &'a PhpString,
        value: &'a Expr,
    },
}

/// A declaration in a file.
#[derive(Clone, Copy)]
pub struct Declaration<'a> {
    pub declared: Declared<'a>,
    /// The declaring statement's span, attributes included; a
    /// `define()` call's span.
    pub span: Span,
    /// Whether it is declared only once some code runs: in a function or
    /// method body, a closure, a loop, a `switch`, a `try`, an `if` branch
    /// (the `if (!function_exists('foo')) { function foo() {} }` idiom), or
    /// nested in an expression. A braced namespace body or a `declare`
    /// block runs as the file does.
    pub conditional: bool,
}

impl Declaration<'_> {
    /// The fully-qualified name it declares.
    pub fn name(&self) -> String {
        match self.declared {
            Declared::Class(c) => c.fq_name().to_string(),
            Declared::Function(f) => f.fq_name().to_string(),
            Declared::Constant(c) => c.fq_name().to_string(),
            Declared::Define { name, .. } => name.to_str_lossy().into_owned(),
        }
    }
}

/// One thing that must hold for a conditional declaration to run, outermost
/// first.
#[derive(Clone, Copy)]
pub enum GuardTerm<'a> {
    /// An `if` or `elseif` condition, taken as it stands or - for a later
    /// branch of the chain, an `else`, or the code after an
    /// `if (c) { return; }` - negated.
    Condition { expr: &'a Expr, negated: bool },
    /// A loop, `switch`, `try`, function, class or expression the
    /// declaration is nested in: running code that decides nothing a reader
    /// of the terms could know.
    Opaque,
}

/// Calls `f` with every declaration in `stmts`, in source order, with the
/// name context in effect where it sits - namespaces and imports aren't
/// scoped to blocks or functions - and the terms that guard it, empty for
/// an unconditional one.
///
/// An `if (c) { return …; }` without an `else` at the top of the file
/// guards, with `!c`, every declaration nested after it. It guards no
/// declaration at the top level: PHP declares those before running
/// anything, so they exist whatever the return decides.
pub fn for_each_declaration<'a>(
    stmts: &'a [Stmt],
    f: impl FnMut(Declaration<'a>, &NameContext, &[GuardTerm<'a>]),
) {
    for_each_declaration_in(stmts, true, f);
}

/// `for_each_declaration`, which looks inside expressions only when
/// `in_expressions`: for a `define()` call, and for a declaration in the
/// body of a closure or an anonymous class. A file in which there is
/// neither (`FileFacts`) has no declaration there to find, and its
/// expressions need not be walked.
pub fn for_each_declaration_in<'a>(
    stmts: &'a [Stmt],
    in_expressions: bool,
    f: impl FnMut(Declaration<'a>, &NameContext, &[GuardTerm<'a>]),
) {
    for_each_declaration_with(stmts, in_expressions, true, f);
}

/// `for_each_declaration_in`, which enters the body of a function or
/// method only when `in_bodies`: a file with no declaration as a statement
/// of a body (`FileFacts::has_declaration_in_body`), and none in an
/// expression, has none there to find.
pub fn for_each_declaration_with<'a>(
    stmts: &'a [Stmt],
    in_expressions: bool,
    in_bodies: bool,
    mut f: impl FnMut(Declaration<'a>, &NameContext, &[GuardTerm<'a>]),
) {
    let mut walk = Declarations {
        in_expressions,
        in_bodies,
        ctx: NameContext::default(),
        conditional: false,
        early: Vec::new(),
        guards: Vec::new(),
        f: &mut f,
    };
    for stmt in stmts {
        walk.visit_stmt(stmt);
        if let StmtKind::If(if_stmt) = &stmt.kind
            && if_stmt.elseifs.is_empty()
            && if_stmt.else_branch.is_none()
            && ends_in_return(&if_stmt.then_branch)
        {
            walk.early.push(GuardTerm::Condition {
                expr: &if_stmt.cond,
                negated: true,
            });
        }
    }
}

/// Whether `stmt` is a `return`, or a block whose last statement is one.
fn ends_in_return(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return(_) => true,
        StmtKind::Block(stmts) => stmts.last().is_some_and(ends_in_return),
        _ => false,
    }
}

struct Declarations<'a, 'f, F> {
    in_expressions: bool,
    in_bodies: bool,
    ctx: NameContext,
    conditional: bool,
    /// The negated conditions of the early returns passed so far.
    early: Vec<GuardTerm<'a>>,
    /// The terms of the branches and bodies being walked.
    guards: Vec<GuardTerm<'a>>,
    f: &'f mut F,
}

impl<'a, F: FnMut(Declaration<'a>, &NameContext, &[GuardTerm<'a>])> Declarations<'a, '_, F> {
    fn found(&mut self, declared: Declared<'a>, span: Span) {
        let declaration = Declaration {
            declared,
            span,
            conditional: self.conditional,
        };
        if self.conditional && !self.early.is_empty() {
            let guards: Vec<GuardTerm<'a>> =
                self.early.iter().chain(&self.guards).copied().collect();
            (self.f)(declaration, &self.ctx, &guards);
        } else {
            (self.f)(declaration, &self.ctx, &self.guards);
        }
    }

    /// Walks with every declaration found conditional, on what `walk`
    /// does not say.
    fn conditionally(&mut self, walk: impl FnOnce(&mut Self)) {
        self.guarded(GuardTerm::Opaque, walk);
    }

    /// Walks with every declaration found conditional, under `term`.
    fn guarded(&mut self, term: GuardTerm<'a>, walk: impl FnOnce(&mut Self)) {
        self.guards.push(term);
        let was = std::mem::replace(&mut self.conditional, true);
        walk(self);
        self.conditional = was;
        self.guards.pop();
    }

    /// Walks the `if` chain: each branch's body under its own condition
    /// and the negation of every one before it.
    fn if_chain(&mut self, if_stmt: &'a IfStmt) {
        let was = std::mem::replace(&mut self.conditional, true);
        let mark = self.guards.len();
        let mut branches: Vec<(&'a Expr, &'a Stmt)> = vec![(&if_stmt.cond, &if_stmt.then_branch)];
        branches.extend(if_stmt.elseifs.iter().map(|e| (&e.cond, &*e.body)));
        for (expr, body) in branches {
            self.visit_expr(expr);
            self.guards.push(GuardTerm::Condition {
                expr,
                negated: false,
            });
            self.visit_stmt(body);
            self.guards.pop();
            // Every later branch runs only when this one's condition failed.
            self.guards.push(GuardTerm::Condition {
                expr,
                negated: true,
            });
        }
        if let Some(else_branch) = &if_stmt.else_branch {
            self.visit_stmt(else_branch);
        }
        self.guards.truncate(mark);
        self.conditional = was;
    }
}

impl<'a, F: FnMut(Declaration<'a>, &NameContext, &[GuardTerm<'a>])> Visitor<'a>
    for Declarations<'a, '_, F>
{
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        stack::guard(|| match &stmt.kind {
            StmtKind::Namespace(name) => self.ctx.set_namespace(name),
            StmtKind::Use(imports) => self.ctx.add_uses(imports),
            StmtKind::ClassDecl(c) => {
                self.found(Declared::Class(c), stmt.span);
                self.conditionally(|v| walk_class_decl(v, c));
            }
            StmtKind::FunctionDecl(func) => {
                self.found(Declared::Function(func), stmt.span);
                if self.in_bodies {
                    self.conditionally(|v| walk_function_decl(v, func));
                }
            }
            StmtKind::GlobalConst(consts) => {
                for c in consts {
                    self.found(Declared::Constant(c), stmt.span);
                }
                walk_stmt(self, stmt);
            }
            StmtKind::If(if_stmt) => self.if_chain(if_stmt),
            StmtKind::While { .. }
            | StmtKind::DoWhile { .. }
            | StmtKind::For { .. }
            | StmtKind::Foreach { .. }
            | StmtKind::Switch { .. }
            | StmtKind::Try { .. } => self.conditionally(|v| walk_stmt(v, stmt)),
            _ => walk_stmt(self, stmt),
        })
    }

    fn visit_function_decl(&mut self, function: &'a FunctionDecl) {
        if self.in_bodies {
            walk_function_decl(self, function);
        }
    }

    fn visit_expr(&mut self, e: &'a Expr) {
        if !self.in_expressions {
            return;
        }
        if let Some((name, value)) = define_call(e) {
            self.found(Declared::Define { name, value }, e.span);
        }
        self.conditionally(|v| walk_expr(v, e));
    }
}

/// `define('NAME', value)`'s name and value: a call to the global
/// `define` with a literal name and no spread or named argument.
fn define_call(e: &Expr) -> Option<(&PhpString, &Expr)> {
    let ExprKind::Call(callee, args) = &e.kind else {
        return None;
    };
    let ExprKind::Name(name) = &callee.kind else {
        return None;
    };
    let (primary, fallback) = name.candidates();
    if !fallback.unwrap_or(primary).eq_ignore_ascii_case("define") {
        return None;
    }
    if args.len() < 2 || args.iter().any(|a| a.spread || a.name.is_some()) {
        return None;
    }
    let ExprKind::Literal(Literal::String(const_name)) = &args[0].value.kind else {
        return None;
    };
    Some((const_name, &args[1].value))
}

#[cfg(test)]
mod tests;
