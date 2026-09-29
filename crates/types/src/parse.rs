use std::sync::Arc;

use crate::{
    AccessoryKind, CallSiteVariance, CallableParam, CallableSig, ClassKind, ConditionalSubject,
    Repr, Shape, ShapeField, ShapeKey, Type, late,
};

/// Parses one of our parser's raw type-hint strings (`"?string"`,
/// `"int|string"`, `"\App\Foo"`, `"Countable&Iterator"`, or a PHPDoc type
/// like `"array<int, User>"` / `"array{name: string, age?: int}"`) into a
/// `Type`. Does not handle mixed union+intersection (DNF types) or
/// parentheses - PHPStan's own stub/native syntax rarely needs them outside
/// advanced generic bounds.
pub fn parse_type_string(raw: &str) -> Type {
    syntax::stack::guard(|| {
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
            let mut members: Vec<Type> = pipe_parts.into_iter().map(parse_type_atom).collect();
            // `Mixed` already includes every other possible value, so a
            // union with `Mixed` alongside anything else (real shape:
            // `end()`'s stub `@return mixed|false`) is simply `Mixed` -
            // `Type::union` special-cases this at construction time, but
            // this lower-level parser builds a `Repr::Union` directly from
            // doc text with no such normalization; a stray non-`Mixed`
            // member would otherwise fail an ordinary compatibility check
            // even though the whole union is vacuously satisfiable.
            if let Some(explicit) = members
                .iter()
                .filter_map(|m| match m {
                    Type(Repr::Mixed(explicit)) => Some(*explicit),
                    _ => None,
                })
                .reduce(|a, b| a || b)
            {
                return Type::mixed_with(explicit);
            }
            crate::union::merge_int_ranges(&mut members);
            if members.len() == 1 {
                members.pop().expect("one member")
            } else {
                Type(Repr::Union(members))
            }
        } else if amp_parts.len() > 1 {
            crate::intersect_types(amp_parts.into_iter().map(parse_type_atom).collect())
        } else {
            parse_type_atom(s)
        };

        if nullable && ty != Type::mixed() {
            Type(Repr::Union(vec![ty, Type::null()]))
        } else {
            ty
        }
    })
}

/// Real structural parsing of a `Closure(...)`/`callable(...)` PHPDoc
/// shape - `stripped` is `s` with any leading `\` already removed (case
/// preserved, unlike the `lower` prefix-matching gate at this function's
/// call site). `None` on anything not confidently understood (a
/// missing/unbalanced `(`, or the mandatory trailing `: ReturnType`
/// missing - real phpdoc-parser's grammar unconditionally consumes a
/// `:` token) - the caller falls back to the bare `Repr::Callable(None)`
/// marker, so a shape this doesn't recognize can only ever lose
/// precision, never mis-parse.
fn try_parse_callable_shape(stripped: &str) -> Option<Type> {
    let head_end = stripped.find(['(', '<'])?;
    let keyword = stripped[..head_end].trim().to_ascii_lowercase();
    let mut templates = Vec::new();
    let mut open = head_end;
    if stripped[head_end..].starts_with('<') {
        let close = matching_close(stripped, head_end)?;
        for raw in split_top_level(&stripped[head_end + 1..close], ',') {
            if !raw.trim().is_empty() {
                templates.push(parse_callable_template(raw.trim())?);
            }
        }
        let after = &stripped[close + 1..];
        let trimmed = after.trim_start();
        if !trimmed.starts_with('(') {
            return None;
        }
        open = close + 1 + (after.len() - trimmed.len());
    }
    let close = matching_close(stripped, open)?;
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
    let sig = CallableSig {
        params,
        return_type: Box::new(parse_type_string(ret_text)),
        templates,
    };
    let sig = mark_callable_templates(sig);
    // `Closure(...)` is a real `Closure` instance with that signature
    // (PHPStan's `ClosureType`), stricter than `callable(...)`, which a
    // string or invokable object satisfies too.
    if matches!(keyword.as_str(), "closure" | "pure-closure") {
        Some(Type::closure_literal(sig))
    } else {
        Some(Type::callable(Some(Arc::new(sig))))
    }
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
/// The index of the bracket closing the one at `open`.
fn matching_close(s: &str, open: usize) -> Option<usize> {
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

/// One `T [of|as Bound] [super Lower] [= Default]` entry of a callable's
/// template list; the lower bound is read and dropped, as PHPStan does
/// for callables.
fn parse_callable_template(raw: &str) -> Option<crate::TemplateParam> {
    let name_end = raw
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '\\'))
        .unwrap_or(raw.len());
    let name = &raw[..name_end];
    if name.is_empty() {
        return None;
    }
    let mut rest = raw[name_end..].trim();
    let mut default = None;
    if let Some(eq) = top_level_find(rest, "=") {
        default = Some(Box::new(parse_type_string(rest[eq + 1..].trim())));
        rest = rest[..eq].trim();
    }
    if rest.starts_with("super ") {
        rest = "";
    } else if let Some(sup) = top_level_find(rest, " super ") {
        rest = rest[..sup].trim();
    }
    let bound = match rest
        .strip_prefix("of ")
        .or_else(|| rest.strip_prefix("as "))
    {
        Some(b) => parse_type_string(b.trim()),
        None if rest.is_empty() => Type::mixed(),
        None => return None,
    };
    Some(crate::TemplateParam {
        name: name.to_string(),
        bound: Box::new(bound),
        variance: crate::Variance::Invariant,
        scope: Some(crate::CALLABLE_TEMPLATE_SCOPE.to_string()),
        default,
    })
}

/// The byte index of `pat` in `s` outside any bracket or quote.
fn top_level_find(s: &str, pat: &str) -> Option<usize> {
    let mut depth = 0i32;
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
            _ if depth == 0 && s[i..].starts_with(pat) => return Some(i),
            _ => {}
        }
    }
    None
}

/// Turns every bare reference to one of `sig`'s own templates into its
/// `TemplateParam`.
fn mark_callable_templates(sig: CallableSig) -> CallableSig {
    fn mark(ty: &Type, templates: &[crate::TemplateParam]) -> Type {
        match ty {
            Type(Repr::Object(name, args)) if args.is_empty() => {
                match templates.iter().find(|t| t.name == *name) {
                    Some(t) => Type::template_param(t.clone()),
                    None => ty.clone(),
                }
            }
            other => other.map_children(&mut |c| mark(c, templates)),
        }
    }
    if sig.templates.is_empty() {
        return sig;
    }
    let params = sig
        .params
        .iter()
        .map(|p| CallableParam {
            ty: mark(&p.ty, &sig.templates),
            ..p.clone()
        })
        .collect();
    let return_type = Box::new(mark(&sig.return_type, &sig.templates));
    CallableSig {
        params,
        return_type,
        templates: sig.templates,
    }
}

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
    // `$param is ...` resolves per call from the argument; any other
    // subject (`T is ...`) names a template, resolved when it's bound.
    let subject_raw = subject_raw.trim();
    let subject = match subject_raw.strip_prefix('$') {
        Some(param) => ConditionalSubject::Parameter(param.into()),
        None => ConditionalSubject::Template(subject_raw.into()),
    };
    Some(Type(Repr::Conditional {
        subject,
        target: Box::new(parse_type_string(target_raw)),
        if_true: Box::new(if_true),
        if_false: Box::new(if_false),
        negated,
    }))
}

/// `array{name: string, age?: int}` - PHPDoc's structural array shape.
/// Field values can themselves be full type strings (unions, generics,
/// nested shapes), so each is re-run through `parse_type_string`.
///
/// Real PHPStan's own `phpdoc-parser` recognizes the identical brace-field
/// grammar under 3 sibling prefixes too (`ArrayShapeNode::KIND_NON_EMPTY_
/// ARRAY`/`KIND_LIST`/`KIND_NON_EMPTY_LIST`) - `list{Tk, Tv}`'s own
/// positional fields already number identically to `array{Tk, Tv}`'s
/// (both auto-index `0, 1, ...`), so treating all 4 prefixes as the same
/// underlying shape grammar is correct, not just convenient: a real,
/// confirmed corpus bug (`list{Tk, Tv}` falling through this function's
/// old `array`-only gate straight to the generic bare-identifier
/// fallback, namespace-qualified as if it were a literal class reference)
/// found via this project's own `class.notFound` existence checking newly
/// reaching an inline `@var list<list{Tk, Tv}> $entries` tag.
fn try_parse_array_shape(s: &str) -> Option<Type> {
    let lower = s.to_ascii_lowercase();
    let prefix_len = ["non-empty-array", "non-empty-list", "array", "list"]
        .iter()
        .find(|p| lower.starts_with(*p))?
        .len();
    let rest = s[prefix_len..].trim_start();
    if !rest.starts_with('{') || !rest.ends_with('}') {
        return None;
    }
    let inner = &rest[1..rest.len() - 1];

    let mut shape = Shape::sealed(Vec::new());
    // A shape is notation, numbered by the current PHP rule.
    let mut next_index = crate::NextArrayIndex::new(crate::TrinaryLogic::Yes);
    for field_str in split_top_level(inner, ',') {
        let field_str = field_str.trim();
        if field_str.is_empty() {
            continue;
        }
        // PHPStan/Psalm's "unsealed shape" marker (`...<string, mixed>`,
        // `...<mixed>` or bare `...`): the array may carry other,
        // unlisted entries of those types too.
        if let Some(tail) = field_str.strip_prefix("...") {
            let default = Shape::unsealed(Vec::new()).unsealed;
            shape.unsealed = match split_generic_args(tail.trim()).1.as_slice() {
                [value] => Some(Box::new((
                    Type::unknown_array_key(),
                    parse_type_string(value),
                ))),
                [key, value] => Some(Box::new((parse_type_string(key), parse_type_string(value)))),
                _ => default,
            };
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
        // A `::` (`self::STRATEGY_*`) is a class-constant reference, not
        // a `key:` separator.
        let keyed = field_str.split_once(':').filter(|(key_part, type_part)| {
            !type_part.starts_with(':') && looks_like_shape_key(key_part.trim())
        });
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
                let key = ShapeKey::from_name(strip_shape_key_quotes(key));
                if let ShapeKey::Int(n) = key {
                    next_index.observe(n);
                }
                shape.set(ShapeField::new(
                    key,
                    parse_type_string(type_part.trim()),
                    optional,
                ));
            }
            None => {
                // No key is left after `PHP_INT_MAX`: no array has this
                // shape.
                let crate::AppendKey::Key(n) = next_index.take() else {
                    return Some(Type::never());
                };
                let key = ShapeKey::Int(n);
                shape.set(ShapeField::new(key, parse_type_string(field_str), false));
            }
        }
    }
    Some(Type::array_shape(shape))
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
        let key = ShapeKey::from_name(strip_shape_key_quotes(key));
        fields.push(ShapeField::new(
            key,
            parse_type_string(type_part.trim()),
            optional,
        ));
    }
    Some(Type::object_shape(Shape::sealed(fields)))
}

/// The inverse of `strip_shape_key_quotes`, for display: a shape key
/// shown bare only when it's exactly the shape `looks_like_shape_key`
/// accepts unquoted (a bare integer, or an identifier-shaped token) -
/// anything else is single-quoted, so a diagnostic message displaying
/// this type stays round-trippable back through this same parser rather
/// than showing a key text that would parse differently (or not at all)
/// if pasted back into a real `@var`/`@param` tag.
pub(crate) fn display_shape_key(key: &ShapeKey) -> String {
    let ShapeKey::Str(text) = key else {
        return key.to_string();
    };
    let is_bare_identifier = text.as_str().is_some_and(|key| {
        let mut chars = key.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    });
    if is_bare_identifier {
        text.to_string()
    } else {
        crate::display::quote_php_string(text)
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

/// Real PHP's own array-key canonicalization rule for a string key,
/// mirroring `ConstantStringType::toArrayKey()`: `Some(n)` exactly when
/// `key` is a "canonical decimal integer" string - an optional single
/// leading `-`, no leading zero unless the whole value is literally
/// `"0"`, `"-0"` itself excluded (`(string)(int)"-0"` is `"0"`, not
/// `"-0"`), and the value fits in `i64`. `None` otherwise (stays a
/// string key). Plain `str::parse::<i64>()` is **not** sufficient: it
/// happily accepts a leading zero (`"01".parse::<i64>() == Ok(1)`) and
/// `"-0"`, both of which real PHP does *not* canonicalize.
pub fn canonical_int_key(key: &str) -> Option<i64> {
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
    let callable_head = ["closure", "callable", "pure-closure", "pure-callable"]
        .iter()
        .find_map(|k| lower.strip_prefix(k))
        .map(str::trim_start);
    if let Some(rest) = callable_head {
        if rest.starts_with('(') {
            return try_parse_callable_shape(stripped).unwrap_or(Type::callable(None));
        }
        // `callable<T>(T): T`; `Closure<...>` not followed by a
        // parameter list stays a generic reference.
        if rest.starts_with('<')
            && let Some(ty) = try_parse_callable_shape(stripped)
        {
            return ty;
        }
    }

    // A real, documented PHPStan/Psalm convention: a quoted literal
    // (`'ASC'|'DESC'`, `"foo"`) as one member of a union means "exactly
    // this string value," not a class reference - without this, real
    // code in this project's own corpus (`@param 'ASC'|'DESC' $dir`)
    // fell through to the generic-class branch below and got the quotes
    // themselves treated as part of a literal class name, then
    // nonsensically namespace-qualified into e.g. `Piwigo\Db\'ASC'`.
    if let Some(inner) = s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return Type::string_literal(inner.into());
    }
    if let Some(inner) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Type::string_literal(inner.into());
    }

    // Same idea as the quoted-string case just above: a bare integer
    // literal as one member of a union (`1|2|3`, constraining a value to
    // a fixed set - log levels, status codes, mode flags, ...) means
    // "exactly this integer value," not a class reference.
    if let Ok(n) = s.parse::<i64>() {
        return Type::int_literal(n);
    }
    // Mirrors the int-literal case just above, for a real float
    // literal (`1.5|2.5`) - checked after the int case so an
    // integer-looking string is still caught as `IntLiteral` first.
    if let Ok(f) = s.parse::<f64>() {
        return Type::float_literal(f);
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
            ty = Type::array(Type::benevolent_array_key(), ty);
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
            let key_ty = ShapeKey::from_name(key).to_type();
            let mut ty = match late::offset_access(&base_ty, &key_ty) {
                Some(ty) => ty,
                // Possibly a template, known once `mark_template_params`
                // runs; resolved when a call binds it.
                None if matches!(&base_ty, Type(Repr::Object(_, args)) if args.is_empty()) => {
                    Type::offset_access(base_ty, key_ty)
                }
                None => Type::mixed(),
            };
            for _ in 0..array_depth {
                ty = Type::array(Type::benevolent_array_key(), ty);
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
            ty = Type::array(Type::benevolent_array_key(), ty);
        }
        return ty;
    }

    let (base, generic_args) = split_generic_args(s);
    let original = base.trim();
    let bare = original.trim_start_matches('\\');
    let mut ty = if let Some((class_part, const_part)) = original.split_once("::") {
        // A class-constant-reference type (`Foo::BAR`, `self::RFC_*`,
        // `Foo::*`) isn't a class name even though it's shaped like one.
        // Resolving it means reading the referenced class's own
        // constants, which only exist once the registry is built
        // (`Registry::resolve_deferred_value_of`/
        // `resolve_deferred_value_of_now`). `Foo::class` is just a
        // class-string.
        let class_ty = parse_type_string(class_part);
        if const_part.eq_ignore_ascii_case("class") {
            Type::class_string(class_ty)
        } else {
            Type(Repr::ClassConstant {
                class: Box::new(class_ty),
                pattern: const_part.into(),
            })
        }
    } else {
        parse_type_keyword_or_class(bare, original, &generic_args)
    };
    for _ in 0..array_depth {
        ty = Type::array(Type::benevolent_array_key(), ty);
    }
    ty
}

/// The type a native type declaration denotes, its class names as name
/// resolution resolved them.
pub fn native_type(hint: &syntax::ast::TypeHint) -> Type {
    use syntax::ast::TypeHintKind;
    syntax::stack::guard(|| match &hint.kind {
        TypeHintKind::Keyword(_, written) => parse_type_string(written),
        TypeHintKind::Name(name) => Type::object(
            name.resolved_class().unwrap_or(&name.written).into(),
            Vec::new(),
        ),
        TypeHintKind::Nullable(inner) => Type(Repr::Union(vec![native_type(inner), Type::null()])),
        TypeHintKind::Union(members) => {
            Type(Repr::Union(members.iter().map(native_type).collect()))
        }
        TypeHintKind::Intersection(members) => Type(Repr::Intersection(
            members.iter().map(native_type).collect(),
        )),
    })
}

/// Parses one bound of an `int<min, max>` range - real PHPStan's own
/// `"min"`/`"max"` keywords mean unbounded on that side (`Some(None)`),
/// a plain integer literal is a real bound (`Some(Some(n))`). A
/// malformed bound (neither) is `None` - distinct from "unbounded" so
/// the caller can fall back to plain `Repr::Int` instead of silently
/// treating garbage as an open range.
fn parse_int_bound(s: &str) -> Option<Option<i64>> {
    match s {
        "min" | "max" => Some(None),
        s => s.parse().ok().map(Some),
    }
}

/// `bare` (backslash-stripped) drives keyword matching; `original`
/// (backslash-preserved) is what the class-name fallback arm constructs
/// its `Repr::Object` from - a leading `\` means "already fully
/// qualified from the global namespace," which
/// `name_resolution::NameContext::resolve` needs to see later (its first check
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
        // bound falls back to plain `Repr::Int` rather than guessing.
        "int" | "integer" => match generic_args {
            [min, max] => match (parse_int_bound(min), parse_int_bound(max)) {
                (Some(min), Some(max)) => Type::int_range(min, max),
                _ => Type::int(),
            },
            _ => Type::int(),
        },
        "positive-int" => Type::int_range(Some(1), None),
        "negative-int" => Type::int_range(None, Some(-1)),
        "non-negative-int" => Type::int_range(Some(0), None),
        "non-positive-int" => Type::int_range(None, Some(0)),
        // Real PHPStan represents this as a union of two ranges, not its
        // own type (confirmed via dumpType()) - composed the same way.
        "non-zero-int" => Type(Repr::Union(vec![
            Type::int_range(None, Some(-1)),
            Type::int_range(Some(1), None),
        ])),
        // `int-mask<Flag::*>`/`int-mask-of<Flag::*>` - real PHPStan
        // expands these into a union of every combinable bitmask value,
        // which this project can't enumerate without resolving the
        // referenced class constants (same boundary `value-of`/`key-of`
        // below hit). Degrades to plain `Repr::Int` (the pseudo-type's
        // real underlying kind) rather than falling through to the
        // generic class-name branch, which would produce a bogus
        // namespace-qualified "class" that fails every ordinary
        // `int`-typed assignability check.
        "int-mask" | "int-mask-of" => Type::int(),
        "float" | "double" => Type::float(),
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
        // `crate::subtype`'s `LowercaseString`/`UppercaseString`
        // literal-fact arms and the `strtolower`/`strtoupper`/etc.
        // case-tracking producers (`checker/expr/builtin_calls.rs`).
        "lowercase-string" => Type(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::LowercaseString),
        ])),
        "uppercase-string" => Type(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::UppercaseString),
        ])),
        "non-empty-lowercase-string" => Type(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonEmptyString),
            Type::accessory(AccessoryKind::LowercaseString),
        ])),
        "non-empty-uppercase-string" => Type(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonEmptyString),
            Type::accessory(AccessoryKind::UppercaseString),
        ])),
        // `decimal-int-string` mirrors real PHPStan's own
        // `IntersectionType([StringType, AccessoryDecimalIntegerStringType])`.
        // The `inverse` variant (`non-decimal-int-string`) stays
        // unaliased (falls through to the generic-class-name branch)
        // since it needs a wholly separate dynamic-array-key-narrowing
        // consumer this project doesn't build - aliasing it to a kind
        // with no real consumer would be a silent, false claim of
        // support.
        "decimal-int-string" => Type(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::DecimalIntegerString),
        ])),
        "non-empty-string" => Type(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonEmptyString),
        ])),
        "non-falsy-string" | "truthy-string" => Type(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonFalsyString),
        ])),
        "literal-string" => Type(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::LiteralString),
        ])),
        "numeric-string" => Type(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NumericString),
        ])),
        "string" => Type::string(),
        // `callable-string` mirrors real PHPStan's own resolution
        // (`IntersectionType([StringType, CallableType])`) -
        // architecturally unrelated to the `class-string` family below
        // despite the shared "-string" name.
        "callable-string" => Type(Repr::Intersection(vec![
            Type::string(),
            Type::callable(None),
        ])),
        // `enum-string`/`enum-string<T>` mirror real PHPStan's own
        // `GenericClassStringType(UnitEnum)`/`GenericClassStringType(T &
        // UnitEnum)` (`TypeNodeResolver.php` - `GenericClassStringType
        // extends ClassStringType`, i.e. `enum-string` really is
        // `class-string<UnitEnum>` under real PHPStan's own hood too).
        // Every enum this project's own registry sees already
        // synthesizes `implements UnitEnum` at declaration time
        // (`registry/class_info.rs`), so this gets real narrowing/
        // assignability for free through the ordinary Object-vs-Object
        // class-hierarchy walk in `registry::accepts` - no new
        // type-system machinery needed.
        "enum-string" => {
            let unit_enum = Type::object("UnitEnum".into(), Vec::new());
            match generic_args {
                [single] => Type::class_string(Type(Repr::Intersection(vec![
                    parse_type_string(single),
                    unit_enum,
                ]))),
                _ => Type::class_string(unit_enum),
            }
        }
        // `interface-string`/`trait-string` (bare form): real PHPStan
        // gives these no distinguishing semantics from bare
        // `class-string`, but this project checks more strictly -
        // `ClassReflection::kind` already tracks the distinction. No generic
        // `<T>` form: real PHPStan has none for `trait-string`, and
        // `interface-string<T>` has no real corpus evidence calling for
        // it.
        "interface-string" => Type(Repr::ClassKindString(ClassKind::Interface)),
        "trait-string" => Type(Repr::ClassKindString(ClassKind::Trait)),
        // Unlike its string-refinement neighbors above, `class-string<T>`
        // carries a real, template-bindable inner type when given one -
        // needed for `@template R of Base` / `@param class-string<R> $x`
        // shapes to bind `R` from a real `Foo::class` argument. A bare
        // `class-string` has nothing to preserve and stays plain
        // `String`.
        "class-string" => match generic_args {
            [] => Type::string(),
            [single] => Type::class_string(parse_type_string(single)),
            _ => Type::string(),
        },
        "bool" | "boolean" => Type::bool(),
        "true" => Type::bool_literal(true),
        "false" => Type::bool_literal(false),
        // Real, documented PHPStan/Psalm pseudo-types. `numeric` is an
        // honest approximation of real PHPStan's own
        // `int|float|numeric-string`: this project doesn't model
        // numeric-string as a distinct literal-set from plain `string`,
        // so `string` is the closest fit.
        "array-key" => Type(Repr::Union(vec![Type::int(), Type::string()])),
        "scalar" => Type(Repr::Union(vec![
            Type::int(),
            Type::float(),
            Type::string(),
            Type::bool(),
        ])),
        "numeric" => Type(Repr::Union(vec![
            Type::int(),
            Type::float(),
            Type::string(),
        ])),
        // A real, if untyped-at-the-language-level, PHP value kind
        // (file/curl/... handles). `open-resource` is an ordinary (open)
        // `resource`; a `closed-resource` is its own kind, one
        // `is_resource()` rejects (PHPStan's `ClosedResourceType`).
        "resource" | "open-resource" => Type(Repr::Resource { closed: false }),
        "closed-resource" => Type(Repr::Resource { closed: true }),
        // `value-of<T>`/`key-of<T>` - real PHPStan's own
        // `ValueOfType`/`KeyOfType`, both evaluated against `T`'s
        // resolved type only once needed. This project has no general
        // lazy/deferred type-resolution mechanism, so `T` parsing to a
        // real `Array`/`ArrayShape` (needing no registry access) is
        // resolved immediately below; `T` naming a bare class/enum
        // reference is real PHPStan's own `value-of<T>`-on-an-enum
        // special case (`key-of<T>` has no such case) and genuinely
        // needs registry access this parse-time function doesn't have,
        // so it stays a `Repr::ValueOf`, resolved later by
        // `Registry::resolve_deferred_value_of`.
        "value-of" => match generic_args {
            [single] => match parse_type_string(single) {
                ty @ (Type(Repr::Array(..)) | Type(Repr::ArrayShape(_))) => {
                    late::value_of(&ty).unwrap_or(Type::mixed())
                }
                ty @ Type(Repr::Object(..))
                    if ty.as_object().is_some_and(|(_, args)| args.is_empty()) =>
                {
                    Type::value_of(ty)
                }
                target @ Type(Repr::ClassConstant { .. }) => Type::value_of(target),
                _ => Type::mixed(),
            },
            _ => Type::mixed(),
        },
        "key-of" => match generic_args {
            [single] => match parse_type_string(single) {
                ty @ (Type(Repr::Array(..)) | Type(Repr::ArrayShape(_))) => {
                    late::key_of(&ty).unwrap_or(Type::mixed())
                }
                // Possibly a template: kept until a call binds it.
                ty @ Type(Repr::Object(..))
                    if ty.as_object().is_some_and(|(_, args)| args.is_empty()) =>
                {
                    Type::key_of(ty)
                }
                _ => Type::mixed(),
            },
            _ => Type::mixed(),
        },
        "null" => Type::null(),
        "void" => Type::void(),
        // The one genuine source of explicit `mixed` (Phase 84's own
        // `Repr::Mixed`'s doc comment, `types.rs`) - the literal keyword
        // was written somewhere in this type expression, real PHPStan's
        // own `ParserNodeTypeToPHPStanType`/`TypeNodeResolver` both
        // resolving it to `MixedType(true)` too (confirmed directly
        // against `vendor/phpstan-src`).
        "mixed" => Type::explicit_mixed(),
        "never" | "noreturn" | "never-return" | "never-returns" | "no-return" => Type::never(),
        // `array<K, V>`'s key argument is real
        // (`Repr::Array`'s own key slot), not discarded. Bare `array`
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
            0 => Type::array_of(Type::mixed()),
            1 if bare == "array" => Type::array(
                Type::benevolent_array_key(),
                parse_type_string(generic_args[0]),
            ),
            1 => Type::array_of(parse_type_string(generic_args[0])),
            _ => {
                let key_ty = parse_type_string(generic_args[0]);
                let key_ty = if bare == "class-string-map" {
                    Type::class_string(key_ty)
                } else {
                    key_ty
                };
                Type::array(key_ty, parse_type_string(generic_args[1]))
            }
        },
        // `list`/`non-empty-array`/`non-empty-list` are real
        // `Intersection([Array(_), Accessory(_)])` types, mirroring real
        // PHPStan's `IntersectionType([ArrayType,
        // AccessoryArrayListType/NonEmptyArrayType])`.
        "list" | "non-empty-array" | "non-empty-list" => {
            let elem_ty = match generic_args.len() {
                0 => Type::mixed(),
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
                Type::int()
            };
            let mut members = vec![Type::array(key_ty, elem_ty)];
            if bare != "non-empty-array" {
                members.push(Type::accessory(AccessoryKind::ArrayList));
            }
            if bare != "list" {
                members.push(Type::accessory(AccessoryKind::NonEmptyArray));
            }
            crate::intersect_types(members)
        }
        // Unlike its neighbors above, `iterable` is real PHP's own
        // native type hint, defined as exactly `array|Traversable` - a
        // broader type than `array`, not a narrower refinement of it.
        // Parsing it as bare `array` would reject any real
        // Generator/Iterator/IteratorAggregate passed to an
        // `iterable`-hinted parameter.
        "iterable" => {
            let element_ty = match generic_args.len() {
                0 => Type::mixed(),
                1 => parse_type_string(generic_args[0]),
                _ => parse_type_string(generic_args[1]),
            };
            Type(Repr::Union(vec![
                Type::array(Type::unknown_array_key(), element_ty),
                Type::object("Traversable".into(), Vec::new()),
            ]))
        }
        // `Closure` is a concrete class only a real closure object
        // satisfies, strictly narrower than the four shapes `callable`
        // accepts (see `accepts`'s `Repr::Callable` arms) -
        // represented as a real `Object` so `accepts`'s ordinary
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
            Type::object(("\\Closure".to_string()).into(), Vec::new())
        }
        "callable" => Type::callable(None),
        // `self<T>`/`static<T>`: the current class with those arguments.
        "self" => Type::self_ref(generic_args.iter().map(|a| parse_type_string(a)).collect()),
        "parent" => Type::parent_ref(),
        // `@return $this` is a real, PHPStan-documented PHPDoc
        // convention - not a variable reference despite the `$` sigil
        // (real shape: amphp/amp's `@return $this` on several fluent
        // methods).
        "$this" if generic_args.is_empty() => Type::this(),
        "static" | "$this" => {
            Type::static_ref(generic_args.iter().map(|a| parse_type_string(a)).collect())
        }
        "object" => Type::object_without_class(),
        // A user-defined generic class (`Collection<User>`): each type
        // argument is a full type string, re-run through
        // `parse_generic_arg` (call-site-variance-aware; falls straight
        // through to `parse_type_string` for an ordinary argument). A
        // bare (non-generic) reference gets an empty arg list,
        // indistinguishable from "erased" generics - `TemplateParam`
        // substitution treats an unbound parameter as `Mixed` either
        // way.
        _ => Type::object(
            original.into(),
            generic_args.iter().map(|a| parse_generic_arg(a)).collect(),
        ),
    }
}

/// One generic type argument, with its call-site variance annotation
/// if written - `Collection<covariant Foo>`, `Collection<contravariant
/// Foo>`, or `Collection<*>` ("don't check this argument at all"),
/// parsed on every generic type position (`@param`/`@return`/`@var`/
/// `@extends`/`@implements` alike). Consulted by
/// `Registry::effective_slot_variance`, which lets a marker here
/// override a class's declared slot variance for one comparison,
/// matching `GenericObjectType::isSuperTypeOf`'s "call-site variance
/// wins when present" rule.
fn parse_generic_arg(a: &str) -> Type {
    let trimmed = a.trim();
    if trimmed == "*" {
        return Type::variance_arg(CallSiteVariance::Bivariant, Type(Repr::Mixed(false)));
    }
    if let Some(rest) = trimmed.strip_prefix("covariant ") {
        return Type::variance_arg(CallSiteVariance::Covariant, parse_type_string(rest));
    }
    if let Some(rest) = trimmed.strip_prefix("contravariant ") {
        return Type::variance_arg(CallSiteVariance::Contravariant, parse_type_string(rest));
    }
    parse_type_string(trimmed)
}

/// True when `text` mentions the word `mixed` (case-insensitively, as a
/// whole word rather than a substring of some longer identifier like
/// `MixedTypeHelper`) anywhere at all - used to recover real PHPStan's
/// `MixedType::isExplicitMixed()` bit for a value type nested inside an
/// `array`/`iterable`/`list`/... generic, which (like a top-level
/// explicit `mixed`) this project's `Type` model has no room to carry
/// through parsing. Scoped to exactly one declaration's own single type
/// snippet (never a whole docblock), so any occurrence of the word is,
/// in practice, always the author's own explicit `mixed` written
/// somewhere in that same type - worst case a rare false *negative*
/// (silently accepting a still-implicit `mixed` elsewhere in the same
/// union/intersection), never a new false positive.
pub fn text_mentions_mixed(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let is_ident_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut search_from = 0;
    while let Some(offset) = lower[search_from..].find("mixed") {
        let start = search_from + offset;
        let end = start + "mixed".len();
        let before_ok = start == 0 || !is_ident_byte(lower.as_bytes()[start - 1]);
        let after_ok = end == lower.len() || !is_ident_byte(lower.as_bytes()[end]);
        if before_ok && after_ok {
            return true;
        }
        search_from = end;
    }
    false
}
