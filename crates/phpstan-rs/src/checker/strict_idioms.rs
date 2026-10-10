//! Two opt-in families (`rector/type-perfect`/`shipmonk` style):
//!
//! `check_null_coercion` - `isset()`/`empty()` on an object-typed value
//! (`isset.object`/`empty.object`: a null check or `instanceof` says what
//! is meant), and a possibly-null value PHP silently coerces: in string
//! interpolation (`interpolation.nullable`), concatenation
//! (`concat.nullable`), arithmetic (`binaryOp.nullable`), or a compound
//! assignment (`assignOp.nullable`).
//!
//! `check_enum_exhaustiveness` - a `match` over an enum with a `default`
//! arm (`match.enumDefault`: a newly added case is silently absorbed), and
//! a `switch` (`enum.switch`) or an `if`/`elseif` chain of `===`
//! comparisons (`enum.ifChain`) over enum cases, where a `match` would be
//! checked for exhaustiveness.
//!
//! `check_identity_vs_equals` - `===`/`!==` between two objects whose
//! class defines an `equals()` method accepting the other side
//! (`identical.equalsMethod`): the class compares by value, identity
//! only holds for the very same instance.

use crate::checker::{Checker, Locals};
use syntax::ast::*;
use types::Type;

/// Whether `ty` is `null` joined with something known.
fn is_nullable(ty: &Type) -> bool {
    let members = ty.union_members();
    members.iter().any(|m| m.is_null().yes())
        && members.iter().any(|m| !m.is_null().yes() && !m.is_mixed())
}

/// Whether every value of `ty` is an object, or `null`.
fn is_object_like(ty: &Type) -> bool {
    !ty.is_null().yes()
        && ty
            .union_members()
            .iter()
            .all(|m| m.is_null().yes() || m.is_object().yes())
}

impl<'a> Checker<'a> {
    fn enum_of(&self, ty: &Type) -> Option<String> {
        let classes = ty.object_class_names();
        let (&class, rest) = classes.split_first()?;
        if rest.iter().any(|c| *c != class) {
            return None;
        }
        self.registry
            .class_by_name(class)
            .is_some_and(|i| i.kind() == ClassKind::Enum)
            .then_some(class.to_string())
    }

    /// The class of `ty`, when it declares a public instance `equals()`
    /// taking one required argument `other` can be passed as; `None`
    /// inside that class's own `equals()`, which may use identity as its
    /// fast path.
    fn equals_method_owner(&self, ty: &Type, other: &Type) -> Option<String> {
        let (class, _) = ty.as_object()?;
        if !other.is_object().yes() {
            return None;
        }
        let info = self.registry.class_by_name(class)?;
        if info.kind() == ClassKind::Enum {
            return None;
        }
        let name = self
            .registry
            .lookup_method_canonical_name(class, "equals")?;
        let (owner, sig) = self.lookup_method_with_owner(class, "equals")?;
        let is_static = self.registry.method_is_static(owner.as_ref(), name);
        let required = sig
            .params()
            .iter()
            .filter(|p| !p.has_default() && !p.variadic())
            .count();
        if is_static
            || sig.visibility() != Visibility::Public
            || required != 1
            || !self.accepts(sig.params()[0].ty(), other).yes()
        {
            return None;
        }
        let inside_equals = self
            .current_method
            .as_deref()
            .is_some_and(|m| m.eq_ignore_ascii_case("equals"))
            && self.current_class.as_deref().is_some_and(|c| {
                self.registry.is_subclass_of(c, owner.as_ref())
                    || self.registry.is_subclass_of(owner.as_ref(), c)
            });
        (!inside_equals).then(|| class.to_string())
    }

    fn nullable_operand(
        &mut self,
        operand: &ExprNode,
        locals: &Locals,
        context: &str,
        id: &'static str,
    ) {
        let ty = self.infer_expr(operand, locals);
        if is_nullable(&ty) {
            report!(
                self,
                format!("A possibly-null value ({ty}) is silently coerced {context}."),
                self.expr_span(operand.id),
                0,
                id,
            );
        }
    }

    /// `isset`/`empty` of a variable or property holding an object, which a
    /// null check or `instanceof` says more precisely.
    fn check_object_probes(
        &mut self,
        arenas: &NodeArenas,
        which: &str,
        operands: &[ExprId],
        locals: &Locals,
    ) {
        for &operand_id in operands {
            let Some(operand) = arenas.expr(operand_id) else {
                continue;
            };
            // An offset or static probe asks whether the key or member
            // exists, not what the value is.
            if !matches!(
                operand.kind,
                ExprKind::Variable(_) | ExprKind::PropertyAccess(..)
            ) {
                continue;
            }
            let ty = self.infer_expr(operand, locals);
            if is_object_like(&ty) {
                let instead = if ty.is_null().maybe() {
                    "a !== null check"
                } else {
                    "instanceof"
                };
                report!(
                    self,
                    format!("{which}() on an object ({ty}) says less than {instead}."),
                    self.expr_span(operand.id),
                    0,
                    format!("{which}.object"),
                );
            }
        }
    }

    /// The expression-level checks, before `e` itself is checked.
    pub(crate) fn check_strict_expression_idioms(&mut self, e: &ExprNode, locals: &Locals) {
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        if self.settings.shipmonk.check_null_coercion {
            match &e.kind {
                ExprKind::Isset(operands) => {
                    self.check_object_probes(arenas, "isset", arenas.expr_ids(operands), locals)
                }
                ExprKind::Empty(operand) => {
                    self.check_object_probes(arenas, "empty", std::slice::from_ref(operand), locals)
                }
                ExprKind::Interpolated(parts) => {
                    for part in arenas
                        .expr_ids(parts)
                        .iter()
                        .filter_map(|id| arenas.expr(*id))
                    {
                        if !matches!(part.kind, ExprKind::Literal(Literal::String(_))) {
                            self.nullable_operand(
                                part,
                                locals,
                                "to \"\" in a string",
                                "interpolation.nullable",
                            );
                        }
                    }
                }
                ExprKind::Binary(BinOp::Concat, l, r) => {
                    for side in [*l, *r].into_iter().filter_map(|id| arenas.expr(id)) {
                        self.nullable_operand(
                            side,
                            locals,
                            "to \"\" by concatenation",
                            "concat.nullable",
                        );
                    }
                }
                ExprKind::Binary(
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow,
                    l,
                    r,
                ) => {
                    for side in [*l, *r].into_iter().filter_map(|id| arenas.expr(id)) {
                        self.nullable_operand(
                            side,
                            locals,
                            "to 0 by arithmetic",
                            "binaryOp.nullable",
                        );
                    }
                }
                ExprKind::CompoundAssign(op, target, value) if !matches!(op, BinOp::Coalesce) => {
                    for side in [*target, *value]
                        .into_iter()
                        .filter_map(|id| arenas.expr(id))
                    {
                        self.nullable_operand(
                            side,
                            locals,
                            "by a compound assignment",
                            "assignOp.nullable",
                        );
                    }
                }
                _ => {}
            }
        }
        if self.settings.extras.check_identity_vs_equals
            && let ExprKind::Binary(op @ (BinOp::Identical | BinOp::NotIdentical), l, r) = &e.kind
        {
            let (Some(left_expr), Some(right_expr)) = (arenas.expr(*l), arenas.expr(*r)) else {
                return;
            };
            let left = self.infer_expr(left_expr, locals);
            let right = self.infer_expr(right_expr, locals);
            let owner = match self.equals_method_owner(&left, &right) {
                Some(class) => Some((class, l, r)),
                None => self
                    .equals_method_owner(&right, &left)
                    .map(|class| (class, r, l)),
            };
            if let Some((class, receiver, argument)) = owner {
                let (operator, negation) = if *op == BinOp::Identical {
                    ("===", "")
                } else {
                    ("!==", "!")
                };
                report!(
                    self,
                    format!(
                        "{operator} between {class} objects compares identity; {class} defines equals() to compare values, use {negation}{}->equals({}).",
                        syntax::printer::print_expr(arenas, *receiver,),
                        syntax::printer::print_expr(arenas, *argument,)
                    ),
                    self.expr_span(e.id),
                    0,
                    "identical.equalsMethod",
                );
            }
        }
        if self.settings.shipmonk.check_enum_exhaustiveness
            && let ExprKind::Match { subject, arms } = &e.kind
        {
            let arms = arenas.match_arms(arms.clone());
            let subject = arenas
                .expr(*subject)
                .expect("match subject belongs to its retained file owner");
            if let Some(default) = match_default(arms).map(|id| {
                arenas
                    .expr(id)
                    .expect("match default body belongs to its retained file owner")
            }) && let Some(enum_name) = self.enum_of(&self.infer_expr(subject, locals))
            {
                report!(
                    self,
                    format!(
                        "match over enum {enum_name} has a default arm; a newly added case would be absorbed instead of flagged as unhandled."
                    ),
                    self.expr_span(default.id),
                    0,
                    "match.enumDefault",
                );
            }
        }
    }

    /// The statement-level checks, before `stmt` itself is checked.
    pub(crate) fn check_strict_statement_idioms(&mut self, stmt: &StmtNode, locals: &Locals) {
        if !self.settings.shipmonk.check_enum_exhaustiveness {
            return;
        }
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        match &stmt.kind {
            StmtKind::Switch { subject, .. } => {
                let Some(subject) = arenas.expr(*subject) else {
                    return;
                };
                if let Some(enum_name) = self.enum_of(&self.infer_expr(subject, locals)) {
                    report!(
                        self,
                        format!(
                            "switch over enum {enum_name}; a match would be checked for exhaustiveness."
                        ),
                        self.stmt_span(stmt.id),
                        0,
                        "enum.switch",
                    );
                }
            }
            StmtKind::If(_) => {
                if self.enum_chain_tails.contains(&self.stmt_span(stmt.id)) {
                    return;
                }
                // The chain's conditions, `elseif`s and `else if`s alike,
                // while each compares one subject with an enum case.
                let mut compared: Vec<(String, String)> = Vec::new();
                let mut chain_ifs: Vec<syntax::span::Span> = Vec::new();
                let mut current = Some(stmt.id);
                'chain: while let Some(stmt_id) = current {
                    let Some(stmt) = arenas.stmt(stmt_id) else {
                        break 'chain;
                    };
                    let StmtKind::If(if_stmt) = &stmt.kind else {
                        break 'chain;
                    };
                    chain_ifs.push(
                        arenas
                            .stmt_span(stmt_id)
                            .expect("if statement has an arena span"),
                    );
                    for cond in if_stmt.conditions(arenas).filter_map(|id| arenas.expr(id)) {
                        let ExprKind::Binary(BinOp::Identical, l, r) = &cond.kind else {
                            break 'chain;
                        };
                        let (Some(left), Some(right)) = (arenas.expr(*l), arenas.expr(*r)) else {
                            break 'chain;
                        };
                        let (subject, case) = match (&left.kind, &right.kind) {
                            (_, ExprKind::ClassConstFetch(..)) => (*l, right),
                            (ExprKind::ClassConstFetch(..), _) => (*r, left),
                            _ => break 'chain,
                        };
                        let Some(enum_name) = self.enum_of(&self.infer_expr(case, locals)) else {
                            break 'chain;
                        };
                        compared.push((syntax::printer::print_expr(arenas, subject), enum_name));
                    }
                    current = if_stmt.else_branch;
                }
                if compared.len() >= 2 && compared.windows(2).all(|w| w[0] == w[1]) {
                    self.enum_chain_tails.extend(chain_ifs);
                    let enum_name = &compared[0].1;
                    report!(
                        self,
                        format!(
                            "if/elseif chain over cases of enum {enum_name}; a match would be checked for exhaustiveness."
                        ),
                        self.stmt_span(stmt.id),
                        0,
                        "enum.ifChain",
                    );
                }
            }
            _ => {}
        }
    }
}
