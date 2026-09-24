//! `FileWhitespaceRule`: output a file sends before or after its PHP code
//! without meaning to. A leading UTF-8 BOM (`whitespace.bom`), a
//! whitespace-only run before the first `<?php` (`whitespace.fileStart`),
//! and a whitespace-only run after the last `?>` of the file, of a
//! namespace, or of a `declare` block (`whitespace.fileEnd`).

use crate::ast::*;
use crate::checker::Checker;

const BOM: &str = "\u{feff}";

fn is_whitespace_run(html: &str) -> bool {
    !html.is_empty() && html.chars().all(char::is_whitespace)
}

/// The last statement of the file, of each namespace, and of each
/// `declare(...) { }` block.
fn section_ends(program: &[Stmt]) -> Vec<&Stmt> {
    let mut ends = Vec::new();
    for (i, stmt) in program.iter().enumerate() {
        if let StmtKind::Block(body) = &stmt.kind
            && matches!(
                body.first().map(|s| &s.kind),
                Some(StmtKind::Declare {
                    block_mode: true,
                    ..
                })
            )
            && body.len() > 1
        {
            ends.extend(body.last());
        }
        if i > 0 && matches!(stmt.kind, StmtKind::Namespace(_)) {
            ends.push(&program[i - 1]);
        }
    }
    ends.extend(program.last());
    ends
}

impl<'a> Checker<'a> {
    pub(crate) fn check_file_whitespace(&mut self, program: &[Stmt]) {
        if let Some(first) = program.first()
            && let StmtKind::InlineHtml(html) = &first.kind
        {
            let after_bom = html.strip_prefix(BOM);
            if after_bom.is_some() {
                self.report(
                    "<main>".to_string(),
                    "File begins with UTF-8 BOM character. This may cause problems when running the code in the web browser."
                        .to_string(),
                    first.span,
                    0,
                    "whitespace.bom",
                );
            }
            if program.len() > 1 && is_whitespace_run(after_bom.unwrap_or(html)) {
                self.report(
                    "<main>".to_string(),
                    "File begins with whitespace before the opening <?php tag. This may cause problems when running the code in the web browser."
                        .to_string(),
                    first.span,
                    0,
                    "whitespace.fileStart",
                );
            }
        }
        let mut reported = Vec::new();
        for end in section_ends(program) {
            let StmtKind::InlineHtml(html) = &end.kind else {
                continue;
            };
            if !is_whitespace_run(html)
                || std::ptr::eq(end, &program[0])
                || reported.iter().any(|r| std::ptr::eq(*r, end))
            {
                continue;
            }
            reported.push(end);
            self.report(
                "<main>".to_string(),
                "File ends with a trailing whitespace. This may cause problems when running the code in the web browser. Remove the closing ?> mark or remove the whitespace."
                    .to_string(),
                end.span,
                0,
                "whitespace.fileEnd",
            );
        }
    }
}
