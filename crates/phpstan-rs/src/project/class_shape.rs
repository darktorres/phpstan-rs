//! `Project::check_class_shapes`: the opt-in "closed by default" class
//! shape (`check_closed_classes`) and `[[parent_calls]]` requirements.
//!
//! Closed by default: a concrete class should be final, a final class's
//! protected members private, and an abstract class's concrete methods
//! final - except where the project itself relies on the opposite (a
//! class something extends, a member a subclass overrides or inherits
//! as protected, a Doctrine entity its proxies subclass).

use rustc_hash::{FxHashMap, FxHashSet};

use crate::file_id::FileId;
use crate::parent_calls::{HasContent, Invocation, ParentCallRule};
use syntax::ast::*;
use syntax::span::{LineIndex, Span};
use syntax::visit::any_expr_in_body;

use super::Project;
use diagnostics::{Diagnostic, DiagnosticBuilder};

const DOCTRINE_MAPPED: [&str; 3] = ["Entity", "Embeddable", "MappedSuperclass"];

/// Every class, interface and trait `stmts` declares unconditionally, by
/// its fully-qualified name. An enum is final and has no properties or
/// subclasses.
///
/// A declaration is unconditional where the file runs it as it is read:
/// at its top level, in a bare block, in the body of a `declare`. Nothing
/// declared inside an expression, a function, a loop or a branch is, so
/// these statements are all there is to look through - not every
/// expression of every body, as `for_each_declaration` does.
pub(super) fn collect_classes<'a>(stmts: &'a [Stmt], out: &mut Vec<(String, &'a ClassDecl)>) {
    for stmt in stmts {
        stack::guard(|| match &stmt.kind {
            StmtKind::ClassDecl(c) if c.def.kind != ClassKind::Enum => {
                out.push((c.fq_name().to_string(), c));
            }
            StmtKind::Block(inner)
            | StmtKind::Declare {
                body: Some(inner), ..
            } => collect_classes(inner, out),
            _ => {}
        });
    }
}

fn is_doctrine_mapped(c: &ClassDef, ctx_names: &[String]) -> bool {
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
        ExprKind::StaticCall(StaticTarget::Name(target), called, _)
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
    any_expr_in_body(stmts, |e| is_parent_call(e, method))
}

fn diagnostic(message: String, span: Span, level: u8, id: &'static str) -> DiagnosticBuilder {
    DiagnosticBuilder::new(message, span, level, id)
}

impl Project {
    /// Every closed-class-shape (`closed_classes`) and parent-call
    /// (`parent_calls`) finding among the analysed classes.
    pub fn check_class_shapes(
        &self,
        closed_classes: bool,
        parent_calls: &[ParentCallRule],
    ) -> FxHashMap<FileId, Vec<Diagnostic>> {
        let mut out: FxHashMap<FileId, Vec<Diagnostic>> = FxHashMap::default();
        if !closed_classes && parent_calls.is_empty() {
            return out;
        }
        let mut files: Vec<(FileId, &super::FileState)> = self
            .files
            .iter_with_ids()
            .filter(|(_, _, f)| f.analyzed)
            .map(|(id, _, f)| (id, f))
            .collect();
        files.sort_by_key(|&(id, _)| self.files.rank_of(id));
        for (id, file) in files {
            let Some(ast) = self.ast_of(id) else {
                continue;
            };
            let mut classes = Vec::new();
            collect_classes(&ast, &mut classes);
            let mut found = Vec::new();
            for (fqn, c) in classes {
                if closed_classes {
                    self.closed_class_shape(&fqn, c, &file.line_index, &mut found);
                }
                if !parent_calls.is_empty() {
                    self.parent_call_rules(
                        &fqn,
                        &c.def,
                        parent_calls,
                        &file.line_index,
                        &mut found,
                    );
                }
            }
            if !found.is_empty() {
                found.sort_by_key(|d| (d.span.start.line, d.span.start.col));
                out.insert(id, found);
            }
        }
        out
    }

    /// The classes extending `class` through their first parent,
    /// directly or transitively. Only the subtypes the registry indexes
    /// are read, not every class there is.
    fn extending_classes(&self, class: &str) -> Vec<&crate::registry::ClassReflection> {
        let mut seen: FxHashSet<String> = FxHashSet::default();
        seen.insert(class.to_ascii_lowercase());
        let mut found = Vec::new();
        let mut stack = vec![class.to_ascii_lowercase()];
        while let Some(current) = stack.pop() {
            for child in self
                .registry
                .subtype_children
                .get(&current)
                .into_iter()
                .flatten()
            {
                let Some(info) = self.registry.classes.get(child) else {
                    continue;
                };
                let extends_current = info
                    .extends()
                    .first()
                    .is_some_and(|parent| parent.eq_ignore_ascii_case(&current));
                if extends_current && seen.insert(child.to_ascii_lowercase()) {
                    found.push(info);
                    stack.push(child.to_ascii_lowercase());
                }
            }
        }
        found
    }

    /// Whether some class extends `lower` directly.
    fn is_extended(&self, lower: &str) -> bool {
        self.registry
            .subtype_children
            .get(lower)
            .into_iter()
            .flatten()
            .any(|child| {
                self.registry.classes.get(child).is_some_and(|info| {
                    info.extends()
                        .first()
                        .is_some_and(|parent| parent.eq_ignore_ascii_case(lower))
                })
            })
    }

    /// Whether a class below `lower` declares `method`.
    fn is_overridden(&self, lower: &str, method: &str) -> bool {
        self.extending_classes(lower).iter().any(|info| {
            info.methods()
                .keys()
                .any(|name| name.eq_ignore_ascii_case(method))
        })
    }

    fn closed_class_shape(
        &self,
        fqn: &str,
        c: &ClassDecl,
        index: &LineIndex,
        out: &mut Vec<Diagnostic>,
    ) {
        if c.def.kind != ClassKind::Class {
            return;
        }
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let attribute_names: Vec<String> = c
            .def
            .attrs
            .iter()
            .map(|a| a.name.written.to_string())
            .collect();
        let lower = fqn.to_ascii_lowercase();
        if !info.is_abstract()
            && !info.is_final()
            && !self.is_extended(&lower)
            && !is_doctrine_mapped(&c.def, &attribute_names)
        {
            out.push(
                diagnostic(
                    format!("Class {fqn} is not final."),
                    c.name.span,
                    0,
                    "class.notFinal",
                )
                .build(index),
            );
        }
        if info.is_final() {
            let inherited = |member: &str, of_method: bool| {
                info.extends().first().is_some_and(|parent| {
                    if of_method {
                        self.registry.lookup_method(parent, member).is_some()
                    } else {
                        self.registry.lookup_property(parent, member).is_some()
                    }
                }) || info
                    .implements()
                    .iter()
                    .any(|i| of_method && self.registry.lookup_method(i, member).is_some())
            };
            for m in &c.def.methods {
                if m.visibility == Visibility::Protected && !inherited(&m.function.name, true) {
                    out.push(
                        diagnostic(
                            format!(
                                "Method {fqn}::{}() is protected, but since the containing class is final, it can be private.",
                                m.function.name
                            ),
                            m.function.name.span,
                            0,
                            "method.protectedInFinalClass",
                        )
                        .build(index),
                    );
                }
            }
            for p in &c.def.properties {
                if p.visibility == Visibility::Protected && !inherited(&p.name, false) {
                    out.push(
                        diagnostic(
                            format!(
                                "Property {fqn}::${} is protected, but since the containing class is final, it can be private.",
                                p.name
                            ),
                            p.name.span,
                            0,
                            "property.protectedInFinalClass",
                        )
                        .build(index),
                    );
                }
            }
            for k in &c.def.consts {
                let from_parent = info
                    .extends()
                    .first()
                    .is_some_and(|parent| self.registry.lookup_const(parent, &k.name).is_some());
                if k.visibility == Visibility::Protected && !from_parent {
                    out.push(
                        diagnostic(
                            format!(
                                "Constant {fqn}::{} is protected, but since the containing class is final, it can be private.",
                                k.name
                            ),
                            k.name.span,
                            0,
                            "classConstant.protectedInFinalClass",
                        )
                        .build(index),
                    );
                }
            }
        }
        if info.is_abstract() {
            for m in &c.def.methods {
                let name = m.function.name.as_ref();
                if m.is_abstract
                    || m.is_final
                    || m.visibility == Visibility::Private
                    || syntax::ast::is_constructor_name(name)
                    || self.is_overridden(&lower, name)
                {
                    continue;
                }
                out.push(
                    diagnostic(
                        format!(
                            "Method {fqn}::{name}() is not final, but since the containing class is abstract, it should be."
                        ),
                        m.function.name.span,
                        0,
                        "method.notFinalInAbstractClass",
                    )
                    .build(index),
                );
            }
        }
    }

    fn parent_call_rules(
        &self,
        fqn: &str,
        c: &ClassDef,
        rules: &[ParentCallRule],
        index: &LineIndex,
        out: &mut Vec<Diagnostic>,
    ) {
        for m in &c.methods {
            let f = &m.function;
            if f.body.is_none() {
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
                let parent_does_something = self
                    .bodies()
                    .summary(parent_sig.provenance())
                    .is_some_and(|summary| !summary.empty);
                let call = format!("parent::{}()", f.name);
                let body = f.body_stmts();
                let found = contains_parent_call(body, &f.name);
                let label = format!("{fqn}::{}", f.name);
                let mut report =
                    |message: String, id: &'static str, fix: Option<(Span, String)>| {
                        let mut builder = diagnostic(message, f.name.span, 2, id);
                        if let Some((span, text)) = fix {
                            builder = builder.fix(span, text, format!("Call {call}"));
                        }
                        out.push(builder.build(index));
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
                    .params()
                    .iter()
                    .any(|p| !p.has_default() && !p.variadic());
                let call_text = format!("parent::{}();", f.name);
                if !found {
                    let fix = if needs_arguments {
                        None
                    } else {
                        insertion(
                            f,
                            index,
                            index.line_col(m.span.start).col,
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
fn insertion(
    f: &FunctionDecl,
    index: &LineIndex,
    member_col: u32,
    call: &str,
    last: bool,
) -> Option<(Span, String)> {
    let line_break = index.line_break().as_str();
    if last {
        let tail = f.body_stmts().last()?;
        if matches!(tail.kind, StmtKind::Return(_)) {
            return None;
        }
        let at = tail.span.end;
        let indent = " ".repeat(index.line_col(tail.span.start).col.saturating_sub(1) as usize);
        return Some((
            Span { start: at, end: at },
            format!("{line_break}{indent}{call}"),
        ));
    }
    if let Some(first) = f.body_stmts().first() {
        let at = first.span.start;
        let indent = " ".repeat(index.line_col(at).col.saturating_sub(1) as usize);
        return Some((
            Span { start: at, end: at },
            format!("{call}{line_break}{indent}"),
        ));
    }
    let open = f.body.as_ref()?.open;
    let close = f.span.end;
    let indent = member_col.saturating_sub(1) as usize;
    Some((
        Span {
            start: open.start,
            end: close,
        },
        format!(
            "{{{line_break}{}{call}{line_break}{}}}",
            " ".repeat(indent + 4),
            " ".repeat(indent)
        ),
    ))
}
