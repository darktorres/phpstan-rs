//! Applying diagnostics' suggested fixes (`DiagnosticFix`) to source text.

use crate::{Diagnostic, DiagnosticFix};
use syntax::token::{Position, Span};

/// Byte offset of 1-based `position` (column in characters) in `source`,
/// or `None` past the end of its line or the text.
fn byte_offset(source: &str, position: Position) -> Option<usize> {
    let mut line_start = 0;
    for _ in 1..position.line {
        line_start += source[line_start..].find('\n')? + 1;
    }
    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |i| line_start + i);
    let line = &source[line_start..line_end];
    let col = position.col.checked_sub(1)? as usize;
    if col == line.chars().count() {
        return Some(line_end);
    }
    line.char_indices().nth(col).map(|(i, _)| line_start + i)
}

fn position_at(source: &str, offset: usize) -> Position {
    let before = &source[..offset];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    Position {
        line: before.matches('\n').count() as u32 + 1,
        col: source[line_start..offset].chars().count() as u32 + 1,
    }
}

/// The exact span `fix` edits in `source`: its own span, or for a `within`
/// fix the first whole-word occurrence of that text inside it.
pub fn resolve_span(source: &str, fix: &DiagnosticFix) -> Option<Span> {
    let Some(text) = &fix.within else {
        return Some(fix.span);
    };
    let start = byte_offset(source, fix.span.start)?;
    let end = byte_offset(source, fix.span.end)?;
    let region = source.get(start..end)?;
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let found = region.match_indices(text.as_str()).find(|(i, _)| {
        !region[..*i].chars().next_back().is_some_and(is_word)
            && !region[i + text.len()..].chars().next().is_some_and(is_word)
    })?;
    let from = start + found.0;
    Some(Span {
        start: position_at(source, from),
        end: position_at(source, from + text.len()),
    })
}

/// `source` with every fix applied, and how many were. A fix overlapping
/// one already applied, or pointing outside the text, is skipped.
pub fn apply_fixes<'d>(
    source: &str,
    fixes: impl IntoIterator<Item = &'d DiagnosticFix>,
) -> (String, usize) {
    let mut edits: Vec<(usize, usize, &str)> = fixes
        .into_iter()
        .filter_map(|fix| {
            let span = resolve_span(source, fix)?;
            let start = byte_offset(source, span.start)?;
            let end = byte_offset(source, span.end)?;
            (start <= end).then_some((start, end, fix.replacement.as_str()))
        })
        .collect();
    edits.sort_by_key(|(start, end, _)| (*start, *end));
    let mut applied: Vec<(usize, usize, &str)> = Vec::new();
    for edit in edits {
        if applied
            .last()
            .is_some_and(|(_, previous_end, _)| edit.0 < *previous_end)
            || applied.contains(&edit)
        {
            continue;
        }
        applied.push(edit);
    }
    let mut out = source.to_string();
    for (start, end, replacement) in applied.iter().rev() {
        out.replace_range(*start..*end, replacement);
    }
    (out, applied.len())
}

/// The fixes `diagnostics` carry.
pub fn fixes_of(diagnostics: &[Diagnostic]) -> impl Iterator<Item = &DiagnosticFix> {
    diagnostics.iter().filter_map(|d| d.fix.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(start: (u32, u32), end: (u32, u32), replacement: &str) -> DiagnosticFix {
        DiagnosticFix {
            span: Span {
                start: Position {
                    line: start.0,
                    col: start.1,
                },
                end: Position {
                    line: end.0,
                    col: end.1,
                },
            },
            replacement: replacement.to_string(),
            description: String::new(),
            within: None,
        }
    }

    #[test]
    fn fixes_apply_back_to_front_and_overlaps_are_skipped() {
        let source = "<?php\n$ä = foo();\nbar();\n";
        let fixes = [
            fix((2, 6), (2, 9), "Foo"),
            fix((3, 1), (3, 4), "Baz"),
            fix((3, 2), (3, 3), "x"),
            fix((9, 1), (9, 2), "out of range"),
        ];
        let (out, applied) = apply_fixes(source, &fixes);
        assert_eq!(out, "<?php\n$ä = Foo();\nBaz();\n");
        assert_eq!(applied, 2);
    }

    #[test]
    fn a_fix_can_delete_whole_lines_and_insert_at_a_point() {
        let source = "a\nb\nc\n";
        let (out, _) = apply_fixes(source, &[fix((2, 1), (3, 1), ""), fix((3, 1), (3, 1), "x")]);
        assert_eq!(out, "a\nxc\n");
    }

    #[test]
    fn a_within_fix_edits_only_the_first_whole_word_occurrence_in_its_span() {
        let source = "<?php\n$RUNNER->RUN();\n";
        let mut within = fix((2, 1), (2, 16), "run");
        within.within = Some("RUN".to_string());
        assert_eq!(
            resolve_span(source, &within),
            Some(Span {
                start: Position { line: 2, col: 10 },
                end: Position { line: 2, col: 13 },
            })
        );
        assert_eq!(apply_fixes(source, [&within]).0, "<?php\n$RUNNER->run();\n");
        within.within = Some("MISSING".to_string());
        assert_eq!(resolve_span(source, &within), None);
    }
}
