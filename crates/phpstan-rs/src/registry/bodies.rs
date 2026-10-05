//! What is kept of function and method bodies, and of property hooks', per
//! file rather than in the registry: a declaration's provenance finds its
//! body's summary and flow.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use syntax::ast::{FunctionDecl, MethodDecl, Param, PropertyDecl, PropertyHooks, Stmt};
use syntax::name::Name;
use syntax::span::Span;
use syntax::visit::{self, Visitor};

use crate::registry::{BodySummary, InitFlow, Provenance};

/// Where the body of a function or method is found from its provenance.
pub trait BodySource: Sync {
    /// What the passes read of the body of the function or method declared
    /// where `provenance` records, when its file is at hand.
    fn summary(&self, provenance: &Provenance) -> Option<&Arc<BodySummary>>;

    /// The flow of that body, when it is a method's.
    fn init_flow(&self, provenance: &Provenance) -> Option<&Arc<InitFlow>>;

    /// What the passes read of the body of the `get` hook, or the `set`
    /// hook when `set`, of the property declared where `provenance`
    /// records, when its file is at hand; with a `set` hook's, the name of
    /// the parameter the value is passed in.
    fn hook_summary(&self, provenance: &Provenance, set: bool) -> Option<HookBody<'_>>;
}

/// What the passes read of a hook's body, and the name of the parameter a
/// `set` hook is passed the value in.
pub type HookBody<'a> = (&'a Arc<BodySummary>, Option<&'a Name>);

/// No bodies at all.
pub struct NoBodies;

impl BodySource for NoBodies {
    fn summary(&self, _: &Provenance) -> Option<&Arc<BodySummary>> {
        None
    }

    fn init_flow(&self, _: &Provenance) -> Option<&Arc<InitFlow>> {
        None
    }

    fn hook_summary(&self, _: &Provenance, _: bool) -> Option<HookBody<'_>> {
        None
    }
}

/// One body: what the passes read of its statements.
#[derive(Debug, Clone)]
struct Body {
    summary: Arc<BodySummary>,
    /// A method's flow, which a call to it is followed through.
    flow: Option<Arc<InitFlow>>,
}

/// A property's hooks with a body: its `get` hook's, then its `set` hook's
/// with the name of the parameter the value is passed in.
type Hooks = (Option<Arc<BodySummary>>, Option<(Arc<BodySummary>, Name)>);

/// One file's function and method bodies, anonymous classes' included, by
/// the span of their name, and its properties' hooks, by the span of the
/// property's name.
#[derive(Debug, Default, Clone)]
pub struct FileBodies {
    bodies: FxHashMap<Span, Body>,
    hooks: FxHashMap<Span, Hooks>,
}

impl FileBodies {
    pub fn of(stmts: &[Stmt]) -> FileBodies {
        FileBodies::collect(stmts, true)
    }

    /// The bodies of a file whose facts are known: its expressions are
    /// walked only when a body can be in one, in an anonymous class or a
    /// closure.
    pub fn of_file(stmts: &[Stmt], facts: &syntax::FileFacts) -> FileBodies {
        FileBodies::collect(
            stmts,
            facts.has_anonymous_class || facts.has_nested_declaration,
        )
    }

    fn collect(stmts: &[Stmt], in_expressions: bool) -> FileBodies {
        struct Collect {
            bodies: FxHashMap<Span, Body>,
            hooks: FxHashMap<Span, Hooks>,
            // Whether the next function declaration visited is a method.
            method: bool,
            in_expressions: bool,
        }
        impl Collect {
            fn hooks(&mut self, name: &syntax::ast::Ident, hooks: &PropertyHooks) {
                let get = hooks
                    .get
                    .as_ref()
                    .and_then(|hook| BodySummary::of_hook(hook, &name.name, false));
                let set = hooks.set.as_ref().and_then(|hook| {
                    let summary = BodySummary::of_hook(hook, &name.name, true)?;
                    let value = hook
                        .param
                        .as_ref()
                        .map_or_else(|| Name::from("value"), |param| param.name.name.clone());
                    Some((Arc::new(summary), value))
                });
                let found = (get.map(Arc::new), set);
                if found.0.is_some() || found.1.is_some() {
                    self.hooks.insert(name.span, found);
                }
            }
        }
        impl<'ast> Visitor<'ast> for Collect {
            fn visit_expr(&mut self, expr: &'ast syntax::ast::Expr) {
                if self.in_expressions {
                    visit::walk_expr(self, expr);
                }
            }

            fn visit_method_decl(&mut self, method: &'ast MethodDecl) {
                self.method = true;
                visit::walk_method_decl(self, method);
            }

            fn visit_property_decl(&mut self, property: &'ast PropertyDecl) {
                if let Some(hooks) = &property.hooks {
                    self.hooks(&property.name, hooks);
                }
                visit::walk_property_decl(self, property);
            }

            fn visit_param(&mut self, param: &'ast Param) {
                if let Some(hooks) = &param.hooks
                    && param.promotion.is_some()
                {
                    self.hooks(&param.name, hooks);
                }
                visit::walk_param(self, param);
            }

            fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
                let is_method = std::mem::take(&mut self.method);
                if let Some(body) = &function.body {
                    self.bodies.insert(
                        function.name.span,
                        Body {
                            summary: Arc::new(BodySummary::of_function(
                                &function.params,
                                &body.stmts,
                            )),
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
            hooks: FxHashMap::default(),
            method: false,
            in_expressions,
        };
        visit::walk_stmts(&mut collect, stmts);
        FileBodies {
            bodies: collect.bodies,
            hooks: collect.hooks,
        }
    }

    /// What the passes read of the body of the declaration whose name is at
    /// `span`.
    pub fn summary(&self, span: Span) -> Option<&Arc<BodySummary>> {
        self.bodies.get(&span).map(|body| &body.summary)
    }

    /// The flow of the method whose name is at `span`.
    pub fn init_flow(&self, span: Span) -> Option<&Arc<InitFlow>> {
        self.bodies.get(&span)?.flow.as_ref()
    }

    /// What the passes read of the body of the `get` hook, or the `set`
    /// hook when `set`, of the property whose name is at `span`; with a
    /// `set` hook's, the name of the parameter the value is passed in.
    pub fn hook_summary(&self, span: Span, set: bool) -> Option<HookBody<'_>> {
        let (get, set_hook) = self.hooks.get(&span)?;
        if set {
            set_hook
                .as_ref()
                .map(|(summary, value)| (summary, Some(value)))
        } else {
            get.as_ref().map(|summary| (summary, None))
        }
    }
}

/// A file's bodies as stored: by the span of the declaration's name, in
/// that order, then the hooks by the span of the property's name, in that
/// order.
impl serde::Serialize for FileBodies {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut bodies: Vec<(&Span, &Body)> = self.bodies.iter().collect();
        bodies.sort_by_key(|(span, _)| (span.start, span.end));
        let mut hooks: Vec<(&Span, &Hooks)> = self.hooks.iter().collect();
        hooks.sort_by_key(|(span, _)| (span.start, span.end));
        let bodies: Vec<_> = bodies
            .into_iter()
            .map(|(span, body)| (span, &body.summary, &body.flow))
            .collect();
        serde::Serialize::serialize(&(bodies, hooks), serializer)
    }
}

impl<'de> serde::Deserialize<'de> for FileBodies {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<FileBodies, D::Error> {
        type Stored = (
            Vec<(Span, Arc<BodySummary>, Option<Arc<InitFlow>>)>,
            Vec<(Span, Hooks)>,
        );
        let (bodies, hooks): Stored = serde::Deserialize::deserialize(deserializer)?;
        Ok(FileBodies {
            bodies: bodies
                .into_iter()
                .map(|(span, summary, flow)| (span, Body { summary, flow }))
                .collect(),
            hooks: hooks.into_iter().collect(),
        })
    }
}

/// A single file's bodies answer for any declaration, whatever file it
/// records: a registry built from one program.
impl BodySource for FileBodies {
    fn summary(&self, provenance: &Provenance) -> Option<&Arc<BodySummary>> {
        self.summary(provenance.declaration_span())
    }

    fn init_flow(&self, provenance: &Provenance) -> Option<&Arc<InitFlow>> {
        self.init_flow(provenance.declaration_span())
    }

    fn hook_summary(&self, provenance: &Provenance, set: bool) -> Option<HookBody<'_>> {
        self.hook_summary(provenance.declaration_span(), set)
    }
}

/// A body's summary and flow.
impl syntax::canonical::Canonical for Body {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let Body { summary, flow } = self;
        p.node("Body", &[("summary", summary), ("flow", flow)]);
    }
}

/// By the span of the declaration each body is of, in the order of the
/// file, then the hooks by the span of the property's name.
impl syntax::canonical::Canonical for FileBodies {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let FileBodies { bodies, hooks } = self;
        let mut bodies: Vec<(&Span, &Body)> = bodies.iter().collect();
        bodies.sort_unstable_by_key(|(span, _)| (span.start, span.end));
        let mut hooks: Vec<(&Span, &Hooks)> = hooks.iter().collect();
        hooks.sort_unstable_by_key(|(span, _)| (span.start, span.end));
        p.node("FileBodies", &[("bodies", &bodies), ("hooks", &hooks)]);
    }
}
