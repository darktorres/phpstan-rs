//! `textDocument/documentSymbol` - a per-file hierarchical outline, built
//! straight from `Project::ast`'s own top-level declarations (their
//! `name_span`/whole-declaration `span` fields already carry everything
//! needed) rather than `DeclLocations`, which is flattened project-wide
//! by FQN and would lose the per-file nesting a document outline needs.

use crate::registry::class_decl_name;
use lsp_types::{DocumentSymbol, SymbolKind};

use syntax::ast::{ClassDecl, ClassKind, EnumDecl, FunctionDecl, MethodDecl, Stmt, StmtKind};
use syntax::token::Span;

use super::protocol::to_lsp_range;

pub(super) fn document_symbols(stmts: &[Stmt]) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::FunctionDecl(f) => symbols.push(function_symbol(f, SymbolKind::FUNCTION)),
            StmtKind::ClassDecl(c) => symbols.push(class_symbol(c)),
            StmtKind::EnumDecl(e) => symbols.push(enum_symbol(e)),
            StmtKind::Namespace(_) | StmtKind::Use(_) => {}
            // A conditional top-level declaration (`if (...) { class Foo {} }`)
            // still gets an outline entry - mirrors `DeclLocations::index`'s
            // own recursion into `If`/`Block` for the identical reason.
            StmtKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                symbols.extend(document_symbols(std::slice::from_ref(then_branch)));
                if let Some(else_branch) = else_branch {
                    symbols.extend(document_symbols(std::slice::from_ref(else_branch)));
                }
            }
            StmtKind::Block(inner) => symbols.extend(document_symbols(inner)),
            _ => {}
        }
    }
    symbols
}

#[allow(deprecated)] // `DocumentSymbol::deprecated`: no replacement field exists yet
fn symbol(
    name: String,
    kind: SymbolKind,
    range: Span,
    selection_range: Span,
    children: Vec<DocumentSymbol>,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range: to_lsp_range(range),
        selection_range: to_lsp_range(selection_range),
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

fn function_symbol(f: &FunctionDecl, kind: SymbolKind) -> DocumentSymbol {
    symbol(f.name.to_string(), kind, f.span, f.name_span, Vec::new())
}

fn method_symbol(m: &MethodDecl) -> DocumentSymbol {
    symbol(
        m.function.name.to_string(),
        SymbolKind::METHOD,
        m.span,
        m.function.name_span,
        Vec::new(),
    )
}

/// Shared with `workspace_symbol::workspace_symbols`, which needs the
/// same mapping for a registry-sourced `ClassReflection.kind` (the AST-level
/// `ClassDecl.kind` this function itself uses is the same
/// `syntax::ast::ClassKind` type the registry reuses directly, not a
/// separate registry-local enum).
pub(super) fn class_kind_symbol_kind(kind: ClassKind) -> SymbolKind {
    match kind {
        ClassKind::Class | ClassKind::Trait => SymbolKind::CLASS,
        ClassKind::Interface => SymbolKind::INTERFACE,
        ClassKind::Enum => SymbolKind::ENUM,
    }
}

fn class_symbol(c: &ClassDecl) -> DocumentSymbol {
    let kind = class_kind_symbol_kind(c.kind);
    let mut children: Vec<DocumentSymbol> = c.consts.iter().map(const_symbol).collect();
    children.extend(c.properties.iter().map(|p| {
        symbol(
            p.name.to_string(),
            SymbolKind::PROPERTY,
            p.span,
            p.name_span,
            Vec::new(),
        )
    }));
    children.extend(c.methods.iter().map(method_symbol));
    symbol(
        class_decl_name(c).to_string(),
        kind,
        c.span,
        c.name_span,
        children,
    )
}

fn enum_symbol(e: &EnumDecl) -> DocumentSymbol {
    let mut children: Vec<DocumentSymbol> = e.consts.iter().map(const_symbol).collect();
    children.extend(e.cases.iter().map(|case| {
        symbol(
            case.name.to_string(),
            SymbolKind::ENUM_MEMBER,
            case.span,
            case.name_span,
            Vec::new(),
        )
    }));
    children.extend(e.methods.iter().map(method_symbol));
    symbol(
        e.name.to_string(),
        SymbolKind::ENUM,
        e.span,
        e.name_span,
        children,
    )
}

fn const_symbol(cst: &syntax::ast::ClassConst) -> DocumentSymbol {
    symbol(
        cst.name.to_string(),
        SymbolKind::CONSTANT,
        cst.name_span,
        cst.name_span,
        Vec::new(),
    )
}

#[cfg(test)]
mod tests;
