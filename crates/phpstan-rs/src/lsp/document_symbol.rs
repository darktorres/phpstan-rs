//! `textDocument/documentSymbol` - a per-file hierarchical outline, built
//! straight from `Project::ast`'s own top-level declarations (their
//! name spans and whole-declaration `span` fields already carry everything
//! needed), keeping the per-file nesting a document outline needs.

use lsp_types::{DocumentSymbol, SymbolKind};

use syntax::ast::{ClassDecl, ClassKind, FunctionDecl, MethodDecl, NodeArenas, StmtId, StmtKind};
use syntax::span::Span;

use super::protocol::Positions;

pub(super) fn document_symbols(
    arenas: &NodeArenas,
    stmts: &[StmtId],
    index: Positions<'_>,
) -> Vec<DocumentSymbol> {
    syntax::stack::guard(|| {
        let mut symbols = Vec::new();
        for id in stmts {
            let Some(stmt) = arenas.stmt(id.clone()) else {
                continue;
            };
            match &stmt.kind {
                StmtKind::FunctionDecl(id) => {
                    if let Some(f) = arenas.function(id.clone()) {
                        symbols.push(function_symbol(f, SymbolKind::FUNCTION, index));
                    }
                }
                StmtKind::ClassDecl(c) => {
                    symbols.push(class_symbol(arenas, arenas.class_decl(c.clone()), index))
                }
                StmtKind::Namespace(_) | StmtKind::Use(_) => {}
                // A conditional top-level declaration (`if (...) { class Foo {} }`)
                // still gets an outline entry.
                StmtKind::If(if_stmt) => {
                    for body in if_stmt.bodies(arenas) {
                        symbols.extend(document_symbols(
                            arenas,
                            std::slice::from_ref(&body),
                            index,
                        ));
                    }
                }
                StmtKind::Block(inner) => {
                    symbols.extend(document_symbols(arenas, arenas.stmt_ids(inner), index))
                }
                StmtKind::Declare {
                    body: Some(inner), ..
                } => symbols.extend(document_symbols(arenas, arenas.stmt_ids(inner), index)),
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

fn class_symbol(arenas: &NodeArenas, c: &ClassDecl, index: Positions<'_>) -> DocumentSymbol {
    let def = arenas
        .class(c.def)
        .expect("class definition ID belongs to its syntax tree");
    let kind = class_kind_symbol_kind(def.kind);
    let mut children: Vec<DocumentSymbol> = def
        .consts(arenas)
        .iter()
        .map(|k| const_symbol(k, index))
        .collect();
    children.extend(def.enum_cases(arenas).iter().map(|case| {
        symbol(
            case.name.to_string(),
            SymbolKind::ENUM_MEMBER,
            case.span,
            case.name.span,
            Vec::new(),
            index,
        )
    }));
    children.extend(def.properties(arenas).iter().map(|p| {
        symbol(
            p.name.to_string(),
            SymbolKind::PROPERTY,
            p.span,
            p.name.span,
            Vec::new(),
            index,
        )
    }));
    children.extend(
        arenas
            .methods(def.methods.clone())
            .iter()
            .map(|m| method_symbol(m, index)),
    );
    symbol(
        c.name.to_string(),
        kind,
        def.span,
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
