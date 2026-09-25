//! A tokenizer and recursive-descent parser for PHPDoc content: tag
//! structure and the type-expression grammar, with recoverable-error
//! tracking (`PhpDocNode::errors`) - real PHPStan's `phpDoc.parseError`.
//! `docblock` builds its `DocBlock` from this parse.
//!
//! Narrower than real PHPStan's full PHPDoc grammar: `@method`'s
//! default-value expressions are skipped rather than parsed as their own
//! sub-grammar (PHP expression syntax, not a type), and a handful of
//! rarer shapes (nested conditional-type `is`-chains inside a template's
//! bound, PHPStan's newer `WhereType` constraint syntax) aren't modeled.

mod ast;
mod lexer;
mod parser;

pub use ast::{
    ArrayShapeField, CallableParam, CallableTemplate, MethodTagParam, PhpDocNode, PhpDocParseError,
    PhpDocTag, PhpDocTagValue, ShapeKey, TypeNode,
};

/// Parses one `/** ... */` doc-comment's own text (`raw` includes the
/// `/**`/`*/` delimiters, matching `docblock::parse`'s own calling
/// convention exactly - a caller migrating from one to the other passes
/// the identical string).
pub fn parse(raw: &str) -> PhpDocNode {
    parser::parse(raw)
}

/// See `parser::doctrine_argument_errors`'s own doc comment.
pub(crate) use parser::doctrine_argument_errors;
/// See `parser::parse_tag_rest`'s own doc comment.
pub(crate) use parser::parse_tag_rest;
/// See `parser::parse_type_prefix`'s own doc comment.
pub use parser::parse_type_prefix;

#[cfg(test)]
mod tests;
