//! Phase 42 Sub-item D: per-callee dynamic throw-type extensions - a
//! curated, disclosed subset of real PHPStan's own `vendor/phpstan-src/
//! src/Type/Php/*ThrowTypeExtension.php` family (each verified by reading
//! the real extension source directly, not guessed), tried before the
//! plain `FunctionSig.throws` fallback at every one of `throws/mod.rs`'s
//! own recording hooks.
//!
//! Deliberately simplified relative to the real extensions in one
//! consistent direction: real PHPStan often tries hard to *prove* a call
//! can't possibly throw (a provably-in-range divisor, a provably-valid
//! reflection target, ...) before contributing nothing at all. This
//! project instead contributes the exception type whenever the call
//! *shape* matches, unless a literal argument makes the "can't throw"
//! case trivial to prove - the safe direction for what a throw point
//! feeds into (`TooWideFunctionThrowTypeRule`/`TooWideMethodThrowTypeRule`,
//! `checker/throws/too_wide.rs`): under-recognizing a real throw point
//! would make a real `@throws` tag look wrongly "unused"; over-
//! recognizing one only ever makes this project *more* conservative
//! about flagging that, never less.
//!
//! Excluded from this subset, each for its own disclosed reason:
//! `version_compare()` (PHP-version-conditional in real PHPStan - this
//! project has no PHP-version-awareness plumbing to gate it on, and the
//! value density is low); DateTime's own relative-date-string parsing
//! (`DateTimeConstructorThrowTypeExtension`/`DateTimeModifyMethodThrow
//! TypeExtension` - genuinely infeasible, real PHP invokes its actual
//! C/ICU date parser at analysis time, with no formally-specified
//! grammar to replicate the way DOM/SimpleXML's own extensions have).
//! DOM (`DomDocumentCreateElementDynamicThrowTypeExtension`) and
//! SimpleXML (`SimpleXMLElementConstructorThrowTypeExtension`) are both
//! covered below.

use crate::ast::{Arg, ExprKind, Literal, UnaryOp};
use crate::checker::date_interval::is_valid_date_interval_spec;
use crate::checker::expr::{contains_flag_name, unwrap_grouping};
use crate::checker::xml_name::is_valid_xml_name;
use crate::checker::xml_wellformedness::is_well_formed_xml;
use crate::checker::{Checker, Locals};
use crate::types::Type;

fn throwable(class: &str) -> Type {
    Type::Object(class.to_string(), Vec::new())
}

/// A literal, non-negative-or-negative integer argument's own value, or
/// `None` for anything else (a variable, a computed expression, ...) -
/// this file's own narrow "is this argument a provable literal" need,
/// distinct from `literal_string_value`/`literal_class_name_from_expr`
/// (neither handles an integer).
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
    /// Phase 42 Sub-item D: an ordinary (not method/constructor) function
    /// call's own dynamic throw type, tried before `sig.throws`.
    /// `bare_name` is already the last namespace segment, lowercased -
    /// matching every other by-name builtin dispatch in this checker.
    pub(crate) fn dynamic_function_throw_type(
        &self,
        bare_name: &str,
        args: &[Arg],
        locals: &Locals,
    ) -> Option<Type> {
        match bare_name {
            // `IntdivThrowTypeExtension`: contributes `DivisionByZeroError`
            // unless the divisor is a literal, provably-nonzero integer.
            // Real PHPStan also contributes `ArithmeticError` for the
            // `PHP_INT_MIN / -1` overflow case - skipped here (a real,
            // disclosed narrower scope: that combination needs both
            // arguments' own provable ranges, not just the divisor's).
            "intdiv" => {
                let divisor = args.get(1)?;
                match literal_int_value(divisor) {
                    Some(0) => Some(throwable("DivisionByZeroError")),
                    // A real, previously-live bug found and fixed this
                    // session: `None` here (this file's own established
                    // "not recognized/no contribution" sentinel) was
                    // indistinguishable, at every one of this function's
                    // own call sites (`record_call_throw_point`), from
                    // "this exact call shape isn't `intdiv` at all" - both
                    // fell through to `intdiv`'s own generic stub-level
                    // `@throws DivisionByZeroError`/`ArithmeticError` tag,
                    // completely undoing the whole point of proving *this*
                    // call, with a literal nonzero divisor, safe.
                    // `Type::Void` is `record_call_throw_point`'s own
                    // already-built "recognized, but definitely
                    // contributes nothing - don't fall back to the sig"
                    // sentinel (its own `if t != Type::Void { push(t); }
                    // return;` already handles this correctly; nothing
                    // there needed to change, only this file's own choice
                    // of which "nothing" to return).
                    Some(_) => Some(Type::Void),
                    None => Some(throwable("DivisionByZeroError")),
                }
            }
            // `ArrayCombineFunctionThrowTypeExtension`: real PHPStan only
            // contributes `ValueError` when it can't prove the two arrays'
            // own element counts match - this project has no element-
            // count tracking to attempt that proof at all, so (favoring
            // the "recognize more, not less" direction this file's own
            // doc comment describes) it's contributed unconditionally
            // whenever `array_combine` is called with both arguments.
            "array_combine" if args.len() >= 2 => Some(throwable("ValueError")),
            // `AssertThrowTypeExtension`: a custom second argument that's
            // itself a `Throwable` subtype is thrown as-is instead of the
            // default `AssertionError` - this project's own `infer_expr`
            // stands in for real PHPStan's `$scope->getType`.
            "assert" => {
                if let Some(custom) = args.get(1)
                    && custom.name.is_none()
                    && !custom.spread
                {
                    let custom_ty = self.infer_expr(&custom.value, locals);
                    if let Type::Object(class, _) = &custom_ty
                        && self.registry.is_subclass_of(class, "Throwable")
                    {
                        return Some(custom_ty);
                    }
                }
                Some(throwable("AssertionError"))
            }
            // `JsonThrowTypeExtension`: `JSON_THROW_ON_ERROR` present in a
            // bitwise-or flags argument.
            "json_encode" => Self::json_throw_on_error(args, 1),
            "json_decode" => Self::json_throw_on_error(args, 3),
            // `FilterVarThrowTypeExtension`: `FILTER_THROW_ON_FAILURE`
            // present in `filter_var`'s own third argument.
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

    /// Phase 42 Sub-item D: `new X(...)`'s own dynamic throw type -
    /// `DateIntervalConstructorThrowTypeExtension` (reuses the already-
    /// shipped spec validator - `check_date_interval_constructor`'s own
    /// separate "is this malformed" diagnostic and this throw-type
    /// contribution are two different consumers of the same underlying
    /// fact) and the four `Reflection*ConstructorThrowTypeExtension`s
    /// (each collapsed to "contribute `ReflectionException`
    /// unconditionally whenever constructed with at least one argument" -
    /// real PHPStan tries to prove the target class/method/property
    /// genuinely exists first; this project doesn't attempt that proof
    /// here, matching this file's own "recognize more, not less"
    /// direction).
    pub(crate) fn dynamic_new_throw_type(&self, class: &str, args: &[Arg]) -> Option<Type> {
        match class {
            "DateInterval" => {
                let [arg] = args else { return None };
                if arg.name.is_some() || arg.spread {
                    return None;
                }
                match &unwrap_grouping(&arg.value).kind {
                    // Same bug, same fix as `intdiv`'s own identical
                    // `Type::Void` sentinel just above: a *valid* literal
                    // spec previously returned bare `None` here, silently
                    // falling back to `DateInterval::__construct()`'s own
                    // generic stub-level `@throws Exception`/
                    // `DateMalformedIntervalStringException` tag - exactly
                    // undoing this arm's own proof that *this* literal
                    // spec can never throw.
                    ExprKind::Literal(Literal::String(spec)) => {
                        if is_valid_date_interval_spec(spec) {
                            Some(Type::Void)
                        } else {
                            Some(throwable("Exception"))
                        }
                    }
                    _ => Some(throwable("Exception")),
                }
            }
            "ReflectionClass" | "ReflectionObject" | "ReflectionFunction" | "ReflectionMethod"
            | "ReflectionProperty" => (!args.is_empty()).then(|| throwable("ReflectionException")),
            // Phase 78: `SimpleXMLElementConstructorThrowTypeExtension` -
            // real PHP throws a plain `Exception` when the argument isn't
            // well-formed XML.
            "SimpleXMLElement" => {
                let arg = args.first()?;
                if arg.name.is_some() || arg.spread {
                    return Some(throwable("Exception"));
                }
                match &unwrap_grouping(&arg.value).kind {
                    ExprKind::Literal(Literal::String(xml)) => {
                        if is_well_formed_xml(xml).is_none() {
                            Some(Type::Void)
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

    /// Phase 42 Sub-item D: `$obj->method(...)`'s own dynamic throw type -
    /// `DateTimeSubMethodThrowTypeExtension` (`DateTime[Immutable]::sub()`)
    /// and `DomDocumentCreateElementDynamicThrowTypeExtension`
    /// (`DOMDocument::createElement()`).
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
                    if is_valid_xml_name(name) {
                        Some(Type::Void)
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
