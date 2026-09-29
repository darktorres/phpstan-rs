//! Function and method bodies, kept with their file's AST rather than in
//! the registry: a declaration's provenance finds its body.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use syntax::ast::{FunctionDecl, Stmt};
use syntax::token::Span;
use syntax::visit::{self, Visitor};

use crate::registry::Provenance;

/// Where the body of a function or method is found from its provenance.
pub trait BodySource: Sync {
    /// The body of the function or method declared where `provenance`
    /// records, when its file is at hand.
    fn body(&self, provenance: &Provenance) -> Option<&Arc<Vec<Stmt>>>;
}

/// No bodies at all.
pub struct NoBodies;

impl BodySource for NoBodies {
    fn body(&self, _: &Provenance) -> Option<&Arc<Vec<Stmt>>> {
        None
    }
}

/// One file's function and method bodies, anonymous classes' included,
/// by the span of their name.
#[derive(Debug, Default, Clone)]
pub struct FileBodies(FxHashMap<Span, Arc<Vec<Stmt>>>);

impl FileBodies {
    pub fn of(stmts: &[Stmt]) -> FileBodies {
        struct Collect(FxHashMap<Span, Arc<Vec<Stmt>>>);
        impl<'ast> Visitor<'ast> for Collect {
            fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
                if let Some(body) = &function.body {
                    self.0.insert(function.name_span, body.stmts.clone());
                }
                visit::walk_function_decl(self, function);
            }
        }
        let mut collect = Collect(FxHashMap::default());
        visit::walk_stmts(&mut collect, stmts);
        FileBodies(collect.0)
    }

    /// The body of the declaration whose name is at `span`.
    pub fn get(&self, span: Span) -> Option<&Arc<Vec<Stmt>>> {
        self.0.get(&span)
    }
}

/// A single file's bodies answer for any declaration, whatever file it
/// records: a registry built from one program.
impl BodySource for FileBodies {
    fn body(&self, provenance: &Provenance) -> Option<&Arc<Vec<Stmt>>> {
        self.get(provenance.declaration_span())
    }
}
