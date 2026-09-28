//! Statement keywords PHP validates at compile time, per function-like
//! body (and the file's top level): `break`/`continue` levels
//! (`ContinueBreakInLoopRule`, plus a non-positive level and a `continue`
//! that targets a `switch`), `goto`/labels (`GotoUndefinedLabelRule`,
//! `UnusedLabelRule`, plus duplicate labels and jumps into a loop, a
//! `switch`, or a `finally` block, or out of one), and `declare`
//! directives (`DeclareStrictTypesRule`, plus block-mode `strict_types`
//! and unknown directives).

use rustc_hash::{FxHashMap, FxHashSet};

use crate::checker::Checker;
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use syntax::name::Name;
use syntax::token::Span;
use syntax::visit::child_stmts;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Loop,
    Switch,
}

struct Placement {
    span: Span,
    frames: Vec<usize>,
    finallies: Vec<usize>,
}

#[derive(Default)]
struct Scope {
    frames: Vec<(FrameKind, usize)>,
    finallies: Vec<usize>,
    next_id: usize,
    labels: FxHashMap<Name, Placement>,
    duplicate_labels: Vec<(Name, Span)>,
    gotos: Vec<(Name, Placement)>,
    jumps: Vec<(&'static str, Span, String)>,
}

impl Scope {
    fn placement(&self, span: Span) -> Placement {
        Placement {
            span,
            frames: self.frames.iter().map(|(_, id)| *id).collect(),
            finallies: self.finallies.clone(),
        }
    }

    fn fresh_id(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }

    fn walk(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.walk_stmt(stmt);
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => {}
            StmtKind::While { body, .. }
            | StmtKind::DoWhile { body, .. }
            | StmtKind::For { body, .. }
            | StmtKind::Foreach { body, .. } => {
                let id = self.fresh_id();
                self.frames.push((FrameKind::Loop, id));
                self.walk_stmt(body);
                self.frames.pop();
            }
            StmtKind::Switch { cases, .. } => {
                let id = self.fresh_id();
                self.frames.push((FrameKind::Switch, id));
                for case in cases {
                    self.walk(&case.body);
                }
                self.frames.pop();
            }
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.walk(body);
                for catch in catches {
                    self.walk(&catch.body);
                }
                if let Some(finally) = finally {
                    let id = self.fresh_id();
                    self.finallies.push(id);
                    self.walk(finally);
                    self.finallies.pop();
                }
            }
            StmtKind::Break(level) | StmtKind::Continue(level) => {
                let keyword = if matches!(stmt.kind, StmtKind::Break(_)) {
                    "break"
                } else {
                    "continue"
                };
                let level = level.unwrap_or(1);
                if level < 1 {
                    self.jumps.push((
                        "invalidLevel",
                        stmt.span,
                        format!("Keyword {keyword} accepts only positive integers."),
                    ));
                } else if level as usize > self.frames.len() {
                    self.jumps.push((
                        "outOfLoop",
                        stmt.span,
                        format!("Keyword {keyword} used outside of a loop or a switch statement."),
                    ));
                } else if keyword == "continue"
                    && self.frames[self.frames.len() - level as usize].0 == FrameKind::Switch
                {
                    self.jumps.push((
                        "switch",
                        stmt.span,
                        "Keyword continue targeting switch is equivalent to break.".to_string(),
                    ));
                }
            }
            StmtKind::Label(name) => {
                if self.labels.contains_key(&name.name) {
                    self.duplicate_labels.push((name.name.clone(), stmt.span));
                } else {
                    let placement = self.placement(stmt.span);
                    self.labels.insert(name.name.clone(), placement);
                }
            }
            StmtKind::Goto(name) => {
                let placement = self.placement(stmt.span);
                self.gotos.push((name.name.clone(), placement));
            }
            _ => {
                for child in child_stmts(stmt) {
                    self.walk_stmt(child);
                }
            }
        }
    }
}

fn is_prefix(prefix: &[usize], of: &[usize]) -> bool {
    prefix.len() <= of.len() && prefix.iter().zip(of).all(|(a, b)| a == b)
}

impl<'a> Checker<'a> {
    /// One function-like body, or the file's top-level statements.
    pub(crate) fn check_statement_keywords(&mut self, body: &[Stmt]) {
        let mut scope = Scope::default();
        scope.walk(body);
        for (kind, span, message) in std::mem::take(&mut scope.jumps) {
            let keyword = if message.contains("continue") {
                "continue"
            } else {
                "break"
            };
            let level = if kind == "switch" { 2 } else { 0 };
            let mut builder =
                DiagnosticBuilder::new(message, span, level, format!("{keyword}.{kind}"));
            if kind != "switch" {
                builder = builder.non_ignorable();
            }
            self.report_builder(builder);
        }
        for (name, span) in &scope.duplicate_labels {
            self.report_keyword(
                format!("Label '{name}' is already defined."),
                *span,
                "label.duplicate",
            );
        }
        let mut used: FxHashSet<Name> = FxHashSet::default();
        for (name, goto) in &scope.gotos {
            let Some(target) = scope.labels.get(name) else {
                self.report_keyword(
                    format!("Goto to undefined label '{name}'."),
                    goto.span,
                    "goto.labelUndefined",
                );
                continue;
            };
            used.insert(name.clone());
            if !is_prefix(&target.frames, &goto.frames) {
                self.report_keyword(
                    format!("Goto to label '{name}' jumps into a loop or switch statement."),
                    goto.span,
                    "goto.intoLoop",
                );
            } else if !is_prefix(&goto.finallies, &target.finallies) {
                self.report_keyword(
                    format!("Goto to label '{name}' jumps out of a finally block."),
                    goto.span,
                    "goto.outOfFinally",
                );
            } else if !is_prefix(&target.finallies, &goto.finallies) {
                self.report_keyword(
                    format!("Goto to label '{name}' jumps into a finally block."),
                    goto.span,
                    "goto.intoFinally",
                );
            }
        }
        let mut labels: Vec<_> = scope.labels.iter().collect();
        labels.sort_by_key(|(_, p)| (p.span.start.line, p.span.start.col));
        for (name, placement) in labels {
            if !used.contains(name) {
                self.report(
                    format!("Label '{name}' is unused."),
                    placement.span,
                    4,
                    "label.unused",
                );
            }
        }
    }

    /// `declare(...)` directives anywhere in `program`: `strict_types`
    /// must be `0`/`1`, the file's first statement, and never block
    /// mode; `ticks` needs an int and `encoding` a string; any other
    /// directive is unsupported.
    pub(crate) fn check_declare_directives(&mut self, program: &[Stmt]) {
        let mut declares: Vec<(&Stmt, bool)> = Vec::new();
        collect_declares(program, true, &mut declares);
        for (stmt, is_first) in declares {
            let StmtKind::Declare { directives, body } = &stmt.kind else {
                continue;
            };
            for (name, value) in directives {
                let literal_int = match &value.kind {
                    ExprKind::Literal(Literal::Int(n)) => Some(*n),
                    _ => None,
                };
                match name.to_ascii_lowercase().as_str() {
                    "strict_types" => {
                        if !matches!(literal_int, Some(0 | 1)) {
                            self.report_keyword(
                                format!(
                                    "Declare strict_types must have 0 or 1 as its value, {} given.",
                                    describe_literal(value)
                                ),
                                value.span,
                                "declareStrictTypes.value",
                            );
                        } else if body.is_some() {
                            self.report_keyword(
                                "Declare strict_types must not use block mode.".to_string(),
                                stmt.span,
                                "declareStrictTypes.blockMode",
                            );
                        } else if !is_first {
                            self.report_keyword(
                                "Declare strict_types must be the very first statement."
                                    .to_string(),
                                stmt.span,
                                "declareStrictTypes.notFirst",
                            );
                        }
                    }
                    "ticks" if literal_int.is_none() => self.report_keyword(
                        format!(
                            "Declare ticks must have an integer value, {} given.",
                            describe_literal(value)
                        ),
                        value.span,
                        "declare.ticksValue",
                    ),
                    "encoding" if !matches!(value.kind, ExprKind::Literal(Literal::String(_))) => {
                        self.report_keyword(
                            format!(
                                "Declare encoding must have a string value, {} given.",
                                describe_literal(value)
                            ),
                            value.span,
                            "declare.encodingValue",
                        )
                    }
                    "ticks" | "encoding" => {}
                    _ => self.report_keyword(
                        format!("Unsupported declare '{name}'."),
                        name.span,
                        "declare.unsupported",
                    ),
                }
            }
        }
    }

    fn report_keyword(&mut self, message: String, span: Span, identifier: &'static str) {
        self.report_builder(DiagnosticBuilder::new(message, span, 0, identifier).non_ignorable());
    }
}

/// Every `declare` statement, with whether it's the file's first
/// statement.
fn collect_declares<'s>(stmts: &'s [Stmt], top: bool, out: &mut Vec<(&'s Stmt, bool)>) {
    for (i, stmt) in stmts.iter().enumerate() {
        match &stmt.kind {
            StmtKind::Declare { body, .. } => {
                out.push((stmt, top && i == 0));
                if let Some(body) = body {
                    collect_declares(body, false, out);
                }
            }
            StmtKind::FunctionDecl(f) => collect_declares(&f.body, false, out),
            StmtKind::ClassDecl(c) => {
                for m in &c.methods {
                    collect_declares(&m.function.body, false, out);
                }
            }
            _ => {
                let first_child_is_file_start = top && i == 0;
                for (j, child) in child_stmts(stmt).into_iter().enumerate() {
                    collect_declares(
                        std::slice::from_ref(child),
                        first_child_is_file_start
                            && j == 0
                            && matches!(stmt.kind, StmtKind::Block(_)),
                        out,
                    );
                }
            }
        }
    }
}

fn describe_literal(value: &Expr) -> String {
    match &value.kind {
        ExprKind::Literal(Literal::Int(n)) => n.to_string(),
        ExprKind::Literal(Literal::String(s)) => format!("'{s}'"),
        ExprKind::Literal(Literal::Bool(b)) => b.to_string(),
        ExprKind::Literal(Literal::Null) => "null".to_string(),
        ExprKind::Literal(Literal::Float(f)) => f.to_string(),
        ExprKind::Name(n) => n.to_string(),
        _ => "an expression".to_string(),
    }
}
