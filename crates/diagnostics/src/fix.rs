//! Applying diagnostics' suggested fixes (`DiagnosticFix`) to source text.

use crate::{Diagnostic, DiagnosticFix};
use syntax::span::Span;

/// The exact span `fix` edits in `source`: its own span, or for a `within`
/// fix the first whole-word occurrence of that text inside it.
pub fn resolve_span(source: &[u8], fix: &DiagnosticFix) -> Option<Span> {
    let Some(text) = &fix.within else {
        return Some(fix.span);
    };
    let region = std::str::from_utf8(fix.span.slice(source)?).ok()?;
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let found = region.match_indices(text.as_str()).find(|(i, _)| {
        !region[..*i].chars().next_back().is_some_and(is_word)
            && !region[i + text.len()..].chars().next().is_some_and(is_word)
    })?;
    let from = fix.span.start + found.0 as u32;
    Some(Span {
        start: from,
        end: from + text.len() as u32,
    })
}

/// `source` with every fix applied, and how many were. A fix overlapping
/// one already applied, or pointing outside the text, is skipped.
pub fn apply_fixes<'d>(
    source: &[u8],
    fixes: impl IntoIterator<Item = &'d DiagnosticFix>,
) -> (Vec<u8>, usize) {
    let mut edits: Vec<(usize, usize, &str)> = fixes
        .into_iter()
        .filter_map(|fix| {
            let span = resolve_span(source, fix)?;
            let (start, end) = (span.start as usize, span.end as usize);
            (start <= end && end <= source.len()).then_some((start, end, fix.replacement.as_str()))
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
    let mut out = source.to_vec();
    for (start, end, replacement) in applied.iter().rev() {
        out.splice(*start..*end, replacement.bytes());
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

    fn fix(start: usize, end: usize, replacement: &str) -> DiagnosticFix {
        DiagnosticFix {
            span: Span {
                start: start as u32,
                end: end as u32,
            },
            replacement: replacement.to_string(),
            description: String::new(),
            within: None,
        }
    }

    /// The fix replacing `text` where it first occurs in `source`.
    fn fix_of(source: &str, text: &str, replacement: &str) -> DiagnosticFix {
        let start = source.find(text).expect("the text is in the source");
        fix(start, start + text.len(), replacement)
    }

    #[test]
    fn fixes_apply_back_to_front_and_overlaps_are_skipped() {
        let source = "<?php\n$ä = foo();\nbar();\n";
        let fixes = [
            fix_of(source, "foo", "Foo"),
            fix_of(source, "bar", "Baz"),
            fix_of(source, "a", "x"),
            fix(900, 901, "out of range"),
        ];
        let (out, applied) = apply_fixes(source.as_bytes(), &fixes);
        assert_eq!(out, "<?php\n$ä = Foo();\nBaz();\n".as_bytes());
        assert_eq!(applied, 2);
    }

    #[test]
    fn a_fix_can_delete_whole_lines_and_insert_at_a_point() {
        let source = "a\nb\nc\n";
        let (out, _) = apply_fixes(source.as_bytes(), &[fix(2, 4, ""), fix(4, 4, "x")]);
        assert_eq!(out, b"a\nxc\n");
    }

    #[test]
    fn a_fix_edits_bytes_that_are_not_utf8() {
        let source = b"a \xff b";
        let (out, applied) = apply_fixes(source, &[fix(2, 3, "x")]);
        assert_eq!((out, applied), (b"a x b".to_vec(), 1));
    }

    #[test]
    fn a_within_fix_edits_only_the_first_whole_word_occurrence_in_its_span() {
        let source = "<?php\n$RUNNER->RUN();\n";
        let start = source.find("$RUNNER").unwrap();
        let mut within = fix(start, start + 15, "run");
        within.within = Some("RUN".to_string());
        let run = source.find("->RUN").unwrap() + 2;
        assert_eq!(
            resolve_span(source.as_bytes(), &within),
            Some(Span {
                start: run as u32,
                end: run as u32 + 3,
            })
        );
        assert_eq!(
            apply_fixes(source.as_bytes(), [&within]).0,
            "<?php\n$RUNNER->run();\n".as_bytes()
        );
        within.within = Some("MISSING".to_string());
        assert_eq!(resolve_span(source.as_bytes(), &within), None);
    }
}
