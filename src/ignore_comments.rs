//! Phase 44 (partial): `@phpstan-ignore-line`/`@phpstan-ignore-next-line`
//! inline suppression - real PHPStan's own two "simple line marker" forms
//! of `@phpstan-ignore`, distinct from the fuller bare `@phpstan-ignore
//! <identifier list>` form (which needs real per-identifier matching
//! against a diagnostic's own `identifier`, plus real PHPStan's own
//! genuinely tricky "does this comment trail real code on its own line, or
//! stand alone and so apply to the *next* line of real code" placement
//! logic - left as a separate, still-open follow-up; see ROADMAP.md).
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

use std::collections::HashSet;

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
        // The fuller identifier-list form is a deliberately separate,
        // still-open follow-up - this scan must not mistake it for either
        // simple line marker.
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
}
