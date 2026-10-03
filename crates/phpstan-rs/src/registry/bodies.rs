//! Function and method bodies, kept with their file's AST rather than in
//! the registry: a declaration's provenance finds its body.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use syntax::ast::{FunctionDecl, MethodDecl, Stmt};
use syntax::span::Span;
use syntax::visit::{self, Visitor};

use crate::registry::{BodySummary, InitFlow, Provenance};

/// Where the body of a function or method is found from its provenance.
pub trait BodySource: Sync {
    /// The body of the function or method declared where `provenance`
    /// records, when its file is at hand.
    fn body(&self, provenance: &Provenance) -> Option<&Arc<Vec<Stmt>>>;

    /// What the passes read of that body.
    fn summary(&self, provenance: &Provenance) -> Option<&Arc<BodySummary>>;

    /// The flow of that body, when it is a method's.
    fn init_flow(&self, provenance: &Provenance) -> Option<&Arc<InitFlow>>;
}

/// No bodies at all.
pub struct NoBodies;

impl BodySource for NoBodies {
    fn body(&self, _: &Provenance) -> Option<&Arc<Vec<Stmt>>> {
        None
    }

    fn summary(&self, _: &Provenance) -> Option<&Arc<BodySummary>> {
        None
    }

    fn init_flow(&self, _: &Provenance) -> Option<&Arc<InitFlow>> {
        None
    }
}

/// One body: its statements, and what the passes read of them.
#[derive(Debug, Clone)]
struct Body {
    stmts: Arc<Vec<Stmt>>,
    summary: Arc<BodySummary>,
    /// A method's flow, which a call to it is followed through.
    flow: Option<Arc<InitFlow>>,
}

/// One file's function and method bodies, anonymous classes' included,
/// by the span of their name.
#[derive(Debug, Default, Clone)]
pub struct FileBodies(FxHashMap<Span, Body>);

impl FileBodies {
    pub fn of(stmts: &[Stmt]) -> FileBodies {
        struct Collect {
            bodies: FxHashMap<Span, Body>,
            // Whether the next function declaration visited is a method.
            method: bool,
        }
        impl<'ast> Visitor<'ast> for Collect {
            fn visit_method_decl(&mut self, method: &'ast MethodDecl) {
                self.method = true;
                visit::walk_method_decl(self, method);
            }

            fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
                let is_method = std::mem::take(&mut self.method);
                if let Some(body) = &function.body {
                    self.bodies.insert(
                        function.name.span,
                        Body {
                            stmts: body.stmts.clone(),
                            summary: Arc::new(BodySummary::of(&body.stmts)),
                            flow: is_method.then(|| Arc::new(InitFlow::of(&body.stmts))),
                        },
                    );
                }
                visit::walk_function_decl(self, function);
            }
        }
        let mut collect = Collect {
            bodies: FxHashMap::default(),
            method: false,
        };
        visit::walk_stmts(&mut collect, stmts);
        FileBodies(collect.bodies)
    }

    /// The body of the declaration whose name is at `span`.
    pub fn get(&self, span: Span) -> Option<&Arc<Vec<Stmt>>> {
        self.0.get(&span).map(|body| &body.stmts)
    }

    /// What the passes read of the body of the declaration whose name is at
    /// `span`.
    pub fn summary(&self, span: Span) -> Option<&Arc<BodySummary>> {
        self.0.get(&span).map(|body| &body.summary)
    }

    /// The flow of the method whose name is at `span`.
    pub fn init_flow(&self, span: Span) -> Option<&Arc<InitFlow>> {
        self.0.get(&span)?.flow.as_ref()
    }
}

/// A single file's bodies answer for any declaration, whatever file it
/// records: a registry built from one program.
impl BodySource for FileBodies {
    fn body(&self, provenance: &Provenance) -> Option<&Arc<Vec<Stmt>>> {
        self.get(provenance.declaration_span())
    }

    fn summary(&self, provenance: &Provenance) -> Option<&Arc<BodySummary>> {
        self.summary(provenance.declaration_span())
    }

    fn init_flow(&self, provenance: &Provenance) -> Option<&Arc<InitFlow>> {
        self.init_flow(provenance.declaration_span())
    }
}
