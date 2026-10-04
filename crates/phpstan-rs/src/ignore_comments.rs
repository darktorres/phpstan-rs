//! Inline suppression comments: `@phpstan-ignore-line`,
//! `@phpstan-ignore-next-line`, and `@phpstan-ignore <identifier list>`,
//! read from the file's real comment tokens (so a string that happens to
//! contain the text suppresses nothing). A comment that trails code on
//! its line targets that line; one on its own targets the line of the
//! next code token when that's the very next line, else the comment's
//! own last line. A malformed identifier list suppresses nothing and is
//! reported as `ignore.parseError` instead.

use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct IgnoreComments {
    /// Lines every diagnostic on which is suppressed.
    pub(crate) lines: FxHashSet<u32>,
    /// Lines on which only the listed identifiers are suppressed.
    pub(crate) identifiers: FxHashMap<u32, Vec<String>>,
    /// `(line, message)` for each malformed identifier list.
    pub(crate) parse_errors: Vec<(u32, String)>,
}

/// Parses `source` and scans its comments.
#[cfg(test)]
pub(crate) fn scan(source: &str) -> IgnoreComments {
    let parsed = syntax::parse(source.as_bytes()).expect("the source parses");
    scan_parsed(&parsed)
}

/// A comment waiting for the code after it.
struct Pending<'t> {
    text: &'t str,
    /// The offset of the marker in the text.
    marker: usize,
    /// The line the lines of an error in its list count from.
    error_base: i64,
    /// Its last line.
    last_line: u32,
    /// The line and offset of the first token after it, when there is one.
    next_code: Option<(u32, usize)>,
}

impl Pending<'_> {
    /// Applies the comment: to the line after it when code starts there,
    /// else to its own last line.
    fn settle(self, out: &mut IgnoreComments) {
        match parse_identifiers(self.text, self.marker) {
            Err((error_line, message)) => out.push_error(self.error_base + error_line, message),
            Ok(ids) => {
                let target = match self.next_code {
                    Some((line, _)) if line == self.last_line + 1 => line,
                    _ => self.last_line,
                };
                out.identifiers.entry(target).or_default().extend(ids);
            }
        }
    }
}

/// The scan over a parsed file's marks. A pending mark waits for the next
/// code, and a mark after code on its line belongs to that line; the lexer
/// found for each what lies next to it.
pub(crate) fn scan_parsed(parsed: &syntax::ParsedFile) -> IgnoreComments {
    let mut out = IgnoreComments::default();
    let mut pending: Option<Pending> = None;
    for mark in &parsed.ignore_marks {
        // The code that settles a pending mark came before this one when
        // the lexer found it before this one's start.
        if pending
            .as_ref()
            .is_some_and(|p| p.next_code.is_some_and(|(_, start)| start < mark.start))
            && let Some(settled) = pending.take()
        {
            settled.settle(&mut out);
        }
        let (text, mut line, is_doc) = (&*mark.text, mark.line, mark.is_doc);
        let is_next_line = text.contains("@phpstan-ignore-next-line");
        let is_current_line = text.contains("@phpstan-ignore-line");
        if is_doc {
            mark_each_marker(&mut out.lines, text, line, "@phpstan-ignore-line", false);
            if is_next_line {
                let mut text = text;
                if let Some((start, end)) = last_doc_tag(text)
                    && &text[start..end] == "@phpstan-ignore-next-line"
                {
                    out.lines.insert(line + 1 + newlines(text));
                    text = &text[..start];
                }
                mark_each_marker(
                    &mut out.lines,
                    text,
                    line,
                    "@phpstan-ignore-next-line",
                    true,
                );
            }
            if is_next_line || is_current_line {
                continue;
            }
        } else {
            if is_next_line {
                line += 1;
            }
            if is_next_line || is_current_line {
                out.lines.insert(line + newlines(text));
                continue;
            }
        }
        let Some(marker) = text.find("@phpstan-ignore") else {
            continue;
        };
        let marker_line = newlines(&text[..marker]) as i64 - 1;
        if mark.code_before_on_line {
            match parse_identifiers(text, marker) {
                Ok(ids) => out.identifiers.entry(line).or_default().extend(ids),
                Err((error_line, message)) => {
                    out.push_error(line as i64 + error_line + marker_line, message)
                }
            }
            continue;
        }
        pending = Some(Pending {
            text,
            marker,
            error_base: line as i64 + marker_line,
            last_line: line + newlines(text),
            next_code: mark.next_code,
        });
    }
    if let Some(pending) = pending {
        pending.settle(&mut out);
    }
    out
}

impl IgnoreComments {
    fn push_error(&mut self, line: i64, message: String) {
        self.parse_errors.push((line.max(1) as u32, message));
    }
}

fn newlines(text: &str) -> u32 {
    text.matches('\n').count() as u32
}

/// Every occurrence of `marker` in a doc comment starting on `line`
/// suppresses its own line, or the one after it.
fn mark_each_marker(lines: &mut FxHashSet<u32>, text: &str, line: u32, marker: &str, next: bool) {
    for (pos, _) in text.match_indices(marker) {
        lines.insert(line + newlines(&text[..pos]) + u32::from(next));
    }
}

/// Byte range of the last PHPDoc tag (`@name`, `@ns:name`, or a
/// Doctrine-style `@Name\Space`) in `text`.
fn last_doc_tag(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut last = None;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@'
            && bytes
                .get(i + 1)
                .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_' || *b == b'\\')
        {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || matches!(bytes[i], b'-' | b'_' | b'\\' | b':'))
            {
                i += 1;
            }
            last = Some((start, i));
            continue;
        }
        i += 1;
    }
    last
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum IgnoreToken {
    Whitespace,
    End,
    Identifier,
    Comma,
    OpenParenthesis,
    CloseParenthesis,
    Other,
}

impl IgnoreToken {
    fn label(self) -> &'static str {
        match self {
            IgnoreToken::Whitespace => "T_WHITESPACE",
            IgnoreToken::End => "end",
            IgnoreToken::Identifier => "identifier",
            IgnoreToken::Comma => "comma (,)",
            IgnoreToken::OpenParenthesis => "T_OPEN_PARENTHESIS",
            IgnoreToken::CloseParenthesis => "T_CLOSE_PARENTHESIS",
            IgnoreToken::Other => "T_OTHER",
        }
    }
}

/// Upstream `IgnoreLexer`: `(content, kind, line)`, line 1-based within
/// `input`, always ending in an `End`.
fn tokenize_ignore(input: &str) -> Vec<(&str, IgnoreToken, i64)> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut line = 1;
    let mut i = 0;
    let mut last_kind = None;
    while i < bytes.len() {
        let start = i;
        let rest = &input[i..];
        let kind = if matches!(bytes[i], b' ' | b'\t') {
            while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
                i += 1;
            }
            IgnoreToken::Whitespace
        } else if rest.starts_with('\n') || rest.starts_with("\r\n") {
            i += if rest.starts_with('\r') { 2 } else { 1 };
            while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
                i += 1;
            }
            if bytes.get(i) == Some(&b'*') && bytes.get(i + 1) != Some(&b'/') {
                i += 1;
                if bytes.get(i) == Some(&b' ') {
                    i += 1;
                }
            }
            IgnoreToken::End
        } else if rest.starts_with("*/") {
            i += 2;
            IgnoreToken::End
        } else if bytes[i].is_ascii_alphanumeric() {
            let mut end = i + 1;
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'.') {
                j += 1;
                if bytes[j - 1].is_ascii_alphanumeric() {
                    end = j;
                }
            }
            i = end;
            IgnoreToken::Identifier
        } else if bytes[i] == b',' {
            i += 1;
            IgnoreToken::Comma
        } else if bytes[i] == b'(' {
            i += 1;
            IgnoreToken::OpenParenthesis
        } else if bytes[i] == b')' {
            i += 1;
            IgnoreToken::CloseParenthesis
        } else if !bytes[i].is_ascii_whitespace() {
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && !matches!(bytes[i], b'(' | b')')
            {
                i += 1;
            }
            IgnoreToken::Other
        } else {
            break;
        };
        tokens.push((&input[start..i], kind, line));
        if kind == IgnoreToken::End {
            line += 1;
        }
        last_kind = Some(kind);
    }
    if last_kind != Some(IgnoreToken::End) {
        tokens.push(("", IgnoreToken::End, line));
    }
    tokens
}

/// Upstream `RichParser::parseIdentifiers` over the text after the
/// `@phpstan-ignore` at `marker`: the identifiers, or `(line within the
/// text, message)`.
fn parse_identifiers(text: &str, marker: usize) -> Result<Vec<String>, (i64, String)> {
    let tokens = tokenize_ignore(&text[marker + "@phpstan-ignore".len()..]);
    let mut identifiers = Vec::new();
    let mut open_parentheses = 0;
    let mut expected: Option<Vec<IgnoreToken>> = Some(vec![IgnoreToken::Identifier]);
    let mut last_label = "@phpstan-ignore";
    let mut previous: Option<IgnoreToken> = None;
    let mut token_line = 1;
    for &(content, kind, line) in &tokens {
        if let Some(p) = previous
            && p != IgnoreToken::Whitespace
        {
            last_label = p.label();
        }
        previous = Some(kind);
        token_line = line;
        if let Some(expected) = &expected
            && kind != IgnoreToken::Whitespace
            && !expected.contains(&kind)
        {
            let other = if kind == IgnoreToken::Other {
                format!(" '{content}'")
            } else {
                String::new()
            };
            let expected_labels = expected
                .iter()
                .map(|t| t.label())
                .collect::<Vec<_>>()
                .join(" or ");
            return Err((
                line,
                format!(
                    "Unexpected {}{other} after {last_label}, expected {expected_labels}",
                    kind.label()
                ),
            ));
        }
        match kind {
            IgnoreToken::OpenParenthesis => {
                open_parentheses += 1;
                expected = None;
            }
            IgnoreToken::CloseParenthesis => {
                open_parentheses -= 1;
                if open_parentheses == 0 {
                    expected = Some(vec![IgnoreToken::Comma, IgnoreToken::End]);
                }
            }
            _ if open_parentheses > 0 => {}
            IgnoreToken::Identifier => {
                identifiers.push(content.to_string());
                expected = Some(vec![
                    IgnoreToken::Comma,
                    IgnoreToken::End,
                    IgnoreToken::OpenParenthesis,
                ]);
            }
            IgnoreToken::Comma => expected = Some(vec![IgnoreToken::Identifier]),
            _ => {}
        }
    }
    if open_parentheses > 0 {
        return Err((
            token_line,
            "Unexpected end, unclosed opening parenthesis".to_string(),
        ));
    }
    if identifiers.is_empty() {
        return Err((1, "Missing identifier".to_string()));
    }
    Ok(identifiers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trailing_line_comment_ignores_its_own_line() {
        let source = "<?php\n$x = 1 / 0; // @phpstan-ignore-line\necho $x;\n";
        assert_eq!(scan(source).lines, FxHashSet::from_iter([2]));
    }

    #[test]
    fn a_standalone_comment_ignores_the_next_line() {
        let source = "<?php\n// @phpstan-ignore-next-line\n$x = 1 / 0;\n";
        assert_eq!(scan(source).lines, FxHashSet::from_iter([3]));
    }

    #[test]
    fn a_doc_comment_s_closing_line_ignores_its_own_line() {
        let source = "<?php\n/**\n * Some note.\n * @phpstan-ignore-line\n */\n$x = 1 / 0;\n";
        assert_eq!(scan(source).lines, FxHashSet::from_iter([4]));
    }

    #[test]
    fn a_bare_phpstan_ignore_with_identifiers_is_never_treated_as_a_line_marker() {
        // The fuller identifier-list form (`ignored_identifiers`, below)
        // is a deliberately separate mechanism - this scan must not
        // mistake it for either simple line marker.
        let source = "<?php\n$x = 1 / 0; // @phpstan-ignore argument.type\n";
        assert!(scan(source).lines.is_empty());
    }

    #[test]
    fn no_marker_at_all_ignores_nothing() {
        let source = "<?php\n$x = 1 / 0;\n";
        assert!(scan(source).lines.is_empty());
    }

    #[test]
    fn multiple_markers_in_one_file_are_all_collected() {
        let source = "<?php\n$a = 1 / 0; // @phpstan-ignore-line\n// @phpstan-ignore-next-line\n$b = 1 / 0;\n";
        assert_eq!(scan(source).lines, FxHashSet::from_iter([2, 4]));
    }

    #[test]
    fn a_trailing_identifier_list_targets_its_own_line() {
        let source = "<?php\n$x = \"a\" + 1; // @phpstan-ignore binaryOp.invalid\n";
        assert_eq!(
            scan(source).identifiers,
            FxHashMap::from_iter([(2, vec!["binaryOp.invalid".to_string()])])
        );
    }

    #[test]
    fn a_standalone_identifier_list_targets_the_next_line() {
        let source = "<?php\n// @phpstan-ignore binaryOp.invalid\n$x = \"a\" + 1;\n";
        assert_eq!(
            scan(source).identifiers,
            FxHashMap::from_iter([(3, vec!["binaryOp.invalid".to_string()])])
        );
    }

    #[test]
    fn a_multiline_doc_comment_tag_skips_past_its_own_bare_closing_line() {
        let source = "<?php\n/**\n * @phpstan-ignore binaryOp.invalid\n */\n$x = \"a\" + 1;\n";
        assert_eq!(
            scan(source).identifiers,
            FxHashMap::from_iter([(5, vec!["binaryOp.invalid".to_string()])])
        );
    }

    #[test]
    fn multiple_identifiers_on_one_line_are_all_collected() {
        let source = "<?php\n$x = \"a\" + 1; // @phpstan-ignore binaryOp.invalid, argument.type\n";
        assert_eq!(
            scan(source).identifiers,
            FxHashMap::from_iter([(
                2,
                vec!["binaryOp.invalid".to_string(), "argument.type".to_string()]
            )])
        );
    }

    #[test]
    fn a_parenthesized_comment_after_an_identifier_is_discarded() {
        let source = "<?php\n$x = \"a\" + 1; // @phpstan-ignore binaryOp.invalid (known, tracked in JIRA-123)\n";
        assert_eq!(
            scan(source).identifiers,
            FxHashMap::from_iter([(2, vec!["binaryOp.invalid".to_string()])])
        );
    }

    #[test]
    fn a_malformed_identifier_list_suppresses_nothing_and_is_reported() {
        let source = "<?php\n$x = \"a\" + 1; // @phpstan-ignore ,\n";
        let scanned = scan(source);
        assert!(scanned.identifiers.is_empty());
        assert_eq!(
            scanned.parse_errors,
            vec![(
                2,
                "Unexpected comma (,) after @phpstan-ignore, expected identifier".to_string()
            )]
        );
    }

    fn errors(source: &str) -> Vec<(u32, String)> {
        scan(source).parse_errors
    }

    #[test]
    fn upstream_parse_error_messages() {
        assert_eq!(
            errors("<?php\n$x = 1; // @phpstan-ignore\n"),
            vec![(
                2,
                "Unexpected end after @phpstan-ignore, expected identifier".to_string()
            )]
        );
        assert_eq!(
            errors("<?php\n$x = 1; // @phpstan-ignore foo.bar baz\n"),
            vec![(
                2,
                "Unexpected identifier after identifier, expected comma (,) or end or T_OPEN_PARENTHESIS"
                    .to_string()
            )]
        );
        assert_eq!(
            errors("<?php\n$x = 1; // @phpstan-ignore foo.bar (reason\n"),
            vec![(
                2,
                "Unexpected end, unclosed opening parenthesis".to_string()
            )]
        );
        assert_eq!(
            errors("<?php\n$x = 1; // @phpstan-ignore foo.bar, -x\n"),
            vec![(
                2,
                "Unexpected T_OTHER '-x' after comma (,), expected identifier".to_string()
            )]
        );
        assert_eq!(
            errors("<?php\n$x = 1; // @phpstan-ignore foo.bar (ok), baz (fine)\n"),
            vec![]
        );
    }

    #[test]
    fn a_standalone_malformed_list_reports_on_the_comment_line() {
        assert_eq!(
            errors("<?php\n\n// @phpstan-ignore\n$x = 1;\n"),
            vec![(
                3,
                "Unexpected end after @phpstan-ignore, expected identifier".to_string()
            )]
        );
        assert_eq!(
            errors("<?php\n/**\n * @phpstan-ignore foo.bar\n * trailing prose\n */\n$x = 1;\n"),
            vec![(
                4,
                "Unexpected identifier after end, expected comma (,) or end or T_OPEN_PARENTHESIS"
                    .to_string()
            )]
        );
    }

    #[test]
    fn text_in_a_string_is_not_a_comment() {
        let source = "<?php\n$x = '// @phpstan-ignore-line'; $y = \"@phpstan-ignore ,\";\n";
        assert_eq!(scan(source), IgnoreComments::default());
    }

    #[test]
    fn a_standalone_list_before_a_blank_line_targets_its_own_line() {
        let source = "<?php\n// @phpstan-ignore foo.bar\n\n$x = 1;\n";
        assert_eq!(
            scan(source).identifiers,
            FxHashMap::from_iter([(2, vec!["foo.bar".to_string()])])
        );
    }

    #[test]
    fn a_hash_comment_and_a_block_comment_both_count() {
        let source =
            "<?php\n$a = 1; # @phpstan-ignore-line\n/* @phpstan-ignore foo.bar */\n$b = 1;\n";
        let scanned = scan(source);
        assert_eq!(scanned.lines, FxHashSet::from_iter([2]));
        assert_eq!(
            scanned.identifiers,
            FxHashMap::from_iter([(4, vec!["foo.bar".to_string()])])
        );
    }

    #[test]
    fn a_doc_comment_ending_in_next_line_ignores_the_line_after_the_comment() {
        let source = "<?php\n/**\n * @var int $x\n * @phpstan-ignore-next-line\n */\n$x = 1;\n";
        assert!(scan(source).lines.contains(&6), "{:?}", scan(source).lines);
    }

    #[test]
    fn a_simple_line_marker_is_never_also_treated_as_an_identifier_list() {
        let source = "<?php\n$x = 1 / 0; // @phpstan-ignore-line\n";
        assert!(scan(source).identifiers.is_empty());
    }

    #[test]
    fn no_phpstan_ignore_at_all_collects_nothing() {
        let source = "<?php\n$x = \"a\" + 1;\n";
        assert!(scan(source).identifiers.is_empty());
    }
}

syntax::canonical_struct!(IgnoreComments {
    lines,
    identifiers,
    parse_errors,
});
