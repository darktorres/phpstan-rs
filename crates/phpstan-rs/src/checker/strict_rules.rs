//! Opt-in ports of `phpstan-strict-rules`, one registry flag per family:
//!
//! - `check_illegal_constructor_calls` - `$obj->__construct()`, and a
//!   static `X::__construct()` anywhere but a constructor calling one of
//!   its own parent classes' (`constructor.call`).
//! - `check_dynamic_static_calls` - a static method called, or turned
//!   into a first-class callable, through an instance
//!   (`staticMethod.dynamicCall`).
//! - `check_loop_variable_overwrite` - a `foreach` key or value variable
//!   (`foreach.keyOverwrite`/`foreach.valueOverwrite`) or a `for` loop's
//!   initial assignment (`for.variableOverwrite`) replacing a variable
//!   that is already defined.
//! - `check_switch_case_types` - a `switch` case whose type can never be
//!   the subject's (`switch.type`).
//! - `check_strict_function_calls` - `in_array()`/`array_search()`/
//!   `array_keys()`/`base64_decode()` without their strict argument
//!   `true`, and `iterator_to_array()` leaving `$preserve_keys` to its
//!   default (`function.strict`).
//! - `check_strict_array_filter` - `array_filter()` without a callback
//!   over values that aren't all booleans, all arrays, or a union of
//!   always-truthy and always-falsy types, or with a `null` callback
//!   (`arrayFilter.strict`); a callback that may be `null` is a level 8
//!   finding.
//! - `check_useless_casts` - a cast to a type the expression already has
//!   (`cast.useless`).
//! - `check_implicit_array_creation` - `$x[...] = ...` creating `$x`
//!   because it does not exist, or might not (`variable.implicitArray`).
//!   It might when the function assigns it earlier, or anywhere within a
//!   loop enclosing this assignment, this one included.
//! - `check_arithmetic_operands` - an operand of `+ - * / % **` (or their
//!   compound assignments), unary `+`/`-` or `++`/`--` that isn't a
//!   number (`plus.leftNonNumeric`, `unaryMinus.nonNumeric`,
//!   `postInc.nonNumeric`, ...). Arrays and objects are left to the
//!   core operand checks, except under `++`/`--`; a string may be
//!   incremented.
//! - `check_comparable_types` (from `shipmonk/phpstan-rules`) - `<`,
//!   `>`, `<=`, `>=` or `<=>` with an operand that isn't
//!   `int|float|string|DateTimeInterface` or a list tuple of those
//!   (`comparison.nonComparable`), or between two such types that don't
//!   order against each other (`comparison.differentTypes`).
//! - `check_mixed_type_operands` (from `voku/phpstan-rules`) - a string
//!   operand combined with a non-string one, or an array operand with a
//!   definite non-array, by any binary operator but `===`, `!==`, `??`,
//!   `&&`, `||` and `|>`, or by a compound assignment
//!   (`binaryOp.nonStringOperand`/`binaryOp.nonArrayOperand`,
//!   `assignOp.nonStringOperand`/`assignOp.nonArrayOperand`).
//!   Concatenation, `==` and `!=` accept any operand that converts to a
//!   string. Trait bodies are skipped: their `$this` has no single type.

use crate::checker::{Checker, Locals};
use crate::diagnostic::DiagnosticBuilder;
use crate::registry::{FunctionSig, Trinary};
use crate::types::{AccessoryKind, Type, is_numeric_string_literal};
use syntax::ast::*;

impl<'a> Checker<'a> {
    /// The expression-level ports, before `e` itself is checked.
    pub(crate) fn check_strict_rules_expression(&mut self, e: &Expr, locals: &Locals, label: &str) {
        if self.registry.check_dynamic_static_calls {
            let call = match &e.kind {
                ExprKind::FirstClassCallable(inner) => inner,
                _ => e,
            };
            if let ExprKind::MethodCall(receiver, method, _, _) = &call.kind
                && let Some((owner, name)) =
                    self.static_method_through_instance(receiver, method, locals)
            {
                self.report(
                    label.to_string(),
                    format!("Dynamic call to static method {owner}::{name}()."),
                    e.span,
                    0,
                    "staticMethod.dynamicCall",
                );
            }
        }
        if self.registry.check_arithmetic_operands {
            self.check_arithmetic_operands(e, locals, label);
        }
        if self.registry.check_comparable_types
            && let ExprKind::Binary(
                op @ (BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq | BinOp::Spaceship),
                l,
                r,
            ) = &e.kind
        {
            self.check_comparable_types(op, l, r, e.span, locals, label);
        }
        if self.registry.check_mixed_type_operands && self.current_trait.is_none() {
            match &e.kind {
                ExprKind::Binary(op, l, r)
                    if !matches!(
                        op,
                        BinOp::Identical
                            | BinOp::NotIdentical
                            | BinOp::Coalesce
                            | BinOp::And
                            | BinOp::Or
                            | BinOp::Pipe
                    ) =>
                {
                    self.check_mixed_type_operands(op, l, r, false, e.span, locals, label);
                }
                ExprKind::CompoundAssign(op, l, r) => {
                    self.check_mixed_type_operands(op, l, r, true, e.span, locals, label);
                }
                _ => {}
            }
        }
        if self.registry.check_implicit_array_creation
            && let ExprKind::Assign(target, _) = &e.kind
            && matches!(target.kind, ExprKind::Index(..))
        {
            let mut root = target.as_ref();
            while let ExprKind::Index(base, _) = &root.kind {
                root = base;
            }
            if let ExprKind::Variable(name) = &root.kind
                && name.as_ref() != "this"
                && !locals.contains_key(name)
                && !is_superglobal(name)
            {
                let certainty = if self.might_be_assigned_before(name, root.span) {
                    "might not exist"
                } else {
                    "does not exist"
                };
                self.report(
                    label.to_string(),
                    format!(
                        "Implicit array creation is not allowed - variable ${name} {certainty}."
                    ),
                    e.span,
                    0,
                    "variable.implicitArray",
                );
            }
        }
        if self.registry.check_illegal_constructor_calls {
            match &e.kind {
                ExprKind::MethodCall(_, method, _, _)
                    if method.eq_ignore_ascii_case("__construct") =>
                {
                    self.report(
                        label.to_string(),
                        "Call to __construct() on an existing object is not allowed.".to_string(),
                        e.span,
                        0,
                        "constructor.call",
                    );
                }
                ExprKind::StaticCall(target, method, _)
                    if method.eq_ignore_ascii_case("__construct")
                        && !self.is_parent_constructor_call(target) =>
                {
                    self.report(
                        label.to_string(),
                        "Static call to __construct() is only allowed on a parent class in the constructor.".to_string(),
                        e.span,
                        0,
                        "constructor.call",
                    );
                }
                _ => {}
            }
        }
    }

    fn check_arithmetic_operands(&mut self, e: &Expr, locals: &Locals, label: &str) {
        let (op, left, right) = match &e.kind {
            ExprKind::Binary(op, l, r) | ExprKind::CompoundAssign(op, l, r) => (op, l, r),
            ExprKind::Unary(op, operand) => {
                let (name, id, valid) = match op {
                    UnaryOp::Neg => ("unary -", "unaryMinus", Operand::Arithmetic),
                    UnaryOp::Plus => ("unary +", "unaryPlus", Operand::Arithmetic),
                    UnaryOp::PreInc => ("pre-increment", "preInc", Operand::Increment),
                    UnaryOp::PostInc => ("post-increment", "postInc", Operand::Increment),
                    UnaryOp::PreDec => ("pre-decrement", "preDec", Operand::Decrement),
                    UnaryOp::PostDec => ("post-decrement", "postDec", Operand::Decrement),
                    _ => return,
                };
                let ty = self.infer_expr(operand, locals);
                if !self.valid_operand(&ty, valid) {
                    self.report(
                        label.to_string(),
                        format!(
                            "Only numeric types are allowed in {name}, {} given.",
                            ty.describe_type_only()
                        ),
                        e.span,
                        0,
                        &format!("{id}.nonNumeric"),
                    );
                }
                return;
            }
            _ => return,
        };
        let (sigil, id) = match op {
            BinOp::Add => ("+", "plus"),
            BinOp::Sub => ("-", "minus"),
            BinOp::Mul => ("*", "mul"),
            BinOp::Div => ("/", "div"),
            BinOp::Mod => ("%", "mod"),
            BinOp::Pow => ("**", "pow"),
            _ => return,
        };
        let left_ty = self.infer_expr(left, locals);
        let right_ty = self.infer_expr(right, locals);
        if *op == BinOp::Add && has_array(&left_ty) && has_array(&right_ty) {
            return;
        }
        for (ty, side) in [(&left_ty, "left"), (&right_ty, "right")] {
            if !self.valid_operand(ty, Operand::Arithmetic) {
                self.report(
                    label.to_string(),
                    format!(
                        "Only numeric types are allowed in {sigil}, {} given on the {side} side.",
                        ty.describe_type_only()
                    ),
                    e.span,
                    0,
                    &format!("{id}.{side}NonNumeric"),
                );
            }
        }
    }

    /// `OperatorRuleHelper::isValidForArithmeticOperation`/
    /// `isValidForIncrement`/`isValidForDecrement`.
    fn valid_operand(&self, ty: &Type, operand: Operand) -> bool {
        if contains_mixed(ty) || is_number(ty) {
            return true;
        }
        if let Type::BenevolentUnion(members) = ty {
            return members.iter().any(is_number);
        }
        let members: Vec<&Type> = match ty {
            Type::Union(members) => members.iter().collect(),
            other => vec![other],
        };
        match operand {
            // Arrays and objects already fail PHP's own number conversion.
            Operand::Arithmetic => members.iter().any(|m| not_convertible_to_number(m)),
            Operand::Increment | Operand::Decrement => {
                (operand == Operand::Increment && members.iter().all(|m| is_string(m)))
                    || members.iter().all(|m| self.overloads_arithmetic(m))
            }
        }
    }

    fn overloads_arithmetic(&self, ty: &Type) -> bool {
        matches!(ty, Type::Object(class, _)
            if ["GMP", "BcMath\\Number"].iter().any(|c| self.registry.is_subclass_of(class, c)))
    }

    fn check_comparable_types(
        &mut self,
        op: &BinOp,
        l: &Expr,
        r: &Expr,
        span: syntax::token::Span,
        locals: &Locals,
        label: &str,
    ) {
        let left = self.infer_expr(l, locals);
        let right = self.infer_expr(r, locals);
        let describe = |ty: &Type| {
            if has_array(ty) {
                ty.to_string()
            } else {
                ty.describe_type_only()
            }
        };
        let sigil = match op {
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::LtEq => "<=",
            BinOp::GtEq => ">=",
            _ => "<=>",
        };
        let (l_text, r_text) = (describe(&left), describe(&right));
        if !self.is_comparable(&left) || !self.is_comparable(&right) {
            let mut builder = DiagnosticBuilder::new(
                label.to_string(),
                format!(
                    "Comparison {l_text} {sigil} {r_text} contains non-comparable type, only int|float|string|DateTimeInterface or comparable tuple is allowed."
                ),
                span,
                0,
                "comparison.nonComparable",
            );
            if self.registry.php_version.is_some_and(|v| v >= (8, 4)) {
                builder = builder.tip("Also BcMath\\Number is allowed.");
            }
            self.report_builder(builder);
        } else if !self.is_comparable_together(&left, &right) {
            self.report(
                label.to_string(),
                format!("Cannot compare different types in {l_text} {sigil} {r_text}."),
                span,
                0,
                "comparison.differentTypes",
            );
        }
    }

    fn contains_only(&self, ty: &Type, allowed: &[Type]) -> bool {
        let allowed = if allowed.len() == 1 {
            allowed[0].clone()
        } else {
            Type::Union(allowed.to_vec())
        };
        self.registry.is_super_type_of(&allowed, ty) == Trinary::Yes
    }

    fn is_comparable(&self, ty: &Type) -> bool {
        if self.contains_only(
            ty,
            &[
                Type::Int,
                Type::Float,
                Type::String,
                Type::Object("DateTimeInterface".into(), Vec::new()),
                Type::Object(("BcMath\\Number".to_string()).into(), Vec::new()),
            ],
        ) {
            return true;
        }
        let Some(tuples) = list_tuples(ty) else {
            return false;
        };
        tuples
            .iter()
            .flatten()
            .all(|value| self.is_comparable(value))
    }

    fn is_comparable_together(&self, left: &Type, right: &Type) -> bool {
        let number = Type::Object(("BcMath\\Number".to_string()).into(), Vec::new());
        let date = Type::Object("DateTimeInterface".into(), Vec::new());
        if self.contains_only(left, &[number.clone(), Type::Int])
            && self.contains_only(right, &[number, Type::Int])
        {
            return true;
        }
        for family in [&[Type::Int, Type::Float][..], &[Type::String], &[date]] {
            if self.contains_only(left, family) {
                return self.contains_only(right, family);
            }
        }
        let (Some(left_tuples), Some(right_tuples)) =
            (constant_arrays(left), constant_arrays(right))
        else {
            return false;
        };
        left_tuples.iter().all(|l| {
            right_tuples.iter().all(|r| {
                l.len() == r.len()
                    && l.iter()
                        .zip(r.iter())
                        .all(|(a, b)| self.is_comparable_together(a, b))
            })
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn check_mixed_type_operands(
        &mut self,
        op: &BinOp,
        l: &Expr,
        r: &Expr,
        compound: bool,
        span: syntax::token::Span,
        locals: &Locals,
        label: &str,
    ) {
        let left = self.infer_expr(l, locals);
        let right = self.infer_expr(r, locals);
        let converts_to_string_ok = if compound {
            *op == BinOp::Concat
        } else {
            matches!(op, BinOp::Concat | BinOp::Eq | BinOp::NotEq)
        };
        let prefix = if compound { "assignOp" } else { "binaryOp" };
        for (one, other) in [(&left, &right), (&right, &left)] {
            let kind = if all_members(one, is_string)
                && !matches!(other, Type::Mixed(_))
                && !is_string_or_nullable_string(other)
            {
                if converts_to_string_ok
                    && !matches!(one, Type::StringLiteral(s) if s.is_empty())
                    && self.converts_to_string(other)
                {
                    continue;
                }
                "string"
            } else if all_members(one, is_array)
                && !matches!(other, Type::Mixed(_))
                && !has_array(other)
                && !contains_mixed(other)
            {
                "array"
            } else {
                continue;
            };
            let (id, capitalized) = if kind == "string" {
                ("nonStringOperand", "non-string")
            } else {
                ("nonArrayOperand", "non-array")
            };
            self.report(
                label.to_string(),
                format!(
                    "{kind} ({one}) in combination with {capitalized} ({other}) is not allowed."
                ),
                span,
                0,
                &format!("{prefix}.{id}"),
            );
            return;
        }
    }

    /// Whether every value of `ty` has a string conversion.
    pub(crate) fn converts_to_string(&self, ty: &Type) -> bool {
        match ty {
            Type::Union(members) => members.iter().all(|m| self.converts_to_string(m)),
            Type::Intersection(members) => members.iter().any(|m| self.converts_to_string(m)),
            Type::Array(..) | Type::ArrayShape(_) | Type::ObjectShape(_) | Type::Callable(_) => {
                false
            }
            Type::Object(class, _) => self.registry.lookup_method(class, "__toString").is_some(),
            Type::EnumCase(..) => false,
            _ => true,
        }
    }

    pub(crate) fn check_strict_array_filter(
        &mut self,
        sig: &FunctionSig,
        args: &[Arg],
        locals: &Locals,
        span: syntax::token::Span,
        label: &str,
    ) {
        if args.iter().any(|a| a.spread) {
            return;
        }
        let Some(array) = arg_for_param(args, sig, 0) else {
            return;
        };
        let Some(callback) = arg_for_param(args, sig, 1) else {
            let mode_given = arg_for_param(args, sig, 2).is_some();
            if !mode_given {
                let array_ty = self.infer_expr(&array.value, locals);
                if !self.filters_unambiguously(&array_ty) {
                    self.report(
                        label.to_string(),
                        "Call to function array_filter() requires parameter #2 to be passed to avoid loose comparison semantics.".to_string(),
                        span,
                        0,
                        "arrayFilter.strict",
                    );
                }
                return;
            }
            self.report(
                label.to_string(),
                "Parameter #2 of array_filter() cannot be null to avoid loose comparison semantics (null given).".to_string(),
                span,
                0,
                "arrayFilter.strict",
            );
            return;
        };
        let callback_ty = self.infer_expr(&callback.value, locals);
        let level = match &callback_ty {
            Type::Null => 0,
            Type::Union(members) if members.iter().any(|m| matches!(m, Type::Null)) => 8,
            _ => return,
        };
        self.report(
            label.to_string(),
            format!(
                "Parameter #2 of array_filter() cannot be null to avoid loose comparison semantics ({} given).",
                callback_ty.describe_type_only()
            ),
            span,
            level,
            "arrayFilter.strict",
        );
    }

    /// Whether filtering `array`'s values by truthiness is unambiguous:
    /// they are all booleans, all arrays, or a union of members each
    /// always truthy or always falsy, with both kinds present.
    fn filters_unambiguously(&self, array: &Type) -> bool {
        let mut values = Vec::new();
        if !collect_array_values(array, &mut values) {
            return false;
        }
        let Some(item) = values
            .into_iter()
            .reduce(crate::checker::type_union::union_types)
        else {
            return false;
        };
        if let Type::Union(members) = &item {
            let mut truthy = false;
            let mut falsy = false;
            for member in members {
                match truthiness(member) {
                    Some(true) => truthy = true,
                    Some(false) => falsy = true,
                    None => {
                        truthy = false;
                        falsy = false;
                        break;
                    }
                }
            }
            if truthy && falsy {
                return true;
            }
            return members
                .iter()
                .all(|m| matches!(m, Type::Bool | Type::True | Type::False))
                || members.iter().all(is_array);
        }
        matches!(item, Type::Bool | Type::True | Type::False) || is_array(&item)
    }

    pub(crate) fn check_useless_cast(
        &mut self,
        cast: &Type,
        expr: &Type,
        span: syntax::token::Span,
        label: &str,
    ) {
        if matches!(expr, Type::Never) || self.registry.is_super_type_of(cast, expr) != Trinary::Yes
        {
            return;
        }
        self.report(
            label.to_string(),
            format!(
                "Casting to {} something that's already {}.",
                cast.describe_type_only(),
                expr.describe_type_only()
            ),
            span,
            0,
            "cast.useless",
        );
    }

    /// Replaces the current function's assignment sites with `body`'s,
    /// returning the previous ones to restore.
    pub(crate) fn enter_assigned_variables(&mut self, body: &[Stmt]) -> Vec<AssignedVariable> {
        let entered = if self.registry.check_implicit_array_creation {
            assigned_variables(body)
        } else {
            Vec::new()
        };
        std::mem::replace(&mut self.current_function_assigned_variables, entered)
    }

    /// Whether an assignment to `name` could have run before the one at
    /// `site`: it comes earlier, or shares an enclosing loop with it (as
    /// `site` itself does on a later iteration).
    fn might_be_assigned_before(&self, name: &str, site: syntax::token::Span) -> bool {
        let sites = &self.current_function_assigned_variables;
        let own_loops = sites
            .iter()
            .find(|a| a.span == site)
            .map(|a| a.loops.as_slice())
            .unwrap_or_default();
        sites.iter().any(|a| {
            a.name.as_ref() == name
                && ((a.span.start.line, a.span.start.col) < (site.start.line, site.start.col)
                    || a.loops.iter().any(|l| own_loops.contains(l)))
        })
    }

    /// `function.strict`, for one of the functions it covers.
    pub(crate) fn check_strict_function_call(
        &mut self,
        function: &str,
        sig: &FunctionSig,
        args: &[Arg],
        locals: &Locals,
        span: syntax::token::Span,
        label: &str,
    ) {
        if args.iter().any(|a| a.spread) {
            return;
        }
        let (position, must_be_true) = match function {
            "in_array" | "array_search" | "array_keys" => (2, true),
            "base64_decode" => (1, true),
            "iterator_to_array" => (1, false),
            _ => return,
        };
        if function == "array_keys" && arg_for_param(args, sig, 1).is_none() {
            return;
        }
        let message = match arg_for_param(args, sig, position) {
            None => format!(
                "Call to function {function}() requires parameter #{} to be set.",
                position + 1
            ),
            Some(arg) if must_be_true => {
                if self.infer_expr(&arg.value, locals) == Type::True {
                    return;
                }
                format!(
                    "Call to function {function}() requires parameter #{} to be true.",
                    position + 1
                )
            }
            Some(_) => return,
        };
        self.report(label.to_string(), message, span, 0, "function.strict");
    }

    /// The statement-level ports, before `stmt` itself is checked.
    pub(crate) fn check_strict_rules_statement(
        &mut self,
        stmt: &Stmt,
        locals: &Locals,
        label: &str,
    ) {
        if self.registry.check_loop_variable_overwrite {
            let mut overwritten: Vec<(syntax::token::Span, &str, String)> = Vec::new();
            match &stmt.kind {
                StmtKind::Foreach { key, value, .. } => {
                    if let Some(key) = key
                        && let ExprKind::Variable(name) = &key.kind
                        && locals.contains_key(name)
                    {
                        overwritten.push((
                            key.span,
                            "foreach.keyOverwrite",
                            format!("Foreach overwrites ${name} with its key variable."),
                        ));
                    }
                    for (name, span) in defined_targets(value, locals) {
                        let message =
                            format!("Foreach overwrites ${name} with its value variable.");
                        overwritten.push((span, "foreach.valueOverwrite", message));
                    }
                }
                StmtKind::For { init, .. } => {
                    for expr in init {
                        if let ExprKind::Assign(target, _) = &expr.kind {
                            for (name, span) in defined_targets(target, locals) {
                                let message = format!(
                                    "For loop initial assignment overwrites variable ${name}."
                                );
                                overwritten.push((span, "for.variableOverwrite", message));
                            }
                        }
                    }
                }
                _ => {}
            }
            for (span, id, message) in overwritten {
                self.report(label.to_string(), message, span, 0, id);
            }
        }
        if self.registry.check_switch_case_types
            && let StmtKind::Switch { subject, cases } = &stmt.kind
        {
            let subject_ty = self.infer_expr(subject, locals);
            for case in cases {
                let Some(test) = &case.test else {
                    continue;
                };
                let case_ty = self.infer_expr(test, locals);
                if self.registry.is_super_type_of(&subject_ty, &case_ty) != Trinary::No {
                    continue;
                }
                self.report(
                    label.to_string(),
                    format!(
                        "Switch condition type ({subject_ty}) does not match case condition {} ({}).",
                        syntax::printer::print_expr(test),
                        case_ty.describe_type_only()
                    ),
                    case.span,
                    0,
                    "switch.type",
                );
            }
        }
    }

    /// The declaring class and name of `method` when every object type
    /// `receiver` can be declares it static.
    fn static_method_through_instance(
        &self,
        receiver: &Expr,
        method: &str,
        locals: &Locals,
    ) -> Option<(String, String)> {
        let ty = self.infer_expr(receiver, locals);
        let members: Vec<&Type> = match &ty {
            Type::Union(members) => members
                .iter()
                .filter(|m| !matches!(m, Type::Null))
                .collect(),
            other => vec![other],
        };
        let mut found: Option<(String, String)> = None;
        for member in members {
            let Type::Object(class, _) = member else {
                return None;
            };
            let (owner, _) = self.registry.lookup_method_with_owner(class, method)?;
            let name = self
                .registry
                .lookup_method_canonical_name(class, method)
                .unwrap_or(method)
                .to_string();
            let owner_info = self.registry.classes.get(owner.as_ref())?;
            if !owner_info.static_methods.contains(name.as_str()) {
                return None;
            }
            // PHPStan's own test-case base classes expose their static
            // assertion helpers for `$this->` use.
            let prototype_in_phpstan_test_case = [
                "PHPStan\\Testing\\TypeInferenceTestCase",
                "PHPStan\\Testing\\PHPStanTestCase",
            ]
            .iter()
            .any(|base| {
                self.registry.is_subclass_of(&owner, base)
                    && self.registry.lookup_method(base, &name).is_some()
            });
            if prototype_in_phpstan_test_case {
                return None;
            }
            found.get_or_insert((owner.to_string(), name));
        }
        found
    }

    /// Whether `target::__construct()` runs a parent class's constructor
    /// from inside a constructor, or from a trait method some class
    /// imports as its constructor.
    fn is_parent_constructor_call(&self, target: &StaticTarget) -> bool {
        let Some(method) = self.current_method.as_deref() else {
            return false;
        };
        let StaticTarget::Name(name, _) = target else {
            return false;
        };
        if let Some(trait_name) = self.current_trait.as_deref() {
            let users: Vec<&str> = self
                .registry
                .classes
                .iter()
                .filter(|(_, info)| {
                    info.uses.iter().any(|u| u.eq_ignore_ascii_case(trait_name))
                        && (method.eq_ignore_ascii_case("__construct")
                            || info.trait_aliases.iter().any(|alias| {
                                alias.alias.eq_ignore_ascii_case("__construct")
                                    && alias.method.eq_ignore_ascii_case(method)
                                    && alias
                                        .trait_name
                                        .as_deref()
                                        .is_none_or(|t| t.eq_ignore_ascii_case(trait_name))
                            }))
                })
                .map(|(class, _)| class.as_ref())
                .collect();
            if name.eq_ignore_ascii_case("parent") {
                return !users.is_empty();
            }
            let Some(called) = self.resolve_static_class_name(name) else {
                return false;
            };
            return users.iter().any(|user| {
                self.parent_classes(user)
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(&called))
            });
        }
        if !method.eq_ignore_ascii_case("__construct") {
            return false;
        }
        let Some(class) = self.current_class.as_deref() else {
            return false;
        };
        let Some(called) = self.resolve_static_class_name(name) else {
            return false;
        };
        self.parent_classes(class)
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&called))
    }

    /// `class`'s parent classes, nearest first.
    fn parent_classes(&self, class: &str) -> Vec<String> {
        let mut parents: Vec<String> = Vec::new();
        let mut current = class.to_string();
        while let Some(parent) = self
            .registry
            .classes
            .get(current.as_str())
            .and_then(|info| info.extends.first())
        {
            let parent = parent.to_string();
            if parents.iter().any(|p| p.eq_ignore_ascii_case(&parent))
                || parent.eq_ignore_ascii_case(class)
            {
                break;
            }
            parents.push(parent.clone());
            current = parent;
        }
        parents
    }
}

/// The plain variables `target` binds (recursing into `[...]`/`list()`
/// destructuring) that `locals` already defines.
fn defined_targets(target: &Expr, locals: &Locals) -> Vec<(String, syntax::token::Span)> {
    match &target.kind {
        ExprKind::Variable(name) if locals.contains_key(name) => {
            vec![(name.to_string(), target.span)]
        }
        ExprKind::ArrayLiteral(items) => items
            .iter()
            .flat_map(|item| defined_targets(&item.value, locals))
            .collect(),
        _ => Vec::new(),
    }
}

/// The argument bound to `sig`'s parameter at `position`, positionally or
/// by name.
fn arg_for_param<'e>(args: &'e [Arg], sig: &FunctionSig, position: usize) -> Option<&'e Arg> {
    let positional = args.iter().take_while(|a| a.name.is_none()).count();
    if position < positional {
        return args.get(position);
    }
    let name = sig.params.get(position)?.name.as_ref();
    args.iter().find(|a| {
        a.name
            .as_deref()
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operand {
    Arithmetic,
    Increment,
    Decrement,
}

fn contains_mixed(ty: &Type) -> bool {
    match ty {
        Type::Mixed(_) | Type::Never | Type::TemplateParam(_) => true,
        Type::Union(members) => members.iter().any(contains_mixed),
        _ => false,
    }
}

/// Whether every value of `ty` is an `int`, a `float`, or a
/// `numeric-string`.
fn is_number(ty: &Type) -> bool {
    match ty {
        Type::Int
        | Type::IntLiteral(_)
        | Type::IntRange(..)
        | Type::Float
        | Type::FloatLiteral(_) => true,
        Type::StringLiteral(s) => is_numeric_string_literal(s),
        Type::Union(members) => members.iter().all(is_number),
        Type::Intersection(members) => members.iter().any(|m| {
            is_number(m)
                || matches!(
                    m,
                    Type::Accessory(
                        AccessoryKind::NumericString | AccessoryKind::DecimalIntegerString
                    )
                )
        }),
        _ => false,
    }
}

fn is_string(ty: &Type) -> bool {
    match ty {
        Type::String | Type::StringLiteral(_) | Type::ClassString(_) | Type::ClassKindString(_) => {
            true
        }
        Type::Intersection(members) => members.iter().any(is_string),
        _ => false,
    }
}

fn not_convertible_to_number(ty: &Type) -> bool {
    match ty {
        Type::Array(..)
        | Type::ArrayShape(_)
        | Type::Object(..)
        | Type::ObjectWithoutClass
        | Type::Resource { .. }
        | Type::ObjectShape(_)
        | Type::EnumCase(..)
        | Type::Callable(_)
        | Type::SelfRef(_)
        | Type::ParentRef
        | Type::StaticRef(_)
        | Type::This => true,
        Type::Intersection(members) => members.iter().any(not_convertible_to_number),
        _ => false,
    }
}

fn has_array(ty: &Type) -> bool {
    match ty {
        Type::Array(..) | Type::ArrayShape(_) => true,
        Type::Union(members) | Type::Intersection(members) => members.iter().any(has_array),
        _ => false,
    }
}

/// Each constant-array member's value types, when every member of `ty` is
/// a constant array.
fn constant_arrays(ty: &Type) -> Option<Vec<Vec<Type>>> {
    match ty {
        Type::ArrayShape(shape) if shape.is_sealed() => {
            Some(vec![shape.fields.iter().map(|f| f.ty.clone()).collect()])
        }
        Type::Union(members) => {
            let mut all = Vec::new();
            for member in members {
                all.extend(constant_arrays(member)?);
            }
            Some(all)
        }
        _ => None,
    }
}

/// `constant_arrays`, when every member is also a list: keys `0..n` in
/// order, none optional.
fn list_tuples(ty: &Type) -> Option<Vec<Vec<Type>>> {
    let is_list =
        |ty: &Type| -> bool {
            match ty {
                Type::ArrayShape(shape) => {
                    shape.is_sealed()
                        && shape.fields.iter().enumerate().all(|(i, f)| {
                            !f.optional && f.key == crate::types::ShapeKey::Int(i as i64)
                        })
                }
                _ => false,
            }
        };
    let all_lists = match ty {
        Type::Union(members) => members.iter().all(is_list),
        other => is_list(other),
    };
    if !all_lists {
        return None;
    }
    constant_arrays(ty)
}

fn all_members(ty: &Type, f: fn(&Type) -> bool) -> bool {
    match ty {
        Type::Union(members) => members.iter().all(f),
        other => f(other),
    }
}

fn is_array(ty: &Type) -> bool {
    match ty {
        Type::Array(..) | Type::ArrayShape(_) => true,
        Type::Intersection(members) => members.iter().any(is_array),
        _ => false,
    }
}

/// `string` or `?string` once literal values are generalized.
fn is_string_or_nullable_string(ty: &Type) -> bool {
    match ty {
        Type::Union(members) => {
            members
                .iter()
                .all(|m| is_string(m) || matches!(m, Type::Null))
                && members.iter().any(is_string)
        }
        other => is_string(other),
    }
}

fn truthiness(ty: &Type) -> Option<bool> {
    match ty {
        Type::EnumCase(..) | Type::ClassString(_) | Type::ClassKindString(_) => Some(true),
        other => crate::checker::expr::ranges::type_truthiness(other),
    }
}

/// The value types of `ty`'s array members; `false` when some member
/// isn't an array.
fn collect_array_values(ty: &Type, out: &mut Vec<Type>) -> bool {
    match ty {
        Type::Array(_, value) => {
            out.push((**value).clone());
            true
        }
        Type::ArrayShape(shape) => {
            out.extend(shape.types().cloned());
            true
        }
        Type::Union(members) => members.iter().all(|m| collect_array_values(m, out)),
        Type::Intersection(members) => members
            .iter()
            .find(|m| matches!(m, Type::Array(..) | Type::ArrayShape(_)))
            .is_some_and(|base| collect_array_values(base, out)),
        _ => false,
    }
}

/// One variable assignment site, with the spans of the loops around it.
#[derive(Clone)]
pub(crate) struct AssignedVariable {
    name: std::sync::Arc<str>,
    span: syntax::token::Span,
    loops: Vec<syntax::token::Span>,
}

/// Every site in `body` that binds a variable, outside nested functions,
/// closures and classes.
pub(crate) fn assigned_variables(body: &[Stmt]) -> Vec<AssignedVariable> {
    let mut out = Vec::new();
    let mut loops = Vec::new();
    for stmt in body {
        walk_stmt(stmt, &mut loops, &mut out);
    }
    out
}

fn walk_stmt(stmt: &Stmt, loops: &mut Vec<syntax::token::Span>, out: &mut Vec<AssignedVariable>) {
    match &stmt.kind {
        StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) | StmtKind::EnumDecl(_) => return,
        StmtKind::Foreach { key, value, .. } => {
            loops.push(stmt.span);
            for target in key.iter().chain(std::iter::once(value)) {
                bind_targets(target, loops, out);
            }
        }
        StmtKind::For { .. } | StmtKind::While { .. } | StmtKind::DoWhile { .. } => {
            loops.push(stmt.span);
        }
        StmtKind::Try { catches, .. } => {
            for catch in catches {
                if let (Some(var), Some(span)) = (&catch.var, catch.var_span) {
                    out.push(AssignedVariable {
                        name: var.clone(),
                        span,
                        loops: loops.clone(),
                    });
                }
            }
        }
        StmtKind::StaticVar(vars) => {
            for (name, _) in vars {
                out.push(AssignedVariable {
                    name: name.clone(),
                    span: stmt.span,
                    loops: loops.clone(),
                });
            }
        }
        _ => {}
    }
    for expr in syntax::visit::stmt_exprs(stmt) {
        walk_expr(expr, loops, out);
    }
    for child in syntax::visit::child_stmts(stmt) {
        walk_stmt(child, loops, out);
    }
    if matches!(
        stmt.kind,
        StmtKind::Foreach { .. }
            | StmtKind::For { .. }
            | StmtKind::While { .. }
            | StmtKind::DoWhile { .. }
    ) {
        loops.pop();
    }
}

fn walk_expr(e: &Expr, loops: &mut Vec<syntax::token::Span>, out: &mut Vec<AssignedVariable>) {
    match &e.kind {
        ExprKind::Closure { .. } | ExprKind::ArrowFn { .. } | ExprKind::NewAnonymousClass(..) => {
            return;
        }
        ExprKind::Assign(target, _)
        | ExprKind::AssignRef(target, _)
        | ExprKind::CompoundAssign(_, target, _) => {
            bind_targets(target, loops, out);
        }
        _ => {}
    }
    for child in syntax::visit::child_exprs(e) {
        walk_expr(child, loops, out);
    }
}

/// The variables an assignment target binds: a plain variable, the root
/// of an offset chain, or each element of a destructuring.
fn bind_targets(target: &Expr, loops: &[syntax::token::Span], out: &mut Vec<AssignedVariable>) {
    match &target.kind {
        ExprKind::Variable(name) => out.push(AssignedVariable {
            name: name.clone(),
            span: target.span,
            loops: loops.to_vec(),
        }),
        ExprKind::Index(base, _) => bind_targets(base, loops, out),
        ExprKind::ArrayLiteral(items) => {
            for item in items {
                bind_targets(&item.value, loops, out);
            }
        }
        _ => {}
    }
}
