use crate::types::{AccessoryKind, Type};

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
///
/// A `'...'`/`"..."` string-literal segment is tracked as one opaque run,
/// the exact same fix `docblock.rs`'s own `take_type_token` needed for the
/// identical reason - without it, a bracket-shaped character *inside* the
/// quotes (a real corpus shape: PHPUnit's own literal-string union
/// `'->'|'::' `, documenting a real property-access-vs-static-call
/// operator) was read as a real closing bracket, permanently decrementing
/// `depth` below zero and silently swallowing every top-level separator
/// for the rest of the string - `'->'|'::' ` mangled into the single,
/// nonsensical literal `Type::StringLiteral("->'|'::")`.
fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
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

/// PHPStan's own conditional return type syntax: `($param is SomeType ?
/// TrueType : FalseType)` - real PHPStan evaluates this per call site,
/// binding `$param`'s own actual argument; this project has no
/// per-call-site type-expression evaluation at all. Previously, the
/// DNF-group unwrap right above this function's own caller silently
/// mis-parsed the whole expression as an ordinary intersection group,
/// mangling it into nonsense (`$param`/`is`/etc. each getting treated as
/// class-name fragments, then namespace-qualified into garbage like
/// `Piwigo\Mail\$action`). Takes the safe, imprecise-but-correct answer
/// instead: the union of both branches, matching "can't evaluate, don't
/// guess which one" the same way this project's other unevaluable-
/// expression leniency already works elsewhere. Requires the literal
/// `is`/`?`/`:` tokens at the top level (`split_top_level`'s own
/// bracket-depth tracking) so this never misfires on an ordinary DNF
/// group, which has no ternary syntax at all.
fn try_parse_conditional_type(s: &str) -> Option<Type> {
    let q_parts = split_top_level(s, '?');
    let [cond, branches] = q_parts.as_slice() else {
        return None;
    };
    if !cond.contains(" is ") {
        return None;
    }
    let c_parts = split_top_level(branches, ':');
    let [true_branch, false_branch] = c_parts.as_slice() else {
        return None;
    };
    let true_ty = parse_type_string(true_branch);
    let false_ty = parse_type_string(false_branch);
    Some(if true_ty == false_ty {
        true_ty
    } else {
        Type::Union(vec![true_ty, false_ty])
    })
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
        // too. Recorded as a sentinel field (`"..."`, impossible to
        // collide with any real key this same grammar could ever parse -
        // `looks_like_shape_key` only ever accepts a bare integer, a
        // quoted string, or a bare-identifier-shaped token, never three
        // literal dots) rather than the value's own type, which this
        // project doesn't attempt to track precisely for the unsealed
        // portion (`Type::Mixed`, matching a plain `array`'s own
        // unknown-value-type default) - `is_unsealed_shape`/
        // `narrowing::narrow_array_shape_by_key_presence`'s own doc
        // comment cover how this gets consumed. Previously silently
        // discarded entirely (found the hard way: `?` below used to
        // propagate this field's missing `:` into failing the ENTIRE
        // shape parse, silently falling through to treating the whole
        // `array{...}` text as a literal, then namespace-mangled, class
        // name - degrading just this one field, rather than aborting
        // every other real field alongside it, is still the right
        // choice, just no longer a silent no-op once degraded).
        if field_str.starts_with("...") {
            fields.push(("...".to_string(), Type::Mixed, false));
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
                // `looks_like_shape_key` above already accepts a
                // `'...'`/`"..."`-quoted key (a real, common shape for a
                // key that isn't a valid bare identifier, e.g. `array{
                // 'content-type': string}`) - the surrounding quote
                // characters themselves were never stripped before this
                // fix, so the field ended up keyed by the *literal*
                // quoted text (`"'content-type'"`) instead of the real
                // string PHP itself would use as the array key
                // (`"content-type"`), silently breaking every real
                // lookup against it (`literal_string_key` on `$arr
                // ['content-type']` produces the unquoted form).
                let key = strip_shape_key_quotes(key);
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

/// `object{name: string, age?: int}` - PHPDoc's structural object shape.
/// Mirrors `try_parse_array_shape` exactly (same field grammar, same
/// per-field `parse_type_string` recursion) - just builds `ObjectShape`
/// instead of `ArrayShape`, and has no positional-field/`...`-unsealed-
/// marker concept to carry over (an object's own properties are always
/// named, never positional).
fn try_parse_object_shape(s: &str) -> Option<Type> {
    let lower = s.to_ascii_lowercase();
    if !lower.starts_with("object") {
        return None;
    }
    let rest = s[6..].trim_start();
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
        let Some((key_part, type_part)) = field_str
            .split_once(':')
            .filter(|(key_part, _)| looks_like_shape_key(key_part.trim()))
        else {
            // Same "degrade just this one field" leniency `try_parse_
            // array_shape`'s own `...`-marker arm documents - an
            // unrecognized field shouldn't abort every other real field
            // alongside it.
            continue;
        };
        let key_part = key_part.trim();
        let (key, optional) = match key_part.strip_suffix('?') {
            Some(k) => (k.trim(), true),
            None => (key_part, false),
        };
        // See `try_parse_array_shape`'s own identical fix's doc comment.
        let key = strip_shape_key_quotes(key);
        fields.push((
            key.to_string(),
            parse_type_string(type_part.trim()),
            optional,
        ));
    }
    Some(Type::ObjectShape(fields))
}

/// Strips one layer of matching `'...'`/`"..."` quoting from a shape
/// field key, if present - `looks_like_shape_key` accepts a quoted key
/// as a real, common way to write one that isn't a valid bare identifier
/// (`array{'content-type': string}`), but the field itself must be
/// stored under the *unquoted* text (`"content-type"`, what `literal_
/// string_key` on a real `$arr['content-type']` access actually
/// produces) - otherwise every real lookup against it silently fails to
/// match. A bare (unquoted) key, integer or identifier-shaped, passes
/// through unchanged.
/// The inverse of `strip_shape_key_quotes`, for display: a shape key
/// shown bare only when it's exactly the shape `looks_like_shape_key`
/// itself accepts unquoted (a bare integer, or an identifier-shaped
/// token - alphabetic/underscore start, alphanumeric/underscore/hyphen
/// rest) - anything else (a key containing a space, a leading digit
/// followed by non-digits, an empty string, ...) is single-quoted, so a
/// diagnostic message displaying this type stays round-trippable back
/// through this same parser rather than silently showing a key text
/// that would parse as something else (or fail to parse at all) if
/// pasted back into a real `@var`/`@param` tag.
pub(crate) fn display_shape_key(key: &str) -> String {
    let is_bare_integer = key.parse::<i64>().is_ok();
    let is_bare_identifier = {
        let mut chars = key.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    if is_bare_integer || is_bare_identifier {
        key.to_string()
    } else {
        format!("'{}'", key.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

fn strip_shape_key_quotes(key: &str) -> &str {
    for quote in ['\'', '"'] {
        if let Some(inner) = key.strip_prefix(quote).and_then(|k| k.strip_suffix(quote)) {
            return inner;
        }
    }
    key
}

/// Phase 80 follow-up, real PHPStan's own `OffsetAccessType::getResult()`:
/// extracts `key`'s own value type out of `base`. Only the shapes that
/// come up in practice are resolved (`ArrayShape`/`ObjectShape`'s own
/// per-field lookup, a plain `Array`'s uniform value type, `Union`
/// recursing member-wise); anything else (an unresolved class, `Mixed`,
/// ...) falls back to `Mixed` - safe, and matches this parser's own
/// established "an unresolvable PHPDoc construct never hard-fails,
/// it just degrades" convention throughout.
fn offset_access_type(base: &Type, key: &str) -> Type {
    match base {
        Type::ArrayShape(fields) | Type::ObjectShape(fields) => fields
            .iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, v, _)| v.clone())
            .unwrap_or(Type::Mixed),
        Type::Array(_, value_ty) => (**value_ty).clone(),
        Type::Union(members) => {
            let resolved: Vec<Type> = members.iter().map(|m| offset_access_type(m, key)).collect();
            match resolved.len() {
                0 => Type::Mixed,
                1 => resolved.into_iter().next().unwrap(),
                _ => Type::Union(resolved),
            }
        }
        _ => Type::Mixed,
    }
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
/// `key-of<array{foo: string, 5: int}>`'s own per-field answer - a
/// shape's own field name is always stored as a plain `String` (see
/// `ArrayShape`'s own doc comment), the same way a real array literal's
/// keys are canonicalized, so a field name that parses as an integer
/// names an int key, matching PHP's own key-canonicalization rule.
fn shape_key_literal(name: &str) -> Type {
    match name.parse::<i64>() {
        Ok(n) => Type::IntLiteral(n),
        Err(_) => Type::StringLiteral(name.to_string()),
    }
}

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
    if let Some(shape) = try_parse_object_shape(s) {
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
        if let Some(conditional) = try_parse_conditional_type(inner) {
            return conditional;
        }
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
    // Stripped of a leading `\` first - real corpus code commonly spells
    // this fully qualified (`@param \Closure(CancelledException) $cb`,
    // amphp/amp's own convention throughout), which otherwise defeated
    // this exact check (`"\closure(...".starts_with("closure(")` is
    // false), falling through to the generic-class branch and mangling
    // into a bogus `Type::Object("Closure(CancelledException)", [])`.
    let lower = s.trim_start_matches('\\').to_ascii_lowercase();
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
    // Phase 14: mirrors the int-literal case just above, for a real
    // float literal (`1.5|2.5`) - checked after the int case so an
    // integer-looking string is still caught as `IntLiteral` first.
    if let Ok(f) = s.parse::<f64>() {
        return Type::FloatLiteral(f);
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

    // Real, serious corpus find (nikic/php-parser's own `@param
    // array{int, string, string}[] $patches`): `try_parse_array_shape`/
    // `try_parse_object_shape` are only ever tried once, at the very top
    // of this function, against the *original*, un-stripped `s` - a
    // shape wrapped in the `[]`-array-shorthand suffix just stripped
    // above (`array{...}[]`, `object{...}[]`) never ends with the shape's
    // own closing `}` at that point (it ends with `[]`), so both checks
    // correctly declined, and the shape was never re-attempted against
    // the *reduced* string afterward - falling straight through to the
    // generic class-name branch far below and mangling the shape's own
    // literal text into a bogus, namespace-qualified "class name" (this
    // exact corpus case: `PhpParser\Lexer\array{int, string, string}`).
    if array_depth > 0
        && let Some(shape) = try_parse_array_shape(s).or_else(|| try_parse_object_shape(s))
    {
        let mut ty = shape;
        for _ in 0..array_depth {
            ty = Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty));
        }
        return ty;
    }

    // Phase 80 follow-up: PHPDoc/PHPStan's own `OffsetAccessType` syntax -
    // `SomeShape['key']` (a bare/unquoted key works too, e.g. an int
    // key) extracts one key's own value type out of whatever `SomeShape`
    // resolves to. Checked *after* the `[]`-array-shorthand loop just
    // above (which only ever strips a genuinely *empty* bracket, so it
    // can't have consumed this) - `SomeShape['key'][]` still parses
    // correctly this way: the trailing empty `[]` is stripped first
    // (`array_depth = 1`), leaving exactly `SomeShape['key']` for this
    // check to resolve, then wrapped back into an array below like any
    // other array-shorthand result. Deliberately not specially
    // recursive for a chained `Foo['a']['b']`: `s.rfind('[')` finds the
    // *last* bracket pair first (`['b']`), leaving `base_part` as
    // `"Foo['a']"` - recursing into `parse_type_string` for that
    // naturally resolves the inner `['a']` the identical way on its own
    // next call, one bracket peeled per level.
    if s.ends_with(']')
        && let Some(open) = s.rfind('[')
    {
        let key_part = s[open + 1..s.len() - 1].trim();
        if !key_part.is_empty() {
            let base_part = s[..open].trim();
            let key = strip_shape_key_quotes(key_part);
            let base_ty = parse_type_string(base_part);
            let mut ty = offset_access_type(&base_ty, key);
            for _ in 0..array_depth {
                ty = Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty));
            }
            return ty;
        }
    }

    // `(A|B)[]`/`(A&B)[]` - a parenthesized union/intersection wrapped in
    // the same array-shorthand syntax, needed whenever the element type
    // itself isn't a single bare name (`@param (string|int)[] $keys`,
    // a real, common PHPDoc idiom - PHPStan's own array-shorthand grammar
    // requires the parens here since `A|B[]` alone would ambiguously read
    // as `A|(B[])`). Recurses into `parse_type_string`, not `parse_type_
    // atom` - the inner content can have its own top-level union/
    // intersection that needs `split_top_level`'s own handling, which
    // this function never does itself (its caller, `parse_type_string`,
    // already owns that). Without this, the whole parenthesized text -
    // parens, pipe, and all - fell through to the generic class-name
    // branch below and got nonsensically namespace-qualified as if
    // `"(string|int)"` were a real class name.
    if array_depth > 0
        && let Some(inner) = s.strip_prefix('(').and_then(|s| s.strip_suffix(')'))
    {
        let mut ty = parse_type_string(inner);
        for _ in 0..array_depth {
            ty = Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty));
        }
        return ty;
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
        ty = Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty));
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
/// Parses one bound of an `int<min, max>` range - real PHPStan's own
/// `"min"`/`"max"` keywords mean unbounded on that side (`Some(None)`),
/// a plain integer literal is a real bound (`Some(Some(n))`). A
/// malformed bound (neither) is `None` - distinct from "unbounded" so
/// the caller can fall back to plain `Type::Int` instead of silently
/// treating garbage as an open range.
fn parse_int_bound(s: &str) -> Option<Option<i64>> {
    match s {
        "min" | "max" => Some(None),
        s => s.parse().ok().map(Some),
    }
}

fn parse_type_keyword_or_class(bare: &str, original: &str, generic_args: &[&str]) -> Type {
    match bare.to_ascii_lowercase().as_str() {
        // Phase 14: `int<min, max>` and its named shorthands are real
        // ranges now, not aliased to plain `Int` - `IntRange` fully
        // replaces `Int`'s own range, the same relationship `IntLiteral`
        // has to `Int`. `int`/`integer` additionally read `generic_args`
        // when present (`int<0, 255>`, already structurally parsed as
        // generic args today) - a malformed bound falls back to plain
        // `Type::Int` rather than guessing.
        "int" | "integer" => match generic_args {
            [min, max] => match (parse_int_bound(min), parse_int_bound(max)) {
                (Some(min), Some(max)) => Type::IntRange(min, max),
                _ => Type::Int,
            },
            _ => Type::Int,
        },
        "positive-int" => Type::IntRange(Some(1), None),
        "negative-int" => Type::IntRange(None, Some(-1)),
        "non-negative-int" => Type::IntRange(Some(0), None),
        "non-positive-int" => Type::IntRange(None, Some(0)),
        // Real PHPStan represents this as a union of two ranges, not its
        // own type (confirmed via dumpType()) - composed the same way.
        "non-zero-int" => Type::Union(vec![
            Type::IntRange(None, Some(-1)),
            Type::IntRange(Some(1), None),
        ]),
        // `int-mask<Flag::*>`/`int-mask-of<Flag::*>` - real PHPStan
        // expands these into a real union of every combinable bitmask
        // value, which this project has no way to enumerate without
        // resolving the referenced class constants (the same "no lazy/
        // deferred resolution" boundary `value-of`/`key-of` below already
        // hit). Falling through to the generic class-name branch instead
        // produced a bogus namespace-qualified "class" like `Composer\
        // Pcre\int-mask<Composer\Pcre\PREG_UNMATCHED_AS_NULL>`, which
        // then failed every ordinary `int`-typed assignability check -
        // a real corpus false positive. Degrading to plain `Type::Int`
        // (the pseudo-type's own real underlying kind) is a strict
        // improvement, matching this project's own "don't guess past
        // what's verifiable" convention for the string-identity pseudo-
        // types just below. Any `<...>` generic argument is parsed (via
        // the caller's existing generic-arg splitting) and simply
        // discarded here, not left unconsumed.
        "int-mask" | "int-mask-of" => Type::Int,
        "float" | "double" => Type::Float,
        // Phase 14: the four accessory-refined string pseudo-types are
        // now real `Intersection([String, Accessory(_)])` types, mirroring
        // real PHPStan's own `IntersectionType([StringType,
        // AccessoryXType])` representation. `callable-string`/
        // `trait-string`/`interface-string`/`enum-string` stay aliased
        // to plain `string`: each names "a string believed to hold a
        // specific kind of identifier," with no literal-based or
        // narrowing-based way for this project to ever verify content -
        // `class-string`'s own optional `<Foo>` generic argument is
        // similarly just dropped rather than tracked. `lowercase-string`/
        // `uppercase-string`/`non-empty-lowercase-string`/`non-empty-
        // uppercase-string` join the same "can't verify content" group
        // for an identical reason (case-content isn't something this
        // project can prove from a type alone) - previously missing
        // entirely, falling through to the generic class-name branch and
        // producing bogus namespace-qualified "class" names (e.g.
        // `Psl\HPACK\non-empty-lowercase-string`) that then failed every
        // ordinary `string`-typed assignability check, a real corpus
        // false positive.
        "non-empty-string" => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NonEmptyString),
        ]),
        "non-falsy-string" | "truthy-string" => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NonFalsyString),
        ]),
        "literal-string" => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::LiteralString),
        ]),
        "numeric-string" => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NumericString),
        ]),
        "string"
        | "callable-string"
        | "trait-string"
        | "interface-string"
        | "enum-string"
        | "lowercase-string"
        | "uppercase-string"
        | "non-empty-lowercase-string"
        | "non-empty-uppercase-string" => Type::String,
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
        // Phase 80: `value-of<T>`/`key-of<T>` - real PHPStan's own
        // `ValueOfType`/`KeyOfType`, both `LateResolvableType`s evaluated
        // against `T`'s own resolved type only once actually needed. This
        // project has no general lazy/deferred type-resolution mechanism
        // (adding one would touch every exhaustive `match` over `Type`
        // across the codebase, for a rarely-used feature) - `T` itself
        // already parsing to a real `Array`/`ArrayShape` needs no registry
        // access at all and is resolved immediately below (real PHPStan's
        // own `key-of<T>` never special-cases an enum either, only
        // `value-of<T>` does, confirmed directly against `KeyOfType::
        // getResult()`/`ValueOfType::getResult()`).
        //
        // Phase 80 follow-up: `T` naming a bare class/enum reference (no
        // template args) is exactly real PHPStan's own `value-of<T>`-on-an-
        // enum special case (`key-of<T>` on a class/enum has no real
        // PHPStan special case at all - it just falls to `getIterableKeyType()`,
        // `Mixed` for an ordinary class, so it's left unchanged below) -
        // this one case genuinely needs registry access this parse-time
        // function doesn't have, so it's encoded as a sentinel
        // (`Type::Object("$$valueof", [Type::Object(name, [])])`) instead
        // of guessed at here, resolved once real class/enum data exists by
        // `Registry::resolve_deferred_value_of` (`registry.rs`) - the same
        // "encode a special case via an existing variant, guarded by an
        // impossible name" idiom `"resource"`/`"object"` above already
        // use (`$` can never appear in a real PHP identifier, so this can
        // never collide with a genuine class/enum/template-param/type-
        // alias name anywhere else in the type tree).
        "value-of" => match generic_args {
            [single] => match parse_type_string(single) {
                Type::Array(_, value) => *value,
                // The unsealed-shape sentinel (`try_parse_array_shape`'s
                // own doc comment) is never a real field - filtered out
                // before counting/collecting, the same way `Registry::
                // is_assignable`'s own `ArrayShape` arms already have to.
                Type::ArrayShape(fields) => {
                    let fields: Vec<_> = fields
                        .into_iter()
                        .filter(|(name, _, _)| name != "...")
                        .collect();
                    match fields.len() {
                        0 => Type::Never,
                        1 => fields.into_iter().next().unwrap().1,
                        _ => Type::Union(fields.into_iter().map(|(_, ty, _)| ty).collect()),
                    }
                }
                Type::Object(name, args) if args.is_empty() => {
                    Type::Object("$$valueof".to_string(), vec![Type::Object(name, args)])
                }
                _ => Type::Mixed,
            },
            _ => Type::Mixed,
        },
        "key-of" => match generic_args {
            [single] => match parse_type_string(single) {
                Type::Array(key, _) => *key,
                Type::ArrayShape(fields) => {
                    let fields: Vec<_> = fields
                        .into_iter()
                        .filter(|(name, _, _)| name != "...")
                        .collect();
                    match fields.len() {
                        0 => Type::Never,
                        1 => shape_key_literal(&fields[0].0),
                        _ => Type::Union(
                            fields
                                .iter()
                                .map(|(name, _, _)| shape_key_literal(name))
                                .collect(),
                        ),
                    }
                }
                _ => Type::Mixed,
            },
            _ => Type::Mixed,
        },
        "null" => Type::Null,
        "void" => Type::Void,
        "mixed" => Type::Mixed,
        "never" | "noreturn" => Type::Never,
        // Phase 50: `array<K, V>`'s key argument is now real
        // (`Type::Array`'s own key slot), not discarded. `array<V>`
        // (1-arg) and bare `array` still mean "key unknown" -
        // `Type::unknown_array_key()`. `class-string-map<TKey, TValue>`
        // (Doctrine's own refinement of `array<class-string<TKey>,
        // TValue>`) wraps its own key in `ClassString` to match.
        "array" | "class-string-map" => match generic_args.len() {
            0 => Type::array_of(Type::Mixed),
            1 => Type::array_of(parse_type_string(generic_args[0])),
            _ => {
                let key_ty = parse_type_string(generic_args[0]);
                let key_ty = if bare == "class-string-map" {
                    Type::ClassString(Box::new(key_ty))
                } else {
                    key_ty
                };
                Type::Array(
                    Box::new(key_ty),
                    Box::new(parse_type_string(generic_args[1])),
                )
            }
        },
        // Phase 14: `list`/`non-empty-array`/`non-empty-list` are now
        // real `Intersection([Array(_), Accessory(_)])` types, mirroring
        // real PHPStan's own `IntersectionType([ArrayType,
        // AccessoryArrayListType/NonEmptyArrayType])` representation -
        // `elem_ty` computed the same "0/1/2-arg generic" way `array`
        // itself already is above.
        "list" | "non-empty-array" | "non-empty-list" => {
            let elem_ty = match generic_args.len() {
                0 => Type::Mixed,
                1 => parse_type_string(generic_args[0]),
                _ => parse_type_string(generic_args[1]),
            };
            // Phase 50: `list`/`non-empty-list` genuinely can only ever
            // have sequential int keys - a real precision win over the
            // default `int|string`, not just arity-matching.
            //
            // Phase 74 corpus find: `non-empty-array` makes no such
            // *implicit* promise (non-emptiness alone says nothing about
            // key type) - but an *explicit* 2-arg `non-empty-array<K, V>`
            // still names a real `K`, exactly like plain `array<K, V>`
            // does, and previously always got thrown away here in favor
            // of the unknown default regardless of what was actually
            // written - `key()`/`array_key_first()`/`array_key_last()`'s
            // own new precision (this same phase) immediately surfaced
            // it by silently losing a docblock's own `non-empty-array<
            // string, int>` key type.
            let key_ty = if bare == "non-empty-array" {
                match generic_args.len() {
                    2 => parse_type_string(generic_args[0]),
                    _ => Type::unknown_array_key(),
                }
            } else {
                Type::Int
            };
            let mut members = vec![Type::Array(Box::new(key_ty), Box::new(elem_ty))];
            if bare != "non-empty-array" {
                members.push(Type::Accessory(AccessoryKind::ArrayList));
            }
            if bare != "list" {
                members.push(Type::Accessory(AccessoryKind::NonEmptyArray));
            }
            Type::Intersection(members)
        }
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
                Type::Array(Box::new(Type::unknown_array_key()), Box::new(element_ty)),
                Type::Object("Traversable".to_string(), Vec::new()),
            ])
        }
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
        // Preserves `original`'s own leading `\`, same reason this whole
        // function's own doc comment gives for the generic user-class arm
        // below: a bare, unqualified `Closure` is genuinely ambiguous (real
        // corpus shape: `rector/rector` consumers of `nikic/php-parser`
        // routinely `use PhpParser\Node\Expr\Closure;`, an entirely
        // different real class, then type-hint bare `Closure` meaning
        // *that* one) - only `\Closure`, root-anchored, unambiguously means
        // the real global class, and hardcoding the bare name here (losing
        // the backslash) used to make every native `\Closure $fn` type hint
        // in a namespaced file look identical to the ambiguous bare case,
        // then get silently mis-qualified into nonsense like `Amp\Closure`
        // once `NameContext::resolve` ran (a real, corpus-found bug: amphp/
        // amp's own `Cancellation::subscribe(\Closure $callback)`). A bare,
        // unqualified `Closure` instead falls through to the ordinary
        // class-reference resolution below, exactly like any other class
        // name - correctly deferring to a real `use` import when one
        // exists, or PHP's own genuine (if easy-to-typo) "resolves relative
        // to the current namespace" rule otherwise.
        "closure" if original.starts_with('\\') => {
            Type::Object("\\Closure".to_string(), Vec::new())
        }
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
        // nested generic, ...), so it's re-run through `parse_generic_arg`
        // (Phase 54: call-site-variance-aware; falls straight through to
        // `parse_type_string` - the same recursive-field pattern
        // `try_parse_array_shape` already uses - for an ordinary
        // argument). A bare (non-generic) reference just gets an empty
        // arg list, indistinguishable from "erased"/unspecified generics
        // on a class that does declare `@template` - `TemplateParam`
        // substitution treats an unbound parameter as `Mixed` either way.
        _ => Type::Object(
            original.to_string(),
            generic_args.iter().map(|a| parse_generic_arg(a)).collect(),
        ),
    }
}

/// Phase 54: real PHPStan's own call-site variance annotation syntax on
/// one generic type argument - `Collection<covariant Foo>`,
/// `Collection<contravariant Foo>`, or `Collection<*>` ("don't check
/// this argument at all"). Confirmed directly against `phpdoc-parser`'s
/// own `GenericTypeNode::VARIANCE_*` constants/`__toString()` and
/// `PhpDoc\TypeNodeResolver::resolveGenericTypeNode` in
/// `vendor/phpstan-src` - real, current PHPStan syntax, parsed on every
/// generic type position (`@param`/`@return`/`@var`/`@extends`/
/// `@implements` alike), not just a hypothetical. Encoded with the same
/// "sentinel via an existing `Type::Object` variant, guarded by a name
/// no real PHP identifier can ever collide with" idiom Phase 80 already
/// established for `value-of<T>`'s own `"$$valueof"` -
/// `"$$covariant"`/`"$$contravariant"` each wrap the real argument
/// type; `"$$bivariant"` (`*`) has no real argument to wrap. Consulted
/// by `Registry::effective_slot_variance` (`registry.rs`), which lets a
/// marker here override a class's own declared slot variance for one
/// comparison, exactly matching `GenericObjectType::isSuperTypeOf`'s/
/// `getReferencedTemplateTypes`'s own "call-site variance wins when
/// present" rule.
fn parse_generic_arg(a: &str) -> Type {
    let trimmed = a.trim();
    if trimmed == "*" {
        return Type::Object("$$bivariant".to_string(), Vec::new());
    }
    if let Some(rest) = trimmed.strip_prefix("covariant ") {
        return Type::Object("$$covariant".to_string(), vec![parse_type_string(rest)]);
    }
    if let Some(rest) = trimmed.strip_prefix("contravariant ") {
        return Type::Object("$$contravariant".to_string(), vec![parse_type_string(rest)]);
    }
    parse_type_string(trimmed)
}

/// Strips a `parse_generic_arg` call-site variance sentinel down to the
/// real type it wraps, for every consumer that only cares "what type is
/// really being referenced here," not the variance override itself
/// (existence-checking, bound-checking, substitution's own recursive
/// rebuild, ...). `Registry::effective_slot_variance`/`is_assignable`
/// are the two places that DO care about the marker itself and read it
/// directly rather than through this helper. `"$$bivariant"` (`*`) has
/// no real type to unwrap to at all - `None` here means "nothing to
/// verify," the same as this project already treats any other
/// unresolvable/erased generic argument elsewhere.
pub(crate) fn strip_call_site_variance(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Object(name, args) if name == "$$covariant" || name == "$$contravariant" => {
            Some(&args[0])
        }
        Type::Object(name, _) if name == "$$bivariant" => None,
        other => Some(other),
    }
}
