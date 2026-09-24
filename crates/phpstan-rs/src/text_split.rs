//! Recovers a real per-name `Span` from a larger span whose source text
//! holds more than one name (`Foo::CONST`, `?Foo`,
//! `Foo|Bar`, `extends A, B`) - none of which the AST tracks separately
//! (see `ast.rs`'s own `Span`-carrying fields this module's callers
//! read). Re-lexes *just that span's own text* (never the whole file)
//! via `Lexer::new_in_php` and re-parses each qualified name in it via
//! the real parser's own `parse_qualified_name_spanned` - reusing the
//! exact grammar `parse_qualified_name`'s ~30 other call sites already
//! rely on, rather than a hand-rolled parallel implementation that could
//! silently diverge from it.

use crate::deps::is_primitive_type_name;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::token::{Position, Span, Token};

/// Every qualified-name-shaped token run found inside `source[span]`, in
/// source order, each with its own real `Span` in `source`'s own
/// coordinates. Deliberately does **not** filter out scalar/pseudo-type
/// keywords here (`int`, `self`, `static`, ...) the way `deps.rs`'s own
/// `is_primitive_type_name` would - unlike that function's "does this
/// file mention a real class name" purpose, a caller here (`StaticAccess`/
/// `MethodCall`'s own class+member split) often *wants* exactly one of
/// those names back (`self::CONST`'s own `"self"` half is a real,
/// resolvable navigation target, just resolved differently than an
/// ordinary class name - see `navigation::resolve_class_ref`), and
/// filtering it out here would silently break this function's own
/// `expected`-count check for that whole family of references. Callers
/// that *do* want primitive/pseudo-type filtering (a compound type hint's
/// own class names) apply it themselves - see `type_hint_identifiers_qualified`.
///
/// `expected` is the name count the caller already has from the AST
/// (e.g. `[class_name, member]` for a `StaticAccess`) - this function
/// only ever *locates* text the parser already agreed exists, so a count
/// mismatch means the re-lex found a different shape than expected (a
/// lexer edge case, an off-by-one in the span itself) and the caller
/// should fall back to the coarser enclosing span rather than trust a
/// wrong split.
pub(crate) fn split_identifier_spans(
    source: &str,
    span: Span,
    expected: &[&str],
) -> Option<Vec<Span>> {
    let text = slice_span(source, span)?;
    let tokens = Lexer::new_in_php(&text).tokenize_with_spans();
    let mut parser = Parser::new(tokens);
    let mut spans = Vec::new();
    while !parser.at_eof() {
        if matches!(
            parser.peek(),
            Token::Backslash | Token::Identifier(_) | Token::Keyword(_)
        ) {
            let (name, relative_span) = parser.parse_qualified_name_spanned();
            if name.is_empty() {
                // A lone, dangling backslash with no segment after it -
                // parse_qualified_name_spanned already advanced past it;
                // nothing to record.
                continue;
            }
            spans.push(offset_span(span.start, relative_span));
        } else {
            parser.skip();
        }
    }
    if spans.len() == expected.len() {
        Some(spans)
    } else {
        None
    }
}

/// The class-name identifiers inside a raw type-hint *string* (`"?Foo"`,
/// `"Foo|Bar"`, `"array<Baz>"`), in source order. `deps.rs`'s own
/// `collect_type_hint` does the identical character-scan (splitting on
/// anything that isn't part of a bare/qualified identifier, discarding
/// primitive type keywords), but into an unordered `HashSet` for its own
/// "does this file mention that name anywhere" purpose. This needs the
/// same names *in order*, to zip positionally against `split_identifier_
/// spans`'s own ordered output and learn which name is at which sub-span.
/// Structured after (not sharing code with) `collect_type_hint`, for
/// exactly the reason this whole module's own doc comment gives for
/// `parse_qualified_name_spanned` rather than changing existing call
/// sites. Each name comes with whether it was written fully qualified (a
/// leading `\`), which name resolution must know.
pub(crate) fn type_hint_identifiers_qualified(hint: &str) -> Vec<(String, bool)> {
    let mut names = Vec::new();
    let mut current = String::new();
    for c in hint.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() || c == '_' || c == '\\' {
            current.push(c);
        } else if !current.is_empty() {
            let name = current.trim_start_matches('\\');
            if !name.is_empty() && !is_primitive_type_name(name) {
                names.push((name.to_string(), current.starts_with('\\')));
            }
            current.clear();
        }
    }
    names
}

/// `source`'s own substring covered by `span` - `Position`'s own doc
/// comment: 1-indexed, `col` counts UTF-8 *characters*, not bytes, the
/// same convention every span in this project already uses.
pub(crate) fn slice_span(source: &str, span: Span) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let start_line = (span.start.line as usize).checked_sub(1)?;
    let end_line = (span.end.line as usize).checked_sub(1)?;
    if start_line == end_line {
        let chars: Vec<char> = lines.get(start_line)?.chars().collect();
        let start = (span.start.col as usize).checked_sub(1)?;
        let end = (span.end.col as usize).checked_sub(1)?;
        if start > end || end > chars.len() {
            return None;
        }
        return Some(chars[start..end].iter().collect());
    }
    let mut out = String::new();
    let first_chars: Vec<char> = lines.get(start_line)?.chars().collect();
    let start = (span.start.col as usize).checked_sub(1)?;
    if start > first_chars.len() {
        return None;
    }
    out.extend(&first_chars[start..]);
    out.push('\n');
    for line in lines.get(start_line + 1..end_line)? {
        out.push_str(line);
        out.push('\n');
    }
    let last_chars: Vec<char> = lines.get(end_line)?.chars().collect();
    let end = (span.end.col as usize).checked_sub(1)?;
    if end > last_chars.len() {
        return None;
    }
    out.extend(&last_chars[..end]);
    Some(out)
}

/// Translates a position produced by re-lexing an isolated text slice
/// (1-indexed from line 1, col 1 within that slice) back into `origin`'s
/// own coordinate system - `origin` being the absolute position the
/// re-lexed slice's own first character actually sits at in the real
/// file.
fn offset_position(origin: Position, relative: Position) -> Position {
    if relative.line == 1 {
        Position {
            line: origin.line,
            col: origin.col + relative.col - 1,
        }
    } else {
        Position {
            line: origin.line + relative.line - 1,
            col: relative.col,
        }
    }
}

fn offset_span(origin: Position, relative: Span) -> Span {
    Span {
        start: offset_position(origin, relative.start),
        end: offset_position(origin, relative.end),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::StmtKind;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    /// Parses `src` for real and returns its `FunctionDecl`'s own
    /// `return_type_span` - real compound-type-hint spans, not
    /// hand-written `Span` literals, so these tests exercise the exact
    /// spans `navigation.rs` will actually feed this module.
    fn return_type_span(src: &str) -> Span {
        let program = Parser::new(Lexer::new(src).tokenize_with_spans())
            .parse_program()
            .unwrap();
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        f.return_type_span.expect("return type span")
    }

    fn text_at(src: &str, span: Span) -> String {
        let chars: Vec<char> = src
            .lines()
            .nth((span.start.line - 1) as usize)
            .unwrap()
            .chars()
            .collect();
        chars[(span.start.col - 1) as usize..(span.end.col - 1) as usize]
            .iter()
            .collect()
    }

    #[test]
    fn splits_a_nullable_hint_into_its_one_real_class_name() {
        let src = "<?php function f(): ?Foo {}";
        let span = return_type_span(src);
        let spans = split_identifier_spans(src, span, &["Foo"]).expect("split");
        assert_eq!(spans.len(), 1);
        assert_eq!(text_at(src, spans[0]), "Foo");
    }

    #[test]
    fn splits_a_union_hint_into_each_class_name_in_order() {
        let src = "<?php function f(): Foo|Bar {}";
        let span = return_type_span(src);
        let spans = split_identifier_spans(src, span, &["Foo", "Bar"]).expect("split");
        assert_eq!(spans.len(), 2);
        assert_eq!(text_at(src, spans[0]), "Foo");
        assert_eq!(text_at(src, spans[1]), "Bar");
    }

    #[test]
    fn a_namespaced_name_is_one_span_not_split_at_its_own_backslashes() {
        let src = "<?php function f(): \\App\\Foo|null {}";
        let span = return_type_span(src);
        // split_identifier_spans itself doesn't filter "null" out (see its
        // own doc comment on why) - both names are real token runs here.
        let spans = split_identifier_spans(src, span, &["\\App\\Foo", "null"]).expect("split");
        assert_eq!(spans.len(), 2);
        assert_eq!(text_at(src, spans[0]), "\\App\\Foo");
        assert_eq!(text_at(src, spans[1]), "null");
    }

    #[test]
    fn type_hint_identifiers_skips_scalar_and_null_keywords() {
        // Unlike split_identifier_spans (which finds every token run
        // unfiltered), type_hint_identifiers_qualified is specifically for a
        // compound type hint's own *class* names - nothing real-class-
        // shaped in this hint at all.
        assert!(type_hint_identifiers_qualified("int|string|null").is_empty());
        assert_eq!(
            type_hint_identifiers_qualified("?\\App\\Foo|Bar"),
            vec![("App\\Foo".to_string(), true), ("Bar".to_string(), false)]
        );
    }

    #[test]
    fn split_identifier_spans_does_not_filter_self_static_parent() {
        // is_primitive_type_name (deps.rs's own "not an independent
        // dependency edge" list) also lists self/static/parent -
        // filtering those out here would break the count check for every
        // self::X/static::X/parent::X StaticAccess, since a caller there
        // always expects both halves back.
        let src = "<?php self::LIMIT;";
        let span = Span {
            start: Position { line: 1, col: 7 },
            end: Position { line: 1, col: 18 },
        };
        assert_eq!(text_at(src, span), "self::LIMIT");
        let spans = split_identifier_spans(src, span, &["self", "LIMIT"]).expect("split");
        assert_eq!(spans.len(), 2);
        assert_eq!(text_at(src, spans[0]), "self");
        assert_eq!(text_at(src, spans[1]), "LIMIT");
    }

    #[test]
    fn a_mismatched_expected_count_returns_none_rather_than_a_wrong_split() {
        let src = "<?php function f(): Foo|Bar {}";
        let span = return_type_span(src);
        assert!(split_identifier_spans(src, span, &["Foo"]).is_none());
    }

    #[test]
    fn splits_a_static_access_expression_into_its_class_and_member_halves() {
        use crate::ast::{Expr, ExprKind};
        let src = "<?php Foo::CONST;";
        let program = Parser::new(Lexer::new(src).tokenize_with_spans())
            .parse_program()
            .unwrap();
        let StmtKind::Expression(Expr {
            kind: ExprKind::StaticAccess(class, member),
            span,
        }) = &program[0].kind
        else {
            panic!("expected a StaticAccess expression statement");
        };
        let spans = split_identifier_spans(src, *span, &[class, member]).expect("split");
        assert_eq!(spans.len(), 2);
        assert_eq!(text_at(src, spans[0]), "Foo");
        assert_eq!(text_at(src, spans[1]), "CONST");
    }
}
