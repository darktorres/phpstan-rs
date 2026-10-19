use std::sync::Arc;

use crate::{
    AccessoryKind, CallSiteVariance, CallableParam, CallableSig, ClassKind, ConditionalSubject,
    Repr, Shape, ShapeField, ShapeKey, Type, late,
};

/// The type a type string (`"?string"`, `"int|string"`, `"\App\Foo"`,
/// `"Countable&Iterator"`, or a PHPDoc type like `"array<int, User>"` /
/// `"array{name: string, age?: int}"`) denotes, its class names resolved
/// in the global namespace.
pub fn parse_type_string(raw: &str) -> Type {
    parse_written_type(raw).map_class_names(&mut |name| match name.strip_prefix('\\') {
        Some(absolute) => absolute.into(),
        None => name.clone(),
    })
}

/// The type a type string denotes with its class names as written, a
/// leading `\` kept: the class names a PHPDoc type names, still to be
/// resolved in its file's namespace and imports. A class this syntax
/// names itself (`iterable`'s `Traversable`, `Closure(...)`'s `Closure`)
/// is written fully qualified. A `|`/`&` mix needs parentheses (a DNF
/// `(A&B)|null`), as PHPDoc requires.
pub fn parse_written_type(raw: &str) -> Type {
    // The same few thousand type texts are read hundreds of thousands of
    // times (`@param int $x` and its kind, in every file): each is parsed
    // once, and found after. A text's type depends on the text alone.
    static WRITTEN: std::sync::LazyLock<syntax::memo::Memo<Type>> =
        std::sync::LazyLock::new(|| syntax::memo::Memo::with_limits(2048, 1 << 20));
    Type::clone(&WRITTEN.get(raw, parse_written_type_unmemoized))
}

pub(crate) fn parse_written_type_unmemoized(raw: &str) -> Type {
    stack::guard(|| {
        let mut s = raw.trim();
        let nullable = s.starts_with('?');
        if nullable {
            s = &s[1..];
        }

        // Splitting must respect <>/{} nesting - `Collection<int|string>` has
        // no top-level union, but a naive `s.contains('|')` would still split
        // on the one hiding inside the angle brackets.
        if is_plain_word(s) {
            let ty = parse_type_atom(s);
            return if nullable && ty != Type::mixed() {
                Type::new(Repr::Union(vec![ty, Type::null()]))
            } else {
                ty
            };
        }
        let pipe_parts = split_top_level(s, '|');
        let amp_parts = split_top_level(s, '&');
        let ty = if pipe_parts.len() > 1 {
            // Normalized as any union is (`mixed|false` is `mixed`,
            // `Dog|Dog` is `Dog`).
            Type::union_of(pipe_parts.into_iter().map(parse_type_atom).collect())
        } else if amp_parts.len() > 1 {
            crate::intersect_types(amp_parts.into_iter().map(parse_type_atom).collect())
        } else {
            parse_type_atom(s)
        };

        if nullable && ty != Type::mixed() {
            Type::new(Repr::Union(vec![ty, Type::null()]))
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
            ty: parse_written_type(text),
            variadic,
            optional,
            by_ref,
        });
    }
    let sig = CallableSig {
        params,
        return_type: Box::new(parse_written_type(ret_text)),
        templates,
    };
    let sig = mark_callable_templates(sig);
    // `Closure(...)` is a real `Closure` instance with that signature
    // (PHPStan's `ClosureType`), stricter than `callable(...)`, which a
    // string or invokable object satisfies too.
    if matches!(keyword.as_str(), "closure" | "pure-closure") {
        Some(Type::new(Repr::Intersection(vec![
            Type::object("\\Closure".into(), Vec::new()),
            Type::callable(Some(Arc::new(sig))),
        ])))
    } else {
        Some(Type::callable(Some(Arc::new(sig))))
    }
}

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
        default = Some(Box::new(parse_written_type(rest[eq + 1..].trim())));
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
        Some(b) => parse_written_type(b.trim()),
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
        match ty.repr() {
            Repr::Object(name, args) if args.is_empty() => {
                match templates.iter().find(|t| t.name == *name) {
                    Some(t) => Type::template_param(t.clone()),
                    None => ty.clone(),
                }
            }
            _ => ty.map_children(&mut |c| mark(c, templates)),
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

/// PHPStan's conditional return type syntax, `($param is SomeType ?
/// TrueType : FalseType)` or `(T is ...)`, as a `Repr::Conditional`
/// resolved per call site. Requires the literal `is`/`?`/`:` tokens at
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
    let if_true = parse_written_type(true_branch);
    let if_false = parse_written_type(false_branch);
    // `$param is ...` resolves per call from the argument; any other
    // subject (`T is ...`) names a template, resolved when it's bound.
    let subject_raw = subject_raw.trim();
    let subject = match subject_raw.strip_prefix('$') {
        Some(param) => ConditionalSubject::Parameter(param.into()),
        None => ConditionalSubject::Template(subject_raw.into()),
    };
    Some(Type::new(Repr::Conditional {
        subject,
        target: parse_written_type(target_raw),
        if_true,
        if_false,
        negated,
    }))
}

/// Whether `s` starts with the ASCII `prefix`, in any case: the
/// comparison `s.to_ascii_lowercase().starts_with(prefix)` makes for a
/// lowercase `prefix`, without the copy.
fn starts_with_ignore_case(s: &str, prefix: &str) -> bool {
    s.as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
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
    let prefix_len = ["non-empty-array", "non-empty-list", "array", "list"]
        .iter()
        .find(|p| starts_with_ignore_case(s, p))?
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
                    parse_written_type(value),
                ))),
                [key, value] => Some(Box::new((
                    parse_written_type(key),
                    parse_written_type(value),
                ))),
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
                    parse_written_type(type_part.trim()),
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
                shape.set(ShapeField::new(key, parse_written_type(field_str), false));
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
    if !starts_with_ignore_case(s, "object") {
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
            parse_written_type(type_part.trim()),
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

/// Whether `s` is one plain word: a name or keyword of letters, digits,
/// `_`, `-` and `\`, which starts with a letter, `_` or `\`. No other
/// syntax of a type can be in it, and no number (the words Rust reads as
/// floats, `inf`, `infinity` and `nan`, are left out), so it is read as the
/// keyword or class name it is.
pub(crate) fn is_plain_word(s: &str) -> bool {
    /// The bytes a plain word is made of: letters, digits, `_`, `-`, `\`.
    const WORD_BYTE: [bool; 256] = {
        let mut table = [false; 256];
        let mut b = 0;
        while b < 256 {
            let c = b as u8;
            table[b] = c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'\\';
            b += 1;
        }
        table
    };
    let bytes = s.as_bytes();
    let Some(&first) = bytes.first() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b'\\') {
        return false;
    }
    if !bytes.iter().all(|&b| WORD_BYTE[b as usize]) {
        return false;
    }
    // The words Rust reads as floats have the lengths of `inf`, `nan` and
    // `infinity`; no other word is looked at.
    !(matches!(bytes.len(), 3 | 8)
        && ["inf", "infinity", "nan"]
            .iter()
            .any(|word| s.eq_ignore_ascii_case(word)))
}

pub(crate) fn parse_type_atom(s: &str) -> Type {
    let s = s.trim();
    if is_plain_word(s) {
        return parse_type_keyword_or_class(s.trim_start_matches('\\'), s, &[]);
    }
    parse_type_atom_by_form(s)
}

/// `parse_type_atom` read off the text's syntax, without the plain word
/// shortcut.
pub(crate) fn parse_type_atom_by_form(s: &str) -> Type {
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
        return parse_written_type(inner);
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
    let callable_head = ["closure", "callable", "pure-closure", "pure-callable"]
        .iter()
        .find(|k| starts_with_ignore_case(stripped, k))
        .map(|k| stripped[k.len()..].trim_start());
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
            let base_ty = parse_written_type(base_part);
            let key_ty = ShapeKey::from_name(key).to_type();
            let mut ty = match late::offset_access(&base_ty, &key_ty) {
                Some(ty) => ty,
                // Possibly a template, known once `mark_template_params`
                // runs; resolved when a call binds it.
                None if matches!(base_ty.repr(), Repr::Object(_, args) if args.is_empty()) => {
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
        let mut ty = parse_written_type(inner);
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
        let class_ty = parse_written_type(class_part);
        if const_part.eq_ignore_ascii_case("class") {
            Type::class_string(class_ty)
        } else {
            Type::new(Repr::ClassConstant {
                class: class_ty,
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
    stack::guard(|| match &hint.kind {
        TypeHintKind::Keyword(kind, _) => keyword_type(*kind),
        TypeHintKind::Name(name) => Type::object(
            name.resolved_class().unwrap_or(&name.written).into(),
            Vec::new(),
        ),
        TypeHintKind::Nullable(inner) => {
            Type::new(Repr::Union(vec![native_type(inner), Type::null()]))
        }
        TypeHintKind::Union(members) => {
            Type::new(Repr::Union(members.iter().map(native_type).collect()))
        }
        TypeHintKind::Intersection(members) => Type::new(Repr::Intersection(
            members.iter().map(native_type).collect(),
        )),
    })
}

/// The type a keyword type declaration denotes, built once: the parser reads
/// a keyword without regard to its case, so every spelling of one keyword is
/// the same type.
pub(crate) fn keyword_type(kind: syntax::ast::NativeType) -> Type {
    use std::sync::OnceLock;
    use syntax::ast::NativeType as N;
    static TYPES: [OnceLock<Type>; 17] = [const { OnceLock::new() }; 17];
    let slot = match kind {
        N::Int => 0,
        N::Float => 1,
        N::String => 2,
        N::Bool => 3,
        N::Array => 4,
        N::Object => 5,
        N::Mixed => 6,
        N::Void => 7,
        N::Never => 8,
        N::Null => 9,
        N::True => 10,
        N::False => 11,
        N::Iterable => 12,
        N::Callable => 13,
        N::SelfType => 14,
        N::Static => 15,
        N::Parent => 16,
    };
    TYPES[slot]
        .get_or_init(|| parse_type_string(kind.as_str()))
        .clone()
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

/// The keywords a written type reads as the type they name, each with every
/// spelling of it, lower-case: the one list `parse_type_keyword_or_class`
/// matches on and `is_reserved_type_word` asks.
macro_rules! type_keywords {
    ($($variant:ident: $($word:literal),+;)+) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub(crate) enum TypeKeyword {
            $($variant),+
        }

        /// The keyword spelled `lowered`, which is lower-case.
        fn type_keyword(lowered: &str) -> Option<TypeKeyword> {
            match lowered {
                $($($word)|+ => Some(TypeKeyword::$variant),)+
                _ => None,
            }
        }

        /// Every spelling of every keyword.
        #[cfg(test)]
        pub(crate) const TYPE_KEYWORD_WORDS: &[(&str, TypeKeyword)] =
            &[$($(($word, TypeKeyword::$variant)),+),+];
    };
}

type_keywords! {
    Int: "int", "integer";
    PositiveInt: "positive-int";
    NegativeInt: "negative-int";
    NonNegativeInt: "non-negative-int";
    NonPositiveInt: "non-positive-int";
    NonZeroInt: "non-zero-int";
    IntMask: "int-mask", "int-mask-of";
    Float: "float", "double";
    LowercaseString: "lowercase-string";
    UppercaseString: "uppercase-string";
    NonEmptyLowercaseString: "non-empty-lowercase-string";
    NonEmptyUppercaseString: "non-empty-uppercase-string";
    DecimalIntString: "decimal-int-string";
    NonDecimalIntString: "non-decimal-int-string";
    NonEmptyString: "non-empty-string";
    NonFalsyString: "non-falsy-string", "truthy-string";
    LiteralString: "literal-string";
    NumericString: "numeric-string";
    String: "string";
    CallableString: "callable-string";
    EnumString: "enum-string";
    InterfaceString: "interface-string";
    TraitString: "trait-string";
    ClassString: "class-string";
    Bool: "bool", "boolean";
    True: "true";
    False: "false";
    ArrayKey: "array-key";
    Scalar: "scalar";
    Numeric: "numeric";
    Resource: "resource", "open-resource";
    ClosedResource: "closed-resource";
    ValueOf: "value-of";
    KeyOf: "key-of";
    Null: "null";
    Void: "void";
    Mixed: "mixed";
    Never: "never", "noreturn", "never-return", "never-returns", "no-return";
    Array: "array", "class-string-map";
    List: "list", "non-empty-array", "non-empty-list";
    Iterable: "iterable";
    Closure: "closure";
    Callable: "callable";
    SelfType: "self";
    Parent: "parent";
    Static: "static", "$this";
    Object: "object";
}

/// Whether `word` is a keyword a written type reads as the type it names
/// and never as a class (`int`, `string`, `array`, `mixed`,
/// `non-empty-string`, `positive-int`, `self`, `static`, ...), whatever its
/// case, with or without a leading `\`. `Closure` is read as the class of
/// that name, so it is not one.
pub fn is_reserved_type_word(word: &str) -> bool {
    let word = word.strip_prefix('\\').unwrap_or(word);
    let mut buffer = [0u8; 32];
    let Some(bytes) = buffer.get_mut(..word.len()) else {
        return false;
    };
    bytes.copy_from_slice(word.as_bytes());
    bytes.make_ascii_lowercase();
    std::str::from_utf8(bytes)
        .ok()
        .and_then(type_keyword)
        .is_some_and(|keyword| keyword != TypeKeyword::Closure)
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
    // Lowercased on the stack: a keyword is short, and a longer name is a
    // class, whatever its case.
    let mut buffer = [0u8; 32];
    let owned;
    let lowered: &str = if bare.len() <= buffer.len() {
        let bytes = &mut buffer[..bare.len()];
        bytes.copy_from_slice(bare.as_bytes());
        bytes.make_ascii_lowercase();
        std::str::from_utf8(bytes).expect("lowercasing ASCII letters keeps UTF-8")
    } else {
        owned = bare.to_ascii_lowercase();
        &owned
    };
    match type_keyword(lowered) {
        // `int<min, max>` and its named shorthands are real ranges,
        // not aliased to plain `Int`. `int`/`integer` also read
        // `generic_args` when present (`int<0, 255>`) - a malformed
        // bound falls back to plain `Repr::Int` rather than guessing.
        Some(TypeKeyword::Int) => match generic_args {
            [min, max] => match (parse_int_bound(min), parse_int_bound(max)) {
                (Some(min), Some(max)) => Type::int_range(min, max),
                _ => Type::int(),
            },
            _ => Type::int(),
        },
        Some(TypeKeyword::PositiveInt) => Type::int_range(Some(1), None),
        Some(TypeKeyword::NegativeInt) => Type::int_range(None, Some(-1)),
        Some(TypeKeyword::NonNegativeInt) => Type::int_range(Some(0), None),
        Some(TypeKeyword::NonPositiveInt) => Type::int_range(None, Some(0)),
        // Real PHPStan represents this as a union of two ranges, not its
        // own type (confirmed via dumpType()) - composed the same way.
        Some(TypeKeyword::NonZeroInt) => Type::new(Repr::Union(vec![
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
        Some(TypeKeyword::IntMask) => Type::int(),
        Some(TypeKeyword::Float) => Type::float(),
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
        Some(TypeKeyword::LowercaseString) => Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::LowercaseString),
        ])),
        Some(TypeKeyword::UppercaseString) => Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::UppercaseString),
        ])),
        Some(TypeKeyword::NonEmptyLowercaseString) => Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonEmptyString),
            Type::accessory(AccessoryKind::LowercaseString),
        ])),
        Some(TypeKeyword::NonEmptyUppercaseString) => Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonEmptyString),
            Type::accessory(AccessoryKind::UppercaseString),
        ])),
        // `decimal-int-string` mirrors real PHPStan's own
        // `IntersectionType([StringType, AccessoryDecimalIntegerStringType])`,
        // `non-decimal-int-string` its inverse.
        Some(TypeKeyword::DecimalIntString) => Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::DecimalIntegerString),
        ])),
        Some(TypeKeyword::NonDecimalIntString) => Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonDecimalIntegerString),
        ])),
        Some(TypeKeyword::NonEmptyString) => Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonEmptyString),
        ])),
        Some(TypeKeyword::NonFalsyString) => Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonFalsyString),
        ])),
        Some(TypeKeyword::LiteralString) => Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::LiteralString),
        ])),
        Some(TypeKeyword::NumericString) => Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NumericString),
        ])),
        Some(TypeKeyword::String) => Type::string(),
        // `callable-string` mirrors real PHPStan's own resolution
        // (`IntersectionType([StringType, CallableType])`) -
        // architecturally unrelated to the `class-string` family below
        // despite the shared "-string" name.
        Some(TypeKeyword::CallableString) => Type::new(Repr::Intersection(vec![
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
        Some(TypeKeyword::EnumString) => {
            let unit_enum = Type::object("UnitEnum".into(), Vec::new());
            match generic_args {
                [single] => Type::class_string(Type::new(Repr::Intersection(vec![
                    parse_written_type(single),
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
        Some(TypeKeyword::InterfaceString) => {
            Type::new(Repr::ClassKindString(ClassKind::Interface))
        }
        Some(TypeKeyword::TraitString) => Type::new(Repr::ClassKindString(ClassKind::Trait)),
        // Unlike its string-refinement neighbors above, `class-string<T>`
        // carries a real, template-bindable inner type when given one -
        // needed for `@template R of Base` / `@param class-string<R> $x`
        // shapes to bind `R` from a real `Foo::class` argument. A bare
        // `class-string` has nothing to preserve and stays plain
        // `String`.
        Some(TypeKeyword::ClassString) => match generic_args {
            [] => Type::string(),
            [single] => Type::class_string(parse_written_type(single)),
            _ => Type::string(),
        },
        Some(TypeKeyword::Bool) => Type::bool(),
        Some(TypeKeyword::True) => Type::bool_literal(true),
        Some(TypeKeyword::False) => Type::bool_literal(false),
        // Real, documented PHPStan/Psalm pseudo-types.
        Some(TypeKeyword::ArrayKey) => Type::new(Repr::Union(vec![Type::int(), Type::string()])),
        Some(TypeKeyword::Scalar) => Type::new(Repr::Union(vec![
            Type::int(),
            Type::float(),
            Type::string(),
            Type::bool(),
        ])),
        Some(TypeKeyword::Numeric) => Type::new(Repr::Union(vec![
            Type::int(),
            Type::float(),
            Type::new(Repr::Intersection(vec![
                Type::string(),
                Type::accessory(AccessoryKind::NumericString),
            ])),
        ])),
        // A real, if untyped-at-the-language-level, PHP value kind
        // (file/curl/... handles). `open-resource` is an ordinary (open)
        // `resource`; a `closed-resource` is its own kind, one
        // `is_resource()` rejects (PHPStan's `ClosedResourceType`).
        Some(TypeKeyword::Resource) => Type::new(Repr::Resource { closed: false }),
        Some(TypeKeyword::ClosedResource) => Type::new(Repr::Resource { closed: true }),
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
        Some(TypeKeyword::ValueOf) => match generic_args {
            [single] => {
                let ty = parse_written_type(single);
                match ty.repr() {
                    Repr::Array(..) | Repr::ArrayShape(_) => {
                        late::value_of(&ty).unwrap_or(Type::mixed())
                    }
                    Repr::Object(_, args) if args.is_empty() => Type::value_of(ty),
                    Repr::ClassConstant { .. } => Type::value_of(ty),
                    _ => Type::mixed(),
                }
            }
            _ => Type::mixed(),
        },
        Some(TypeKeyword::KeyOf) => match generic_args {
            [single] => {
                let ty = parse_written_type(single);
                match ty.repr() {
                    Repr::Array(..) | Repr::ArrayShape(_) => {
                        late::key_of(&ty).unwrap_or(Type::mixed())
                    }
                    // Possibly a template: kept until a call binds it.
                    Repr::Object(_, args) if args.is_empty() => Type::key_of(ty),
                    _ => Type::mixed(),
                }
            }
            _ => Type::mixed(),
        },
        Some(TypeKeyword::Null) => Type::null(),
        Some(TypeKeyword::Void) => Type::void(),
        // The written keyword is explicit `mixed` (`Repr::Mixed`).
        Some(TypeKeyword::Mixed) => Type::explicit_mixed(),
        Some(TypeKeyword::Never) => Type::never(),
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
        Some(TypeKeyword::Array) => match generic_args.len() {
            0 => Type::array_of(Type::mixed()),
            1 if bare == "array" => Type::array(
                Type::benevolent_array_key(),
                parse_written_type(generic_args[0]),
            ),
            1 => Type::array_of(parse_written_type(generic_args[0])),
            _ => {
                let key_ty = parse_written_type(generic_args[0]);
                let key_ty = if bare == "class-string-map" {
                    Type::class_string(key_ty)
                } else {
                    key_ty
                };
                Type::array(key_ty, parse_written_type(generic_args[1]))
            }
        },
        // `list`/`non-empty-array`/`non-empty-list` are real
        // `Intersection([Array(_), Accessory(_)])` types, mirroring real
        // PHPStan's `IntersectionType([ArrayType,
        // AccessoryArrayListType/NonEmptyArrayType])`.
        Some(TypeKeyword::List) => {
            let elem_ty = match generic_args.len() {
                0 => Type::mixed(),
                1 => parse_written_type(generic_args[0]),
                _ => parse_written_type(generic_args[1]),
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
                    2 => parse_written_type(generic_args[0]),
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
        Some(TypeKeyword::Iterable) => {
            let (key_ty, element_ty) = match generic_args {
                [] => (Type::mixed(), Type::mixed()),
                [value] => (Type::mixed(), parse_written_type(value)),
                [key, value, ..] => (parse_written_type(key), parse_written_type(value)),
            };
            let traversable_args = if generic_args.is_empty() {
                Vec::new()
            } else {
                vec![key_ty, element_ty.clone()]
            };
            Type::new(Repr::Union(vec![
                Type::array(Type::unknown_array_key(), element_ty),
                Type::object("\\Traversable".into(), traversable_args),
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
        Some(TypeKeyword::Closure) if original.starts_with('\\') => {
            Type::object(("\\Closure".to_string()).into(), Vec::new())
        }
        Some(TypeKeyword::Callable) => Type::callable(None),
        // `self<T>`/`static<T>`: the current class with those arguments.
        Some(TypeKeyword::SelfType) => {
            Type::self_ref(generic_args.iter().map(|a| parse_written_type(a)).collect())
        }
        Some(TypeKeyword::Parent) => Type::parent_ref(),
        // `@return $this` is a real, PHPStan-documented PHPDoc
        // convention - not a variable reference despite the `$` sigil
        // (real shape: amphp/amp's `@return $this` on several fluent
        // methods).
        Some(TypeKeyword::Static) if lowered == "$this" && generic_args.is_empty() => Type::this(),
        Some(TypeKeyword::Static) => {
            Type::static_ref(generic_args.iter().map(|a| parse_written_type(a)).collect())
        }
        Some(TypeKeyword::Object) => Type::object_without_class(),
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
/// `effective_slot_variance` (`hierarchy.rs`), which lets a marker here
/// override a class's declared slot variance for one comparison,
/// matching `GenericObjectType::isSuperTypeOf`'s "call-site variance
/// wins when present" rule.
fn parse_generic_arg(a: &str) -> Type {
    let trimmed = a.trim();
    if trimmed == "*" {
        return Type::variance_arg(
            CallSiteVariance::Bivariant,
            Type::new(Repr::Mixed(false, None)),
        );
    }
    if let Some(rest) = trimmed.strip_prefix("covariant ") {
        return Type::variance_arg(CallSiteVariance::Covariant, parse_written_type(rest));
    }
    if let Some(rest) = trimmed.strip_prefix("contravariant ") {
        return Type::variance_arg(CallSiteVariance::Contravariant, parse_written_type(rest));
    }
    parse_written_type(trimmed)
}
