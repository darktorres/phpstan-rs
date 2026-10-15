//! Parsing a whole file: lexing, parsing and name resolution, with what the
//! lexer found besides the tree.

use std::sync::Arc;

use crate::ast::{FunctionBody, GotoLabel, Stmt};
pub use crate::lexer::FileFacts;
use crate::lexer::{IgnoreMark, LexEnd, LexWarning, Lexer, WatchList};
use crate::name::NameCache;
use crate::name_resolution::NameContext;
use crate::parser::{ParseError, Parser};
use crate::references::FileReferences;
use crate::span::LineIndex;

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
    /// What the file refers to.
    pub references: FileReferences,
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
    parse_file(source.into(), names, watch, false)
}

/// `parse_watching`, leaving the statements of the function and method
/// bodies that nothing at the load of a file reads unparsed: a body is
/// `skimmed` in the tree, and is read from its text with
/// `parse_function_body` when something asks. A syntax error in a body
/// skipped is not found, and its references are not in the file's.
pub fn parse_skimming<'a>(
    source: impl Into<Source<'a>>,
    names: Option<&mut NameCache>,
    watch: Option<&'static WatchList>,
) -> Result<ParsedFile, ParseError> {
    parse_file(source.into(), names, watch, true)
}

fn parse_file(
    source: Source<'_>,
    names: Option<&mut NameCache>,
    watch: Option<&'static WatchList>,
    skim_bodies: bool,
) -> Result<ParsedFile, ParseError> {
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
    parser.skip_bodies(skim_bodies);
    let parsed = parser.parse_program();
    let has_anonymous_class = parser.saw_anonymous_class();
    let has_nested_declaration = parser.saw_nested_declaration();
    let has_declaration_in_body = parser.saw_declaration_in_body();
    let has_global_or_include = parser.saw_global_or_include();
    let has_skimmed_bodies = parser.skipped_a_body();
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
    facts.has_declaration_in_body = has_declaration_in_body;
    facts.has_global_or_include = has_global_or_include;
    facts.has_skimmed_bodies = has_skimmed_bodies;
    Ok(ParsedFile {
        stmts,
        ignore_marks,
        line_index,
        warnings,
        facts,
        goto_labels,
        references,
    })
}

/// The body `bytes` holds, a `{ ... }` found in a file at the byte offset
/// `base`, parsed as it was in that file when `ctx` was the context in
/// effect at its declaration: its spans are where they are in the file. The
/// file's warnings are not read again.
pub fn parse_function_body(
    bytes: &[u8],
    base: u32,
    ctx: &NameContext,
) -> Result<ParsedBody, ParseError> {
    let mut parser = Parser::from_lexer(Lexer::fragment_bytes(bytes, base));
    *parser.ctx.get_mut() = ctx.clone();
    let body = parser.parse_function_body()?;
    Ok(ParsedBody {
        body,
        references: parser.take_references(),
    })
}

/// A function body parsed from its text, and what it refers to.
#[derive(Debug)]
pub struct ParsedBody {
    pub body: FunctionBody,
    pub references: FileReferences,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts_of(src: &str) -> FileFacts {
        parse(src.as_bytes()).expect("parses").facts
    }

    #[test]
    fn a_global_statement_or_an_include_is_a_fact_of_the_file() {
        assert!(!facts_of("<?php function f() { return 1; } $x = 2;").has_global_or_include);
        assert!(facts_of("<?php function f() { global $x; }").has_global_or_include);
        assert!(facts_of("<?php function f() { return include 'a.php'; }").has_global_or_include);
        assert!(facts_of("<?php require_once 'a.php';").has_global_or_include);
        assert!(
            facts_of("<?php function f($a) { return \"{$a->m(include 'a.php')}\"; }")
                .has_global_or_include
        );
    }

    #[test]
    fn a_declaration_as_a_statement_of_a_body_is_a_fact_of_the_file() {
        assert!(
            !facts_of("<?php function f() {} class A { function m() {} } const C = 1;")
                .has_declaration_in_body
        );
        for src in [
            "<?php function f() { function g() {} }",
            "<?php class A { function m() { class B {} } }",
            "<?php function f() { const C = 1; }",
            "<?php function f($a) { return \"{$a->m(function () { function h() {} })}\"; }",
        ] {
            assert!(facts_of(src).has_declaration_in_body, "{src}");
        }
    }

    /// `text` with the numbers of the nodes taken out, which differ when
    /// a body is parsed on its own.
    fn without_ids(text: String) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(at) = rest.find("NodeId(") {
            let (head, tail) = rest.split_at(at + "NodeId(".len());
            out.push_str(head);
            let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
            rest = &tail[digits..];
        }
        out.push_str(rest);
        out
    }

    fn bodies_of(stmts: &[Stmt]) -> Vec<&FunctionBody> {
        use crate::visit::{Visitor, walk_function_decl};
        struct Bodies<'a>(Vec<&'a FunctionBody>);
        impl<'a> Visitor<'a> for Bodies<'a> {
            fn visit_function_decl(&mut self, function: &'a crate::ast::FunctionDecl) {
                self.0.extend(function.body.as_ref());
                walk_function_decl(self, function);
            }
        }
        let mut bodies = Bodies(Vec::new());
        crate::visit::walk_stmts(&mut bodies, stmts);
        bodies.0
    }

    fn skimmed_and_full(src: &str) -> (ParsedFile, ParsedFile) {
        (
            parse_skimming(src.as_bytes(), None, None).expect("parses"),
            parse(src.as_bytes()).expect("parses"),
        )
    }

    const PLAIN: &str = "<?php
namespace App;
use Lib\\Tool;
function free(int $n): int { if ($n > 1) { return strlen('a') + helper($n); } return 0; }
class Box {
    public function make(): Tool { $t = new Tool(); return $t->run(Other::class, fn($x) => $x + 1); }
    public function name(): string { return \"v{$this->v}\" . \"$this->w\"; }
    abstract protected function nothing(): void;
}
";

    #[test]
    fn a_body_with_nothing_the_load_reads_is_skipped_and_parses_to_what_the_file_gives() {
        let (skimmed, full) = skimmed_and_full(PLAIN);
        assert!(skimmed.facts.has_skimmed_bodies && !full.facts.has_skimmed_bodies);
        let skipped = bodies_of(&skimmed.stmts);
        let eager = bodies_of(&full.stmts);
        assert_eq!(skipped.len(), 3);
        assert!(
            skipped
                .iter()
                .all(|body| body.skimmed && body.stmts.is_empty())
        );
        // What of the file is read at the load is what the full parse has.
        for (skipped, eager) in skipped.iter().zip(&eager) {
            let ctx = crate::name_resolution::context_at(&skimmed.stmts, skipped.open.start);
            let text = PLAIN.as_bytes();
            let start = skipped.open.start as usize;
            let end = (start..text.len())
                .find(|&end| {
                    parse_function_body(&text[start..=end], skipped.open.start, &ctx).is_ok()
                })
                .expect("a body");
            let parsed =
                parse_function_body(&text[start..=end], skipped.open.start, &ctx).expect("a body");
            assert_eq!(
                without_ids(format!("{:?}", parsed.body.stmts)),
                without_ids(format!("{:?}", eager.stmts))
            );
        }
        let mut declared_skimmed = Vec::new();
        let mut declared_full = Vec::new();
        crate::declarations::for_each_declaration(&skimmed.stmts, |d, _, _| {
            declared_skimmed.push(d.name().to_string())
        });
        crate::declarations::for_each_declaration(&full.stmts, |d, _, _| {
            declared_full.push(d.name().to_string())
        });
        assert_eq!(declared_skimmed, declared_full);
        assert!(!declared_full.is_empty());
        // Where the statements after a body are is where they are.
        let spans = |file: &ParsedFile| {
            file.stmts
                .iter()
                .map(|stmt| (stmt.span.start, stmt.span.end))
                .collect::<Vec<_>>()
        };
        assert_eq!(spans(&skimmed), spans(&full));
    }

    #[test]
    fn a_body_that_declares_adds_to_the_global_scope_or_has_a_doc_comment_is_parsed() {
        for body in [
            "function g() {}",
            "class B {}",
            "interface I {}",
            "trait T {}",
            "enum E {}",
            "return new class {};",
            "define('X', 1);",
            "\\define('X', 1);",
            "global $x;",
            "return include 'a.php';",
            "require_once 'a.php';",
            "yield 1;",
            "goto end; end:",
            "/** @var int $x */ $x = 1;",
            "return 1; /** tail */",
            "$x = new \\PHPStan\\Type\\Type();",
            "$x = new namespace\\PHPStan\\Type();",
            "?> html <?php",
            "return \"{$a->m()}\";",
            "$f = function &() { return 1; }; function h() {}",
        ] {
            let src = format!("<?php function f() {{ {body} }} function after() {{ return 1; }}");
            let (skimmed, full) = skimmed_and_full(&src);
            let bodies = bodies_of(&skimmed.stmts);
            assert!(!bodies[0].skimmed, "{body}");
            assert!(bodies.last().is_some_and(|body| body.skimmed), "{body}");
            // The first body is parsed as it is by a parse of the whole,
            // from the tokens that were read.
            assert_eq!(
                format!("{:?}", bodies[0].stmts),
                format!("{:?}", bodies_of(&full.stmts)[0].stmts),
                "{body}"
            );
            assert_eq!(
                skimmed.facts.has_anonymous_class,
                full.facts.has_anonymous_class
            );
            assert_eq!(
                skimmed.facts.has_declaration_in_body,
                full.facts.has_declaration_in_body
            );
            assert_eq!(
                skimmed.facts.has_global_or_include,
                full.facts.has_global_or_include
            );
        }
    }

    #[test]
    fn a_keyword_after_an_arrow_or_a_scope_operator_is_a_member_and_a_closure_is_not_a_declaration()
    {
        for body in [
            "return Foo::class;",
            "return $this->class . $this->function . $this->global;",
            "return Foo::include($a);",
            "$f = function () { return 1; }; $g = function &($x) use ($f) { return $x; };",
            "return static function () { return 2; };",
        ] {
            let src = format!("<?php function f() {{ {body} }}");
            let (skimmed, _) = skimmed_and_full(&src);
            assert!(bodies_of(&skimmed.stmts)[0].skimmed, "{body}");
        }
    }

    #[test]
    fn names_that_can_be_phpstan_keep_the_body_parsed() {
        for src in [
            "<?php namespace PHPStan\\Foo; function f() { return 1; }",
            "<?php use PHPStan\\Type\\Type as T; function f() { return new T(); }",
            "<?php use PHPStan; function f() { return 1; }",
        ] {
            let (skimmed, _) = skimmed_and_full(src);
            assert!(!bodies_of(&skimmed.stmts)[0].skimmed, "{src}");
        }
    }

    #[test]
    fn a_syntax_error_in_a_skipped_body_is_not_found_but_a_lexer_error_is() {
        let src = "<?php function f() { $a = ; } function g() { return 1; }";
        assert!(parse(src.as_bytes()).is_err());
        let skimmed = parse_skimming(src.as_bytes(), None, None).expect("a body is not parsed");
        assert!(bodies_of(&skimmed.stmts).iter().all(|body| body.skimmed));
        assert!(parse_function_body(b"{ $a = ; }", 20, &NameContext::default()).is_err());
        let unbalanced = "<?php function f() { if (1) { return 1; } ";
        assert!(parse(unbalanced.as_bytes()).is_err());
        assert!(parse_skimming(unbalanced.as_bytes(), None, None).is_err());
        let lexer_error = "<?php function f() { return 08; }";
        assert!(parse(lexer_error.as_bytes()).is_err());
        assert!(parse_skimming(lexer_error.as_bytes(), None, None).is_err());
    }

    #[test]
    fn a_files_references_are_those_outside_its_skipped_bodies_and_those_of_the_bodies() {
        let (skimmed, full) = skimmed_and_full(PLAIN);
        let mut names = skimmed.references.names.clone();
        let text = PLAIN.as_bytes();
        for body in bodies_of(&skimmed.stmts) {
            let start = body.open.start as usize;
            let ctx = crate::name_resolution::context_at(&skimmed.stmts, body.open.start);
            let end = (start..text.len())
                .find(|&end| parse_function_body(&text[start..=end], body.open.start, &ctx).is_ok())
                .expect("a body");
            let ctx = crate::name_resolution::context_at(&skimmed.stmts, body.open.start);
            names.extend(
                parse_function_body(&text[start..=end], body.open.start, &ctx)
                    .expect("a body")
                    .references
                    .names,
            );
        }
        assert!(skimmed.references.names.len() < full.references.names.len());
        assert_eq!(skimmed.references.outside, full.references.outside);
        assert_eq!(names.len(), full.references.names.len());
        assert!(
            full.references
                .names
                .iter()
                .all(|name| names.contains(name))
        );
    }

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
