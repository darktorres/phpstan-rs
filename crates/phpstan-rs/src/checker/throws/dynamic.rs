//! Per-callee dynamic throw-type extensions, tried before the plain
//! `FunctionReflection.throws` fallback at every one of `throws/mod.rs`'s
//! recording hooks.
//!
//! Simplified in one consistent direction relative to what a full
//! per-callee model could prove: rather than trying to *prove* a call
//! can't possibly throw (a provably-in-range divisor, a provably-valid
//! reflection target, ...), this contributes the exception type whenever
//! the call *shape* matches, unless a literal argument makes the "can't
//! throw" case trivial to prove. This is the safe direction for what a
//! throw point feeds into (`checker/throws/too_wide.rs`): under-
//! recognizing a real throw point would make a real `@throws` tag look
//! wrongly "unused"; over-recognizing one only makes this project more
//! conservative, never less.
//!
//! Excluded: `version_compare()` (would need PHP-version-awareness
//! plumbing this project doesn't have, and the value density is low).
//! `DateTime`/`DateTimeImmutable`'s own relative-date-string parsing is
//! *not* modeled here - `checker::date_time::check_date_time_constructor`
//! validates it as a direct diagnostic instead (mirroring `DateInterval`'s
//! own established pattern, `is_valid_date_interval_spec` below), not as
//! a throw-type extension.

use crate::checker::date_interval::is_valid_date_interval_spec;
use crate::checker::expr::contains_flag_name;
use crate::checker::xml_name::is_valid_xml_name;
use crate::checker::xml_wellformedness::is_well_formed_xml;
use crate::checker::{Checker, Locals};
use syntax::ast::unwrap_grouping;
use syntax::ast::{Arg, ExprKind, Literal, UnaryOp};
use types::Type;

fn throwable(class: &str) -> Type {
    Type::object(class.into(), Vec::new())
}

/// A literal integer argument's value, or `None` for anything else (a
/// variable, a computed expression, ...).
fn literal_int_value(a: &Arg) -> Option<i64> {
    if a.name.is_some() || a.spread {
        return None;
    }
    match &unwrap_grouping(&a.value).kind {
        ExprKind::Literal(Literal::Int(n)) => Some(*n),
        ExprKind::Unary(UnaryOp::Neg, inner) => match &unwrap_grouping(inner).kind {
            ExprKind::Literal(Literal::Int(n)) => Some(-*n),
            _ => None,
        },
        _ => None,
    }
}

impl<'a> Checker<'a> {
    /// An ordinary (not method/constructor) function call's dynamic
    /// throw type, tried before `sig.throws`. `bare_name` is already the
    /// last namespace segment, lowercased.
    pub(crate) fn dynamic_function_throw_type(
        &self,
        bare_name: &str,
        args: &[Arg],
        locals: &Locals,
    ) -> Option<Type> {
        match bare_name {
            // Contributes `DivisionByZeroError` unless the divisor is a
            // literal, provably-nonzero integer. Omits `ArithmeticError`
            // for the `PHP_INT_MIN / -1` overflow case, which needs both
            // arguments' provable ranges, not just the divisor's.
            "intdiv" => {
                let divisor = args.get(1)?;
                match literal_int_value(divisor) {
                    Some(0) => Some(throwable("DivisionByZeroError")),
                    // `Type::Void` is `record_call_throw_point`'s
                    // "recognized, but definitely contributes nothing -
                    // don't fall back to the sig" sentinel; returning
                    // bare `None` here would be indistinguishable from
                    // "not `intdiv` at all" and fall back to the generic
                    // stub-level `@throws` tag, undoing the proof that
                    // this call with a literal nonzero divisor is safe.
                    Some(_) => Some(Type::void()),
                    None => Some(throwable("DivisionByZeroError")),
                }
            }
            // No element-count tracking exists to prove the two arrays'
            // counts match, so `ValueError` is contributed unconditionally
            // whenever both arguments are present.
            "array_combine" if args.len() >= 2 => Some(throwable("ValueError")),
            // A custom second argument that's itself a `Throwable`
            // subtype is thrown as-is instead of the default
            // `AssertionError`.
            "assert" => {
                if let Some(custom) = args.get(1)
                    && custom.name.is_none()
                    && !custom.spread
                {
                    let custom_ty = self.infer_expr(&custom.value, locals);
                    if let Some((class, _)) = custom_ty.as_object()
                        && self.registry.is_subclass_of(class, "Throwable")
                    {
                        return Some(custom_ty);
                    }
                }
                Some(throwable("AssertionError"))
            }
            // `JSON_THROW_ON_ERROR` present in a bitwise-or flags argument.
            "json_encode" => Self::json_throw_on_error(args, 1),
            "json_decode" => Self::json_throw_on_error(args, 3),
            // `FILTER_THROW_ON_FAILURE` present in the third argument.
            "filter_var" => {
                let flags = args.get(2)?;
                (flags.name.is_none()
                    && !flags.spread
                    && contains_flag_name(&flags.value, "FILTER_THROW_ON_FAILURE"))
                .then(|| throwable("Filter\\FilterFailedException"))
            }
            _ => None,
        }
    }

    fn json_throw_on_error(args: &[Arg], flags_index: usize) -> Option<Type> {
        let flags = args.get(flags_index)?;
        (flags.name.is_none()
            && !flags.spread
            && contains_flag_name(&flags.value, "JSON_THROW_ON_ERROR"))
        .then(|| throwable("JsonException"))
    }

    /// `new X(...)`'s dynamic throw type. `DateInterval` reuses the
    /// already-shipped spec validator (`check_date_interval_constructor`'s
    /// "is this malformed" diagnostic and this throw-type contribution
    /// are two different consumers of the same fact); the `Reflection*`
    /// classes contribute `ReflectionException` unconditionally whenever
    /// constructed with at least one argument, without trying to prove
    /// the target class/method/property genuinely exists.
    pub(crate) fn dynamic_new_throw_type(&self, class: &str, args: &[Arg]) -> Option<Type> {
        match class {
            "DateInterval" => {
                let [arg] = args else { return None };
                if arg.name.is_some() || arg.spread {
                    return None;
                }
                match &unwrap_grouping(&arg.value).kind {
                    // Same `Type::Void` sentinel as `intdiv` above.
                    ExprKind::Literal(Literal::String(spec)) => {
                        if is_valid_date_interval_spec(&spec.to_str_lossy()) {
                            Some(Type::void())
                        } else {
                            Some(throwable("Exception"))
                        }
                    }
                    _ => Some(throwable("Exception")),
                }
            }
            "ReflectionClass" | "ReflectionObject" | "ReflectionFunction" | "ReflectionMethod"
            | "ReflectionProperty" => (!args.is_empty()).then(|| throwable("ReflectionException")),
            // Real PHP throws a plain `Exception` when the argument isn't
            // well-formed XML.
            "SimpleXMLElement" => {
                let arg = args.first()?;
                if arg.name.is_some() || arg.spread {
                    return Some(throwable("Exception"));
                }
                match &unwrap_grouping(&arg.value).kind {
                    ExprKind::Literal(Literal::String(xml)) => {
                        if xml
                            .as_str()
                            .is_some_and(|xml| is_well_formed_xml(xml).is_none())
                        {
                            Some(Type::void())
                        } else {
                            Some(throwable("Exception"))
                        }
                    }
                    _ => Some(throwable("Exception")),
                }
            }
            _ => None,
        }
    }

    /// `$obj->method(...)`'s dynamic throw type: `DateTime[Immutable]::
    /// sub()` and `DOMDocument::createElement()`.
    pub(crate) fn dynamic_method_throw_type(
        &self,
        class: &str,
        method: &str,
        args: &[Arg],
    ) -> Option<Type> {
        if method == "sub" && matches!(class, "DateTime" | "DateTimeImmutable") && !args.is_empty()
        {
            return Some(throwable("DateInvalidOperationException"));
        }
        if class == "DOMDocument" && method == "createElement" {
            let [arg] = args else {
                return Some(throwable("DOMException"));
            };
            if arg.name.is_some() || arg.spread {
                return Some(throwable("DOMException"));
            }
            return match &unwrap_grouping(&arg.value).kind {
                ExprKind::Literal(Literal::String(name)) => {
                    if name.as_str().is_some_and(is_valid_xml_name) {
                        Some(Type::void())
                    } else {
                        Some(throwable("DOMException"))
                    }
                }
                _ => Some(throwable("DOMException")),
            };
        }
        None
    }
}
