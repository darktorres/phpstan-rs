//! What is kept of function and method bodies, and of property hooks', per
//! file rather than in the registry: a declaration's provenance finds its
//! body's summary and flow.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use syntax::ast::{
    ExprId, ExprKind, FunctionDecl, Literal, NodeArenas, Param, PropertyDecl, PropertyHooks,
    ResolvedName, StaticTarget, StmtId, StmtKind,
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

    /// Adds to `out` the classes every file at hand registers as stream
    /// wrappers.
    fn stream_wrappers(&self, out: &mut StreamWrappers);

    /// Whether any file at hand names something that writes a property
    /// without a write in its code (`MENTIONS_PROPERTY_WRITER`); a source
    /// that cannot say answers yes.
    fn names_property_writer(&self) -> bool;

    /// Every file at hand.
    fn files(&self) -> Vec<LoadedFile<'_>>;
}

/// A loaded file, as the passes that settle what code writes read it.
pub struct LoadedFile<'a> {
    pub(crate) id: crate::file_id::FileId,
    pub(crate) path: &'a std::path::Path,
    pub(crate) global_facts: &'a crate::registry::global_vars::GlobalFacts,
    /// What the file's top level, the code that runs when it is included,
    /// may write.
    pub(crate) top_level: &'a Arc<BodySummary>,
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

    fn stream_wrappers(&self, _: &mut StreamWrappers) {}

    fn names_property_writer(&self) -> bool {
        false
    }

    fn files(&self) -> Vec<LoadedFile<'_>> {
        Vec::new()
    }
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

/// The bit of a file's `FileFacts::mentions` that notes it names
/// `stream_wrapper_register`.
pub(crate) const MENTIONS_STREAM_WRAPPER: u32 = 1 << 4;

/// The spellings that set `MENTIONS_STREAM_WRAPPER`.
pub(crate) const WATCH: &[(&str, u32)] = &[("stream_wrapper_register", MENTIONS_STREAM_WRAPPER)];

/// The bit of a file's `FileFacts::mentions` that notes it names something
/// that writes a property without a write in the code: reflection, the
/// fetch modes that fill an object, unserialisation, or a wrapper that
/// exposes an object's properties as array elements. Names are also noted
/// when a string is one, as a callable is.
pub(crate) const MENTIONS_PROPERTY_WRITER: u32 = 1 << 7;

/// The spellings that set `MENTIONS_PROPERTY_WRITER`.
pub(crate) const PROPERTY_WRITER_WATCH: &[(&str, u32)] = &[
    ("setvalue", MENTIONS_PROPERTY_WRITER),
    (
        "setrawvaluewithoutlazyinitialization",
        MENTIONS_PROPERTY_WRITER,
    ),
    ("unserialize", MENTIONS_PROPERTY_WRITER),
    ("igbinary_unserialize", MENTIONS_PROPERTY_WRITER),
    ("msgpack_unserialize", MENTIONS_PROPERTY_WRITER),
    ("_unserialize", MENTIONS_PROPERTY_WRITER),
    ("fetchobject", MENTIONS_PROPERTY_WRITER),
    ("fetch_object", MENTIONS_PROPERTY_WRITER),
    ("mysqli_fetch_object", MENTIONS_PROPERTY_WRITER),
    ("cubrid_fetch_object", MENTIONS_PROPERTY_WRITER),
    ("db2_fetch_object", MENTIONS_PROPERTY_WRITER),
    ("fbird_fetch_object", MENTIONS_PROPERTY_WRITER),
    ("ibase_fetch_object", MENTIONS_PROPERTY_WRITER),
    ("mssql_fetch_object", MENTIONS_PROPERTY_WRITER),
    ("mysql_fetch_object", MENTIONS_PROPERTY_WRITER),
    ("oci_fetch_object", MENTIONS_PROPERTY_WRITER),
    ("odbc_fetch_object", MENTIONS_PROPERTY_WRITER),
    ("pg_fetch_object", MENTIONS_PROPERTY_WRITER),
    ("sqlite_fetch_object", MENTIONS_PROPERTY_WRITER),
    ("sqlsrv_fetch_object", MENTIONS_PROPERTY_WRITER),
    ("swoole_substr_unserialize", MENTIONS_PROPERTY_WRITER),
    ("sybase_fetch_object", MENTIONS_PROPERTY_WRITER),
    ("fetch_class", MENTIONS_PROPERTY_WRITER),
    ("fetch_into", MENTIONS_PROPERTY_WRITER),
    ("fetch_props_late", MENTIONS_PROPERTY_WRITER),
    ("setfetchmode", MENTIONS_PROPERTY_WRITER),
    ("arrayobject", MENTIONS_PROPERTY_WRITER),
    ("arrayiterator", MENTIONS_PROPERTY_WRITER),
    ("recursivearrayiterator", MENTIONS_PROPERTY_WRITER),
];

/// The classes code registers as stream wrappers with
/// `stream_wrapper_register()`: only such a class has its methods run by a
/// stream function.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StreamWrappers {
    // The registered classes, lowercased, sorted, each once.
    classes: Vec<Box<str>>,
    // The classes registered by their own name (`self::class`, `static::class`,
    // `__CLASS__`) in code a subclass inherits, lowercased, sorted, each once:
    // they and their subclasses.
    with_subclasses: Vec<Box<str>>,
    // A registration names a class not known.
    any: bool,
}

impl StreamWrappers {
    /// The classes the statements `stmts` register, those of the closures,
    /// the functions and the classes they declare included.
    fn of(arenas: &NodeArenas, stmts: &[StmtId]) -> StreamWrappers {
        struct Registrations {
            found: StreamWrappers,
            // The class each nested declaration is in, `None` for an
            // anonymous one.
            classes: Vec<Option<Box<str>>>,
        }
        impl Registrations {
            fn enclosing(&mut self) {
                match self.classes.last() {
                    Some(Some(class)) => self.found.with_subclasses.push(class.clone()),
                    _ => self.found.any = true,
                }
            }

            fn registered(&mut self, arenas: &NodeArenas, value: ExprId) {
                let Some(value) = arenas.peeled(value).and_then(|id| arenas.expr(id)) else {
                    self.found.any = true;
                    return;
                };
                match &value.kind {
                    ExprKind::ClassNameFetch(StaticTarget::Name(name)) => {
                        let written = name.written.trim_start_matches('\\');
                        if written.eq_ignore_ascii_case("self")
                            || written.eq_ignore_ascii_case("static")
                        {
                            self.enclosing();
                        } else if written.eq_ignore_ascii_case("parent") {
                            self.found.any = true;
                        } else {
                            let class = match &name.resolved {
                                ResolvedName::Exact(class) => class.as_str(),
                                _ => written,
                            };
                            self.found.classes.push(class.to_ascii_lowercase().into());
                        }
                    }
                    ExprKind::Literal(Literal::String(text)) => {
                        match arenas
                            .string_bytes(text)
                            .and_then(|b| std::str::from_utf8(b).ok())
                        {
                            Some(class) => self
                                .found
                                .classes
                                .push(class.trim_start_matches('\\').to_ascii_lowercase().into()),
                            None => self.found.any = true,
                        }
                    }
                    ExprKind::Name(name)
                        if arenas
                            .qualified_name(name)
                            .is_some_and(|name| name.written.eq_ignore_ascii_case("__CLASS__")) =>
                    {
                        self.enclosing();
                    }
                    _ => self.found.any = true,
                }
            }
        }
        impl ArenaVisitor for Registrations {
            fn visit_class_decl(&mut self, arenas: &NodeArenas, class: &syntax::ast::ClassDecl) {
                self.classes
                    .push(Some(class.fq_name().to_ascii_lowercase().into()));
                arena::walk_class_decl(self, arenas, class);
                self.classes.pop();
            }

            fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
                if let Some(e) = arenas.expr(id) {
                    match &e.kind {
                        ExprKind::NewAnonymousClass(_) => {
                            self.classes.push(None);
                            arena::walk_expr(self, arenas, id);
                            self.classes.pop();
                            return;
                        }
                        ExprKind::Call(callee, args) => {
                            let named = arenas
                                .peeled(*callee)
                                .and_then(|id| arenas.expr(id))
                                .and_then(|callee| match &callee.kind {
                                    ExprKind::Name(name) => arenas.qualified_name(name),
                                    _ => None,
                                })
                                .is_some_and(|name| {
                                    name.written.rsplit('\\').next().is_some_and(|last| {
                                        last.eq_ignore_ascii_case("stream_wrapper_register")
                                    })
                                });
                            if named {
                                let args = arenas.args(args.clone());
                                let class = args
                                    .iter()
                                    .enumerate()
                                    .find(|(at, arg)| match &arg.name {
                                        Some(name) => name.name.eq_ignore_ascii_case("class"),
                                        None => *at == 1,
                                    })
                                    .map(|(_, arg)| arg);
                                match class {
                                    Some(arg) if !arg.spread => self.registered(arenas, arg.value),
                                    _ => self.found.any = true,
                                }
                            }
                        }
                        _ => {}
                    }
                }
                arena::walk_expr(self, arenas, id);
            }
        }
        let mut walk = Registrations {
            found: StreamWrappers::default(),
            classes: Vec::new(),
        };
        arena::walk_stmts(&mut walk, arenas, stmts);
        let mut found = walk.found;
        found.settle();
        found
    }

    /// Adds the registrations `other` holds.
    pub(crate) fn add(&mut self, other: &StreamWrappers) {
        self.classes.extend(other.classes.iter().cloned());
        self.with_subclasses
            .extend(other.with_subclasses.iter().cloned());
        self.any |= other.any;
    }

    /// Sorts the names and keeps each once.
    pub(crate) fn settle(&mut self) {
        for names in [&mut self.classes, &mut self.with_subclasses] {
            names.sort_unstable();
            names.dedup();
        }
    }

    /// Whether `class` (lowercased) may be registered (of a settled set);
    /// `is_subclass_of` tells whether it extends or implements a class.
    pub(crate) fn may_be_registered(
        &self,
        class: &str,
        is_subclass_of: impl Fn(&str) -> bool,
    ) -> bool {
        self.any
            || self.classes.binary_search_by(|c| (**c).cmp(class)).is_ok()
            || self
                .with_subclasses
                .iter()
                .any(|base| **base == *class || is_subclass_of(base))
    }
}

/// The classes code registers as stream wrappers.
impl syntax::canonical::Canonical for StreamWrappers {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let StreamWrappers {
            classes,
            with_subclasses,
            any,
        } = self;
        p.node(
            "StreamWrappers",
            &[
                ("classes", classes),
                ("with_subclasses", with_subclasses),
                ("any", any),
            ],
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
#[derive(Debug, Clone)]
pub struct FileBodies {
    bodies: FxHashMap<Span, Body>,
    hooks: FxHashMap<Span, Hooks>,
    unset: UnsetProperties,
    wrappers: StreamWrappers,
    top_level: Arc<BodySummary>,
}

impl Default for FileBodies {
    fn default() -> FileBodies {
        FileBodies {
            bodies: FxHashMap::default(),
            hooks: FxHashMap::default(),
            unset: UnsetProperties::default(),
            wrappers: StreamWrappers::default(),
            top_level: Arc::new(BodySummary::of_file(&NodeArenas::default(), &[])),
        }
    }
}

impl FileBodies {
    pub fn of(arenas: &NodeArenas, stmts: &[StmtId]) -> FileBodies {
        FileBodies::collect(arenas, stmts, true, true, true)
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
            facts.mentions & MENTIONS_STREAM_WRAPPER != 0,
        )
    }

    fn collect(
        arenas: &NodeArenas,
        stmts: &[StmtId],
        in_expressions: bool,
        unsets: bool,
        registers: bool,
    ) -> FileBodies {
        struct Collect<'a> {
            bodies: FxHashMap<Span, Body>,
            hooks: FxHashMap<Span, Hooks>,
            // Whether the next function declaration visited is a method.
            method: bool,
            in_expressions: bool,
            // The file's statements, when a doc comment in it may carry an
            // inline `@var`.
            file: Option<&'a [StmtId]>,
        }
        impl Collect<'_> {
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
        impl ArenaVisitor for Collect<'_> {
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
                            summary: Arc::new(BodySummary::of_function_in(
                                arenas,
                                arenas.params(function.params.clone()),
                                arenas.stmt_ids(&body.stmts),
                                self.file,
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
            file: arenas
                .doc_sites
                .iter()
                .any(|site| site.text.contains("var"))
                .then_some(stmts),
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
            wrappers: if registers {
                StreamWrappers::of(arenas, stmts)
            } else {
                StreamWrappers::default()
            },
            top_level: Arc::new(BodySummary::of_file(arenas, stmts)),
        }
    }

    /// What the top level of the file, the code that runs when it is
    /// included, may write.
    pub(crate) fn top_level(&self) -> &Arc<BodySummary> {
        &self.top_level
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
/// order, then the properties it unsets and the classes it registers as
/// stream wrappers.
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
        serde::Serialize::serialize(
            &(bodies, hooks, &self.unset, &self.wrappers, &self.top_level),
            serializer,
        )
    }
}

impl<'de> serde::Deserialize<'de> for FileBodies {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<FileBodies, D::Error> {
        type Stored = (
            Vec<(Span, Arc<BodySummary>, Option<Arc<InitFlow>>)>,
            Vec<(Span, Hooks)>,
            UnsetProperties,
            StreamWrappers,
            Arc<BodySummary>,
        );
        let (bodies, hooks, unset, wrappers, top_level): Stored =
            serde::Deserialize::deserialize(deserializer)?;
        Ok(FileBodies {
            bodies: bodies
                .into_iter()
                .map(|(span, summary, flow)| (span, Body { summary, flow }))
                .collect(),
            hooks: hooks.into_iter().collect(),
            unset,
            wrappers,
            top_level,
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

    fn stream_wrappers(&self, out: &mut StreamWrappers) {
        out.add(&self.wrappers);
    }

    fn names_property_writer(&self) -> bool {
        true
    }

    fn files(&self) -> Vec<LoadedFile<'_>> {
        Vec::new()
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
            wrappers,
            top_level,
        } = self;
        let mut bodies: Vec<(&Span, &Body)> = bodies.iter().collect();
        bodies.sort_unstable_by_key(|(span, _)| (span.start, span.end));
        let mut hooks: Vec<(&Span, &Hooks)> = hooks.iter().collect();
        hooks.sort_unstable_by_key(|(span, _)| (span.start, span.end));
        p.node(
            "FileBodies",
            &[
                ("bodies", &bodies),
                ("hooks", &hooks),
                ("unset", unset),
                ("wrappers", wrappers),
                ("top_level", top_level),
            ],
        );
    }
}
