//! Phase 56, Stage A: a real tokenizer + recursive-descent parser for
//! PHPDoc content - `docblock.rs`'s existing hand-rolled pipeline
//! (line-splitting + `take_type_token`'s bracket-depth-only scan,
//! deferring real type-string parsing to a separate, later `types::
//! parse_type_string` call) never actually validates a type expression's
//! own internal grammar; a malformed one (a stray unclosed generic
//! bracket, say) silently produces whatever substring `take_type_token`
//! happened to stop at, with no diagnostic at all. This module parses
//! the type-expression grammar inline, alongside tag structure, with
//! real recoverable-error tracking (`PhpDocNode::errors`) - matching
//! real PHPStan's own `phpDoc.parseError`.
//!
//! Deliberately additive only, not yet load-bearing: nothing in
//! `checker/`/`registry/` consumes this module's own `PhpDocNode` yet -
//! `docblock::parse`/`types::parse_type_string` remain the real,
//! consumed pipeline. Migrating `DocBlock` itself to be populated from
//! this parser (re-stringifying parsed types back into `DocBlock`'s own
//! `Option<String>` fields for full backward compatibility, then
//! retiring the old hand-rolled tokenizer once nothing calls it anymore)
//! is Stage B - a real, separate, higher-risk migration this pass
//! deliberately doesn't attempt, since it touches the single most
//! heavily-consumed subsystem in the whole checker and needs its own
//! full Piwigo corpus diff to verify no silent behavior regression -
//! the one phase in the whole "Group 1" plan where that's a real, live
//! risk (every other phase there was purely additive).
//!
//! Deliberately narrower than real PHPStan's own full PHPDoc grammar:
//! `@method`'s own default-value expressions are skipped rather than
//! parsed as their own sub-grammar (real PHP expression syntax, not a
//! type), and a handful of rarer shapes (nested conditional-type
//! `is`-chains inside a template's own bound, PHPStan's newer `WhereType`
//! constraint syntax) aren't modeled - a disclosed scope limit, not a
//! silent one.

mod ast;
mod lexer;
mod parser;

pub use ast::{
    ArrayShapeField, CallableParam, MethodTagParam, PhpDocNode, PhpDocParseError, PhpDocTag,
    PhpDocTagValue, ShapeKey, TypeNode,
};

/// Parses one `/** ... */` doc-comment's own text (`raw` includes the
/// `/**`/`*/` delimiters, matching `docblock::parse`'s own calling
/// convention exactly - a caller migrating from one to the other passes
/// the identical string).
pub fn parse(raw: &str) -> PhpDocNode {
    parser::parse(raw)
}

#[cfg(test)]
mod tests;
