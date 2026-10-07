//! `textDocument/completion`: two independent mechanisms sharing one
//! token-scan classifier (`classify`).
//!
//! **Bare position** (`Foo|`, a fresh identifier, or nothing at all) -
//! every declared class/enum/interface/trait and top-level function,
//! each with an auto-import edit when the current file doesn't already
//! have it in scope. Reads `Project::registry()` directly, UNIONED with
//! the current (possibly mid-edit, unparsed) file's own top-level
//! declared names (`project::declared_names`) - `Project::update_file`
//! removes a file's own registry entries *before* re-parsing, so while
//! the live buffer fails to parse, that file's own classes/functions
//! have already vanished from the registry even though they're still
//! real, valid declarations sitting a few lines away from the edit.
//!
//! **Member position** (`$x->|`, `Foo::|`, `$x->fo|`, `$x->method()::|`,
//! ...) - every method/property/const reachable on the receiver
//! expression's inferred type. The receiver is very often not part of
//! any successfully-parsed AST at all (`$x->` alone is a parse error),
//! so this can't reuse `navigation::resolve_at`'s "find the real AST
//! node" approach the way hover does. Instead: re-lex the live buffer,
//! find the enclosing function/class *by name* via a backward brace scan
//! (never by stale line numbers - see `find_enclosing_names`'s own doc
//! comment for why), look that name up in the last-good `Project::ast`,
//! clone it, drop every statement reaching at or past the edit line
//! (`truncate_before`), and append two synthetic statements: `$__phpstan_
//! rs_completion_probe = <receiver>;` then a bare read of that same
//! variable. Reusing `Workspace::probe_receiver_type` (originally built for
//! hover) to run this through the checker recovers the receiver's real,
//! flow-sensitive inferred type via the *already-shipped*
//! `ExprKind::Variable` probe hook (`checker/expr.rs`'s `self.probe_hit_
//! at(self.expr_span(e.id), &ty)` inside that arm) - no new Checker API, and uniform
//! for `->`/`::` alike (`ExprKind::ClassConstFetch`/`StaticCall`'s class
//! side is a bare `String`, not an `ExprNode`, so there is no AST-node shape
//! a `::` receiver could otherwise be spliced into).
//!
//! Truncating (not "inserting before the next statement") means a stale,
//! not-yet-reparsed version of whatever's on the edit's own line is
//! dropped entirely rather than re-run alongside the fresh probe - sound
//! (parameter-seeded locals are untouched; only body-statement-derived
//! narrowing from the dropped tail is lost) even though it can't
//! precisely re-run partial per-statement edits.

use rustc_hash::FxHashSet;
use std::path::Path;

use lsp_types::{CompletionItem, CompletionItemKind, TextEdit};

use crate::project::{self, Project};
use crate::registry::Registry;
use syntax::ast::{
    ClassKind, ExprId, ExprKind, ExprNode, FunctionBody, FunctionDecl, NodeArenas, StmtId,
    StmtKind, StmtNode, Visibility,
};
use syntax::span::{LineIndex, Span, Unit};
use syntax::token::{Keyword, TokenKind, TokenRef as Token, TokenStream};
use types::Type;

use super::navigation::ProbeScope;
use super::protocol::Positions;
use syntax::ParsedFile;
use syntax::name_resolution::{NameContext, context_at_ids};

/// The synthetic local `Locals`/the checker never sees a real declaration
/// for - deliberately unlikely to collide with a real user variable.
const PROBE_VAR: &str = "__phpstan_rs_completion_probe";

pub(super) enum Trigger {
    /// `$x->|`/`Foo::|`/`$x->fo|` - `anchor_idx` is the `Arrow`/
    /// `NullsafeArrow`/`DoubleColon` token's own index in the tokenized
    /// buffer; any partial member name already typed after it is
    /// ignored (real clients filter/re-rank the returned set themselves
    /// as more is typed - matching this project's own established "let
    /// the caller re-filter" convention elsewhere, e.g. `context.only`
    /// in `textDocument/codeAction`, rather than a server-side prefix
    /// filter that could disagree with the client's own).
    Member { anchor_idx: usize, is_static: bool },
    /// Everywhere else - offers every declared class/function.
    Bare,
}

/// Tokenizes the live buffer into its flat stream. Completion only reads
/// kinds, spans, and borrowed name payloads while scanning for a trigger.
pub(super) fn tokenize(text: &str) -> TokenStream {
    syntax::lexer::Lexer::new(text).lex_token_stream()
}

/// Classifies `cursor`'s trigger shape against an already-tokenized
/// buffer (see `tokenize`).
pub(super) fn classify(tokens: &TokenStream, cursor: u32) -> Trigger {
    // `Token::Eof` is a zero-width sentinel whose own span sits at (or
    // past) the very end of the buffer, so it always satisfies `span.end
    // <= cursor` too - excluded here, or it would always win `rposition`
    // over the real last token, e.g. the `Arrow` in `$x->` with the
    // cursor right after it.
    let Some(idx) = tokens
        .kinds
        .iter()
        .enumerate()
        .rposition(|(index, kind)| *kind != TokenKind::Eof && tokens.span_at(index).end <= cursor)
    else {
        return Trigger::Bare;
    };
    match tokens.kind_at(idx) {
        TokenKind::Arrow | TokenKind::NullsafeArrow => Trigger::Member {
            anchor_idx: idx,
            is_static: false,
        },
        TokenKind::DoubleColon => Trigger::Member {
            anchor_idx: idx,
            is_static: true,
        },
        TokenKind::Identifier if idx > 0 => match tokens.kind_at(idx - 1) {
            TokenKind::Arrow | TokenKind::NullsafeArrow => Trigger::Member {
                anchor_idx: idx - 1,
                is_static: false,
            },
            TokenKind::DoubleColon => Trigger::Member {
                anchor_idx: idx - 1,
                is_static: true,
            },
            _ => Trigger::Bare,
        },
        _ => Trigger::Bare,
    }
}

/// Walks backward from `anchor_idx` (the `->`/`::` token) tracking
/// paren/bracket/brace depth uniformly, stopping at the nearest
/// unmatched opening bracket or a `;` at depth 0 - the receiver
/// expression's own text always starts right there or later (a real
/// PHP statement never crosses one of those boundaries). Deliberately
/// generous rather than precise: including e.g. a leading `$total = `
/// assignment prefix still re-parses as one valid (chained-assignment)
/// expression whose value/type equals the true receiver's, so this
/// doesn't need to find the *minimal* receiver boundary, only a safe
/// one - either it parses as one expression (correct) or it doesn't
/// (graceful no-completions, never a wrong answer).
fn receiver_start_idx(tokens: &TokenStream, anchor_idx: usize) -> usize {
    let mut depth: i32 = 0;
    let mut i = anchor_idx;
    while i > 0 {
        i -= 1;
        match tokens.kind_at(i) {
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => depth += 1,
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => {
                if depth == 0 {
                    return i + 1;
                }
                depth -= 1;
            }
            TokenKind::Semicolon if depth == 0 => return i + 1,
            _ => {}
        }
    }
    0
}

/// Re-lexes and re-parses an isolated expression fragment (never the
/// whole file). The
/// fragment's own inner spans are deliberately left in slice-local
/// coordinates and never remapped: nothing downstream reads them (the
/// probe match is by the two sentinel statements' own spans, never the
/// receiver's internal ones, and any diagnostics this throwaway checker
/// pass produces are discarded the same way the existing hover probe
/// already discards its own).
/// The header token run immediately before `end_idx` (an opening `{`) -
/// back to the nearest preceding `;`/`}` at bracket/paren depth 0. Used
/// to find a `function <name> (` or `class|interface|trait|enum <name>`
/// pattern without needing to precisely skip a variable-length
/// extends/implements list or return-type annotation: the header is
/// captured whole, then scanned *forward* for the pattern instead.
fn header_start(tokens: &TokenStream, end_idx: usize) -> usize {
    let mut depth: i32 = 0;
    let mut i = end_idx;
    while i > 0 {
        i -= 1;
        match tokens.kind_at(i) {
            TokenKind::RBracket | TokenKind::RParen => depth += 1,
            TokenKind::LBracket | TokenKind::LParen => depth -= 1,
            TokenKind::Semicolon | TokenKind::RBrace if depth <= 0 => return i + 1,
            _ => {}
        }
    }
    0
}

fn function_name_before(tokens: &TokenStream, brace_idx: usize) -> Option<String> {
    let start = header_start(tokens, brace_idx);
    (start..brace_idx.saturating_sub(1)).find_map(|i| match (tokens.token_at(i), tokens.token_at(i + 1)) {
        (Token::Keyword(Keyword::Function), Token::Identifier(name)) => Some(name.to_string()),
        _ => None,
    })
}

fn class_name_before(tokens: &TokenStream, brace_idx: usize) -> Option<String> {
    let start = header_start(tokens, brace_idx);
    (start..brace_idx.saturating_sub(1)).find_map(|i| match (tokens.token_at(i), tokens.token_at(i + 1)) {
            (
                Token::Keyword(
                    Keyword::Class | Keyword::Interface | Keyword::Trait | Keyword::Enum,
                ),
                Token::Identifier(name),
            ) => Some(name.to_string()),
            _ => None,
        })
}

/// Finds the enclosing function/method (and, if any, its own enclosing
/// class) *by name*, via a backward brace scan over the LIVE buffer's
/// tokens - never by comparing the trigger's line number against the
/// last-good AST's own (potentially stale, post-edit-drifted) span
/// ranges. If lines were inserted/removed above the trigger since the
/// last successful parse, every real line number after that point has
/// drifted while the stale AST's spans haven't - a raw line-containment
/// check could then land inside a numerically-coincidental but
/// semantically wrong enclosing function. A name lookup that fails
/// finds nothing (falls back to `ProbeScope::TopLevel` below), it can
/// never silently substitute a *different*, wrongly-matching function.
fn find_enclosing_names(
    tokens: &TokenStream,
    anchor_idx: usize,
) -> (Option<String>, Option<String>) {
    let mut function_name = None;
    let mut class_name = None;
    let mut depth: i32 = 0;
    let mut i = anchor_idx;
    while i > 0 {
        i -= 1;
        match tokens.kind_at(i) {
            TokenKind::RBrace => depth += 1,
            TokenKind::LBrace if depth > 0 => depth -= 1,
            TokenKind::LBrace => {
                if function_name.is_none() {
                    match function_name_before(tokens, i) {
                        Some(name) => function_name = Some(name),
                        None => continue,
                    }
                } else {
                    class_name = class_name_before(tokens, i);
                    break;
                }
            }
            _ => {}
        }
    }
    (function_name, class_name)
}

fn find_function_by_name<'a>(
    file: &'a ParsedFile,
    class_name: Option<&str>,
    function_name: &str,
) -> Option<(FunctionDecl, bool)> {
    let arenas = &file.arenas;
    match class_name {
        Some(class_name) => file.statements().iter().find_map(|id| {
            let stmt = arenas.stmt(id.clone())?;
            let StmtKind::ClassDecl(decl) = &stmt.kind else {
                return None;
            };
            let decl = arenas.class_decl(decl.clone());
            if *decl.name != *class_name {
                return None;
            }
            let def = arenas.class(decl.def)?;
            let methods = arenas.methods(def.methods.clone());
            methods
                .iter()
                .find(|m| m.function.name.as_ref() == function_name)
                .map(|m| (m.function.clone(), m.is_static))
        }),
        None => file.statements().iter().find_map(|id| {
            let stmt = arenas.stmt(id.clone())?;
            let StmtKind::FunctionDecl(function) = &stmt.kind else {
                return None;
            };
            let function = arenas.function(function.clone())?;
            (function.name.as_ref() == function_name).then(|| (function.clone(), false))
        }),
    }
}

/// Two sentinel `Span`s built from an offset no real file reaches
/// (`u32::MAX - 1`/`u32::MAX - 3`) - can never collide with a real span, so
/// the probe match (`Checker::probe_span`, matched by exact span
/// equality) can never mistake a real statement for one of these two
/// synthetic ones. A false collision is not a hard impossibility, only
/// overwhelmingly impractical, and even then would only corrupt one
/// throwaway request's read, never panic or persist.
fn sentinel_span(at: u32) -> Span {
    Span {
        start: at,
        end: at + 1,
    }
}

/// The two synthetic statements every member-completion probe splices
/// in: `$__phpstan_rs_completion_probe = <receiver>;` then a bare read
/// of that same variable, whose span is `probe_span` below - reusing
/// the checker's already-shipped `ExprKind::Assign` (binds the probe
/// variable's type from the receiver) and `ExprKind::Variable` read
/// (`checker/expr.rs`'s own `self.probe_hit_at(self.expr_span(e.id), &ty)`) rather
/// than any new Checker-side mechanism.
fn probe_statements(file: &mut ParsedFile, receiver: ExprId) -> (StmtId, StmtId, Span) {
    let assign_span = sentinel_span(u32::MAX - 3);
    let read_span = sentinel_span(u32::MAX - 1);
    let alloc_expr = |file: &mut ParsedFile, kind, span| {
        let id = file.arenas.next_expr_id();
        file.arenas.alloc_expr(ExprNode { id, kind }, span)
    };
    let probe_var =
        |file: &mut ParsedFile, span| alloc_expr(file, ExprKind::Variable(PROBE_VAR.into()), span);
    let lhs = probe_var(file, assign_span);
    let assign_expr = alloc_expr(file, ExprKind::Assign(lhs, receiver), assign_span);
    let assign_stmt = StmtNode {
        id: file.arenas.next_stmt_id(),
        kind: StmtKind::Expression(assign_expr),
    };
    let assign_stmt = file.arenas.alloc_stmt(assign_stmt, assign_span);
    let read_expr = probe_var(file, read_span);
    let read_stmt = StmtNode {
        id: file.arenas.next_stmt_id(),
        kind: StmtKind::Expression(read_expr),
    };
    let read_stmt = file.arenas.alloc_stmt(read_stmt, read_span);
    (assign_stmt, read_stmt, read_span)
}

/// Builds the `ProbeScope` a member-completion request re-checks:
/// whichever named function/method `find_enclosing_names` found (looked
/// up by name in the last-good AST, cloned via `Arc::make_mut` on its
/// `body`), or the whole file's own top-level statements when no
/// enclosing function was found (or it no longer exists under that
/// name, e.g. renamed in this same still-unparsed edit; degrades to a
/// coarser but never wrong scope, same as the file's script-level scope
/// hover already falls back to). Either way, every statement reaching at
/// or past `cut_line` (a line of the live buffer, read as a line of the
/// last-good AST, whose lines `index` holds) is dropped before the two probe statements are
/// appended - see this module's own doc comment for why truncating
/// (never "insert before") is the sound choice.
fn build_probe_scope(
    mut file: ParsedFile,
    class_name: Option<&str>,
    function_name: Option<&str>,
    cut_line: u32,
    index: &LineIndex,
    receiver: ExprId,
) -> (ProbeScope, bool, Option<String>, Span, ParsedFile) {
    let (assign_stmt, read_stmt, probe_span) = probe_statements(&mut file, receiver);
    let found = function_name.and_then(|f| find_function_by_name(&file, class_name, f));
    match found {
        Some((mut cloned, is_static)) => {
            let f_span = cloned.span;
            let empty_stmts = file.arenas.alloc_stmt_ids([]);
            let body = cloned.body.get_or_insert_with(|| FunctionBody {
                open: f_span,
                stmts: empty_stmts,
                goto_labels: Box::default(),
                is_generator: false,
            });
            let mut retained: Vec<StmtId> = file
                .arenas
                .stmt_ids(&body.stmts)
                .iter()
                .copied()
                .filter(|id| {
                    file.arenas
                        .stmt_span(*id)
                        .is_some_and(|span| index.line(span.end) < cut_line)
                })
                .collect();
            retained.extend([assign_stmt, read_stmt]);
            body.stmts = file.arenas.alloc_stmt_ids(retained);
            (
                ProbeScope::Function {
                    function: Box::new(cloned),
                    owner: None,
                },
                is_static,
                class_name.map(str::to_string),
                probe_span,
                file,
            )
        }
        None => {
            let mut top: Vec<StmtId> = file
                .statements()
                .iter()
                .filter(|id| {
                    file.arenas
                        .stmt_span(**id)
                        .is_some_and(|span| index.line(span.end) < cut_line)
                })
                .copied()
                .collect();
            top.push(assign_stmt);
            top.push(read_stmt);
            file.stmts = file.arenas.alloc_stmt_ids(top.iter().copied());
            (
                ProbeScope::TopLevel {
                    statements: top,
                    owner: None,
                },
                false,
                None,
                probe_span,
                file,
            )
        }
    }
}

fn is_visible(
    registry: &Registry,
    owner: &str,
    visibility: &Visibility,
    current_class: Option<&str>,
) -> bool {
    match visibility {
        Visibility::Public => true,
        Visibility::Private => current_class == Some(owner),
        Visibility::Protected => current_class.is_some_and(|c| {
            c == owner || registry.is_subclass_of(c, owner) || registry.is_subclass_of(owner, c)
        }),
    }
}

/// Every method/property/const/enum-case reachable on `ty`, filtered to
/// what's actually syntactically valid after the trigger that produced
/// `is_static` (a plain identifier method/property/const label - a
/// static *property* is additionally offered `$`-prefixed, since
/// `Foo::prop` alone is invalid PHP, only `Foo::$prop` is) and to what
/// `current_class` can actually see (`is_visible`).
pub(super) fn enumerate_members(
    registry: &Registry,
    ty: &Type,
    is_static: bool,
    current_class: Option<&str>,
) -> Vec<CompletionItem> {
    let classes: Vec<String> = ty
        .object_class_names()
        .iter()
        .map(|c| c.to_string())
        .collect();
    let mut seen: FxHashSet<(u8, String)> = FxHashSet::default();
    let mut items = Vec::new();

    for class in &classes {
        for name in registry.all_method_names(class) {
            if !seen.insert((0, name.to_string())) {
                continue;
            }
            let Some((owner, sig)) = registry.lookup_method_with_owner(class, name) else {
                continue;
            };
            if !is_visible(registry, &owner, &sig.visibility(), current_class) {
                continue;
            }
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(owner.to_string()),
                ..Default::default()
            });
        }

        if is_static {
            for name in registry.all_const_names(class) {
                if !seen.insert((1, name.to_string())) {
                    continue;
                }
                let Some((owner, sig)) = registry.lookup_const_with_owner(class, name) else {
                    continue;
                };
                let constant_type = registry
                    .lookup_const_type(class, name)
                    .unwrap_or_else(|| sig.ty());
                if !is_visible(registry, &owner, &sig.visibility(), current_class) {
                    continue;
                }
                let kind = if constant_type.is_enum_case() {
                    CompletionItemKind::ENUM_MEMBER
                } else {
                    CompletionItemKind::CONSTANT
                };
                items.push(CompletionItem {
                    label: name.to_string(),
                    kind: Some(kind),
                    detail: Some(owner.to_string()),
                    ..Default::default()
                });
            }
            for name in registry.all_property_names(class) {
                let Some((owner, sig)) = registry.lookup_property_with_owner(class, name) else {
                    continue;
                };
                if !sig.is_static() || !seen.insert((2, name.to_string())) {
                    continue;
                }
                if !is_visible(registry, &owner, &sig.visibility(), current_class) {
                    continue;
                }
                items.push(CompletionItem {
                    label: format!("${name}"),
                    kind: Some(CompletionItemKind::PROPERTY),
                    detail: Some(owner.to_string()),
                    ..Default::default()
                });
            }
        } else {
            for name in registry.all_property_names(class) {
                let Some((owner, sig)) = registry.lookup_property_with_owner(class, name) else {
                    continue;
                };
                if sig.is_static() || !seen.insert((2, name.to_string())) {
                    continue;
                }
                if !is_visible(registry, &owner, &sig.visibility(), current_class) {
                    continue;
                }
                items.push(CompletionItem {
                    label: name.to_string(),
                    kind: Some(CompletionItemKind::PROPERTY),
                    detail: Some(owner.to_string()),
                    ..Default::default()
                });
            }
        }
    }
    items
}

fn class_kind_completion_kind(kind: ClassKind) -> CompletionItemKind {
    match kind {
        ClassKind::Class | ClassKind::Trait => CompletionItemKind::CLASS,
        ClassKind::Interface => CompletionItemKind::INTERFACE,
        ClassKind::Enum => CompletionItemKind::ENUM,
    }
}

fn short_name(fqn: &str) -> &str {
    fqn.rsplit('\\').next().unwrap_or(fqn)
}

/// Where a new `use X;` this file doesn't already have gets inserted -
/// right after the last existing `use`/`namespace` statement. `None`
/// (neither exists) degrades to a fully-qualified insert with no edit at
/// all (see `resolve_import`) rather than guessing a top-of-file
/// insertion point.
fn find_use_insertion_point(arenas: &NodeArenas, stmts: &[StmtId]) -> Option<u32> {
    stmts
        .iter()
        .filter_map(|id| arenas.stmt(id.clone()))
        .rfind(|stmt| matches!(&stmt.kind, StmtKind::Use(_) | StmtKind::Namespace(_)))
        .and_then(|s| arenas.stmt_span(s.id).map(|span| span.end))
}

/// Decides how a bare-completion candidate FQN should actually be
/// inserted: as its own short name, with a new `use` edit alongside it
/// (the common case for a class not yet imported in this file); as its
/// bare short name with NO edit, when it's already reachable that way
/// (already `use`-imported, or declared in this exact namespace); or, on
/// a short-name collision with something else already in scope, as a
/// fully-qualified `\Some\Fqn` with no edit at all, rather than silently
/// importing over an unrelated existing name.
fn resolve_import(
    arenas: &NodeArenas,
    stmts: &[StmtId],
    index: Positions<'_>,
    ctx: &NameContext,
    fqn: &str,
) -> (String, Option<TextEdit>) {
    let short = short_name(fqn);
    if ctx.resolves_to(short, fqn) {
        return (short.to_string(), None);
    }
    if ctx.is_use_imported(short) {
        return (format!("\\{fqn}"), None);
    }
    match find_use_insertion_point(arenas, stmts) {
        Some(pos) => {
            let insert_span = Span {
                start: pos,
                end: pos,
            };
            (
                short.to_string(),
                Some(TextEdit {
                    range: index.range(insert_span),
                    new_text: {
                        let mut text = String::with_capacity(fqn.len() + 6);
                        text.push_str("\nuse ");
                        text.push_str(fqn);
                        text.push(';');
                        text
                    },
                }),
            )
        }
        None => (format!("\\{fqn}"), None),
    }
}

/// Every declared class/enum/interface/trait and top-level function,
/// each with an auto-import edit unless already reachable. Unions the
/// registry's own candidates with `project::declared_names` on the
/// current file's own (possibly stale, if this file's own edit is what
/// broke parsing) AST - see this module's own doc comment for why the
/// registry alone under-reports during exactly the moment completion is
/// most commonly triggered. Names resolve in the context at `cursor`.
pub(super) fn enumerate_bare(
    project: &Project,
    path: &Path,
    current_arenas: &NodeArenas,
    current_stmts: &[StmtId],
    cursor: u32,
    unit: Unit,
) -> Vec<CompletionItem> {
    let ctx = context_at_ids(current_arenas, current_stmts, cursor);
    let registry = project.registry();
    let Some(index) = project.line_index(path) else {
        return Vec::new();
    };
    let index = Positions { index, unit };
    let mut seen: FxHashSet<String> = FxHashSet::default();
    let mut items = Vec::new();

    for (_, fqn, info) in registry.classes_by_id() {
        if seen.insert(fqn.to_string()) {
            let (insert_text, edit) =
                resolve_import(current_arenas, current_stmts, index, &ctx, fqn);
            items.push(CompletionItem {
                label: short_name(fqn).to_string(),
                kind: Some(class_kind_completion_kind(info.kind())),
                insert_text: Some(insert_text),
                additional_text_edits: edit.map(|e| vec![e]),
                ..Default::default()
            });
        }
    }
    for (_, fqn, _) in registry.functions_by_id() {
        if seen.insert(fqn.to_string()) {
            let (insert_text, edit) =
                resolve_import(current_arenas, current_stmts, index, &ctx, fqn);
            items.push(CompletionItem {
                label: short_name(fqn).to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                insert_text: Some(insert_text),
                additional_text_edits: edit.map(|e| vec![e]),
                ..Default::default()
            });
        }
    }

    // `Project::update_file` removes this exact file's own registry
    // entries before re-parsing it, so while the live buffer fails to
    // parse (the overwhelmingly common state at the moment completion
    // is triggered - `Foo->` isn't valid PHP on its own), this file's
    // own not-otherwise-touched declarations have already vanished from
    // the registry above. `project::declared_names` is the exact
    // traversal `update_file` itself uses to know what to remove/re-add
    // - reused here, not re-derived.
    if let Some(current_ast) = project.ast_of(path) {
        let (functions, classes) =
            project::declared_names(current_ast.arenas(), current_ast.statements());
        for fqn in functions.iter().chain(&classes) {
            if seen.insert(fqn.to_string()) {
                let (insert_text, edit) = resolve_import(
                    current_ast.arenas(),
                    current_ast.statements(),
                    index,
                    &ctx,
                    fqn,
                );
                let kind = if classes.contains(fqn) {
                    CompletionItemKind::CLASS
                } else {
                    CompletionItemKind::FUNCTION
                };
                items.push(CompletionItem {
                    label: short_name(fqn).to_string(),
                    kind: Some(kind),
                    insert_text: Some(insert_text),
                    additional_text_edits: edit.map(|e| vec![e]),
                    ..Default::default()
                });
            }
        }
    }
    items
}

/// What a member-completion trigger needs next: either the receiver's
/// type is already known outright (`self`/`static`/`parent`, or a
/// literal class-name reference before `::` - a bare `ExprKind::Name`
/// has no *value* of its own the general splice-and-probe mechanism
/// below could read a type off; it's a name reference, resolved the same
/// way `navigation::resolve_class_ref` already resolves one for hover),
/// or it needs the general probe (`Workspace::probe_receiver_type`).
pub(super) enum MemberProbe {
    Resolved {
        ty: Type,
        is_static: bool,
        class: Option<String>,
    },
    NeedsProbe {
        owner: std::sync::Arc<ParsedFile>,
        scope: ProbeScope,
        scope_is_static: bool,
        class: Option<String>,
        probe_span: Span,
        is_static: bool,
    },
}

/// Everything `Workspace::completion` needs, factored out so it's directly
/// unit-testable without a `Server`: given the live buffer's full text
/// and tokens, the cursor position, and the last-good AST for `path`,
/// returns what's needed to answer a member-completion trigger, or
/// `None` for a bare trigger (the caller enumerates bare candidates
/// itself, which needs `Project`/`path`, not just the AST slice this
/// function is given) or an unparseable/malformed receiver.
pub(super) fn build_member_probe(
    text: &str,
    tokens: &TokenStream,
    cursor: Cursor,
    file: &ParsedFile,
    registry: &Registry,
) -> Option<MemberProbe> {
    let Trigger::Member {
        anchor_idx,
        is_static,
    } = classify(tokens, cursor.live)
    else {
        return None;
    };
    let start_idx = receiver_start_idx(tokens, anchor_idx);
    if start_idx >= anchor_idx {
        return None;
    }
    let receiver_span = Span {
        start: tokens.span_at(start_idx).start,
        end: tokens.span_at(anchor_idx).start,
    };
    let receiver_text = slice_span(text, receiver_span)?;
    let context = context_at_ids(&file.arenas, file.statements(), cursor.stale);
    let mut owner = file.clone();
    let receiver =
        syntax::parse_expression_into_arenas(&receiver_text, &context, &mut owner.arenas)?;
    let (function_name, class_name) = find_enclosing_names(tokens, anchor_idx);

    // The cheap path: `self`/`static`/`parent`/a literal (possibly
    // namespace-qualified) class name before `::` needs no probe at
    // all - it's a name reference, not a value, so the general
    // assign-then-read splice below (which reads back a *value*'s
    // inferred type) doesn't apply to it in the first place.
    if is_static
        && let Some(ExprNode {
            kind: ExprKind::Name(name),
            ..
        }) = owner.arenas.expr(receiver)
        && let Some(name) = owner.arenas.qualified_name(name)
    {
        let mut name = name.clone();
        syntax::name_resolution::resolve_class_name(&mut name, &context);
        let resolved =
            super::navigation::resolve_class_ref(&name, class_name.as_deref(), registry)?;
        return Some(MemberProbe::Resolved {
            ty: Type::object(resolved.into(), Vec::new()),
            is_static,
            class: class_name,
        });
    }

    let cut_line = cursor.live_index.line(tokens.span_at(anchor_idx).start);
    let (scope, scope_is_static, scope_class, probe_span, owner) = build_probe_scope(
        owner,
        class_name.as_deref(),
        function_name.as_deref(),
        cut_line,
        cursor.stale_index,
        receiver,
    );
    Some(MemberProbe::NeedsProbe {
        owner: std::sync::Arc::new(owner),
        scope,
        scope_is_static,
        class: scope_class,
        probe_span,
        is_static,
    })
}

/// Where the cursor is: in the live buffer, whose tokens are scanned, and
/// in the last-good AST, which the live position is read as a position of.
#[derive(Clone, Copy)]
pub(super) struct Cursor<'a> {
    /// The byte offset in the live buffer.
    pub(super) live: u32,
    /// The lines of the live buffer.
    pub(super) live_index: &'a LineIndex,
    /// The byte offset of the same line and column in the last-good AST's
    /// file.
    pub(super) stale: u32,
    /// The lines of the last-good AST's file.
    pub(super) stale_index: &'a LineIndex,
}

/// `source`'s own substring covered by `span`, a range of its bytes.
pub(crate) fn slice_span(source: &str, span: Span) -> Option<String> {
    source
        .get(span.start as usize..span.end as usize)
        .map(str::to_string)
}

#[cfg(test)]
mod tests;
