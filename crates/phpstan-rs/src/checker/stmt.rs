use crate::checker::keys::LocalKey;
use crate::checker::{
    ATTRIBUTE_TARGET_CLASS, ATTRIBUTE_TARGET_CLASS_CONSTANT, ATTRIBUTE_TARGET_PROPERTY,
    BlockCompletion, Checker, Completion, ExitKind, Locals, Reach, generator_missing_return_type,
    generator_template_param, is_truthy_literal, merge_scopes, purge_stale_narrowing_for_var,
};
use syntax::ast::*;
use syntax::visit::arena::{self, Visitor};
use types::Name;
use types::Type;

/// See `DiscriminantArrayFactsMap`'s own doc comment (`checker.rs`) - `e`
/// must be exactly a bare `Variable` (the same narrow shape the write side
/// currently triggers on) for this to find anything at all. `None` when
/// there's nothing recorded for it, the common case for every function
/// this mechanism doesn't apply to. Each branch's type stays its own:
/// a union would merge `list<A>|list<B>` into `list<A|B>`.
fn reconstructed_discriminant_return_types(e: &ExprNode, locals: &Locals) -> Vec<Type> {
    let ExprKind::Variable(name) = &e.kind else {
        return Vec::new();
    };
    let Some(container_key) = locals.find_var(name) else {
        return Vec::new();
    };
    locals
        .discriminant_array_facts()
        .iter()
        .filter(|((container, _, _), _)| *container == container_key)
        .map(|(_, ty)| ty.clone())
        .collect()
}

mod loops;
mod new_exception_discarded;
mod switch;
mod try_catch;
use try_catch::TryStatement;

/// What a block has found of its statements so far.
#[derive(Clone, Default)]
struct BlockState {
    reported_unreachable: bool,
    reach: Reach,
}

/// A stretch of a statement list that a backward `goto` loops.
struct GotoRegion {
    first: usize,
    last: usize,
    labels: Vec<syntax::name::Name>,
}

/// How many loops or switches a `break` or `continue` names: one when it
/// does not say.
fn level_of(level: Option<i64>) -> u32 {
    u32::try_from(level.unwrap_or(1).clamp(1, 16)).unwrap_or(1)
}

impl<'a> Checker<'a> {
    pub(crate) fn check_block(
        &mut self,
        stmts: &[StmtId],
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) -> BlockCompletion {
        if stmts.is_empty() {
            return BlockCompletion::default();
        }
        let mut state = BlockState::default();
        let regions = self.goto_regions(stmts);
        let mut next = 0;
        for region in regions {
            self.check_stmt_range(
                stmts,
                next..region.first,
                locals,
                declared_return,
                label,
                &mut state,
            );
            self.check_goto_region(stmts, &region, locals, declared_return, label, &mut state);
            next = region.last + 1;
        }
        self.check_stmt_range(
            stmts,
            next..stmts.len(),
            locals,
            declared_return,
            label,
            &mut state,
        );
        state.reach.finish()
    }

    /// The statements `stmts[range]` in order, with what a block finds as it
    /// goes in `state`.
    fn check_stmt_range(
        &mut self,
        stmts: &[StmtId],
        range: std::ops::Range<usize>,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
        state: &mut BlockState,
    ) {
        // `UnreachableStatementRule`: relies on `converge_loop_body`'s
        // fixed-point loop-body convergence so a variable reassigned
        // inside a loop doesn't keep its pre-loop-entry type for the
        // whole single-pass body check, which could wrongly narrow it to
        // `never` and read here as "this statement always
        // terminates." Real PHPStan's own equivalent
        // (`UnreachableStatementRule`, `deadCode.unreachable`, level 4)
        // reports exactly once per maximal unreachable run, at its first
        // statement - matched here via `reported_unreachable`, gating
        // further reports without gating further checking. Every
        // statement is still run through `check_stmt` regardless - real
        // PHPStan still fully analyzes code after an
        // `UnreachableStatementNode` for every other rule, and stopping
        // early here would silently drop real, unrelated diagnostics
        // sitting in a since-dead branch.
        let syntax_owner = self
            .syntax_tree_owner()
            .expect("checking a statement block requires its syntax owner");
        let arenas = syntax_owner.arenas();
        for i in range {
            let stmt = arenas
                .stmt(stmts[i])
                .expect("statement block ID must resolve in its syntax owner");
            if let StmtKind::Label(name) = &stmt.kind
                && self.current_function_goto_targets.is_target(&name.name)
            {
                // The scope at a label is what every path that reaches it
                // gives: the one falling through to it, if that is
                // reachable, and the scope of each `goto` that jumped to it.
                let points = &mut self.current_function_goto_targets.points;
                let landing: Vec<Locals> = points
                    .iter()
                    .filter(|(to, _)| *to == name.name)
                    .map(|(_, scope)| scope.clone())
                    .collect();
                points.retain(|(to, _)| *to != name.name);
                let mut scopes = landing.into_iter();
                if let Some(first) = scopes.next() {
                    let merged =
                        scopes.fold(first, |acc, s| merge_scopes(&self.relation(), &acc, &s));
                    *locals = if state.reach.reachable() {
                        merge_scopes(&self.relation(), locals, &merged)
                    } else {
                        merged
                    };
                }
                state.reach.label();
            }
            let (was_reachable, exits_before) = (
                state.reach.reachable(),
                self.current_function_exit_points.len(),
            );
            let last = self.check_stmt(stmt, locals, declared_return, label);
            // What cannot be reached cannot be left by.
            if !was_reachable {
                self.current_function_exit_points.truncate(exits_before);
            }
            state.reach.add(last);
            if !state.reported_unreachable
                && last.terminates
                && let Some(next) = stmts.get(i + 1).and_then(|id| arenas.stmt(*id))
                // A labeled statement `goto`-targeted somewhere else in
                // this same function is a real jump target, not dead
                // code, whatever the statement immediately before it
                // does - see `Checker::current_function_goto_targets`'s.
                && !matches!(&next.kind, StmtKind::Label(name) if self.current_function_goto_targets.is_target(&name.name))
            {
                report!(self, 
                    "Unreachable statement - code above always terminates.".to_string(),
                    arenas
                        .stmt_span(next.id)
                        .expect("next statement has a parallel span"),
                    4,
                    "deadCode.unreachable",
                );
                state.reported_unreachable = true;
            }
        }
    }

    /// The stretches of `stmts` a `goto` that jumps back makes a loop of: from
    /// the first statement holding the label to the last holding a `goto` to
    /// it, where both are in this list and not in one statement of it (the
    /// list inside that statement has the stretch then). Overlapping
    /// stretches are one.
    fn goto_regions(&self, stmts: &[StmtId]) -> Vec<GotoRegion> {
        let labels = &self.current_function_goto_targets.labels;
        if !labels.iter().any(|l| l.backward) {
            return Vec::new();
        }
        let syntax_owner = self
            .syntax_tree_owner()
            .expect("a backward goto requires its syntax owner");
        let arenas = syntax_owner.arenas();
        let holds = |id: StmtId, is_label: bool, name: &syntax::name::Name| {
            arena::any_stmt_in_body(arenas, id, |s| match &s.kind {
                StmtKind::Label(n) => is_label && n.name == *name,
                StmtKind::Goto(n) => !is_label && n.name == *name,
                _ => false,
            })
        };
        let mut regions: Vec<GotoRegion> = Vec::new();
        for label in labels.iter().filter(|l| l.backward) {
            let Some(first) = stmts.iter().position(|id| holds(*id, true, &label.name)) else {
                continue;
            };
            let Some(last) = stmts.iter().rposition(|id| holds(*id, false, &label.name)) else {
                continue;
            };
            if first >= last {
                continue;
            }
            regions.push(GotoRegion {
                first,
                last,
                labels: vec![label.name.clone()],
            });
        }
        regions.sort_by_key(|r| r.first);
        let mut merged: Vec<GotoRegion> = Vec::new();
        for region in regions {
            match merged.last_mut() {
                Some(previous) if region.first <= previous.last => {
                    previous.last = previous.last.max(region.last);
                    previous.labels.extend(region.labels);
                }
                _ => merged.push(region),
            }
        }
        merged
    }

    /// A stretch of statements a backward `goto` makes a loop of, run until
    /// the scope at its start stops changing: each pass starts from the scope
    /// before the stretch joined with the scope of each `goto` that jumped
    /// back, and only the last pass's findings stay.
    fn check_goto_region(
        &mut self,
        stmts: &[StmtId],
        region: &GotoRegion,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
        state: &mut BlockState,
    ) {
        const MAX_PASSES: usize = 3;
        let entry = locals.clone();
        let state_before = state.clone();
        let mut pass_entry = entry.clone();
        for pass in 0..MAX_PASSES {
            let mark = self.pass_mark();
            let mut pass_locals = pass_entry.clone();
            let mut pass_state = state_before.clone();
            self.check_stmt_range(
                stmts,
                region.first..region.last + 1,
                &mut pass_locals,
                declared_return,
                label,
                &mut pass_state,
            );
            // The scopes the `goto`s of the stretch jumped back with.
            let points = &mut self.current_function_goto_targets.points;
            let back: Vec<Locals> = points
                .iter()
                .filter(|(to, _)| region.labels.contains(to))
                .map(|(_, scope)| scope.clone())
                .collect();
            points.retain(|(to, _)| !region.labels.contains(to));
            let mut next_entry = entry.clone();
            for scope in &back {
                next_entry = merge_scopes(&self.relation(), &next_entry, scope);
            }
            if pass >= 1 && next_entry != pass_entry {
                self.widen_towards_entry(&pass_entry, &mut next_entry);
            }
            if next_entry == pass_entry || pass + 1 == MAX_PASSES {
                *locals = pass_locals;
                *state = pass_state;
                return;
            }
            self.rewind_to(mark);
            pass_entry = next_entry;
        }
    }

    /// Thin wrapper over `check_stmt_impl` - see `check_expr`'s own
    /// identically-shaped wrapper (`checker/expr.rs`) for the full
    /// reasoning.
    pub(crate) fn check_stmt(
        &mut self,
        stmt: &StmtNode,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) -> Completion {
        let at = self
            .syntax_arenas()
            .and_then(|arenas| arenas.stmt_span(stmt.id))
            .map(|span| span.start);
        let outer = self.stmt_at.replace(at);
        let completion = self.check_stmt_in_place(stmt, locals, declared_return, label);
        self.stmt_at.set(outer);
        completion
    }

    fn check_stmt_in_place(
        &mut self,
        stmt: &StmtNode,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) -> Completion {
        syntax::stack::guard(|| {
            if self.settings.shipmonk.check_enum_exhaustiveness {
                self.check_strict_statement_idioms(stmt, locals);
            }
            if self.settings.sidz.check_magic_numbers {
                self.check_magic_numbers_statement(stmt);
            }
            if self.settings.strict_rules.check_loop_variable_overwrite
                || self.settings.strict_rules.check_switch_case_types
            {
                self.check_strict_rules_statement(stmt, locals);
            }
            let completion =
                crate::checker::profiling::measure_outermost!("checker::check_stmt", {
                    self.check_stmt_impl(stmt, locals, declared_return, label)
                });
            self.record_type_dump(stmt, locals);
            self.note_captured_types(locals);
            if !self.external_rules.is_empty() {
                self.dispatch_stmt_rules(stmt, locals);
            }
            completion
        })
    }

    /// `phpDoc.parseError` for a statement's own docblock - the
    /// statement-level half of real PHPStan's `InvalidPhpDocTagValueRule`
    /// (the declaration-level half runs from
    /// `check_unknown_phpstan_tags_on`). Real PHPStan checks every
    /// statement's docblock except an expression statement that isn't an
    /// assignment; a function/class/enum declaration's own docblock is
    /// skipped here, since the declaration-level half already covers it.
    fn check_statement_doc_tag_values(&mut self, stmt: &StmtNode) {
        let syntax_owner = self.syntax_tree_owner();
        let arenas = syntax_owner
            .as_ref()
            .map(crate::checker::SyntaxTreeOwner::arenas);
        let Some(site) = arenas.and_then(|arenas| arenas.doc_site(self.stmt_span(stmt.id).start))
        else {
            return;
        };
        let doc = site.text.as_ref();
        let Some(arenas) = arenas else {
            return;
        };
        match &stmt.kind {
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => return,
            StmtKind::Expression(e)
                if !matches!(
                    arenas.expr(*e).map(|e| &e.kind),
                    Some(
                        ExprKind::Assign(..)
                            | ExprKind::AssignRef(..)
                            | ExprKind::CompoundAssign(..)
                    )
                ) =>
            {
                return;
            }
            _ => {}
        }
        self.report_invalid_doc_tag_values(doc, self.stmt_span(stmt.id));
    }

    /// Real PHPStan's `WrongVariableNameInVarTagRule`: a statement's
    /// `@var` tags must name the variables that statement is about (the
    /// assigned ones, a `foreach`'s key/value/subject, a `static`/`global`
    /// list) or, elsewhere, a variable already defined; an unnamed tag
    /// needs exactly one candidate. `locals` is the scope before `stmt`.
    /// Whether every tag's variable name checked out.
    fn check_statement_var_tag_names(
        &mut self,
        stmt: &StmtNode,
        locals: &Locals,
        label: &str,
    ) -> bool {
        let syntax_owner = self.syntax_tree_owner();
        let arenas = syntax_owner
            .as_ref()
            .map(crate::checker::SyntaxTreeOwner::arenas);
        let Some(site) = arenas.and_then(|arenas| arenas.doc_site(self.stmt_span(stmt.id).start))
        else {
            return true;
        };
        let doc = site.text.as_ref();
        if !doc.contains("var")
            || matches!(
                stmt.kind,
                StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) | StmtKind::GlobalConst(_)
            )
        {
            return true;
        }
        let tags: Vec<Option<String>> = super::phpdoc_unresolvable::var_tag_nodes(doc)
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        if tags.is_empty() {
            return true;
        }
        let Some(owner) = self.syntax_tree_owner() else {
            return true;
        };
        let arenas = owner.arenas();
        // Reported on the statement itself, unlike the comment-line
        // `varTag.unresolvableType`.
        let span = self.stmt_span(stmt.id);
        let mut found: Vec<(String, &'static str)> = Vec::new();
        // Superglobals always exist; `$argv`/`$argc` in a script's own
        // top-level scope.
        let top_level = label == "<main>";
        let defined = |name: &str| {
            locals.contains_var(name)
                || matches!(
                    name,
                    "GLOBALS"
                        | "_SERVER"
                        | "_GET"
                        | "_POST"
                        | "_FILES"
                        | "_COOKIE"
                        | "_SESSION"
                        | "_REQUEST"
                        | "_ENV"
                )
                || (top_level && matches!(name, "argv" | "argc"))
        };
        let list = |names: &[String]| {
            names
                .iter()
                .map(|n| format!("${n}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        match &stmt.kind {
            StmtKind::Expression(e_id)
                if matches!(
                    arenas.expr(*e_id).map(|e| &e.kind),
                    Some(
                        ExprKind::Assign(..)
                            | ExprKind::AssignRef(..)
                            | ExprKind::CompoundAssign(..)
                    )
                ) =>
            {
                let Some(e) = arenas.expr(*e_id) else {
                    return true;
                };
                let (ExprKind::Assign(lhs, _)
                | ExprKind::AssignRef(lhs, _)
                | ExprKind::CompoundAssign(_, lhs, _)) = &e.kind
                else {
                    unreachable!()
                };
                let assigned = assigned_variable_names(arenas, *lhs);
                let mut reported_multiple = false;
                for tag in &tags {
                    match tag {
                        None if tags.len() != 1 => {
                            if !reported_multiple {
                                found.push((
                                    "Multiple PHPDoc @var tags above single variable assignment are not supported."
                                        .to_string(),
                                    "varTag.multipleTags",
                                ));
                                reported_multiple = true;
                            }
                        }
                        None if assigned.len() != 1 => found.push((
                            "PHPDoc tag @var above assignment does not specify variable name."
                                .to_string(),
                            "varTag.noVariable",
                        )),
                        None => {}
                        Some(name) if defined(name) || assigned.contains(name) => {}
                        Some(name) if assigned.len() == 1 && tags.len() == 1 => found.push((
                            format!(
                                "Variable ${name} in PHPDoc tag @var does not match assigned variable ${}.",
                                assigned[0]
                            ),
                            "varTag.differentVariable",
                        )),
                        Some(name) => found.push((
                            format!("Variable ${name} in PHPDoc tag @var does not exist."),
                            "varTag.variableNotFound",
                        )),
                    }
                }
            }
            StmtKind::Foreach {
                subject,
                key,
                value,
                ..
            } => {
                let mut names = Vec::new();
                if let Some(subject) = arenas.expr(*subject)
                    && let ExprKind::Variable(v) = &subject.kind
                {
                    names.push(v.to_string());
                }
                if let Some(k) = key.and_then(|id| arenas.expr(id))
                    && let ExprKind::Variable(v) = &k.kind
                {
                    names.push(v.to_string());
                }
                names.extend(assigned_variable_names(arenas, *value));
                for tag in &tags {
                    match tag {
                        None if names.len() == 1 => {}
                        None => found.push((
                            "PHPDoc tag @var above foreach loop does not specify variable name."
                                .to_string(),
                            "varTag.noVariable",
                        )),
                        Some(name) if names.contains(name) => {}
                        Some(name) => found.push((
                            format!(
                                "Variable ${name} in PHPDoc tag @var does not match any variable in the foreach loop: {}",
                                list(&names)
                            ),
                            "varTag.differentVariable",
                        )),
                    }
                }
            }
            StmtKind::StaticVar(vars) => {
                let names: Vec<String> = arenas
                    .static_vars(vars.clone())
                    .iter()
                    .map(|variable| variable.name.to_string())
                    .collect();
                for tag in &tags {
                    match tag {
                        None if names.len() == 1 => {}
                        None => found.push((
                            "PHPDoc tag @var above multiple static variables does not specify variable name."
                                .to_string(),
                            "varTag.noVariable",
                        )),
                        Some(name) if names.contains(name) => {}
                        Some(name) => found.push((
                            format!(
                                "Variable ${name} in PHPDoc tag @var does not match any static variable: {}",
                                list(&names)
                            ),
                            "varTag.differentVariable",
                        )),
                    }
                }
            }
            StmtKind::Global(vars) => {
                let names: Vec<String> = arenas
                    .global_vars(vars.clone())
                    .iter()
                    .filter_map(|v| v.name().map(Name::to_string))
                    .collect();
                for tag in &tags {
                    match tag {
                        None if names.len() == 1 => {}
                        None => found.push((
                            "PHPDoc tag @var above multiple global variables does not specify variable name."
                                .to_string(),
                            "varTag.noVariable",
                        )),
                        Some(name) if names.contains(name) => {}
                        Some(name) => found.push((
                            format!(
                                "Variable ${name} in PHPDoc tag @var does not match any global variable: {}",
                                list(&names)
                            ),
                            "varTag.differentVariable",
                        )),
                    }
                }
            }
            other => {
                // `return`/`throw` carry an expression an unnamed tag can
                // describe; any other statement has none.
                let has_expr = matches!(other, StmtKind::Return(Some(_)) | StmtKind::Throw(_));
                let mut unnamed = 0;
                for tag in &tags {
                    match tag {
                        None => unnamed += 1,
                        Some(name) if defined(name) => {}
                        Some(name) => found.push((
                            format!("Variable ${name} in PHPDoc tag @var does not exist."),
                            "varTag.variableNotFound",
                        )),
                    }
                }
                if unnamed > 0 && (unnamed != 1 || !has_expr) {
                    found.push((
                        "PHPDoc tag @var does not specify variable name.".to_string(),
                        "varTag.noVariable",
                    ));
                }
            }
        }
        let ok = found.is_empty();
        for (message, identifier) in found {
            report!(self, message, span, 2, identifier);
        }
        ok
    }

    /// `InvalidPhpDocVarTagTypeRule`'s unresolvable half: every `@var` on
    /// a statement (not a declaration, whose own rules check its tags),
    /// reported on the comment's own line.
    fn check_statement_var_tags_resolvable(&mut self, stmt: &StmtNode) {
        let syntax_owner = self.syntax_tree_owner();
        let arenas = syntax_owner
            .as_ref()
            .map(crate::checker::SyntaxTreeOwner::arenas);
        let Some(site) = arenas.and_then(|arenas| arenas.doc_site(self.stmt_span(stmt.id).start))
        else {
            return;
        };
        let doc = site.text.as_ref();
        if !doc.contains("var")
            || matches!(
                stmt.kind,
                StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) | StmtKind::GlobalConst(_)
            )
        {
            return;
        }
        let class = self.current_class.clone();
        let line = arenas
            .and_then(|arenas| arenas.doc_site(self.stmt_span(stmt.id).start))
            .map_or_else(
                || self.line_index.line(self.stmt_span(stmt.id).start),
                |site| site.line,
            );
        let at = self
            .line_index
            .line_start(line)
            .unwrap_or(self.stmt_span(stmt.id).start);
        let span = syntax::span::Span { start: at, end: at };
        for (variable, node) in super::phpdoc_unresolvable::var_tag_nodes(doc).iter() {
            if self.type_node_unresolvable(node, class.as_deref()) {
                let subject = match variable {
                    Some(v) => format!("PHPDoc tag @var for variable ${v}"),
                    None => "PHPDoc tag @var".to_string(),
                };
                report!(self, 
                    format!("{subject} contains unresolvable type."),
                    span,
                    2,
                    "varTag.unresolvableType",
                );
            }
        }
    }

    fn dispatch_stmt_rules(&mut self, stmt: &StmtNode, locals: &Locals) {
        let rules = self.external_rules;
        let mut found = Vec::new();
        for rule in rules {
            if rule.wants_stmt(&stmt.kind) {
                let scope = crate::rule::Scope::new(self, locals);
                found.extend(rule.check_stmt(&scope, stmt));
            }
        }
        self.diagnostics.extend(found);
    }

    fn check_stmt_impl(
        &mut self,
        stmt: &StmtNode,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) -> Completion {
        let mut completion = Completion::FALLS_THROUGH;
        let exits_from = self.current_function_exit_points.len();
        // Whether an expression statement's value was `never`, as it was
        // checked.
        let mut value_is_never = false;
        self.check_statement_doc_tag_values(stmt);
        self.check_statement_var_tags_resolvable(stmt);
        let names_ok = self.check_statement_var_tag_names(stmt, locals, label);
        let var_tags = self.statement_var_tags_of(stmt);
        if !var_tags.is_empty() {
            self.check_statement_var_tag_classes(stmt, &var_tags);
            self.check_statement_var_tag_types(stmt, &var_tags, names_ok, locals);
        }
        // A lone nameless `/** @var T */` on `return expr;` types the
        // returned value itself.
        let return_var_tag = match (&stmt.kind, var_tags.as_slice()) {
            (StmtKind::Return(Some(_)), [(None, ty)]) => Some(ty.clone()),
            _ => None,
        };
        let owner = self
            .syntax_tree_owner()
            .expect("inline variable annotations retain their parsed file owner");
        let arenas = owner.arenas();
        let inline_var = inline_var_annotations(arenas, stmt, var_tags);
        // A tag naming a variable this statement assigns describes the
        // assigned value, so it applies after the assignment (below) -
        // the right-hand side still reads the variable's prior type.
        let assigned = if inline_var.is_empty() {
            Vec::new()
        } else {
            statement_assigned_variable_names(arenas, &stmt.kind)
        };
        for (name, ty) in &inline_var {
            if assigned.iter().any(|assigned| assigned.as_str() == name) {
                continue;
            }
            purge_stale_narrowing_for_var(locals, name);
            locals.insert_var(&name.clone(), ty.clone());
        }
        match &stmt.kind {
            StmtKind::Expression(e_id) => {
                let Some(e) = arenas.expr(*e_id) else {
                    return completion;
                };
                self.in_bare_statement_position = true;
                let ty = self.check_expr(e, locals, label);
                value_is_never = ty.is_never();
                if self.collect_inlay_hints
                    && let ExprKind::Assign(lhs, _) | ExprKind::AssignRef(lhs, _) = &e.kind
                    && let Some(lhs) = arenas.expr(*lhs)
                    && let ExprKind::Variable(name) = &lhs.kind
                    && !ty.is_mixed()
                    && !inline_var.iter().any(|(n, _)| n.as_str() == name.as_ref())
                {
                    self.inlay_local_type_hints
                        .push((self.expr_span(lhs.id), ty.clone()));
                }
                // The result of an expression statement is destroyed where
                // it was made.
                self.forget_what_a_temporary_destroys(e, &ty, locals);
                self.check_no_discard_statement(e, locals);
                self.check_new_exception_discarded(e, locals);
                // NoopRule's own supported slice - see `checker::noop`'s
                // module doc comment.
                self.check_noop_statement(e, locals);
                // A bare call to a now-known-pure declaration.
                self.check_pure_call_without_effect(e, locals);
                // `assert($cond)` is a real, common narrowing idiom real
                // PHPStan explicitly documents support for - if execution
                // continues past this statement, `$cond` held, so it's
                // narrowed the same way a terminating `if`-guard clause's
                // surviving branch already is, just without any
                // branch/merge machinery since there's only one path
                // forward (assert either holds or the program aborts).
                // Deliberately not `instanceof`-specific: apply_narrowing
                // already handles whatever condition shapes it
                // understands and no-ops harmlessly on the rest.
                //
                // `name.trim_start_matches('\\')`: a fully-qualified call
                // (`\assert(...)`) keeps its leading backslash in
                // `ExprKind::Name`, unlike ordinary call resolution - the
                // same gap `apply_narrowing`'s own `ExprKind::Call` arm
                // already fixes for `is_string`/`is_array`/etc (see its
                // own comment), needed since a common defensive-global-
                // namespace style writes `\assert(\is_string($input))`
                // throughout, which would otherwise never narrow.
                //
                // `assert($cond) && sideEffect();` (the assertion
                // doubling as a guard clause for whatever follows) needs
                // the identical narrowing, not just a bare
                // `assert($cond);` alone - `bare_assert_condition`'s own
                // doc comment has the "which chain position is actually
                // safe" reasoning.
                // A variable an assertion narrows to `never` fails it.
                let assert_condition = bare_assert_condition(arenas, e.id);
                let inhabited: Vec<LocalKey> = if matches!(
                    e.kind,
                    ExprKind::Call(..) | ExprKind::MethodCall(..) | ExprKind::StaticCall(..)
                ) || assert_condition.is_some()
                {
                    locals
                        .iter()
                        .filter(|(_, t)| !t.is_never())
                        .map(|(k, _)| k)
                        .collect()
                } else {
                    Vec::new()
                };
                if let Some(cond) = assert_condition
                    && let Some(cond) = arenas.expr(cond)
                {
                    self.apply_narrowing(cond, locals, true);
                }
                // PHPUnit's own narrowing-relevant assertions
                // (`self::assertIsString($x)`, `$this->assertNotNull($x)`)
                // - the same "statement narrows unconditionally" shape as
                // `assert($cond)` above, just per-assertion-name rather
                // than a generic boolean condition.
                self.apply_phpunit_assertion_narrowing(e, locals);
                // A bare `@phpstan-assert Type $target` on this call's
                // own declaration - the same "narrows unconditionally,
                // no branch needed" shape `assert($cond)`/PHPUnit's own
                // assertions get just above, generalized via a real
                // `FunctionReflection` lookup instead of a hardcoded name table
                // (see `FunctionReflection::assert_if_true`'s own doc comment).
                self.apply_bare_statement_assert_narrowing(e, locals);
                if inhabited
                    .iter()
                    .any(|k| locals.get(*k).is_some_and(Type::is_never))
                {
                    value_is_never = true;
                }
                // A bare `$cond || neverReturningCall();` statement -
                // PHP's classic "guard or die" idiom
                // (`$pos !== false || $this->ierror('Missing .', 400);`).
                // `ierror(): never` is a real `never` return type; if
                // execution reaches the statement *after* this one, the
                // `||`'s right side didn't run, so (by short-circuit
                // evaluation) the left side was already true - the same
                // "statement narrows unconditionally" reasoning as
                // `assert($cond)` above, just for a condition spelled as
                // an `Or` instead of a direct call. `Or`-only, not `And` -
                // `A && B;` gives no equivalent guarantee (if `A` is
                // false, `B` never runs and the statement simply ends,
                // without proving `A` either way).
                if let ExprKind::Binary(BinOp::Or | BinOp::LogicalOr, lhs, rhs) = &e.kind
                    && arenas
                        .expr(*rhs)
                        .is_some_and(|rhs| self.infer_expr(rhs, locals).is_never())
                    && let Some(lhs) = arenas.expr(*lhs)
                {
                    self.apply_narrowing(lhs, locals, true);
                }
            }
            StmtKind::Return(Some(e_id)) => {
                let Some(e) = arenas.expr(*e_id) else {
                    return completion;
                };
                let checked = self.check_expr(e, locals, label);
                let actual = return_var_tag.clone().unwrap_or(checked);
                // Real PHPStan pushes the actual returned value's type
                // onto its per-declaration "actually returned"
                // accumulator here - never for a generator (`return $x;`
                // inside one sets `Generator::getReturn()`, not this
                // function's own return value).
                if !self.current_function_is_generator {
                    self.current_function_saw_explicit_return = true;
                    self.current_function_return_points.push(actual.clone());
                }
                self.record_by_ref_exit_snapshot(locals);
                if let Some(expected) = declared_return {
                    let expected = match &self.current_class {
                        Some(c) => self.resolve_pseudo(expected, c),
                        None => expected.clone(),
                    };
                    // Inside a real generator, a `return $x;` statement's
                    // value sets `Generator::getReturn()` - it's checked
                    // against the declared
                    // `Generator<TKey, TValue, TSend, TReturn>`'s own
                    // TReturn, never against the whole `Generator<...>`
                    // type itself (that would always mismatch: no real
                    // `return` statement inside a generator body ever
                    // produces an actual `Generator` object).
                    let expected = if self.current_function_is_generator {
                        generator_template_param(&expected, self.registry, "TReturn")
                    } else {
                        expected
                    };
                    // A `never`-typed function/method reaching any
                    // `return` at all - value or bare - is its own
                    // dedicated real-PHPStan identifier/message
                    // (`return.never`, "should never return but return
                    // statement found"), not an ordinary type mismatch -
                    // `never` promises the caller execution can't
                    // continue past this call, a stronger claim than
                    // "returns the wrong type." Checked before the
                    // generic path below, which would otherwise report
                    // this identically to any other type mismatch.
                    if expected.is_never() {
                        report!(self, 
                            "should never return, but a return statement was reached".to_string(),
                            self.expr_span(e.id),
                            3,
                            "return.never",
                        );
                    } else if expected.is_void() && !actual.is_void() {
                        // A `void` function returning an actual value is
                        // real PHPStan's own dedicated `return.void`,
                        // not the generic mismatch path (`void` isn't a
                        // "wrong type" the way e.g. `string` vs `int`
                        // is - it's "no value was ever supposed to come
                        // back"). `!actual.is_void()` is a real,
                        // necessary exception: `return anotherVoidFn();`
                        // delegating to another void-returning call is
                        // legal, common PHP (real PHPStan doesn't flag
                        // it either - a `void` function returning
                        // nothing new, just handing back void-ness
                        // itself, isn't "returning a value" at all) -
                        // without it, every such delegation would report
                        // the nonsensical "expected void, got void".
                        report!(self, 
                            format!("return type mismatch: expected void, got {actual}"),
                            self.expr_span(e.id),
                            3,
                            "return.void",
                        );
                    // See `checker::calls`' own identical comment:
                    // `rejected_from` already accepts a `Mixed` actual
                    // value below real PHPStan's own level 9/10.
                    } else if let Some(from) = self.rejected_from(&expected, &actual) {
                        // `DiscriminantArrayFactsMap`'s own doc comment -
                        // before reporting a real mismatch, retry against
                        // a *reconstructed* type built from this exact
                        // returned variable's own per-branch accumulator
                        // history, when any was recorded. Only ever
                        // *suppresses* a diagnostic the ordinary,
                        // still-flat tracked type would otherwise
                        // produce - never used in place of `actual` for
                        // anything else, so a function this mechanism
                        // doesn't apply to (the overwhelming majority)
                        // sees no change in behavior at all.
                        // The reconstructed types pass at the levels that
                        // accept every one of them: the mismatch is found
                        // from the lowest level that rejects one of them,
                        // and at none when every level accepts them all.
                        let reconstructed = reconstructed_discriminant_return_types(e, locals);
                        let found_from = if reconstructed.is_empty() {
                            Some(from)
                        } else {
                            reconstructed
                                .iter()
                                .filter_map(|r| self.rejected_from(&expected, r))
                                .min()
                                .map(|passes_below| from.max(passes_below))
                        };
                        if let Some(from) = found_from {
                            report_found_from!(self, 
                                format!("return type mismatch: expected {expected}, got {actual}"),
                                self.expr_span(e.id),
                                3,
                                from,
                                "return.type",
                            );
                        }
                    }
                }
            }
            // A bare `return;` always returns `null` in real PHP,
            // regardless of the function's declared return type - a
            // genuine mismatch inside a function declared to return a
            // concrete, non-nullable, non-`void` type (the same easy
            // slip as forgetting to add a return value after an
            // early-exit guard clause).
            StmtKind::Return(None) => {
                // Mirrors real PHPStan pushing `new VoidType()` for a
                // bare `return;` - deliberately `void`, not
                // `null` (`Void` is a return-position-only marker
                // meaning "produced no value," while a too-wide check's
                // accumulated union needs to know that specifically, not
                // conflate it with an actual `null` value).
                if !self.current_function_is_generator {
                    self.current_function_saw_explicit_return = true;
                    self.current_function_return_points.push(Type::void());
                }
                self.record_by_ref_exit_snapshot(locals);
                // A bare `return;` inside a generator sets
                // Generator::getReturn()'s own value, not this function's
                // return value - real PHPStan never checks it against the
                // declared Generator/Iterator/Traversable return type
                // directly. It's still checked against the declared
                // TReturn specifically, same as `check_function`'s
                // falls-off-the-end completeness check - a bare
                // `return;` sets `getReturn()` to `null`, a real mismatch
                // when TReturn requires a concrete, non-nullable type.
                let generator_treturn = self
                    .current_function_is_generator
                    .then(|| {
                        generator_missing_return_type(
                            self.current_declared_return_type.as_ref(),
                            self.registry,
                        )
                    })
                    .flatten();
                if let Some(expected) = declared_return
                    && (!self.current_function_is_generator || generator_treturn.is_some())
                {
                    let expected = if let Some(treturn) = generator_treturn {
                        treturn
                    } else {
                        match &self.current_class {
                            Some(c) => self.resolve_pseudo(expected, c),
                            None => expected.clone(),
                        }
                    };
                    // See the identical `return.never` special case on
                    // `Return(Some(e))` just above - a bare `return;`
                    // reaches this exact "never returns" claim just as
                    // surely as one with a value does.
                    if expected.is_never() {
                        report!(self, 
                            "should never return, but a return statement was reached".to_string(),
                            self.stmt_span(stmt.id),
                            3,
                            "return.never",
                        );
                    // Real PHPStan's own `FunctionReturnTypeCheck::
                    // checkReturnType` - `return.empty`. Deliberately
                    // NOT keyed on whether `null` happens to be
                    // assignable to `expected` (this project's own
                    // former condition here, before this fix): real
                    // PHP's bare `return;` is not the same thing as
                    // `return null;` for a *typed* return position - a
                    // declared `?int`/`int|null` return type still
                    // requires an explicit `return null;`, exactly the
                    // way a real PHP runtime type error confirms
                    // (`Return value must be of type ?int, none
                    // returned`) - real PHPStan's own `Type::isVoid()`
                    // this check ports answers "does this type accept
                    // the special *void* marker," never "does it accept
                    // the *value* null," and every nullable/union type
                    // this project's own `Type` can represent answers
                    // that `No` the identical way a plain `null`
                    // itself does (confirmed directly against `vendor/
                    // phpstan-src`'s own `JustNullableTypeTrait`/
                    // `NullType::isVoid`) - only `void` (`Yes`) and
                    // `mixed` (`Maybe`, matching `MixedType::
                    // isVoid`, no explicit/implicit distinction there
                    // either) are exempt.
                    } else if !(expected.is_mixed() || expected.is_void()) {
                        report!(self, 
                            format!("should return {expected} but empty return statement found"),
                            self.stmt_span(stmt.id),
                            3,
                            "return.empty",
                        );
                    }
                }
            }
            StmtKind::Echo(exprs) => {
                for (i, e_id) in arenas.expr_ids(exprs).iter().enumerate() {
                    let Some(e) = arenas.expr(*e_id) else {
                        continue;
                    };
                    let ty = self.check_expr(e, locals, label);
                    self.check_echo_validity(&ty, i, self.expr_span(e.id));
                    // `echo` implicitly invokes `__toString()` on an
                    // object argument - see
                    // `check_implicit_tostring_usage`'s doc comment.
                    self.check_implicit_tostring_usage(&ty, self.expr_span(e.id));
                    self.forget_what_an_operation_may_change(
                        &ty,
                        crate::checker::call_effects::is_this(self, e),
                        crate::registry::summaries::Operation::String,
                        locals,
                    );
                }
            }
            StmtKind::Block(stmts) => {
                completion = self
                    .check_block(arenas.stmt_ids(stmts), locals, declared_return, label)
                    .completion
            }
            StmtKind::Declare {
                body: Some(stmts), ..
            } => {
                completion = self
                    .check_block(arenas.stmt_ids(stmts), locals, declared_return, label)
                    .completion
            }
            StmtKind::If(if_stmt) => {
                let mut branches: Vec<(&ExprNode, &StmtNode)> =
                    Vec::with_capacity(1 + arenas.elseifs(if_stmt.elseifs.clone()).len());
                branches.push((
                    arenas
                        .expr(if_stmt.cond)
                        .expect("if condition belongs to its file owner"),
                    arenas
                        .stmt(if_stmt.then_branch)
                        .expect("if body belongs to its file owner"),
                ));
                for elseif in arenas.elseifs(if_stmt.elseifs.clone()) {
                    branches.push((
                        arenas
                            .expr(elseif.cond)
                            .expect("elseif condition belongs to its file owner"),
                        arenas
                            .stmt(elseif.body)
                            .expect("elseif body belongs to its file owner"),
                    ));
                }
                let else_branch = if_stmt.else_branch.map(|id| {
                    arenas
                        .stmt(id)
                        .expect("else body belongs to its file owner")
                });
                completion = self.check_if(
                    &branches,
                    else_branch,
                    locals,
                    declared_return,
                    label,
                    false,
                );
            }
            StmtKind::While { cond, body } => {
                let cond = arenas
                    .expr(*cond)
                    .expect("while condition belongs to its file owner");
                let body = arenas
                    .stmt(*body)
                    .expect("while body belongs to its file owner");
                self.check_while(cond, body, locals, declared_return, label);
                completion =
                    Completion::while_loop(is_truthy_literal(cond), self.loop_body_completion);
            }
            StmtKind::DoWhile { body, cond } => {
                let body = arenas
                    .stmt(*body)
                    .expect("do-while body belongs to its file owner");
                let cond = arenas
                    .expr(*cond)
                    .expect("do-while condition belongs to its file owner");
                self.check_do_while(body, cond, locals, declared_return, label);
                completion =
                    Completion::do_while(is_truthy_literal(cond), self.loop_body_completion);
            }
            StmtKind::For {
                init,
                cond,
                step,
                body,
            } => {
                let init = arenas.expr_ids(init);
                let cond = arenas.expr_ids(cond);
                let step = arenas.expr_ids(step);
                self.check_for(init, cond, step, *body, locals, declared_return, label);
                completion = Completion::while_loop(
                    cond.last()
                        .and_then(|id| arenas.expr(*id))
                        .is_none_or(is_truthy_literal),
                    self.loop_body_completion,
                );
            }
            StmtKind::Foreach {
                subject,
                key,
                value,
                by_ref,
                body,
            } => {
                let subject = arenas
                    .expr(*subject)
                    .expect("foreach subject belongs to its file owner");
                let key = key.map(|id| {
                    arenas
                        .expr(id)
                        .expect("foreach key belongs to its file owner")
                });
                let value = arenas
                    .expr(*value)
                    .expect("foreach value belongs to its file owner");
                let body = arenas
                    .stmt(*body)
                    .expect("foreach body belongs to its file owner");
                let at_least_once = self.check_foreach(
                    subject,
                    key,
                    value,
                    *by_ref,
                    body,
                    locals,
                    declared_return,
                    label,
                    &inline_var,
                );
                completion = Completion::foreach(at_least_once, self.loop_body_completion);
            }
            StmtKind::Switch { subject, cases } => {
                let subject = arenas
                    .expr(*subject)
                    .expect("switch subject belongs to its file owner");
                completion = self.check_switch(
                    subject,
                    arenas.switch_cases(cases.clone()),
                    self.stmt_span(stmt.id),
                    locals,
                    declared_return,
                    label,
                )
            }
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                completion = self.check_try(
                    arenas,
                    TryStatement {
                        body: body.clone(),
                        catches: arenas.catch_clauses(catches.clone()),
                        finally,
                    },
                    locals,
                    declared_return,
                    label,
                )
            }
            StmtKind::Throw(e_id) => {
                let Some(e) = arenas.expr(*e_id) else {
                    return completion;
                };
                self.check_throw(e, locals, label);
                completion = Completion::EXITS;
            }
            // Nothing after a `goto` is reached by falling through. It lands
            // at its label with the scope it left from.
            StmtKind::Goto(name) => {
                self.current_function_goto_targets
                    .points
                    .push((name.name.clone(), locals.clone()));
                completion = Completion::TERMINATES;
            }
            StmtKind::Unset(targets) => {
                for target in arenas.expr_ids(targets) {
                    if let Some(target) = arenas.expr(*target) {
                        self.check_unset_target(target, locals, label);
                    }
                }
            }
            // `global $x;` binds $x to whatever the global scope holds,
            // which this checker doesn't track - Mixed is the honest
            // answer, but the important part is that it's bound at all, so
            // using it afterward isn't flagged as undefined.
            StmtKind::Global(vars) => {
                for var in arenas.global_vars(vars.clone()) {
                    let name = match var {
                        GlobalVar::Name(name) => name,
                        // `global ${expr};` imports a variable whose name
                        // is only known at runtime.
                        GlobalVar::Dynamic(e) => {
                            let Some(e) = arenas.expr(*e) else {
                                continue;
                            };
                            let ty = self.check_expr(e, locals, label);
                            self.forget_what_an_operation_may_change(
                                &ty,
                                crate::checker::call_effects::is_this(self, e),
                                crate::registry::summaries::Operation::String,
                                locals,
                            );
                            continue;
                        }
                    };
                    // `$this` is real PHP's own implicitly-bound
                    // reference, not a real global - "Cannot use $this
                    // as global variable." is a real, unconditional
                    // fatal error.
                    if &**name == "this" {
                        report!(self, 
                            "cannot use $this as global variable".to_string(),
                            self.stmt_span(stmt.id),
                            0,
                            "global.this",
                        );
                        continue;
                    }
                    // A `global $x;` declares `$x` here. `Registry::
                    // global_variables` (`registry::global_vars::infer_
                    // global_variables`) may already have a real type
                    // folded from every assignment site to this name
                    // anywhere in the corpus - fall back to `Mixed` only
                    // when it doesn't. `.declare` unconditionally
                    // overwrites, so it's only called when there's
                    // nothing here yet to overwrite.
                    if !locals.contains_var(name) {
                        let ty = self
                            .registry
                            .global_variables()
                            .get(&**name)
                            .cloned()
                            .unwrap_or(Type::mixed());
                        self.probe_hit_at(self.stmt_span(stmt.id), &ty);
                        locals.declare_var(name, ty, name.span);
                    }
                }
            }
            StmtKind::StaticVar(vars) => {
                for variable in arenas.static_vars(vars.clone()) {
                    let name = &variable.name;
                    // Same idea as `global $this;` just above - "Cannot
                    // use $this as static variable."
                    if **name == *"this" {
                        report!(self, 
                            "cannot use $this as static variable".to_string(),
                            self.stmt_span(stmt.id),
                            0,
                            "static.this",
                        );
                        continue;
                    }
                    // The initializer only holds on a first call; later
                    // calls see whatever an earlier call left behind, so
                    // only its general type is known.
                    let ty = variable
                        .default
                        .and_then(|id| arenas.expr(id))
                        .map(|e| static_variable_type(&self.check_expr(e, locals, label)))
                        .unwrap_or(Type::mixed());
                    purge_stale_narrowing_for_var(locals, name);
                    self.probe_hit_at(self.stmt_span(stmt.id), &ty);
                    locals.declare_var(name, ty, name.span);
                    self.static_var_names.insert(name.name.clone());
                }
            }
            // A declaration nested in a block, as in the `if
            // (!function_exists('foo')) { function foo() {} }` polyfill
            // idiom or a class declared conditionally, has a scope of its
            // own. At a file's top level, `check_program` checks it once,
            // after the passes over the top level's statements.
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) if self.shared_scope.top_level() => {
            }
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => {
                self.check_nested_declaration(stmt)
            }
            // A bare `break;` (`None`, level 1) or an explicit `break N;`
            // targets the Nth-innermost open loop/switch frame -
            // `checked_sub` naturally no-ops for an out-of-range level
            // (more levels than open loops/switches, invalid PHP
            // anyway). The level is already parse-time-resolved to a
            // plain `i64`, no expression evaluation needed.
            StmtKind::Break(level) => {
                let depth = level.unwrap_or(1).max(1) as usize;
                if let Some(idx) = self.loop_break_states.len().checked_sub(depth) {
                    self.loop_break_states[idx].push(locals.clone());
                }
                completion = Completion::break_(level.unwrap_or(1));
            }
            // The `continue` counterpart of `Break` just above - see
            // `loop_continue_states`'s doc comment on `Checker` for why
            // this exists (feeding a `continue`-reached state into the
            // next convergence pass, not just past-the-loop exit state
            // the way `break`'s states are used).
            StmtKind::Continue(level) => {
                let depth = level.unwrap_or(1).max(1) as usize;
                if let Some(idx) = self.loop_continue_states.len().checked_sub(depth) {
                    self.loop_continue_states[idx].push(locals.clone());
                }
                // A `continue` of one level inside a `switch` is a `break`
                // of it: the switch's own rules read it as that.
                completion = Completion::continue_(level.unwrap_or(1));
            }
            // `ConstantAttributesRule` - a top-level (or nested,
            // conditionally-declared) `const` statement's own `#[Attr]`
            // group(s). Covers both the top-level program walk (which
            // already falls through to this same `check_stmt` catch-all
            // for anything it doesn't special-case) and any nested
            // occurrence - `GlobalConst` needs no class/FQN context the
            // way `ClassDecl` does, so one arm here is enough.
            StmtKind::GlobalConst(consts) => {
                let owner = self
                    .syntax_tree_owner()
                    .expect("global-constant values retain their parsed file owner");
                let arenas = owner.arenas();
                let consts = arenas.class_consts(consts);
                self.check_global_const_attributes(consts, self.stmt_span(stmt.id), label);
                for c in consts {
                    let expr = arenas
                        .expr(c.value)
                        .expect("global-constant value belongs to its parsed file owner");
                    let value = self.infer_expr(expr, locals);
                    self.check_configured_constant_value(
                        None,
                        c.fq_name(),
                        &value,
                        "constant.value",
                        c.span,
                    );
                }
            }
            _ => {}
        }
        // Re-applied *after* dispatch too, but only when this exact
        // statement is itself a plain assignment to the same variable the
        // assertion names (the bare `/** @var Type */ $x = expr();` form,
        // or an explicitly-named `@var Type $x` sitting on its own
        // assignment) - the whole point of `@var` there is to override
        // whatever the assignment's own natural inference produces, which
        // `bind_assignment_target` (called from the `match` above, for
        // the `Assign` case) would otherwise silently clobber the
        // pre-applied assertion with. For every other statement shape
        // (an unrelated statement merely *reading* the variable), this
        // never matches - the single before-dispatch application above
        // already covers it, since the statement's own body needs to see
        // the narrowed type *during* its own check, not after.
        //
        // Deliberately does NOT extend this to a tag sitting directly on
        // a `foreach` header (`/** @var T $x */ foreach (...) { ... }`):
        // `bind_assignment_target` for the loop variable, and the loop
        // body's own check, both happen *inside* the `Foreach` arm above,
        // before this point ever runs - reapplying only here would fix
        // the type seen *after* the loop but leave the body itself seeing
        // the loop's own natural (usually `Mixed`) element type, a
        // half-working result worse than not supporting the shape at
        // all. The evidenced real-world case (`findCallSites`) instead
        // puts the tag on a statement *inside* the loop body, which the
        // before-dispatch application above already handles correctly
        // and completely on its own.
        let owner = self
            .syntax_tree_owner()
            .expect("statement variable names retain their parsed file owner");
        let arenas = owner.arenas();
        let defined = if inline_var.is_empty() {
            Vec::new()
        } else {
            statement_defined_variable_names(arenas, &stmt.kind)
        };
        for (name, ty) in inline_var {
            if defined.iter().any(|defined| defined.as_str() == name) {
                purge_stale_narrowing_for_var(locals, &name);
                locals.insert_var(&name, ty);
            }
        }
        match &stmt.kind {
            StmtKind::Return(_) => {
                completion = Completion::EXITS;
                self.push_exit_point(ExitKind::Return, self.stmt_span(stmt.id));
            }
            StmtKind::Throw(_) => self.push_exit_point(ExitKind::Throw, self.stmt_span(stmt.id)),
            StmtKind::Break(level) => {
                self.push_exit_point(ExitKind::Break(level_of(*level)), self.stmt_span(stmt.id))
            }
            StmtKind::Continue(level) => self.push_exit_point(
                ExitKind::Continue(level_of(*level)),
                self.stmt_span(stmt.id),
            ),
            // An expression statement whose value is `never` (a call to a
            // function that never returns, `exit`), or an assertion that
            // can only fail: control does not pass it.
            StmtKind::Expression(_) if value_is_never => completion = Completion::EXITS,
            // A loop or a switch takes the `break`s and `continue`s that
            // name it.
            StmtKind::While { .. }
            | StmtKind::DoWhile { .. }
            | StmtKind::For { .. }
            | StmtKind::Foreach { .. }
            | StmtKind::Switch { .. } => self.settle_exit_points(exits_from),
            _ => {}
        }
        completion
    }

    /// A bare `@phpstan-assert Type $target` on this call's own
    /// declaration - the same "narrows unconditionally, no branch needed"
    /// shape `assert($cond)`/PHPUnit's own assertions get, generalized via
    /// a real `FunctionReflection` lookup instead of a hardcoded name table (see
    /// `FunctionReflection::assert_if_true`'s own doc comment).
    ///
    /// `@phpstan-assert` is vanishingly rare in real code, so the
    /// `infer_expr` receiver-type recompute and `find_method_owner`'s
    /// hierarchy walk for `MethodCall`, `StaticCall`'s own
    /// `lookup_method` walk, and `Call`'s `Registry::resolve_function`
    /// would be wasted work on nearly every call site.
    /// `registry.names_with_assert_tags` (see its own doc comment)
    /// turns that into a single cheap set-membership check
    /// by the call's bare name before paying for any receiver-type
    /// inference or hierarchy walk - safe because the set is a superset
    /// of "names that could possibly match" (see that field's doc
    /// comment for why it never under-approximates).
    fn apply_bare_statement_assert_narrowing(&mut self, e: &ExprNode, locals: &mut Locals) {
        let Some(owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = owner.arenas();
        match &e.kind {
            ExprKind::MethodCall(recv, method, args, _) => {
                if !self.registry.names_with_assert_tags.contains(&method.name) {
                    return;
                }
                let Some(recv) = arenas.expr(*recv) else {
                    return;
                };
                let recv_ty = self.infer_expr(recv, locals);
                if let Some((_, sig)) = self.find_method_owner(&recv_ty, method)
                    && !sig.assert().is_empty()
                {
                    self.apply_assert_infos(
                        sig.assert(),
                        sig,
                        Some(recv),
                        arenas.args(args.clone()),
                        locals,
                    );
                }
            }
            ExprKind::StaticCall(target, method, args) => {
                if !self.registry.names_with_assert_tags.contains(&method.name) {
                    return;
                }
                if let Some(class) = self.infer_static_target_class(target, locals)
                    && let Some(sig) = self.registry.lookup_method(&class, method)
                    && !sig.assert().is_empty()
                {
                    self.apply_assert_infos(
                        sig.assert(),
                        sig,
                        None,
                        arenas.args(args.clone()),
                        locals,
                    );
                }
            }
            ExprKind::Call(callee, args) => {
                let Some(callee) = arenas.expr(*callee) else {
                    return;
                };
                if let ExprKind::Name(name) = &callee.kind {
                    let Some(name) = arenas.qualified_name(name) else {
                        return;
                    };
                    let bare = name.rsplit('\\').next().unwrap_or(name);
                    if !self.registry.names_with_assert_tags.contains(bare) {
                        return;
                    }
                    if let Some((_, sig)) = self.registry.resolve_function(name)
                        && !sig.assert().is_empty()
                    {
                        self.apply_assert_infos(
                            sig.assert(),
                            sig,
                            None,
                            arenas.args(args.clone()),
                            locals,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// An `if` chain from `branches[0]` on: its condition, its body, then
    /// the rest of the chain (each later branch an `elseif`) or the
    /// `else`, in the scope the condition leaves false.
    ///
    /// `IfConstantConditionRule`/`ElseIfConstantConditionRule` trust
    /// type inference (`inferred_constant_condition`). An `elseif` gets its
    /// own identifier and message, and the last `elseif` with no `else`
    /// after it is exempt from `alwaysTrue` (PHPStan's
    /// `reportAlwaysTrueInLastCondition: false`): a trailing `elseif (true)`
    /// is the "acts as an implicit else" idiom. `alwaysFalse` is never
    /// exempted, and an `if` has no such position.
    #[allow(clippy::too_many_arguments)]
    fn check_if(
        &mut self,
        branches: &[(&ExprNode, &StmtNode)],
        else_branch: Option<&StmtNode>,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
        is_elseif: bool,
    ) -> Completion {
        syntax::stack::guard(|| {
            let [(cond, then_branch), rest @ ..] = branches else {
                return Completion::FALLS_THROUGH;
            };
            let (cond, then_branch) = (*cond, *then_branch);
            let is_last_without_else = rest.is_empty() && else_branch.is_none();
            self.in_condition_position = true;
            self.in_exempt_last_condition = is_elseif && is_last_without_else;
            let cond_ty = self.check_expr(cond, locals, label);
            self.check_non_boolean_condition(
                cond,
                &cond_ty,
                if is_elseif {
                    "an elseif condition"
                } else {
                    "an if condition"
                },
                if is_elseif {
                    "elseif.condNotBoolean"
                } else {
                    "if.condNotBoolean"
                },
            );
            let known_condition = self.inferred_constant_condition(cond, locals);
            if let Some(value) = known_condition {
                if is_elseif {
                    if !(value && is_last_without_else) {
                        report!(self, 
                            format!(
                                "Elseif condition is always {}.",
                                if value { "true" } else { "false" }
                            ),
                            self.expr_span(cond.id),
                            4,
                            if value {
                                "elseif.alwaysTrue"
                            } else {
                                "elseif.alwaysFalse"
                            },
                        );
                    }
                } else {
                    report!(self, 
                        format!(
                            "If condition is always {}.",
                            if value { "true" } else { "false" }
                        ),
                        self.expr_span(cond.id),
                        4,
                        if value {
                            "if.alwaysTrue"
                        } else {
                            "if.alwaysFalse"
                        },
                    );
                }
            }

            // Disjunctive-syllogism narrowing (`Checker::pending_or_
            // disjunctions`'s own doc comment): a `Binary(Or, l, r))`
            // condition's own true branch pushes `(l, r)` for the duration
            // of `then_branch`'s own check, so any nested `if` inside it
            // proving one disjunct false can assert the other true - popped
            // unconditionally afterward, matching an ordinary stack
            // discipline regardless of how `then_branch`'s own check
            // returns.
            let owner = self
                .syntax_tree_owner()
                .expect("if narrowing requires the syntax tree owner");
            let arenas = owner.arenas();
            let peeled_cond = arenas.peeled(cond.id).and_then(|id| arenas.expr(id));
            let or_disjuncts = match peeled_cond.map(|e| &e.kind) {
                Some(ExprKind::Binary(BinOp::Or | BinOp::LogicalOr, l, r)) => Some((*l, *r)),
                _ => None,
            };
            if let Some(pair) = or_disjuncts {
                self.pending_or_disjunctions.push(pair);
            }
            // `Checker::active_discriminant`'s own doc comment - a bare
            // boolean `Variable` condition is this mechanism's only
            // currently-recognized shape, matching this project's own
            // "narrow correctly first" precedent for a new fact-tracking
            // trigger.
            let discriminant_subject = match peeled_cond.map(|e| &e.kind) {
                Some(ExprKind::Variable(name)) => Some(locals.var_key(name)),
                _ => None,
            };
            let mut then_locals = locals.clone();
            self.apply_narrowing(cond, &mut then_locals, true);
            if let Some(subject) = &discriminant_subject {
                self.active_discriminant.push((*subject, true));
            }
            let then_completion =
                self.check_stmt(then_branch, &mut then_locals, declared_return, label);
            if discriminant_subject.is_some() {
                self.active_discriminant.pop();
            }
            if or_disjuncts.is_some() {
                self.pending_or_disjunctions.pop();
            }
            self.record_branch_implied_facts(cond, locals, &mut then_locals, true);

            let mut else_locals = locals.clone();
            self.apply_narrowing(cond, &mut else_locals, false);
            self.apply_disjunctive_syllogism(cond, &mut else_locals);
            // Before the else branch's own body is checked, so a flag the
            // else branch itself also reassigns still gets purged by that
            // check's own ordinary rebind-invalidation, the same as any
            // other alias.
            self.record_if_body_flag_alias(cond, then_branch, &mut then_locals, &mut else_locals);
            if let Some(subject) = &discriminant_subject {
                self.active_discriminant.push((*subject, false));
            }
            // How the rest of the chain, or the `else`, completes; a chain with
            // no `else` falls through.
            let else_completion = if !rest.is_empty() {
                self.check_if(
                    rest,
                    else_branch,
                    &mut else_locals,
                    declared_return,
                    label,
                    true,
                )
            } else if let Some(e) = else_branch {
                self.check_stmt(e, &mut else_locals, declared_return, label)
            } else {
                Completion::FALLS_THROUGH
            };
            if discriminant_subject.is_some() {
                self.active_discriminant.pop();
            }
            self.record_branch_implied_facts(cond, locals, &mut else_locals, false);

            // The classic guard clause - `if ($x === null) { return; }` -
            // depends on this: a branch that unconditionally
            // returns/throws/breaks/continues never reaches the merge point,
            // so only the surviving branch's (narrowed) scope should
            // propagate past the if. Without this, the terminated branch's
            // narrowing (here, "$x is null") would incorrectly leak into the
            // code after the guard.
            let then_terminates = then_completion.terminates;
            let else_terminates = else_completion.terminates;

            *locals = match (then_terminates, else_terminates) {
                (true, false) => else_locals,
                (false, true) => then_locals,
                _ => {
                    let mut merged = merge_scopes(&self.relation(), &then_locals, &else_locals);
                    // `cond`'s own discriminant cannot hold in the other
                    // branch, so its facts stay as that branch recorded them.
                    for (branch, branch_true) in [(&then_locals, true), (&else_locals, false)] {
                        if let Some(key) = self.primary_discriminant_key(cond, locals, branch_true)
                            && let Some(facts) = branch.branch_implied_facts().get(&key)
                        {
                            merged.branch_implied_facts_mut().insert(key, facts.clone());
                        }
                    }
                    merged
                }
            };
            // A condition known by its type to be true leaves only the
            // branch it picks, and one known false only the rest of the
            // chain. Otherwise the chain terminates when every branch does,
            // an `else` among them: `exits` when every branch exits.
            Completion::if_chain(then_completion, else_completion, known_condition)
        })
    }

    /// A statement's resolved `@var` tags; none on a declaration, whose
    /// own rules read its tags.
    fn statement_var_tags_of(&self, stmt: &StmtNode) -> Vec<(Option<String>, Type)> {
        let arenas = self.syntax_arenas();
        let Some(site) = arenas.and_then(|arenas| arenas.doc_site(self.stmt_span(stmt.id).start))
        else {
            return Vec::new();
        };
        let doc = site.text.as_ref();
        if !doc.contains("var")
            || matches!(
                stmt.kind,
                StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) | StmtKind::GlobalConst(_)
            )
        {
            return Vec::new();
        }
        self.statement_var_tags(doc)
    }

    /// Checks a function or class declared in a block, as `check_program`
    /// checks one declared at a file's top level; otherwise it would be
    /// indexed into the registry, so calls to it resolve, but exempt from
    /// every diagnostic.
    pub(crate) fn check_nested_declaration(&mut self, stmt: &StmtNode) {
        let syntax_owner = self.syntax_tree_owner();
        let arenas = syntax_owner
            .as_ref()
            .map(crate::checker::SyntaxTreeOwner::arenas);
        match &stmt.kind {
            StmtKind::FunctionDecl(id) => {
                let arenas = arenas.expect("nested function checks retain their parsed file owner");
                if let Some(function) = arenas.function(*id) {
                    self.check_function(function, None, false, true);
                }
            }
            StmtKind::ClassDecl(c) => {
                let arenas = arenas.expect("nested class checks retain their parsed file owner");
                let c = arenas.class_decl(c.clone());
                let def = arenas
                    .class(c.def)
                    .expect("nested class definition belongs to its retained file owner");
                let fqn = c.fq_name().to_string();
                self.check_nested_class_declaration(&fqn, c);
                if self.settings.sidz.check_magic_numbers {
                    self.check_magic_numbers_in_properties(arenas, def.properties(arenas));
                }
                let class_attrs = arenas.attributes(def.attrs.clone());
                self.check_attributes(class_attrs, &fqn);
                self.check_attribute_targets(class_attrs, ATTRIBUTE_TARGET_CLASS, "class");
                self.check_self_attribute_class_declaration(
                    class_attrs,
                    &fqn,
                    def.kind,
                    def.is_abstract,
                );
                self.check_class_like_marker_attributes(
                    class_attrs,
                    def.kind,
                    def.is_readonly,
                    &fqn,
                );
                if def.kind != ClassKind::Enum {
                    self.check_enum_cases_outside_enum(def.enum_cases(arenas));
                }
                for case in def.enum_cases(arenas) {
                    let attrs = arenas.attributes(case.attrs.clone());
                    self.check_attributes(attrs, &fqn);
                    self.check_attribute_targets(
                        attrs,
                        ATTRIBUTE_TARGET_CLASS_CONSTANT,
                        "class constant",
                    );
                }
                for p in def.properties(arenas) {
                    let attrs = arenas.attributes(p.attrs.clone());
                    self.check_attributes(attrs, &fqn);
                    self.check_attribute_targets(attrs, ATTRIBUTE_TARGET_PROPERTY, "property");
                }
                for cc in def.consts(arenas) {
                    let attrs = arenas.attributes(cc.attrs.clone());
                    self.check_attributes(attrs, &fqn);
                    self.check_attribute_targets(
                        attrs,
                        ATTRIBUTE_TARGET_CLASS_CONSTANT,
                        "class constant",
                    );
                }
                // A conditionally-declared trait (the common `if
                // (version_compare(...)) { trait Foo {...} }`/`if
                // (PHP_VERSION_ID ...) { trait Foo {...} }` shape)
                // needs the same
                // `check_trait_methods` dispatch `check_program`'s
                // top-level walk uses, so its `$this->hostOnlyMethod()`
                // calls are checked against a real using class, not
                // standalone - an unconditional per-method loop here
                // would instead check every trait method with
                // `current_class` set to the trait's own name, wrongly
                // flagging a method provided only by a sibling trait on
                // the real host class.
                if def.kind == ClassKind::Trait {
                    self.check_trait_methods(&fqn, c);
                } else {
                    for m in arenas.methods(def.methods.clone()) {
                        self.check_function(
                            &m.function,
                            Some(&fqn),
                            m.is_static,
                            !m.is_abstract && def.kind != ClassKind::Interface,
                        );
                    }
                    self.record_property_type_site(arenas, &fqn, vec![Name::from(&fqn)], def);
                }
            }
            _ => {}
        }
    }

    /// Checks the declarations `stmt` holds in its blocks
    /// (`check_nested_declaration`); not those in its expressions, which
    /// belong to a closure or an anonymous class and are checked with it.
    pub(crate) fn check_nested_declarations(&mut self, stmt: &StmtNode) {
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        let mut declarations = NestedDeclarations(Vec::new());
        declarations.visit_stmt(arenas, stmt.id);
        for declaration in declarations.0 {
            if let Some(declaration) = arenas.stmt(declaration) {
                self.check_nested_declaration(declaration);
            }
        }
    }
}

/// See `Checker::check_nested_declarations`.
struct NestedDeclarations(Vec<StmtId>);

impl Visitor for NestedDeclarations {
    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else {
            return;
        };
        match &stmt.kind {
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => {
                self.0.push(id);
            }
            _ => arena::walk_stmt(self, arenas, id),
        }
    }

    fn visit_expr(&mut self, _: &NodeArenas, _: ExprId) {}
}

/// The `(name, type)` assertions a statement's `@var` tags make: each
/// named tag, and a single unnamed one for the one variable the
/// statement defines (see `statement_defined_variable_names`).
fn inline_var_annotations(
    arenas: &NodeArenas,
    stmt: &StmtNode,
    tags: Vec<(Option<String>, Type)>,
) -> Vec<(String, Type)> {
    let unnamed_target = match statement_defined_variable_names(arenas, &stmt.kind).as_slice() {
        [single] => Some(single.to_string()),
        _ => None,
    };
    let unnamed = tags.iter().filter(|(n, _)| n.is_none()).count();
    let mut out: Vec<(String, Type)> = Vec::new();
    for (name, ty) in tags {
        match name {
            Some(name) => out.push((name, ty)),
            None if unnamed == 1 => {
                if let Some(target) = &unnamed_target {
                    out.push((target.clone(), ty));
                }
            }
            None => {}
        }
    }
    out
}

/// Every bare-variable name `kind` itself unconditionally (re)binds via a
/// plain `$x = ...;` expression statement's own LHS. Used two ways:
/// inferring a *nameless* `@var Type` tag's own target (only when there's
/// exactly one candidate), and deciding whether an inline `@var` assertion
/// (named or not) needs re-applying *after* this statement's own dispatch,
/// because the assignment's own natural binding would otherwise silently
/// overwrite it first. Deliberately doesn't cover `foreach` - see the
/// comment in `check_stmt` on why a tag on a `foreach` header isn't
/// supported at all, rather than half-supported.
/// The condition to narrow for, if `e` is a bare `assert(...)` call
/// (`\assert` included), or a top-level `&&`-chain whose *leftmost*
/// operand is one - `None` otherwise. Only the leftmost position is
/// safe: `&&` evaluates left to right and short-circuits on the first
/// falsy operand, so the leftmost operand is unconditionally evaluated
/// the moment this whole statement is reached at all, and (being an
/// `assert()` call) throws rather than let execution continue if its
/// own condition were false - meaning reaching *past* this statement
/// proves it held, exactly like the bare, whole-statement case. Any
/// operand *after* the first has no such guarantee (if an earlier
/// operand was falsy, `&&` short-circuits before ever reaching it at
/// all, and the statement still completes normally either way) - not
/// walked past the leftmost position, even for a real, common `assert(
/// $a) && assert($b);` double-assertion, a real, disclosed scope trim
/// rather than a wrong answer.
fn bare_assert_condition(arenas: &NodeArenas, id: ExprId) -> Option<ExprId> {
    fn as_bare_assert_call(arenas: &NodeArenas, id: ExprId) -> Option<ExprId> {
        let e = arenas.expr(id)?;
        let ExprKind::Call(callee, args) = &e.kind else {
            return None;
        };
        let callee = arenas.expr(*callee)?;
        let ExprKind::Name(name) = &callee.kind else {
            return None;
        };
        let name = arenas.qualified_name(name)?;
        if name.trim_start_matches('\\') != "assert" {
            return None;
        }
        arenas.args(args.clone()).first().map(|a| a.value)
    }
    let e = arenas.expr(id)?;
    match &e.kind {
        ExprKind::Binary(BinOp::And | BinOp::LogicalAnd, l, _) => as_bare_assert_call(arenas, *l),
        _ => as_bare_assert_call(arenas, id),
    }
}

fn statement_defined_variable_names(arenas: &NodeArenas, kind: &StmtKind) -> Vec<Name> {
    match kind {
        StmtKind::Expression(_) => statement_assigned_variable_names(arenas, kind),
        // `StmtKind::StaticVar`'s own arm above infers purely from the
        // declaration's default expression (`static $x = null;` types
        // `$x` as exactly `Null`), so it needs the same "trust the
        // annotation over the initializer" override every other
        // assignment gets from `/** @var string|null */` - listed here
        // so both existing mechanisms (the nameless-tag target inference
        // and the post-dispatch reapplication) pick a `static`
        // declaration up for free.
        StmtKind::StaticVar(vars) => arenas
            .static_vars(vars.clone())
            .iter()
            .map(|variable| Name::from(&*variable.name))
            .collect(),
        // A bare `@var Type` header on a `foreach` targets the loop's
        // own value variable, mirroring every other statement kind
        // here - only when it's a plain `$value` (not a destructuring
        // target) *and* there's no separate key variable too (a
        // `foreach ($x as $k => $v)` binds two names, the same
        // ambiguity the `[single]` guard at every call site already
        // declines to guess between) - the same "don't guess" scope
        // every other unlisted shape already gets by falling through to
        // the empty-`Vec` arm.
        StmtKind::Foreach { key, value, .. } if key.is_none() => arenas
            .expr(*value)
            .and_then(|value| match &value.kind {
                ExprKind::Variable(name) => Some(vec![name.clone()]),
                _ => None,
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Every variable an assignment statement (`$x = ...`, `[$a, [$b]] =
/// ...`, `$x = &...`) binds.
fn statement_assigned_variable_names(arenas: &NodeArenas, kind: &StmtKind) -> Vec<Name> {
    fn collect(arenas: &NodeArenas, id: ExprId, out: &mut Vec<Name>) {
        let Some(target) = arenas.expr(id) else {
            return;
        };
        match &target.kind {
            ExprKind::Variable(name) => out.push(name.clone()),
            ExprKind::Destructure(items, _) => {
                for target in arenas
                    .destructure_items(items)
                    .iter()
                    .filter_map(|item| item.target)
                {
                    collect(arenas, target, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let StmtKind::Expression(e) = kind
        && let Some(e) = arenas.expr(*e)
        && let ExprKind::Assign(lhs, _) | ExprKind::AssignRef(lhs, _) = &e.kind
    {
        collect(arenas, *lhs, &mut out);
    }
    out
}

/// A static variable's type across calls: its initializer's general
/// type, with `null` (the "not computed yet" sentinel) unknown.
fn static_variable_type(initializer: &Type) -> Type {
    if initializer.is_null().yes() {
        Type::mixed()
    } else if initializer.is_array().yes() {
        Type::array(Type::unknown_array_key(), Type::mixed())
    } else {
        initializer.generalize()
    }
}

/// The plain variables an assignment target writes: `$x`, or each of a
/// `[$a, $b]`/`list($a, $b)` destructuring's.
pub(super) fn assigned_variable_names(arenas: &NodeArenas, target: ExprId) -> Vec<String> {
    let Some(target) = arenas.expr(target) else {
        return Vec::new();
    };
    syntax::stack::guard(|| match &target.kind {
        ExprKind::Variable(v) => vec![v.to_string()],
        ExprKind::Destructure(items, _) => arenas
            .destructure_items(items)
            .iter()
            .filter_map(|item| item.target)
            .flat_map(|id| assigned_variable_names(arenas, id))
            .collect(),
        _ => Vec::new(),
    })
}
