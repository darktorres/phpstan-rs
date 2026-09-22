//! `in_array()`/`array_search()`/`array_keys()` against a constant-array
//! haystack holding a finite value the needle can never match, mirroring
//! real PHPStan's `ImpossibleInArrayHaystackFiniteTypesRule`
//! (`function.impossibleHaystackValue`, level 4).

use crate::ast::Arg;
use crate::checker::expr::ranges::loose_compare_literals;
use crate::checker::{Checker, Locals};
use crate::token::Span;
use crate::types::Type;

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
        label: &str,
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
        let shapes: Vec<&Vec<(String, Type, bool)>> = match &haystack {
            Type::ArrayShape(fields) => vec![fields],
            Type::Union(members) => {
                let shapes: Vec<_> = members
                    .iter()
                    .filter_map(|m| match m {
                        Type::ArrayShape(fields) => Some(fields),
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
        for fields in shapes {
            for (key, value, _) in fields {
                if key == "..." {
                    continue;
                }
                if !can_never_match(self, value) {
                    any_possible_match = true;
                    continue;
                }
                if !is_finite(value) || dead.iter().any(|(t, _)| t == value) {
                    continue;
                }
                let another_value_of_needle =
                    self.registry.is_assignable(&needle, &generalize(value));
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
            self.report(
                label.to_string(),
                format!(
                    "Value {value} in the haystack passed to {function}() can never be {verb} the needle type {needle}."
                ),
                span,
                4,
                "function.impossibleHaystackValue",
            );
        }
    }
}
