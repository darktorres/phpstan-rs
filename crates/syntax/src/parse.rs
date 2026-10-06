//! Parsing a whole file: lexing, parsing and name resolution, with what the
//! lexer found besides the tree.

use std::sync::Arc;

use crate::ast::{GotoLabel, Stmt};
pub use crate::lexer::FileFacts;
use crate::lexer::{IgnoreMark, LexEnd, LexWarning, Lexer, WatchList};
use crate::name::{Name, NameCache, NameId};
use crate::parser::{ParseError, Parser};
use crate::references::FileReferences;
use crate::span::LineIndex;

/// A parsed file.
#[derive(Debug)]
pub struct ParsedFile {
    /// The bytes this tree was parsed from. AST payloads will become ranges
    /// into this buffer as the tree moves to typed arenas.
    pub source: Box<[u8]>,
    /// The distinct names encountered in the file, in first-use order.
    pub names: Box<[Name]>,
    pub stmts: Vec<Stmt>,
    /// Every comment that holds `@phpstan-ignore`, doc comments included,
    /// in source order, with what the code next to it is.
    pub ignore_marks: Vec<IgnoreMark>,
    /// The file's lines, for the positions of its offsets.
    pub line_index: Arc<LineIndex>,
    /// What the lexer skipped over.
    pub warnings: Vec<LexWarning>,
    pub facts: FileFacts,
    /// The labels the file's own top-level `goto`s name.
    pub goto_labels: Box<[GotoLabel]>,
    /// What the file refers to.
    pub references: FileReferences,
}

impl ParsedFile {
    /// Reads an in-file byte range, such as a source-backed AST payload.
    pub fn source_slice(&self, span: crate::span::Span) -> Option<&[u8]> {
        self.source.get(span.start as usize..span.end as usize)
    }

    /// Resolves a file-local name id to the shared interned name.
    pub fn name(&self, id: NameId) -> Option<&Name> {
        self.names.get(u32::from(id.into_raw()) as usize)
    }
}

/// A file's source: text already known to be UTF-8, or bytes, which are
/// checked once and read lossily where they are not.
#[derive(Debug, Clone, Copy)]
pub enum Source<'a> {
    Text(&'a str),
    Bytes(&'a [u8]),
}

impl<'a> From<&'a str> for Source<'a> {
    fn from(text: &'a str) -> Self {
        Source::Text(text)
    }
}

impl<'a> From<&'a String> for Source<'a> {
    fn from(text: &'a String) -> Self {
        Source::Text(text)
    }
}

impl<'a> From<&'a [u8]> for Source<'a> {
    fn from(bytes: &'a [u8]) -> Self {
        Source::Bytes(bytes)
    }
}

impl<'a> From<&'a Vec<u8>> for Source<'a> {
    fn from(bytes: &'a Vec<u8>) -> Self {
        Source::Bytes(bytes)
    }
}

impl<'a, const N: usize> From<&'a [u8; N]> for Source<'a> {
    fn from(bytes: &'a [u8; N]) -> Self {
        Source::Bytes(bytes)
    }
}

impl<'a> Lexer<'a> {
    /// A lexer over `source`: nothing is validated for text.
    pub fn from_source(source: Source<'a>) -> Self {
        match source {
            Source::Text(text) => Lexer::new(text),
            Source::Bytes(bytes) => Lexer::from_bytes(bytes),
        }
    }
}

/// Lexes and parses `source`, interning its names.
pub fn parse<'a>(source: impl Into<Source<'a>>) -> Result<ParsedFile, ParseError> {
    parse_with_names(source, None)
}

/// `parse`, with the names the files of a load share: the cache is asked
/// for a name before the interner is.
pub fn parse_with_names<'a>(
    source: impl Into<Source<'a>>,
    names: Option<&mut NameCache>,
) -> Result<ParsedFile, ParseError> {
    parse_watching(source, names, None)
}

/// `parse_with_names`, noting in the file's facts which of the spellings
/// of `watch` the file names.
pub fn parse_watching<'a>(
    source: impl Into<Source<'a>>,
    names: Option<&mut NameCache>,
    watch: Option<&'static WatchList>,
) -> Result<ParsedFile, ParseError> {
    let source = source.into();
    let source_bytes: Box<[u8]> = match source {
        Source::Text(text) => text.as_bytes().into(),
        Source::Bytes(bytes) => bytes.into(),
    };
    let lexer = Lexer::from_source(source);
    let lexer = match watch {
        Some(watch) => lexer.watching(watch),
        None => lexer,
    };
    let lexer = match names {
        Some(cache) => lexer.with_name_cache(cache),
        None => lexer,
    };
    let mut parser = Parser::from_lexer(lexer);
    let parsed = parser.parse_program();
    let has_anonymous_class = parser.saw_anonymous_class();
    let has_nested_declaration = parser.saw_nested_declaration();
    let unsets_property = parser.saw_property_unset();
    let syntax_marks = parser.syntax_marks();
    let goto_labels = parser.file_goto_labels();
    let references = parser.take_references();
    // The rest of the file is lexed even when the parse failed: a lexer
    // error anywhere in the file is the file's error, before a syntax
    // error found in the part the parse read.
    let LexEnd {
        ignore_marks,
        warnings,
        mut facts,
        line_index,
        names,
    } = parser
        .into_lex_end()
        .expect("a parser of a lexer ends with the lexer's record");
    if let Some(error) = warnings.iter().find(|w| w.is_error) {
        return Err(ParseError {
            message: error.message.clone(),
            span: error.span,
        });
    }
    let stmts = parsed?;
    facts.has_anonymous_class = has_anonymous_class;
    facts.has_nested_declaration = has_nested_declaration;
    facts.unsets_property = unsets_property;
    facts.syntax.merge(&syntax_marks);
    Ok(ParsedFile {
        source: source_bytes,
        names,
        stmts,
        ignore_marks,
        line_index,
        warnings,
        facts,
        goto_labels,
        references,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_zero_literal_with_an_8_or_9_is_a_parse_error() {
        let error = parse(b"<?php $x = 08;").unwrap_err();
        assert_eq!(error.message, "Invalid numeric literal");
        assert!(parse(b"<?php $x = 07; $y = 09.5;").is_ok());
    }

    #[test]
    fn a_number_or_escape_php_cant_read_whole_is_a_parse_error() {
        for bad in [
            &b"<?php $x = 1e;"[..],
            b"<?php $x = 1e+;",
            b"<?php $x = 0x;",
            b"<?php $x = 0b;",
            b"<?php $x = 1__0;",
            b"<?php $x = 1_;",
            b"<?php $x = 1_.5;",
            b"<?php $x = \"\\u{110000}\";",
            b"<?php $x = \"\\u{zz}\";",
            b"<?php $x = match (1) { default => 1, default => 2 };",
            b"<?php $x = new final class {};",
            b"<?php $x = new class extends A, B {};",
            b"<?php $x = [...$a => 1];",
            b"<?php $x = ['k' => ...$a];",
            b"<?php $x = isset();",
            b"<?php $x = \"{$a b}\";",
            b"<?php $x = [1, , 2];",
            b"<?php f([, $a]);",
            b"<?php $x = array(1, , 2);",
            b"<?php $x = list($a);",
            b"<?php f(list($a));",
            b"<?php [] = $x;",
            b"<?php [, ] = $x;",
            b"<?php list() = $x;",
            b"<?php [...$a] = $x;",
            b"<?php [1 => $a, $b] = $x;",
            b"<?php ['a' => $a, , ] = $x;",
            b"<?php [$a, list($b)] = $x;",
            b"<?php list($a, [$b]) = $x;",
            b"<?php [$a, [, ]] = $x;",
            b"<?php [array($a)] = $x;",
            b"<?php array($a) = $x;",
            b"<?php [$a] += $x;",
            b"<?php [$a] =& $x;",
            b"<?php list($a) ??= $x;",
            b"<?php ($a) = 1;",
            b"<?php foreach ($x as [$a] => $v) {}",
            b"<?php foreach ($x as list($a) => $v) {}",
            b"<?php foreach ($x as &$k => $v) {}",
            b"<?php foreach ($x as &[$a]) {}",
            b"<?php foreach ($x as array($a)) {}",
            b"<?php $s = \"{$x[, ]}\";",
        ] {
            assert!(parse(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
        for good in [
            &b"<?php $x = 1_000.5_5e1_0;"[..],
            b"<?php $x = 0x1_F;",
            b"<?php $x = .5; $y = 1.; $z = 1E-3;",
            b"<?php $x = \"\\u{D800}\\u{10FFFF}\";",
            b"<?php function f() { $x = $b ? yield : 2; $y = yield ?? 1; }",
            b"<?php $x = [&$a, 'k' => &$b];",
            b"<?php [$a, , $b] = $x; [, $a, , ] = $x; [$a, ] = $x;",
            b"<?php list($a, list($b)) = $x; list($a, ) = $x; [[$a], [$b]] = $x;",
            b"<?php ['a' => [$x], 'b' => &$y] = $z; [&$a, $b] = $x;",
            b"<?php $x = [$a] = [1]; [$a] = [1] = [2]; ($a)[0] = 1;",
            b"<?php foreach ($x as [$a, [, $b]]) {} foreach ($x as $k => list($a)) {}",
            b"<?php foreach ($x as [&$a]) {} foreach ($x as &$v) {}",
        ] {
            assert!(parse(good).is_ok(), "{}", String::from_utf8_lossy(good));
        }
    }

    #[test]
    fn files_parsed_with_a_shared_name_cache_give_the_trees_and_names_they_give_without() {
        let first =
            "<?php namespace App; class Foo { public function bar($baz) { return $baz->qux(); } }";
        let second = "<?php namespace App; function f(Foo $baz) { return (new Foo)->bar($baz); }";
        let mut cache = NameCache::default();
        let shared: Vec<_> = [first, second]
            .iter()
            .map(|source| {
                format!(
                    "{:?}",
                    parse_with_names(*source, Some(&mut cache)).unwrap().stmts
                )
            })
            .collect();
        let alone: Vec<_> = [first, second]
            .iter()
            .map(|source| format!("{:?}", parse(*source).unwrap().stmts))
            .collect();
        assert_eq!(shared, alone);
        // What the cache holds is what the interner gives.
        let interner = crate::name::Interner::global();
        for text in ["Foo", "bar", "baz", "qux"] {
            let found = cache.find(text, crate::name::text_hash(text)).expect(text);
            assert!(found.same_allocation(&interner.name(text)), "{text}");
        }
        assert!(!cache.is_empty());
    }
}
