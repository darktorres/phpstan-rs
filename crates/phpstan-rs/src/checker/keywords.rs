//! Statement keywords PHP validates at compile time, per function-like
//! body (and the file's top level): `break`/`continue` levels
//! (`ContinueBreakInLoopRule`, plus a non-positive level and a `continue`
//! that targets a `switch`), `goto`/labels (`GotoUndefinedLabelRule`,
//! `UnusedLabelRule`, plus duplicate labels and jumps into a loop, a
//! `switch`, or a `finally` block, or out of one), and `declare`
//! directives (`DeclareStrictTypesRule`, plus block-mode `strict_types`
//! and unknown directives).

use syntax::name::{NameMap, NameSet};

use crate::checker::Checker;
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use syntax::name::Name;
use syntax::span::Span;
use syntax::visit::{Visitor, walk_stmt, walk_stmts};

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
    labels: NameMap<Placement>,
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
}

/// Walks one function-like body: expressions and nested declarations are
/// not entered.
impl<'ast> Visitor<'ast> for Scope {
    fn visit_expr(&mut self, _: &'ast Expr) {}
    fn visit_function_decl(&mut self, _: &'ast FunctionDecl) {}
    fn visit_class_def(&mut self, _: &'ast ClassDef) {}

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        stack::guard(|| match &stmt.kind {
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => {}
            StmtKind::While { body, .. }
            | StmtKind::DoWhile { body, .. }
            | StmtKind::For { body, .. }
            | StmtKind::Foreach { body, .. } => {
                let id = self.fresh_id();
                self.frames.push((FrameKind::Loop, id));
                self.visit_stmt(body);
                self.frames.pop();
            }
            StmtKind::Switch { cases, .. } => {
                let id = self.fresh_id();
                self.frames.push((FrameKind::Switch, id));
                for case in cases {
                    walk_stmts(self, &case.body);
                }
                self.frames.pop();
            }
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                walk_stmts(self, body);
                for catch in catches {
                    walk_stmts(self, &catch.body);
                }
                if let Some(finally) = finally {
                    let id = self.fresh_id();
                    self.finallies.push(id);
                    walk_stmts(self, finally);
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
            _ => walk_stmt(self, stmt),
        })
    }
}

fn is_prefix(prefix: &[usize], of: &[usize]) -> bool {
    prefix.len() <= of.len() && prefix.iter().zip(of).all(|(a, b)| a == b)
}

impl<'a> Checker<'a> {
    /// One function-like body, or the file's top-level statements.
    pub(crate) fn check_statement_keywords(&mut self, body: &[Stmt]) {
        let mut scope = Scope::default();
        walk_stmts(&mut scope, body);
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
        let mut used: NameSet = NameSet::default();
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
        labels.sort_by_key(|(_, p)| p.span.start);
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
        for (stmt, is_first) in collect_declares(program) {
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
fn collect_declares(program: &[Stmt]) -> Vec<(&Stmt, bool)> {
    struct Declares<'s> {
        out: Vec<(&'s Stmt, bool)>,
        // The next statement visited is the file's first, looking through
        // leading blocks.
        next_is_first: bool,
    }
    impl<'s> Visitor<'s> for Declares<'s> {
        fn visit_expr(&mut self, _: &'s Expr) {}
        fn visit_stmt(&mut self, stmt: &'s Stmt) {
            let is_first = std::mem::replace(&mut self.next_is_first, false);
            match &stmt.kind {
                StmtKind::Declare { body, .. } => {
                    self.out.push((stmt, is_first));
                    if let Some(body) = body {
                        walk_stmts(self, body);
                    }
                }
                StmtKind::Block(body) => {
                    self.next_is_first = is_first;
                    walk_stmts(self, body);
                }
                StmtKind::FunctionDecl(f) => walk_stmts(self, f.body_stmts()),
                StmtKind::ClassDecl(c) => {
                    for m in &c.def.methods {
                        walk_stmts(self, m.function.body_stmts());
                    }
                }
                _ => walk_stmt(self, stmt),
            }
        }
    }
    let mut declares = Declares {
        out: Vec::new(),
        next_is_first: true,
    };
    walk_stmts(&mut declares, program);
    declares.out
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
