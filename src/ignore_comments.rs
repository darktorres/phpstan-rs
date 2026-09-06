//! Phase 44: `@phpstan-ignore-line`/`@phpstan-ignore-next-line` inline
//! suppression - real PHPStan's own two "simple line marker" forms of
//! `@phpstan-ignore` - plus (Phase 44 follow-up) the fuller bare
//! `@phpstan-ignore <identifier list>` form, which needs real
//! per-identifier matching against a diagnostic's own `identifier`
//! (`ignored_identifiers`, below).
//!
//! Deliberately a plain text scan over the raw source, one line at a time,
//! rather than a lexer-level change: this project's own lexer discards
//! every non-doc comment already (`Lexer::skip_whitespace_and_comments`),
//! and `FileState` (`project.rs`) doesn't keep the raw source around after
//! parsing either - both would need real, more invasive plumbing to
//! recover comment text at diagnostic-filtering time. A one-time text scan
//! at `load_file`/`update_file` time (while the raw source is still on
//! hand) avoids all of that, at the cost of the one real imprecision a
//! lexer-level scan wouldn't have: a string literal that happens to
//! contain the literal text `@phpstan-ignore-line`/`-next-line` would also
//! suppress that line, same as it does at the real comment site - no such
//! case found anywhere in this project's own real Piwigo+vendor
//! verification corpus, and real PHPStan's own scan is comment-token-based
//! specifically to avoid this, not because it's otherwise likely.

use std::collections::{HashMap, HashSet};

/// Every real PHP line comment (`//`) is single-line by construction, and
/// a `/** ... */`/`/* ... */` block comment - which *can* span multiple
/// physical lines - almost always has its own `@phpstan-ignore-*` marker
/// on its last, closing line, matching real PHPStan's own actual usage
/// throughout its own corpus (a marker earlier in a longer, multi-line
/// doc comment would still need the comment's own *closing* line here,
/// which this plain per-physical-line scan naturally already gives: the
/// marker's own line is used directly, not the comment's opening line).
///
/// `@phpstan-ignore-line` marks its own line; `@phpstan-ignore-next-line`
/// marks the line immediately after it - real PHP source lines are
/// 1-indexed here, matching `Span`/`Position`'s own convention throughout
/// this project.
pub(crate) fn ignored_lines(source: &str) -> HashSet<u32> {
    let mut lines = HashSet::new();
    for (zero_based, text) in source.lines().enumerate() {
        let line = zero_based as u32 + 1;
        if text.contains("@phpstan-ignore-next-line") {
            lines.insert(line + 1);
        } else if text.contains("@phpstan-ignore-line") {
            lines.insert(line);
        }
    }
    lines
}

/// The fuller bare `@phpstan-ignore <identifier list>` form - a
/// comma-separated list of diagnostic identifiers (each optionally
/// followed by a parenthesized, freeform, discarded comment), matched
/// against a diagnostic's own `Diagnostic::identifier` at filtering time
/// rather than suppressing every diagnostic on the line the way the two
/// simple markers do. Mirrors real PHPStan's own `RichParser`
/// (`vendor/phpstan-src/src/Parser/RichParser.php`) trailing-vs-
/// standalone placement rule *exactly* for the single-physical-line case
/// it actually reduces to here: a comment trailing real code on its own
/// line (`$x = foo(); // @phpstan-ignore argument.type`) targets that
/// same line; a comment standing alone on its own line targets the line
/// immediately after it - real PHPStan's own rule is in fact *only* ever
/// "immediately after," with no forward-skip over blank lines/other
/// comments in between (confirmed directly reading `RichParser::
/// parseString`'s own pending-token merge: a gap falls back to the
/// comment's own line, which has nothing on it to ever match a real
/// diagnostic against - functionally a no-op, not a skip-forward).
///
/// One accepted, documented deviation from real PHPStan, matching this
/// module's own established "plain per-physical-line text scan, not a
/// real lexer" tradeoff: a multi-line `/** ... */` doc comment is only
/// handled correctly when the `@phpstan-ignore` tag itself sits on the
/// comment's own *closing* line (literally ending in `*/`, or the last
/// content line immediately before a bare `*/`-only line) - real
/// PHPStan tracks the comment's own opening/closing token positions
/// precisely; a tag on an *earlier*, non-closing line of a longer doc
/// comment isn't recognized as standalone here (this scan has no way to
/// see the comment's own closing line from an earlier one), the same
/// "closing line only" simplification `ignored_lines` above already
/// documents for the two simple markers.
///
/// A malformed identifier list (an empty entry, one containing anything
/// outside `[a-zA-Z0-9.]`, or an unclosed `(...)` comment) drops that
/// whole line's suppression silently rather than reporting a parse
/// error the way real PHPStan does - the same "don't guess, don't
/// crash" leniency `ignore_errors.rs`'s own malformed-regex handling
/// already applies to a different, related mechanism.
pub(crate) fn ignored_identifiers(source: &str) -> HashMap<u32, Vec<String>> {
    let lines: Vec<&str> = source.lines().collect();
    let mut result: HashMap<u32, Vec<String>> = HashMap::new();
    for (zero_based, text) in lines.iter().enumerate() {
        if text.contains("@phpstan-ignore-next-line") || text.contains("@phpstan-ignore-line") {
            continue;
        }
        let Some(marker_pos) = text.find("@phpstan-ignore") else {
            continue;
        };
        let after_marker = &text[marker_pos + "@phpstan-ignore".len()..];
        let id_text = after_marker.split("*/").next().unwrap_or(after_marker);
        let Some(identifiers) = parse_identifier_list(id_text) else {
            continue;
        };
        let prefix = &text[..marker_pos];
        let comment_start = [prefix.rfind("//"), prefix.rfind("/*")]
            .into_iter()
            .flatten()
            .max();
        let is_trailing = match comment_start {
            Some(pos) => !prefix[..pos].trim().is_empty(),
            // No comment-open marker on *this* line at all - a
            // continuation line of a multi-line doc comment
            // (` * @phpstan-ignore ...`). Its own leading whitespace/
            // `*` decoration must not itself count as "real code before
            // it".
            None => !prefix
                .trim_start_matches([' ', '\t', '*'])
                .trim()
                .is_empty(),
        };
        let target_line = if is_trailing {
            zero_based
        } else {
            // The real, common multi-line shape (`/**\n * @phpstan-
            // ignore ...\n */\n$real;`) puts the doc comment's own
            // closing `*/` on its *own* following line, not the tag's -
            // skip past any number of bare closing-marker-only lines so
            // "standalone" still lands on the first genuine line of code
            // after the whole comment, not the `*/` line itself.
            let mut target = zero_based + 1;
            while lines.get(target).is_some_and(|l| l.trim() == "*/") {
                target += 1;
            }
            target
        };
        result
            .entry(target_line as u32 + 1)
            .or_default()
            .extend(identifiers);
    }
    result
}

/// Parses a comma-separated `@phpstan-ignore` identifier list (each
/// entry optionally followed by a parenthesized comment, discarded) -
/// `None` on any malformed entry, the whole list rejected together
/// rather than partially trusted. Real PHPStan's own identifier syntax
/// (`Error::PATTERN_IDENTIFIER`): `[a-zA-Z0-9](?:[a-zA-Z0-9.]*[a-zA-Z0-9])?`
/// - alphanumeric, dot-separated segments, no leading/trailing dot.
fn parse_identifier_list(text: &str) -> Option<Vec<String>> {
    // Not a plain `text.split(',')` - a parenthesized comment can
    // itself contain a comma (`@phpstan-ignore binaryOp.invalid (known,
    // tracked in JIRA-123)`, a real, common shape), which must not be
    // mistaken for an identifier separator. Tracks paren depth instead,
    // only splitting on a comma seen at depth 0.
    let mut identifiers = Vec::new();
    let mut depth: i32 = 0;
    let mut current = String::new();
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            ',' if depth == 0 => {
                identifiers.push(finish_identifier_entry(&current)?);
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if depth != 0 {
        return None;
    }
    identifiers.push(finish_identifier_entry(&current)?);
    if identifiers.is_empty() {
        None
    } else {
        Some(identifiers)
    }
}

fn finish_identifier_entry(raw: &str) -> Option<String> {
    let name = raw.split('(').next().unwrap_or(raw).trim();
    is_valid_identifier(name).then(|| name.to_string())
}

fn is_valid_identifier(s: &str) -> bool {
    let is_id_char = |c: char| c.is_ascii_alphanumeric() || c == '.';
    !s.is_empty()
        && s.chars().all(is_id_char)
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
        && s.ends_with(|c: char| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trailing_line_comment_ignores_its_own_line() {
        let source = "<?php\n$x = 1 / 0; // @phpstan-ignore-line\necho $x;\n";
        assert_eq!(ignored_lines(source), HashSet::from([2]));
    }

    #[test]
    fn a_standalone_comment_ignores_the_next_line() {
        let source = "<?php\n// @phpstan-ignore-next-line\n$x = 1 / 0;\n";
        assert_eq!(ignored_lines(source), HashSet::from([3]));
    }

    #[test]
    fn a_doc_comment_s_closing_line_ignores_its_own_line() {
        let source = "<?php\n/**\n * Some note.\n * @phpstan-ignore-line\n */\n$x = 1 / 0;\n";
        assert_eq!(ignored_lines(source), HashSet::from([4]));
    }

    #[test]
    fn a_bare_phpstan_ignore_with_identifiers_is_never_treated_as_a_line_marker() {
        // The fuller identifier-list form (`ignored_identifiers`, below)
        // is a deliberately separate mechanism - this scan must not
        // mistake it for either simple line marker.
        let source = "<?php\n$x = 1 / 0; // @phpstan-ignore argument.type\n";
        assert!(ignored_lines(source).is_empty());
    }

    #[test]
    fn no_marker_at_all_ignores_nothing() {
        let source = "<?php\n$x = 1 / 0;\n";
        assert!(ignored_lines(source).is_empty());
    }

    #[test]
    fn multiple_markers_in_one_file_are_all_collected() {
        let source = "<?php\n$a = 1 / 0; // @phpstan-ignore-line\n// @phpstan-ignore-next-line\n$b = 1 / 0;\n";
        assert_eq!(ignored_lines(source), HashSet::from([2, 4]));
    }

    #[test]
    fn a_trailing_identifier_list_targets_its_own_line() {
        let source = "<?php\n$x = \"a\" + 1; // @phpstan-ignore binaryOp.invalid\n";
        assert_eq!(
            ignored_identifiers(source),
            HashMap::from([(2, vec!["binaryOp.invalid".to_string()])])
        );
    }

    #[test]
    fn a_standalone_identifier_list_targets_the_next_line() {
        let source = "<?php\n// @phpstan-ignore binaryOp.invalid\n$x = \"a\" + 1;\n";
        assert_eq!(
            ignored_identifiers(source),
            HashMap::from([(3, vec!["binaryOp.invalid".to_string()])])
        );
    }

    #[test]
    fn a_multiline_doc_comment_tag_skips_past_its_own_bare_closing_line() {
        let source = "<?php\n/**\n * @phpstan-ignore binaryOp.invalid\n */\n$x = \"a\" + 1;\n";
        assert_eq!(
            ignored_identifiers(source),
            HashMap::from([(5, vec!["binaryOp.invalid".to_string()])])
        );
    }

    #[test]
    fn multiple_identifiers_on_one_line_are_all_collected() {
        let source = "<?php\n$x = \"a\" + 1; // @phpstan-ignore binaryOp.invalid, argument.type\n";
        assert_eq!(
            ignored_identifiers(source),
            HashMap::from([(
                2,
                vec!["binaryOp.invalid".to_string(), "argument.type".to_string()]
            )])
        );
    }

    #[test]
    fn a_parenthesized_comment_after_an_identifier_is_discarded() {
        let source = "<?php\n$x = \"a\" + 1; // @phpstan-ignore binaryOp.invalid (known, tracked in JIRA-123)\n";
        assert_eq!(
            ignored_identifiers(source),
            HashMap::from([(2, vec!["binaryOp.invalid".to_string()])])
        );
    }

    #[test]
    fn a_malformed_identifier_list_is_silently_dropped() {
        let source = "<?php\n$x = \"a\" + 1; // @phpstan-ignore ,\n";
        assert!(ignored_identifiers(source).is_empty());
    }

    #[test]
    fn a_simple_line_marker_is_never_also_treated_as_an_identifier_list() {
        let source = "<?php\n$x = 1 / 0; // @phpstan-ignore-line\n";
        assert!(ignored_identifiers(source).is_empty());
    }

    #[test]
    fn no_phpstan_ignore_at_all_collects_nothing() {
        let source = "<?php\n$x = \"a\" + 1;\n";
        assert!(ignored_identifiers(source).is_empty());
    }
}
