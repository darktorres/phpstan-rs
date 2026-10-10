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
use syntax::span::Span;

impl<'a> Checker<'a> {
    /// The value of `e` when it's a magic number.
    fn magic_number(&self, arenas: &NodeArenas, id: ExprId) -> Option<f64> {
        let value = self.numeric_literal_value(arenas, id)?;
        let exempt = match &self.settings.sidz.magic_number_exemptions {
            Some(numbers) => numbers.iter().any(|n| n.value() == value),
            None => value == 0.0 || value == 1.0,
        };
        (!exempt).then_some(value)
    }

    fn numeric_literal_value(&self, arenas: &NodeArenas, id: ExprId) -> Option<f64> {
        let id = arenas
            .peeled(id)
            .expect("numeric-expression ID belongs to its retained file owner");
        let e = arenas
            .expr(id)
            .expect("numeric expression belongs to its retained file owner");
        syntax::stack::guard(|| match &e.kind {
            ExprKind::Literal(Literal::Int(i)) => Some(*i as f64),
            ExprKind::Literal(Literal::Float(f)) => Some(*f),
            ExprKind::Literal(Literal::String(s))
                if !self.settings.sidz.magic_numbers_ignore_numeric_strings =>
            {
                let bytes = arenas.string_bytes(s)?;
                let text = std::str::from_utf8(bytes).ok()?;
                php_numeric_string(text)
            }
            ExprKind::Unary(UnaryOp::Neg, inner) => {
                self.numeric_literal_value(arenas, *inner).map(|v| -v)
            }
            ExprKind::Unary(UnaryOp::Plus, inner) => self.numeric_literal_value(arenas, *inner),
            ExprKind::Cast(cast, inner) if cast.kind() == CastKind::String => {
                self.numeric_literal_value(arenas, *inner)
            }
            _ => None,
        })
    }

    fn report_magic_number(&mut self, span: Span, id: &str, place: &str) {
        report!(self, 
            format!("Do not use magic number {place}. Move to constant with a suitable name."),
            span,
            0,
            format!("magicNumber.{id}"),
        );
    }

    fn check_magic_operand_pair(
        &mut self,
        arenas: &NodeArenas,
        l: ExprId,
        r: ExprId,
        span: Span,
        id: &str,
        place: &str,
    ) {
        if self.magic_number(arenas, l).is_some() || self.magic_number(arenas, r).is_some() {
            self.report_magic_number(span, id, place);
        }
    }

    pub(crate) fn check_magic_numbers_in_params(&mut self, arenas: &NodeArenas, params: &[Param]) {
        for param in params {
            if let Some(default) = param.default
                && self.magic_number(arenas, default).is_some()
            {
                let span = arenas
                    .expr_span(default)
                    .expect("parameter default belongs to its retained file owner");
                self.report_magic_number(span, "defaultParameter", "as default parameter");
            }
        }
    }

    pub(crate) fn check_magic_numbers_in_properties(
        &mut self,
        arenas: &NodeArenas,
        properties: &[PropertyDecl],
    ) {
        for property in properties {
            if let Some(default) = property.default
                && self.magic_number(arenas, default).is_some()
            {
                let span = arenas
                    .expr_span(default)
                    .expect("property default belongs to its retained file owner");
                report!(self, "Do not assign magic number to property. Move to constant with a suitable name.".to_string(),
                    span,
                    0,
                    "magicNumber.property",
                );
            }
        }
    }

    pub(crate) fn check_magic_numbers_expression(&mut self, e: &ExprNode) {
        let syntax_owner = self
            .syntax_tree_owner()
            .expect("magic-number checks retain their parsed file owner");
        let arenas = syntax_owner.arenas();
        match &e.kind {
            ExprKind::Call(callee, args) => {
                let callee = arenas
                    .expr(*callee)
                    .expect("call callee belongs to its retained file owner");
                let is_define = matches!(&callee.kind, ExprKind::Name(name)
                    if arenas.qualified_name(name).is_some_and(|name| name.trim_start_matches('\\').eq_ignore_ascii_case("define")));
                if !is_define {
                    self.check_magic_arguments(arenas, args.clone());
                }
            }
            ExprKind::MethodCall(_, _, args, _)
            | ExprKind::StaticCall(_, _, args)
            | ExprKind::DynamicMethodCall(_, _, args, _)
            | ExprKind::DynamicStaticCall(_, _, args)
            | ExprKind::New(_, args) => {
                self.check_magic_arguments(arenas, args.clone());
            }
            ExprKind::NewAnonymousClass(class_id) => {
                let class = arenas
                    .anonymous_class(*class_id)
                    .expect("anonymous class belongs to its retained file owner");
                let def = arenas
                    .class(class.def)
                    .expect("anonymous class definition belongs to its retained file owner");
                self.check_magic_arguments(arenas, class.args.clone());
                self.check_magic_numbers_in_properties(arenas, def.properties(arenas));
            }
            ExprKind::Closure(closure_id) | ExprKind::ArrowFn(closure_id) => {
                let payload = arenas
                    .closure(*closure_id)
                    .expect("closure payload belongs to its retained file owner");
                let params = match payload {
                    ClosurePayload::Closure(closure) => arenas.params(closure.params.clone()),
                    ClosurePayload::ArrowFn(arrow) => arenas.params(arrow.params.clone()),
                };
                self.check_magic_numbers_in_params(arenas, params);
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
                self.check_magic_operand_pair(arenas, *l, *r, self.expr_span(e.id), id, place);
            }
            ExprKind::Ternary(_, then, otherwise) => {
                let then_magic = then.is_some_and(|id| self.magic_number(arenas, id).is_some());
                if then_magic || self.magic_number(arenas, *otherwise).is_some() {
                    self.report_magic_number(
                        self.expr_span(e.id),
                        "ternary",
                        "in ternary operator",
                    );
                }
            }
            ExprKind::Assign(_, value) | ExprKind::AssignRef(_, value) => {
                if self.magic_number(arenas, *value).is_some() {
                    report!(self, "Do not assign magic number to variable. Move to constant with a suitable name.".to_string(),
                        self.expr_span(e.id),
                        0,
                        "magicNumber.assignment",
                    );
                }
            }
            ExprKind::Match { subject, arms, .. } => {
                if self.magic_number(arenas, *subject).is_some() {
                    let span = arenas
                        .expr_span(*subject)
                        .expect("match subject belongs to its retained file owner");
                    self.report_magic_number(span, "match", "in match condition");
                }
                for conditions in arenas
                    .match_arms(arms.clone())
                    .iter()
                    .filter_map(|arm| arm.conditions.as_ref())
                {
                    for condition in arenas.expr_ids(conditions).iter().copied() {
                        if self.magic_number(arenas, condition).is_some() {
                            let span = arenas
                                .expr_span(condition)
                                .expect("match condition belongs to its retained file owner");
                            self.report_magic_number(
                                span,
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

    fn check_magic_arguments(&mut self, arenas: &NodeArenas, args: ArgRange) {
        for arg in arenas.args(args.clone()) {
            if self.magic_number(arenas, arg.value).is_some() {
                let span = arenas
                    .expr_span(arg.value)
                    .expect("call argument belongs to its retained file owner");
                self.report_magic_number(span, "argument", "as a function argument");
            }
        }
    }

    pub(crate) fn check_magic_numbers_statement(&mut self, stmt: &StmtNode) {
        let syntax_owner = self
            .syntax_tree_owner()
            .expect("magic-number checks retain their parsed file owner");
        let arenas = syntax_owner.arenas();
        match &stmt.kind {
            StmtKind::Return(Some(value)) if self.magic_number(arenas, *value).is_some() => {
                report!(self, 
                    "Do not return magic number. Move to constant with a suitable name."
                        .to_string(),
                    self.stmt_span(stmt.id),
                    0,
                    "magicNumber.return",
                );
            }
            StmtKind::Switch { subject, cases } => {
                if self.magic_number(arenas, *subject).is_some() {
                    let span = arenas
                        .expr_span(*subject)
                        .expect("switch subject belongs to its retained file owner");
                    self.report_magic_number(span, "switch", "in case condition");
                }
                for case in arenas.switch_cases(cases.clone()) {
                    if let Some(test) = case.test
                        && self.magic_number(arenas, test).is_some()
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
