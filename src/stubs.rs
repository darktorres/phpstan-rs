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
        ("strlen", sig(&[("string", Type::String, false)], Type::Int)),
        (
            "count",
            sig(&[("value", array_of(Type::Mixed), false)], Type::Int),
        ),
        (
            "is_string",
            sig(&[("value", Type::Mixed, false)], Type::Bool),
        ),
        ("is_int", sig(&[("value", Type::Mixed, false)], Type::Bool)),
        (
            "is_array",
            sig(&[("value", Type::Mixed, false)], Type::Bool),
        ),
        ("is_null", sig(&[("value", Type::Mixed, false)], Type::Bool)),
        (
            "in_array",
            sig(
                &[
                    ("needle", Type::Mixed, false),
                    ("haystack", array_of(Type::Mixed), false),
                ],
                Type::Bool,
            ),
        ),
        (
            "implode",
            sig(
                &[
                    ("separator", Type::String, false),
                    ("array", array_of(Type::Mixed), false),
                ],
                Type::String,
            ),
        ),
        (
            "explode",
            sig(
                &[
                    ("separator", Type::String, false),
                    ("string", Type::String, false),
                ],
                array_of(Type::String),
            ),
        ),
        (
            "trim",
            sig(&[("string", Type::String, false)], Type::String),
        ),
        (
            "strtolower",
            sig(&[("string", Type::String, false)], Type::String),
        ),
        (
            "strtoupper",
            sig(&[("string", Type::String, false)], Type::String),
        ),
        (
            "substr",
            sig(
                &[
                    ("string", Type::String, false),
                    ("offset", Type::Int, false),
                ],
                Type::String,
            ),
        ),
        (
            "array_keys",
            sig(
                &[("array", array_of(Type::Mixed), false)],
                array_of(Type::Int),
            ),
        ),
        (
            "array_values",
            sig(
                &[("array", array_of(Type::Mixed), false)],
                array_of(Type::Mixed),
            ),
        ),
        (
            "array_merge",
            sig(
                &[("arrays", array_of(Type::Mixed), true)],
                array_of(Type::Mixed),
            ),
        ),
        (
            "sprintf",
            sig(
                &[
                    ("format", Type::String, false),
                    ("values", Type::Mixed, true),
                ],
                Type::String,
            ),
        ),
        (
            "gettype",
            sig(&[("value", Type::Mixed, false)], Type::String),
        ),
    ]
}

fn array_of(value: Type) -> Type {
    Type::Array(Box::new(value))
}

fn sig(params: &[(&str, Type, bool)], return_type: Type) -> FunctionSig {
    FunctionSig {
        params: params
            .iter()
            .map(|(name, ty, variadic)| ParamSig {
                name: name.to_string(),
                ty: ty.clone(),
                variadic: *variadic,
                // None of these curated stubs model optional args; every
                // listed param is treated as required.
                has_default: false,
            })
            .collect(),
        return_type,
    }
}
