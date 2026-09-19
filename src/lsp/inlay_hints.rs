//! `textDocument/inlayHint` - parameter-name hints for call arguments
//! (`greet(name: "hi")`-style labels painted in front of a positional
//! argument) and inferred-type hints for an unannotated local-variable
//! assignment, both collected in one `Checker::check_program` pass via
//! the `collect_inlay_hints` gate. Split into `collect` (the expensive,
//! whole-file part, cached by `Server` on a per-file version counter)
//! and `filter_to_range` (the cheap per-request mapping/filtering), so a
//! repeated request against an unchanged document costs no extra pass.

use std::path::Path;
use std::sync::Arc;

use lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Range as LspRange};

use crate::project::Project;
use crate::token::Span;
use crate::types::Type;

use super::protocol::to_lsp_position;

#[allow(clippy::type_complexity)]
pub(super) fn collect(
    project: &Project,
    path: &Path,
) -> Option<(Vec<(Span, Arc<str>)>, Vec<(Span, Type)>)> {
    let (mut checker, ast) = project.checker_for(path)?;
    checker.collect_inlay_hints = true;
    checker.check_program(ast);
    Some((
        checker.inlay_param_name_hints,
        checker.inlay_local_type_hints,
    ))
}

pub(super) fn filter_to_range(
    param_hints: &[(Span, Arc<str>)],
    local_hints: &[(Span, Type)],
    range: LspRange,
) -> Vec<InlayHint> {
    let mut hints = Vec::new();
    for (span, name) in param_hints {
        let position = to_lsp_position(span.start);
        if position < range.start || position > range.end {
            continue;
        }
        hints.push(InlayHint {
            position,
            label: InlayHintLabel::String(format!("{name}:")),
            kind: Some(InlayHintKind::PARAMETER),
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: Some(true),
            data: None,
        });
    }
    for (span, ty) in local_hints {
        let position = to_lsp_position(span.end);
        if position < range.start || position > range.end {
            continue;
        }
        hints.push(InlayHint {
            position,
            label: InlayHintLabel::String(format!(": {ty}")),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: Some(true),
            padding_right: None,
            data: None,
        });
    }
    hints
}
