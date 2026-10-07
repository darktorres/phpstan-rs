//! `textDocument/documentSymbol` - a per-file hierarchical outline, built
//! straight from `Project::ast`'s own top-level declarations (their
//! name spans and whole-declaration `span` fields already carry everything
//! needed), keeping the per-file nesting a document outline needs.

use lsp_types::{DocumentSymbol, SymbolKind};

use syntax::ast::{ClassDecl, ClassKind, FunctionDecl, MethodDecl, StmtKind, StmtNode};
use syntax::span::Span;

use super::protocol::Positions;

pub(super) fn document_symbols(stmts: &[StmtNode], index: Positions<'_>) -> Vec<DocumentSymbol> {
    syntax::stack::guard(|| {
        let mut symbols = Vec::new();
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::FunctionDecl(f) => {
                    symbols.push(function_symbol(f, SymbolKind::FUNCTION, index))
                }
                StmtKind::ClassDecl(c) => symbols.push(class_symbol(c, index)),
                StmtKind::Namespace(_) | StmtKind::Use(_) => {}
                // A conditional top-level declaration (`if (...) { class Foo {} }`)
                // still gets an outline entry.
                StmtKind::If(if_stmt) => {
                    for body in if_stmt.bodies() {
                        symbols.extend(document_symbols(std::slice::from_ref(body), index));
                    }
                }
                StmtKind::Block(inner)
                | StmtKind::Declare {
                    body: Some(inner), ..
                } => symbols.extend(document_symbols(inner, index)),
                _ => {}
            }
        }
        symbols
    })
}

#[allow(deprecated)] // `DocumentSymbol::deprecated`: no replacement field exists yet
fn symbol(
    name: String,
    kind: SymbolKind,
    range: Span,
    selection_range: Span,
    children: Vec<DocumentSymbol>,
    index: Positions<'_>,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range: index.range(range),
        selection_range: index.range(selection_range),
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

fn function_symbol(f: &FunctionDecl, kind: SymbolKind, index: Positions<'_>) -> DocumentSymbol {
    symbol(
        f.name.to_string(),
        kind,
        f.span,
        f.name.span,
        Vec::new(),
        index,
    )
}

fn method_symbol(m: &MethodDecl, index: Positions<'_>) -> DocumentSymbol {
    symbol(
        m.function.name.to_string(),
        SymbolKind::METHOD,
        m.span,
        m.function.name.span,
        Vec::new(),
        index,
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

fn class_symbol(c: &ClassDecl, index: Positions<'_>) -> DocumentSymbol {
    let kind = class_kind_symbol_kind(c.def.kind);
    let mut children: Vec<DocumentSymbol> = c
        .def
        .consts
        .iter()
        .map(|k| const_symbol(k, index))
        .collect();
    children.extend(c.def.enum_cases.iter().map(|case| {
        symbol(
            case.name.to_string(),
            SymbolKind::ENUM_MEMBER,
            case.span,
            case.name.span,
            Vec::new(),
            index,
        )
    }));
    children.extend(c.def.properties.iter().map(|p| {
        symbol(
            p.name.to_string(),
            SymbolKind::PROPERTY,
            p.span,
            p.name.span,
            Vec::new(),
            index,
        )
    }));
    children.extend(c.def.methods.iter().map(|m| method_symbol(m, index)));
    symbol(
        c.name.to_string(),
        kind,
        c.def.span,
        c.name.span,
        children,
        index,
    )
}

fn const_symbol(cst: &syntax::ast::ClassConst, index: Positions<'_>) -> DocumentSymbol {
    symbol(
        cst.name.to_string(),
        SymbolKind::CONSTANT,
        cst.name.span,
        cst.name.span,
        Vec::new(),
        index,
    )
}

#[cfg(test)]
mod tests;
