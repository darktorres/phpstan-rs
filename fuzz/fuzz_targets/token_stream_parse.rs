#![no_main]

//! Secondary fuzz target, lower priority than `lex_parse` - bypasses the
//! lexer entirely, feeding an `arbitrary`-derived token
//! stream straight into `Parser::new`/`parse_program`. Gated behind the
//! `syntax` crate's opt-in `fuzzing` feature (`Token`/`Keyword`/
//! `StringPart`/`Span` only derive `arbitrary::Arbitrary` under it),
//! so an ordinary build never pulls `arbitrary` in.
//!
//! `Span::default()` (real zeros) is used for every synthesized
//! token's own span rather than an `arbitrary`-derived one: `Parser`
//! only ever reads span *values* to build diagnostic positions, never to
//! make a control-flow decision, so a placeholder span can't hide a real
//! parser bug - it only means a real finding's own reported line/column
//! would be meaningless, irrelevant to what this harness looks for
//! (crashes/panics, not diagnostic accuracy). `Parser::at_eof`/`peek`
//! (`parser/tokens.rs`) fall back to `Token::Eof` purely from running
//! off the end of the token vector (`self.tokens.get(self.pos).
//! unwrap_or(&Token::Eof)`) - no trailing `Token::Eof` sentinel needs to
//! be appended here, and `arbitrary` is free to generate a literal
//! `Token::Eof` value anywhere mid-stream too (the parser then correctly
//! treats that position as an early "end of input", not a special case
//! this harness needs to avoid).
//!
//! Findings here need human triage before being treated with `lex_parse`'s
//! own urgency: a token sequence `arbitrary` can produce isn't
//! necessarily one the real lexer could ever actually emit (their real
//! producer, `tokenize_with_spans`, only ever emits certain token
//! *shapes* in certain contexts) - a panic found only through a
//! synthetic, lexer-unreachable sequence is a real robustness gap worth
//! fixing, but not the same severity as one `lex_parse` reaches from
//! real source bytes.

use libfuzzer_sys::fuzz_target;
use syntax::parser::Parser;
use syntax::span::Span;
use syntax::token::Token;

fuzz_target!(|tokens: Vec<Token>| {
    if tokens.len() > 2000 {
        // No silent cap on coverage otherwise - just a bound on how much
        // work one single fuzz iteration does, matching `arbitrary`'s
        // own general convention of bounding collection sizes so a
        // single malformed input can't make one iteration pathologically
        // slow.
        return;
    }
    let span = Span::default();
    let with_spans: Vec<(Token, Span, Option<String>)> =
        tokens.into_iter().map(|t| (t, span, None)).collect();
    let _ = Parser::new(with_spans).parse_program();
});
