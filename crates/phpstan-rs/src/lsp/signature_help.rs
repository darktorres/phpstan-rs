//! `textDocument/signatureHelp` - an active-parameter tooltip while
//! typing inside a call's parens. A `Visitor` over the file: at each
//! call-shaped `ExprNode` it asks whether the cursor is inside this call's own
//! argument list, and if so, which argument.
//!
//! **Claim-and-clear, not overwrite-only-on-match.** Every call-shaped
//! node (`Call`, `New`, `MethodCall`, `DynamicMethodCall`, `StaticCall`,
//! `NewAnonymousClass`) runs a three-step protocol every time it's
//! visited: (1) if the cursor is anywhere inside this node's own overall
//! span, unconditionally clear `best` to `None` first; (2) only if the
//! cursor is also past this call's own args-region boundary (see below)
//! AND the callee/member actually resolves, set `best` to `Some`; (3)
//! recurse into children, so a still-more-specific nested match
//! overrides step 1's clear. Skipping step 1 would leak an enclosing
//! resolvable call's signature into an unresolvable or out-of-scope
//! nested construct sitting inside it (a dynamically-called callee, or
//! an anonymous class's own constructor arg gap) - the cursor is
//! genuinely inside *that* inner, non-matching construct, not the outer
//! call, so the right answer is `None`, not a stale outer match.
//!
//! **The args-region gate.** A call's own overall span starts at the
//! callee's first token, so `contains(call.span, pos)` alone is true
//! anywhere from `fo|o(...)` onward, including while the cursor still
//! sits on the function/method/class name itself, before the opening
//! paren - too early to offer a *parameter* tooltip for. Step 2 above
//! additionally requires `pos` to be at or past an args-region boundary,
//! computed per kind from a span the AST gives: `Call`/`New` ->
//! `callee.span.end`; `MethodCall` -> the end of the method name's
//! span; `DynamicMethodCall` ->
//! `name_expr.span.end` (a few characters before the real `(` for the
//! `{$expr}` brace form - narrow, disclosed slack, never a wrong-call
//! leak); `StaticCall` -> the end of the method name's span.

use std::path::Path;

use crate::project::Project;
use crate::registry::Registry;
use syntax::ast::{Arg, ClassDecl, ExprId, ExprKind, FunctionDecl, MethodDecl, NodeArenas, StmtId};
use syntax::span::Span;
use syntax::visit::arena::{
    Visitor, walk_args, walk_class_decl, walk_class_def, walk_expr, walk_function_decl,
    walk_method_decl, walk_stmts,
};

use super::navigation::{ProbeScope, contains, resolve_class_ref};

/// What a resolved signature-help target needs to look its own
/// `FunctionReflection` up (or, for `Instance`, to run the same checker probe
/// `navigation::Resolved::Instance` already uses for hover).
pub(crate) enum Target {
    Function(String),
    /// `new Foo(...)` - resolves via `registry.lookup_method_with_owner
    /// (class, "__construct")`, not a plain function lookup.
    Constructor(String),
    /// `$this->method()`/`self::method()`/`Foo::method()` (a literal,
    /// statically-known owner) - resolves directly, no probe needed.
    Member(String, String),
    /// Any other `->`/`::` receiver - needs the same checker probe
    /// `navigation::Resolved::Instance` already runs for hover; may
    /// resolve to more than one real owner for a `Union`/`Intersection`
    /// receiver, each offered as its own `SignatureInformation`.
    Instance {
        receiver_span: Span,
        member: String,
        scope: ProbeScope,
        is_static: bool,
        class: Option<String>,
    },
}

pub(crate) struct SigHelp {
    pub(crate) target: Target,
    pub(crate) active_parameter: usize,
}

pub(crate) fn resolve_at(project: &Project, path: &Path, pos: u32) -> Option<SigHelp> {
    let ast = project.ast_of(path)?;
    let stmts = ast.statements();
    let arenas = ast.arenas();
    let syntax_owner = ast.parsed_file_for_completion();
    let mut w = Walker {
        registry: project.registry(),
        syntax_owner,
        pos,
        top_level_stmts: stmts.to_vec(),
        current_function: None,
        current_is_static: false,
        current_class: None,
        best: None,
    };
    walk_stmts(&mut w, arenas, stmts);
    w.best
}

/// The number of arguments whose own span ends strictly before `pos`,
/// clamped to the last real parameter index for a non-variadic
/// signature (typing past the last declared parameter still highlights
/// it, matching every other LSP server's convention, rather than
/// reporting an out-of-range index) - `lsp.rs`'s own response-building
/// step is what actually knows `param_count` and does the clamping
/// (this function only counts).
fn active_parameter(arenas: &NodeArenas, args: &[Arg], pos: u32) -> usize {
    args.iter()
        .filter(|arg| {
            arenas
                .expr_span(arg.value)
                .is_some_and(|span| span.end < pos)
        })
        .count()
}

struct Walker<'a> {
    registry: &'a Registry,
    syntax_owner: std::sync::Arc<syntax::ParsedFile>,
    pos: u32,
    top_level_stmts: Vec<StmtId>,
    current_function: Option<FunctionDecl>,
    current_is_static: bool,
    /// The class whose body the walk is in; an anonymous class's
    /// constructor arguments are in the enclosing one.
    current_class: Option<String>,
    /// The innermost call around `pos` so far: a later, nested call
    /// replaces it, and a call with no signature to offer clears it.
    best: Option<SigHelp>,
}

impl Walker<'_> {
    fn probe_scope(&self) -> ProbeScope {
        match &self.current_function {
            Some(f) => ProbeScope::Function {
                function: Box::new(f.clone()),
                owner: Some(std::sync::Arc::clone(&self.syntax_owner)),
            },
            None => ProbeScope::TopLevel {
                statements: self.top_level_stmts.clone(),
                owner: Some(std::sync::Arc::clone(&self.syntax_owner)),
            },
        }
    }

    /// Offers `target` for a call spanning `span` whose arguments are
    /// `args`, once the cursor is past `boundary`: the callee or member
    /// name. A call around the cursor with no target clears the offer.
    fn offer(
        &mut self,
        arenas: &NodeArenas,
        span: Span,
        boundary: u32,
        args: &[Arg],
        target: Option<Target>,
    ) {
        if !contains(span, self.pos) {
            return;
        }
        self.best = target
            .filter(|_| self.pos_past(boundary))
            .map(|target| SigHelp {
                target,
                active_parameter: active_parameter(arenas, args, self.pos),
            });
    }

    /// The args-region gate (see this module's own doc comment) -
    /// `boundary` is a callee/member sub-span's own end position; the
    /// cursor must be at or past it, not merely somewhere inside the
    /// call's whole span, before a signature is actually offered.
    fn pos_past(&self, boundary: u32) -> bool {
        boundary <= self.pos
    }
}

impl Visitor for Walker<'_> {
    fn visit_class_decl(&mut self, arenas: &NodeArenas, class: &ClassDecl) {
        let previous = self.current_class.replace(class.fq_name().to_string());
        walk_class_decl(self, arenas, class);
        self.current_class = previous;
    }

    fn visit_method_decl(&mut self, arenas: &NodeArenas, method: &MethodDecl) {
        let previous = std::mem::replace(&mut self.current_is_static, method.is_static);
        walk_method_decl(self, arenas, method);
        self.current_is_static = previous;
    }

    fn visit_function_decl(&mut self, arenas: &NodeArenas, function: &FunctionDecl) {
        let previous = self.current_function.replace(function.clone());
        walk_function_decl(self, arenas, function);
        self.current_function = previous;
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(expr) = arenas.expr(id) else {
            return;
        };
        let expr_span = arenas
            .expr_span(id)
            .expect("signature-help expression has a parallel span");
        syntax::stack::guard(|| {
            match &expr.kind {
                ExprKind::Call(callee, args) => {
                    let callee_node = arenas.expr(*callee);
                    let target = match callee_node.map(|callee| &callee.kind) {
                        Some(ExprKind::Name(n)) => arenas
                            .qualified_name(n)
                            .and_then(|n| self.registry.resolve_function(n))
                            .map(|(fqn, _)| Target::Function(fqn.to_string())),
                        _ => None,
                    };
                    let boundary = callee_node.map_or(expr_span.start, |callee| {
                        arenas
                            .expr_span(callee.id)
                            .expect("callee expression has a parallel span")
                            .end
                    });
                    self.offer(
                        arenas,
                        expr_span,
                        boundary,
                        arenas.args(args.clone()),
                        target,
                    );
                }
                ExprKind::New(callee, args) => {
                    let callee_node = arenas.expr(*callee);
                    let target = match callee_node.map(|callee| &callee.kind) {
                        Some(ExprKind::Name(n)) => arenas
                            .qualified_name(n)
                            .and_then(|n| {
                                resolve_class_ref(n, self.current_class.as_deref(), self.registry)
                            })
                            .filter(|fqn| {
                                self.registry
                                    .lookup_method_with_owner(fqn, "__construct")
                                    .is_some()
                            })
                            .map(Target::Constructor),
                        _ => None,
                    };
                    let boundary = callee_node.map_or(expr_span.start, |callee| {
                        arenas
                            .expr_span(callee.id)
                            .expect("callee expression has a parallel span")
                            .end
                    });
                    self.offer(
                        arenas,
                        expr_span,
                        boundary,
                        arenas.args(args.clone()),
                        target,
                    );
                }
                ExprKind::MethodCall(recv, method, args, _) => {
                    let recv_node = arenas.expr(*recv);
                    let target = match &self.current_class {
                        Some(owner) if matches!(recv_node.map(|recv| &recv.kind), Some(ExprKind::Variable(v)) if *v == "this") => {
                            Target::Member(owner.clone(), method.to_string())
                        }
                        _ => Target::Instance {
                            receiver_span: recv_node.map_or(expr_span, |recv| {
                                arenas
                                    .expr_span(recv.id)
                                    .expect("receiver expression has a parallel span")
                            }),
                            member: method.to_string(),
                            scope: self.probe_scope(),
                            is_static: self.current_is_static,
                            class: self.current_class.clone(),
                        },
                    };
                    self.offer(
                        arenas,
                        expr_span,
                        method.span.end,
                        arenas.args(args.clone()),
                        Some(target),
                    );
                }
                // A dynamic name has no signature to offer.
                ExprKind::DynamicStaticCall(_, _, args)
                | ExprKind::DynamicMethodCall(_, _, args, _) => {
                    self.offer(
                        arenas,
                        expr_span,
                        expr_span.start,
                        arenas.args(args.clone()),
                        None,
                    );
                }
                ExprKind::NewAnonymousClass(class_id) => {
                    let Some(class) = arenas.anonymous_class(*class_id) else {
                        return;
                    };
                    let target = arenas
                        .anonymous_class_name(*class_id)
                        .filter(|name| {
                            self.registry
                                .lookup_method_with_owner(name, "__construct")
                                .is_some()
                        })
                        .map(|name| Target::Constructor(name.to_string()));
                    self.offer(
                        arenas,
                        expr_span,
                        expr_span.start,
                        arenas.args(class.args.clone()),
                        target,
                    );
                    walk_args(self, arenas, class.args.clone());
                    let previous = std::mem::replace(
                        &mut self.current_class,
                        arenas
                            .anonymous_class_name(*class_id)
                            .map(ToString::to_string),
                    );
                    if let Some(def) = arenas.class(class.def) {
                        walk_class_def(self, arenas, def);
                    }
                    self.current_class = previous;
                    return;
                }
                ExprKind::StaticCall(target, member, args) => {
                    // A dynamic receiver has no class-name text to resolve
                    // here (this pass runs no type probe the way
                    // `navigation.rs` does).
                    let resolved = target.as_name().and_then(|class| {
                        resolve_class_ref(class, self.current_class.as_deref(), self.registry)
                    });
                    let target = resolved.map(|fqn| Target::Member(fqn, member.to_string()));
                    self.offer(
                        arenas,
                        expr_span,
                        member.span.end,
                        arenas.args(args.clone()),
                        target,
                    );
                }
                ExprKind::FirstClassCallable(_) => return,
                _ => {}
            }
            walk_expr(self, arenas, id);
        })
    }
}

#[cfg(test)]
mod tests;
