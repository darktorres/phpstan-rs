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
use syntax::span::{LineIndex, Span};
use syntax::visit::arena::{self, Visitor};

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
pub(super) fn collect_classes<'a>(
    arenas: &'a NodeArenas,
    stmts: &[StmtId],
    out: &mut Vec<(String, &'a ClassDecl, &'a ClassDef)>,
) {
    for &id in stmts {
        let Some(stmt) = arenas.stmt(id) else {
            continue;
        };
        syntax::stack::guard(|| match &stmt.kind {
            StmtKind::ClassDecl(c) => {
                let c = arenas.class_decl(c.clone());
                if let Some(def) = arenas.class(c.def)
                    && def.kind != ClassKind::Enum
                {
                    out.push((c.fq_name().to_string(), c, def));
                }
            }
            StmtKind::Block(inner) => collect_classes(arenas, arenas.stmt_ids(inner), out),
            StmtKind::Declare {
                body: Some(inner), ..
            } => collect_classes(arenas, arenas.stmt_ids(inner), out),
            _ => {}
        });
    }
}

fn is_doctrine_mapped(arenas: &NodeArenas, c: &ClassDef, ctx_names: &[String]) -> bool {
    let doc = c
        .doc_comment
        .as_ref()
        .and_then(|ranges| arenas.doc_comment_text(ranges));
    let doc = doc.as_deref().unwrap_or("");
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
fn is_parent_call(arenas: &NodeArenas, id: ExprId, method: &str) -> bool {
    matches!(
        arenas.expr(id).map(|e| &e.kind),
        Some(ExprKind::StaticCall(StaticTarget::Name(target), called, _))
            if target.eq_ignore_ascii_case("parent") && called.eq_ignore_ascii_case(method)
    )
}

/// Whether statement `stmt` is (or returns) `parent::method(...)`.
fn stmt_is_parent_call(arenas: &NodeArenas, id: StmtId, method: &str) -> bool {
    match arenas.stmt(id).map(|stmt| &stmt.kind) {
        Some(StmtKind::Expression(e)) => is_parent_call(arenas, *e, method),
        Some(StmtKind::Return(Some(e))) => is_parent_call(arenas, *e, method),
        _ => false,
    }
}

fn contains_parent_call(arenas: &NodeArenas, stmts: &[StmtId], method: &str) -> bool {
    struct ParentCall<'a> {
        method: &'a str,
        found: bool,
    }
    impl Visitor for ParentCall<'_> {
        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            self.found |= is_parent_call(arenas, id, self.method);
            if !self.found {
                arena::walk_expr(self, arenas, id);
            }
        }
    }
    let mut visitor = ParentCall {
        method,
        found: false,
    };
    arena::walk_stmts(&mut visitor, arenas, stmts);
    visitor.found
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
    ) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        let mut out: FxHashMap<PathBuf, Vec<Diagnostic>> = FxHashMap::default();
        if !closed_classes && parent_calls.is_empty() {
            return out;
        }
        let mut files: Vec<(&PathBuf, &super::FileState)> =
            self.files.iter().filter(|(_, f)| f.analyzed).collect();
        files.sort_by(|a, b| a.0.cmp(b.0));
        for (path, file) in files {
            let tree = file
                .tree()
                .expect("analyzed class-shape file must retain its syntax tree");
            let arenas = tree.arenas();
            let mut classes = Vec::new();
            collect_classes(arenas, tree.statements(), &mut classes);
            let mut found = Vec::new();
            for (fqn, c, def) in classes {
                if closed_classes {
                    self.closed_class_shape(arenas, &fqn, c, def, &file.line_index, &mut found);
                }
                if !parent_calls.is_empty() {
                    self.parent_call_rules(
                        arenas,
                        &fqn,
                        def,
                        parent_calls,
                        &file.line_index,
                        &mut found,
                    );
                }
            }
            if !found.is_empty() {
                found.sort_by_key(|d| (d.span.start.line, d.span.start.col));
                out.insert(path.clone(), found);
            }
        }
        out
    }

    /// The classes extending `class` through their first parent,
    /// directly or transitively. Only the subtypes the registry indexes
    /// are read, not every class there is.
    fn extending_classes(&self, class: &str) -> Vec<types::Name> {
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
                if self.registry.class_by_name(child).is_none() {
                    continue;
                }
                let extends_current =
                    self.registry
                        .class_references_by_name(child)
                        .is_some_and(|references| {
                            references
                                .extends
                                .first()
                                .is_some_and(|parent| parent.eq_ignore_ascii_case(&current))
                        });
                if extends_current && seen.insert(child.to_ascii_lowercase()) {
                    found.push(types::Name::from(child.as_str()));
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
                self.registry
                    .class_references_by_name(child)
                    .is_some_and(|references| {
                        references
                            .extends
                            .first()
                            .is_some_and(|parent| parent.eq_ignore_ascii_case(lower))
                    })
            })
    }

    /// Whether a class below `lower` declares `method`.
    fn is_overridden(&self, lower: &str, method: &str) -> bool {
        self.extending_classes(lower).iter().any(|class| {
            self.registry
                .method_names(class)
                .any(|name| name.eq_ignore_ascii_case(method))
        })
    }

    fn closed_class_shape(
        &self,
        arenas: &NodeArenas,
        fqn: &str,
        c: &ClassDecl,
        def: &ClassDef,
        index: &LineIndex,
        out: &mut Vec<Diagnostic>,
    ) {
        if def.kind != ClassKind::Class {
            return;
        }
        let Some(info) = self.registry.class_by_name(fqn) else {
            return;
        };
        let Some(references) = self.registry.class_references_by_name(fqn) else {
            return;
        };
        let attribute_names: Vec<String> = arenas
            .attributes(def.attrs.clone())
            .iter()
            .map(|a| a.name.written.to_string())
            .collect();
        let lower = fqn.to_ascii_lowercase();
        if !info.is_abstract()
            && !info.is_final()
            && !self.is_extended(&lower)
            && !is_doctrine_mapped(arenas, def, &attribute_names)
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
                references.extends.first().is_some_and(|parent| {
                    if of_method {
                        self.registry.lookup_method(parent, member).is_some()
                    } else {
                        self.registry.lookup_property(parent, member).is_some()
                    }
                }) || references
                    .implements
                    .iter()
                    .any(|i| of_method && self.registry.lookup_method(i, member).is_some())
            };
            for m in arenas.methods(def.methods.clone()) {
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
            for p in def.properties(arenas) {
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
            for k in def.consts(arenas) {
                let from_parent = references
                    .extends
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
            for m in arenas.methods(def.methods.clone()) {
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
        arenas: &NodeArenas,
        fqn: &str,
        c: &ClassDef,
        rules: &[ParentCallRule],
        index: &LineIndex,
        out: &mut Vec<Diagnostic>,
    ) {
        for m in arenas.methods(c.methods.clone()) {
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
                let body = f.body_stmts(arenas);
                let found = contains_parent_call(arenas, body, &f.name);
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
                            arenas,
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
                            .is_some_and(|s| stmt_is_parent_call(arenas, *s, &f.name)) =>
                    {
                        report(
                            format!("Method {label}() should call {call} first."),
                            "parentCall.notFirst",
                            None,
                        )
                    }
                    Invocation::Last
                        if !body
                            .last()
                            .is_some_and(|s| stmt_is_parent_call(arenas, *s, &f.name)) =>
                    {
                        report(
                            format!("Method {label}() should call {call} last."),
                            "parentCall.notLast",
                            None,
                        )
                    }
                    _ => {}
                }
                let only_the_call =
                    body.len() == 1 && stmt_is_parent_call(arenas, body[0], &f.name);
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
    arenas: &NodeArenas,
    f: &FunctionDecl,
    index: &LineIndex,
    member_col: u32,
    call: &str,
    last: bool,
) -> Option<(Span, String)> {
    if last {
        let tail_id = *f.body_stmts(arenas).last()?;
        let tail = arenas.stmt(tail_id)?;
        if matches!(tail.kind, StmtKind::Return(_)) {
            return None;
        }
        let span = arenas.stmt_span(tail_id)?;
        let at = span.end;
        let indent = " ".repeat(index.line_col(span.start).col.saturating_sub(1) as usize);
        return Some((Span { start: at, end: at }, format!("\n{indent}{call}")));
    }
    if let Some(first) = f.body_stmts(arenas).first().copied() {
        let at = arenas.stmt_span(first)?.start;
        let indent = " ".repeat(index.line_col(at).col.saturating_sub(1) as usize);
        return Some((Span { start: at, end: at }, format!("{call}\n{indent}")));
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
            "{{\n{}{call}\n{}}}",
            " ".repeat(indent + 4),
            " ".repeat(indent)
        ),
    ))
}
