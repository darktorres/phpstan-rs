use std::fmt;

use syntax::ast::ClassKind;

use crate::parse::display_shape_key;
use crate::{
    AccessoryKind, CallSiteVariance, CallableSig, ConditionalSubject, PhpString, Repr, Shape, Type,
};

/// `keyword(params): return` - shared by a shaped `callable(...)`/
/// `Closure(...)` PHPDoc type and a closure literal's own
/// `Type::closure_literal` intersection.
fn write_callable_sig(f: &mut fmt::Formatter<'_>, keyword: &str, sig: &CallableSig) -> fmt::Result {
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
    write!(f, "{keyword}")?;
    if !sig.templates.is_empty() {
        let templates: Vec<String> = sig
            .templates
            .iter()
            .map(|t| {
                let mut s = t.name.clone();
                if !matches!((*t.bound).repr(), Repr::Mixed(..)) {
                    s.push_str(&format!(" of {}", t.bound));
                }
                if let Some(default) = &t.default {
                    s.push_str(&format!(" = {default}"));
                }
                s
            })
            .collect();
        write!(f, "<{}>", templates.join(", "))?;
    }
    write!(f, "({}): {}", parts.join(", "), sig.return_type)
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        stack::guard(|| {
            match self.repr() {
                Repr::Mixed(_, None) => write!(f, "mixed"),
                Repr::Mixed(_, Some(subtracted)) => {
                    if subtracted.union_members().len() > 1 {
                        write!(f, "mixed~({subtracted})")
                    } else {
                        write!(f, "mixed~{subtracted}")
                    }
                }
                Repr::Error => write!(f, "*ERROR*"),
                Repr::Never => write!(f, "never"),
                Repr::Void => write!(f, "void"),
                Repr::Null => write!(f, "null"),
                Repr::Bool => write!(f, "bool"),
                Repr::True => write!(f, "true"),
                Repr::False => write!(f, "false"),
                Repr::Int => write!(f, "int"),
                Repr::IntLiteral(n) => write!(f, "{n}"),
                Repr::Float => write!(f, "float"),
                Repr::String => write!(f, "string"),
                Repr::StringLiteral(s) => f.write_str(&quote_php_string(s)),
                Repr::Array(key, value) => {
                    let known_key = *key != Type::unknown_array_key();
                    match (known_key, value.repr()) {
                        (false, Repr::Mixed(..)) => write!(f, "array"),
                        (false, _) => write!(f, "array<{value}>"),
                        (true, _) => write!(f, "array<{key}, {value}>"),
                    }
                }
                Repr::ClassString(inner) => write!(f, "class-string<{inner}>"),
                Repr::ClassKindString(ClassKind::Interface) => write!(f, "interface-string"),
                Repr::ClassKindString(ClassKind::Trait) => write!(f, "trait-string"),
                Repr::ClassKindString(ClassKind::Enum) => write!(f, "enum-string"),
                Repr::ClassKindString(ClassKind::Class) => write!(f, "class-string"),
                Repr::ArrayShape(shape) => write_shape(f, "array", shape),
                Repr::ObjectShape(shape) => write_shape(f, "object", shape),
                // Real PHPStan's own `GenericTypeNode::__toString()` format.
                Repr::VarianceArg(CallSiteVariance::Bivariant, _) => write!(f, "*"),
                Repr::VarianceArg(CallSiteVariance::Covariant, inner) => {
                    write!(f, "covariant {inner}")
                }
                Repr::VarianceArg(CallSiteVariance::Contravariant, inner) => {
                    write!(f, "contravariant {inner}")
                }
                // Normally resolved away before display; shown in its source
                // form if it ever isn't (an unknown class).
                Repr::ClassConstant { class, pattern } => write!(f, "{class}::{pattern}"),
                Repr::ValueOf(inner) => write!(f, "value-of<{inner}>"),
                Repr::Extension(data) => data.describe(f),
                Repr::KeyOf(inner) => write!(f, "key-of<{inner}>"),
                Repr::OffsetAccess(base, key) => write!(f, "{base}[{key}]"),
                Repr::Object(name, args) => write_generic(f, name, args),
                Repr::ObjectWithoutClass => write!(f, "object"),
                Repr::Resource { closed: false } => write!(f, "resource"),
                Repr::Resource { closed: true } => write!(f, "closed-resource"),
                Repr::EnumCase(fqn, case) => write!(f, "{fqn}::{case}"),
                Repr::Callable(None) => write!(f, "callable"),
                Repr::Callable(Some(sig)) => write_callable_sig(f, "callable", sig),
                Repr::SelfRef(args) => write_generic(f, "self", args),
                Repr::ParentRef => write!(f, "parent"),
                Repr::StaticRef(args) => write_generic(f, "static", args),
                Repr::This => write!(f, "$this"),
                Repr::TemplateParam(tp) => write!(f, "{}", tp.name),
                // Parenthesized, as PHPStan describes a benevolent union, so
                // a message tells it apart from a plain one.
                Repr::BenevolentUnion(members) => {
                    let parts: Vec<String> = members.iter().map(Type::to_string).collect();
                    write!(f, "({})", parts.join("|"))
                }
                Repr::Union(members) => {
                    let parts: Vec<String> = members.iter().map(Type::to_string).collect();
                    write!(f, "{}", parts.join("|"))
                }
                Repr::Intersection(members) => {
                    if let Some(name) = describe_accessory_intersection(members) {
                        return write!(f, "{name}");
                    }
                    if let [object, callable] = members.as_slice()
                        && let Repr::Object(class, args) = object.repr()
                        && let Repr::Callable(Some(sig)) = callable.repr()
                        && class == "Closure"
                        && args.is_empty()
                    {
                        return write_callable_sig(f, "Closure", sig);
                    }
                    let parts: Vec<String> = members.iter().map(Type::to_string).collect();
                    write!(f, "{}", parts.join("&"))
                }
                Repr::IntRange(min, max) => write!(
                    f,
                    "int<{}, {}>",
                    min.map_or("min".to_string(), |n| n.to_string()),
                    max.map_or("max".to_string(), |n| n.to_string()),
                ),
                Repr::FloatLiteral(n) => f.write_str(&syntax::php_float::var_export_float(*n)),
                Repr::Accessory(kind) => write!(f, "{}", accessory_kind_name(*kind)),
                // The offset shows as its key's type (`0`, `'a'`), as
                // PHPStan's `HasOffsetValueType::describe()` does.
                Repr::HasOffsetValue(key, value) => {
                    write!(f, "hasOffsetValue({}, {value})", key.to_type())
                }
                Repr::HasOffset(key) => write!(f, "hasOffset({})", key.to_type()),
                Repr::HasMethod(name) => write!(f, "hasMethod({name})"),
                Repr::HasProperty(name) => write!(f, "hasProperty({name})"),
                // No real PHPStan equivalent to mirror (this variant is a
                // this-project-only synthesis, see its own doc comment) -
                // a plain, self-describing debug format.
                Repr::ReflectedMember(owner, name) => {
                    write!(f, "reflectedMember({owner}, '{name}')")
                }
                Repr::Conditional {
                    subject,
                    target,
                    if_true,
                    if_false,
                    negated,
                } => write!(
                    f,
                    "({}{} is {}{target} ? {if_true} : {if_false})",
                    match subject {
                        ConditionalSubject::Parameter(_) => "$",
                        ConditionalSubject::Template(_) => "",
                    },
                    subject.name(),
                    if *negated { "not " } else { "" },
                ),
            }
        })
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
        AccessoryKind::NonDecimalIntegerString => "non-decimal-int-string",
    }
}

/// Recognizes the canonical `[base, Accessory(kind), ...]` shapes
/// `parse_type_keyword_or_class` constructs and renders them
/// as PHPStan's own atomic pseudo-type name, rather than the generic
/// `&`-joined form `Repr::Intersection`'s own `Display` otherwise uses
/// (correct for a real user intersection like `Countable&Iterator`, but
/// wrong - `"string&non-empty-string"` - for an accessory-refined one).
/// Checks for the *presence* of each member independently, not an exact
/// positional slice pattern, so a semantically-identical intersection
/// built in a different member order still renders correctly. Returns
/// `None` for anything that doesn't match a known shape (most commonly
/// a real class intersection), leaving the generic `&`-joined fallback
/// in place.
fn describe_accessory_intersection(members: &[Type]) -> Option<String> {
    let has = |k: AccessoryKind| members.contains(&Type::accessory(k));
    // A shape that is only also non-empty.
    if members.len() == 2
        && has(AccessoryKind::NonEmptyArray)
        && let Some(shape) = members
            .iter()
            .find(|m| matches!(m.repr(), Repr::ArrayShape(_)))
    {
        let shape = shape.to_string();
        return Some(match shape.strip_prefix("array") {
            Some(fields) => format!("non-empty-array{fields}"),
            None => shape,
        });
    }
    if let Some((key, elem)) = members.iter().find_map(|m| match m.repr() {
        Repr::Array(k, e) => Some((k, e)),
        _ => None,
    }) {
        let is_list = has(AccessoryKind::ArrayList);
        // The key type must not be discarded in favor of always
        // formatting via the plain, key-less `<V>` form (or nothing, for
        // `Mixed`): `array_flip()`'s own key/value-swapping return type
        // can genuinely carry a known, non-default key (e.g.
        // `non-empty-array<string, int>`), which would otherwise render
        // as `non-empty-array<int>`. Mirrors `Repr::Array`'s
        // own `Display` arm just above: a known key always shows both
        // positions, even when the value happens to be `Mixed`. Never
        // shown for a `list`/`non-empty-list`, though, even though its
        // own key really is a "known" `Repr::Int` under the same
        // `!= unknown_array_key()` test - a list's keys are *always*
        // sequential ints by definition, so real PHPStan's own
        // `list<V>` convention never states it either.
        let known_key = !is_list && *key != Type::unknown_array_key();
        let elem_str = if known_key {
            format!("<{key}, {elem}>")
        } else if *elem == Type::mixed() {
            String::new()
        } else {
            format!("<{elem}>")
        };
        let name = match (is_list, has(AccessoryKind::NonEmptyArray)) {
            (true, true) => format!("non-empty-list{elem_str}"),
            (true, false) => format!("list{elem_str}"),
            (false, true) => format!("non-empty-array{elem_str}"),
            (false, false) => return None,
        };
        // Parts the name doesn't cover (`hasOffsetValue('a', 1)`) follow it.
        let rest: Vec<String> = members
            .iter()
            .filter(|m| {
                !matches!(
                    m.repr(),
                    Repr::Array(..)
                        | Repr::Accessory(AccessoryKind::ArrayList | AccessoryKind::NonEmptyArray)
                )
            })
            .map(Type::to_string)
            .collect();
        return Some(
            std::iter::once(name)
                .chain(rest)
                .collect::<Vec<_>>()
                .join("&"),
        );
    }
    if members.contains(&Type::string()) {
        // A generic ordered-parts join correctly composes any
        // combination of accessory kinds (e.g. `date('Y')`'s own real
        // `non-empty-numeric-string`) rather than only a fixed set of
        // singleton cases, falling back to the raw `&`-joined form
        // otherwise. No `VerbosityLevel` concept exists anywhere in this
        // project (real PHPStan hides `lowercase`/`uppercase` unless
        // asked for "precise" verbosity) - always showing every part is
        // strictly more informative, in-bounds per "stricter than
        // PHPStan is fine."
        let mut parts: Vec<&str> = Vec::new();
        if has(AccessoryKind::NonFalsyString) {
            parts.push("non-falsy");
        } else if has(AccessoryKind::NonEmptyString) {
            parts.push("non-empty");
        }
        // Each refinement shows; one another implies stands in for both
        // (`non-falsy` for `non-empty`, `decimal-int` for `numeric`).
        if has(AccessoryKind::LowercaseString) {
            parts.push("lowercase");
        }
        if has(AccessoryKind::UppercaseString) {
            parts.push("uppercase");
        }
        if has(AccessoryKind::LiteralString) {
            parts.push("literal");
        }
        if has(AccessoryKind::DecimalIntegerString) {
            parts.push("decimal-int");
        } else {
            if has(AccessoryKind::NonDecimalIntegerString) {
                parts.push("non-decimal-int");
            }
            if has(AccessoryKind::NumericString) {
                parts.push("numeric");
            }
        }
        return if parts.is_empty() {
            None
        } else {
            Some(format!("{}-string", parts.join("-")))
        };
    }
    None
}

/// `name`, with `<args>` when there are any.
fn write_generic(f: &mut fmt::Formatter<'_>, name: &str, args: &[Type]) -> fmt::Result {
    write!(f, "{name}")?;
    if !args.is_empty() {
        let parts: Vec<String> = args.iter().map(Type::to_string).collect();
        write!(f, "<{}>", parts.join(", "))?;
    }
    Ok(())
}

/// `keyword{key: type, key?: type, ...}`; an unsealed tail shows as the
/// bare `...` when its types are the unknown default, `...<K, V>`
/// otherwise.
fn write_shape(f: &mut fmt::Formatter<'_>, keyword: &str, shape: &Shape) -> fmt::Result {
    let mut parts: Vec<String> = shape
        .fields()
        .iter()
        .map(|field| {
            let q = if field.optional { "?" } else { "" };
            format!("{}{q}: {}", display_shape_key(&field.key), field.ty)
        })
        .collect();
    if let Some(tail) = &shape.unsealed {
        let (key, value) = &**tail;
        if *key == Type::unknown_array_key() && matches!(value.repr(), Repr::Mixed(..)) {
            parts.push("...".to_string());
        } else {
            parts.push(format!("...<{key}, {value}>"));
        }
    }
    write!(f, "{keyword}{{{}}}", parts.join(", "))
}

impl Type {
    /// `VerbosityLevel::typeOnly()`: literal values and array shapes
    /// collapse to their plain type.
    pub fn describe_type_only(&self) -> String {
        match self.repr() {
            Repr::IntLiteral(_) | Repr::IntRange(..) => "int".to_string(),
            Repr::FloatLiteral(_) => "float".to_string(),
            Repr::StringLiteral(_) => "string".to_string(),
            Repr::True | Repr::False => "bool".to_string(),
            Repr::Array(..) | Repr::ArrayShape(_) => "array".to_string(),
            Repr::Object(class, _) | Repr::EnumCase(class, _) => class.to_string(),
            Repr::Intersection(members) => members
                .iter()
                .find(|m| !m.is_accessory())
                .map_or_else(|| self.to_string(), Type::describe_type_only),
            Repr::Union(members) => {
                let mut parts: Vec<String> = Vec::new();
                for member in members {
                    let part = member.describe_type_only();
                    if !parts.contains(&part) {
                        parts.push(part);
                    }
                }
                parts.join("|")
            }
            _ => self.to_string(),
        }
    }
}

/// A string as PHP source would write it, unambiguously: single-quoted
/// with `\` and `'` escaped when it's printable UTF-8 (`'it\'s'`),
/// otherwise double-quoted with each control or non-UTF-8 byte as an
/// escape (`"\xff"`, `"a\n"`).
pub fn quote_php_string(s: &PhpString) -> String {
    if let Some(text) = s.as_str()
        && !text.chars().any(char::is_control)
    {
        return format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"));
    }
    let mut out = String::from("\"");
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // A valid multi-byte UTF-8 character stays as it is.
        let width = match bytes[i] {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 1,
        };
        if width > 1
            && let Some(ch) = bytes
                .get(i..i + width)
                .and_then(|b| std::str::from_utf8(b).ok())
        {
            out.push_str(ch);
            i += width;
            continue;
        }
        match bytes[i] {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'$' => out.push_str("\\$"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b if !(0x20..0x7f).contains(&b) => out.push_str(&format!("\\x{b:02x}")),
            b => out.push(b as char),
        }
        i += 1;
    }
    out.push('"');
    out
}
