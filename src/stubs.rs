// Curated signatures for a handful of common PHP built-ins, standing in for
// what a real implementation would source from php-src's *.stub.php files
// (see the earlier discussion on generating stubs at build time). These are
// hand-written approximations - e.g. real `array_merge`/`sprintf` are
// variadic with much richer conditional return types - good enough to prove
// the merge-into-the-registry mechanism, not meant to be exhaustive.

use crate::registry::{FunctionSig, ParamSig};
use crate::types::Type;

pub fn builtin_stubs() -> Vec<(&'static str, FunctionSig)> {
    vec![
        (
            "strlen",
            sig(&[("string", Type::String, false, false)], Type::Int),
        ),
        (
            "count",
            sig(&[("value", array_of(Type::Mixed), false, false)], Type::Int),
        ),
        (
            "is_string",
            sig(&[("value", Type::Mixed, false, false)], Type::Bool),
        ),
        (
            "is_int",
            sig(&[("value", Type::Mixed, false, false)], Type::Bool),
        ),
        (
            "is_array",
            sig(&[("value", Type::Mixed, false, false)], Type::Bool),
        ),
        (
            "is_null",
            sig(&[("value", Type::Mixed, false, false)], Type::Bool),
        ),
        (
            "in_array",
            sig(
                &[
                    ("needle", Type::Mixed, false, false),
                    ("haystack", array_of(Type::Mixed), false, false),
                ],
                Type::Bool,
            ),
        ),
        (
            "implode",
            sig(
                &[
                    ("separator", Type::String, false, false),
                    ("array", array_of(Type::Mixed), false, false),
                ],
                Type::String,
            ),
        ),
        (
            "explode",
            sig(
                &[
                    ("separator", Type::String, false, false),
                    ("string", Type::String, false, false),
                ],
                array_of(Type::String),
            ),
        ),
        (
            "trim",
            sig(&[("string", Type::String, false, false)], Type::String),
        ),
        (
            "strtolower",
            sig(&[("string", Type::String, false, false)], Type::String),
        ),
        (
            "strtoupper",
            sig(&[("string", Type::String, false, false)], Type::String),
        ),
        (
            "substr",
            sig(
                &[
                    ("string", Type::String, false, false),
                    ("offset", Type::Int, false, false),
                ],
                Type::String,
            ),
        ),
        (
            "array_keys",
            sig(
                &[("array", array_of(Type::Mixed), false, false)],
                array_of(Type::Int),
            ),
        ),
        (
            "array_values",
            sig(
                &[("array", array_of(Type::Mixed), false, false)],
                array_of(Type::Mixed),
            ),
        ),
        (
            "array_merge",
            sig(
                &[("arrays", array_of(Type::Mixed), true, false)],
                array_of(Type::Mixed),
            ),
        ),
        (
            "sprintf",
            sig(
                &[
                    ("format", Type::String, false, false),
                    ("values", Type::Mixed, true, false),
                ],
                Type::String,
            ),
        ),
        (
            "gettype",
            sig(&[("value", Type::Mixed, false, false)], Type::String),
        ),
        // The motivating case for by-ref tracking: these all define their
        // last (or only) listed argument as a side effect of the call,
        // rather than reading a value the caller already had to provide -
        // `preg_match($pattern, $subject, $matches)` is idiomatic PHP, not
        // a bug, even though $matches is typically undefined beforehand.
        (
            "preg_match",
            sig(
                &[
                    ("pattern", Type::String, false, false),
                    ("subject", Type::String, false, false),
                    ("matches", array_of(Type::Mixed), false, true),
                ],
                Type::Int,
            ),
        ),
        (
            "preg_match_all",
            sig(
                &[
                    ("pattern", Type::String, false, false),
                    ("subject", Type::String, false, false),
                    ("matches", array_of(Type::Mixed), false, true),
                ],
                Type::Int,
            ),
        ),
        (
            "str_replace",
            sig(
                &[
                    ("search", Type::Mixed, false, false),
                    ("replace", Type::Mixed, false, false),
                    ("subject", Type::String, false, false),
                    ("count", Type::Int, false, true),
                ],
                Type::String,
            ),
        ),
        (
            "similar_text",
            sig(
                &[
                    ("string1", Type::String, false, false),
                    ("string2", Type::String, false, false),
                    ("percent", Type::Float, false, true),
                ],
                Type::Int,
            ),
        ),
        (
            "parse_str",
            sig(
                &[
                    ("string", Type::String, false, false),
                    ("result", array_of(Type::Mixed), false, true),
                ],
                Type::Void,
            ),
        ),
    ]
}

fn array_of(value: Type) -> Type {
    Type::Array(Box::new(value))
}

fn sig(params: &[(&str, Type, bool, bool)], return_type: Type) -> FunctionSig {
    FunctionSig {
        params: params
            .iter()
            .map(|(name, ty, variadic, by_ref)| ParamSig {
                name: name.to_string(),
                ty: ty.clone(),
                variadic: *variadic,
                // None of these curated stubs model optional args; every
                // listed param is treated as required.
                has_default: false,
                by_ref: *by_ref,
            })
            .collect(),
        return_type,
    }
}
