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
use crate::registry::FunctionReflection;
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use types::Name;
use types::TrinaryLogic;
use types::{AccessoryKind, Type};

impl<'a> Checker<'a> {
    /// The expression-level ports, before `e` itself is checked.
    pub(crate) fn check_strict_rules_expression(&mut self, e: &Expr, locals: &Locals) {
        if self.settings.strict_rules.check_dynamic_static_calls {
            let call = match &e.kind {
                ExprKind::FirstClassCallable(inner) => inner,
                _ => e,
            };
            if let ExprKind::MethodCall(receiver, method, _, _) = &call.kind
                && let Some((owner, name)) =
                    self.static_method_through_instance(receiver, method, locals)
            {
                self.report(
                    format!("Dynamic call to static method {owner}::{name}()."),
                    e.span,
                    0,
                    "staticMethod.dynamicCall",
                );
            }
        }
        if self.settings.strict_rules.check_arithmetic_operands {
            self.check_arithmetic_operands(e, locals);
        }
        if self.settings.shipmonk.check_comparable_types
            && let ExprKind::Binary(
                op @ (BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq | BinOp::Spaceship),
                l,
                r,
            ) = &e.kind
        {
            self.check_comparable_types(op, l, r, e.span, locals);
        }
        if self.settings.voku.check_mixed_type_operands && self.current_trait.is_none() {
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
                    self.check_mixed_type_operands(op, l, r, false, e.span, locals);
                }
                ExprKind::CompoundAssign(op, l, r) => {
                    self.check_mixed_type_operands(op, l, r, true, e.span, locals);
                }
                _ => {}
            }
        }
        if self.settings.strict_rules.check_implicit_array_creation
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
                    format!(
                        "Implicit array creation is not allowed - variable ${name} {certainty}."
                    ),
                    e.span,
                    0,
                    "variable.implicitArray",
                );
            }
        }
        if self.settings.strict_rules.check_illegal_constructor_calls {
            match &e.kind {
                ExprKind::MethodCall(_, method, _, _)
                    if syntax::ast::is_constructor_name(method) =>
                {
                    self.report(
                        "Call to __construct() on an existing object is not allowed.".to_string(),
                        e.span,
                        0,
                        "constructor.call",
                    );
                }
                ExprKind::StaticCall(target, method, _)
                    if syntax::ast::is_constructor_name(method)
                        && !self.is_parent_constructor_call(target) =>
                {
                    self.report("Static call to __construct() is only allowed on a parent class in the constructor.".to_string(),
                        e.span,
                        0,
                        "constructor.call",
                    );
                }
                _ => {}
            }
        }
    }

    fn check_arithmetic_operands(&mut self, e: &Expr, locals: &Locals) {
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
                        format!(
                            "Only numeric types are allowed in {name}, {} given.",
                            ty.describe_type_only()
                        ),
                        e.span,
                        0,
                        format!("{id}.nonNumeric"),
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
        if *op == BinOp::Add && !left_ty.arrays().is_empty() && !right_ty.arrays().is_empty() {
            return;
        }
        for (ty, side) in [(&left_ty, "left"), (&right_ty, "right")] {
            if !self.valid_operand(ty, Operand::Arithmetic) {
                self.report(
                    format!(
                        "Only numeric types are allowed in {sigil}, {} given on the {side} side.",
                        ty.describe_type_only()
                    ),
                    e.span,
                    0,
                    format!("{id}.{side}NonNumeric"),
                );
            }
        }
    }

    /// `OperatorRuleHelper::isValidForArithmeticOperation`/
    /// `isValidForIncrement`/`isValidForDecrement`.
    fn valid_operand(&self, ty: &Type, operand: Operand) -> bool {
        if ty.has_unknown_part() {
            return true;
        }
        let number = self.relation().union_of(vec![
            Type::int(),
            Type::float(),
            Type::intersection_of(vec![
                Type::string(),
                Type::accessory(AccessoryKind::NumericString),
            ]),
        ]);
        let is_number = self.relation().is_super_type_of(&number, ty);
        if is_number.yes() || (ty.is_benevolent_union() && !is_number.no()) {
            return true;
        }
        match operand {
            // Arrays, objects, resources and callables already fail PHP's
            // own number conversion.
            Operand::Arithmetic => {
                let not_a_number = self.relation().union_of(vec![
                    Type::array(Type::mixed(), Type::mixed()),
                    Type::object_without_class(),
                    Type::resource(false),
                    Type::resource(true),
                    Type::callable(None),
                ]);
                ty.union_members()
                    .iter()
                    .any(|m| self.relation().is_super_type_of(&not_a_number, m).yes())
            }
            Operand::Increment | Operand::Decrement => {
                (operand == Operand::Increment && ty.is_string().yes())
                    || ty
                        .union_members()
                        .iter()
                        .all(|m| self.overloads_arithmetic(m))
            }
        }
    }

    fn overloads_arithmetic(&self, ty: &Type) -> bool {
        let overloading = self.relation().union_of(vec![
            Type::object("GMP".into(), Vec::new()),
            Type::object("BcMath\\Number".into(), Vec::new()),
        ]);
        self.relation().is_super_type_of(&overloading, ty).yes()
    }

    fn check_comparable_types(
        &mut self,
        op: &BinOp,
        l: &Expr,
        r: &Expr,
        span: syntax::span::Span,
        locals: &Locals,
    ) {
        let left = self.infer_expr(l, locals);
        let right = self.infer_expr(r, locals);
        let describe = |ty: &Type| {
            if !ty.arrays().is_empty() {
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
                format!(
                    "Comparison {l_text} {sigil} {r_text} contains non-comparable type, only int|float|string|DateTimeInterface or comparable tuple is allowed."
                ),
                span,
                0,
                "comparison.nonComparable",
            );
            if self
                .registry
                .php_version_min()
                .is_some_and(|v| v >= types::PhpVersion::new(8, 4))
            {
                builder = builder.tip("Also BcMath\\Number is allowed.");
            }
            self.report_builder(builder);
        } else if !self.is_comparable_together(&left, &right) {
            self.report(
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
            self.relation().union_of(allowed.to_vec())
        };
        self.registry.is_super_type_of(&allowed, ty) == TrinaryLogic::Yes
    }

    fn is_comparable(&self, ty: &Type) -> bool {
        syntax::stack::guard(|| {
            if self.contains_only(
                ty,
                &[
                    Type::int(),
                    Type::float(),
                    Type::string(),
                    Type::object("DateTimeInterface".into(), Vec::new()),
                    Type::object(("BcMath\\Number".to_string()).into(), Vec::new()),
                ],
            ) {
                return true;
            }
            let tuples = ty.constant_arrays();
            !tuples.is_empty()
                && tuples.iter().all(|t| is_tuple(t))
                && tuples
                    .iter()
                    .flat_map(|t| t.types())
                    .all(|value| self.is_comparable(value))
        })
    }

    fn is_comparable_together(&self, left: &Type, right: &Type) -> bool {
        syntax::stack::guard(|| {
            let number = Type::object(("BcMath\\Number".to_string()).into(), Vec::new());
            let date = Type::object("DateTimeInterface".into(), Vec::new());
            if self.contains_only(left, &[number.clone(), Type::int()])
                && self.contains_only(right, &[number, Type::int()])
            {
                return true;
            }
            for family in [
                &[Type::int(), Type::float()][..],
                &[Type::string()],
                &[date],
            ] {
                if self.contains_only(left, family) {
                    return self.contains_only(right, family);
                }
            }
            let (left_tuples, right_tuples) = (left.constant_arrays(), right.constant_arrays());
            if left_tuples.is_empty() || right_tuples.is_empty() {
                return false;
            }
            left_tuples.iter().all(|l| {
                right_tuples.iter().all(|r| {
                    l.fields().len() == r.fields().len()
                        && l.types()
                            .zip(r.types())
                            .all(|(a, b)| self.is_comparable_together(a, b))
                })
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
        span: syntax::span::Span,
        locals: &Locals,
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
            let string_or_null = self.relation().union_of(vec![Type::string(), Type::null()]);
            let is_string_or_nullable_string = self
                .relation()
                .is_super_type_of(&string_or_null, other)
                .yes()
                && !other.is_null().yes();
            let kind =
                if one.is_string().yes() && !other.is_mixed() && !is_string_or_nullable_string {
                    if converts_to_string_ok
                        && !matches!(one.constant_strings_text().as_slice(), [s] if s.is_empty())
                        && self.converts_to_string(other)
                    {
                        continue;
                    }
                    "string"
                } else if one.is_array().yes()
                    && !other.is_mixed()
                    && other.arrays().is_empty()
                    && !other.has_unknown_part()
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
                format!(
                    "{kind} ({one}) in combination with {capitalized} ({other}) is not allowed."
                ),
                span,
                0,
                format!("{prefix}.{id}"),
            );
            return;
        }
    }

    /// Whether every value of `ty` has a string conversion.
    /// An object of no known class or a callable may have no string
    /// form, so neither surely converts.
    pub(crate) fn converts_to_string(&self, ty: &Type) -> bool {
        ty.union_members().iter().all(|m| {
            m.to_string_type(self.registry).is_some()
                && !(m.is_object().yes() && m.first_object_class().is_none())
                && !self
                    .relation()
                    .is_super_type_of(&Type::callable(None), m)
                    .yes()
        })
    }

    pub(crate) fn check_strict_array_filter(
        &mut self,
        sig: &FunctionReflection,
        args: &[Arg],
        locals: &Locals,
        span: syntax::span::Span,
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
                    self.report("Call to function array_filter() requires parameter #2 to be passed to avoid loose comparison semantics.".to_string(),
                        span,
                        0,
                        "arrayFilter.strict",
                    );
                }
                return;
            }
            self.report("Parameter #2 of array_filter() cannot be null to avoid loose comparison semantics (null given).".to_string(),
                span,
                0,
                "arrayFilter.strict",
            );
            return;
        };
        let callback_ty = self.infer_expr(&callback.value, locals);
        let is_null = callback_ty.is_null();
        let level = if is_null.yes() {
            0
        } else if is_null.maybe() {
            8
        } else {
            return;
        };
        self.report(format!(
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
        let Some(item) = array.array_value_type() else {
            return false;
        };
        let members = item.union_members();
        let truthiness: Vec<Option<bool>> = members
            .iter()
            .map(|m| m.to_boolean().bool_literal_value())
            .collect();
        let mixes_truthy_and_falsy = members.len() > 1
            && truthiness.iter().all(Option::is_some)
            && truthiness.contains(&Some(true))
            && truthiness.contains(&Some(false));
        mixes_truthy_and_falsy || item.is_bool().yes() || item.is_array().yes()
    }

    pub(crate) fn check_useless_cast(
        &mut self,
        cast: &Type,
        expr: &Type,
        span: syntax::span::Span,
    ) {
        if expr.is_never() || self.registry.is_super_type_of(cast, expr) != TrinaryLogic::Yes {
            return;
        }
        self.report(
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
        let entered = if self.settings.strict_rules.check_implicit_array_creation {
            assigned_variables(body)
        } else {
            Vec::new()
        };
        std::mem::replace(&mut self.current_function_assigned_variables, entered)
    }

    /// Whether an assignment to `name` could have run before the one at
    /// `site`: it comes earlier, or shares an enclosing loop with it (as
    /// `site` itself does on a later iteration).
    fn might_be_assigned_before(&self, name: &str, site: syntax::span::Span) -> bool {
        let sites = &self.current_function_assigned_variables;
        let own_loops = sites
            .iter()
            .find(|a| a.span == site)
            .map(|a| a.loops.as_slice())
            .unwrap_or_default();
        sites.iter().any(|a| {
            a.name.as_ref() == name
                && (a.span.start < site.start || a.loops.iter().any(|l| own_loops.contains(l)))
        })
    }

    /// `function.strict`, for one of the functions it covers.
    pub(crate) fn check_strict_function_call(
        &mut self,
        function: &str,
        sig: &FunctionReflection,
        args: &[Arg],
        locals: &Locals,
        span: syntax::span::Span,
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
                if self.infer_expr(&arg.value, locals).is_true().yes() {
                    return;
                }
                format!(
                    "Call to function {function}() requires parameter #{} to be true.",
                    position + 1
                )
            }
            Some(_) => return,
        };
        self.report(message, span, 0, "function.strict");
    }

    /// The statement-level ports, before `stmt` itself is checked.
    pub(crate) fn check_strict_rules_statement(&mut self, stmt: &Stmt, locals: &Locals) {
        if self.settings.strict_rules.check_loop_variable_overwrite {
            let mut overwritten: Vec<(syntax::span::Span, &str, String)> = Vec::new();
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
                self.report(message, span, 0, id);
            }
        }
        if self.settings.strict_rules.check_switch_case_types
            && let StmtKind::Switch { subject, cases } = &stmt.kind
        {
            let subject_ty = self.infer_expr(subject, locals);
            for case in cases {
                let Some(test) = &case.test else {
                    continue;
                };
                let case_ty = self.infer_expr(test, locals);
                if self.registry.is_super_type_of(&subject_ty, &case_ty) != TrinaryLogic::No {
                    continue;
                }
                self.report(format!(
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
        let mut found: Option<(String, String)> = None;
        for member in ty.union_members().iter().filter(|m| !m.is_null().yes()) {
            let (class, _) = member.as_object()?;
            let (owner, _) = self.lookup_method_with_owner(class, method)?;
            let name = self
                .registry
                .lookup_method_canonical_name(class, method)
                .unwrap_or(method)
                .to_string();
            let owner_info = self.registry.classes.get(owner.as_ref())?;
            if !owner_info.static_methods().contains(name.as_str()) {
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
        let StaticTarget::Name(name) = target else {
            return false;
        };
        if let Some(trait_name) = self.current_trait.as_deref() {
            let users: Vec<&str> = self
                .registry
                .classes
                .iter()
                .filter(|(_, info)| {
                    info.uses()
                        .iter()
                        .any(|u| u.eq_ignore_ascii_case(trait_name))
                        && (syntax::ast::is_constructor_name(method)
                            || info.trait_aliases().iter().any(|alias| {
                                syntax::ast::is_constructor_name(&alias.alias)
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
        if !syntax::ast::is_constructor_name(method) {
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
            .and_then(|info| info.extends().first())
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
fn defined_targets(target: &Expr, locals: &Locals) -> Vec<(String, syntax::span::Span)> {
    syntax::stack::guard(|| match &target.kind {
        ExprKind::Variable(name) if locals.contains_key(name) => {
            vec![(name.to_string(), target.span)]
        }
        ExprKind::Destructure(items, _) => items
            .iter()
            .filter_map(|item| item.target.as_ref())
            .flat_map(|target| defined_targets(target, locals))
            .collect(),
        _ => Vec::new(),
    })
}

/// The argument bound to `sig`'s parameter at `position`, positionally or
/// by name.
fn arg_for_param<'e>(
    args: &'e [Arg],
    sig: &FunctionReflection,
    position: usize,
) -> Option<&'e Arg> {
    let positional = args.iter().take_while(|a| a.name.is_none()).count();
    if position < positional {
        return args.get(position);
    }
    let name = sig.params().get(position)?.name().as_ref();
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

/// A list with every key required.
fn is_tuple(shape: &types::Shape) -> bool {
    shape.is_list() && shape.fields().iter().all(|f| !f.optional)
}

/// One variable assignment site, with the spans of the loops around it.
#[derive(Clone)]
pub(crate) struct AssignedVariable {
    name: Name,
    span: syntax::span::Span,
    loops: Vec<syntax::span::Span>,
}

/// Every site in `body` that binds a variable, outside nested functions,
/// closures and classes.
pub(crate) fn assigned_variables(body: &[Stmt]) -> Vec<AssignedVariable> {
    let mut assigned = Assigned {
        loops: Vec::new(),
        out: Vec::new(),
    };
    syntax::visit::walk_stmts(&mut assigned, body);
    assigned.out
}

struct Assigned {
    loops: Vec<syntax::span::Span>,
    out: Vec<AssignedVariable>,
}

impl<'ast> syntax::visit::Visitor<'ast> for Assigned {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        let loops = &mut self.loops;
        let out = &mut self.out;
        match &stmt.kind {
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => return,
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
                    if let Some(var) = &catch.var {
                        out.push(AssignedVariable {
                            name: var.name.clone(),
                            span: var.span,
                            loops: loops.clone(),
                        });
                    }
                }
            }
            StmtKind::StaticVar(vars) => {
                for (name, _) in vars {
                    out.push(AssignedVariable {
                        name: name.name.clone(),
                        span: name.span,
                        loops: loops.clone(),
                    });
                }
            }
            _ => {}
        }
        syntax::visit::walk_stmt(self, stmt);
        if matches!(
            stmt.kind,
            StmtKind::Foreach { .. }
                | StmtKind::For { .. }
                | StmtKind::While { .. }
                | StmtKind::DoWhile { .. }
        ) {
            self.loops.pop();
        }
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        match &e.kind {
            ExprKind::Closure { .. }
            | ExprKind::ArrowFn { .. }
            | ExprKind::NewAnonymousClass(..) => return,
            ExprKind::Assign(target, _)
            | ExprKind::AssignRef(target, _)
            | ExprKind::CompoundAssign(_, target, _) => {
                bind_targets(target, &self.loops, &mut self.out);
            }
            _ => {}
        }
        syntax::visit::walk_expr(self, e);
    }
}

/// The variables an assignment target binds: a plain variable, the root
/// of an offset chain, or each element of a destructuring.
fn bind_targets(target: &Expr, loops: &[syntax::span::Span], out: &mut Vec<AssignedVariable>) {
    syntax::stack::guard(|| match &target.kind {
        ExprKind::Variable(name) => out.push(AssignedVariable {
            name: name.clone(),
            span: target.span,
            loops: loops.to_vec(),
        }),
        ExprKind::Index(base, _) => bind_targets(base, loops, out),
        ExprKind::Destructure(items, _) => {
            for target in items.iter().filter_map(|item| item.target.as_ref()) {
                bind_targets(target, loops, out);
            }
        }
        _ => {}
    })
}
