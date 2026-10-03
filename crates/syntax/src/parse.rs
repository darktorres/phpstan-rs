//! Parsing a whole file: lexing, parsing and name resolution, with what the
//! lexer found besides the tree.

use std::sync::Arc;

use crate::ast::{GotoLabel, Stmt};
use crate::lexer::{IgnoreMark, LexOutput, LexWarning, Lexer};
use crate::name::{Interner, NameCache};
use crate::parser::{ParseError, Parser};
use crate::span::LineIndex;
use crate::token::Token;

/// What is true of a file as a whole, found as it is lexed and parsed, so
/// that a walk over its tree that can only find something in a file where
/// the fact holds can be left out of the others.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileFacts {
    /// The file has an anonymous class (`new class`).
    pub has_anonymous_class: bool,
    /// The file names `bind`, `bindTo` or `call`, in any case: a closure
    /// rebind (`Closure::bind`, `->bindTo()`, `->call()`) needs one of
    /// them.
    pub mentions_closure_rebind: bool,
    /// The file names one of the attributes the stub readers act on
    /// (`PhpStormStubsElementAvailable`, `LanguageLevelTypeAware`,
    /// `PhpVersionAware`), in any case, however it imports them.
    pub mentions_stub_attribute: bool,
}

impl FileFacts {
    /// The facts a token list gives: what the file names. An identifier
    /// is a whole name, or one segment of a qualified one.
    fn of_tokens(tokens: &[Token]) -> FileFacts {
        let mut facts = FileFacts::default();
        for token in tokens {
            let Token::Identifier(name) = token else {
                continue;
            };
            let name = name.as_str();
            let is_one_of = |names: &[&str]| {
                names
                    .iter()
                    .any(|n| name.len() == n.len() && name.eq_ignore_ascii_case(n))
            };
            facts.mentions_closure_rebind |= is_one_of(&["bind", "call", "bindto"]);
            facts.mentions_stub_attribute |= is_one_of(&[
                "PhpVersionAware",
                "LanguageLevelTypeAware",
                "PhpStormStubsElementAvailable",
            ]);
        }
        facts
    }
}

/// A parsed file.
#[derive(Debug)]
pub struct ParsedFile {
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

/// Lexes and parses `source`, interning its names in `interner`.
pub fn parse<'a>(
    source: impl Into<Source<'a>>,
    interner: &Arc<Interner>,
) -> Result<ParsedFile, ParseError> {
    parse_with_names(source, interner, None)
}

/// `parse`, with the names the files of a load share: the cache is asked
/// for a name before the interner is.
pub fn parse_with_names<'a>(
    source: impl Into<Source<'a>>,
    interner: &Arc<Interner>,
    names: Option<&mut NameCache>,
) -> Result<ParsedFile, ParseError> {
    let lexer = Lexer::from_source(source.into()).with_interner(interner);
    let lexer = match names {
        Some(cache) => lexer.with_name_cache(cache),
        None => lexer,
    };
    let LexOutput {
        lexed,
        ignore_marks,
        warnings,
        ..
    } = lexer.lex();
    if let Some(error) = warnings.iter().find(|w| w.is_error) {
        return Err(ParseError {
            message: error.message.clone(),
            span: error.span,
        });
    }
    let line_index = Arc::clone(&lexed.line_index);
    let mut facts = FileFacts::of_tokens(&lexed.tokens);
    let mut parser = Parser::from_lexed(lexed).with_interner(interner);
    let stmts = parser.parse_program()?;
    facts.has_anonymous_class = parser.saw_anonymous_class();
    let goto_labels = parser.file_goto_labels();
    Ok(ParsedFile {
        stmts,
        ignore_marks,
        line_index,
        warnings,
        facts,
        goto_labels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_zero_literal_with_an_8_or_9_is_a_parse_error() {
        let interner = Arc::clone(Interner::global());
        let error = parse(b"<?php $x = 08;", &interner).unwrap_err();
        assert_eq!(error.message, "Invalid numeric literal");
        assert!(parse(b"<?php $x = 07; $y = 09.5;", &interner).is_ok());
    }

    #[test]
    fn a_number_or_escape_php_cant_read_whole_is_a_parse_error() {
        let interner = Arc::clone(Interner::global());
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
            assert!(
                parse(bad, &interner).is_err(),
                "{}",
                String::from_utf8_lossy(bad)
            );
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
            assert!(
                parse(good, &interner).is_ok(),
                "{}",
                String::from_utf8_lossy(good)
            );
        }
    }

    #[test]
    fn files_parsed_with_a_shared_name_cache_give_the_trees_and_names_they_give_without() {
        let interner = Arc::clone(Interner::global());
        let first =
            "<?php namespace App; class Foo { public function bar($baz) { return $baz->qux(); } }";
        let second = "<?php namespace App; function f(Foo $baz) { return (new Foo)->bar($baz); }";
        let mut cache = NameCache::default();
        let shared: Vec<_> = [first, second]
            .iter()
            .map(|source| {
                format!(
                    "{:?}",
                    parse_with_names(*source, &interner, Some(&mut cache))
                        .unwrap()
                        .stmts
                )
            })
            .collect();
        let alone: Vec<_> = [first, second]
            .iter()
            .map(|source| format!("{:?}", parse(*source, &interner).unwrap().stmts))
            .collect();
        assert_eq!(shared, alone);
        // What the cache holds is what the interner gives.
        for text in ["Foo", "bar", "baz", "qux"] {
            let found = cache.find(text, crate::name::text_hash(text)).expect(text);
            assert!(found.same_allocation(&interner.name(text)), "{text}");
        }
        assert!(!cache.is_empty());
    }
}
