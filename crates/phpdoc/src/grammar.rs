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
    ArrayShapeField, CallableParam, CallableTemplate, Description, MethodTagParam, PhpDocNode,
    PhpDocParseError, PhpDocTag, PhpDocTagValue, ShapeKey, TypeNode,
};

/// Parses one `/** ... */` doc-comment's own text (`raw` includes the
/// `/**`/`*/` delimiters, matching `docblock::parse`'s own calling
/// convention exactly - a caller migrating from one to the other passes
/// the identical string).
pub fn parse(raw: &str) -> PhpDocNode {
    parser::parse(raw)
}

fn shared() -> &'static syntax::memo::Memo<PhpDocNode> {
    static MEMO: std::sync::LazyLock<syntax::memo::Memo<PhpDocNode>> =
        std::sync::LazyLock::new(syntax::memo::Memo::new);
    &MEMO
}

/// `parse`, kept by the text: the node of a doc comment parsed before is
/// the same one, shared.
pub fn parse_shared(raw: &str) -> std::sync::Arc<PhpDocNode> {
    shared().get(raw, parse)
}

/// How `parse_shared` has been asked so far.
pub fn parse_shared_stats() -> crate::MemoStats {
    shared().stats()
}

/// See `parser::doctrine_argument_errors`'s own doc comment.
pub(crate) use parser::doctrine_argument_errors;
/// See `parser::parse_tag_rest`'s own doc comment.
pub(crate) use parser::parse_tag_rest;

/// `parse_tag_rest`, kept by the tag's name and text: the same tags -
/// `@param int $x`, `@return void`, `@var string` - are in docblocks of
/// every file, and each is parsed from its text alone.
pub(crate) fn parse_tag_rest_shared(
    tag: &str,
    rest: &str,
) -> (PhpDocTagValue, Vec<PhpDocParseError>) {
    static MEMO: std::sync::LazyLock<syntax::memo::Memo<(PhpDocTagValue, Vec<PhpDocParseError>)>> =
        std::sync::LazyLock::new(|| syntax::memo::Memo::with_limits(1024, 256 * 1024));
    // The tag's name cannot hold a NUL, which separates it from its text.
    let key = format!("{tag}\0{rest}");
    let found = MEMO.get(&key, |key| {
        let (tag, rest) = key.split_once('\0').expect("the key holds its separator");
        parse_tag_rest(tag, rest)
    });
    (*found).clone()
}

/// `parse_type_prefix`, kept by its text: the type and the errors the
/// text has, and where in it the type ends.
pub fn parse_type_prefix_shared(rest: &str) -> (Option<TypeNode>, &str, Vec<PhpDocParseError>) {
    type Prefix = (Option<TypeNode>, usize, Vec<PhpDocParseError>);
    static MEMO: std::sync::LazyLock<syntax::memo::Memo<Prefix>> =
        std::sync::LazyLock::new(|| syntax::memo::Memo::with_limits(1024, 256 * 1024));
    let found = MEMO.get(rest, |rest| {
        let (ty, remainder, errors) = parse_type_prefix(rest);
        (ty, rest.len() - remainder.len(), errors)
    });
    let (ty, at, errors) = &*found;
    (ty.clone(), &rest[*at..], errors.clone())
}
/// See `parser::parse_type_prefix`'s own doc comment.
pub use parser::parse_type_prefix;

#[cfg(test)]
mod tests;
