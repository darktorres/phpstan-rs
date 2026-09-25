//! `Project::check_class_shapes`: the opt-in "closed by default" class
//! shape (`check_closed_classes`) and `[[parent_calls]]` requirements.
//!
//! Closed by default: a concrete class should be final, a final class's
//! protected members private, and an abstract class's concrete methods
//! final - except where the project itself relies on the opposite (a
//! class something extends, a member a subclass overrides or inherits
//! as protected, a Doctrine entity its proxies subclass).

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;

use crate::parent_calls::{HasContent, Invocation, ParentCallRule};
use syntax::ast::*;
use syntax::token::Span;
use syntax::visit::{any_child_expr, child_stmts, stmt_exprs};

use super::Project;
use crate::diagnostic::{Diagnostic, DiagnosticBuilder};
use syntax::name_resolution::NameContext;

const DOCTRINE_MAPPED: [&str; 3] = ["Entity", "Embeddable", "MappedSuperclass"];

fn collect_classes<'a>(
    stmts: &'a [Stmt],
    ctx: &mut NameContext,
    out: &mut Vec<(String, &'a ClassDecl)>,
) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Namespace(name) => ctx.set_namespace(name),
            StmtKind::Use(imports) => ctx.add_uses(imports),
            StmtKind::ClassDecl(c) => out.push((ctx.declared_name(&c.name), c)),
            StmtKind::Block(body) => collect_classes(body, ctx, out),
            _ => {}
        }
    }
}

fn is_doctrine_mapped(c: &ClassDecl, ctx_names: &[String]) -> bool {
    let doc = c.doc_comment.as_deref().unwrap_or("");
    DOCTRINE_MAPPED.iter().any(|name| {
        doc.contains(&format!("@ORM\\{name}"))
            || doc.contains(&format!("@{name}"))
            || ctx_names.iter().any(|a| {
                a.rsplit('\\')
                    .next()
                    .is_some_and(|short| short.eq_ignore_ascii_case(name))
            })
    })
}

/// Whether `e` is `parent::method(...)`.
fn is_parent_call(e: &Expr, method: &str) -> bool {
    matches!(
        &e.kind,
        ExprKind::StaticCall(StaticTarget::Name(target, _), called, _)
            if target.eq_ignore_ascii_case("parent") && called.eq_ignore_ascii_case(method)
    )
}

/// Whether statement `stmt` is (or returns) `parent::method(...)`.
fn stmt_is_parent_call(stmt: &Stmt, method: &str) -> bool {
    match &stmt.kind {
        StmtKind::Expression(e) => is_parent_call(e, method),
        StmtKind::Return(Some(e)) => is_parent_call(e, method),
        _ => false,
    }
}

fn contains_parent_call(stmts: &[Stmt], method: &str) -> bool {
    stmts
        .iter()
        .any(|stmt| stmt_contains_parent_call(stmt, method))
}

fn stmt_contains_parent_call(stmt: &Stmt, method: &str) -> bool {
    fn in_expr(e: &Expr, method: &str) -> bool {
        is_parent_call(e, method) || any_child_expr(e, &mut |child| in_expr(child, method))
    }
    stmt_exprs(stmt).into_iter().any(|e| in_expr(e, method))
        || child_stmts(stmt)
            .into_iter()
            .any(|child| stmt_contains_parent_call(child, method))
}

fn diagnostic(label: &str, message: String, span: Span, level: u8, id: &str) -> DiagnosticBuilder {
    DiagnosticBuilder::new(label.to_string(), message, span, level, id)
}

impl Project {
    /// Every closed-class-shape (`closed_classes`) and parent-call
    /// (`parent_calls`) finding among the analysed classes.
    pub fn check_class_shapes(
        &self,
        closed_classes: bool,
        parent_calls: &[ParentCallRule],
    ) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        let mut out: FxHashMap<PathBuf, Vec<Diagnostic>> = FxHashMap::default();
        if !closed_classes && parent_calls.is_empty() {
            return out;
        }
        let mut subclassed: FxHashSet<String> = FxHashSet::default();
        let mut overridden: FxHashSet<(String, String)> = FxHashSet::default();
        for info in self.registry.classes.values() {
            if let Some(parent) = info.extends.first() {
                subclassed.insert(parent.to_ascii_lowercase());
            }
            let mut ancestor = info.extends.first().cloned();
            let mut seen = FxHashSet::default();
            while let Some(a) = ancestor {
                if !seen.insert(a.to_ascii_lowercase()) {
                    break;
                }
                for method in info.methods.keys() {
                    overridden.insert((a.to_ascii_lowercase(), method.to_ascii_lowercase()));
                }
                ancestor = self
                    .registry
                    .classes
                    .get(&a)
                    .and_then(|i| i.extends.first().cloned());
            }
        }

        let mut files: Vec<(&PathBuf, &super::FileState)> =
            self.files.iter().filter(|(_, f)| f.analyzed).collect();
        files.sort_by(|a, b| a.0.cmp(b.0));
        for (path, file) in files {
            let mut classes = Vec::new();
            collect_classes(&file.ast, &mut NameContext::default(), &mut classes);
            let mut found = Vec::new();
            for (fqn, c) in classes {
                if closed_classes {
                    self.closed_class_shape(&fqn, c, &subclassed, &overridden, &mut found);
                }
                if !parent_calls.is_empty() {
                    self.parent_call_rules(&fqn, c, parent_calls, &mut found);
                }
            }
            if !found.is_empty() {
                found.sort_by_key(|d| (d.span.start.line, d.span.start.col));
                out.insert(path.clone(), found);
            }
        }
        out
    }

    fn closed_class_shape(
        &self,
        fqn: &str,
        c: &ClassDecl,
        subclassed: &FxHashSet<String>,
        overridden: &FxHashSet<(String, String)>,
        out: &mut Vec<Diagnostic>,
    ) {
        if c.kind != ClassKind::Class {
            return;
        }
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let attribute_names: Vec<String> =
            c.attrs.iter().map(|a| a.name.written.to_string()).collect();
        let lower = fqn.to_ascii_lowercase();
        if !info.is_abstract
            && !info.is_final
            && !subclassed.contains(&lower)
            && !is_doctrine_mapped(c, &attribute_names)
        {
            out.push(
                diagnostic(
                    fqn,
                    format!("Class {fqn} is not final."),
                    c.name_span,
                    0,
                    "class.notFinal",
                )
                .build(),
            );
        }
        if info.is_final {
            let inherited = |member: &str, of_method: bool| {
                info.extends.first().is_some_and(|parent| {
                    if of_method {
                        self.registry.lookup_method(parent, member).is_some()
                    } else {
                        self.registry.lookup_property(parent, member).is_some()
                    }
                }) || info
                    .implements
                    .iter()
                    .any(|i| of_method && self.registry.lookup_method(i, member).is_some())
            };
            for m in &c.methods {
                if m.visibility == Visibility::Protected && !inherited(&m.function.name, true) {
                    out.push(
                        diagnostic(
                            fqn,
                            format!(
                                "Method {fqn}::{}() is protected, but since the containing class is final, it can be private.",
                                m.function.name
                            ),
                            m.function.name_span,
                            0,
                            "method.protectedInFinalClass",
                        )
                        .build(),
                    );
                }
            }
            for p in &c.properties {
                if p.visibility == Visibility::Protected && !inherited(&p.name, false) {
                    out.push(
                        diagnostic(
                            fqn,
                            format!(
                                "Property {fqn}::${} is protected, but since the containing class is final, it can be private.",
                                p.name
                            ),
                            p.name_span,
                            0,
                            "property.protectedInFinalClass",
                        )
                        .build(),
                    );
                }
            }
            for k in &c.consts {
                let from_parent = info
                    .extends
                    .first()
                    .is_some_and(|parent| self.registry.lookup_const(parent, &k.name).is_some());
                if k.visibility == Visibility::Protected && !from_parent {
                    out.push(
                        diagnostic(
                            fqn,
                            format!(
                                "Constant {fqn}::{} is protected, but since the containing class is final, it can be private.",
                                k.name
                            ),
                            k.name_span,
                            0,
                            "classConstant.protectedInFinalClass",
                        )
                        .build(),
                    );
                }
            }
        }
        if info.is_abstract {
            for m in &c.methods {
                let name = m.function.name.as_ref();
                if m.is_abstract
                    || m.is_final
                    || m.visibility == Visibility::Private
                    || name.eq_ignore_ascii_case("__construct")
                    || overridden.contains(&(lower.clone(), name.to_ascii_lowercase()))
                {
                    continue;
                }
                out.push(
                    diagnostic(
                        fqn,
                        format!(
                            "Method {fqn}::{name}() is not final, but since the containing class is abstract, it should be."
                        ),
                        m.function.name_span,
                        0,
                        "method.notFinalInAbstractClass",
                    )
                    .build(),
                );
            }
        }
    }

    fn parent_call_rules(
        &self,
        fqn: &str,
        c: &ClassDecl,
        rules: &[ParentCallRule],
        out: &mut Vec<Diagnostic>,
    ) {
        for m in &c.methods {
            let f = &m.function;
            if !f.has_body {
                continue;
            }
            for rule in rules
                .iter()
                .filter(|r| r.method.eq_ignore_ascii_case(&f.name))
            {
                if let Some(class) = &rule.class {
                    let class = class.trim_start_matches('\\');
                    if fqn.eq_ignore_ascii_case(class) || !self.registry.is_subclass_of(fqn, class)
                    {
                        continue;
                    }
                }
                let Some((owner, parent_sig)) = self
                    .registry
                    .lookup_ancestor_method_with_owner(fqn, &f.name)
                else {
                    continue;
                };
                let parent_does_something = parent_sig.body.as_ref().is_some_and(|b| !b.is_empty());
                let call = format!("parent::{}()", f.name);
                let body = &f.body;
                let found = contains_parent_call(body, &f.name);
                let label = format!("{fqn}::{}", f.name);
                let mut report = |message: String, id: &str, fix: Option<(Span, String)>| {
                    let mut builder = diagnostic(&label, message, f.name_span, 2, id);
                    if let Some((span, text)) = fix {
                        builder = builder.fix(span, text, format!("Call {call}"));
                    }
                    out.push(builder.build());
                };
                if rule.invocation == Invocation::Never {
                    if found {
                        report(
                            format!("Method {label}() calls {call}, which it must not."),
                            "parentCall.forbidden",
                            None,
                        );
                    }
                    continue;
                }
                if !parent_does_something {
                    continue;
                }
                let needs_arguments = parent_sig
                    .params
                    .iter()
                    .any(|p| !p.has_default && !p.variadic);
                let call_text = format!("parent::{}();", f.name);
                if !found {
                    let fix = if needs_arguments {
                        None
                    } else {
                        insertion(
                            f,
                            m.span.start.col,
                            &call_text,
                            rule.invocation == Invocation::Last,
                        )
                    };
                    report(
                        format!(
                            "Method {label}() doesn't call {call}, which {owner}::{}() implements.",
                            f.name
                        ),
                        "parentCall.missing",
                        fix,
                    );
                    continue;
                }
                match rule.invocation {
                    Invocation::First
                        if !body
                            .first()
                            .is_some_and(|s| stmt_is_parent_call(s, &f.name)) =>
                    {
                        report(
                            format!("Method {label}() should call {call} first."),
                            "parentCall.notFirst",
                            None,
                        )
                    }
                    Invocation::Last
                        if !body.last().is_some_and(|s| stmt_is_parent_call(s, &f.name)) =>
                    {
                        report(
                            format!("Method {label}() should call {call} last."),
                            "parentCall.notLast",
                            None,
                        )
                    }
                    _ => {}
                }
                let only_the_call = body.len() == 1 && stmt_is_parent_call(&body[0], &f.name);
                match rule.has_content {
                    HasContent::Yes if only_the_call => report(
                        format!(
                            "Method {label}() only calls {call}; the override does nothing else."
                        ),
                        "parentCall.noContent",
                        None,
                    ),
                    HasContent::No if !only_the_call => report(
                        format!("Method {label}() should do nothing besides calling {call}."),
                        "parentCall.extraContent",
                        None,
                    ),
                    _ => {}
                }
            }
        }
    }
}

/// Where to insert `call` so it becomes `f`'s first (or last) statement,
/// at the body's indentation (an empty body is rewritten onto its own
/// lines, indented from the member's column `member_col`); `None` for a
/// last call after a trailing `return`, which nothing can follow.
fn insertion(f: &FunctionDecl, member_col: u32, call: &str, last: bool) -> Option<(Span, String)> {
    if last {
        let tail = f.body.last()?;
        if matches!(tail.kind, StmtKind::Return(_)) {
            return None;
        }
        let at = tail.span.end;
        let indent = " ".repeat(tail.span.start.col.saturating_sub(1) as usize);
        return Some((Span { start: at, end: at }, format!("\n{indent}{call}")));
    }
    if let Some(first) = f.body.first() {
        let at = first.span.start;
        let indent = " ".repeat(at.col.saturating_sub(1) as usize);
        return Some((Span { start: at, end: at }, format!("{call}\n{indent}")));
    }
    let open = f.body_open?;
    let close = f.span.end;
    let indent = member_col.saturating_sub(1) as usize;
    Some((
        Span {
            start: open.start,
            end: close,
        },
        format!(
            "{{\n{}{call}\n{}}}",
            " ".repeat(indent + 4),
            " ".repeat(indent)
        ),
    ))
}
