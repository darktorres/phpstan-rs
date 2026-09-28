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

use crate::checker::{Checker, Locals};
use crate::registry::FunctionReflection;
use syntax::ast::Arg;
use syntax::token::Span;
use types::arithmetic::loose_compare_literals;

use types::{Type, is_numeric_string_literal};

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
        let shapes = haystack.constant_arrays();
        if shapes.is_empty() {
            return;
        }
        let strict = match args.get(2).map(|a| self.infer_expr(&a.value, locals)) {
            None => Some(false),
            Some(ty) => ty.bool_literal_value(),
        };
        let can_never_match = |this: &Self, value: &Type| -> bool {
            let never_identical = || this.registry.identical_result(&needle, value) == Some(false);
            let never_equal = || {
                this.registry
                    .php_version_consensus(|v| loose_compare_literals(&needle, value, v))
                    == Some(false)
            };
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
                if value.finite_types().is_empty() || dead.iter().any(|(t, _)| t == value) {
                    continue;
                }
                let another_value_of_needle = self.accepts(&value.generalize(), &needle).yes();
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
        arms: &[syntax::ast::MatchArm],
        locals: &Locals,
        span: Span,
    ) {
        let mut saw_void = false;
        for result in arms.iter().map(|arm| &arm.body) {
            let ty = self.infer_expr(result, locals);
            if ty.is_void() {
                saw_void = true;
            } else if !ty.is_never() {
                return;
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

/// The kinds of `ty`'s members into `out`; `false` when some member is
/// of no kind loose comparison is decided for.
fn loose_kinds(ty: &Type, out: &mut Vec<LooseKind>) -> bool {
    for member in ty.union_members() {
        let kind = if member.is_int().yes() {
            LooseKind::Int
        } else if member.is_float().yes() {
            LooseKind::Float
        } else if let [literal] = member.constant_strings()[..] {
            if is_numeric_string_literal(literal) {
                LooseKind::NumericString
            } else {
                LooseKind::NonNumericString
            }
        } else if member.is_numeric_string().yes() {
            LooseKind::NumericString
        } else if member.is_string().yes() {
            LooseKind::String
        } else if member.is_bool().yes() {
            LooseKind::Bool
        } else if member.is_null().yes() {
            LooseKind::Null
        } else if member.is_array().yes() {
            LooseKind::Array
        } else if member.is_enum_case() {
            LooseKind::Enum
        } else if member.is_object().yes()
            && let Some(class) = member.object_class_names().first()
        {
            LooseKind::Object(class.to_string())
        } else {
            return false;
        };
        if !out.contains(&kind) {
            out.push(kind);
        }
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
            .php_version_min()
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
        sig: &FunctionReflection,
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
            let name = sig.params().get(position)?.name().as_ref();
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
            && self.infer_expr(&strict.value, locals).bool_literal_value() != Some(false)
        {
            return;
        }
        let needle_ty = self.infer_expr(&needle.value, locals);
        let haystack_ty = self.infer_expr(&haystack.value, locals);
        let Some(values) = haystack_ty.array_value_type() else {
            return;
        };
        let mut needle_kinds = Vec::new();
        if !loose_kinds(&needle_ty, &mut needle_kinds) {
            return;
        }
        let mut value_kinds = Vec::new();
        if !loose_kinds(&values, &mut value_kinds) {
            return;
        }
        let hazard = needle_kinds.iter().any(|n| {
            value_kinds
                .iter()
                .any(|v| self.loose_comparison_differs(n, v))
        });
        if hazard {
            self.report(format!(
                    "Loose comparison in {function}() between needle {needle_ty} and haystack value {values} can match values that are not identical; pass true as the strict argument."
                ),
                span,
                0,
                "function.looseComparison",
            );
        }
    }
}
