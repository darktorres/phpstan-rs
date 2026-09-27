//! Array arguments whose values a function implicitly casts, mirroring
//! real PHPStan's `ParameterCastableToStringRule`,
//! `ParameterCastableToNumberRule`, `ImplodeParameterCastableToStringRule`,
//! and `SortParameterCastableToStringRule` (all level 5, `argument.type`),
//! plus `ArrayUnpackingRule`'s string-key gate for an unpacked array.

use crate::checker::{Checker, Locals};
use crate::registry::FunctionReflection;
use syntax::ast::{Arg, Expr, ExprKind};
use types::Type;

const SORT_REGULAR: i64 = 0;
const SORT_NUMERIC: i64 = 1;
const SORT_STRING: i64 = 2;
const SORT_LOCALE_STRING: i64 = 5;
const SORT_NATURAL: i64 = 6;
const SORT_FLAG_CASE: i64 = 8;

#[derive(Clone, Copy)]
enum Cast {
    String,
    ImplodeString,
    Number,
    Float,
    StringAndFloat,
}

impl Cast {
    fn expectation(self) -> &'static str {
        match self {
            Cast::String => "an array of values castable to string",
            Cast::ImplodeString => "array<string>",
            Cast::Number => "an array of values castable to number",
            Cast::Float => "an array of values castable to float",
            Cast::StringAndFloat => "an array of values castable to string and float",
        }
    }
}

impl<'a> Checker<'a> {
    /// `resolved` is `args` mapped onto `sig`'s parameters by position
    /// and name.
    pub(crate) fn check_castable_array_arguments(
        &mut self,
        function: &str,
        sig: &FunctionReflection,
        args: &[Arg],
        resolved: &[Option<&Expr>],
        locals: &Locals,
    ) {
        let function = function.trim_start_matches('\\').to_ascii_lowercase();
        match function.as_str() {
            "array_intersect" | "array_intersect_assoc" | "array_diff" | "array_diff_assoc" => {
                for (i, arg) in args.iter().enumerate() {
                    if arg.spread {
                        continue;
                    }
                    let name = self.castable_parameter_name(sig, args, i);
                    self.check_castable_array(&function, &name, &arg.value, Cast::String, locals);
                }
            }
            "array_combine" | "natcasesort" | "natsort" | "array_count_values"
            | "array_fill_keys" => {
                if let Some(value) = resolved.first().copied().flatten() {
                    let name = self.castable_parameter_name(sig, args, 0);
                    self.check_castable_array(&function, &name, value, Cast::String, locals);
                }
            }
            "array_sum" | "array_product" => {
                if let [arg] = args
                    && !arg.spread
                {
                    let name = self.castable_parameter_name(sig, args, 0);
                    self.check_castable_array(&function, &name, &arg.value, Cast::Number, locals);
                }
            }
            "implode" | "join" => {
                if args.iter().any(|a| a.spread) {
                    return;
                }
                let named = |n: &str| args.iter().any(|a| a.name.as_deref() == Some(n));
                let (value, index) = match args {
                    [only] => (&only.value, 0),
                    [_, second] if !named("separator") && !named("array") => (&second.value, 1),
                    [first, second] => {
                        let array_arg = if first.name.as_deref() == Some("array") {
                            first
                        } else {
                            second
                        };
                        (&array_arg.value, 1)
                    }
                    _ => return,
                };
                let name = if named("array") {
                    "$array".to_string()
                } else if named("separator") && args.len() == 1 {
                    "$separator".to_string()
                } else {
                    format!("#{} $array", index + 1)
                };
                self.check_castable_array(&function, &name, value, Cast::ImplodeString, locals);
            }
            "array_unique" | "sort" | "rsort" | "asort" | "arsort" => {
                let Some(value) = resolved.first().copied().flatten() else {
                    return;
                };
                let flags = match resolved.get(1).copied().flatten() {
                    Some(flag_expr) => match self.infer_expr(flag_expr, locals) {
                        Type::IntLiteral(n) => Some(vec![n]),
                        Type::Union(members) => members
                            .iter()
                            .map(|m| match m {
                                Type::IntLiteral(n) => Some(*n),
                                _ => None,
                            })
                            .collect(),
                        _ => None,
                    },
                    None if function == "array_unique" => Some(vec![SORT_STRING]),
                    None => Some(vec![SORT_REGULAR]),
                };
                if flags.as_deref() == Some(&[SORT_REGULAR]) {
                    return;
                }
                let (mut to_string, mut to_float) = (flags.is_none(), flags.is_none());
                for flag in flags.iter().flatten() {
                    if *flag == SORT_NUMERIC {
                        to_float = true;
                    } else if matches!(
                        flag & !SORT_FLAG_CASE,
                        SORT_STRING | SORT_LOCALE_STRING | SORT_NATURAL
                    ) {
                        to_string = true;
                    }
                }
                let cast = match (to_string, to_float) {
                    (true, false) => Cast::String,
                    (true, true) => Cast::StringAndFloat,
                    (false, true) => Cast::Float,
                    (false, false) => return,
                };
                let name = self.castable_parameter_name(sig, args, 0);
                self.check_castable_array(&function, &name, value, cast, locals);
            }
            _ => {}
        }
    }

    /// `#N $name` for a positional argument, `$name` for a named one,
    /// `#N` past the declared parameter list.
    fn castable_parameter_name(
        &self,
        sig: &FunctionReflection,
        args: &[Arg],
        index: usize,
    ) -> String {
        let Some(param) = sig.params().get(index) else {
            return format!("#{}", index + 1);
        };
        if args
            .iter()
            .any(|a| a.name.as_deref() == Some(param.name().as_ref()))
        {
            format!("${}", param.name())
        } else {
            format!("#{} ${}", index + 1, param.name())
        }
    }

    fn check_castable_array(
        &mut self,
        function: &str,
        param_name: &str,
        value: &Expr,
        cast: Cast,
        locals: &Locals,
    ) {
        let array_ty = self.infer_expr(value, locals);
        let Some(element_ty) = Self::array_element_type(&array_ty) else {
            return;
        };
        let castable = match cast {
            Cast::String | Cast::ImplodeString => {
                element_ty.to_string_type(self.registry).is_some()
            }
            Cast::Number => self.type_to_number(&element_ty),
            Cast::Float => self.type_to_float_strict(&element_ty),
            Cast::StringAndFloat => {
                self.type_to_float_strict(&element_ty)
                    && element_ty.to_string_type(self.registry).is_some()
            }
        };
        if castable {
            return;
        }
        self.report(
            format!(
                "Parameter {param_name} of function {function} expects {}, {array_ty} given.",
                cast.expectation()
            ),
            match &value.kind {
                ExprKind::Grouping(inner) => inner.span,
                _ => value.span,
            },
            5,
            "argument.type",
        );
    }

    /// `Type::toNumber()`: an array or a non-numeric object can't be
    /// used as a number.
    fn type_to_number(&self, ty: &Type) -> bool {
        match ty {
            Type::Array(..) | Type::ArrayShape(_) => false,
            Type::Intersection(members) if members.iter().any(|m| matches!(m, Type::Array(..))) => {
                false
            }
            Type::Object(..) | Type::ObjectWithoutClass | Type::Resource { .. } => {
                ty.to_float(self.registry).is_some()
            }
            Type::Union(members) | Type::Intersection(members) => {
                members.iter().any(|m| self.type_to_number(m))
            }
            _ => true,
        }
    }

    /// `type_to_float`, with an array also illegal (`ArrayType::
    /// toFloat()` is an error for sorting).
    fn type_to_float_strict(&self, ty: &Type) -> bool {
        self.type_to_number(ty) && ty.to_float(self.registry).is_some()
    }

    /// `ArrayUnpackingRule` (`arrayUnpacking.stringOffset`): below PHP
    /// 8.1, `[...$x]` can't unpack string keys. Level 3 when the keys
    /// are definitely strings, level 7 when only some may be.
    pub(crate) fn check_unpacked_string_keys(
        &mut self,
        value_ty: &Type,
        span: syntax::token::Span,
    ) {
        if self
            .registry
            .php_supports_everywhere(types::php_version::PhpFeature::ArrayUnpackingWithStringKeys)
            != Some(false)
        {
            return;
        }
        let Some(key_ty) = Self::array_key_type(value_ty) else {
            return;
        };
        let is_string = |t: &Type| matches!(t, Type::String | Type::StringLiteral(_));
        let (definitely, possibly) = match &key_ty {
            t if is_string(t) => (true, true),
            Type::Union(members) => (members.iter().all(is_string), members.iter().any(is_string)),
            _ => (false, false),
        };
        if !possibly {
            return;
        }
        self.report(
            format!(
                "Array unpacking cannot be used on an array with {}string keys: {value_ty}",
                if definitely { "" } else { "potential " }
            ),
            span,
            if definitely { 3 } else { 7 },
            "arrayUnpacking.stringOffset",
        );
    }
}
