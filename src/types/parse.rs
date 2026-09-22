use std::sync::Arc;

use crate::types::{AccessoryKind, CallableParam, CallableSig, Type};

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
        // `Mixed` already includes every other possible value, so a
        // union with `Mixed` alongside anything else (real shape:
        // `end()`'s stub `@return mixed|false`) is simply `Mixed` -
        // `union_types` special-cases this at construction time, but
        // this lower-level parser builds a `Type::Union` directly from
        // doc text with no such normalization; a stray non-`Mixed`
        // member would otherwise fail an ordinary compatibility check
        // even though the whole union is vacuously satisfiable.
        if members.contains(&Type::Mixed(false)) {
            return Type::Mixed(false);
        }
        Type::Union(members)
    } else if amp_parts.len() > 1 {
        Type::Intersection(amp_parts.into_iter().map(parse_type_atom).collect())
    } else {
        parse_type_atom(s)
    };

    if nullable && ty != Type::Mixed(false) {
        Type::Union(vec![ty, Type::Null])
    } else {
        ty
    }
}

/// The byte index of the `)` matching the FIRST `(` in `s`
/// (`s` must start at or before that `(`), tracking bracket depth/quotes
/// the same way `split_top_level` does - no existing helper answers "an
/// opening bracket's own matching close" (`split_top_level` only ever
/// splits *on* a top-level separator).
fn find_matching_paren(s: &str) -> Option<usize> {
    let open = s.find('(')?;
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for (i, c) in s[open..].char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '<' | '{' | '(' | '[' => depth += 1,
            '>' | '}' | ')' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Real structural parsing of a `Closure(...)`/`callable(...)` PHPDoc
/// shape - `stripped` is `s` with any leading `\` already removed (case
/// preserved, unlike the `lower` prefix-matching gate at this function's
/// call site). `None` on anything not confidently understood (a
/// missing/unbalanced `(`, or the mandatory trailing `: ReturnType`
/// missing - real phpdoc-parser's grammar unconditionally consumes a
/// `:` token) - the caller falls back to the bare `Type::Callable(None)`
/// marker, so a shape this doesn't recognize can only ever lose
/// precision, never mis-parse.
fn try_parse_callable_shape(stripped: &str) -> Option<Type> {
    let close = find_matching_paren(stripped)?;
    let open = stripped.find('(')?;
    let inner = &stripped[open + 1..close];
    let rest = stripped[close + 1..].trim_start();
    let ret_text = rest.strip_prefix(':')?;

    let raw_params: Vec<&str> = if inner.trim().is_empty() {
        Vec::new()
    } else {
        split_top_level(inner, ',')
    };
    let mut params = Vec::with_capacity(raw_params.len());
    for raw in raw_params {
        let mut text = raw.trim();
        let optional = text.strip_suffix('=').is_some_and(|t| {
            text = t.trim_end();
            true
        });
        // A trailing `$name` token (optional per the grammar, and this
        // struct never tracks it - discarded, not stored).
        if let Some(dollar) = text.rfind('$')
            && text[dollar..]
                .chars()
                .skip(1)
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            text = text[..dollar].trim_end();
        }
        let variadic = text.strip_suffix("...").is_some_and(|t| {
            text = t.trim_end();
            true
        });
        let by_ref = text.strip_suffix('&').is_some_and(|t| {
            text = t.trim_end();
            true
        });
        if text.is_empty() {
            return None;
        }
        params.push(CallableParam {
            ty: parse_type_string(text),
            variadic,
            optional,
            by_ref,
        });
    }
    Some(Type::Callable(Some(Arc::new(CallableSig {
        params,
        return_type: Box::new(parse_type_string(ret_text)),
    }))))
}

/// Splits `s` on `sep`, but only where `sep` appears outside any
/// `<>`/`{}`/`()`/`[]` nesting. Always returns at least one part, mirroring
/// `str::split` - callers check `.len() > 1` to know whether a real
/// top-level separator was found at all.
///
/// A `'...'`/`"..."` string-literal segment is tracked as one opaque
/// run, since a bracket-shaped character *inside* the quotes (real
/// shape: PHPUnit's own literal-string union `'->'|'::' `) would
/// otherwise be read as a real closing bracket, permanently
/// decrementing `depth` below zero and swallowing every top-level
/// separator for the rest of the string.
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
/// binding `$param`'s actual argument; this project has no per-call-site
/// type-expression evaluation, so it must not let the DNF-group unwrap
/// above this function's caller mis-parse the whole expression as an
/// ordinary intersection group (mangling `$param`/`is`/etc. into
/// namespace-qualified garbage). Takes the safe answer instead: the
/// union of both branches. Requires the literal `is`/`?`/`:` tokens at
/// the top level (`split_top_level`'s bracket-depth tracking) so this
/// never misfires on an ordinary DNF group, which has no ternary syntax.
fn try_parse_conditional_type(s: &str) -> Option<Type> {
    let q_parts = split_top_level(s, '?');
    let [cond, branches] = q_parts.as_slice() else {
        return None;
    };
    // `" is not "` must be tried before `" is "` - it's a strict superset
    // textually (every `" is not "` occurrence also contains `" is "`),
    // so checking the shorter needle first would always win and silently
    // drop the negation.
    let (subject_raw, target_raw, negated) = if let Some(idx) = cond.find(" is not ") {
        (&cond[..idx], &cond[idx + " is not ".len()..], true)
    } else {
        let idx = cond.find(" is ")?;
        (&cond[..idx], &cond[idx + " is ".len()..], false)
    };
    let c_parts = split_top_level(branches, ':');
    let [true_branch, false_branch] = c_parts.as_slice() else {
        return None;
    };
    let if_true = parse_type_string(true_branch);
    let if_false = parse_type_string(false_branch);
    // Only a bare `$param` subject is modeled (real PHPStan also allows a
    // non-parameter type-expression subject, e.g. a template name) -
    // fall back to the eager union for that unmodeled shape.
    let Some(subject) = subject_raw.trim().strip_prefix('$') else {
        return Some(if if_true == if_false {
            if_true
        } else {
            Type::Union(vec![if_true, if_false])
        });
    };
    Some(Type::Conditional {
        subject: subject.to_string(),
        target: Box::new(parse_type_string(target_raw)),
        if_true: Box::new(if_true),
        if_false: Box::new(if_false),
        negated,
    })
}

/// `try_parse_array_shape`'s own field accumulator - overwrites an
/// already-present field with the same key in place (last-one-wins),
/// matching real PHPStan's own
/// `ConstantArrayTypeBuilder::setOffsetValueType` (a later call for an
/// already-seen key always replaces it, never duplicates it). Reachable
/// via an explicit `0:` field colliding with an earlier *positional*
/// field's own auto-numbered index (`array{int, 0: string}`), or a
/// literal duplicate key (`array{1: int, 1: string}`).
fn push_shape_field(fields: &mut Vec<(String, Type, bool)>, key: String, ty: Type, optional: bool) {
    match fields.iter_mut().find(|(k, _, _)| *k == key) {
        Some(field) => *field = (key, ty, optional),
        None => fields.push((key, ty, optional)),
    }
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
        // collide with any real key this grammar could parse) rather
        // than tracking the unsealed portion's value type precisely
        // (`Type::Mixed(false)`, matching a plain `array`'s unknown-value-type
        // default) - see `is_unsealed_shape`/
        // `narrowing::narrow_array_shape_by_key_presence` for how this
        // gets consumed.
        if field_str.starts_with("...") {
            fields.push(("...".to_string(), Type::Mixed(false), false));
            continue;
        }
        // A *positional* (unkeyed) field (`array{int, string}`, real
        // PHPStan/Psalm tuple syntax) has no `key:` prefix - `split_once
        // (':')` alone can't tell that apart from a keyed field whose
        // type happens to contain a colon (`Closure(int): string`), so
        // `looks_like_shape_key` also checks that whatever precedes the
        // candidate colon is actually shaped like a key. Numbers a
        // positional field the same way `infer_array_literal_type`
        // numbers a real array literal's positional elements, so a
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
                // `looks_like_shape_key` above accepts a
                // `'...'`/`"..."`-quoted key (real shape: `array{
                // 'content-type': string}`) - the surrounding quotes
                // must be stripped here, or the field ends up keyed by
                // the *literal* quoted text instead of the real string
                // PHP would use as the array key.
                let key = strip_shape_key_quotes(key);
                if let Some(n) = canonical_int_key(key) {
                    next_index = next_index.max(n + 1);
                }
                push_shape_field(
                    &mut fields,
                    key.to_string(),
                    parse_type_string(type_part.trim()),
                    optional,
                );
            }
            None => {
                let key = next_index.to_string();
                next_index += 1;
                push_shape_field(&mut fields, key, parse_type_string(field_str), false);
            }
        }
    }
    Some(Type::ArrayShape(fields))
}

/// `object{name: string, age?: int}` - PHPDoc's structural object shape.
/// Mirrors `try_parse_array_shape` (same field grammar, same per-field
/// `parse_type_string` recursion), minus the positional-field/`...`-
/// unsealed-marker concept (an object's properties are always named).
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
            // An unrecognized field shouldn't abort every other real
            // field alongside it.
            continue;
        };
        let key_part = key_part.trim();
        let (key, optional) = match key_part.strip_suffix('?') {
            Some(k) => (k.trim(), true),
            None => (key_part, false),
        };
        let key = strip_shape_key_quotes(key);
        fields.push((
            key.to_string(),
            parse_type_string(type_part.trim()),
            optional,
        ));
    }
    Some(Type::ObjectShape(fields))
}

/// The inverse of `strip_shape_key_quotes`, for display: a shape key
/// shown bare only when it's exactly the shape `looks_like_shape_key`
/// accepts unquoted (a bare integer, or an identifier-shaped token) -
/// anything else is single-quoted, so a diagnostic message displaying
/// this type stays round-trippable back through this same parser rather
/// than showing a key text that would parse differently (or not at all)
/// if pasted back into a real `@var`/`@param` tag.
pub(crate) fn display_shape_key(key: &str) -> String {
    let is_bare_integer = canonical_int_key(key).is_some();
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

/// Strips one layer of matching `'...'`/`"..."` quoting from a shape
/// field key, if present - `looks_like_shape_key` accepts a quoted key
/// as a way to write one that isn't a valid bare identifier
/// (`array{'content-type': string}`), but the field itself must be
/// stored under the *unquoted* text (what a real `$arr['content-type']`
/// access's own literal key actually is), or every real lookup against
/// it fails to match. A bare (unquoted) key passes through unchanged.
fn strip_shape_key_quotes(key: &str) -> &str {
    for quote in ['\'', '"'] {
        if let Some(inner) = key.strip_prefix(quote).and_then(|k| k.strip_suffix(quote)) {
            return inner;
        }
    }
    key
}

/// Real PHPStan's own `OffsetAccessType::getResult()`: extracts `key`'s
/// value type out of `base`. Only the shapes that come up in practice
/// are resolved (`ArrayShape`/`ObjectShape`'s per-field lookup, a plain
/// `Array`'s uniform value type, `Union` recursing member-wise);
/// anything else falls back to `Mixed`.
fn offset_access_type(base: &Type, key: &str) -> Type {
    match base {
        Type::ArrayShape(fields) | Type::ObjectShape(fields) => fields
            .iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, v, _)| v.clone())
            .unwrap_or(Type::Mixed(false)),
        Type::Array(_, value_ty) => (**value_ty).clone(),
        Type::Union(members) => {
            let resolved: Vec<Type> = members.iter().map(|m| offset_access_type(m, key)).collect();
            match resolved.len() {
                0 => Type::Mixed(false),
                1 => resolved.into_iter().next().unwrap(),
                _ => Type::Union(resolved),
            }
        }
        _ => Type::Mixed(false),
    }
}

/// `key-of<array{foo: string, 5: int}>`'s own per-field answer - a
/// shape's field name is always stored as a plain `String` (see
/// `ArrayShape`'s doc comment), so a field name that parses as an
/// integer names an int key, matching PHP's own key-canonicalization
/// rule.
fn shape_key_literal(name: &str) -> Type {
    match canonical_int_key(name) {
        Some(n) => Type::IntLiteral(n),
        None => Type::StringLiteral(name.to_string()),
    }
}

/// Real PHP's own array-key canonicalization rule for a string key,
/// mirroring `ConstantStringType::toArrayKey()`: `Some(n)` exactly when
/// `key` is a "canonical decimal integer" string - an optional single
/// leading `-`, no leading zero unless the whole value is literally
/// `"0"`, `"-0"` itself excluded (`(string)(int)"-0"` is `"0"`, not
/// `"-0"`), and the value fits in `i64`. `None` otherwise (stays a
/// string key). Plain `str::parse::<i64>()` is **not** sufficient: it
/// happily accepts a leading zero (`"01".parse::<i64>() == Ok(1)`) and
/// `"-0"`, both of which real PHP does *not* canonicalize.
pub(crate) fn canonical_int_key(key: &str) -> Option<i64> {
    let (negative, digits) = match key.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, key),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if digits != "0" && digits.starts_with('0') {
        return None;
    }
    if negative && digits == "0" {
        return None;
    }
    let value: i64 = digits.parse().ok()?;
    Some(if negative { -value } else { value })
}

/// Whether `key_part` (the text before a candidate `:` inside an
/// `array{...}` field) actually looks like a shape key - a bare
/// identifier (hyphen included: gettext's own `plural-forms`-style
/// dictionary-key convention), an integer, or a quoted string,
/// optionally `?`-suffixed - as opposed to being part of a *positional*
/// field's own type text that merely happens to contain a colon (a
/// callable-shape return type, `Closure(int): string`).
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
    // value's signature, not a generic type-argument list (only `<...>`
    // is one) or a real class name, despite superficially resembling
    // one (real shape: Doctrine DBAL's own
    // `@param Closure(self):T $func`, which would otherwise get mangled
    // into a bogus namespace-qualified "class name"). Stripped of a
    // leading `\` first, since real corpus code commonly spells this
    // fully qualified (amphp/amp's own convention throughout).
    let stripped = s.trim_start_matches('\\');
    let lower = stripped.to_ascii_lowercase();
    if lower.starts_with("closure(")
        || lower.starts_with("closure (")
        || lower.starts_with("callable(")
        || lower.starts_with("callable (")
    {
        return try_parse_callable_shape(stripped).unwrap_or(Type::Callable(None));
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

    // Same idea as the quoted-string case just above: a bare integer
    // literal as one member of a union (`1|2|3`, constraining a value to
    // a fixed set - log levels, status codes, mode flags, ...) means
    // "exactly this integer value," not a class reference.
    if let Ok(n) = s.parse::<i64>() {
        return Type::IntLiteral(n);
    }
    // Mirrors the int-literal case just above, for a real float
    // literal (`1.5|2.5`) - checked after the int case so an
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

    // `try_parse_array_shape`/`try_parse_object_shape` are only tried
    // once, at the top of this function, against the *original*,
    // un-stripped `s` - a shape wrapped in the `[]`-array-shorthand
    // suffix just stripped above (`array{...}[]`) doesn't end with the
    // shape's own closing `}` at that point, so it needs re-attempting
    // against the *reduced* string here too (real shape: nikic/php-parser's
    // own `@param array{int, string, string}[] $patches`).
    if array_depth > 0
        && let Some(shape) = try_parse_array_shape(s).or_else(|| try_parse_object_shape(s))
    {
        let mut ty = shape;
        for _ in 0..array_depth {
            ty = Type::Array(Box::new(Type::benevolent_array_key()), Box::new(ty));
        }
        return ty;
    }

    // PHPDoc/PHPStan's own `OffsetAccessType` syntax - `SomeShape['key']`
    // extracts one key's value type out of whatever `SomeShape` resolves
    // to. Checked *after* the `[]`-array-shorthand loop above (which
    // only strips a genuinely *empty* bracket), so `SomeShape['key'][]`
    // still parses correctly. Not specially recursive for a chained
    // `Foo['a']['b']`: `s.rfind('[')` finds the *last* bracket pair
    // first, leaving `base_part` as `"Foo['a']"` - recursing into
    // `parse_type_string` resolves the inner `['a']` on its own next
    // call, one bracket peeled per level.
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
                ty = Type::Array(Box::new(Type::benevolent_array_key()), Box::new(ty));
            }
            return ty;
        }
    }

    // `(A|B)[]`/`(A&B)[]` - a parenthesized union/intersection wrapped
    // in array-shorthand syntax, needed whenever the element type isn't
    // a single bare name (`@param (string|int)[] $keys` - PHPStan's own
    // grammar requires the parens here since `A|B[]` alone would
    // ambiguously read as `A|(B[])`). Recurses into `parse_type_string`,
    // not `parse_type_atom`, since the inner content can have its own
    // top-level union/intersection needing `split_top_level`'s
    // handling, which only the caller owns.
    if array_depth > 0
        && let Some(inner) = s.strip_prefix('(').and_then(|s| s.strip_suffix(')'))
    {
        let mut ty = parse_type_string(inner);
        for _ in 0..array_depth {
            ty = Type::Array(Box::new(Type::benevolent_array_key()), Box::new(ty));
        }
        return ty;
    }

    let (base, generic_args) = split_generic_args(s);
    let original = base.trim();
    let bare = original.trim_start_matches('\\');
    let mut ty = if bare.contains("::") {
        // A class-constant-reference type (`self::RFC_*`, `Foo::BAR`,
        // PHPStan's own wildcard-constant convention) isn't a class name,
        // even though it's shaped like one syntactically - resolving it
        // for real would mean looking up the named constant on the
        // referenced class, which this project doesn't model. Left as
        // `Mixed` rather than falling through to the generic-class
        // branch below, which would otherwise namespace-qualify it as a
        // literal class name (real shape: monolog/monolog's own
        // `@param self::RFC_* $rfc`).
        Type::Mixed(false)
    } else {
        parse_type_keyword_or_class(bare, original, &generic_args)
    };
    for _ in 0..array_depth {
        ty = Type::Array(Box::new(Type::benevolent_array_key()), Box::new(ty));
    }
    ty
}

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

/// `bare` (backslash-stripped) drives keyword matching; `original`
/// (backslash-preserved) is what the class-name fallback arm constructs
/// its `Type::Object` from - a leading `\` means "already fully
/// qualified from the global namespace," which
/// `registry::NameContext::resolve` needs to see later (its first check
/// is exactly `strip_prefix('\\')`) to know not to re-qualify it against
/// the current file's namespace. Stripping the backslash before it
/// reaches `resolve` would make every absolute native type hint
/// containing a `\` (e.g. `QueryBuilder|\Doctrine\ORM\QueryBuilder`)
/// look identical to a relative one, mis-qualifying it as
/// `CurrentNamespace\Doctrine\ORM\QueryBuilder`.
fn parse_type_keyword_or_class(bare: &str, original: &str, generic_args: &[&str]) -> Type {
    match bare.to_ascii_lowercase().as_str() {
        // `int<min, max>` and its named shorthands are real ranges,
        // not aliased to plain `Int`. `int`/`integer` also read
        // `generic_args` when present (`int<0, 255>`) - a malformed
        // bound falls back to plain `Type::Int` rather than guessing.
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
        // expands these into a union of every combinable bitmask value,
        // which this project can't enumerate without resolving the
        // referenced class constants (same boundary `value-of`/`key-of`
        // below hit). Degrades to plain `Type::Int` (the pseudo-type's
        // real underlying kind) rather than falling through to the
        // generic class-name branch, which would produce a bogus
        // namespace-qualified "class" that fails every ordinary
        // `int`-typed assignability check.
        "int-mask" | "int-mask-of" => Type::Int,
        "float" | "double" => Type::Float,
        // The accessory-refined string pseudo-types are real
        // `Intersection([String, Accessory(_)])` types, mirroring real
        // PHPStan's own `IntersectionType([StringType, AccessoryXType])`.
        // `callable-string`/`trait-string`/`interface-string`/
        // `enum-string` stay aliased to plain `string`: each names "a
        // string believed to hold a specific kind of identifier," with
        // no way for this project to verify content.
        //
        // `lowercase-string`/`uppercase-string`/`non-empty-
        // lowercase-string`/`non-empty-uppercase-string` are unlike
        // those: case-content genuinely *is* provable, via
        // `types::subtype`'s `LowercaseString`/`UppercaseString`
        // literal-fact arms and the `strtolower`/`strtoupper`/etc.
        // case-tracking producers (`checker/expr/builtin_calls.rs`).
        "lowercase-string" => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::LowercaseString),
        ]),
        "uppercase-string" => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::UppercaseString),
        ]),
        "non-empty-lowercase-string" => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NonEmptyString),
            Type::Accessory(AccessoryKind::LowercaseString),
        ]),
        "non-empty-uppercase-string" => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NonEmptyString),
            Type::Accessory(AccessoryKind::UppercaseString),
        ]),
        // `decimal-int-string` mirrors real PHPStan's own
        // `IntersectionType([StringType, AccessoryDecimalIntegerStringType])`.
        // The `inverse` variant (`non-decimal-int-string`) stays
        // unaliased (falls through to the generic-class-name branch)
        // since it needs a wholly separate dynamic-array-key-narrowing
        // consumer this project doesn't build - aliasing it to a kind
        // with no real consumer would be a silent, false claim of
        // support.
        "decimal-int-string" => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::DecimalIntegerString),
        ]),
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
        "string" => Type::String,
        // `callable-string` mirrors real PHPStan's own resolution
        // (`IntersectionType([StringType, CallableType])`) -
        // architecturally unrelated to the `class-string` family below
        // despite the shared "-string" name.
        "callable-string" => Type::Intersection(vec![Type::String, Type::Callable(None)]),
        // `enum-string`/`enum-string<T>` mirror real PHPStan's own
        // `GenericClassStringType(UnitEnum)`/`GenericClassStringType(T &
        // UnitEnum)` (`TypeNodeResolver.php` - `GenericClassStringType
        // extends ClassStringType`, i.e. `enum-string` really is
        // `class-string<UnitEnum>` under real PHPStan's own hood too).
        // Every enum this project's own registry sees already
        // synthesizes `implements UnitEnum` at declaration time
        // (`registry/class_info.rs`), so this gets real narrowing/
        // assignability for free through the ordinary Object-vs-Object
        // class-hierarchy walk in `registry::is_assignable` - no new
        // type-system machinery needed.
        "enum-string" => {
            let unit_enum = Type::Object("UnitEnum".to_string(), Vec::new());
            match generic_args {
                [single] => Type::ClassString(Box::new(Type::Intersection(vec![
                    parse_type_string(single),
                    unit_enum,
                ]))),
                _ => Type::ClassString(Box::new(unit_enum)),
            }
        }
        // `interface-string`/`trait-string` (bare form): real PHPStan
        // gives these no distinguishing semantics from bare
        // `class-string`, but this project checks more strictly.
        // `ClassInfo` already tracks the distinction needed
        // (`ClassKind::Interface`/`ClassKind::Trait`), so these route
        // through `Type::ClassString` bound to an internal `$$`-prefixed
        // sentinel marker (same convention as `$$valueof`/`$$covariant`/
        // etc.) - `registry::is_assignable`'s `ClassString` arm
        // special-cases these two sentinel names to check the
        // candidate's real `ClassKind` instead of walking class
        // hierarchy. No generic `<T>` form: real PHPStan has none for
        // `trait-string`, and `interface-string<T>` has no real corpus
        // evidence calling for it.
        "interface-string" => Type::ClassString(Box::new(Type::Object(
            "$$interface".to_string(),
            Vec::new(),
        ))),
        "trait-string" => {
            Type::ClassString(Box::new(Type::Object("$$trait".to_string(), Vec::new())))
        }
        // Unlike its string-refinement neighbors above, `class-string<T>`
        // carries a real, template-bindable inner type when given one -
        // needed for `@template R of Base` / `@param class-string<R> $x`
        // shapes to bind `R` from a real `Foo::class` argument. A bare
        // `class-string` has nothing to preserve and stays plain
        // `String`.
        "class-string" => match generic_args {
            [] => Type::String,
            [single] => Type::ClassString(Box::new(parse_type_string(single))),
            _ => Type::String,
        },
        "bool" | "boolean" => Type::Bool,
        "true" => Type::True,
        "false" => Type::False,
        // Real, documented PHPStan/Psalm pseudo-types. `numeric` is an
        // honest approximation of real PHPStan's own
        // `int|float|numeric-string`: this project doesn't model
        // numeric-string as a distinct literal-set from plain `string`,
        // so `string` is the closest fit.
        "array-key" => Type::Union(vec![Type::Int, Type::String]),
        "scalar" => Type::Union(vec![Type::Int, Type::Float, Type::String, Type::Bool]),
        "numeric" => Type::Union(vec![Type::Int, Type::Float, Type::String]),
        // A real, if untyped-at-the-language-level, PHP value kind
        // (file/curl/... handles) - represented the same way the native
        // `object` hint is (a sentinel `Object`, not a real class), with
        // the same namespace-qualification exemption below: "resource"
        // isn't a real class reference to resolve per-file.
        "resource" => Type::Object("resource".to_string(), Vec::new()),
        // `value-of<T>`/`key-of<T>` - real PHPStan's own
        // `ValueOfType`/`KeyOfType`, both evaluated against `T`'s
        // resolved type only once needed. This project has no general
        // lazy/deferred type-resolution mechanism, so `T` parsing to a
        // real `Array`/`ArrayShape` (needing no registry access) is
        // resolved immediately below; `T` naming a bare class/enum
        // reference is real PHPStan's own `value-of<T>`-on-an-enum
        // special case (`key-of<T>` has no such case) and genuinely
        // needs registry access this parse-time function doesn't have,
        // so it's encoded as a sentinel
        // (`Type::Object("$$valueof", [Type::Object(name, [])])`),
        // resolved later by `Registry::resolve_deferred_value_of` - the
        // same "impossible name" idiom `"resource"`/`"object"` use
        // (`$` can never appear in a real PHP identifier).
        "value-of" => match generic_args {
            [single] => match parse_type_string(single) {
                Type::Array(_, value) => *value,
                // The unsealed-shape sentinel is never a real field -
                // filtered out before counting/collecting, the same way
                // `registry::is_assignable`'s `ArrayShape` arms do.
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
                _ => Type::Mixed(false),
            },
            _ => Type::Mixed(false),
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
                _ => Type::Mixed(false),
            },
            _ => Type::Mixed(false),
        },
        "null" => Type::Null,
        "void" => Type::Void,
        // The one genuine source of explicit `mixed` (Phase 84's own
        // `Type::Mixed`'s doc comment, `types.rs`) - the literal keyword
        // was written somewhere in this type expression, real PHPStan's
        // own `ParserNodeTypeToPHPStanType`/`TypeNodeResolver` both
        // resolving it to `MixedType(true)` too (confirmed directly
        // against `vendor/phpstan-src`).
        "mixed" => Type::Mixed(true),
        "never" | "noreturn" => Type::Never,
        // `array<K, V>`'s key argument is real
        // (`Type::Array`'s own key slot), not discarded. Bare `array`
        // still means "key unknown" - `Type::unknown_array_key()`.
        // `array<V>` (1-arg) is different: real PHPStan's own
        // `TypeNodeResolver::resolveGenericTypeNode` constructs a real
        // `BenevolentUnionType` key for exactly this one shape
        // (confirmed reading it directly - the *same* construction its
        // sibling `resolveArrayTypeNode` uses for the `V[]`-shorthand
        // suffix, `Type::benevolent_array_key()`'s own doc comment).
        // Scoped to `bare == "array"` only - `class-string-map<TKey,
        // TValue>` (Doctrine's own refinement of `array<class-string
        // <TKey>, TValue>`, this project's own extension with no real
        // PHPStan precedent to check a 1-arg form against) keeps the
        // plain, non-benevolent default it always had.
        "array" | "class-string-map" => match generic_args.len() {
            0 => Type::array_of(Type::Mixed(false)),
            1 if bare == "array" => Type::Array(
                Box::new(Type::benevolent_array_key()),
                Box::new(parse_type_string(generic_args[0])),
            ),
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
        // `list`/`non-empty-array`/`non-empty-list` are real
        // `Intersection([Array(_), Accessory(_)])` types, mirroring real
        // PHPStan's `IntersectionType([ArrayType,
        // AccessoryArrayListType/NonEmptyArrayType])`.
        "list" | "non-empty-array" | "non-empty-list" => {
            let elem_ty = match generic_args.len() {
                0 => Type::Mixed(false),
                1 => parse_type_string(generic_args[0]),
                _ => parse_type_string(generic_args[1]),
            };
            // `list`/`non-empty-list` genuinely can only ever have
            // sequential int keys - a real precision win over the
            // default `int|string`. `non-empty-array` makes no such
            // *implicit* promise, but an *explicit* 2-arg
            // `non-empty-array<K, V>` still names a real `K`, exactly
            // like plain `array<K, V>` does, and must not be thrown away
            // in favor of the unknown default.
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
        // Unlike its neighbors above, `iterable` is real PHP's own
        // native type hint, defined as exactly `array|Traversable` - a
        // broader type than `array`, not a narrower refinement of it.
        // Parsing it as bare `array` would reject any real
        // Generator/Iterator/IteratorAggregate passed to an
        // `iterable`-hinted parameter.
        "iterable" => {
            let element_ty = match generic_args.len() {
                0 => Type::Mixed(false),
                1 => parse_type_string(generic_args[0]),
                _ => parse_type_string(generic_args[1]),
            };
            Type::Union(vec![
                Type::Array(Box::new(Type::unknown_array_key()), Box::new(element_ty)),
                Type::Object("Traversable".to_string(), Vec::new()),
            ])
        }
        // `Closure` is a concrete class only a real closure object
        // satisfies, strictly narrower than the four shapes `callable`
        // accepts (see `is_assignable`'s `Type::Callable` arms) -
        // represented as a real `Object` so `is_assignable`'s ordinary
        // class-hierarchy check handles it.
        //
        // Preserves `original`'s leading `\`: a bare, unqualified
        // `Closure` is genuinely ambiguous (real shape: `rector/rector`
        // consumers of `nikic/php-parser` routinely
        // `use PhpParser\Node\Expr\Closure;`, then type-hint bare
        // `Closure` meaning *that* one) - only `\Closure`, root-anchored,
        // unambiguously means the real global class. Losing the
        // backslash here would make every native `\Closure $fn` hint in
        // a namespaced file look identical to the ambiguous bare case
        // and get mis-qualified once `NameContext::resolve` runs (real
        // shape: amphp/amp's own
        // `Cancellation::subscribe(\Closure $callback)`). A bare,
        // unqualified `Closure` falls through to the ordinary
        // class-reference resolution below instead, deferring to a real
        // `use` import when one exists.
        "closure" if original.starts_with('\\') => {
            Type::Object("\\Closure".to_string(), Vec::new())
        }
        "callable" => Type::Callable(None),
        "self" => Type::SelfRef,
        "parent" => Type::ParentRef,
        // `@return $this` is a real, PHPStan-documented PHPDoc
        // convention, identical in meaning to `@return static` - not a
        // variable reference despite the `$` sigil (real shape: amphp/amp's
        // `@return $this` on several fluent methods).
        "static" | "$this" => Type::StaticRef,
        "object" => Type::Object("object".to_string(), Vec::new()),
        // A user-defined generic class (`Collection<User>`): each type
        // argument is a full type string, re-run through
        // `parse_generic_arg` (call-site-variance-aware; falls straight
        // through to `parse_type_string` for an ordinary argument). A
        // bare (non-generic) reference gets an empty arg list,
        // indistinguishable from "erased" generics - `TemplateParam`
        // substitution treats an unbound parameter as `Mixed` either
        // way.
        _ => Type::Object(
            original.to_string(),
            generic_args.iter().map(|a| parse_generic_arg(a)).collect(),
        ),
    }
}

/// Real PHPStan's own call-site variance annotation syntax on one
/// generic type argument - `Collection<covariant Foo>`,
/// `Collection<contravariant Foo>`, or `Collection<*>` ("don't check
/// this argument at all"), parsed on every generic type position
/// (`@param`/`@return`/`@var`/`@extends`/`@implements` alike). Encoded
/// with the same "sentinel via an existing `Type::Object` variant,
/// guarded by an impossible name" idiom `value-of<T>` uses for
/// `"$$valueof"` - `"$$covariant"`/`"$$contravariant"` each wrap the
/// real argument type; `"$$bivariant"` (`*`) has no real argument to
/// wrap. Consulted by `Registry::effective_slot_variance`, which lets a
/// marker here override a class's declared slot variance for one
/// comparison, matching `GenericObjectType::isSuperTypeOf`'s "call-site
/// variance wins when
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
