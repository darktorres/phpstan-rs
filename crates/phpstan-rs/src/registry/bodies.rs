//! What is kept of function and method bodies, and of property hooks', per
//! file rather than in the registry: a declaration's provenance finds its
//! body's summary and flow.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use syntax::ast::{
    ExprId, ExprKind, FunctionDecl, NodeArenas, Param, PropertyDecl, PropertyHooks, StmtId,
    StmtKind,
};
use syntax::name::Name;
use syntax::span::Span;
use syntax::visit::arena::{self, Visitor as ArenaVisitor};

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

    /// Adds to `out` the properties every file at hand unsets.
    fn unset_properties(&self, out: &mut UnsetProperties);
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

    fn unset_properties(&self, _: &mut UnsetProperties) {}
}

/// The properties code unsets (`unset($o->p)`): by name, and whether it
/// unsets one whose name it computes, which may be any. A declared
/// property, once unset, is accessed through the object's magic methods.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UnsetProperties {
    // Sorted, each once.
    names: Vec<Name>,
    computed: bool,
}

impl UnsetProperties {
    /// The properties the statements `stmts` unset, those of the closures,
    /// the functions and the classes they declare included.
    fn of(arenas: &NodeArenas, stmts: &[StmtId]) -> UnsetProperties {
        struct Unsets(UnsetProperties);
        impl ArenaVisitor for Unsets {
            fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
                let Some(stmt) = arenas.stmt(id) else {
                    return;
                };
                if let StmtKind::Unset(targets) = &stmt.kind {
                    for target in arenas.expr_ids(targets) {
                        let Some(target) = arenas.peeled(*target).and_then(|id| arenas.expr(id))
                        else {
                            continue;
                        };
                        match &target.kind {
                            ExprKind::PropertyAccess(_, prop, _) => {
                                self.0.names.push(prop.name.clone());
                            }
                            ExprKind::DynamicPropertyAccess(..) => self.0.computed = true,
                            _ => {}
                        }
                    }
                }
                arena::walk_stmt(self, arenas, id);
            }
        }
        let mut walk = Unsets(UnsetProperties::default());
        arena::walk_stmts(&mut walk, arenas, stmts);
        let mut unset = walk.0;
        unset.settle();
        unset
    }

    /// Adds the properties `other` holds.
    pub(crate) fn add(&mut self, other: &UnsetProperties) {
        self.names.extend(other.names.iter().cloned());
        self.computed |= other.computed;
    }

    /// Sorts the names and keeps each once.
    pub(crate) fn settle(&mut self) {
        self.names
            .sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
        self.names.dedup();
    }

    /// Whether a property named `property` may be unset (of a settled set).
    pub(crate) fn may_unset(&self, property: &str) -> bool {
        self.computed
            || self
                .names
                .binary_search_by(|name| name.as_str().cmp(property))
                .is_ok()
    }
}

/// The properties code unsets, by name and then whether one with a computed
/// name.
impl syntax::canonical::Canonical for UnsetProperties {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let UnsetProperties { names, computed } = self;
        p.node(
            "UnsetProperties",
            &[("names", names), ("computed", computed)],
        );
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
/// the span of their name, its properties' hooks, by the span of the
/// property's name, and the properties it unsets.
#[derive(Debug, Default, Clone)]
pub struct FileBodies {
    bodies: FxHashMap<Span, Body>,
    hooks: FxHashMap<Span, Hooks>,
    unset: UnsetProperties,
}

impl FileBodies {
    pub fn of(arenas: &NodeArenas, stmts: &[StmtId]) -> FileBodies {
        FileBodies::collect(arenas, stmts, true, true)
    }

    /// The bodies of a file whose facts are known: its expressions are
    /// walked only when a body can be in one, in an anonymous class or a
    /// closure, and the whole file for what it unsets only when it unsets
    /// a property.
    pub fn of_file(arenas: &NodeArenas, stmts: &[StmtId], facts: &syntax::FileFacts) -> FileBodies {
        FileBodies::collect(
            arenas,
            stmts,
            facts.has_anonymous_class || facts.has_nested_declaration,
            facts.unsets_property,
        )
    }

    fn collect(
        arenas: &NodeArenas,
        stmts: &[StmtId],
        in_expressions: bool,
        unsets: bool,
    ) -> FileBodies {
        struct Collect {
            bodies: FxHashMap<Span, Body>,
            hooks: FxHashMap<Span, Hooks>,
            // Whether the next function declaration visited is a method.
            method: bool,
            in_expressions: bool,
        }
        impl Collect {
            fn hooks(
                &mut self,
                arenas: &NodeArenas,
                name: &syntax::ast::Ident,
                hooks: &PropertyHooks,
            ) {
                let get = hooks
                    .get
                    .as_ref()
                    .and_then(|hook| BodySummary::of_hook(arenas, hook, &name.name, false));
                let set = hooks.set.as_ref().and_then(|hook| {
                    let summary = BodySummary::of_hook(arenas, hook, &name.name, true)?;
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
        impl ArenaVisitor for Collect {
            fn visit_expr(&mut self, arenas: &NodeArenas, expr: ExprId) {
                if self.in_expressions {
                    arena::walk_expr(self, arenas, expr);
                }
            }

            fn visit_method_decl(&mut self, arenas: &NodeArenas, method: &syntax::ast::MethodDecl) {
                self.method = true;
                arena::walk_method_decl(self, arenas, method);
            }

            fn visit_property_decl(&mut self, arenas: &NodeArenas, property: &PropertyDecl) {
                if let Some(hooks) = &property.hooks {
                    self.hooks(arenas, &property.name, hooks);
                }
                arena::walk_property_decl(self, arenas, property);
            }

            fn visit_param(&mut self, arenas: &NodeArenas, param: &Param) {
                if let Some(hooks) = &param.hooks
                    && param.promotion.is_some()
                {
                    self.hooks(arenas, &param.name, hooks);
                }
                arena::walk_param(self, arenas, param);
            }

            fn visit_function_decl(&mut self, arenas: &NodeArenas, function: &FunctionDecl) {
                let is_method = std::mem::take(&mut self.method);
                if let Some(body) = &function.body {
                    self.bodies.insert(
                        function.name.span,
                        Body {
                            summary: Arc::new(BodySummary::of_function(
                                arenas,
                                arenas.params(function.params.clone()),
                                arenas.stmt_ids(&body.stmts),
                            )),
                            flow: is_method.then(|| {
                                Arc::new(InitFlow::of(
                                    arenas,
                                    arenas.stmt_ids(&body.stmts),
                                    &body.goto_labels,
                                ))
                            }),
                        },
                    );
                }
                arena::walk_function_decl(self, arenas, function);
            }
        }
        let mut collect = Collect {
            bodies: FxHashMap::default(),
            hooks: FxHashMap::default(),
            method: false,
            in_expressions,
        };
        arena::walk_stmts(&mut collect, arenas, stmts);
        FileBodies {
            bodies: collect.bodies,
            hooks: collect.hooks,
            unset: if unsets {
                UnsetProperties::of(arenas, stmts)
            } else {
                UnsetProperties::default()
            },
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
/// order, then the properties it unsets.
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
        serde::Serialize::serialize(&(bodies, hooks, &self.unset), serializer)
    }
}

impl<'de> serde::Deserialize<'de> for FileBodies {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<FileBodies, D::Error> {
        type Stored = (
            Vec<(Span, Arc<BodySummary>, Option<Arc<InitFlow>>)>,
            Vec<(Span, Hooks)>,
            UnsetProperties,
        );
        let (bodies, hooks, unset): Stored = serde::Deserialize::deserialize(deserializer)?;
        Ok(FileBodies {
            bodies: bodies
                .into_iter()
                .map(|(span, summary, flow)| (span, Body { summary, flow }))
                .collect(),
            hooks: hooks.into_iter().collect(),
            unset,
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

    fn unset_properties(&self, out: &mut UnsetProperties) {
        out.add(&self.unset);
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
/// file, then the hooks by the span of the property's name, then the
/// properties the file unsets.
impl syntax::canonical::Canonical for FileBodies {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let FileBodies {
            bodies,
            hooks,
            unset,
        } = self;
        let mut bodies: Vec<(&Span, &Body)> = bodies.iter().collect();
        bodies.sort_unstable_by_key(|(span, _)| (span.start, span.end));
        let mut hooks: Vec<(&Span, &Hooks)> = hooks.iter().collect();
        hooks.sort_unstable_by_key(|(span, _)| (span.start, span.end));
        p.node(
            "FileBodies",
            &[("bodies", &bodies), ("hooks", &hooks), ("unset", unset)],
        );
    }
}
