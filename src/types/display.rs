use std::fmt;

use crate::types::parse::display_shape_key;
use crate::types::{AccessoryKind, Type};

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Mixed => write!(f, "mixed"),
            Type::Never => write!(f, "never"),
            Type::Void => write!(f, "void"),
            Type::Null => write!(f, "null"),
            Type::Bool => write!(f, "bool"),
            Type::True => write!(f, "true"),
            Type::False => write!(f, "false"),
            Type::Int => write!(f, "int"),
            Type::IntLiteral(n) => write!(f, "{n}"),
            Type::Float => write!(f, "float"),
            Type::String => write!(f, "string"),
            Type::StringLiteral(s) => write!(f, "'{s}'"),
            Type::Array(key, value) => {
                let known_key = **key != Type::unknown_array_key();
                match (known_key, value.as_ref()) {
                    (false, Type::Mixed) => write!(f, "array"),
                    (false, v) => write!(f, "array<{v}>"),
                    (true, v) => write!(f, "array<{key}, {v}>"),
                }
            }
            Type::ClassString(inner) => write!(f, "class-string<{inner}>"),
            Type::ArrayShape(fields) => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(name, ty, optional)| {
                        // The unsealed-shape sentinel (`try_parse_array_
                        // shape`'s own doc comment) displays as the bare
                        // `...` marker it was parsed from, not an
                        // ordinary `key: type` field.
                        if name == "..." {
                            return "...".to_string();
                        }
                        let q = if *optional { "?" } else { "" };
                        format!("{}{q}: {ty}", display_shape_key(name))
                    })
                    .collect();
                write!(f, "array{{{}}}", parts.join(", "))
            }
            Type::ObjectShape(fields) => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(name, ty, optional)| {
                        let q = if *optional { "?" } else { "" };
                        format!("{}{q}: {ty}", display_shape_key(name))
                    })
                    .collect();
                write!(f, "object{{{}}}", parts.join(", "))
            }
            // Phase 54: call-site variance sentinels print in real
            // PHPStan's own `GenericTypeNode::__toString()` format
            // (`covariant Foo`/`contravariant Foo`/`*`) rather than
            // leaking the internal `"$$covariant"<Foo>`-shaped sentinel
            // name into a real diagnostic message - unlike `"$$valueof"`
            // (Phase 80), which is always resolved away before display,
            // these persist in the type tree as a permanent annotation.
            Type::Object(name, args) if name == "$$bivariant" => write!(f, "*"),
            Type::Object(name, args) if name == "$$covariant" => {
                write!(f, "covariant {}", args[0])
            }
            Type::Object(name, args) if name == "$$contravariant" => {
                write!(f, "contravariant {}", args[0])
            }
            Type::Object(name, args) => {
                write!(f, "{name}")?;
                if !args.is_empty() {
                    let parts: Vec<String> = args.iter().map(Type::to_string).collect();
                    write!(f, "<{}>", parts.join(", "))?;
                }
                Ok(())
            }
            Type::EnumCase(fqn, case) => write!(f, "{fqn}::{case}"),
            Type::Callable(None) => write!(f, "callable"),
            Type::Callable(Some(sig)) => {
                let parts: Vec<String> = sig
                    .params
                    .iter()
                    .map(|p| {
                        let mut s = String::new();
                        if p.by_ref {
                            s.push('&');
                        }
                        s.push_str(&p.ty.to_string());
                        if p.variadic {
                            s.push_str("...");
                        }
                        if p.optional {
                            s.push('=');
                        }
                        s
                    })
                    .collect();
                write!(f, "callable({}): {}", parts.join(", "), sig.return_type)
            }
            Type::SelfRef => write!(f, "self"),
            Type::ParentRef => write!(f, "parent"),
            Type::StaticRef => write!(f, "static"),
            Type::TemplateParam(tp) => write!(f, "{}", tp.name),
            Type::Union(members) => {
                let parts: Vec<String> = members.iter().map(Type::to_string).collect();
                write!(f, "{}", parts.join("|"))
            }
            Type::Intersection(members) => {
                if let Some(name) = describe_accessory_intersection(members) {
                    return write!(f, "{name}");
                }
                let parts: Vec<String> = members.iter().map(Type::to_string).collect();
                write!(f, "{}", parts.join("&"))
            }
            Type::IntRange(min, max) => write!(
                f,
                "int<{}, {}>",
                min.map_or("min".to_string(), |n| n.to_string()),
                max.map_or("max".to_string(), |n| n.to_string()),
            ),
            Type::FloatLiteral(n) => write!(f, "{n}"),
            Type::Accessory(kind) => write!(f, "{}", accessory_kind_name(*kind)),
            Type::Conditional {
                subject,
                target,
                if_true,
                if_false,
                negated,
            } => write!(
                f,
                "(${subject} is {}{target} ? {if_true} : {if_false})",
                if *negated { "not " } else { "" },
            ),
        }
    }
}

fn accessory_kind_name(kind: AccessoryKind) -> &'static str {
    match kind {
        AccessoryKind::NonEmptyString => "non-empty-string",
        AccessoryKind::NonFalsyString => "non-falsy-string",
        AccessoryKind::LiteralString => "literal-string",
        AccessoryKind::NumericString => "numeric-string",
        AccessoryKind::ArrayList => "list",
        AccessoryKind::NonEmptyArray => "non-empty-array",
        AccessoryKind::LowercaseString => "lowercase-string",
        AccessoryKind::UppercaseString => "uppercase-string",
        AccessoryKind::DecimalIntegerString => "decimal-int-string",
    }
}

/// Recognizes the canonical `[base, Accessory(kind), ...]` shapes
/// `parse_type_keyword_or_class` constructs (Phase 14) and renders them
/// as PHPStan's own atomic pseudo-type name, rather than the generic
/// `&`-joined form `Type::Intersection`'s own `Display` otherwise uses
/// (correct for a real user intersection like `Countable&Iterator`, but
/// wrong - `"string&non-empty-string"` - for an accessory-refined one).
/// Checks for the *presence* of each member independently, not an exact
/// positional slice pattern, so a semantically-identical intersection
/// built in a different member order still renders correctly. Returns
/// `None` for anything that doesn't match a known shape (most commonly
/// a real class intersection), leaving the generic `&`-joined fallback
/// in place.
fn describe_accessory_intersection(members: &[Type]) -> Option<String> {
    let has = |k: AccessoryKind| members.contains(&Type::Accessory(k));
    if let Some((key, elem)) = members.iter().find_map(|m| match m {
        Type::Array(k, e) => Some((k.as_ref(), e.as_ref())),
        _ => None,
    }) {
        let is_list = has(AccessoryKind::ArrayList);
        // Phase 73 corpus find: this used to discard the key type
        // entirely, always formatting via the plain, key-less `<V>`
        // form (or nothing, for `Mixed`) - harmless while every
        // accessory-marked array this project produced also had an
        // unknown key, but `array_flip()`'s own new key/value-swapping
        // return type can genuinely carry a known, non-default key
        // (e.g. `non-empty-array<string, int>`) that this was silently
        // rendering as `non-empty-array<int>`. Mirrors `Type::Array`'s
        // own `Display` arm just above: a known key always shows both
        // positions, even when the value happens to be `Mixed`. Never
        // shown for a `list`/`non-empty-list`, though, even though its
        // own key really is a "known" `Type::Int` under the same
        // `!= unknown_array_key()` test - a list's keys are *always*
        // sequential ints by definition, so real PHPStan's own
        // `list<V>` convention never states it either.
        let known_key = !is_list && *key != Type::unknown_array_key();
        let elem_str = if known_key {
            format!("<{key}, {elem}>")
        } else if *elem == Type::Mixed {
            String::new()
        } else {
            format!("<{elem}>")
        };
        return match (is_list, has(AccessoryKind::NonEmptyArray)) {
            (true, true) => Some(format!("non-empty-list{elem_str}")),
            (true, false) => Some(format!("list{elem_str}")),
            (false, true) => Some(format!("non-empty-array{elem_str}")),
            (false, false) => None,
        };
    }
    if members.contains(&Type::String) {
        // Phase 75: a generic ordered-parts join, replacing the old fixed
        // 4-tuple match - behavior-preserving for every existing
        // singleton case (each still produces the exact same name it
        // always did), but also correctly composes a combination none of
        // those 4 tuples covered (e.g. `date('Y')`'s own real
        // `non-empty-numeric-string`, silently mis-rendered via the raw
        // `&`-joined fallback before this fix) and the two new case-
        // tracking kinds this phase adds. No `VerbosityLevel` concept
        // exists anywhere in this project (real PHPStan hides `lowercase`/
        // `uppercase` unless asked for "precise" verbosity) - always
        // showing every part is strictly more informative, in-bounds per
        // "stricter than PHPStan is fine."
        let mut parts: Vec<&str> = Vec::new();
        if has(AccessoryKind::NonFalsyString) {
            parts.push("non-falsy");
        } else if has(AccessoryKind::NonEmptyString) {
            parts.push("non-empty");
        }
        if has(AccessoryKind::LowercaseString) {
            parts.push("lowercase");
        } else if has(AccessoryKind::UppercaseString) {
            parts.push("uppercase");
        }
        if has(AccessoryKind::DecimalIntegerString) {
            parts.push("decimal-int");
        } else if has(AccessoryKind::LiteralString) {
            parts.push("literal");
        } else if has(AccessoryKind::NumericString) {
            parts.push("numeric");
        }
        return if parts.is_empty() {
            None
        } else {
            Some(format!("{}-string", parts.join("-")))
        };
    }
    None
}
