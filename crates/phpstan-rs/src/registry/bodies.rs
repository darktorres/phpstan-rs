//! What is kept of function and method bodies, per file rather than in the
//! registry: a declaration's provenance finds its body's summary and flow.
//! The statements themselves are kept only in a debug build, which checks
//! the summaries against them.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use syntax::ast::{FunctionDecl, MethodDecl, Stmt};
use syntax::span::Span;
use syntax::visit::{self, Visitor};

use crate::registry::{BodySummary, InitFlow, Provenance};

/// Where the body of a function or method is found from its provenance.
pub trait BodySource: Sync {
    /// The statements of the body of the function or method declared where
    /// `provenance` records, when its file is at hand.
    #[cfg(debug_assertions)]
    fn body(&self, provenance: &Provenance) -> Option<&Arc<Vec<Stmt>>>;

    /// What the passes read of that body.
    fn summary(&self, provenance: &Provenance) -> Option<&Arc<BodySummary>>;

    /// The flow of that body, when it is a method's.
    fn init_flow(&self, provenance: &Provenance) -> Option<&Arc<InitFlow>>;
}

/// No bodies at all.
pub struct NoBodies;

impl BodySource for NoBodies {
    #[cfg(debug_assertions)]
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

/// One body: what the passes read of its statements.
#[derive(Debug, Clone)]
struct Body {
    #[cfg(debug_assertions)]
    stmts: Option<Arc<Vec<Stmt>>>,
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
                            #[cfg(debug_assertions)]
                            stmts: Some(body.stmts.clone()),
                            summary: Arc::new(BodySummary::of(&body.stmts)),
                            flow: is_method
                                .then(|| Arc::new(InitFlow::of(&body.stmts, &body.goto_labels))),
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

    /// The statements of the body of the declaration whose name is at `span`.
    #[cfg(debug_assertions)]
    pub fn get(&self, span: Span) -> Option<&Arc<Vec<Stmt>>> {
        self.0.get(&span)?.stmts.as_ref()
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

/// A file's bodies as stored: by the span of the declaration's name, in
/// that order, without the statements.
impl serde::Serialize for FileBodies {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut bodies: Vec<(&Span, &Body)> = self.0.iter().collect();
        bodies.sort_by_key(|(span, _)| (span.start, span.end));
        serializer.collect_seq(
            bodies
                .into_iter()
                .map(|(span, body)| (span, &body.summary, &body.flow)),
        )
    }
}

impl<'de> serde::Deserialize<'de> for FileBodies {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<FileBodies, D::Error> {
        let bodies: Vec<(Span, Arc<BodySummary>, Option<Arc<InitFlow>>)> =
            serde::Deserialize::deserialize(deserializer)?;
        Ok(FileBodies(
            bodies
                .into_iter()
                .map(|(span, summary, flow)| {
                    (
                        span,
                        Body {
                            #[cfg(debug_assertions)]
                            stmts: None,
                            summary,
                            flow,
                        },
                    )
                })
                .collect(),
        ))
    }
}

/// A single file's bodies answer for any declaration, whatever file it
/// records: a registry built from one program.
impl BodySource for FileBodies {
    #[cfg(debug_assertions)]
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
