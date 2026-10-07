//! `FileWhitespaceRule`: output a file sends before or after its PHP code
//! without meaning to. A leading UTF-8 BOM (`whitespace.bom`), a
//! whitespace-only run before the first `<?php` (`whitespace.fileStart`),
//! and a whitespace-only run after the last `?>` of the file, of a
//! namespace, or of a `declare` block (`whitespace.fileEnd`).

use crate::checker::Checker;
use syntax::ast::*;

const BOM: &[u8] = b"\xEF\xBB\xBF";

fn is_whitespace_run(html: &[u8]) -> bool {
    !html.is_empty()
        && std::str::from_utf8(html).is_ok_and(|text| text.chars().all(char::is_whitespace))
}

/// The last statement of the file, of each namespace, and of each
/// `declare(...) { }` block.
fn section_ends(arenas: &NodeArenas, program: &[StmtId]) -> Vec<StmtId> {
    let mut ends = Vec::new();
    for (i, &id) in program.iter().enumerate() {
        let Some(stmt) = arenas.stmt(id) else {
            continue;
        };
        if let StmtKind::Declare {
            body: Some(body), ..
        } = &stmt.kind
        {
            ends.extend(arenas.stmt_ids(body).last().copied());
        }
        if i > 0 && matches!(stmt.kind, StmtKind::Namespace(_)) {
            ends.push(program[i - 1]);
        }
    }
    ends.extend(program.last().copied());
    ends
}

impl<'a> Checker<'a> {
    pub(crate) fn check_file_whitespace(&mut self, arenas: &NodeArenas, program: &[StmtId]) {
        if let Some(first_id) = program.first().copied()
            && let Some(first) = arenas.stmt(first_id)
            && let StmtKind::InlineHtml(html_span) = &first.kind
        {
            let (has_bom, starts_with_whitespace) = self
                .syntax_source_slice(*html_span)
                .map(|html| {
                    let after_bom = html.strip_prefix(BOM);
                    (
                        after_bom.is_some(),
                        program.len() > 1 && is_whitespace_run(after_bom.unwrap_or(html)),
                    )
                })
                .unwrap_or((false, false));
            if has_bom {
                self.report("File begins with UTF-8 BOM character. This may cause problems when running the code in the web browser."
                        .to_string(),
                    arenas.stmt_span(first.id).expect("first statement has a parallel span"),
                    0,
                    "whitespace.bom",
                );
            }
            if starts_with_whitespace {
                self.report("File begins with whitespace before the opening <?php tag. This may cause problems when running the code in the web browser."
                        .to_string(),
                    arenas.stmt_span(first.id).expect("first statement has a parallel span"),
                    0,
                    "whitespace.fileStart",
                );
            }
        }
        let mut reported = Vec::new();
        for end_id in section_ends(arenas, program) {
            let Some(end) = arenas.stmt(end_id) else {
                continue;
            };
            let StmtKind::InlineHtml(html_span) = &end.kind else {
                continue;
            };
            let Some(html) = self.syntax_source_slice(*html_span) else {
                continue;
            };
            if !is_whitespace_run(html)
                || program.first() == Some(&end_id)
                || reported.contains(&end_id)
            {
                continue;
            }
            reported.push(end_id);
            self.report("File ends with a trailing whitespace. This may cause problems when running the code in the web browser. Remove the closing ?> mark or remove the whitespace."
                    .to_string(),
                arenas.stmt_span(end.id).expect("HTML statement has a parallel span"),
                0,
                "whitespace.fileEnd",
            );
        }
    }
}
