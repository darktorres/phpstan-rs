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

fn is_nullable(ty: &Type) -> bool {
    matches!(ty, Type::Union(members) if members.iter().any(|m| matches!(m, Type::Null))
        && members.iter().any(|m| !matches!(m, Type::Null | Type::Mixed(_))))
}

fn is_object_like(ty: &Type) -> bool {
    match ty {
        Type::Object(..) | Type::ObjectWithoutClass => true,
        Type::Union(members) => {
            members
                .iter()
                .any(|m| matches!(m, Type::Object(..) | Type::ObjectWithoutClass))
                && members
                    .iter()
                    .all(|m| matches!(m, Type::Object(..) | Type::ObjectWithoutClass | Type::Null))
        }
        Type::Intersection(members) => members
            .iter()
            .all(|m| matches!(m, Type::Object(..) | Type::ObjectWithoutClass)),
        _ => false,
    }
}

impl<'a> Checker<'a> {
    fn enum_of(&self, ty: &Type) -> Option<String> {
        let class = match ty {
            Type::Object(name, _) => name.clone(),
            Type::EnumCase(class, _) => class.clone(),
            Type::Union(members) => {
                let mut classes = members.iter().filter_map(|m| match m {
                    Type::Object(name, _) | Type::EnumCase(name, _) => Some(name.clone()),
                    _ => None,
                });
                let first = classes.next()?;
                if classes.all(|c| c == first) {
                    first
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        self.registry
            .classes
            .get(class.as_str())
            .is_some_and(|i| i.kind == ClassKind::Enum)
            .then_some(class.to_string())
    }

    /// The class of `ty`, when it declares a public instance `equals()`
    /// taking one required argument `other` can be passed as; `None`
    /// inside that class's own `equals()`, which may use identity as its
    /// fast path.
    fn equals_method_owner(&self, ty: &Type, other: &Type) -> Option<String> {
        let Type::Object(class, _) = ty else {
            return None;
        };
        if !matches!(other, Type::Object(..) | Type::ObjectWithoutClass) {
            return None;
        }
        let info = self.registry.classes.get(class.as_str())?;
        if info.kind == ClassKind::Enum {
            return None;
        }
        let name = self
            .registry
            .lookup_method_canonical_name(class, "equals")?;
        let (owner, sig) = self.registry.lookup_method_with_owner(class, "equals")?;
        let is_static = self
            .registry
            .classes
            .get(owner.as_ref())
            .is_some_and(|o| o.static_methods.contains(name));
        let required = sig
            .params
            .iter()
            .filter(|p| !p.has_default && !p.variadic)
            .count();
        if is_static
            || sig.visibility != Visibility::Public
            || required != 1
            || !self.registry.accepts(&sig.params[0].ty, other).yes()
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
        operand: &Expr,
        locals: &Locals,
        context: &str,
        id: &'static str,
    ) {
        let ty = self.infer_expr(operand, locals);
        if is_nullable(&ty) {
            self.report(
                format!("A possibly-null value ({ty}) is silently coerced {context}."),
                operand.span,
                0,
                id,
            );
        }
    }

    /// The expression-level checks, before `e` itself is checked.
    pub(crate) fn check_strict_expression_idioms(&mut self, e: &Expr, locals: &Locals) {
        if self.registry.check_null_coercion {
            match &e.kind {
                ExprKind::Call(callee, args) => {
                    if let ExprKind::Name(name) = &callee.kind
                        && let Some(which) = ["isset", "empty"]
                            .into_iter()
                            .find(|n| name.eq_ignore_ascii_case(n))
                    {
                        for arg in args {
                            // An offset or static probe asks whether the key
                            // or member exists, not what the value is.
                            if !matches!(
                                arg.value.kind,
                                ExprKind::Variable(_) | ExprKind::PropertyAccess(..)
                            ) {
                                continue;
                            }
                            let ty = self.infer_expr(&arg.value, locals);
                            if is_object_like(&ty) {
                                let instead = if matches!(ty, Type::Union(_)) {
                                    "a !== null check"
                                } else {
                                    "instanceof"
                                };
                                self.report(
                                    format!(
                                        "{which}() on an object ({ty}) says less than {instead}."
                                    ),
                                    arg.value.span,
                                    0,
                                    format!("{which}.object"),
                                );
                            }
                        }
                    }
                }
                ExprKind::Interpolated(parts) => {
                    for part in parts {
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
                    for side in [l, r] {
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
                    for side in [l, r] {
                        self.nullable_operand(
                            side,
                            locals,
                            "to 0 by arithmetic",
                            "binaryOp.nullable",
                        );
                    }
                }
                ExprKind::CompoundAssign(op, target, value) if !matches!(op, BinOp::Coalesce) => {
                    for side in [target, value] {
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
        if self.registry.check_identity_vs_equals
            && let ExprKind::Binary(op @ (BinOp::Identical | BinOp::NotIdentical), l, r) = &e.kind
        {
            let left = self.infer_expr(l, locals);
            let right = self.infer_expr(r, locals);
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
                self.report(format!(
                        "{operator} between {class} objects compares identity; {class} defines equals() to compare values, use {negation}{}->equals({}).",
                        syntax::printer::print_expr(receiver),
                        syntax::printer::print_expr(argument)
                    ),
                    e.span,
                    0,
                    "identical.equalsMethod",
                );
            }
        }
        if self.registry.check_enum_exhaustiveness
            && let ExprKind::Match {
                subject,
                default: Some(default),
                ..
            } = &e.kind
            && let Some(enum_name) = self.enum_of(&self.infer_expr(subject, locals))
        {
            self.report(format!("match over enum {enum_name} has a default arm; a newly added case would be absorbed instead of flagged as unhandled."),
                default.span,
                0,
                "match.enumDefault",
            );
        }
    }

    /// The statement-level checks, before `stmt` itself is checked.
    pub(crate) fn check_strict_statement_idioms(&mut self, stmt: &Stmt, locals: &Locals) {
        if !self.registry.check_enum_exhaustiveness {
            return;
        }
        match &stmt.kind {
            StmtKind::Switch { subject, .. } => {
                if let Some(enum_name) = self.enum_of(&self.infer_expr(subject, locals)) {
                    self.report(format!("switch over enum {enum_name}; a match would be checked for exhaustiveness."),
                        stmt.span,
                        0,
                        "enum.switch",
                    );
                }
            }
            StmtKind::If { .. } => {
                if self.enum_chain_tails.contains(&stmt.span) {
                    return;
                }
                let mut compared: Vec<(String, String)> = Vec::new();
                let mut chain_ifs: Vec<syntax::token::Span> = Vec::new();
                let mut current = Some(stmt);
                while let Some(StmtKind::If {
                    cond, else_branch, ..
                }) = current.map(|s| &s.kind)
                {
                    let ExprKind::Binary(BinOp::Identical, l, r) = &cond.kind else {
                        break;
                    };
                    let (subject, case) = match (&l.kind, &r.kind) {
                        (_, ExprKind::ClassConstFetch(..)) => (l, r),
                        (ExprKind::ClassConstFetch(..), _) => (r, l),
                        _ => break,
                    };
                    let Some(enum_name) = self.enum_of(&self.infer_expr(case, locals)) else {
                        break;
                    };
                    compared.push((syntax::printer::print_expr(subject), enum_name));
                    chain_ifs.push(current.map_or(stmt.span, |s| s.span));
                    current = else_branch.as_deref();
                }
                if compared.len() >= 2 && compared.windows(2).all(|w| w[0] == w[1]) {
                    self.enum_chain_tails.extend(chain_ifs);
                    let enum_name = &compared[0].1;
                    self.report(format!("if/elseif chain over cases of enum {enum_name}; a match would be checked for exhaustiveness."),
                        stmt.span,
                        0,
                        "enum.ifChain",
                    );
                }
            }
            _ => {}
        }
    }
}
