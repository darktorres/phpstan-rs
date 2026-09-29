//! `textDocument/signatureHelp` - an active-parameter tooltip while
//! typing inside a call's parens. A `Visitor` over the file: at each
//! call-shaped `Expr` it asks whether the cursor is inside this call's own
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
use syntax::ast::{Arg, ClassDecl, Expr, ExprKind, FunctionDecl, MethodDecl, Stmt};
use syntax::token::{Position, Span};
use syntax::visit::{
    Visitor, walk_args, walk_class_decl, walk_expr, walk_function_decl, walk_method_decl,
    walk_stmts,
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

pub(crate) fn resolve_at(project: &Project, path: &Path, pos: Position) -> Option<SigHelp> {
    let stmts = project.ast(path)?;
    let mut w = Walker {
        registry: project.registry(),
        pos,
        top_level_stmts: stmts,
        current_function: None,
        current_is_static: false,
        current_class: None,
        best: None,
    };
    walk_stmts(&mut w, stmts);
    w.best
}

/// The number of arguments whose own span ends strictly before `pos`,
/// clamped to the last real parameter index for a non-variadic
/// signature (typing past the last declared parameter still highlights
/// it, matching every other LSP server's convention, rather than
/// reporting an out-of-range index) - `lsp.rs`'s own response-building
/// step is what actually knows `param_count` and does the clamping
/// (this function only counts).
fn active_parameter(args: &[Arg], pos: Position) -> usize {
    args.iter()
        .filter(|a| {
            let end = a.value.span.end;
            (end.line, end.col) < (pos.line, pos.col)
        })
        .count()
}

struct Walker<'a> {
    registry: &'a Registry,
    pos: Position,
    top_level_stmts: &'a [Stmt],
    current_function: Option<FunctionDecl>,
    current_is_static: bool,
    /// The class whose body the walk is in; an anonymous class's
    /// constructor arguments are in the enclosing one.
    current_class: Option<String>,
    /// The innermost call around `pos` so far: a later, nested call
    /// replaces it, and a call with no signature to offer clears it.
    best: Option<SigHelp>,
}

impl<'a> Walker<'a> {
    fn probe_scope(&self) -> ProbeScope {
        match &self.current_function {
            Some(f) => ProbeScope::Function(Box::new(f.clone())),
            None => ProbeScope::TopLevel(self.top_level_stmts.to_vec()),
        }
    }

    /// Offers `target` for a call spanning `span` whose arguments are
    /// `args`, once the cursor is past `boundary`: the callee or member
    /// name. A call around the cursor with no target clears the offer.
    fn offer(&mut self, span: Span, boundary: Position, args: &[Arg], target: Option<Target>) {
        if !contains(span, self.pos) {
            return;
        }
        self.best = target
            .filter(|_| self.pos_past(boundary))
            .map(|target| SigHelp {
                target,
                active_parameter: active_parameter(args, self.pos),
            });
    }

    /// The args-region gate (see this module's own doc comment) -
    /// `boundary` is a callee/member sub-span's own end position; the
    /// cursor must be at or past it, not merely somewhere inside the
    /// call's whole span, before a signature is actually offered.
    fn pos_past(&self, boundary: Position) -> bool {
        (boundary.line, boundary.col) <= (self.pos.line, self.pos.col)
    }
}

impl<'ast> Visitor<'ast> for Walker<'_> {
    fn visit_class_decl(&mut self, class: &'ast ClassDecl) {
        let previous = self.current_class.replace(class.fq_name().to_string());
        walk_class_decl(self, class);
        self.current_class = previous;
    }

    fn visit_method_decl(&mut self, method: &'ast MethodDecl) {
        let previous = std::mem::replace(&mut self.current_is_static, method.is_static);
        walk_method_decl(self, method);
        self.current_is_static = previous;
    }

    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
        let previous = self.current_function.replace(function.clone());
        walk_function_decl(self, function);
        self.current_function = previous;
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        syntax::stack::guard(|| {
            match &expr.kind {
                ExprKind::Call(callee, args) => {
                    let target = match &callee.kind {
                        ExprKind::Name(n) => self
                            .registry
                            .resolve_function(n)
                            .map(|(fqn, _)| Target::Function(fqn.to_string())),
                        _ => None,
                    };
                    self.offer(expr.span, callee.span.end, args, target);
                }
                ExprKind::New(callee, args) => {
                    let target = match &callee.kind {
                        ExprKind::Name(n) => {
                            resolve_class_ref(n, self.current_class.as_deref(), self.registry)
                                .filter(|fqn| {
                                    self.registry
                                        .lookup_method_with_owner(fqn, "__construct")
                                        .is_some()
                                })
                                .map(Target::Constructor)
                        }
                        _ => None,
                    };
                    self.offer(expr.span, callee.span.end, args, target);
                }
                ExprKind::MethodCall(recv, method, args, _) => {
                    let target = match &self.current_class {
                        Some(owner) if matches!(&recv.kind, ExprKind::Variable(v) if *v == "this") => {
                            Target::Member(owner.clone(), method.to_string())
                        }
                        _ => Target::Instance {
                            receiver_span: recv.span,
                            member: method.to_string(),
                            scope: self.probe_scope(),
                            is_static: self.current_is_static,
                            class: self.current_class.clone(),
                        },
                    };
                    self.offer(expr.span, method.span.end, args, Some(target));
                }
                // A dynamic name has no signature to offer.
                ExprKind::DynamicStaticCall(_, _, args)
                | ExprKind::DynamicMethodCall(_, _, args, _) => {
                    self.offer(expr.span, expr.span.start, args, None);
                }
                ExprKind::NewAnonymousClass(class) => {
                    let target = class
                        .name
                        .as_ref()
                        .filter(|name| {
                            self.registry
                                .lookup_method_with_owner(name, "__construct")
                                .is_some()
                        })
                        .map(|name| Target::Constructor(name.to_string()));
                    self.offer(expr.span, expr.span.start, &class.args, target);
                    walk_args(self, &class.args);
                    let previous = std::mem::replace(
                        &mut self.current_class,
                        class.name.as_ref().map(|name| name.to_string()),
                    );
                    self.visit_class_def(&class.def);
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
                    self.offer(expr.span, member.span.end, args, target);
                }
                ExprKind::FirstClassCallable(_) => return,
                _ => {}
            }
            walk_expr(self, expr);
        })
    }
}

#[cfg(test)]
mod tests;
