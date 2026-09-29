//! The classes, functions and constants a file declares, wherever they
//! sit.

use crate::ast::*;
use crate::name_resolution::NameContext;
use crate::php_string::PhpString;
use crate::token::Span;
use crate::visit::{
    Visitor, walk_class_decl, walk_expr, walk_function_decl, walk_stmt, walk_stmts,
};

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

/// Calls `f` with every declaration in `stmts`, in source order, with the
/// name context in effect where it sits: namespaces and imports aren't
/// scoped to blocks or functions.
pub fn for_each_declaration<'a>(
    stmts: &'a [Stmt],
    mut f: impl FnMut(Declaration<'a>, &NameContext),
) {
    let mut walk = Declarations {
        ctx: NameContext::default(),
        conditional: false,
        f: &mut f,
    };
    walk_stmts(&mut walk, stmts);
}

struct Declarations<'f, F> {
    ctx: NameContext,
    conditional: bool,
    f: &'f mut F,
}

impl<'a, F: FnMut(Declaration<'a>, &NameContext)> Declarations<'_, F> {
    fn found(&mut self, declared: Declared<'a>, span: Span) {
        let declaration = Declaration {
            declared,
            span,
            conditional: self.conditional,
        };
        (self.f)(declaration, &self.ctx);
    }

    /// Walks with every declaration found conditional.
    fn conditionally(&mut self, walk: impl FnOnce(&mut Self)) {
        let was = std::mem::replace(&mut self.conditional, true);
        walk(self);
        self.conditional = was;
    }
}

impl<'a, F: FnMut(Declaration<'a>, &NameContext)> Visitor<'a> for Declarations<'_, F> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        crate::stack::guard(|| match &stmt.kind {
            StmtKind::Namespace(name) => self.ctx.set_namespace(name),
            StmtKind::Use(imports) => self.ctx.add_uses(imports),
            StmtKind::ClassDecl(c) => {
                self.found(Declared::Class(c), stmt.span);
                self.conditionally(|v| walk_class_decl(v, c));
            }
            StmtKind::FunctionDecl(func) => {
                self.found(Declared::Function(func), stmt.span);
                self.conditionally(|v| walk_function_decl(v, func));
            }
            StmtKind::GlobalConst(consts) => {
                for c in consts {
                    self.found(Declared::Constant(c), stmt.span);
                }
                walk_stmt(self, stmt);
            }
            StmtKind::If(_)
            | StmtKind::While { .. }
            | StmtKind::DoWhile { .. }
            | StmtKind::For { .. }
            | StmtKind::Foreach { .. }
            | StmtKind::Switch { .. }
            | StmtKind::Try { .. } => self.conditionally(|v| walk_stmt(v, stmt)),
            _ => walk_stmt(self, stmt),
        })
    }

    fn visit_expr(&mut self, e: &'a Expr) {
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
