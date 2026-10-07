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
use syntax::ast::{Arg, ExprKind, Literal, NodeArenas, UnaryOp};
use types::Type;

fn throwable(class: &str) -> Type {
    Type::object(class.into(), Vec::new())
}

/// A literal integer argument's value, or `None` for anything else (a
/// variable, a computed expression, ...).
fn literal_int_value(arenas: &NodeArenas, a: &Arg) -> Option<i64> {
    if a.name.is_some() || a.spread {
        return None;
    }
    let value = arenas.peeled(a.value).and_then(|id| arenas.expr(id))?;
    match &value.kind {
        ExprKind::Literal(Literal::Int(n)) => Some(*n),
        ExprKind::Unary(UnaryOp::Neg, inner) => arenas
            .peeled(*inner)
            .and_then(|id| arenas.expr(id))
            .and_then(|inner| match &inner.kind {
                ExprKind::Literal(Literal::Int(n)) => Some(-*n),
                _ => None,
            }),
        _ => None,
    }
}

impl<'a> Checker<'a> {
    /// An ordinary (not method/constructor) function call's dynamic
    /// throw type, tried before `sig.throws`. `bare_name` is already the
    /// last namespace segment, lowercased.
    pub(crate) fn dynamic_function_throw_type(
        &self,
        arenas: &NodeArenas,
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
                match literal_int_value(arenas, divisor) {
                    Some(0) => Some(throwable("DivisionByZeroError")),
                    // `void` is `record_call_throw_point`'s
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
                    let custom_ty = arenas
                        .expr(custom.value)
                        .map(|value| self.infer_expr(value, locals))
                        .expect("assert argument belongs to its retained file owner");
                    if let Some((class, _)) = custom_ty.as_object()
                        && self.registry.is_subclass_of(class, "Throwable")
                    {
                        return Some(custom_ty);
                    }
                }
                Some(throwable("AssertionError"))
            }
            // `JSON_THROW_ON_ERROR` present in a bitwise-or flags argument.
            "json_encode" => Self::json_throw_on_error(arenas, args, 1),
            "json_decode" => Self::json_throw_on_error(arenas, args, 3),
            // `FILTER_THROW_ON_FAILURE` present in the third argument.
            "filter_var" => {
                let flags = args.get(2)?;
                (flags.name.is_none()
                    && !flags.spread
                    && contains_flag_name(arenas, flags.value, "FILTER_THROW_ON_FAILURE"))
                .then(|| throwable("Filter\\FilterFailedException"))
            }
            _ => None,
        }
    }

    fn json_throw_on_error(arenas: &NodeArenas, args: &[Arg], flags_index: usize) -> Option<Type> {
        let flags = args.get(flags_index)?;
        (flags.name.is_none()
            && !flags.spread
            && contains_flag_name(arenas, flags.value, "JSON_THROW_ON_ERROR"))
        .then(|| throwable("JsonException"))
    }

    /// `new X(...)`'s dynamic throw type. `DateInterval` reuses the
    /// already-shipped spec validator (`check_date_interval_constructor`'s
    /// "is this malformed" diagnostic and this throw-type contribution
    /// are two different consumers of the same fact); the `Reflection*`
    /// classes contribute `ReflectionException` unconditionally whenever
    /// constructed with at least one argument, without trying to prove
    /// the target class/method/property genuinely exists.
    pub(crate) fn dynamic_new_throw_type(
        &self,
        arenas: &NodeArenas,
        class: &str,
        args: &[Arg],
    ) -> Option<Type> {
        match class {
            "DateInterval" => {
                let [arg] = args else { return None };
                if arg.name.is_some() || arg.spread {
                    return None;
                }
                match arenas
                    .peeled(arg.value)
                    .and_then(|id| arenas.expr(id))
                    .map(|value| &value.kind)
                {
                    // Same `void` sentinel as `intdiv` above.
                    Some(ExprKind::Literal(Literal::String(spec))) => {
                        if arenas
                            .string_bytes(spec)
                            .map(|bytes| {
                                is_valid_date_interval_spec(&String::from_utf8_lossy(bytes))
                            })
                            .unwrap_or(false)
                        {
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
                match arenas
                    .peeled(arg.value)
                    .and_then(|id| arenas.expr(id))
                    .map(|value| &value.kind)
                {
                    Some(ExprKind::Literal(Literal::String(xml))) => {
                        if arenas
                            .string_bytes(xml)
                            .and_then(|bytes| std::str::from_utf8(bytes).ok())
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
            let Some(syntax_owner) = self.syntax_tree_owner() else {
                return Some(throwable("DOMException"));
            };
            let arenas = syntax_owner.arenas();
            let value = arenas.peeled(arg.value).and_then(|id| arenas.expr(id));
            return match value.map(|value| &value.kind) {
                Some(ExprKind::Literal(Literal::String(name))) => {
                    if arenas
                        .string_bytes(name)
                        .and_then(|bytes| std::str::from_utf8(bytes).ok())
                        .is_some_and(is_valid_xml_name)
                    {
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
