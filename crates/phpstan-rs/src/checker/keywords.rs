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
use syntax::visit::arena::{self, Visitor};

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
impl Visitor for Scope {
    fn visit_expr(&mut self, _: &NodeArenas, _: ExprId) {}
    fn visit_function_decl(&mut self, _: &NodeArenas, _: &FunctionDecl) {}
    fn visit_class_def(&mut self, _: &NodeArenas, _: &ClassDef) {}

    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else { return };
        let span = arenas
            .stmt_span(id)
            .expect("keyword statement has an arena span");
        match &stmt.kind {
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => {}
            StmtKind::While { body, .. }
            | StmtKind::DoWhile { body, .. }
            | StmtKind::For { body, .. }
            | StmtKind::Foreach { body, .. } => {
                let id = self.fresh_id();
                self.frames.push((FrameKind::Loop, id));
                self.visit_stmt(arenas, *body);
                self.frames.pop();
            }
            StmtKind::Switch { cases, .. } => {
                let id = self.fresh_id();
                self.frames.push((FrameKind::Switch, id));
                for case in arenas.switch_cases(cases.clone()) {
                    arena::walk_stmts(self, arenas, arenas.stmt_ids(&case.body));
                }
                self.frames.pop();
            }
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                arena::walk_stmts(self, arenas, arenas.stmt_ids(body));
                for catch in arenas.catch_clauses(catches.clone()) {
                    arena::walk_stmts(self, arenas, arenas.stmt_ids(&catch.body));
                }
                if let Some(finally) = finally {
                    let id = self.fresh_id();
                    self.finallies.push(id);
                    arena::walk_stmts(self, arenas, arenas.stmt_ids(finally));
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
                        span,
                        format!("Keyword {keyword} accepts only positive integers."),
                    ));
                } else if level as usize > self.frames.len() {
                    self.jumps.push((
                        "outOfLoop",
                        span,
                        format!("Keyword {keyword} used outside of a loop or a switch statement."),
                    ));
                } else if keyword == "continue"
                    && self.frames[self.frames.len() - level as usize].0 == FrameKind::Switch
                {
                    self.jumps.push((
                        "switch",
                        span,
                        "Keyword continue targeting switch is equivalent to break.".to_string(),
                    ));
                }
            }
            StmtKind::Label(name) => {
                if self.labels.contains_key(&name.name) {
                    self.duplicate_labels.push((name.name.clone(), span));
                } else {
                    let placement = self.placement(span);
                    self.labels.insert(name.name.clone(), placement);
                }
            }
            StmtKind::Goto(name) => {
                let placement = self.placement(span);
                self.gotos.push((name.name.clone(), placement));
            }
            _ => arena::walk_stmt(self, arenas, id),
        }
    }
}

fn is_prefix(prefix: &[usize], of: &[usize]) -> bool {
    prefix.len() <= of.len() && prefix.iter().zip(of).all(|(a, b)| a == b)
}

impl<'a> Checker<'a> {
    /// One function-like body, or the file's top-level statements.
    pub(crate) fn check_statement_keywords(&mut self, body: &[StmtId]) {
        let arenas = self
            .syntax_arenas()
            .expect("statement-keyword checks require their parsed file owner");
        let mut scope = Scope::default();
        arena::walk_stmts(&mut scope, arenas, body);
        for (kind, span, message) in std::mem::take(&mut scope.jumps) {
            let keyword = if message.contains("continue") {
                "continue"
            } else {
                "break"
            };
            let level = if kind == "switch" { 2 } else { 0 };
            if self.shows_level(level) {
                let mut builder =
                    DiagnosticBuilder::new(message, span, level, format!("{keyword}.{kind}"));
                if kind != "switch" {
                    builder = builder.non_ignorable();
                }
                self.report_builder(builder);
            } else {
                self.note_below_level();
            }
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
                report!(
                    self,
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
    pub(crate) fn check_declare_directives(&mut self, arenas: &NodeArenas, program: &[StmtId]) {
        for (id, is_first) in collect_declares(arenas, program) {
            let Some(stmt) = arenas.stmt(id) else {
                continue;
            };
            let StmtKind::Declare { directives, body } = &stmt.kind else {
                continue;
            };
            for directive in arenas.declare_directives(directives.clone()) {
                let name = &directive.name;
                let value = directive.value;
                let Some(value_node) = arenas.expr(value) else {
                    continue;
                };
                let literal_int = match &value_node.kind {
                    ExprKind::Literal(Literal::Int(n)) => Some(*n),
                    _ => None,
                };
                match name.to_ascii_lowercase().as_str() {
                    "strict_types" => {
                        if !matches!(literal_int, Some(0 | 1)) {
                            self.report_keyword(
                                format!(
                                    "Declare strict_types must have 0 or 1 as its value, {} given.",
                                    describe_literal(arenas, value)
                                ),
                                arenas
                                    .expr_span(value_node.id)
                                    .expect("declare value has a parallel span"),
                                "declareStrictTypes.value",
                            );
                        } else if body.is_some() {
                            self.report_keyword(
                                "Declare strict_types must not use block mode.".to_string(),
                                self.stmt_span(stmt.id),
                                "declareStrictTypes.blockMode",
                            );
                        } else if !is_first {
                            self.report_keyword(
                                "Declare strict_types must be the very first statement."
                                    .to_string(),
                                self.stmt_span(stmt.id),
                                "declareStrictTypes.notFirst",
                            );
                        }
                    }
                    "ticks" if literal_int.is_none() => self.report_keyword(
                        format!(
                            "Declare ticks must have an integer value, {} given.",
                            describe_literal(arenas, value)
                        ),
                        arenas
                            .expr_span(value_node.id)
                            .expect("declare value has a parallel span"),
                        "declare.ticksValue",
                    ),
                    "encoding"
                        if !matches!(value_node.kind, ExprKind::Literal(Literal::String(_))) =>
                    {
                        self.report_keyword(
                            format!(
                                "Declare encoding must have a string value, {} given.",
                                describe_literal(arenas, value)
                            ),
                            arenas
                                .expr_span(value_node.id)
                                .expect("declare value has a parallel span"),
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
        report_builder!(
            self,
            DiagnosticBuilder::new(message, span, 0, identifier).non_ignorable()
        );
    }
}

/// Every `declare` statement, with whether it's the file's first
/// statement.
fn collect_declares(arenas: &NodeArenas, program: &[StmtId]) -> Vec<(StmtId, bool)> {
    struct Declares {
        out: Vec<(StmtId, bool)>,
        // The next statement visited is the file's first, looking through
        // leading blocks.
        next_is_first: bool,
    }
    impl Visitor for Declares {
        fn visit_expr(&mut self, _: &NodeArenas, _: ExprId) {}
        fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
            let Some(stmt) = arenas.stmt(id) else { return };
            let is_first = std::mem::replace(&mut self.next_is_first, false);
            match &stmt.kind {
                StmtKind::Declare { body, .. } => {
                    self.out.push((id, is_first));
                    if let Some(body) = body {
                        arena::walk_stmts(self, arenas, arenas.stmt_ids(body));
                    }
                }
                StmtKind::Block(body) => {
                    self.next_is_first = is_first;
                    arena::walk_stmts(self, arenas, arenas.stmt_ids(body));
                }
                StmtKind::FunctionDecl(id) => {
                    if let Some(function) = arenas.function(*id) {
                        arena::walk_stmts(self, arenas, function.body_stmts(arenas));
                    }
                }
                StmtKind::ClassDecl(c) => {
                    let c = arenas.class_decl(c.clone());
                    if let Some(def) = arenas.class(c.def) {
                        for m in arenas.methods(def.methods.clone()) {
                            arena::walk_stmts(self, arenas, m.function.body_stmts(arenas));
                        }
                    }
                }
                _ => arena::walk_stmt(self, arenas, id),
            }
        }
    }
    let mut declares = Declares {
        out: Vec::new(),
        next_is_first: true,
    };
    arena::walk_stmts(&mut declares, arenas, program);
    declares.out
}

fn describe_literal(arenas: &NodeArenas, id: ExprId) -> String {
    let Some(value) = arenas.expr(id) else {
        return "an expression".to_string();
    };
    match &value.kind {
        ExprKind::Literal(Literal::Int(n)) => n.to_string(),
        ExprKind::Literal(Literal::String(s)) => arenas
            .string_bytes(s)
            .map(|bytes| format!("'{}'", String::from_utf8_lossy(bytes)))
            .unwrap_or_else(|| "a string literal".to_string()),
        ExprKind::Literal(Literal::Bool(b)) => b.to_string(),
        ExprKind::Literal(Literal::Null) => "null".to_string(),
        ExprKind::Literal(Literal::Float(f)) => f.to_string(),
        ExprKind::Name(n) => arenas
            .qualified_name(n)
            .map(|name| name.to_string())
            .unwrap_or_else(|| "an expression".to_string()),
        _ => "an expression".to_string(),
    }
}
