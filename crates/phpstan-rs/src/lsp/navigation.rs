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
//! highlight`... no such module - see `Server::document_highlight` in
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

use types::Name;

use crate::project::Project;
use crate::registry::Registry;
use crate::registry::class_decl_fqn;
use rustc_hash::FxHashSet;
use std::path::Path;
use syntax::ast::*;
use syntax::token::{Position, Span};
use syntax::visit::{
    Visitor, walk_args, walk_catch_clause, walk_class_const, walk_class_decl, walk_enum_case,
    walk_expr, walk_function_decl, walk_method_decl, walk_property_decl, walk_stmt, walk_stmts,
};

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
    Function(Box<FunctionDecl>),
    TopLevel(Vec<Stmt>),
}

/// What a candidate `Span` is checked against - a single cursor position
/// (`resolve_at`'s own use, `.matches()` degenerating to the original
/// `contains()` check) or unconditionally everything in the file
/// (`collect_all`'s use, for `document_highlight` - every candidate the
/// walker would otherwise gate on cursor proximity is recorded instead).
pub(crate) enum Anchor {
    Position(Position),
    All,
}

impl Anchor {
    fn matches(&self, span: Span) -> bool {
        match self {
            Anchor::Position(pos) => contains(span, *pos),
            Anchor::All => true,
        }
    }
}

/// The entry point: what's declared/referenced at `pos` in the file
/// loaded under `path`, or `None` for nothing resolvable there. The
/// walk order matches `out`'s own push order, so `.last()` is exactly
/// the old single-`best`-overwritten-on-every-match semantics: the
/// innermost (visited last) containing match wins.
pub(crate) fn resolve_at(project: &Project, path: &Path, pos: Position) -> Option<Resolved> {
    walk_file(project, path, Anchor::Position(pos))?
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
    let stmts = project.ast(path)?;
    let registry = project.registry();
    let mut w = Walker {
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
    walk_stmts(&mut w, stmts);
    Some(w.out)
}

/// 1-indexed, half-open (`span.end` is one past the last real character -
/// confirmed against the lexer's own token-span construction) containment
/// check.
pub(super) fn contains(span: Span, pos: Position) -> bool {
    let after_start =
        span.start.line < pos.line || (span.start.line == pos.line && span.start.col <= pos.col);
    let before_end =
        pos.line < span.end.line || (pos.line == span.end.line && pos.col < span.end.col);
    after_start && before_end
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
            .classes
            .get(current_class?)
            .and_then(|info| info.extends().first())
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
    Closure {
        id: u32,
        transparent: FxHashSet<Name>,
    },
    /// An arrow function's implicit-capture-by-value boundary: PHP's
    /// `fn() => ...` auto-captures every outer variable it reads BY
    /// VALUE, so (unlike `Closure`) there is no explicit capture list to
    /// consult - only its own declared parameters (`owns`) belong to a
    /// genuinely new scope; every other name resolves as the SAME
    /// variable as whatever it refers to outside the arrow function.
    ArrowFn { id: u32, owns: FxHashSet<Name> },
}

struct Walker<'a> {
    registry: &'a Registry,
    anchor: Anchor,
    // The whole file's own top-level statement list - `ProbeScope::
    // TopLevel`'s own source when a probe request lands at script scope
    // (`current_function` is `None`), cloned only then (one clone per
    // request, the same cost `ProbeScope::Function` already accepts for
    // an ordinary function-scoped probe).
    top_level_stmts: &'a [Stmt],
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
            Some(f) => ProbeScope::Function(Box::new(f.clone())),
            None => ProbeScope::TopLevel(self.top_level_stmts.to_vec()),
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
    fn bind_var(&mut self, e: &Expr) {
        syntax::stack::guard(|| match &e.kind {
            ExprKind::Variable(name) => self.record_var_self(e.span, name),
            ExprKind::Destructure(items, _) => {
                for item in items {
                    if let Some(k) = &item.key {
                        self.visit_expr(k);
                    }
                    if let Some(target) = &item.target {
                        self.bind_var(target);
                    }
                }
            }
            _ => self.visit_expr(e),
        })
    }
}

impl<'ast> Visitor<'ast> for Walker<'_> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        syntax::stack::guard(|| match &stmt.kind {
            StmtKind::Foreach {
                subject,
                key,
                value,
                body,
                ..
            } => {
                self.visit_expr(subject);
                if let Some(k) = key {
                    self.bind_var(k);
                }
                self.bind_var(value);
                self.visit_stmt(body);
            }
            StmtKind::FunctionDecl(f) => {
                self.record(f.name.span, Resolved::Function(f.fq_name().to_string()));
                walk_stmt(self, stmt);
            }
            StmtKind::Global(vars) => {
                for var in vars {
                    match var {
                        GlobalVar::Name(name) => self.record_var_self(name.span, &name.name),
                        GlobalVar::Dynamic(e) => self.visit_expr(e),
                    }
                }
            }
            StmtKind::StaticVar(vars) => {
                for (name, default) in vars {
                    if let Some(e) = default {
                        self.visit_expr(e);
                    }
                    self.record_var_self(name.span, &name.name);
                }
            }
            _ => walk_stmt(self, stmt),
        })
    }

    fn visit_catch_clause(&mut self, clause: &'ast CatchClause) {
        for name in &clause.types {
            self.record_class(name);
        }
        if let Some(var) = &clause.var {
            self.record_var_self(var.span, &var.name);
        }
        walk_catch_clause(self, clause);
    }

    /// Resolves a compound type-hint (`?Foo`, `Foo|Bar`) to whichever
    /// single class name inside it the cursor lands on, if any. Under
    /// `Anchor::All`, every class name in the hint gets its own entry.
    fn visit_type_hint(&mut self, hint: &'ast TypeHint) {
        if !self.anchor.matches(hint.span) {
            return;
        }
        for name in hint.class_names() {
            self.record_class(name);
        }
    }

    fn visit_param(&mut self, param: &'ast Param) {
        for attr in &param.attrs {
            self.visit_attribute(attr);
        }
        if let Some(hint) = &param.type_hint {
            self.visit_type_hint(hint);
        }
        self.record_var_self(param.name.span, &param.name.name);
        if let Some(default) = &param.default {
            self.visit_expr(default);
        }
        if let Some(hooks) = &param.hooks {
            self.visit_property_hooks(hooks);
        }
    }

    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
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
        walk_function_decl(self, function);
        self.current_function = previous_function;
        self.scope_frames = previous_scope_frames;
        self.base_scope_id = previous_base_scope_id;
    }

    fn visit_class_decl(&mut self, class: &'ast ClassDecl) {
        let fqn = class_decl_fqn(class).to_string();
        let previous = self.current_class.replace(fqn.clone());
        self.record(class.name_span, Resolved::Class(fqn));
        for name in class
            .extends
            .iter()
            .chain(&class.implements)
            .chain(class.used_traits())
        {
            self.record_class(name);
        }
        walk_class_decl(self, class);
        self.current_class = previous;
    }

    fn visit_method_decl(&mut self, method: &'ast MethodDecl) {
        self.record_member(method.function.name.span, &method.function.name);
        let previous = std::mem::replace(&mut self.current_is_static, method.is_static);
        walk_method_decl(self, method);
        self.current_is_static = previous;
    }

    fn visit_property_decl(&mut self, property: &'ast PropertyDecl) {
        self.record_member(property.name.span, &property.name);
        walk_property_decl(self, property);
    }

    fn visit_class_const(&mut self, constant: &'ast ClassConst) {
        self.record_member(constant.name.span, &constant.name);
        walk_class_const(self, constant);
    }

    fn visit_enum_case(&mut self, case: &'ast EnumCase) {
        self.record_member(case.name.span, &case.name);
        walk_enum_case(self, case);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        syntax::stack::guard(|| {
            match &expr.kind {
                // `foo()`'s callee - a bare `Name` here is always a function
                // reference (never a class), unlike a bare `Name` reached
                // some other way (see the plain `ExprKind::Name` arm below).
                ExprKind::Call(callee, args) if matches!(callee.kind, ExprKind::Name(_)) => {
                    let ExprKind::Name(n) = &callee.kind else {
                        unreachable!()
                    };
                    let fqn = self.registry.called_function_name(n).to_string();
                    self.record(callee.span, Resolved::Function(fqn));
                    walk_args(self, args);
                    return;
                }
                // `new Foo(...)`'s callee - always a class reference, the
                // mirror image of `Call` just above.
                ExprKind::New(callee, args) if matches!(callee.kind, ExprKind::Name(_)) => {
                    let ExprKind::Name(n) = &callee.kind else {
                        unreachable!()
                    };
                    self.record_class(n);
                    walk_args(self, args);
                    return;
                }
                // A bare `Name` reached any *other* way (a first-class
                // callable target `foo(...)`, ...) - neither `Call` nor
                // `New` disambiguated it, so ask the registry which one
                // actually exists, preferring a function (the far more
                // common shape for a standalone name reference).
                ExprKind::Name(n) => {
                    let resolved = match self.registry.resolve_function(n) {
                        Some((fqn, _)) => Some(Resolved::Function(fqn.to_string())),
                        None => self.class_ref(n).map(Resolved::Class),
                    };
                    if let Some(resolved) = resolved {
                        self.record(expr.span, resolved);
                    }
                }
                ExprKind::ClassConstFetch(StaticTarget::Name(class), member) => {
                    self.record_static_member(expr.span, class, member);
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
                    self.record_static_member(expr.span, class, member);
                }
                ExprKind::StaticCall(StaticTarget::Expr(receiver), member, args) => {
                    self.visit_expr(receiver);
                    let instance = self.instance(receiver.span, member);
                    self.record(member.span, instance);
                    walk_args(self, args);
                    return;
                }
                ExprKind::Instanceof(_, InstanceofTarget::Name(name)) => {
                    self.record_class(name);
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
                    self.record_var_read(expr.span, name);
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
                    self.visit_expr(recv);
                    self.record_receiver_member(recv, method);
                    walk_args(self, args);
                    return;
                }
                ExprKind::PropertyAccess(recv, property, _) => {
                    self.visit_expr(recv);
                    self.record_receiver_member(recv, property);
                    return;
                }
                ExprKind::Closure {
                    params, uses, body, ..
                } => {
                    for p in params {
                        self.visit_param(p);
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
                    for u in uses {
                        self.record_var_read(u.name.span, &u.name.name);
                    }
                    let id = self.next_id();
                    self.scope_frames.push(ScopeFrame::Closure {
                        id,
                        transparent: uses.iter().map(|u| u.name.name.clone()).collect(),
                    });
                    walk_stmts(self, body);
                    self.scope_frames.pop();
                    return;
                }
                ExprKind::ArrowFn { params, body, .. } => {
                    for p in params {
                        self.visit_param(p);
                    }
                    let id = self.next_id();
                    self.scope_frames.push(ScopeFrame::ArrowFn {
                        id,
                        owns: params.iter().map(|p| p.name.name.clone()).collect(),
                    });
                    self.visit_expr(body);
                    self.scope_frames.pop();
                    return;
                }
                _ => {}
            }
            walk_expr(self, expr);
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
    fn record_receiver_member(&mut self, recv: &Expr, member: &Ident) {
        let value = match &self.current_class {
            Some(owner) if matches!(&recv.kind, ExprKind::Variable(v) if *v == "this") => {
                Resolved::Member(owner.clone(), member.to_string())
            }
            _ => self.instance(recv.span, member),
        };
        self.record(member.span, value);
    }
}
