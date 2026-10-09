//! Parsing a whole file: lexing, parsing and name resolution, with what the
//! lexer found besides the tree.

use std::borrow::Cow;
use std::sync::Arc;

use crate::ast::{ExprId, GotoLabel, NodeArenas, StmtId};
pub use crate::lexer::FileFacts;
use crate::lexer::{IgnoreMark, LexEnd, LexWarning, Lexer, WatchList};
use crate::name::{Name, NameCache, NameId};
use crate::parser::{ParseError, Parser};
use crate::references::FileReferences;
use crate::span::LineIndex;

/// Parses a standalone expression into an existing file owner. Its nodes,
/// payload ranges and decoded bytes are assigned IDs after the owner's
/// current contents, so the returned ID is meaningful only with `arenas`.
/// The supplied context is cloned into the parser before name references are
/// built, matching how the expression resolves in its containing file.
pub fn parse_expression_into_arenas(
    source: &str,
    context: &crate::name_resolution::NameContext,
    arenas: &mut NodeArenas,
) -> Option<ExprId> {
    let mut lexer = Lexer::new_in_php(source)
        .with_source_backed_strings(false)
        .without_file_names();
    let fragment = arenas.new_fragment();
    let source = lexer.source_bytes();
    lexer.set_decoded_bytes_base(fragment.decoded_bytes_base);
    let stream = lexer.lex_token_stream();
    let mut parser = Parser::from_token_stream_with_scratch(
        stream,
        fragment,
        Some(source),
        crate::parser::ParserScratch::default(),
        false,
    );
    *parser.ctx.borrow_mut() = context.clone();
    let expression = parser.parse_expression().ok()?;
    if !parser.at_eof() {
        return None;
    }
    arenas.append_fragment(parser.take_node_arenas());
    Some(expression)
}

/// A parsed file.
#[derive(Debug, Clone)]
pub struct ParsedFile {
    /// The bytes this tree was parsed from. Empty when the transient-scratch
    /// entry point is used. AST payloads will become ranges into this buffer
    /// as the tree moves to typed arenas.
    pub source: Arc<[u8]>,
    /// The distinct names encountered in the file, in first-use order.
    /// Empty when the transient-scratch entry point is used.
    pub names: Box<[Name]>,
    pub stmts: crate::ast::StmtIdRange,
    /// The file's typed node storage and their parallel span tables.
    pub arenas: NodeArenas,
    /// Every comment that holds `@phpstan-ignore`, doc comments included,
    /// in source order, with what the code next to it is.
    pub ignore_marks: Vec<IgnoreMark>,
    /// The file's lines, for the positions of its offsets.
    pub line_index: Arc<LineIndex>,
    /// What the lexer skipped over.
    pub warnings: Vec<LexWarning>,
    pub facts: FileFacts,
    /// At least one PHPStan stub attribute changed a node while parsing.
    /// Consumers must rebuild parser-recorded reference products from the
    /// final tree because removed declarations were still parsed for syntax.
    pub stub_attributes_changed: bool,
    /// The labels the file's own top-level `goto`s name.
    pub goto_labels: Box<[GotoLabel]>,
    /// What the file refers to.
    /// References the parser recorded while constructing the tree. A
    /// parser-time stub transformation invalidates these products because
    /// it can discard declarations and extend doc comments.
    pub references: Option<FileReferences>,
}

impl ParsedFile {
    /// Resolves the root statement range through this file's owning arena.
    pub fn statements(&self) -> &[StmtId] {
        self.arenas.stmt_ids(&self.stmts)
    }

    /// Returns the file's arenas after its parsed products have been
    /// consumed, so a worker can reuse their capacities for another file.
    pub fn into_node_arenas(self) -> NodeArenas {
        self.arenas
    }

    /// Compacts the node vectors when this parsed file's tree will be kept.
    pub fn shrink_node_arenas_to_fit(&mut self) {
        self.arenas.shrink_to_fit();
    }

    /// Reads an in-file byte range, such as a source-backed AST payload.
    /// Returns `None` for a transient-scratch parse, which retains no source.
    pub fn source_slice(&self, span: crate::span::Span) -> Option<&[u8]> {
        self.arenas.source_slice(span)
    }

    /// Resolves source-backed doc-comment ranges to text at a reader boundary.
    /// A grouped doc keeps the parser's established normalized representation.
    pub fn doc_comment_text<'a>(
        &'a self,
        ranges: &'a crate::ast::DocCommentRanges,
    ) -> Option<Cow<'a, str>> {
        ranges.text(&self.source)
    }

    /// Resolves bytes decoded from an escaped literal in this file.
    pub fn decoded_bytes(&self, range: crate::ast::DecodedBytesRange) -> Option<&[u8]> {
        self.arenas.decoded_bytes(range)
    }

    /// Resolves a literal's bytes through the owner of its source or decoded
    /// range. Callers copy only when retaining a value beyond this file.
    pub fn string_bytes<'a>(&'a self, payload: &'a crate::ast::StringPayload) -> Option<&'a [u8]> {
        self.arenas.string_bytes(payload)
    }

    /// Resolves a file-local name id to the shared interned name.
    /// Returns `None` when the transient-scratch entry point omitted the table.
    pub fn name(&self, id: NameId) -> Option<&Name> {
        self.names.get(u32::from(id.into_raw()) as usize)
    }

    /// Resolves a parser-created identifier through this file's compact
    /// name table. Identifiers synthesized outside a parsed file have no id.
    pub fn ident_name(&self, ident: &crate::ast::Ident) -> Option<&Name> {
        self.name(ident.name_id?)
    }

    /// Looks up an expression by its file-local arena id.
    pub fn expr(&self, id: crate::ast::ExprId) -> Option<&crate::ast::ExprNode> {
        self.arenas.expr(id)
    }

    /// Looks up a statement by its file-local arena id.
    pub fn stmt(&self, id: crate::ast::StmtId) -> Option<&crate::ast::StmtNode> {
        self.arenas.stmt(id)
    }

    /// The source span of an expression in this file.
    pub fn expr_span(&self, id: crate::ast::ExprId) -> Option<crate::span::Span> {
        self.arenas.expr_span(id)
    }

    /// The source span of a statement in this file.
    pub fn stmt_span(&self, id: crate::ast::StmtId) -> Option<crate::span::Span> {
        self.arenas.stmt_span(id)
    }

    pub fn expr_count(&self) -> usize {
        self.arenas.expr_spans.len()
    }

    pub fn stmt_count(&self) -> usize {
        self.arenas.stmt_spans.len()
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
    parse_watching_in_arenas(
        source,
        names,
        watch,
        NodeArenas::default(),
        Keep {
            compact_arenas: true,
            retain_file_data: true,
        },
        Reuse::Fresh,
    )
}

/// Parses with reusable node arenas. On success, the returned file owns the
/// bundle until its products are consumed; `ParsedFile::into_node_arenas`
/// returns it for reuse. On a parse error, the arenas are returned to
/// `scratch` so a worker can keep its warmed capacity.
pub fn parse_watching_with_scratch<'a>(
    source: impl Into<Source<'a>>,
    names: Option<&mut NameCache>,
    watch: Option<&'static WatchList>,
    scratch: &mut NodeArenas,
) -> Result<ParsedFile, ParseError> {
    let arenas = std::mem::take(scratch);
    parse_watching_in_arenas(
        source,
        names,
        watch,
        arenas,
        Keep {
            compact_arenas: false,
            retain_file_data: true,
        },
        Reuse::Arenas(scratch),
    )
}

/// Reuses the token tables and parser temporaries a worker keeps between
/// files, alongside the parsed node arenas.
pub fn parse_watching_with_worker_scratch<'a>(
    source: impl Into<Source<'a>>,
    names: Option<&mut NameCache>,
    watch: Option<&'static WatchList>,
    scratch: WorkerScratch<'_>,
) -> Result<ParsedFile, ParseError> {
    let node_arenas = std::mem::take(scratch.arenas);
    parse_watching_in_arenas(
        source,
        names,
        watch,
        node_arenas,
        Keep {
            compact_arenas: false,
            retain_file_data: true,
        },
        Reuse::Worker(scratch),
    )
}

/// Parses a file whose tree is used only while its products are collected.
/// It reuses `scratch`, returns its arenas with the parsed file on success,
/// and restores them to `scratch` on a parse error. It does not copy the
/// source bytes or retain the file's `NameId` table; the caller must not
/// retain the tree or use its source/name accessors.
pub fn parse_watching_transient_with_scratch<'a>(
    source: impl Into<Source<'a>>,
    names: Option<&mut NameCache>,
    watch: Option<&'static WatchList>,
    scratch: &mut NodeArenas,
) -> Result<ParsedFile, ParseError> {
    let arenas = std::mem::take(scratch);
    parse_watching_in_arenas(
        source,
        names,
        watch,
        arenas,
        Keep {
            compact_arenas: false,
            retain_file_data: false,
        },
        Reuse::Arenas(scratch),
    )
}

/// The buffers a worker keeps between files: the node arenas, the token
/// tables and the lexer's and parser's temporaries.
pub struct WorkerScratch<'s> {
    pub arenas: &'s mut NodeArenas,
    pub tokens: &'s mut crate::token::TokenStream,
    pub lexer: &'s mut crate::lexer::LexerScratch,
    pub parser: &'s mut crate::parser::ParserScratch,
}

/// What a parse keeps of its input and of its arenas.
struct Keep {
    /// Whether the arenas are trimmed to what they hold.
    compact_arenas: bool,
    /// Whether the file's source bytes and name table stay with the result.
    retain_file_data: bool,
}

/// The buffers a parse reuses, which it gives back when the parse fails.
enum Reuse<'s> {
    Fresh,
    Arenas(&'s mut NodeArenas),
    Worker(WorkerScratch<'s>),
}

impl<'s> Reuse<'s> {
    fn give_back(&mut self, arenas: NodeArenas) {
        match self {
            Reuse::Fresh => {}
            Reuse::Arenas(scratch) => **scratch = arenas,
            Reuse::Worker(scratch) => *scratch.arenas = arenas,
        }
    }

    fn worker(&mut self) -> Option<&mut WorkerScratch<'s>> {
        match self {
            Reuse::Worker(scratch) => Some(scratch),
            _ => None,
        }
    }
}

fn parse_watching_in_arenas<'a>(
    source: impl Into<Source<'a>>,
    names: Option<&mut NameCache>,
    watch: Option<&'static WatchList>,
    mut arenas: NodeArenas,
    keep: Keep,
    mut reuse: Reuse<'_>,
) -> Result<ParsedFile, ParseError> {
    let Keep {
        compact_arenas,
        retain_file_data,
    } = keep;
    let source = source.into();
    let source_bytes: Arc<[u8]> = if retain_file_data {
        match source {
            Source::Text(text) => Arc::from(text.as_bytes()),
            Source::Bytes(bytes) => Arc::from(bytes),
        }
    } else {
        Arc::from([])
    };
    arenas.decoded_bytes_base = 0;
    let lexer = Lexer::from_source(source).with_source_backed_strings(retain_file_data);
    let lexer = match watch {
        Some(watch) => lexer.watching(watch),
        None => lexer,
    };
    let mut lexer = match names {
        Some(cache) => lexer.with_name_cache(cache),
        None => lexer,
    };
    // The flat stream's NameIds need their per-file table while the parser
    // reads them, even for transient parses. The table is released with the
    // worker token buffer after parsing when ParsedFile does not retain it.
    let source = lexer.source_bytes();
    lexer.set_decoded_bytes_base(arenas.decoded_bytes_base);
    let token_stream = match reuse.worker() {
        Some(worker) => {
            lexer.lex_token_stream_with_worker_scratch(std::mem::take(worker.tokens), worker.lexer)
        }
        None => lexer.lex_token_stream(),
    };
    let mut parser = match reuse.worker() {
        Some(worker) => Parser::from_token_stream_with_scratch(
            token_stream,
            arenas,
            Some(source),
            std::mem::take(worker.parser),
            retain_file_data,
        ),
        None => Parser::from_token_stream_with_scratch(
            token_stream,
            arenas,
            Some(source),
            crate::parser::ParserScratch::default(),
            retain_file_data,
        ),
    };
    let (parsed_stmts, parse_error, mut arenas) = match parser.parse_program() {
        Ok(program) => (Some(program.stmts), None, program.arenas),
        Err(error) => (None, Some(error), parser.take_node_arenas()),
    };
    let has_anonymous_class = parser.saw_anonymous_class();
    let stub_attributes_changed = parser.stub_attributes_changed();
    let has_nested_declaration = parser.saw_nested_declaration();
    let unsets_property = parser.saw_property_unset();
    let syntax_marks = parser.syntax_marks();
    let goto_labels = parser.file_goto_labels();
    let recorded_references = parser.take_references();
    let references = (!stub_attributes_changed).then_some(recorded_references);
    // The rest of the file is lexed even when the parse failed: a lexer
    // error anywhere in the file is the file's error, before a syntax
    // error found in the part the parse read.
    let LexEnd {
        ignore_marks,
        warnings,
        mut facts,
        line_index,
        names,
        decoded_bytes,
    } = parser
        .take_lex_end(retain_file_data)
        .expect("a parser of a lexer ends with the lexer's record");
    if let Some(worker) = reuse.worker() {
        *worker.tokens = parser.take_token_stream();
        *worker.parser = parser.take_scratch();
    }
    if let Some(error) = warnings.iter().find(|w| w.is_error) {
        reuse.give_back(arenas);
        return Err(ParseError::new(error.message.clone(), error.span));
    }
    let stmts = match (parsed_stmts, parse_error) {
        (Some(stmts), None) => stmts,
        (_, Some(error)) => {
            reuse.give_back(arenas);
            return Err(error);
        }
        _ => unreachable!("parser result carries either a program or an error"),
    };
    arenas.decoded_bytes.extend(decoded_bytes);
    if compact_arenas {
        arenas.shrink_to_fit();
    }
    arenas.source = Arc::clone(&source_bytes);
    facts.has_anonymous_class = has_anonymous_class;
    facts.has_nested_declaration = has_nested_declaration;
    facts.unsets_property = unsets_property;
    facts.syntax.merge(&syntax_marks);
    Ok(ParsedFile {
        source: source_bytes,
        names,
        stmts,
        arenas,
        ignore_marks,
        line_index,
        warnings,
        facts,
        stub_attributes_changed,
        goto_labels,
        references,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expression_fragments_append_to_the_same_owner_with_live_name_context() {
        let mut file = parse(b"<?php namespace App; $existing = 1;").unwrap();
        let initial_exprs = file.arenas.expr_spans.len();
        let initial_args = file.arenas.args.len();
        let mut context = crate::name_resolution::NameContext::default();
        context.set_namespace("App");

        let class_expr =
            parse_expression_into_arenas("new Service()", &context, &mut file.arenas).unwrap();
        let new = file.arenas.expr(class_expr).unwrap();
        let crate::ast::ExprKind::New(class_name, _) = &new.kind else {
            panic!("fragment root should be a new expression");
        };
        let crate::ast::ExprKind::Name(class_name) = &file.arenas.expr(*class_name).unwrap().kind
        else {
            panic!("new expression class should be a name");
        };
        let class_name = file.arenas.qualified_name(class_name).unwrap();
        assert_eq!(
            class_name.resolved,
            crate::ast::ResolvedName::Exact(crate::name::Name::from("App\\Service"))
        );

        let receiver = parse_expression_into_arenas(
            "$service->run(\"decoded\\nbytes\")",
            &context,
            &mut file.arenas,
        )
        .unwrap();

        assert!(u32::from(receiver.into_raw()) as usize >= initial_exprs);
        assert!(file.arenas.expr_spans.len() > initial_exprs);
        assert!(file.arenas.args.len() > initial_args);
        let call = file.arenas.expr(receiver).unwrap();
        let crate::ast::ExprKind::MethodCall(_, _, args, _) = &call.kind else {
            panic!("fragment root should be a method call");
        };
        let string = file
            .arenas
            .args(args.clone())
            .iter()
            .find_map(|arg| {
                let value = file.arenas.expr(arg.value)?;
                match &value.kind {
                    crate::ast::ExprKind::Literal(crate::ast::Literal::String(payload)) => {
                        Some(payload)
                    }
                    _ => None,
                }
            })
            .unwrap();
        assert_eq!(
            file.arenas.string_bytes(string),
            Some(&b"decoded\nbytes"[..])
        );
    }

    #[test]
    fn parser_errors_are_pointer_sized() {
        assert_eq!(
            std::mem::size_of::<ParseError>(),
            std::mem::size_of::<usize>()
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
    fn escaped_literals_resolve_through_the_file_decoded_byte_store() {
        let parsed = parse(b"<?php $value = 'a\\'b'; $other = \"\\u{1f600}\";").unwrap();
        let payloads: Vec<_> = parsed
            .arenas
            .exprs
            .iter()
            .filter_map(|(_, expr)| match &expr.kind {
                crate::ast::ExprKind::Literal(crate::ast::Literal::String(payload)) => {
                    Some(payload)
                }
                _ => None,
            })
            .collect();
        assert!(
            payloads
                .iter()
                .all(|payload| matches!(payload, crate::ast::StringPayload::Decoded(_)))
        );
        assert!(
            payloads
                .iter()
                .any(|payload| parsed.string_bytes(payload) == Some(&b"a'b"[..]))
        );
        assert!(
            payloads
                .iter()
                .any(|payload| { parsed.string_bytes(payload) == Some("😀".as_bytes()) })
        );
    }

    #[test]
    fn unescaped_interpolated_chunks_refer_to_the_source_owner() {
        let parsed = parse(b"<?php $value = \"plain bytes\";").unwrap();
        let payload = parsed
            .arenas
            .exprs
            .iter()
            .find_map(|(_, expr)| match &expr.kind {
                crate::ast::ExprKind::Literal(crate::ast::Literal::String(payload)) => {
                    Some(payload)
                }
                _ => None,
            })
            .unwrap();
        assert!(matches!(payload, crate::ast::StringPayload::Source(_)));
        assert_eq!(parsed.string_bytes(payload), Some(&b"plain bytes"[..]));
    }

    #[test]
    fn transient_unescaped_literals_use_decoded_ranges_without_retaining_source() {
        let mut scratch = NodeArenas::default();
        let parsed = parse_watching_transient_with_scratch(
            b"<?php $value = 'plain bytes';",
            None,
            None,
            &mut scratch,
        )
        .unwrap();
        let payload = parsed
            .arenas
            .exprs
            .iter()
            .find_map(|(_, expr)| match &expr.kind {
                crate::ast::ExprKind::Literal(crate::ast::Literal::String(payload)) => {
                    Some(payload)
                }
                _ => None,
            })
            .unwrap();
        assert!(matches!(payload, crate::ast::StringPayload::Decoded(_)));
        assert!(parsed.source.is_empty());
        assert_eq!(parsed.string_bytes(payload), Some(&b"plain bytes"[..]));
    }

    #[test]
    fn decoded_ranges_from_interpolation_fragments_append_after_the_parent() {
        let parsed = parse(b"<?php $value = \"\\n{$items['a\\'b']}\";").unwrap();
        let values: Vec<_> = parsed
            .arenas
            .exprs
            .iter()
            .filter_map(|(_, expr)| match &expr.kind {
                crate::ast::ExprKind::Literal(crate::ast::Literal::String(payload)) => {
                    parsed.string_bytes(payload)
                }
                _ => None,
            })
            .collect();
        assert!(values.iter().any(|value| *value == b"\n"));
        assert!(values.iter().any(|value| *value == b"a'b"));
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
                crate::canonical::print_line(&parse_with_names(*source, Some(&mut cache)).unwrap())
            })
            .collect();
        let alone: Vec<_> = [first, second]
            .iter()
            .map(|source| crate::canonical::print_line(&parse(*source).unwrap()))
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

    #[test]
    fn parse_watching_with_scratch_reuses_payload_and_list_arenas() {
        let first = parse(
            br#"<?php
                #[A(1)]
                function f(#[B] int $x) {
                    $callback = static fn($a) => f($a);
                    $object = new class($x) {
                        public function method(#[C] $y) {}
                    };
                    return [f(1), 2 => $x];
                }
                $result = match (1) { 1, 2 => f(2), default => null };
            "#,
        )
        .unwrap();
        let mut scratch = first.into_node_arenas();
        let expr_capacity = scratch.expr_spans.capacity();
        let stmt_capacity = scratch.stmt_spans.capacity();
        assert!(expr_capacity > 0);
        assert!(stmt_capacity > 0);
        let list_storage = [
            scratch.args.iter().next().unwrap().1 as *const _ as *const (),
            scratch.array_items.iter().next().unwrap().1 as *const _ as *const (),
            scratch.params.iter().next().unwrap().1 as *const _ as *const (),
            scratch.match_arms.iter().next().unwrap().1 as *const _ as *const (),
            scratch.attributes.iter().next().unwrap().1 as *const _ as *const (),
        ];
        let payload_storage = [
            scratch.functions.iter().next().unwrap().1 as *const _ as *const (),
            scratch.methods.iter().next().unwrap().1 as *const _ as *const (),
            scratch.classes.iter().next().unwrap().1 as *const _ as *const (),
            scratch.closures.iter().next().unwrap().1 as *const _ as *const (),
            scratch.anonymous_classes.iter().next().unwrap().1 as *const _ as *const (),
        ];

        assert!(
            parse_watching_with_scratch(b"<?php $broken = 08;", None, None, &mut scratch,).is_err()
        );
        assert!(scratch.expr_spans.capacity() >= expr_capacity);
        assert!(scratch.stmt_spans.capacity() >= stmt_capacity);
        assert!(scratch.args.is_empty());
        assert!(scratch.array_items.is_empty());
        assert!(scratch.params.is_empty());
        assert!(scratch.match_arms.is_empty());
        assert!(scratch.attributes.is_empty());
        assert!(scratch.functions.is_empty());
        assert!(scratch.methods.is_empty());
        assert!(scratch.classes.is_empty());
        assert!(scratch.closures.is_empty());
        assert!(scratch.anonymous_classes.is_empty());

        let second = parse_watching_with_scratch(
            br#"<?php
                #[A(1)]
                function f(#[B] int $x) {
                    $callback = static fn($a) => f($a);
                    $object = new class($x) {
                        public function method(#[C] $y) {}
                    };
                    return [f(1), 2 => $x];
                }
                $result = match (1) { 1, 2 => f(2), default => null };
            "#,
            None,
            None,
            &mut scratch,
        )
        .unwrap();

        assert!(second.expr_count() > 0);
        assert!(second.stmt_count() > 0);
        assert!(second.arenas.expr_spans.capacity() >= expr_capacity);
        assert!(second.arenas.stmt_spans.capacity() >= stmt_capacity);
        assert_eq!(
            list_storage,
            [
                second.arenas.args.iter().next().unwrap().1 as *const _ as *const (),
                second.arenas.array_items.iter().next().unwrap().1 as *const _ as *const (),
                second.arenas.params.iter().next().unwrap().1 as *const _ as *const (),
                second.arenas.match_arms.iter().next().unwrap().1 as *const _ as *const (),
                second.arenas.attributes.iter().next().unwrap().1 as *const _ as *const (),
            ]
        );
        assert_eq!(
            payload_storage,
            [
                second.arenas.functions.iter().next().unwrap().1 as *const _ as *const (),
                second.arenas.methods.iter().next().unwrap().1 as *const _ as *const (),
                second.arenas.classes.iter().next().unwrap().1 as *const _ as *const (),
                second.arenas.closures.iter().next().unwrap().1 as *const _ as *const (),
                second.arenas.anonymous_classes.iter().next().unwrap().1 as *const _ as *const (),
            ]
        );
        for (index, span) in second.arenas.expr_spans.iter().enumerate() {
            let id = ExprId::from_raw(la_arena::RawIdx::from_u32(index as u32));
            assert_eq!(second.expr_span(id), Some(*span));
        }
        for (index, span) in second.arenas.stmt_spans.iter().enumerate() {
            let id = StmtId::from_raw(la_arena::RawIdx::from_u32(index as u32));
            assert_eq!(second.stmt_span(id), Some(*span));
        }
    }

    #[test]
    fn transient_scratch_parse_omits_source_and_file_name_table() {
        let mut scratch = NodeArenas::default();
        let parsed = parse_watching_transient_with_scratch(
            b"<?php $object->member; $text = \"{$object->nested}\";",
            None,
            None,
            &mut scratch,
        )
        .unwrap();

        assert!(parsed.source.is_empty());
        assert!(
            parsed
                .source_slice(crate::span::Span { start: 0, end: 5 })
                .is_none()
        );
        assert!(parsed.names.is_empty());
        assert!(parsed.expr_count() > 0);
        assert!(parsed.stmt_count() > 0);

        struct NoNameIds;
        impl crate::visit::arena::Visitor for NoNameIds {
            fn visit_expr(&mut self, arenas: &NodeArenas, id: crate::ast::ExprId) {
                let expr = arenas.expr(id).expect("expression ID belongs to its file");
                match &expr.kind {
                    crate::ast::ExprKind::PropertyAccess(_, ident, _) => {
                        assert!(ident.name_id.is_none());
                    }
                    crate::ast::ExprKind::MethodCall(_, ident, _, _) => {
                        assert!(ident.name_id.is_none());
                    }
                    _ => {}
                }
                crate::visit::arena::walk_expr(self, arenas, id);
            }
        }
        crate::visit::arena::walk_stmts(&mut NoNameIds, &parsed.arenas, parsed.statements());
    }

    #[test]
    fn transient_parse_populates_and_reuses_payload_and_list_arenas() {
        let mut scratch = NodeArenas::default();
        let parsed = parse_watching_transient_with_scratch(
            br#"<?php
                #[A(1)]
                function f(#[B] int $x) {
                    $callback = static fn($a) => f($a);
                    $object = new class($x) {
                        public function method(#[C] $y) {}
                    };
                    return [f(1), 2 => $x];
                }
                $result = match (1) { 1, 2 => f(2), default => null };
            "#,
            None,
            None,
            &mut scratch,
        )
        .unwrap();

        assert!(parsed.source.is_empty());
        assert!(parsed.names.is_empty());
        let list_storage = [
            parsed.arenas.args.iter().next().unwrap().1 as *const _ as *const (),
            parsed.arenas.array_items.iter().next().unwrap().1 as *const _ as *const (),
            parsed.arenas.params.iter().next().unwrap().1 as *const _ as *const (),
            parsed.arenas.match_arms.iter().next().unwrap().1 as *const _ as *const (),
            parsed.arenas.attributes.iter().next().unwrap().1 as *const _ as *const (),
        ];
        let payload_storage = [
            parsed.arenas.functions.iter().next().unwrap().1 as *const _ as *const (),
            parsed.arenas.methods.iter().next().unwrap().1 as *const _ as *const (),
            parsed.arenas.classes.iter().next().unwrap().1 as *const _ as *const (),
            parsed.arenas.closures.iter().next().unwrap().1 as *const _ as *const (),
            parsed.arenas.anonymous_classes.iter().next().unwrap().1 as *const _ as *const (),
        ];

        scratch = parsed.into_node_arenas();
        let reparsed = parse_watching_transient_with_scratch(
            br#"<?php
                #[A(1)]
                function f(#[B] int $x) {
                    $callback = static fn($a) => f($a);
                    $object = new class($x) {
                        public function method(#[C] $y) {}
                    };
                    return [f(1), 2 => $x];
                }
                $result = match (1) { 1, 2 => f(2), default => null };
            "#,
            None,
            None,
            &mut scratch,
        )
        .unwrap();
        assert_eq!(
            list_storage,
            [
                reparsed.arenas.args.iter().next().unwrap().1 as *const _ as *const (),
                reparsed.arenas.array_items.iter().next().unwrap().1 as *const _ as *const (),
                reparsed.arenas.params.iter().next().unwrap().1 as *const _ as *const (),
                reparsed.arenas.match_arms.iter().next().unwrap().1 as *const _ as *const (),
                reparsed.arenas.attributes.iter().next().unwrap().1 as *const _ as *const (),
            ]
        );
        assert_eq!(
            payload_storage,
            [
                reparsed.arenas.functions.iter().next().unwrap().1 as *const _ as *const (),
                reparsed.arenas.methods.iter().next().unwrap().1 as *const _ as *const (),
                reparsed.arenas.classes.iter().next().unwrap().1 as *const _ as *const (),
                reparsed.arenas.closures.iter().next().unwrap().1 as *const _ as *const (),
                reparsed.arenas.anonymous_classes.iter().next().unwrap().1 as *const _ as *const (),
            ]
        );
    }
}
