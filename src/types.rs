use std::collections::HashMap;
use std::fmt;

// A deliberately small slice of PHPStan's type system: enough to represent
// what our parser already captures as raw type-hint strings, plus the
// handful of pseudo-types (self/parent/static) that need special handling
// once a class-context resolver exists. `@template` generics (both a
// function/method's own and a class's - registry::mark_template_params /
// checker::class_template_bindings) are parsed and substituted at each
// call site; still no conditional return types, variance checking, or
// inheritance-chain substitution.
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
    // `class-string<T>` - a string known to hold `T`'s (or a subclass's)
    // fully-qualified name, tracked precisely enough to participate in
    // `@template` binding (unlike a bare `class-string`, which collapses to
    // plain `String` since there's no `T` to bind - see
    // `parse_type_keyword_or_class`). Produced by `Foo::class` and
    // `get_class($obj)` when the target class is statically known.
    ClassString(Box<Type>),
    // `array{name: string, age?: int}` - PHPDoc's structural array type.
    // (field name, field type, optional)
    ArrayShape(Vec<(String, Type, bool)>),
    // A class name plus its resolved `@template` type arguments, if any
    // (`Collection<User>` -> `Object("Collection", [Object("User", [])])`)
    // - empty for a non-generic class, or a generic one referenced bare
    // (`Collection` alone, with no `<...>`, is `Object("Collection", [])`
    // too; a bare reference to a generic class isn't an error here, just
    // an unspecified/erased one, the same way real PHPStan treats it).
    // Deliberately invariant, call-site-only substitution - no variance
    // checking, template bound enforcement, or inheritance-chain
    // substitution (a subclass fixing its parent's own type parameter via
    // `@extends Base<Concrete>` isn't modeled).
    Object(String, Vec<Type>),
    Callable,
    SelfRef,
    ParentRef,
    StaticRef,
    // An unbound `@template T` name - a placeholder for a type parameter
    // that hasn't been bound to a concrete type yet. Produced both for a
    // function/method's own declared template parameters and for a
    // class's own `@template` (registry::mark_template_params applies to
    // both, given the enclosing class's own template names alongside the
    // function's) - resolving one against an actual argument or an
    // instance's own carried type arguments is
    // checker::bind_template_param / substitute_template's job.
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
            Type::ClassString(inner) => write!(f, "class-string<{inner}>"),
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
            Type::Object(name, args) => {
                write!(f, "{name}")?;
                if !args.is_empty() {
                    let parts: Vec<String> = args.iter().map(Type::to_string).collect();
                    write!(f, "<{}>", parts.join(", "))?;
                }
                Ok(())
            }
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
        let members: Vec<Type> = pipe_parts.into_iter().map(parse_type_atom).collect();
        // Phase 32 Finding H: `Mixed` already includes every other
        // possible value, so a union with `Mixed` sitting alongside
        // anything else (`end()`'s real stub: `@return mixed|false`,
        // never updated after `end(): mixed` dropped its own native
        // `|false`) is simply `Mixed` - `union_types` (`checker/expr.rs`)
        // already special-cases this at *construction* time everywhere
        // it's used, but this is the lower-level parser building a
        // `Type::Union` directly from doc text, with no such
        // normalization. A stray non-`Mixed` member surviving otherwise
        // fails an ordinary argument-compatibility check on its own,
        // even though the whole union is vacuously satisfiable.
        if members.contains(&Type::Mixed) {
            return Type::Mixed;
        }
        Type::Union(members)
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
    let mut next_index: i64 = 0;
    for field_str in split_top_level(inner, ',') {
        let field_str = field_str.trim();
        if field_str.is_empty() {
            continue;
        }
        // PHPStan/Psalm's "unsealed shape" marker (`...<string, mixed>`
        // or bare `...`) says the array may carry other, unlisted keys
        // too - a real, if rare, extra-precision feature this project
        // doesn't model (same as the accessory/refinement types Phase
        // 10 already skips). Found the hard way: `?` below used to
        // propagate this field's missing `:` into failing the ENTIRE
        // shape parse, silently falling through to treating the whole
        // `array{...}` text as a literal (and then namespace-mangled)
        // class name - degrading just this one field is the safe
        // choice, not aborting every other real field alongside it.
        if field_str.starts_with("...") {
            continue;
        }
        // Phase 33 Finding 3: a *positional* (unkeyed) field
        // (`array{int, string}`, real PHPStan/Psalm tuple syntax) has no
        // `key:` prefix at all - `split_once(':')` alone can't tell that
        // apart from a keyed field whose own type happens to contain a
        // colon (`Closure(int): string`), so `looks_like_shape_key` also
        // checks that whatever comes before the candidate colon is
        // actually shaped like a key before trusting the split. Numbers
        // a positional field the same way `infer_array_literal_type`
        // numbers a real array literal's own positional elements, so a
        // doc-declared shape and a real literal's inferred shape end up
        // structurally identical.
        let keyed = field_str
            .split_once(':')
            .filter(|(key_part, _)| looks_like_shape_key(key_part.trim()));
        match keyed {
            Some((key_part, type_part)) => {
                let key_part = key_part.trim();
                let (key, optional) = match key_part.strip_suffix('?') {
                    Some(k) => (k.trim(), true),
                    None => (key_part, false),
                };
                if let Ok(n) = key.parse::<i64>() {
                    next_index = next_index.max(n + 1);
                }
                fields.push((
                    key.to_string(),
                    parse_type_string(type_part.trim()),
                    optional,
                ));
            }
            None => {
                let key = next_index.to_string();
                next_index += 1;
                fields.push((key, parse_type_string(field_str), false));
            }
        }
    }
    Some(Type::ArrayShape(fields))
}

/// Whether `key_part` (the text before a candidate `:` inside an
/// `array{...}` field) actually looks like a shape key - a bare
/// identifier (hyphen included: `Translator::toDictionaryEntry`'s real
/// `@return array{..., plural-forms: string, ...}`, gettext's own
/// dictionary-key convention, is unquoted in real code and was already
/// accepted before this project ever validated key shape at all - a
/// real regression, caught by the full corpus re-check, not assumed
/// safe), an integer, or a quoted string, optionally `?`-suffixed - as
/// opposed to being part of a *positional* field's own type text that
/// merely happens to contain a colon (a callable-shape return type,
/// `Closure(int): string`). See `try_parse_array_shape`'s own call site.
fn looks_like_shape_key(key_part: &str) -> bool {
    let key_part = key_part.strip_suffix('?').unwrap_or(key_part).trim();
    if key_part.is_empty() {
        return false;
    }
    if key_part.parse::<i64>().is_ok() {
        return true;
    }
    if key_part.starts_with('\'') && key_part.ends_with('\'') && key_part.len() >= 2 {
        return true;
    }
    if key_part.starts_with('"') && key_part.ends_with('"') && key_part.len() >= 2 {
        return true;
    }
    let mut chars = key_part.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
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

    // PHPStan/Psalm's callable-shape syntax: `Closure(ParamType, ...):
    // ReturnType` or `callable(...): ReturnType` - describes a callable
    // value's own signature, not a generic type-argument list (only
    // `<...>` is one, per `split_generic_args`) and not a real class
    // name despite `Closure(...)` superficially resembling one. This
    // project doesn't track a callable's own per-parameter/return
    // signature at all, so - matching its own established leniency for a
    // bare `callable`/`Closure` type elsewhere (`is_assignable`'s
    // `(Type::Array(_) | Type::ArrayShape(_), Type::Callable) => true`
    // and friends) - this degrades to the same `Type::Callable` a bare
    // `callable` hint already produces. Real corpus shape: Doctrine
    // DBAL's own `@param Closure(self):T $func` - without this, the
    // whole literal string `"Closure(self):T"` fell through to the
    // generic class-name branch below and got mangled into a bogus,
    // namespace-qualified "class name" (`Doctrine\DBAL\Closure(self):T`).
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("closure(")
        || lower.starts_with("closure (")
        || lower.starts_with("callable(")
        || lower.starts_with("callable (")
    {
        return Type::Callable;
    }

    // A real, documented PHPStan/Psalm convention: a quoted literal
    // (`'ASC'|'DESC'`, `"foo"`) as one member of a union means "exactly
    // this string value," not a class reference - without this, real
    // code in this project's own corpus (`@param 'ASC'|'DESC' $dir`)
    // fell through to the generic-class branch below and got the quotes
    // themselves treated as part of a literal class name, then
    // nonsensically namespace-qualified into e.g. `Piwigo\Db\'ASC'`.
    if let Some(inner) = s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return Type::StringLiteral(inner.to_string());
    }
    if let Some(inner) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Type::StringLiteral(inner.to_string());
    }

    // A real, documented PHPStan/Psalm convention, the same idea as the
    // quoted-string case just above: a bare integer literal as one
    // member of a union (`1|2|3`, a common way to constrain a value to a
    // fixed set of integers - log levels, status codes, mode flags, ...)
    // means "exactly this integer value," not a class reference -
    // without this it fell through to the generic-class branch below and
    // became a bogus `Type::Object("1", [])`, rejecting every value
    // (including genuinely valid ones) regardless of `is_subtype`'s own
    // otherwise-correct `IntLiteral`/`Union` handling.
    if let Ok(n) = s.parse::<i64>() {
        return Type::IntLiteral(n);
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
    let original = base.trim();
    let bare = original.trim_start_matches('\\');
    let mut ty = if bare.contains("::") {
        // A class-constant-reference type (`self::RFC_*`, `Foo::BAR`,
        // PHPStan's own wildcard-constant convention) isn't a class name
        // at all, even though it's shaped like one syntactically -
        // resolving it for real would mean looking up the named
        // constant('s value) on the referenced class, which this project
        // doesn't model. Left as `Mixed` (never flagged) rather than
        // falling through to the generic-class branch below, which used
        // to treat the whole string - including the "self"/class part -
        // as a literal class name, then (once namespace resolution
        // existed) nonsensically namespace-qualified it: real example,
        // monolog/monolog's own `@param self::RFC_* $rfc` became
        // `Monolog\Handler\self::RFC*`.
        Type::Mixed
    } else {
        parse_type_keyword_or_class(bare, original, &generic_args)
    };
    for _ in 0..array_depth {
        ty = Type::Array(Box::new(ty));
    }
    ty
}

/// `bare` (backslash-stripped) drives keyword matching; `original`
/// (backslash-preserved) is what the class-name fallback arm actually
/// constructs its `Type::Object` from - a leading `\` means "already
/// fully qualified from the global namespace," a real, meaningful
/// distinction `registry::NameContext::resolve` needs to see later (its
/// own first check is exactly `strip_prefix('\\')`) to know not to
/// re-qualify it against the current file's namespace. A real bug this
/// fixed: stripping the backslash *before* it ever reached `resolve`
/// made every absolute native type hint containing a `\` (e.g.
/// `QueryBuilder|\Doctrine\ORM\QueryBuilder`) look identical to a
/// relative one, silently mis-qualifying it as
/// `CurrentNamespace\Doctrine\ORM\QueryBuilder` - found via real code in
/// this project's own verification corpus.
fn parse_type_keyword_or_class(bare: &str, original: &str, generic_args: &[&str]) -> Type {
    match bare.to_ascii_lowercase().as_str() {
        // `positive-int`/`negative-int`/`non-zero-int`/`non-negative-int`/
        // `non-positive-int` are real, widely-used PHPStan-ecosystem
        // refinement pseudo-types (a constrained subset of `int`, the
        // same relationship `list` has to `array`) - without this they
        // fell through to the generic-user-class branch below and got
        // treated as a literal class reference, then (once namespace
        // resolution existed) nonsensically namespace-qualified.
        // `non-negative-int` specifically was a real, previously-missed
        // gap here: confirmed live via real code in this project's own
        // vendor/ corpus (`sebastianbergmann/php-code-coverage`), found
        // only once Phase 12's sprintf() format-string check started
        // comparing its resolved type against `%d` and got a bogus
        // `Vendor\Namespace\non-negative-int` object instead of `Int`.
        "int" | "integer" | "positive-int" | "negative-int" | "non-zero-int"
        | "non-negative-int" | "non-positive-int" => Type::Int,
        "float" | "double" => Type::Float,
        // Same story as the int refinements above, for string: real,
        // common PHPStan/Psalm-ecosystem refinement pseudo-types (real
        // volume seen in a live corpus: 2263 `non-empty-string`
        // occurrences alone across Piwigo's own vendor/ tree), not
        // separately modeled here beyond falling back to plain `string` -
        // `class-string`'s own optional `<Foo>` generic argument (`a
        // string holding Foo's fully-qualified name`) is similarly just
        // dropped rather than tracked.
        "string" | "non-empty-string" | "non-falsy-string" | "truthy-string" | "literal-string"
        | "numeric-string" | "callable-string" | "trait-string" | "interface-string"
        | "enum-string" => Type::String,
        // Unlike its string-refinement neighbors above, `class-string<T>`
        // carries a real, template-bindable inner type when given one -
        // needed for `@template R of Base` / `@param class-string<R> $x`
        // shapes to bind `R` from a real `Foo::class` argument. A bare
        // `class-string` (no `<T>`, the common case) has nothing to
        // preserve and stays plain `String`, unchanged from before.
        "class-string" => match generic_args {
            [] => Type::String,
            [single] => Type::ClassString(Box::new(parse_type_string(single))),
            _ => Type::String,
        },
        "bool" | "boolean" => Type::Bool,
        "true" => Type::True,
        "false" => Type::False,
        // Real, documented PHPStan/Psalm pseudo-types - without these
        // they fell through to the generic-user-class branch below and
        // got treated as a literal (nonsensical) class reference, the
        // same failure mode this whole match arm already exists to avoid
        // for its other keywords. `numeric` is an honest approximation
        // of real PHPStan's own `int|float|numeric-string` - this
        // project doesn't model numeric-string as a distinct literal-set
        // from plain `string` anyway, so `string` is the closest fit.
        "array-key" => Type::Union(vec![Type::Int, Type::String]),
        "scalar" => Type::Union(vec![Type::Int, Type::Float, Type::String, Type::Bool]),
        "numeric" => Type::Union(vec![Type::Int, Type::Float, Type::String]),
        // A real, if untyped-at-the-language-level, PHP value kind
        // (file/curl/... handles) that still appears constantly in real
        // docblocks - represented the same way the native `object` hint
        // is (a sentinel `Object`, not a real class), and given the same
        // namespace-qualification exemption below for the same reason:
        // "resource" isn't a real class reference to resolve per-file.
        "resource" => Type::Object("resource".to_string(), Vec::new()),
        "null" => Type::Null,
        "void" => Type::Void,
        "mixed" => Type::Mixed,
        "never" | "noreturn" => Type::Never,
        // `array<V>` / `iterable<V>` / `list<V>` and `array<K, V>` (key
        // type discarded - keyed array subtyping isn't modeled) all
        // resolve to our existing homogeneous Array(value) representation;
        // no args is bare `array`. `list`/`non-empty-array`/`non-empty-list`
        // are real, widely-used PHPStan-ecosystem refinement pseudo-types
        // (arrays with extra guarantees this project doesn't separately
        // track) - without these cases they fell through to the
        // generic-user-class branch below and got treated as a literal
        // class reference, then nonsensically namespace-qualified.
        // `class-string-map<TKey, TValue>` (Doctrine's own refinement of
        // `array<class-string<TKey>, TValue>`) fits the same "2-arg
        // generic, key discarded, value survives" shape the rest of this
        // arm already handles.
        "array" | "list" | "non-empty-array" | "class-string-map" => match generic_args.len() {
            0 => Type::Array(Box::new(Type::Mixed)),
            1 => Type::Array(Box::new(parse_type_string(generic_args[0]))),
            _ => Type::Array(Box::new(parse_type_string(generic_args[1]))),
        },
        // Unlike its neighbors above, `iterable` is real PHP's own native
        // type hint (not a PHPDoc-only pseudo-type), defined as exactly
        // `array|Traversable` - a broader type than `array`, not a
        // narrower refinement of it. Parsing it as bare `array` silently
        // dropped the entire `Traversable` half, rejecting any real
        // Generator/Iterator/IteratorAggregate passed to an
        // `iterable`-hinted parameter.
        "iterable" => {
            let element_ty = match generic_args.len() {
                0 => Type::Mixed,
                1 => parse_type_string(generic_args[0]),
                _ => parse_type_string(generic_args[1]),
            };
            Type::Union(vec![
                Type::Array(Box::new(element_ty)),
                Type::Object("Traversable".to_string(), Vec::new()),
            ])
        }
        "non-empty-list" => match generic_args.len() {
            0 => Type::Array(Box::new(Type::Mixed)),
            1 => Type::Array(Box::new(parse_type_string(generic_args[0]))),
            _ => Type::Array(Box::new(parse_type_string(generic_args[1]))),
        },
        // Deliberately not conflated - a native `Closure $fn` and a
        // native `callable $fn` are different, if this project doesn't
        // reflect that: `Closure` is a concrete class only a real
        // closure object satisfies, strictly narrower than the four real
        // shapes `callable` itself accepts (see `is_assignable`'s own
        // `Type::Callable` arms). Representing `Closure` as a real
        // `Object` (it's a real, already-stubbed class - no new `Type`
        // variant needed) means `is_assignable`'s ordinary class-hierarchy
        // check already does the right thing for it, unaffected by
        // `callable`'s own broader acceptance rules below.
        "closure" => Type::Object("Closure".to_string(), Vec::new()),
        "callable" => Type::Callable,
        "self" => Type::SelfRef,
        "parent" => Type::ParentRef,
        // `@return $this` is a real, PHPStan-documented PHPDoc convention -
        // identical in meaning to `@return static` (the calling instance's
        // own type) - not a variable reference despite the `$` sigil.
        // Without this it fell through to the generic-user-class branch
        // and became a literal "class" literally named "$this", then got
        // nonsensically namespace-qualified once that existed (real
        // example: amphp/amp's `@return $this` on several fluent methods
        // became `Amp\$this`).
        "static" | "$this" => Type::StaticRef,
        "object" => Type::Object("object".to_string(), Vec::new()),
        // A user-defined generic class (`Collection<User>`): each type
        // argument is itself a full type string (could be a union, a
        // nested generic, ...), so it's re-run through `parse_type_string`
        // - the same recursive-field pattern `try_parse_array_shape`
        // already uses. A bare (non-generic) reference just gets an empty
        // arg list, indistinguishable from "erased"/unspecified generics
        // on a class that does declare `@template` - `TemplateParam`
        // substitution treats an unbound parameter as `Mixed` either way.
        _ => Type::Object(
            original.to_string(),
            generic_args.iter().map(|a| parse_type_string(a)).collect(),
        ),
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
        // PHP's one documented exception to its own scalar type
        // declarations, even under strict_types=1: an int value is
        // always accepted wherever a float is declared (int-to-float
        // widening - explicit, load-bearing language behavior).
        (Type::Int | Type::IntLiteral(_), Type::Float) => true,

        (Type::Union(members), _) => members.iter().all(|m| is_subtype(m, sup)),
        (_, Type::Union(members)) => members.iter().any(|m| is_subtype(sub, m)),

        (Type::Intersection(members), _) => members.iter().any(|m| is_subtype(m, sup)),
        (_, Type::Intersection(members)) => members.iter().all(|m| is_subtype(sub, m)),

        (Type::Array(a), Type::Array(b)) => is_subtype(a, b),

        // `class-string<T>` stays freely interchangeable with plain
        // `string` in both directions - this project has no way to verify
        // a string's actual content names a real class, so a `::class`
        // literal (now precisely tracked) must still satisfy an ordinary
        // `string`-typed sink, and a plain, unrefined `string` must still
        // satisfy a `class-string<T>`-typed one. Two `ClassString`s
        // compared against each other recurse the same way `Array` does
        // just above, for a `class-string<T>` result flowing into another
        // `class-string<T>`-typed sink without erasing to bare `String`
        // along the way.
        (Type::ClassString(a), Type::ClassString(b)) => is_subtype(a, b),
        (Type::ClassString(_), Type::String) => true,
        (Type::StringLiteral(_), Type::ClassString(_)) => true,
        (Type::String, Type::ClassString(_)) => true,

        // A shape is structurally an array, so it satisfies the bare
        // `array` hint; the reverse (does some homogeneous array actually
        // have these exact keys?) isn't verifiable without shape-aware
        // literal inference, so it's left unrelated rather than guessed.
        // Two shapes are only related via the general `a == b` case above -
        // no shape subtyping (e.g. "has at least these keys") is modeled.
        (Type::ArrayShape(_), Type::Array(b)) => matches!(**b, Type::Mixed),

        (Type::SelfRef | Type::ParentRef | Type::StaticRef, Type::Object(..)) => true,

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
        Type::ClassString(inner) => {
            if let Type::ClassString(arg_inner) = arg_ty {
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
        Type::ClassString(inner) => {
            Type::ClassString(Box::new(substitute_template(inner, bindings)))
        }
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

    // Phase 32 Finding H, real shape: `end()`'s real stub is `function
    // end(object|array &$array): mixed {}` with a never-updated
    // `@return mixed|false` doc comment - `Mixed` already includes every
    // other possible value, so a stray `False` (or `Null`) member sitting
    // alongside it fails an ordinary argument-compatibility check on its
    // own, even though the whole union is vacuously satisfiable.
    #[test]
    fn a_union_containing_mixed_collapses_to_plain_mixed() {
        assert_eq!(parse_type_string("mixed|false"), Type::Mixed);
        assert_eq!(parse_type_string("mixed|null"), Type::Mixed);
    }

    #[test]
    fn an_ordinary_union_with_no_mixed_member_is_unaffected() {
        assert_eq!(
            parse_type_string("int|string"),
            Type::Union(vec![Type::Int, Type::String])
        );
    }

    #[test]
    fn quoted_literal_string_union_members_are_string_literals_not_class_names() {
        // Real, previously-live bug found via real code in this
        // project's own verification corpus (`@param 'ASC'|'DESC' $dir`,
        // a real, documented PHPStan/Psalm literal-string-union
        // convention): without recognizing the quotes, `'ASC'` fell
        // through to the generic-class branch and became a literal
        // class named `'ASC'` (quotes included), then got nonsensically
        // namespace-qualified once that machinery ran (`Piwigo\Db\'ASC'`).
        assert_eq!(
            parse_type_string("'ASC'|'DESC'"),
            Type::Union(vec![
                Type::StringLiteral("ASC".to_string()),
                Type::StringLiteral("DESC".to_string()),
            ])
        );
        assert_eq!(
            parse_type_string("\"foo\""),
            Type::StringLiteral("foo".to_string())
        );
    }

    #[test]
    fn intersection_parses_flat() {
        assert_eq!(
            parse_type_string("Countable&Iterator"),
            Type::Intersection(vec![
                Type::Object("Countable".to_string(), Vec::new()),
                Type::Object("Iterator".to_string(), Vec::new())
            ])
        );
    }

    #[test]
    fn a_leading_backslash_is_preserved_by_the_bare_parser() {
        // parse_type_string alone has no namespace context - it's not
        // this layer's job to decide what an absolute reference resolves
        // to, only to preserve the fact that it *is* absolute for
        // registry::resolve_names_in_type to act on later. Stripping the
        // backslash here (the old, buggy behavior this test used to
        // assert) permanently lost that distinction before namespace
        // resolution ever ran, silently mis-qualifying every absolute
        // reference as if it were relative - see
        // registry.rs's own `a_leading_backslash_in_a_native_type_hint_is_not_re_namespaced`
        // for the real, end-to-end regression this caused.
        assert_eq!(
            parse_type_string("\\App\\Foo"),
            Type::Object("\\App\\Foo".to_string(), Vec::new())
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
    fn string_refinement_pseudo_types_are_treated_as_plain_string() {
        // Real, common PHPStan/Psalm-ecosystem refinement types (real
        // volume seen in a live corpus: 2263 non-empty-string occurrences
        // alone) - without this they fell through to the generic-user-class
        // branch and got treated as literal class references.
        for name in [
            "non-empty-string",
            "non-falsy-string",
            "truthy-string",
            "literal-string",
            "numeric-string",
            "callable-string",
            "class-string",
            "trait-string",
            "interface-string",
            "enum-string",
        ] {
            assert_eq!(parse_type_string(name), Type::String, "for {name}");
        }
    }

    #[test]
    fn class_string_with_a_generic_argument_is_tracked_precisely() {
        assert_eq!(
            parse_type_string("class-string<Foo>"),
            Type::ClassString(Box::new(Type::Object("Foo".to_string(), Vec::new())))
        );
        // Two or more args isn't real PHPStan syntax - falls back to plain
        // string rather than guessing which one is meant.
        assert_eq!(parse_type_string("class-string<Foo, Bar>"), Type::String);
    }

    #[test]
    fn class_string_is_interchangeable_with_plain_string_both_ways() {
        let cs = Type::ClassString(Box::new(Type::Object("Foo".to_string(), Vec::new())));
        assert!(is_subtype(&cs, &Type::String));
        assert!(is_subtype(&Type::String, &cs));
        assert!(is_subtype(&Type::StringLiteral("Foo".to_string()), &cs));
        assert!(is_subtype(&cs, &cs));
        let cs_other = Type::ClassString(Box::new(Type::Object("Bar".to_string(), Vec::new())));
        assert!(!is_subtype(&cs, &cs_other));
    }

    #[test]
    fn int_refinement_pseudo_types_are_treated_as_plain_int() {
        for name in [
            "positive-int",
            "negative-int",
            "non-zero-int",
            "non-negative-int",
            "non-positive-int",
        ] {
            assert_eq!(parse_type_string(name), Type::Int, "for {name}");
        }
    }

    #[test]
    fn dollar_this_phpdoc_pseudo_type_is_treated_as_static() {
        // `@return $this` is a real, documented PHPStan/Psalm convention
        // (real example: amphp/amp's Interval::disable() and others use
        // exactly this) - identical in meaning to `@return static`, not a
        // variable reference. Without this it fell through to the
        // generic-user-class branch and became a literal class named
        // "$this", then got nonsensically namespace-qualified once that
        // existed (`Amp\$this`).
        assert_eq!(parse_type_string("$this"), Type::StaticRef);
    }

    #[test]
    fn class_constant_reference_types_degrade_to_mixed() {
        // `self::RFC_*` / `Foo::BAR` (PHPStan's wildcard-constant
        // convention, real example: monolog/monolog's own
        // `@param self::RFC_* $rfc`) look like a class name syntactically
        // but aren't one - resolving them for real would mean looking up
        // the referenced constant('s value), which this project doesn't
        // model. Without this check they fell through to the
        // generic-class branch and became a literal class literally
        // named "self::RFC_*", then got nonsensically namespace-qualified
        // once that existed (`Monolog\Handler\self::RFC*`).
        assert_eq!(parse_type_string("self::RFC_*"), Type::Mixed);
        assert_eq!(parse_type_string("Foo::BAR"), Type::Mixed);
        // The array-wrapping/nullable machinery around parse_type_atom
        // still has to apply correctly around this degraded case.
        assert_eq!(
            parse_type_string("self::RFC_*[]"),
            Type::Array(Box::new(Type::Mixed))
        );
    }

    #[test]
    fn array_refinement_pseudo_types_are_treated_as_plain_array() {
        assert_eq!(
            parse_type_string("non-empty-array<int>"),
            Type::Array(Box::new(Type::Int))
        );
        assert_eq!(
            parse_type_string("non-empty-list<string>"),
            Type::Array(Box::new(Type::String))
        );
        // Doctrine's own refinement of `array<class-string<TKey>, TValue>`
        // - the key type (including its own nested generic) is discarded,
        // same as a plain `array<K, V>`'s key already is.
        assert_eq!(
            parse_type_string("class-string-map<Foo, int>"),
            Type::Array(Box::new(Type::Int))
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
    fn generic_user_class_keeps_base_name_and_type_args() {
        assert_eq!(
            parse_type_string("Collection<User>"),
            Type::Object(
                "Collection".to_string(),
                vec![Type::Object("User".to_string(), Vec::new())]
            )
        );
    }

    #[test]
    fn union_inside_generic_args_is_not_a_top_level_union() {
        // Without depth-aware splitting, the `|` inside `<>` would be
        // wrongly treated as splitting the whole string.
        assert_eq!(
            parse_type_string("Collection<int|string>"),
            Type::Object(
                "Collection".to_string(),
                vec![Type::Union(vec![Type::Int, Type::String])]
            )
        );
    }

    #[test]
    fn top_level_union_around_a_generic_still_splits_correctly() {
        assert_eq!(
            parse_type_string("Collection<int>|null"),
            Type::Union(vec![
                Type::Object("Collection".to_string(), vec![Type::Int]),
                Type::Null
            ])
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
    fn array_shape_with_an_unsealed_marker_still_parses_its_real_fields() {
        // `...<K, V>` (PHPStan/Psalm's "this shape may carry other,
        // unlisted keys too" marker - not modeled, same as other
        // accessory/refinement types this project skips) has no `:` in
        // it - found on real code that this used to make the field loop's
        // `?` propagate a None out of the whole function, silently
        // failing the ENTIRE shape parse (falling through to treating
        // `array{...}` as a literal, then namespace-mangled, class name)
        // over one unmodeled marker, discarding every real field
        // alongside it.
        let shape = parse_type_string("array{items: int, ...<string, mixed>}");
        assert_eq!(
            shape,
            Type::ArrayShape(vec![("items".to_string(), Type::Int, false)])
        );
        // A bare `...` (no generic args) is valid too.
        let shape2 = parse_type_string("array{items: int, ...}");
        assert_eq!(
            shape2,
            Type::ArrayShape(vec![("items".to_string(), Type::Int, false)])
        );
    }

    // Phase 33 Finding 3: a positional (unkeyed) field - real, valid
    // PHPStan/Psalm tuple syntax - used to fail the *entire* shape's parse
    // (`split_once(':')?` had nothing to split), falling through to a
    // bogus, unparsed class-name `Object`. Numbered the same way
    // `infer_array_literal_type` numbers a real array literal's own
    // positional elements, so both end up structurally identical.
    #[test]
    fn array_shape_with_positional_fields_numbers_them_like_a_real_array_literal() {
        let shape = parse_type_string("array{int, string}");
        assert_eq!(
            shape,
            Type::ArrayShape(vec![
                ("0".to_string(), Type::Int, false),
                ("1".to_string(), Type::String, false),
            ])
        );
    }

    // The real `idcUploadSizedPhoto` shape this finding exists for.
    #[test]
    fn array_shape_with_positional_int_range_fields_parses() {
        let shape = parse_type_string("array{int<0, 255>, int<0, 255>, int<0, 255>}");
        assert_eq!(
            shape,
            Type::ArrayShape(vec![
                ("0".to_string(), Type::Int, false),
                ("1".to_string(), Type::Int, false),
                ("2".to_string(), Type::Int, false),
            ])
        );
    }

    #[test]
    fn array_shape_mixing_explicit_int_keys_and_named_keys_still_parses_correctly() {
        let shape = parse_type_string("array{0: int, name: string}");
        assert_eq!(
            shape,
            Type::ArrayShape(vec![
                ("0".to_string(), Type::Int, false),
                ("name".to_string(), Type::String, false),
            ])
        );
    }

    // Regression guard: Translator::toDictionaryEntry's real
    // @return array{domain: string, plural-forms: string, ...} - found by
    // the full corpus re-check after this fix first shipped, since a
    // hyphenated bare key was already accepted before any key-shape
    // validation existed at all, and `looks_like_shape_key`'s first
    // version wrongly rejected it, misclassifying the field as
    // positional.
    #[test]
    fn array_shape_with_a_hyphenated_bare_key_still_parses_as_keyed() {
        let shape = parse_type_string("array{domain: string, plural-forms: string}");
        assert_eq!(
            shape,
            Type::ArrayShape(vec![
                ("domain".to_string(), Type::String, false),
                ("plural-forms".to_string(), Type::String, false),
            ])
        );
    }

    // A positional field whose own type contains a colon (a callable
    // shape's return type) must not be mis-split at that internal colon.
    #[test]
    fn array_shape_with_a_positional_closure_field_is_not_split_at_its_own_colon() {
        let shape = parse_type_string("array{Closure(int): string, int}");
        assert_eq!(
            shape,
            Type::ArrayShape(vec![
                ("0".to_string(), Type::Callable, false),
                ("1".to_string(), Type::Int, false),
            ])
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
        assert!(is_subtype(
            &Type::Object("Foo".to_string(), Vec::new()),
            &Type::Mixed
        ));
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
    fn intersection_subtype_rules() {
        // Coverage measurement found both Intersection arms of is_subtype
        // had zero executions (registry::is_assignable's own, separate
        // Intersection handling was covered independently).
        let both = Type::Intersection(vec![Type::Int, Type::String]);
        // sub is the intersection: satisfies any one of its members.
        assert!(is_subtype(&both, &Type::Int));
        assert!(is_subtype(&both, &Type::String));
        assert!(!is_subtype(&both, &Type::Bool));
        // sup is the intersection: must satisfy every member at once - a
        // plain Int alone doesn't also satisfy "and a String".
        assert!(!is_subtype(&Type::Int, &both));
    }

    #[test]
    fn true_and_false_literals_are_subtypes_of_bool() {
        assert!(is_subtype(&Type::True, &Type::Bool));
        assert!(is_subtype(&Type::False, &Type::Bool));
    }

    #[test]
    fn substitute_template_recurses_into_array_and_intersection_types() {
        // Coverage measurement found substitute_template's Array and
        // Intersection arms had zero executions - every existing
        // template-substitution test only substitutes a bare
        // TemplateParam or one nested inside a Union, never `T[]`/
        // `array<T>` or an intersection type mentioning T.
        let mut bindings = HashMap::new();
        bindings.insert("T".to_string(), Type::String);

        let array_of_t = Type::Array(Box::new(Type::TemplateParam("T".to_string())));
        assert_eq!(
            substitute_template(&array_of_t, &bindings),
            Type::Array(Box::new(Type::String))
        );

        let intersection_with_t = Type::Intersection(vec![
            Type::TemplateParam("T".to_string()),
            Type::Object("Countable".to_string(), Vec::new()),
        ]);
        assert_eq!(
            substitute_template(&intersection_with_t, &bindings),
            Type::Intersection(vec![
                Type::String,
                Type::Object("Countable".to_string(), Vec::new())
            ])
        );
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

    // Phase 30, real shape: Doctrine DBAL's own `@param Closure(self):T
    // $func` (`Connection::transactional()`) - the callable-shape syntax
    // wasn't recognized at all, so the whole literal string
    // `"Closure(self):T"` fell through to the generic class-name branch
    // and got mangled into a bogus, namespace-qualified "class name"
    // once resolved against the current file's own namespace.
    #[test]
    fn closure_shape_syntax_degrades_to_plain_callable() {
        assert_eq!(parse_type_string("Closure(self):T"), Type::Callable);
        assert_eq!(
            parse_type_string("Closure(int, string): bool"),
            Type::Callable
        );
        assert_eq!(parse_type_string("callable(int): void"), Type::Callable);
    }

    #[test]
    fn closure_shape_syntax_still_works_inside_a_union() {
        let ty = parse_type_string("Closure(self):T|null");
        let Type::Union(members) = ty else {
            panic!("expected a top-level union, got {ty:?}");
        };
        assert_eq!(members, vec![Type::Callable, Type::Null]);
    }

    #[test]
    fn a_class_name_merely_starting_with_closure_is_not_mistaken_for_the_callable_shape() {
        // `strip_prefix("closure")` alone would wrongly match a real
        // class like `ClosureFactory` too - only a literal `(` right
        // after `closure`/`callable` (ignoring case) counts.
        assert_eq!(
            parse_type_string("ClosureFactory"),
            Type::Object("ClosureFactory".to_string(), Vec::new())
        );
    }
}
