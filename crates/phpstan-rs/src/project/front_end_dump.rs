//! O1, the front end's dump: what the project's own parse (`parse_file`)
//! makes of a file, and what the stub build makes of an embedded stub,
//! printed canonically (`syntax::canonical`), one text for each file and
//! role. Comparing two builds' texts shows any change in what the front
//! end gives, whatever represents it.
//!
//! A file's text has, in order:
//! - its tokens, with their spans and the spellings of keywords, its
//!   comments and what the lexer skipped;
//! - its parse: every product `ParsedFile` holds, or the parse error, then
//!   what the parse gives that it does not keep: the ignore markers, the
//!   `goto` labels, and the doc sites, each with what its tags refer to;
//! - each doc comment's `DocBlock`, and each type node of its tags with
//!   the type its text reads back as (`parse_written_type`), keyed by the
//!   comment's span, the tag's ordinal and the node's path;
//! - at every token boundary, the line and column, the UTF-8 and UTF-16
//!   columns, and the offsets those columns give back.

use std::path::Path;

use phpdoc::grammar::{PhpDocTagValue, TypeNode};
use syntax::canonical::{Canonical, Printer};
use syntax::lexer::Lexer;
use syntax::span::{LineIndex, Span, Unit};
use syntax::token::Token;

use crate::project::{ParseRecord, Project, parse_file_recording};
use crate::registry::Origin;

/// The dump of the file at `path`, of `bytes`, parsed as the load parses
/// it: as a stub when `is_stub`, with the names `names` holds, and with
/// its use sites indexed (from `source`, the text the load keeps of it)
/// when `index_use_sites`, and its package found by `project`.
pub(crate) fn dump_file(
    path: &Path,
    bytes: &[u8],
    is_stub: bool,
    names: &mut syntax::name::NameCache,
    project: &Project,
    index_use_sites: bool,
    source: &str,
) -> String {
    let mut p = Printer::default();
    let tokens = write_tokens(bytes, &mut p);
    let mut record = ParseRecord::default();
    let parsed = match std::str::from_utf8(bytes) {
        Ok(text) => parse_file_recording(path, text, is_stub, Some(names), Some(&mut record)),
        Err(_) => parse_file_recording(path, bytes, is_stub, Some(names), Some(&mut record)),
    };
    section(&mut p, "parse");
    let line_index = match parsed {
        Ok(mut parsed) => {
            if index_use_sites {
                project.index_use_sites(&mut parsed, source);
            }
            project.note_owner(&mut parsed, path);
            parsed.canonical(&mut p);
            write_record(&record, &mut p);
            std::sync::Arc::clone(&parsed.line_index)
        }
        Err(error) => {
            error.canonical(&mut p);
            std::sync::Arc::new(LineIndex::new(bytes))
        }
    };
    write_docs(&tokens, &mut p);
    write_positions(&tokens, &line_index, &mut p);
    p.line();
    p.finish()
}

/// The dump of the embedded stub `source`, from `origin`, as the stub build
/// reads it: its tree with the stub attributes applied, and the
/// declarations it takes of it.
pub(crate) fn dump_embedded_stub(source: &str, origin: &Origin) -> String {
    let mut p = Printer::default();
    let tokens = write_tokens(source.as_bytes(), &mut p);
    section(&mut p, "stub build");
    match crate::registry::stubs::stub_program(source) {
        Ok(program) => {
            program.canonical(&mut p);
            p.line();
            crate::registry::stubs::index_stub_program(&program, origin).canonical(&mut p);
        }
        Err(error) => error.canonical(&mut p),
    }
    write_docs(&tokens, &mut p);
    write_positions(&tokens, &LineIndex::new(source.as_bytes()), &mut p);
    p.line();
    p.finish()
}

/// A token, its span and a keyword's spelling.
type Spanned = (Token, Span, Option<String>);

/// Writes the tokens, the comments and what the lexer skipped, and gives
/// the tokens and the comments' spans.
fn write_tokens(bytes: &[u8], p: &mut Printer) -> (Vec<Spanned>, Vec<Span>) {
    let (tokens, comments, warnings) = Lexer::from_bytes(bytes).tokenize_with_comments();
    section(p, "tokens");
    p.list(&tokens);
    section(p, "comments");
    p.list(&comments);
    section(p, "lexer warnings");
    p.list(&warnings);
    let comment_spans = comments.iter().map(|(span, _)| *span).collect();
    (tokens, comment_spans)
}

/// Writes what the parse gives that `ParsedFile` does not keep.
fn write_record(record: &ParseRecord, p: &mut Printer) {
    let ParseRecord {
        ignore_marks,
        goto_labels,
        references,
    } = record;
    section(p, "ignore marks");
    p.list(ignore_marks);
    section(p, "goto labels");
    p.list(goto_labels.iter());
    section(p, "doc sites");
    match references {
        Some(references) => {
            let syntax::references::FileReferences {
                names: _,
                outside: _,
                doc_sites: _,
                doc_frames,
            } = references;
            let sites: Vec<DocSiteReferences<'_>> = crate::deps::doc_site_references(references)
                .into_iter()
                .map(|(site, keys)| DocSiteReferences { site, keys })
                .collect();
            p.list(&sites);
            section(p, "doc frames");
            p.list(doc_frames);
        }
        // The names and doc sites the parse recorded are in `ParsedFile`'s
        // references, found by a walk after the stub attributes changed the
        // tree.
        None => p.atom("-"),
    }
}

/// A doc site and what its tags refer to, sorted.
struct DocSiteReferences<'a> {
    site: &'a syntax::references::DocSite,
    keys: crate::deps::DocSiteKeys,
}

impl Canonical for DocSiteReferences<'_> {
    fn canonical(&self, p: &mut Printer) {
        let DocSiteReferences {
            site,
            keys: crate::deps::DocSiteKeys { types, see },
        } = self;
        p.node(
            "DocSiteReferences",
            &[
                ("site", site),
                ("keys", &Sorted(types)),
                ("see", &Sorted(see)),
            ],
        );
    }
}

/// A list printed as a set.
struct Sorted<'a, T>(&'a [T]);

impl<T: Canonical> Canonical for Sorted<'_, T> {
    fn canonical(&self, p: &mut Printer) {
        let Sorted(items) = self;
        p.set(items.iter());
    }
}

/// Writes each doc comment's `DocBlock`, and the type each type node of its
/// tags reads back as.
fn write_docs(tokens: &(Vec<Spanned>, Vec<Span>), p: &mut Printer) {
    section(p, "docs");
    for (token, span, _) in &tokens.0 {
        let Token::DocComment(text) = token else {
            continue;
        };
        p.line();
        p.atom("doc ");
        span.canonical(p);
        p.atom(" ");
        phpdoc::docblock::parse_shared(text).canonical(p);
        let node = phpdoc::grammar::parse_shared(text);
        for (ordinal, tag) in node.tags.iter().enumerate() {
            for (slot, root) in tag_types(&tag.value) {
                write_type_nodes(*span, ordinal, &slot, root, p);
            }
        }
    }
}

/// The type nodes of a tag's value, each with the slot it fills.
fn tag_types(value: &PhpDocTagValue) -> Vec<(String, &TypeNode)> {
    let mut out = Vec::new();
    match value {
        PhpDocTagValue::TypedVar {
            ty,
            variable: _,
            by_ref: _,
            variadic: _,
            description: _,
        }
        | PhpDocTagValue::Type { ty, description: _ } => {
            out.extend(ty.iter().map(|ty| ("ty".to_string(), ty)));
        }
        PhpDocTagValue::Template {
            name: _,
            bound,
            default,
        } => {
            out.extend(bound.iter().map(|ty| ("bound".to_string(), ty)));
            out.extend(default.iter().map(|ty| ("default".to_string(), ty)));
        }
        PhpDocTagValue::ExtendsLike { ty } => out.push(("ty".to_string(), ty)),
        PhpDocTagValue::Method {
            is_static: _,
            return_type,
            name: _,
            templates,
            params,
        } => {
            out.extend(return_type.iter().map(|ty| ("return".to_string(), ty)));
            for (i, template) in templates.iter().enumerate() {
                let phpdoc::grammar::CallableTemplate {
                    name: _,
                    bound,
                    lower,
                    default,
                } = template;
                for (part, ty) in [("bound", bound), ("lower", lower), ("default", default)] {
                    out.extend(ty.iter().map(|ty| (format!("template{i}.{part}"), ty)));
                }
            }
            for (i, param) in params.iter().enumerate() {
                let phpdoc::grammar::MethodTagParam {
                    ty,
                    name: _,
                    variadic: _,
                    by_ref: _,
                    has_default: _,
                } = param;
                out.extend(ty.iter().map(|ty| (format!("param{i}"), ty)));
            }
        }
        PhpDocTagValue::Generic(_) => {}
    }
    out
}

/// Writes `node` and each node below it, keyed by `doc`, the tag's
/// `ordinal` and the node's `path`: its text, and the type the text reads
/// back as.
fn write_type_nodes(doc: Span, ordinal: usize, path: &str, node: &TypeNode, p: &mut Printer) {
    syntax::stack::guard(|| {
        let text = node.to_source_string();
        p.line();
        p.atom("node ");
        doc.canonical(p);
        p.atom(&format!(" {ordinal} {path}: "));
        p.bytes(text.as_bytes());
        p.atom(" => ");
        types::parse_written_type(&text).canonical(p);
        for (i, child) in type_node_children(node).into_iter().enumerate() {
            write_type_nodes(doc, ordinal, &format!("{path}.{i}"), child, p);
        }
    });
}

/// The nodes directly below `node`, in the order they are written.
fn type_node_children(node: &TypeNode) -> Vec<&TypeNode> {
    match node {
        TypeNode::Identifier(_)
        | TypeNode::Wildcard
        | TypeNode::IntRange(_, _)
        | TypeNode::IntLiteral(_)
        | TypeNode::FloatLiteral(_)
        | TypeNode::StringLiteral(_)
        | TypeNode::ConstFetch {
            class: _,
            const_name: _,
        }
        | TypeNode::Error => Vec::new(),
        TypeNode::Nullable(inner)
        | TypeNode::VarianceArg(_, inner)
        | TypeNode::ArrayOf(inner)
        | TypeNode::Paren(inner) => vec![&**inner],
        TypeNode::Union(members) | TypeNode::Intersection(members) => members.iter().collect(),
        TypeNode::Generic(_, args) => args.iter().collect(),
        TypeNode::OffsetAccess(base, offset) => vec![&**base, &**offset],
        TypeNode::ArrayShape {
            keyword: _,
            fields,
            sealed: _,
            unsealed_type,
        } => {
            let mut out: Vec<&TypeNode> = fields
                .iter()
                .map(|field| {
                    let phpdoc::grammar::ArrayShapeField {
                        key: _,
                        ty,
                        optional: _,
                    } = field;
                    ty
                })
                .collect();
            if let Some((key, value)) = unsealed_type {
                out.extend(key.as_deref());
                out.push(value);
            }
            out
        }
        TypeNode::Callable {
            keyword: _,
            templates,
            params,
            return_type,
        } => {
            let mut out = Vec::new();
            for template in templates {
                let phpdoc::grammar::CallableTemplate {
                    name: _,
                    bound,
                    lower,
                    default,
                } = template;
                out.extend(bound.iter().chain(lower).chain(default));
            }
            for param in params {
                let phpdoc::grammar::CallableParam {
                    ty,
                    variadic: _,
                    by_ref: _,
                    optional: _,
                } = param;
                out.push(ty);
            }
            out.extend(return_type.as_deref());
            out
        }
        TypeNode::Conditional {
            subject: _,
            negated: _,
            target,
            if_true,
            if_false,
        } => vec![&**target, &**if_true, &**if_false],
    }
}

/// Writes, at each offset a token or a comment starts or ends at, its line
/// and column, its UTF-8 and UTF-16 columns, and the offset each of those
/// columns gives back on its line.
fn write_positions(tokens: &(Vec<Spanned>, Vec<Span>), line_index: &LineIndex, p: &mut Printer) {
    section(p, "positions");
    let mut offsets: Vec<u32> = tokens
        .0
        .iter()
        .map(|(_, span, _)| span)
        .chain(&tokens.1)
        .flat_map(|span| [span.start, span.end])
        .collect();
    offsets.sort_unstable();
    offsets.dedup();
    for offset in offsets {
        let at = line_index.line_col(offset);
        let utf8 = line_index.column(offset, Unit::Utf8);
        let utf16 = line_index.column(offset, Unit::Utf16);
        let back8 = line_index.offset_in(at.line - 1, utf8, Unit::Utf8);
        let back16 = line_index.offset_in(at.line - 1, utf16, Unit::Utf16);
        p.line();
        p.atom(&format!("{offset} "));
        at.canonical(p);
        p.atom(&format!(" utf8 {utf8} utf16 {utf16} back "));
        back8.canonical(p);
        p.atom(" ");
        back16.canonical(p);
    }
}

/// Starts the section `name` on a line of its own.
fn section(p: &mut Printer, name: &str) {
    p.line();
    p.atom("== ");
    p.atom(name);
    p.line();
}
