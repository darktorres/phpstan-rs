//! Opt-in `check_magic_numbers` (`sidz/phpstan-rules`): a bare numeric
//! literal (also under unary `+`/`-` or a `(string)` cast, and a numeric
//! string unless `magic_numbers_ignore_numeric_strings`) where a named
//! constant belongs:
//!
//! - a call or `new` argument (`magicNumber.argument`), except `define()`'s,
//!   which declare constants;
//! - a property default (`magicNumber.property`) or a parameter default
//!   (`magicNumber.defaultParameter`);
//! - either side of an arithmetic, bitwise, comparison or `and`/`or`/`xor`
//!   operator (`magicNumber.arithmetic`/`.bitwise`/`.comparison`/
//!   `.logical`);
//! - a `return` (`magicNumber.return`), an assignment
//!   (`magicNumber.assignment`), a ternary branch (`magicNumber.ternary`);
//! - a `switch` or `match` subject (`magicNumber.switch`/`.match`) or a
//!   case or arm condition (`magicNumber.switchCase`/`.matchArm`).
//!
//! `magic_number_exemptions` (default `[0, 1]`) lists values accepted
//! anywhere, compared by numeric value, sign included.

use crate::checker::Checker;
use syntax::ast::*;
use syntax::token::Span;

impl<'a> Checker<'a> {
    /// The value of `e` when it's a magic number.
    fn magic_number(&self, e: &Expr) -> Option<f64> {
        let value = self.numeric_literal_value(e)?;
        let exempt = match &self.settings.sidz.magic_number_exemptions {
            Some(numbers) => numbers.iter().any(|n| n.value() == value),
            None => value == 0.0 || value == 1.0,
        };
        (!exempt).then_some(value)
    }

    fn numeric_literal_value(&self, e: &Expr) -> Option<f64> {
        let e = e.peeled();
        syntax::stack::guard(|| match &e.kind {
            ExprKind::Literal(Literal::Int(i)) => Some(*i as f64),
            ExprKind::Literal(Literal::Float(f)) => Some(*f),
            ExprKind::Literal(Literal::String(s))
                if !self.settings.sidz.magic_numbers_ignore_numeric_strings =>
            {
                php_numeric_string(&s.to_str_lossy())
            }
            ExprKind::Unary(UnaryOp::Neg, inner) => self.numeric_literal_value(inner).map(|v| -v),
            ExprKind::Unary(UnaryOp::Plus, inner) => self.numeric_literal_value(inner),
            ExprKind::Cast(cast, inner) if cast.kind() == CastKind::String => {
                self.numeric_literal_value(inner)
            }
            _ => None,
        })
    }

    fn report_magic_number(&mut self, span: Span, id: &str, place: &str) {
        self.report(
            format!("Do not use magic number {place}. Move to constant with a suitable name."),
            span,
            0,
            format!("magicNumber.{id}"),
        );
    }

    fn check_magic_operand_pair(&mut self, l: &Expr, r: &Expr, span: Span, id: &str, place: &str) {
        if self.magic_number(l).is_some() || self.magic_number(r).is_some() {
            self.report_magic_number(span, id, place);
        }
    }

    pub(crate) fn check_magic_numbers_in_params(&mut self, params: &[Param]) {
        for param in params {
            if let Some(default) = &param.default
                && self.magic_number(default).is_some()
            {
                self.report_magic_number(default.span, "defaultParameter", "as default parameter");
            }
        }
    }

    pub(crate) fn check_magic_numbers_in_properties(&mut self, properties: &[PropertyDecl]) {
        for property in properties {
            if let Some(default) = &property.default
                && self.magic_number(default).is_some()
            {
                self.report("Do not assign magic number to property. Move to constant with a suitable name.".to_string(),
                    default.span,
                    0,
                    "magicNumber.property",
                );
            }
        }
    }

    pub(crate) fn check_magic_numbers_expression(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Call(callee, args) => {
                let is_define = matches!(&callee.kind, ExprKind::Name(name)
                    if name.trim_start_matches('\\').eq_ignore_ascii_case("define"));
                if !is_define {
                    self.check_magic_arguments(args);
                }
            }
            ExprKind::MethodCall(_, _, args, _)
            | ExprKind::StaticCall(_, _, args)
            | ExprKind::DynamicMethodCall(_, _, args, _)
            | ExprKind::DynamicStaticCall(_, _, args)
            | ExprKind::New(_, args) => {
                self.check_magic_arguments(args);
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                self.check_magic_arguments(args);
                self.check_magic_numbers_in_properties(&decl.properties);
            }
            ExprKind::Closure { params, .. } | ExprKind::ArrowFn { params, .. } => {
                self.check_magic_numbers_in_params(params);
            }
            ExprKind::Binary(op, l, r) => {
                let (id, place) = match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow => {
                        ("arithmetic", "in arithmetic operations")
                    }
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                        ("bitwise", "in bitwise operations")
                    }
                    BinOp::Eq
                    | BinOp::NotEq
                    | BinOp::Identical
                    | BinOp::NotIdentical
                    | BinOp::Lt
                    | BinOp::Gt
                    | BinOp::LtEq
                    | BinOp::GtEq
                    | BinOp::Spaceship => ("comparison", "in comparison operations"),
                    BinOp::LogicalAnd | BinOp::LogicalOr | BinOp::LogicalXor => {
                        ("logical", "in logical operations")
                    }
                    _ => return,
                };
                self.check_magic_operand_pair(l, r, e.span, id, place);
            }
            ExprKind::Ternary(_, then, otherwise) => {
                let then_magic = then
                    .as_deref()
                    .is_some_and(|t| self.magic_number(t).is_some());
                if then_magic || self.magic_number(otherwise).is_some() {
                    self.report_magic_number(e.span, "ternary", "in ternary operator");
                }
            }
            ExprKind::Assign(_, value) => {
                if self.magic_number(value).is_some() {
                    self.report("Do not assign magic number to variable. Move to constant with a suitable name.".to_string(),
                        e.span,
                        0,
                        "magicNumber.assignment",
                    );
                }
            }
            ExprKind::Match { subject, arms, .. } => {
                if self.magic_number(subject).is_some() {
                    self.report_magic_number(subject.span, "match", "in match condition");
                }
                for conditions in arms.iter().filter_map(|arm| arm.conditions.as_ref()) {
                    for condition in conditions {
                        if self.magic_number(condition).is_some() {
                            self.report_magic_number(
                                condition.span,
                                "matchArm",
                                "in match condition statement",
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn check_magic_arguments(&mut self, args: &[Arg]) {
        for arg in args {
            if self.magic_number(&arg.value).is_some() {
                self.report_magic_number(arg.value.span, "argument", "as a function argument");
            }
        }
    }

    pub(crate) fn check_magic_numbers_statement(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Return(Some(value)) if self.magic_number(value).is_some() => {
                self.report(
                    "Do not return magic number. Move to constant with a suitable name."
                        .to_string(),
                    stmt.span,
                    0,
                    "magicNumber.return",
                );
            }
            StmtKind::Switch { subject, cases } => {
                if self.magic_number(subject).is_some() {
                    self.report_magic_number(subject.span, "switch", "in case condition");
                }
                for case in cases {
                    if case
                        .test
                        .as_ref()
                        .is_some_and(|t| self.magic_number(t).is_some())
                    {
                        self.report_magic_number(case.span, "switchCase", "in case statement");
                    }
                }
            }
            _ => {}
        }
    }
}

/// PHP's `is_numeric()` for a string literal, with its value.
fn php_numeric_string(s: &str) -> Option<f64> {
    if !types::is_numeric_string_literal(s) {
        return None;
    }
    s.trim_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c'])
        .parse::<f64>()
        .ok()
}
