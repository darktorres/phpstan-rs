//! "What symbol is at this (file, position)" - the shared first step
//! both hover and go-to-definition need. Walks a
//! file's AST (already retained by `Project`, see `Project::ast`)
//! looking for the innermost name reference or declaration whose span
//! contains the cursor, resolving it against the file's own namespace/
//! `use`-imports (`NameContext`, the exact machinery `Checker`/
//! `Project::declared_names` already use). Each name in a compound
//! reference (`Foo::CONST`, `?Foo|Bar`) has its own span in the AST.
//!
//! Also the traversal `textDocument/documentHighlight` (`lsp::document_
//! highlight`... no such module - see `Workspace::document_highlight` in
//! `lsp.rs`) reuses wholesale via `collect_all`: the same walker, run
//! with `Anchor::All` instead of `Anchor::Position(pos)`, collects every
//! `(Span, Resolved)` pair in the file instead of stopping at the single
//! best match for one cursor position. `resolve_at` and `collect_all`
//! share one underlying `walk_file` - the only difference between "what
//! is at this exact position" and "everywhere in this file" is which
//! `Anchor` variant gates whether a candidate gets recorded, never the
//! traversal itself.
//!
//! A `Visitor` over the whole file, recording each name, member and
//! variable site it reaches, a node before the nodes inside it.
//!
//! Deliberately does **not** resolve an instance receiver's own type
//! (`$x->method()`, `$x->prop`) or a local variable *read*'s own
//! declaration site - both need real type-inference/dataflow machinery
//! this module has none of. `lsp.rs`'s own orchestration runs a real,
//! throwaway `Checker` probe for both (`Resolved::Instance`/`Resolved::
//! VariableRead`'s own doc comments), reusing the checker's already-
//! correct flow-sensitive analysis (branch merging, loop fixed-point
//! convergence, termination-pruning) rather than a second, hand-rolled
//! approximation of it - see `Checker::probe_var_provenance`'s own doc
//! comment in `checker.rs` for the full design.

use syntax::name::NameSet;
use types::Name;

use crate::project::Project;
use crate::registry::Registry;
use std::path::Path;
use syntax::ast::*;
use syntax::span::Span;
use syntax::visit::arena::{self, Visitor};

#[derive(Debug, Clone)]
pub(crate) enum Resolved {
    Class(String),
    Function(String),
    /// A class member - method, property, or class-const, all looked up
    /// the same way (`Registry::lookup_method`/`lookup_property`/
    /// `lookup_const`, tried in that order by the caller) since a plain
    /// name reference alone doesn't say which kind it is.
    Member(String, String),
    /// An instance receiver's own member access
    /// (`$x->method()`/`$x->prop`) - resolving *which* member this is
    /// needs `$x`'s inferred type, which needs the checker's own
    /// machinery, not just a name lookup. Everything `lsp.rs`'s own
    /// orchestration needs to run that probe: the receiver's own span
    /// (`Checker::probe_span`'s target - matched by exact span, not
    /// position, so a chained call's own inner/outer receivers are never
    /// ambiguous), which member name, whether it's a method call or a
    /// property read, and the enclosing scope to re-check (see
    /// `ProbeScope`'s own doc comment).
    Instance {
        receiver_span: Span,
        member: String,
        scope: ProbeScope,
        is_static: bool,
        class: Option<String>,
    },
    /// A local variable's own declaration site, clicked directly (a
    /// parameter, a `foreach`/`catch`/`static`/`global` binding's own
    /// name) - always same-file, and the click's own position already
    /// *is* the answer for go-to-definition, so no probe is needed for
    /// that. `scope`/`is_static`/`class` exist for hover's own sake only
    /// (what type does this declaration itself bind? - which genuinely
    /// needs a probe, e.g. a `foreach` binding's type depends on
    /// inferring the iterable), mirroring `VariableRead`'s identical
    /// fields below. `name`/`var_scope` exist for `document_highlight`'s
    /// own sake only (which other occurrences are the *same* local -
    /// see `var_scope`'s own doc comment on `Walker`), unused by hover/
    /// definition.
    VariableSelf {
        span: Span,
        scope: ProbeScope,
        is_static: bool,
        class: Option<String>,
        name: Name,
        var_scope: u32,
    },
    /// A local variable *read* - resolved via a real, checker-driven
    /// reaching-definitions probe (`Checker::probe_span`/
    /// `probe_var_provenance`, the same mechanism `Instance` above uses
    /// for a receiver's own type), not a hand-rolled approximation. Might
    /// resolve to more than one real declaration site when the read is
    /// only reachable through mutually exclusive branches that assign the
    /// name differently - `lsp.rs` offers every one as a real alternative
    /// (a `GotoDefinitionResponse::Array`), never an arbitrary guess.
    VariableRead {
        span: Span,
        scope: ProbeScope,
        is_static: bool,
        class: Option<String>,
        name: Name,
        var_scope: u32,
    },
}

/// What `lsp.rs`'s own throwaway `Checker` probe re-checks to answer an
/// `Instance`/`VariableRead` request - a clone of the enclosing named
/// function/method (owned, not borrowed, so `Resolved` stays free of
/// `Project`'s own lifetime, at the cost of one clone of a typically-small
/// function body per request), or, at top-level script scope (no
/// enclosing function at all), a clone of the *whole file's* own top-level
/// statement list, re-checked via `Checker::check_program` instead of
/// `check_function` - real top-level PHP code is exactly as checkable as
/// a function body, just through a different entry point.
#[derive(Debug, Clone)]
pub(crate) enum ProbeScope {
    Function {
        function: Box<FunctionDecl>,
        owner: Option<std::sync::Arc<syntax::ParsedFile>>,
    },
    TopLevel {
        statements: Vec<StmtId>,
        owner: Option<std::sync::Arc<syntax::ParsedFile>>,
    },
}

impl ProbeScope {
    pub(crate) fn owner(&self) -> Option<&std::sync::Arc<syntax::ParsedFile>> {
        match self {
            Self::Function { owner, .. } | Self::TopLevel { owner, .. } => owner.as_ref(),
        }
    }
}

/// What a candidate `Span` is checked against - a single cursor position
/// (`resolve_at`'s own use, `.matches()` degenerating to the original
/// `contains()` check) or unconditionally everything in the file
/// (`collect_all`'s use, for `document_highlight` - every candidate the
/// walker would otherwise gate on cursor proximity is recorded instead).
pub(crate) enum Anchor {
    /// A byte offset of the file.
    Offset(u32),
    All,
}

impl Anchor {
    fn matches(&self, span: Span) -> bool {
        match self {
            Anchor::Offset(at) => contains(span, *at),
            Anchor::All => true,
        }
    }
}

/// The entry point: what's declared/referenced at the byte offset `at` in the file
/// loaded under `path`, or `None` for nothing resolvable there. The
/// walk order matches `out`'s own push order, so `.last()` is exactly
/// the old single-`best`-overwritten-on-every-match semantics: the
/// innermost (visited last) containing match wins.
pub(crate) fn resolve_at(project: &Project, path: &Path, at: u32) -> Option<Resolved> {
    walk_file(project, path, Anchor::Offset(at))?
        .into_iter()
        .map(|(_, r)| r)
        .next_back()
}

/// Every `(Span, Resolved)` pair anywhere in the file - `document_
/// highlight`'s own source list, filtered down to whichever ones are
/// the same symbol as the target the cursor is actually on.
pub(crate) fn collect_all(project: &Project, path: &Path) -> Vec<(Span, Resolved)> {
    walk_file(project, path, Anchor::All).unwrap_or_default()
}

fn walk_file(project: &Project, path: &Path, anchor: Anchor) -> Option<Vec<(Span, Resolved)>> {
    let tree = project.ast_of(path)?;
    let arenas = tree.arenas();
    let stmts = tree.statements();
    let syntax_owner = tree.parsed_file_for_completion();
    let registry = project.registry();
    let mut w = Walker {
        syntax_owner,
        registry,
        anchor,
        top_level_stmts: stmts,
        current_function: None,
        current_is_static: false,
        current_class: None,
        next_scope_id: 0,
        base_scope_id: 0,
        scope_frames: Vec::new(),
        out: Vec::new(),
    };
    arena::walk_stmts(&mut w, arenas, stmts);
    Some(w.out)
}

/// Half-open (`span.end` is one past the last real byte - confirmed
/// against the lexer's own token-span construction) containment check.
pub(super) fn contains(span: Span, at: u32) -> bool {
    span.contains(at)
}

/// The class a class name written in code (`self`/`static`/`parent`, or
/// an ordinary/namespaced name) refers to - the same combined logic
/// `Checker::resolve_name_expr`/`resolve_static_class_name`/
/// `resolve_pseudo` provide together, minus the `Type`-wrapping.
pub(super) fn resolve_class_ref(
    name: &QualifiedName,
    current_class: Option<&str>,
    registry: &Registry,
) -> Option<String> {
    if name.is_keyword("self") || name.is_keyword("static") {
        return current_class.map(str::to_string);
    }
    if name.is_keyword("parent") {
        return registry
            .class_references_by_name(current_class?)
            .and_then(|references| references.extends.first())
            .map(|s| s.to_string());
    }
    name.resolved_class().map(str::to_string)
}

/// A lexically-nested `Closure`/`ArrowFn` scope, pushed on entry and
/// popped on exit - see `Walker::scope_frames`'s own doc comment for how
/// `resolve_var_scope` reads this stack.
enum ScopeFrame {
    /// A `Closure` variable-shadowing boundary: a name NOT in its own
    /// `use (...)` list belongs to the closure's own scope (`id`); a
    /// name that IS in `transparent` is a real capture of an outer
    /// variable, so resolution keeps walking outward past this frame.
    Closure { id: u32, transparent: NameSet },
    /// An arrow function's implicit-capture-by-value boundary: PHP's
    /// `fn() => ...` auto-captures every outer variable it reads BY
    /// VALUE, so (unlike `Closure`) there is no explicit capture list to
    /// consult - only its own declared parameters (`owns`) belong to a
    /// genuinely new scope; every other name resolves as the SAME
    /// variable as whatever it refers to outside the arrow function.
    ArrowFn { id: u32, owns: NameSet },
}

struct Walker<'a> {
    syntax_owner: std::sync::Arc<syntax::ParsedFile>,
    registry: &'a Registry,
    anchor: Anchor,
    // The whole file's own top-level statement list - `ProbeScope::
    // TopLevel`'s own source when a probe request lands at script scope
    // (`current_function` is `None`), cloned only then (one clone per
    // request, the same cost `ProbeScope::Function` already accepts for
    // an ordinary function-scoped probe).
    top_level_stmts: &'a [StmtId],
    // An owned clone (not a borrow - see `ProbeScope`'s own doc comment
    // for why) of whichever *named*
    // `FunctionDecl`/`MethodDecl` currently encloses the statement being
    // walked, updated on entry to `function` and restored on exit -
    // `None` at top-level script scope, in which case a probe request
    // uses `top_level_stmts` instead (see `probe_scope`). Deliberately
    // *not* updated for a `Closure`/`ArrowFn` body: those aren't a
    // separate `Checker` entry point at all - `check_expr`'s own
    // `Closure`/`ArrowFn` arms already recurse into a closure's body as
    // part of ordinarily checking whatever named function contains the
    // closure literal, so leaving `current_function` pointing at that
    // enclosing named function (not resetting it) is exactly what lets a
    // probe find a receiver/variable inside a closure body too -
    // re-running `check_function` on the *named* function reaches the
    // nested closure the same way a real `check-project` pass already
    // does.
    current_function: Option<FunctionDecl>,
    current_is_static: bool,
    // A purely syntactic id source for `document_highlight`'s own same-
    // variable grouping - unrelated to `ProbeScope`/`Checker::
    // probe_var_provenance`'s real flow-sensitive scoping (hover/
    // definition never read `var_scope` at all). Starts at `0` so the
    // first real nested scope's id (`1`) never collides with
    // `base_scope_id`'s own default of `0`.
    next_scope_id: u32,
    // The scope id a variable resolves to once `scope_frames` is
    // exhausted (falls off the end) - `0` by default, meaning "top-level
    // script scope or the current named function/method's own top-level
    // body," reassigned per-function by `function()`'s own swap. This
    // default state (0, no frames) IS the top-level-script-scope case
    // with no special-casing needed: a closure/arrow-fn declared
    // directly in top-level code still pushes its own frame around
    // itself exactly as it would inside a named function.
    base_scope_id: u32,
    // Innermost-last stack of currently-open `Closure`/`ArrowFn` scopes -
    // see `resolve_var_scope` for how a name is resolved against it.
    scope_frames: Vec<ScopeFrame>,
    // The class whose body the walk is in.
    current_class: Option<String>,
    // Every match so far, a node before the nodes inside it, so the last
    // one containing a position is the innermost.
    out: Vec<(Span, Resolved)>,
}

impl<'a> Walker<'a> {
    fn next_id(&mut self) -> u32 {
        self.next_scope_id += 1;
        self.next_scope_id
    }

    /// Which scope `name` actually belongs to, walking `scope_frames`
    /// innermost-first: a `Closure` frame claims `name` unless it's one
    /// of its own explicit `use (...)` captures (in which case resolution
    /// keeps walking outward looking for where that capture really
    /// lives); an `ArrowFn` frame claims `name` only if it's one of the
    /// arrow function's own parameters (everything else is an implicit
    /// by-value capture of whatever the name already means outside it).
    /// Falling off the end of the stack resolves to `base_scope_id`.
    fn resolve_var_scope(&self, name: &Name) -> u32 {
        for frame in self.scope_frames.iter().rev() {
            match frame {
                ScopeFrame::Closure { id, transparent } => {
                    if !transparent.contains(name) {
                        return *id;
                    }
                }
                ScopeFrame::ArrowFn { id, owns } => {
                    if owns.contains(name) {
                        return *id;
                    }
                }
            }
        }
        self.base_scope_id
    }

    /// The `ProbeScope` for a probe request at the current position - the
    /// enclosing named function/method if there is one, else the whole
    /// file's own top-level statements (real, checkable PHP in its own
    /// right, just via `Checker::check_program` instead of `check_
    /// function` - see `ProbeScope`'s own doc comment).
    fn probe_scope(&self) -> ProbeScope {
        match &self.current_function {
            Some(f) => ProbeScope::Function {
                function: Box::new(f.clone()),
                owner: Some(std::sync::Arc::clone(&self.syntax_owner)),
            },
            None => ProbeScope::TopLevel {
                statements: self.top_level_stmts.to_vec(),
                owner: Some(std::sync::Arc::clone(&self.syntax_owner)),
            },
        }
    }

    fn record(&mut self, span: Span, value: Resolved) {
        if self.anchor.matches(span) {
            self.out.push((span, value));
        }
    }

    fn class_ref(&self, name: &QualifiedName) -> Option<String> {
        resolve_class_ref(name, self.current_class.as_deref(), self.registry)
    }

    /// Records `name`, a class name, at its span.
    fn record_class(&mut self, name: &QualifiedName) {
        if self.anchor.matches(name.span)
            && let Some(fqn) = self.class_ref(name)
        {
            self.out.push((name.span, Resolved::Class(fqn)));
        }
    }

    /// Records `member`'s receiver probe: the receiver's type decides
    /// which class's member it is.
    fn instance(&self, receiver_span: Span, member: &Ident) -> Resolved {
        Resolved::Instance {
            receiver_span,
            member: member.to_string(),
            scope: self.probe_scope(),
            is_static: self.current_is_static,
            class: self.current_class.clone(),
        }
    }

    /// Records `class::member` for a literal class name, each name at its
    /// own span; under `Anchor::All` both get an entry.
    fn record_static_member(&mut self, whole: Span, class: &QualifiedName, member: &Ident) {
        if self.anchor.matches(whole)
            && let Some(fqn) = self.class_ref(class)
        {
            self.record(class.span, Resolved::Class(fqn.clone()));
            self.record(member.span, Resolved::Member(fqn, member.to_string()));
        }
    }

    /// Records `span` as a local variable's own declaration site, self-
    /// resolving (no probe needed - the click's own position already is
    /// the answer) for go-to-definition when the cursor lands directly on
    /// it - `scope`/`is_static`/`class` are only for hover's own later,
    /// separate probe (see `Resolved::VariableSelf`'s own doc comment).
    fn record_var_self(&mut self, span: Span, name: &Name) {
        let value = Resolved::VariableSelf {
            span,
            scope: self.probe_scope(),
            is_static: self.current_is_static,
            class: self.current_class.clone(),
            name: name.clone(),
            var_scope: self.resolve_var_scope(name),
        };
        self.record(span, value);
    }

    /// Records `span` as a read of the variable `name`.
    fn record_var_read(&mut self, span: Span, name: &Name) {
        let value = Resolved::VariableRead {
            span,
            scope: self.probe_scope(),
            is_static: self.current_is_static,
            class: self.current_class.clone(),
            name: name.clone(),
            var_scope: self.resolve_var_scope(name),
        };
        self.record(span, value);
    }

    /// A binding target - a `foreach` key/value - is a declaration site,
    /// not a read, including every leaf of a list-destructuring target
    /// (`foreach ($rows as [$a, $b])`). Anything else (rare: a property/
    /// index write target as a foreach binding isn't valid PHP at all)
    /// falls back to an ordinary read-walk.
    fn bind_var(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else {
            return;
        };
        syntax::stack::guard(|| match &e.kind {
            ExprKind::Variable(name) => self.record_var_self(
                arenas
                    .expr_span(e.id)
                    .expect("expression has a parallel span"),
                name,
            ),
            ExprKind::Destructure(items, _) => {
                for item in arenas.destructure_items(items) {
                    if let Some(key) = item.key {
                        self.visit_expr(arenas, key);
                    }
                    if let Some(target) = item.target {
                        self.bind_var(arenas, target);
                    }
                }
            }
            _ => self.visit_expr(arenas, id),
        })
    }
}

impl Visitor for Walker<'_> {
    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else {
            return;
        };
        syntax::stack::guard(|| match &stmt.kind {
            StmtKind::Foreach {
                subject,
                key,
                value,
                body,
                ..
            } => {
                self.visit_expr(arenas, *subject);
                if let Some(key) = key {
                    self.bind_var(arenas, *key);
                }
                self.bind_var(arenas, *value);
                self.visit_stmt(arenas, *body);
            }
            StmtKind::FunctionDecl(function_id) => {
                if let Some(function) = arenas.function(*function_id) {
                    self.record(
                        function.name.span,
                        Resolved::Function(function.fq_name().to_string()),
                    );
                }
                arena::walk_stmt(self, arenas, id);
            }
            StmtKind::Global(vars) => {
                for var in arenas.global_vars(vars.clone()) {
                    match var {
                        GlobalVar::Name(name) => self.record_var_self(name.span, &name.name),
                        GlobalVar::Dynamic(e) => self.visit_expr(arenas, *e),
                    }
                }
            }
            StmtKind::StaticVar(vars) => {
                for variable in arenas.static_vars(vars.clone()) {
                    if let Some(e) = variable.default {
                        self.visit_expr(arenas, e);
                    }
                    self.record_var_self(variable.name.span, &variable.name.name);
                }
            }
            _ => arena::walk_stmt(self, arenas, id),
        })
    }

    fn visit_catch_clause(&mut self, arenas: &NodeArenas, clause: &CatchClause) {
        for name in arenas.qualified_names(&clause.types) {
            self.record_class(name);
        }
        if let Some(var) = &clause.var {
            self.record_var_self(var.span, &var.name);
        }
        arena::walk_catch_clause(self, arenas, clause);
    }

    /// Resolves a compound type-hint (`?Foo`, `Foo|Bar`) to whichever
    /// single class name inside it the cursor lands on, if any. Under
    /// `Anchor::All`, every class name in the hint gets its own entry.
    fn visit_type_hint(&mut self, arenas: &NodeArenas, hint: &TypeHint) {
        if !self.anchor.matches(hint.span) {
            return;
        }
        for name in hint.class_names(arenas) {
            self.record_class(name);
        }
    }

    fn visit_param(&mut self, arenas: &NodeArenas, param: &Param) {
        for attr in arenas.attributes(param.attrs.clone()) {
            self.visit_attribute(arenas, attr);
        }
        if let Some(hint) = &param.type_hint {
            self.visit_type_hint(arenas, hint);
        }
        self.record_var_self(param.name.span, &param.name.name);
        if let Some(default) = param.default {
            self.visit_expr(arenas, default);
        }
        if let Some(hooks) = &param.hooks {
            self.visit_property_hooks(arenas, hooks);
        }
    }

    fn visit_function_decl(&mut self, arenas: &NodeArenas, function: &FunctionDecl) {
        // A named function/method (including one nested inside another
        // function's body - real, legal PHP) is its own fresh top-level
        // variable scope, independent of `current_function`/
        // `current_is_static`, which track the real `Checker` probe
        // scope, not this purely syntactic same-variable grouping.
        // Swapped before the parameters: a parameter's own `var_scope`
        // must resolve against this function's fresh scope, never
        // whatever enclosing `Closure`/`ArrowFn` frame happened to be
        // open at the declaration site.
        let new_base_scope_id = self.next_id();
        let previous_base_scope_id = std::mem::replace(&mut self.base_scope_id, new_base_scope_id);
        let previous_scope_frames = std::mem::take(&mut self.scope_frames);
        let previous_function = self.current_function.replace(function.clone());
        arena::walk_function_decl(self, arenas, function);
        self.current_function = previous_function;
        self.scope_frames = previous_scope_frames;
        self.base_scope_id = previous_base_scope_id;
    }

    fn visit_class_decl(&mut self, arenas: &NodeArenas, class: &ClassDecl) {
        let fqn = class.fq_name().to_string();
        let previous = self.current_class.replace(fqn.clone());
        self.record(class.name.span, Resolved::Class(fqn));
        arena::walk_class_decl(self, arenas, class);
        self.current_class = previous;
    }

    fn visit_class_def(&mut self, arenas: &NodeArenas, class: &ClassDef) {
        for name in class
            .extends(arenas)
            .iter()
            .chain(class.implements(arenas))
            .chain(class.used_traits(arenas))
        {
            self.record_class(name);
        }
        arena::walk_class_def(self, arenas, class);
    }

    fn visit_method_decl(&mut self, arenas: &NodeArenas, method: &MethodDecl) {
        self.record_member(method.function.name.span, &method.function.name);
        let previous = std::mem::replace(&mut self.current_is_static, method.is_static);
        arena::walk_method_decl(self, arenas, method);
        self.current_is_static = previous;
    }

    fn visit_property_decl(&mut self, arenas: &NodeArenas, property: &PropertyDecl) {
        self.record_member(property.name.span, &property.name);
        arena::walk_property_decl(self, arenas, property);
    }

    fn visit_class_const(&mut self, arenas: &NodeArenas, constant: &ClassConst) {
        self.record_member(constant.name.span, &constant.name);
        arena::walk_class_const(self, arenas, constant);
    }

    fn visit_enum_case(&mut self, arenas: &NodeArenas, case: &EnumCase) {
        self.record_member(case.name.span, &case.name);
        arena::walk_enum_case(self, arenas, case);
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(expr) = arenas.expr(id) else {
            return;
        };
        syntax::stack::guard(|| {
            match &expr.kind {
                ExprKind::NewAnonymousClass(class_id) => {
                    let Some(class) = arenas.anonymous_class(*class_id) else {
                        return;
                    };
                    arena::walk_args(self, arenas, class.args.clone());
                    let previous = std::mem::replace(
                        &mut self.current_class,
                        arenas
                            .anonymous_class_name(*class_id)
                            .map(ToString::to_string),
                    );
                    if let Some(def) = arenas.class(class.def) {
                        self.visit_class_def(arenas, def);
                    }
                    self.current_class = previous;
                    return;
                }
                // `foo()`'s callee - a bare `Name` here is always a function
                // reference (never a class), unlike a bare `Name` reached
                // some other way (see the plain `ExprKind::Name` arm below).
                ExprKind::Call(callee, args)
                    if matches!(
                        arenas.expr(*callee).map(|e| &e.kind),
                        Some(ExprKind::Name(_))
                    ) =>
                {
                    let Some(ExprNode {
                        kind: ExprKind::Name(n),
                        ..
                    }) = arenas.expr(*callee)
                    else {
                        unreachable!()
                    };
                    let Some(n) = arenas.qualified_name(n) else {
                        return;
                    };
                    let fqn = self.registry.called_function_name(n).to_string();
                    let span = arenas
                        .expr_span(*callee)
                        .unwrap_or_else(|| arenas.expr_span(expr.id).expect("expression span"));
                    self.record(span, Resolved::Function(fqn));
                    arena::walk_args(self, arenas, args.clone());
                    return;
                }
                // `new Foo(...)`'s callee - always a class reference, the
                // mirror image of `Call` just above.
                ExprKind::New(callee, args)
                    if matches!(
                        arenas.expr(*callee).map(|e| &e.kind),
                        Some(ExprKind::Name(_))
                    ) =>
                {
                    let Some(ExprNode {
                        kind: ExprKind::Name(n),
                        ..
                    }) = arenas.expr(*callee)
                    else {
                        unreachable!()
                    };
                    let Some(n) = arenas.qualified_name(n) else {
                        return;
                    };
                    self.record_class(n);
                    arena::walk_args(self, arenas, args.clone());
                    return;
                }
                // A bare `Name` reached any *other* way (a first-class
                // callable target `foo(...)`, ...) - neither `Call` nor
                // `New` disambiguated it, so ask the registry which one
                // actually exists, preferring a function (the far more
                // common shape for a standalone name reference).
                ExprKind::Name(n) => {
                    let Some(n) = arenas.qualified_name(n) else {
                        return;
                    };
                    let resolved = match self.registry.resolve_function(n) {
                        Some((fqn, _)) => Some(Resolved::Function(fqn.to_string())),
                        None => self.class_ref(n).map(Resolved::Class),
                    };
                    if let Some(resolved) = resolved {
                        self.record(
                            arenas.expr_span(expr.id).expect("expression span"),
                            resolved,
                        );
                    }
                }
                ExprKind::ClassConstFetch(StaticTarget::Name(class), member) => {
                    self.record_static_member(
                        arenas.expr_span(expr.id).expect("expression span"),
                        class,
                        member,
                    );
                }
                ExprKind::ClassNameFetch(StaticTarget::Name(class)) => {
                    self.record_class(class);
                }
                // `Foo::method()` mirrors `ClassConstFetch` above (a literal
                // class name, resolved by plain name lookup). A dynamic
                // receiver (`$obj::method()`) has no class-name text to
                // resolve here at all - it needs the same type-inference
                // probe `MethodCall`'s own "any other receiver" branch
                // below uses, `Resolved::Instance` being agnostic to `->`
                // vs `::` call syntax.
                ExprKind::StaticCall(StaticTarget::Name(class), member, _) => {
                    self.record_static_member(
                        arenas.expr_span(expr.id).expect("expression span"),
                        class,
                        member,
                    );
                }
                ExprKind::StaticCall(StaticTarget::Expr(receiver), member, args) => {
                    self.visit_expr(arenas, *receiver);
                    let span = arenas
                        .expr_span(*receiver)
                        .unwrap_or_else(|| arenas.expr_span(expr.id).expect("expression span"));
                    let instance = self.instance(span, member);
                    self.record(member.span, instance);
                    arena::walk_args(self, arenas, args.clone());
                    return;
                }
                ExprKind::Instanceof(_, InstanceofTarget::Name(name)) => {
                    if let Some(name) = arenas.qualified_name(name) {
                        self.record_class(name);
                    }
                }
                // A plain variable read (`$this` excluded - it's never a
                // locally-declared variable, and is resolved separately via
                // `current_class` wherever it appears as a receiver). Needs
                // a real checker probe to resolve (see `Resolved::
                // VariableRead`'s own doc comment) - this only records enough
                // for `lsp.rs`'s own orchestration to run it, at either scope
                // `probe_scope` covers (a named function/method, or top-level
                // script scope).
                ExprKind::Variable(name) if **name != *"this" => {
                    self.record_var_read(arenas.expr_span(expr.id).expect("expression span"), name);
                }
                // `$this->method()`/`$this->prop` resolve directly (only
                // `current_class` is needed); any *other* receiver needs a
                // type-inference probe - this records enough (the receiver's
                // own span, the member name, and the enclosing scope to
                // re-check) for `lsp.rs`'s own orchestration to run it. The
                // recorded span is the member NAME's own, not the whole
                // call's, which would make every `document_highlight` range
                // for this arm too wide.
                ExprKind::MethodCall(recv, method, args, _) => {
                    self.visit_expr(arenas, *recv);
                    self.record_receiver_member(arenas, *recv, method);
                    arena::walk_args(self, arenas, args.clone());
                    return;
                }
                ExprKind::PropertyAccess(recv, property, _) => {
                    self.visit_expr(arenas, *recv);
                    self.record_receiver_member(arenas, *recv, property);
                    return;
                }
                ExprKind::Closure(closure_id) => {
                    let Some(syntax::ast::ClosurePayload::Closure(closure)) =
                        arenas.closure(*closure_id)
                    else {
                        return;
                    };
                    let syntax::ast::ClosureExpr {
                        params, uses, body, ..
                    } = closure;
                    for p in arenas.params(params.clone()) {
                        self.visit_param(arenas, p);
                    }
                    // A `use (&$x)`/`use ($x)` capture is itself a real,
                    // navigable reference to the outer scope's `$x` -
                    // resolved the same way an ordinary variable read is
                    // (`current_function` is deliberately never updated for
                    // a closure literal - see this file's own module doc -
                    // so it still points at the enclosing named function or
                    // top-level scope). Each capture's own `var_scope` is
                    // resolved before this closure's own frame is pushed
                    // below, since a `use` clause names an outer variable,
                    // not one of the closure's own locals.
                    let uses = arenas.closure_uses(uses);
                    for u in uses {
                        self.record_var_read(u.name.span, &u.name.name);
                    }
                    let id = self.next_id();
                    self.scope_frames.push(ScopeFrame::Closure {
                        id,
                        transparent: uses.iter().map(|u| u.name.name.clone()).collect(),
                    });
                    arena::walk_stmts(self, arenas, arenas.stmt_ids(body));
                    self.scope_frames.pop();
                    return;
                }
                ExprKind::ArrowFn(arrow_id) => {
                    let Some(syntax::ast::ClosurePayload::ArrowFn(arrow)) =
                        arenas.closure(*arrow_id)
                    else {
                        return;
                    };
                    let syntax::ast::ArrowFnExpr { params, body, .. } = arrow;
                    for p in arenas.params(params.clone()) {
                        self.visit_param(arenas, p);
                    }
                    let id = self.next_id();
                    self.scope_frames.push(ScopeFrame::ArrowFn {
                        id,
                        owns: arenas
                            .params(params.clone())
                            .iter()
                            .map(|p| p.name.name.clone())
                            .collect(),
                    });
                    self.visit_expr(arenas, *body);
                    self.scope_frames.pop();
                    return;
                }
                _ => {}
            }
            arena::walk_expr(self, arenas, id);
        })
    }
}

impl Walker<'_> {
    /// Records a member of the class being walked, at its declaration.
    fn record_member(&mut self, span: Span, name: &str) {
        if let Some(class) = self.current_class.clone() {
            self.record(span, Resolved::Member(class, name.to_string()));
        }
    }

    /// Records `member` of `recv`: directly for `$this`, else as a probe
    /// of the receiver's type.
    fn record_receiver_member(&mut self, arenas: &NodeArenas, recv: ExprId, member: &Ident) {
        let Some(recv_node) = arenas.expr(recv) else {
            return;
        };
        let value = match &self.current_class {
            Some(owner) if matches!(&recv_node.kind, ExprKind::Variable(v) if *v == "this") => {
                Resolved::Member(owner.clone(), member.to_string())
            }
            _ => self.instance(
                arenas
                    .expr_span(recv_node.id)
                    .expect("receiver expression has a parallel span"),
                member,
            ),
        };
        self.record(member.span, value);
    }
}
