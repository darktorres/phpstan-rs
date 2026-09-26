//! Two `Rules/Comparison/` checks: `in_array()`/`array_search()`/
//! `array_keys()` against a constant-array haystack holding a finite
//! value the needle can never match
//! (`ImpossibleInArrayHaystackFiniteTypesRule`,
//! `function.impossibleHaystackValue`, level 4), and a void `match` used
//! as a value (`UsageOfVoidMatchExpressionRule`).
//!
//! Opt-in `check_loose_haystack_comparison`: an `in_array()`/
//! `array_search()`/`array_keys()` call comparing loosely where some
//! needle/haystack value type pair can be `==` without being `===`
//! (`function.looseComparison`).

use crate::checker::expr::ranges::loose_compare_literals;
use crate::checker::{Checker, Locals};
use crate::registry::FunctionSig;
use syntax::ast::Arg;
use syntax::token::Span;

use types::{AccessoryKind, Type, is_numeric_string_literal};

/// A value with a finite set of possible values - a literal, `bool`,
/// `null`, or an enum case.
fn is_finite(ty: &Type) -> bool {
    matches!(
        ty,
        Type::IntLiteral(_)
            | Type::StringLiteral(_)
            | Type::FloatLiteral(_)
            | Type::True
            | Type::False
            | Type::Bool
            | Type::Null
            | Type::EnumCase(..)
    )
}

/// `Type::generalize(lessSpecific)` for a finite value.
fn generalize(ty: &Type) -> Type {
    match ty {
        Type::IntLiteral(_) => Type::Int,
        Type::StringLiteral(_) => Type::String,
        Type::FloatLiteral(_) => Type::Float,
        Type::True | Type::False => Type::Bool,
        Type::EnumCase(class, _) => Type::Object(class.clone(), Vec::new()),
        other => other.clone(),
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn check_impossible_haystack_values(
        &mut self,
        function: &str,
        args: &[Arg],
        locals: &Locals,
        span: Span,
    ) {
        let function = function.trim_start_matches('\\').to_ascii_lowercase();
        let (needle_index, haystack_index) = match function.as_str() {
            "in_array" | "array_search" => (0, 1),
            "array_keys" => (1, 0),
            _ => return,
        };
        if self.loop_depth > 0
            || args.len() <= needle_index.max(haystack_index)
            || args.iter().any(|a| a.spread || a.name.is_some())
        {
            return;
        }
        let needle = self.infer_expr(&args[needle_index].value, locals);
        let haystack = self.infer_expr(&args[haystack_index].value, locals);
        let shapes: Vec<&types::Shape> = match &haystack {
            Type::ArrayShape(shape) => vec![shape],
            Type::Union(members) => {
                let shapes: Vec<_> = members
                    .iter()
                    .filter_map(|m| match m {
                        Type::ArrayShape(shape) => Some(shape),
                        _ => None,
                    })
                    .collect();
                if shapes.len() != members.len() {
                    return;
                }
                shapes
            }
            _ => return,
        };
        let strict = match args.get(2).map(|a| self.infer_expr(&a.value, locals)) {
            None | Some(Type::False) => Some(false),
            Some(Type::True) => Some(true),
            Some(_) => None,
        };
        let version = self.registry.php_version;
        let can_never_match = |this: &Self, value: &Type| -> bool {
            let never_identical = || this.registry.identical_result(&needle, value) == Some(false);
            let never_equal = || loose_compare_literals(&needle, value, version) == Some(false);
            match strict {
                Some(true) => never_identical(),
                Some(false) => never_equal(),
                None => never_identical() && never_equal(),
            }
        };
        let mut dead: Vec<(Type, bool)> = Vec::new();
        let mut any_possible_match = false;
        for shape in shapes {
            for field in &shape.fields {
                let value = &field.ty;
                if !can_never_match(self, value) {
                    any_possible_match = true;
                    continue;
                }
                if !is_finite(value) || dead.iter().any(|(t, _)| t == value) {
                    continue;
                }
                let another_value_of_needle =
                    self.registry.accepts(&generalize(value), &needle).yes();
                dead.push((value.clone(), another_value_of_needle));
            }
        }
        if !any_possible_match && function == "in_array" {
            return;
        }
        let verb = if strict == Some(false) {
            "equal to"
        } else {
            "identical to"
        };
        for (value, another_value_of_needle) in dead {
            if any_possible_match && another_value_of_needle {
                continue;
            }
            self.report(format!(
                    "Value {value} in the haystack passed to {function}() can never be {verb} the needle type {needle}."
                ),
                span,
                4,
                "function.impossibleHaystackValue",
            );
        }
    }

    /// `UsageOfVoidMatchExpressionRule` (`match.void`, level 2): a
    /// `match` used as a value while every arm that can produce one is
    /// `void`.
    pub(crate) fn check_void_match_usage(
        &mut self,
        arms: &[(Vec<syntax::ast::Expr>, syntax::ast::Expr)],
        default: Option<&syntax::ast::Expr>,
        locals: &Locals,
        span: Span,
    ) {
        let mut saw_void = false;
        for result in arms.iter().map(|(_, r)| r).chain(default) {
            match self.infer_expr(result, locals) {
                Type::Void => saw_void = true,
                Type::Never => {}
                _ => return,
            }
        }
        if saw_void {
            self.report(
                "Result of match expression (void) is used.".to_string(),
                span,
                2,
                "match.void",
            );
        }
    }
}

/// How a value takes part in a loose `==` comparison.
#[derive(Clone, PartialEq)]
enum LooseKind {
    Int,
    Float,
    /// A string literal PHP never reads as a number.
    NonNumericString,
    /// A numeric string literal, or a `numeric-string`.
    NumericString,
    /// A string that may or may not be numeric.
    String,
    Bool,
    Null,
    Array,
    Enum,
    Object(String),
}

fn loose_kinds(ty: &Type, out: &mut Vec<LooseKind>) -> bool {
    let kind = match ty {
        Type::Int | Type::IntLiteral(_) | Type::IntRange(..) => LooseKind::Int,
        Type::Float | Type::FloatLiteral(_) => LooseKind::Float,
        Type::StringLiteral(s) if is_numeric_string_literal(s) => LooseKind::NumericString,
        Type::StringLiteral(_) => LooseKind::NonNumericString,
        Type::String => LooseKind::String,
        Type::Bool | Type::True | Type::False => LooseKind::Bool,
        Type::Null => LooseKind::Null,
        Type::Array(..) | Type::ArrayShape(_) => LooseKind::Array,
        Type::EnumCase(..) => LooseKind::Enum,
        Type::Object(class, _) => LooseKind::Object(class.clone().to_string()),
        Type::Union(members) => return members.iter().all(|m| loose_kinds(m, out)),
        Type::Intersection(members) => {
            if members
                .iter()
                .any(|m| matches!(m, Type::Accessory(AccessoryKind::NumericString)))
            {
                LooseKind::NumericString
            } else if let Some(base) = members.iter().find(|m| !m.is_accessory()) {
                return loose_kinds(base, out);
            } else {
                return false;
            }
        }
        _ => return false,
    };
    if !out.contains(&kind) {
        out.push(kind);
    }
    true
}

impl<'a> Checker<'a> {
    /// Whether values of kinds `a` and `b` can be `==` without being
    /// `===`.
    fn loose_comparison_differs(&self, a: &LooseKind, b: &LooseKind) -> bool {
        use LooseKind::*;
        let before_php8 = self
            .registry
            .php_version
            .is_none_or(|v| v < types::PhpVersion::new(8, 0));
        let numeric = |k: &LooseKind| matches!(k, Int | Float);
        let possibly_numeric_string = |k: &LooseKind| matches!(k, NumericString | String);
        match (a, b) {
            (Enum, _) | (_, Enum) => false,
            (Bool, _) | (_, Bool) => true,
            (Null, Null) => false,
            (Null, Object(_)) | (Object(_), Null) => false,
            (Null, _) | (_, Null) => true,
            (Int, Int) | (Float, Float) => false,
            (Int, Float) | (Float, Int) => true,
            (x, y) if numeric(x) && possibly_numeric_string(y) => true,
            (x, y) if numeric(y) && possibly_numeric_string(x) => true,
            (x, NonNumericString) | (NonNumericString, x) if numeric(x) => before_php8,
            (NumericString, NumericString | String) | (String, NumericString) => true,
            (Array, Array) => true,
            (Object(_), Object(_)) => true,
            (Object(class), NonNumericString | NumericString | String)
            | (NonNumericString | NumericString | String, Object(class)) => {
                self.registry.lookup_method(class, "__toString").is_some()
            }
            _ => false,
        }
    }

    /// `function.looseComparison`, for `in_array()`/`array_search()`/
    /// `array_keys()`.
    pub(crate) fn check_loose_haystack_comparison(
        &mut self,
        function: &str,
        sig: &FunctionSig,
        args: &[Arg],
        locals: &Locals,
        span: Span,
    ) {
        if args.iter().any(|a| a.spread) {
            return;
        }
        let (needle_at, haystack_at) = match function {
            "in_array" | "array_search" => (0, 1),
            "array_keys" => (1, 0),
            _ => return,
        };
        let bound = |position: usize| -> Option<&Arg> {
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
        };
        let (Some(needle), Some(haystack)) = (bound(needle_at), bound(haystack_at)) else {
            return;
        };
        if let Some(strict) = bound(2)
            && self.infer_expr(&strict.value, locals) != Type::False
        {
            return;
        }
        let needle_ty = self.infer_expr(&needle.value, locals);
        let haystack_ty = self.infer_expr(&haystack.value, locals);
        let mut values: Vec<Type> = Vec::new();
        if !haystack_value_types(&haystack_ty, &mut values) {
            return;
        }
        let mut needle_kinds = Vec::new();
        if !loose_kinds(&needle_ty, &mut needle_kinds) {
            return;
        }
        for value in &values {
            let mut value_kinds = Vec::new();
            if !loose_kinds(value, &mut value_kinds) {
                continue;
            }
            let hazard = needle_kinds.iter().any(|n| {
                value_kinds
                    .iter()
                    .any(|v| self.loose_comparison_differs(n, v))
            });
            if hazard {
                self.report(format!(
                        "Loose comparison in {function}() between needle {needle_ty} and haystack value {value} can match values that are not identical; pass true as the strict argument."
                    ),
                    span,
                    0,
                    "function.looseComparison",
                );
                return;
            }
        }
    }
}

/// The value types an array type holds; `false` when `ty` isn't a known
/// array.
fn haystack_value_types(ty: &Type, out: &mut Vec<Type>) -> bool {
    match ty {
        Type::Array(_, value) => {
            out.push((**value).clone());
            true
        }
        Type::ArrayShape(shape) => {
            out.extend(shape.fields.iter().map(|f| f.ty.clone()));
            true
        }
        Type::Union(members) => members.iter().all(|m| haystack_value_types(m, out)),
        Type::Intersection(members) => members
            .iter()
            .find(|m| matches!(m, Type::Array(..) | Type::ArrayShape(_)))
            .is_some_and(|base| haystack_value_types(base, out)),
        _ => false,
    }
}
