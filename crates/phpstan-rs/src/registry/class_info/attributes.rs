use syntax::ast::{Attribute, BinOp, ExprId, ExprKind, Literal, NodeArenas};

/// PHP's own `Attribute::TARGET_*`/`IS_REPEATABLE` class constant values.
fn attribute_target_constant_value(name: &str) -> Option<u32> {
    match name {
        "TARGET_CLASS" => Some(1),
        "TARGET_FUNCTION" => Some(2),
        "TARGET_METHOD" => Some(4),
        "TARGET_PROPERTY" => Some(8),
        "TARGET_CLASS_CONSTANT" => Some(16),
        "TARGET_PARAMETER" => Some(32),
        "TARGET_CONSTANT" => Some(64),
        "TARGET_ALL" => Some(127),
        "IS_REPEATABLE" => Some(128),
        _ => None,
    }
}

/// Evaluates a `#[\Attribute(...)]` meta-attribute's own `flags` argument -
/// a bitwise-OR chain of `Attribute::TARGET_*`/`IS_REPEATABLE` class-
/// constant references (the name before `::` is never itself checked -
/// the same lenient, no-namespace-resolution convention `is_override_
/// attribute_name`/`is_no_discard_attribute_name` already use for a
/// marker attribute's own bare name) or a plain int literal. Anything
/// else (a computed expression, a variable, a named constant this
/// project doesn't recognize) falls back to `Attribute::TARGET_ALL` -
/// "don't know, don't guess wrong," so an unusual-but-legal flags
/// expression can never manufacture a false `attribute.target` finding.
fn eval_attribute_flags_expr(arenas: &NodeArenas, id: ExprId) -> u32 {
    let Some(e) = arenas.expr(id) else {
        return ATTRIBUTE_TARGET_ALL;
    };
    syntax::stack::guard(|| match &e.kind {
        ExprKind::ClassConstFetch(_, member) => {
            attribute_target_constant_value(member).unwrap_or(ATTRIBUTE_TARGET_ALL)
        }
        ExprKind::Literal(Literal::Int(n)) => u32::try_from(*n).unwrap_or(ATTRIBUTE_TARGET_ALL),
        ExprKind::Binary(BinOp::BitOr, l, r) => {
            eval_attribute_flags_expr(arenas, *l) | eval_attribute_flags_expr(arenas, *r)
        }
        _ => ATTRIBUTE_TARGET_ALL,
    })
}

const ATTRIBUTE_TARGET_ALL: u32 = 127;

/// This class's own attribute-class flags, from a `#[\Attribute(...)]`
/// meta-attribute directly preceding its declaration - `None` when no
/// such meta-attribute is present at all (real PHP's own
/// `ReflectionClass::isAttribute()` would be `false` - this class was
/// simply never declared usable as an attribute). A bare `#[\Attribute]`
/// with no arguments at all defaults to `Attribute::TARGET_ALL`,
/// matching real PHP's own `Attribute::__construct(int $flags =
/// self::TARGET_ALL)` default parameter value.
pub(crate) fn compute_attribute_targets(arenas: &NodeArenas, attrs: &[Attribute]) -> Option<u32> {
    let meta = attrs.iter().find(|a| {
        a.name
            .trim_start_matches('\\')
            .rsplit('\\')
            .next()
            .unwrap_or(&a.name)
            .eq_ignore_ascii_case("Attribute")
    })?;
    let args = arenas.args(meta.args.clone());
    let flags_arg = args
        .iter()
        .find(|a| a.name.as_deref() == Some("flags"))
        .or_else(|| args.first());
    Some(match flags_arg {
        Some(arg) => eval_attribute_flags_expr(arenas, arg.value),
        None => ATTRIBUTE_TARGET_ALL,
    })
}
