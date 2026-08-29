use std::fmt;

// A deliberately small slice of PHPStan's type system: enough to represent
// what our parser already captures as raw type-hint strings, plus the
// handful of pseudo-types (self/parent/static) that need special handling
// once a class-context resolver exists. No generics, no conditional return
// types, no array shapes yet - those need the stub/reflection layer first.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Mixed,
    Never,
    Void,
    Null,
    Bool,
    True,
    False,
    Int,
    IntLiteral(i64),
    Float,
    String,
    StringLiteral(String),
    Array(Box<Type>),
    Object(String),
    Callable,
    SelfRef,
    ParentRef,
    StaticRef,
    Union(Vec<Type>),
    Intersection(Vec<Type>),
}

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
            Type::Array(inner) => match inner.as_ref() {
                Type::Mixed => write!(f, "array"),
                other => write!(f, "array<{other}>"),
            },
            Type::Object(name) => write!(f, "{name}"),
            Type::Callable => write!(f, "callable"),
            Type::SelfRef => write!(f, "self"),
            Type::ParentRef => write!(f, "parent"),
            Type::StaticRef => write!(f, "static"),
            Type::Union(members) => {
                let parts: Vec<String> = members.iter().map(Type::to_string).collect();
                write!(f, "{}", parts.join("|"))
            }
            Type::Intersection(members) => {
                let parts: Vec<String> = members.iter().map(Type::to_string).collect();
                write!(f, "{}", parts.join("&"))
            }
        }
    }
}

/// Parses one of our parser's raw type-hint strings (`"?string"`,
/// `"int|string"`, `"\App\Foo"`, `"Countable&Iterator"`) into a `Type`.
/// Does not handle mixed union+intersection (DNF types) or parentheses -
/// PHPStan's own stub/native syntax rarely needs them outside advanced
/// generic bounds.
pub fn parse_type_string(raw: &str) -> Type {
    let mut s = raw.trim();
    let nullable = s.starts_with('?');
    if nullable {
        s = &s[1..];
    }

    let ty = if s.contains('|') {
        Type::Union(s.split('|').map(parse_type_atom).collect())
    } else if s.contains('&') {
        Type::Intersection(s.split('&').map(parse_type_atom).collect())
    } else {
        parse_type_atom(s)
    };

    if nullable {
        Type::Union(vec![ty, Type::Null])
    } else {
        ty
    }
}

fn parse_type_atom(s: &str) -> Type {
    let bare = s.trim().trim_start_matches('\\');
    match bare.to_ascii_lowercase().as_str() {
        "int" | "integer" => Type::Int,
        "float" | "double" => Type::Float,
        "string" => Type::String,
        "bool" | "boolean" => Type::Bool,
        "true" => Type::True,
        "false" => Type::False,
        "null" => Type::Null,
        "void" => Type::Void,
        "mixed" => Type::Mixed,
        "never" | "noreturn" => Type::Never,
        "array" | "iterable" => Type::Array(Box::new(Type::Mixed)),
        "callable" | "closure" => Type::Callable,
        "self" => Type::SelfRef,
        "parent" => Type::ParentRef,
        "static" => Type::StaticRef,
        "object" => Type::Object("object".to_string()),
        _ => Type::Object(bare.to_string()),
    }
}

/// Is every value described by `sub` also described by `sup`? This is the
/// same relation PHPStan calls `isSuperTypeOf` (inverted) - the thing every
/// "does this assignment/return/argument type-check" question reduces to.
/// Class hierarchy isn't modeled yet, so `Object` subtyping is name-equality
/// only; self/parent/static are accepted against any `Object` as a
/// placeholder until a class-context resolver exists.
pub fn is_subtype(sub: &Type, sup: &Type) -> bool {
    match (sub, sup) {
        (_, Type::Mixed) => true,
        (Type::Never, _) => true,
        (a, b) if a == b => true,

        (Type::IntLiteral(_), Type::Int) => true,
        (Type::StringLiteral(_), Type::String) => true,
        (Type::True, Type::Bool) | (Type::False, Type::Bool) => true,

        (Type::Union(members), _) => members.iter().all(|m| is_subtype(m, sup)),
        (_, Type::Union(members)) => members.iter().any(|m| is_subtype(sub, m)),

        (Type::Intersection(members), _) => members.iter().any(|m| is_subtype(m, sup)),
        (_, Type::Intersection(members)) => members.iter().all(|m| is_subtype(sub, m)),

        (Type::Array(a), Type::Array(b)) => is_subtype(a, b),

        (Type::SelfRef | Type::ParentRef | Type::StaticRef, Type::Object(_)) => true,

        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nullable_parses_as_union_with_null() {
        assert_eq!(
            parse_type_string("?string"),
            Type::Union(vec![Type::String, Type::Null])
        );
    }

    #[test]
    fn union_parses_flat() {
        assert_eq!(
            parse_type_string("int|string"),
            Type::Union(vec![Type::Int, Type::String])
        );
    }

    #[test]
    fn intersection_parses_flat() {
        assert_eq!(
            parse_type_string("Countable&Iterator"),
            Type::Intersection(vec![
                Type::Object("Countable".to_string()),
                Type::Object("Iterator".to_string())
            ])
        );
    }

    #[test]
    fn leading_backslash_is_stripped() {
        assert_eq!(
            parse_type_string("\\App\\Foo"),
            Type::Object("App\\Foo".to_string())
        );
    }

    #[test]
    fn literal_is_subtype_of_its_general_type() {
        assert!(is_subtype(&Type::IntLiteral(5), &Type::Int));
        assert!(!is_subtype(&Type::Int, &Type::IntLiteral(5)));
    }

    #[test]
    fn everything_is_subtype_of_mixed() {
        assert!(is_subtype(&Type::String, &Type::Mixed));
        assert!(is_subtype(&Type::Object("Foo".to_string()), &Type::Mixed));
    }

    #[test]
    fn never_is_subtype_of_everything() {
        assert!(is_subtype(&Type::Never, &Type::String));
        assert!(is_subtype(&Type::Never, &Type::Mixed));
    }

    #[test]
    fn union_subtype_rules() {
        let small = Type::Union(vec![Type::Int, Type::String]);
        let big = Type::Union(vec![Type::Int, Type::String, Type::Bool]);
        assert!(is_subtype(&small, &big));
        assert!(!is_subtype(&big, &small));
        assert!(is_subtype(&Type::String, &small));
    }

    #[test]
    fn nullable_string_accepts_null_and_string() {
        let nullable_string = parse_type_string("?string");
        assert!(is_subtype(&Type::Null, &nullable_string));
        assert!(is_subtype(&Type::String, &nullable_string));
        assert!(!is_subtype(&Type::Int, &nullable_string));
    }

    #[test]
    fn array_subtyping_is_covariant_on_value_type() {
        let ints = Type::Array(Box::new(Type::Int));
        let mixed_arr = Type::Array(Box::new(Type::Mixed));
        assert!(is_subtype(&ints, &mixed_arr));
        assert!(!is_subtype(&mixed_arr, &ints));
    }

    #[test]
    fn display_matches_phpstan_style_syntax() {
        assert_eq!(parse_type_string("?string").to_string(), "string|null");
        assert_eq!(parse_type_string("int|string").to_string(), "int|string");
        assert_eq!(parse_type_string("array").to_string(), "array");
    }
}
