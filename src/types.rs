use std::collections::HashMap;
use std::fmt;

// A deliberately small slice of PHPStan's type system: enough to represent
// what our parser already captures as raw type-hint strings, plus the
// handful of pseudo-types (self/parent/static) that need special handling
// once a class-context resolver exists. Still no conditional return types
// or @template generic substitution - those need real per-call-site type
// parameter binding, not just parsing.
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
    // `array{name: string, age?: int}` - PHPDoc's structural array type.
    // (field name, field type, optional)
    ArrayShape(Vec<(String, Type, bool)>),
    Object(String),
    Callable,
    SelfRef,
    ParentRef,
    StaticRef,
    // An unbound `@template T` name. Only produced for a function/method's
    // own declared template parameters (registry::mark_template_params) -
    // resolving one against an actual argument at a call site is
    // checker::bind_template_param / substitute_template's job. Class-level
    // generics (`Collection<T>` with the binding tracked across statements)
    // aren't modeled - that needs Object to carry type arguments, a bigger
    // change than this slice attempts.
    TemplateParam(String),
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
            Type::ArrayShape(fields) => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(name, ty, optional)| {
                        let q = if *optional { "?" } else { "" };
                        format!("{name}{q}: {ty}")
                    })
                    .collect();
                write!(f, "array{{{}}}", parts.join(", "))
            }
            Type::Object(name) => write!(f, "{name}"),
            Type::Callable => write!(f, "callable"),
            Type::SelfRef => write!(f, "self"),
            Type::ParentRef => write!(f, "parent"),
            Type::StaticRef => write!(f, "static"),
            Type::TemplateParam(name) => write!(f, "{name}"),
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
/// `"int|string"`, `"\App\Foo"`, `"Countable&Iterator"`, or a PHPDoc type
/// like `"array<int, User>"` / `"array{name: string, age?: int}"`) into a
/// `Type`. Does not handle mixed union+intersection (DNF types) or
/// parentheses - PHPStan's own stub/native syntax rarely needs them outside
/// advanced generic bounds.
pub fn parse_type_string(raw: &str) -> Type {
    let mut s = raw.trim();
    let nullable = s.starts_with('?');
    if nullable {
        s = &s[1..];
    }

    // Splitting must respect <>/{} nesting - `Collection<int|string>` has
    // no top-level union, but a naive `s.contains('|')` would still split
    // on the one hiding inside the angle brackets.
    let pipe_parts = split_top_level(s, '|');
    let amp_parts = split_top_level(s, '&');
    let ty = if pipe_parts.len() > 1 {
        Type::Union(pipe_parts.into_iter().map(parse_type_atom).collect())
    } else if amp_parts.len() > 1 {
        Type::Intersection(amp_parts.into_iter().map(parse_type_atom).collect())
    } else {
        parse_type_atom(s)
    };

    if nullable {
        Type::Union(vec![ty, Type::Null])
    } else {
        ty
    }
}

/// Splits `s` on `sep`, but only where `sep` appears outside any
/// `<>`/`{}`/`()`/`[]` nesting. Always returns at least one part, mirroring
/// `str::split` - callers check `.len() > 1` to know whether a real
/// top-level separator was found at all.
fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '{' | '(' | '[' => depth += 1,
            '>' | '}' | ')' | ']' => depth -= 1,
            c if c == sep && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// `array{name: string, age?: int}` - PHPDoc's structural array shape.
/// Field values can themselves be full type strings (unions, generics,
/// nested shapes), so each is re-run through `parse_type_string`.
fn try_parse_array_shape(s: &str) -> Option<Type> {
    let lower = s.to_ascii_lowercase();
    if !lower.starts_with("array") {
        return None;
    }
    let rest = s[5..].trim_start();
    if !rest.starts_with('{') || !rest.ends_with('}') {
        return None;
    }
    let inner = &rest[1..rest.len() - 1];

    let mut fields = Vec::new();
    for field_str in split_top_level(inner, ',') {
        let field_str = field_str.trim();
        if field_str.is_empty() {
            continue;
        }
        let (key_part, type_part) = field_str.split_once(':')?;
        let key_part = key_part.trim();
        let (key, optional) = match key_part.strip_suffix('?') {
            Some(k) => (k.trim(), true),
            None => (key_part, false),
        };
        fields.push((
            key.to_string(),
            parse_type_string(type_part.trim()),
            optional,
        ));
    }
    Some(Type::ArrayShape(fields))
}

/// Splits a trailing `<...>` generic argument list off a type atom, e.g.
/// `"array<int, User>"` -> `("array", ["int", "User"])`. No arguments (or a
/// malformed/unclosed `<`) just returns the whole string with an empty list.
fn split_generic_args(s: &str) -> (&str, Vec<&str>) {
    let Some(open) = s.find('<') else {
        return (s, Vec::new());
    };
    if !s.ends_with('>') {
        return (s, Vec::new());
    }
    let base = &s[..open];
    let inner = &s[open + 1..s.len() - 1];
    let args = split_top_level(inner, ',')
        .into_iter()
        .map(str::trim)
        .collect();
    (base, args)
}

fn parse_type_atom(s: &str) -> Type {
    let s = s.trim();
    if let Some(shape) = try_parse_array_shape(s) {
        return shape;
    }
    // PHP 8.2 DNF (disjunctive normal form) group: `(Countable&Iterator)`
    // as one term of a union like `(Countable&Iterator)|null`. Only one
    // level of parens is legal in real PHP here, so this doesn't need to
    // recurse through parse_type_string - split_top_level's own `(`/`)`
    // depth tracking already kept this whole parenthesized chunk together
    // as a single union member; just strip the parens and parse what's
    // inside as the intersection it has to be.
    if let Some(inner) = s.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        return parse_type_string(inner);
    }

    // PHPDoc's `Type[]` array shorthand (`int[]`, even `string[][]`) has no
    // native-hint equivalent, but reusing this parser for doc-comment types
    // makes it worth handling here rather than in a separate PHPDoc-only
    // parser.
    let mut s = s;
    let mut array_depth = 0;
    while let Some(stripped) = s.strip_suffix("[]") {
        s = stripped.trim();
        array_depth += 1;
    }

    let (base, generic_args) = split_generic_args(s);
    let bare = base.trim().trim_start_matches('\\');
    let mut ty = match bare.to_ascii_lowercase().as_str() {
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
        // `array<V>` / `iterable<V>` / `list<V>` and `array<K, V>` (key
        // type discarded - keyed array subtyping isn't modeled) all
        // resolve to our existing homogeneous Array(value) representation;
        // no args is bare `array`. `list` specifically is a real,
        // widely-used PHPStan-ecosystem pseudo-type (a refined array with
        // sequential int keys) - without this case it fell through to the
        // generic-user-class branch below and got treated as a literal
        // class reference named "list", including getting
        // namespace-qualified into nonsense like `Some\Namespace\list`.
        "array" | "iterable" | "list" => match generic_args.len() {
            0 => Type::Array(Box::new(Type::Mixed)),
            1 => Type::Array(Box::new(parse_type_string(generic_args[0]))),
            _ => Type::Array(Box::new(parse_type_string(generic_args[1]))),
        },
        "callable" | "closure" => Type::Callable,
        "self" => Type::SelfRef,
        "parent" => Type::ParentRef,
        "static" => Type::StaticRef,
        "object" => Type::Object("object".to_string()),
        // A user-defined generic class (`Collection<User>`): the type
        // arguments are parsed (so they don't corrupt the surrounding
        // string) but not modeled - only the base class name survives,
        // which is still a strict improvement over the pre-generics
        // fallback of treating the whole "Collection<User>" string as an
        // unresolvable class name.
        _ => Type::Object(bare.to_string()),
    };
    for _ in 0..array_depth {
        ty = Type::Array(Box::new(ty));
    }
    ty
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

        // A shape is structurally an array, so it satisfies the bare
        // `array` hint; the reverse (does some homogeneous array actually
        // have these exact keys?) isn't verifiable without shape-aware
        // literal inference, so it's left unrelated rather than guessed.
        // Two shapes are only related via the general `a == b` case above -
        // no shape subtyping (e.g. "has at least these keys") is modeled.
        (Type::ArrayShape(_), Type::Array(b)) => matches!(**b, Type::Mixed),

        (Type::SelfRef | Type::ParentRef | Type::StaticRef, Type::Object(_)) => true,

        // An unbound template parameter is a placeholder, not a real type -
        // treat it as compatible with anything on either side rather than
        // ever read as a confident mismatch. Call sites should have already
        // substituted it away (checker::infer_call_return_type); this is
        // the safety net for wherever that doesn't happen.
        (Type::TemplateParam(_), _) | (_, Type::TemplateParam(_)) => true,

        _ => false,
    }
}

/// Unifies a template-bearing param type against an actual argument type,
/// recording the binding it implies. Only the two shapes that come up in
/// practice are handled: a bare `T` param, and `T[]`/`array<T>` unifying
/// against an array argument's element type. Anything else (a template
/// buried in a union, an array shape, etc) just isn't bound, which is safe
/// since `substitute_template` falls back to `Mixed` for anything left
/// unbound. First binding for a given name wins; conflicting uses of the
/// same `T` across multiple params aren't reconciled (no union-widening).
pub fn bind_template_param(param_ty: &Type, arg_ty: &Type, bindings: &mut HashMap<String, Type>) {
    match param_ty {
        Type::TemplateParam(name) => {
            if *arg_ty != Type::Mixed {
                bindings
                    .entry(name.clone())
                    .or_insert_with(|| arg_ty.clone());
            }
        }
        Type::Array(inner) => {
            if let Type::Array(arg_inner) = arg_ty {
                bind_template_param(inner, arg_inner, bindings);
            }
        }
        _ => {}
    }
}

/// Applies a set of template bindings through a type tree, replacing every
/// `TemplateParam` with its bound type (or `Mixed` if it was never bound).
pub fn substitute_template(ty: &Type, bindings: &HashMap<String, Type>) -> Type {
    match ty {
        Type::TemplateParam(name) => bindings.get(name).cloned().unwrap_or(Type::Mixed),
        Type::Array(inner) => Type::Array(Box::new(substitute_template(inner, bindings))),
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|m| substitute_template(m, bindings))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .iter()
                .map(|m| substitute_template(m, bindings))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .iter()
                .map(|(k, t, opt)| (k.clone(), substitute_template(t, bindings), *opt))
                .collect(),
        ),
        other => other.clone(),
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
    fn phpdoc_array_shorthand_parses() {
        assert_eq!(parse_type_string("int[]"), Type::Array(Box::new(Type::Int)));
        assert_eq!(
            parse_type_string("string[][]"),
            Type::Array(Box::new(Type::Array(Box::new(Type::String))))
        );
    }

    #[test]
    fn list_is_treated_as_an_array_alias_not_a_class_reference() {
        // A real, widely-used PHPStan-ecosystem pseudo-type (nikic/php-parser's
        // own `@return list<ClassMethod>` is exactly this shape) - without
        // this, "list" fell through to the generic-user-class branch and
        // got treated as a literal class named "list".
        assert_eq!(
            parse_type_string("list<int>"),
            Type::Array(Box::new(Type::Int))
        );
        assert_eq!(
            parse_type_string("list"),
            Type::Array(Box::new(Type::Mixed))
        );
    }

    #[test]
    fn generic_array_with_one_type_arg_parses_as_value_type() {
        assert_eq!(
            parse_type_string("array<int>"),
            Type::Array(Box::new(Type::Int))
        );
    }

    #[test]
    fn generic_array_with_key_and_value_uses_value_type() {
        assert_eq!(
            parse_type_string("array<string, int>"),
            Type::Array(Box::new(Type::Int))
        );
    }

    #[test]
    fn generic_user_class_keeps_base_name_and_drops_type_args() {
        assert_eq!(
            parse_type_string("Collection<User>"),
            Type::Object("Collection".to_string())
        );
    }

    #[test]
    fn union_inside_generic_args_is_not_a_top_level_union() {
        // Without depth-aware splitting, the `|` inside `<>` would be
        // wrongly treated as splitting the whole string.
        assert_eq!(
            parse_type_string("Collection<int|string>"),
            Type::Object("Collection".to_string())
        );
    }

    #[test]
    fn top_level_union_around_a_generic_still_splits_correctly() {
        assert_eq!(
            parse_type_string("Collection<int>|null"),
            Type::Union(vec![Type::Object("Collection".to_string()), Type::Null])
        );
    }

    #[test]
    fn array_shape_parses_required_and_optional_fields() {
        let shape = parse_type_string("array{name: string, age?: int}");
        assert_eq!(
            shape,
            Type::ArrayShape(vec![
                ("name".to_string(), Type::String, false),
                ("age".to_string(), Type::Int, true),
            ])
        );
    }

    #[test]
    fn array_shape_field_can_itself_be_a_union() {
        let shape = parse_type_string("array{value: int|string}");
        assert_eq!(
            shape,
            Type::ArrayShape(vec![(
                "value".to_string(),
                Type::Union(vec![Type::Int, Type::String]),
                false
            )])
        );
    }

    #[test]
    fn array_shape_display_round_trips_readably() {
        let shape = parse_type_string("array{name: string, age?: int}");
        assert_eq!(shape.to_string(), "array{name: string, age?: int}");
    }

    #[test]
    fn array_shape_is_subtype_of_bare_array_but_not_the_reverse() {
        let shape = parse_type_string("array{name: string}");
        assert!(is_subtype(&shape, &Type::Array(Box::new(Type::Mixed))));
        assert!(!is_subtype(&Type::Array(Box::new(Type::Mixed)), &shape));
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

    #[test]
    fn dnf_type_group_resolves_to_a_real_intersection_inside_the_union() {
        let ty = parse_type_string("(Countable&Iterator)|null");
        let Type::Union(members) = ty else {
            panic!("expected a top-level union");
        };
        assert_eq!(members.len(), 2);
        assert!(matches!(&members[0], Type::Intersection(parts) if parts.len() == 2));
        assert_eq!(members[1], Type::Null);
    }
}
