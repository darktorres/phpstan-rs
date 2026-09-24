//! `textDocument/semanticTokens/full` - encodes `Checker::
//! take_semantic_tokens`'s flat `UsageSemanticToken` list into the LSP
//! wire format's delta-encoded `u32` array. See `checker::semantic_tokens`'s
//! own module doc comment for how each token's `deprecated`/`internal`
//! bits are computed; this module only handles legend/index consistency
//! and the encoding arithmetic itself.

use lsp_types::{SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend};

use crate::checker::semantic_tokens::{UsageSemanticToken, UsageTokenKind};

const DEPRECATED_BIT: u32 = 1 << 0;
const INTERNAL_BIT: u32 = 1 << 1;

/// Kept in sync with `token_type_index` by construction - every index
/// `token_type_index` can return must have a same-position entry here.
pub(super) fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::FUNCTION,
            SemanticTokenType::METHOD,
            SemanticTokenType::PROPERTY,
            SemanticTokenType::CLASS,
        ],
        token_modifiers: vec![
            SemanticTokenModifier::DEPRECATED,
            SemanticTokenModifier::new("internal"),
        ],
    }
}

fn token_type_index(kind: UsageTokenKind) -> u32 {
    match kind {
        UsageTokenKind::Function => 0,
        UsageTokenKind::Method => 1,
        UsageTokenKind::Property | UsageTokenKind::ClassConstant => 2,
        UsageTokenKind::Class => 3,
    }
}

/// Sorts `tokens` by position, defensively merges any two tokens sharing
/// the exact same span (OR-ing their modifier bits rather than emitting
/// a wire-format-violating overlapping pair - currently unexercised by
/// any known call site, kept as cheap insurance), drops any token whose
/// span still crosses a line boundary (`record_usage_semantic_token`'s
/// own choke point already filters this - re-checked here as the true
/// last point before the wire format), then delta-encodes: each token's
/// `delta_line`/`delta_start` is relative to the previous token's own
/// start (0 for the very first token; `delta_start` resets to an
/// absolute column whenever `delta_line` is nonzero, per the LSP spec).
pub(super) fn encode(mut tokens: Vec<UsageSemanticToken>) -> Vec<SemanticToken> {
    tokens.retain(|t| t.span.start.line == t.span.end.line);
    tokens.sort_by_key(|t| (t.span.start.line, t.span.start.col));
    tokens.dedup_by(|next, prev| {
        if prev.span.start.line == next.span.start.line
            && prev.span.start.col == next.span.start.col
            && prev.span.end.col == next.span.end.col
        {
            prev.deprecated |= next.deprecated;
            prev.internal |= next.internal;
            true
        } else {
            false
        }
    });

    let mut result = Vec::with_capacity(tokens.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;
    for (i, token) in tokens.iter().enumerate() {
        let line = token.span.start.line.saturating_sub(1);
        let start = token.span.start.col.saturating_sub(1);
        let (delta_line, delta_start) = if i == 0 {
            (line, start)
        } else if line == prev_line {
            (0, start - prev_start)
        } else {
            (line - prev_line, start)
        };
        let mut bitset = 0u32;
        if token.deprecated {
            bitset |= DEPRECATED_BIT;
        }
        if token.internal {
            bitset |= INTERNAL_BIT;
        }
        result.push(SemanticToken {
            delta_line,
            delta_start,
            length: token.span.end.col.saturating_sub(token.span.start.col),
            token_type: token_type_index(token.kind),
            token_modifiers_bitset: bitset,
        });
        prev_line = line;
        prev_start = start;
    }
    result
}
