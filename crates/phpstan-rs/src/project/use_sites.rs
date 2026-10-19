//! `textDocument/references`/`textDocument/rename`'s own whole-project
//! use-site index - two tiers, kept as two separate mechanisms because
//! they need fundamentally different machinery:
//!
//! Tier 1 (this module): class/function reference sites - a purely
//! syntactic, name-resolution-only walk (`ClassFnRefWalker`), structured
//! after (not sharing code with) `lsp::navigation::Walker` - the same
//! proven traversal shape, reused for "record every match", never "find
//! the one match at a cursor". Real caret-precision spans need the
//! file's retained source text (`index_class_fn_refs`, gated
//! behind `retain_source`); a
//! batch `check-project` run has no retained source but, when `project::
//! whole_program::dead_code` is enabled, still needs to know *whether* a
//! class/function is referenced at all -
//! `index_class_fn_refs_without_source` runs instead there, trading
//! precise sub-spans (needs real source text to carve out of a combined
//! multi-identifier span) for real, already-on-hand AST-node spans that
//! are merely less narrowly scoped - see that function's own doc
//! comment. Not run unconditionally, though: this walk is too
//! expensive to pay
//! for every loaded file (including every vendor/context one) when
//! nothing consumes it - `Project::class_fn_refs_of` gates it on
//! `Project::indexes_class_fn_sites`, see there.
//!
//! Tier 2 (member reference sites - method/property/class-constant
//! access) needs a real, checker-driven resolution of the receiver's
//! declaring owner - built separately, in `checker.rs`/`checker/expr.rs`,
//! and merged into this same index by `Project::ensure_member_index_built`/
//! `Project::reindex_tier2_for_file`.

use rustc_hash::FxHashMap;

use syntax::ast::*;
use syntax::span::Span;
use syntax::visit::{
    Visitor, walk_catch_clause, walk_class_def, walk_expr, walk_function_decl, walk_stmt,
    walk_stmts,
};

use super::lazy_context::{LazyRefs, PathPart};
use crate::checker::include_values::callee_name;
use crate::file_id::FileId;
use crate::shard::{join_map, split_map};

/// What a reference site points at - no local-variable variant (see
/// this item's own edge-case notes: local-variable rename/references is
/// explicitly out of v1 scope, always answered via a `None` short-circuit
/// before this index is ever consulted).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum RefTarget {
    Class(String),
    Function(String),
    /// `(declaring owner FQN, member name)` - always the true,
    /// ancestry-resolved declaring class, never the raw syntactic
    /// receiver/class name a `self::`/inherited-call site was written
    /// against.
    Member(String, String),
}

/// Every reference site this project has ever indexed, in both
/// directions: `by_target` answers "where is X used" (`references`'s own
/// query shape); `by_file` answers "what did this file contribute"
/// (`remove_file`'s own undo shape). A site's file is the project's number
/// for it.
#[derive(Default, Debug, PartialEq, Eq)]
pub(crate) struct UseSiteIndex {
    by_target: FxHashMap<RefTarget, Vec<(FileId, Span)>>,
    by_file: FxHashMap<FileId, Vec<(RefTarget, Span)>>,
}

impl UseSiteIndex {
    pub(crate) fn insert(&mut self, file: FileId, target: RefTarget, span: Span) {
        self.add_target_site(target.clone(), file, span);
        self.add_file_site(file, target, span);
    }

    /// Adds a site at `span` in `file` to the sites of `target`.
    pub(crate) fn add_target_site(&mut self, target: RefTarget, file: FileId, span: Span) {
        self.by_target.entry(target).or_default().push((file, span));
    }

    /// Adds a site of `target` at `span` to the sites of `file`.
    pub(crate) fn add_file_site(&mut self, file: FileId, target: RefTarget, span: Span) {
        self.by_file.entry(file).or_default().push((target, span));
    }

    /// Takes the index apart into `SHARDS` indexes for a merge. Each target's
    /// sites are in the index their target routes to, and each file's in the
    /// index its number routes to. `join_shards` puts them back together.
    pub(crate) fn split_shards(self) -> Vec<UseSiteIndex> {
        split_map(self.by_target)
            .into_iter()
            .zip(split_map(self.by_file))
            .map(|(by_target, by_file)| UseSiteIndex { by_target, by_file })
            .collect()
    }

    /// Joins the indexes `split_shards` made back into one.
    pub(crate) fn join_shards(shards: Vec<UseSiteIndex>) -> UseSiteIndex {
        let (by_target, by_file): (Vec<_>, Vec<_>) = shards
            .into_iter()
            .map(|shard| (shard.by_target, shard.by_file))
            .unzip();
        UseSiteIndex {
            by_target: join_map(by_target),
            by_file: join_map(by_file),
        }
    }

    /// Drops every site `file` previously contributed, from both
    /// directions - called unconditionally on every re-index/removal of
    /// `file` (a no-op before anything was ever inserted for it, in
    /// particular before Tier 2's own index is first built at all).
    pub(crate) fn remove_file(&mut self, file: FileId) {
        let Some(entries) = self.by_file.remove(&file) else {
            return;
        };
        for (target, span) in entries {
            if let Some(sites) = self.by_target.get_mut(&target) {
                sites.retain(|(f, s)| !(*f == file && *s == span));
                if sites.is_empty() {
                    self.by_target.remove(&target);
                }
            }
        }
    }

    pub(crate) fn references(&self, target: &RefTarget) -> &[(FileId, Span)] {
        self.by_target.get(target).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Resolves a bare class-name-shaped reference - structured after (not
/// sharing code with) `lsp::navigation::resolve_class_ref`, for the same
/// "independent reimplementation" reason `navigation.rs`'s own module
/// doc comment gives for its 4 siblings. Deliberately DIVERGES from it on
/// one point: `self`/`static`/`parent` resolve to `None` here, never a
/// real FQN. `navigation.rs`'s own version resolves them (correct for
/// read-only hover/go-to-definition, where jumping from a `self::`
/// reference to the current class's own declaration is exactly right) -
/// but this index feeds BOTH `references` and `rename`, and `rename`
/// literally rewrites a site's own source span to the new name. A
/// `self::CONST` site's span covers the literal text "self", not the
/// class's own name - renaming `Foo` to `Bar` must never turn `self::` in
/// `Foo`'s own body into `Bar::`, since `self` already means "whatever
/// this class is called," automatically, forever. Recording it as a real
/// `Foo` reference site would corrupt exactly that rename.
fn resolve_class_ref(name: &QualifiedName) -> Option<String> {
    name.resolved_class().map(str::to_string)
}

/// Every `(Span, RefTarget::Class|Function)` pair in `stmts` - Tier 1's
/// own entry point. `parent`/member halves of a `ClassConstFetch`/`StaticPropertyFetch`/
/// `StaticCall` are deliberately NOT resolved here (parent needs the
/// registry's own ancestor lookup only `resolve_class_ref` in
/// `navigation.rs` does via `registry.classes`, harmless to add later but
/// unneeded for Tier 1's own class/function scope; the member half is
/// Tier 2's job entirely, via a real Checker-resolved declaring owner).
pub(crate) fn index_class_fn_refs(stmts: &[Stmt], source: &str) -> Vec<(Span, RefTarget)> {
    let mut w = ClassFnRefWalker {
        source: Some(source),
        out: Vec::new(),
    };
    walk_stmts(&mut w, stmts);
    w.out
}

/// Same walk, for a caller with no retained source text at all (a batch
/// `check-project` run - `FileState.source` is `None` there, see
/// `Project::retain_source`'s own doc comment). Each of the 3 spots below
/// that would otherwise need `split_identifier_spans` to carve a precise
/// sub-span out of a combined multi-identifier text span instead records
/// the whole already-real AST span it has on hand - still a correct
/// reference *site* (this project's own convention for "the identifier's
/// own span, when its owning node doesn't carve out a narrower one"), just
/// not narrowed to the exact identifier's own characters. Existence-only
/// callers (a whole-program "was this ever referenced" check, e.g.
/// `project::whole_program::dead_code`) never read a use-site's span at
/// all, so the imprecision is invisible to them; a real caret-precision
/// consumer (`textDocument/references`/`rename`) only ever runs with
/// `retain_source` on, so it always gets `index_class_fn_refs`'s own exact
/// version above, never this one.
pub(crate) fn index_class_fn_refs_without_source(stmts: &[Stmt]) -> Vec<(Span, RefTarget)> {
    let mut w = ClassFnRefWalker {
        source: None,
        out: Vec::new(),
    };
    walk_stmts(&mut w, stmts);
    w.out
}

struct ClassFnRefWalker<'a> {
    source: Option<&'a str>,
    out: Vec<(Span, RefTarget)>,
}

impl ClassFnRefWalker<'_> {
    /// Records `name`, a class name, at its span, or at `whole` without
    /// the source: then a site is the span its node already has.
    fn class_at(&mut self, name: &QualifiedName, whole: Span) {
        if let Some(fqn) = resolve_class_ref(name) {
            let span = if self.source.is_some() {
                name.span
            } else {
                whole
            };
            self.out.push((span, RefTarget::Class(fqn)));
        }
    }

    /// Records `name`, a function name as a call or callable names it.
    fn function_at(&mut self, name: &QualifiedName, span: Span) {
        let fqn = name.candidates().0.to_string();
        self.out.push((span, RefTarget::Function(fqn)));
    }
}

impl<'ast> Visitor<'ast> for ClassFnRefWalker<'_> {
    fn visit_type_hint(&mut self, hint: &'ast TypeHint) {
        for name in hint.class_names() {
            self.class_at(name, hint.span);
        }
    }

    fn visit_class_def(&mut self, class: &'ast ClassDef) {
        for name in class
            .extends
            .iter()
            .chain(&class.implements)
            .chain(class.used_traits())
        {
            self.class_at(name, name.span);
        }
        walk_class_def(self, class);
    }

    fn visit_catch_clause(&mut self, clause: &'ast CatchClause) {
        for name in &clause.types {
            self.class_at(name, name.span);
        }
        walk_catch_clause(self, clause);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        stack::guard(|| {
            match &expr.kind {
                // The callee's name is recorded here, as a function or a
                // class; only the arguments are walked.
                ExprKind::Call(callee, args) | ExprKind::New(callee, args)
                    if matches!(callee.kind, ExprKind::Name(_)) =>
                {
                    let ExprKind::Name(n) = &callee.kind else {
                        unreachable!()
                    };
                    if matches!(expr.kind, ExprKind::Call(..)) {
                        self.function_at(n, callee.span);
                    } else {
                        self.class_at(n, callee.span);
                    }
                    for arg in args {
                        self.visit_arg(arg);
                    }
                    return;
                }
                // A bare `Name` reached any other way than a callee (e.g. a
                // first-class-callable target `foo(...)`) is taken as a
                // function.
                ExprKind::Name(n) => self.function_at(n, expr.span),
                ExprKind::ClassConstFetch(StaticTarget::Name(class), _)
                | ExprKind::ClassNameFetch(StaticTarget::Name(class))
                | ExprKind::StaticPropertyFetch(StaticTarget::Name(class), _)
                | ExprKind::DynamicStaticPropertyFetch(StaticTarget::Name(class), _)
                | ExprKind::StaticCall(StaticTarget::Name(class), _, _) => {
                    self.class_at(class, expr.span);
                }
                ExprKind::Instanceof(_, InstanceofTarget::Name(name)) => {
                    self.class_at(name, name.span);
                }
                _ => {}
            }
            walk_expr(self, expr);
        })
    }
}

/// What `stmts` hands to the lazy lookup. With bodies, an analysed file's:
/// each class its bodies name is checked, so it is needed. Without, a context
/// file's: its bodies run only when called and are not checked, so the classes
/// only they name are not needed.
pub(crate) fn lazy_refs(stmts: &[Stmt], bodies: bool) -> LazyRefs {
    let mut names = NameWalker {
        bodies,
        refs: LazyRefs::default(),
    };
    walk_stmts(&mut names, stmts);
    let mut constants = ConstWalker(Vec::new());
    walk_stmts(&mut constants, stmts);
    names.refs.constants = constants.0;
    names.refs
}

fn short_name(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

struct NameWalker {
    /// Whether a function's body is walked.
    bodies: bool,
    refs: LazyRefs,
}

impl<'ast> Visitor<'ast> for NameWalker {
    fn visit_name(&mut self, name: &'ast QualifiedName) {
        if let Some(fqn) = name.resolved_class() {
            self.refs.names.push(fqn.to_string());
        }
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        stack::guard(|| {
            if let ExprKind::Include(_, arg) = &expr.kind {
                self.refs.includes.push(path_parts_of(arg));
            }
            walk_expr(self, expr);
        })
    }

    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
        if self.bodies {
            walk_function_decl(self, function);
            return;
        }
        for attr in &function.attrs {
            self.visit_attribute(attr);
        }
        for param in &function.params {
            self.visit_param(param);
        }
        if let Some(hint) = &function.return_type {
            self.visit_type_hint(hint);
        }
    }
}

/// The constants a file defines: `const`, and `define()` with a literal name,
/// in any statement or body. A definition the load never runs can only make a
/// name defined twice, which reads as unknown, so every one is taken.
struct ConstWalker(Vec<(String, Vec<PathPart>)>);

impl<'ast> Visitor<'ast> for ConstWalker {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if let StmtKind::GlobalConst(consts) = &stmt.kind {
            for c in consts {
                self.0
                    .push((short_name(&c.name).to_string(), path_parts_of(&c.value)));
            }
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        stack::guard(|| {
            if let ExprKind::Call(callee, args) = &expr.kind
                && callee_name(callee).is_some_and(|n| n.eq_ignore_ascii_case("define"))
                && let [name, value, ..] = args.as_slice()
                && let ExprKind::Literal(Literal::String(name)) = &name.value.kind
            {
                self.0.push((
                    short_name(&name.to_str_lossy()).to_string(),
                    path_parts_of(&value.value),
                ));
            }
            walk_expr(self, expr);
        })
    }
}

/// The parts of an include's path, or of a constant's value.
fn path_parts_of(expr: &Expr) -> Vec<PathPart> {
    let mut parts = Vec::new();
    push_path_parts(expr, &mut parts);
    parts
}

fn push_path_parts(expr: &Expr, parts: &mut Vec<PathPart>) {
    let expr = expr.peeled();
    match &expr.kind {
        ExprKind::Binary(BinOp::Concat, lhs, rhs) => {
            push_path_parts(lhs, parts);
            push_path_parts(rhs, parts);
        }
        ExprKind::Literal(Literal::String(s)) => parts.push(match s.as_str() {
            Some(text) => PathPart::Text(text.to_string()),
            None => PathPart::Unknown,
        }),
        // The separator is the one `/`, which both worlds' paths are read with.
        ExprKind::Name(n)
            if n.trim_start_matches('\\')
                .eq_ignore_ascii_case("DIRECTORY_SEPARATOR") =>
        {
            parts.push(PathPart::Text("/".to_string()));
        }
        // `__DIR__` and the other magic constants, and any name that is not a
        // constant of the project, are not known text.
        ExprKind::Name(n) => {
            let name = n.trim_start_matches('\\');
            parts.push(if name.starts_with("__") && name.ends_with("__") {
                PathPart::Unknown
            } else {
                PathPart::Const(short_name(name).to_string())
            });
        }
        _ => parts.push(PathPart::Unknown),
    }
}

/// The class names a file's doc comments name, found in their text: each word
/// of a doc comment, resolved against the file's namespace and imports as a
/// class name is, each candidate kept. A PHPDoc type is a reference the checker
/// resolves, so the lazy context lookup follows it too. Two kinds of word name
/// no class and are left out: a tag, the word after an `@`, and a PHP reserved
/// word, which no class is named by. A word with a namespace is kept whole,
/// since a namespace segment may be a reserved word.
pub(crate) fn doc_names(source: &str) -> Vec<String> {
    // A file without a doc comment has none to read.
    if !source.contains("/**") {
        return Vec::new();
    }
    let mut namespace = String::new();
    let mut uses: FxHashMap<String, String> = FxHashMap::default();
    for line in source.lines() {
        if let Some(rest) = line.strip_prefix("namespace ") {
            namespace = rest.trim_end_matches(';').trim().to_ascii_lowercase();
        } else if let Some(rest) = line.strip_prefix("use ") {
            if rest.starts_with("function ") || rest.starts_with("const ") {
                continue;
            }
            let rest = rest.trim_end_matches(';').trim();
            let (target, alias) = match rest.rsplit_once(" as ") {
                Some((target, alias)) => (target.trim(), Some(alias.trim())),
                None => (rest, None),
            };
            let short = target.rsplit('\\').next().unwrap_or(target);
            uses.insert(
                alias.unwrap_or(short).to_ascii_lowercase(),
                target.trim_start_matches('\\').to_ascii_lowercase(),
            );
        }
    }
    let mut names: Vec<String> = Vec::new();
    let mut lower = String::new();
    let mut joined = String::new();
    let mut rest = source;
    while let Some(start) = rest.find("/**") {
        let after = &rest[start + 3..];
        let end = after.find("*/").unwrap_or(after.len());
        for (word, tag) in doc_words(&after[..end]) {
            let word = word.trim_matches('\\');
            if word.is_empty() || !word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
                continue;
            }
            lower.clear();
            lower.push_str(word);
            lower.make_ascii_lowercase();
            if !lower.contains('\\') && (tag || RESERVED.contains(&lower.as_str())) {
                continue;
            }
            let head = lower.split('\\').next().unwrap_or(&lower);
            if let Some(target) = uses.get(head) {
                match lower.split_once('\\') {
                    Some((_, tail)) => {
                        joined.clear();
                        joined.push_str(target);
                        joined.push('\\');
                        joined.push_str(tail);
                        add_name(&mut names, &joined);
                    }
                    None => add_name(&mut names, target),
                }
            }
            if !namespace.is_empty() {
                joined.clear();
                joined.push_str(&namespace);
                joined.push('\\');
                joined.push_str(&lower);
                add_name(&mut names, &joined);
            }
            add_name(&mut names, &lower);
        }
        rest = &after[end.min(after.len())..];
    }
    // A word repeats across a file's doc comments, and a name is kept once.
    names.sort_unstable();
    names.dedup();
    names
}

/// The words of a doc comment's text, each with whether an `@` comes just
/// before it.
fn doc_words(text: &str) -> Vec<(&str, bool)> {
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'\\';
    let bytes = text.as_bytes();
    let mut words = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if !is_word(bytes[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && is_word(bytes[at]) {
            at += 1;
        }
        words.push((&text[start..at], start > 0 && bytes[start - 1] == b'@'));
    }
    words
}

/// The words PHP 8 reserves, which no class is named by. Lowercase.
const RESERVED: [&str; 83] = [
    "abstract",
    "and",
    "array",
    "as",
    "bool",
    "break",
    "callable",
    "case",
    "catch",
    "class",
    "clone",
    "const",
    "continue",
    "declare",
    "default",
    "die",
    "do",
    "echo",
    "else",
    "elseif",
    "empty",
    "enddeclare",
    "endfor",
    "endforeach",
    "endif",
    "endswitch",
    "endwhile",
    "eval",
    "exit",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "fn",
    "for",
    "foreach",
    "function",
    "global",
    "goto",
    "if",
    "implements",
    "include",
    "include_once",
    "instanceof",
    "insteadof",
    "int",
    "interface",
    "isset",
    "iterable",
    "list",
    "match",
    "mixed",
    "namespace",
    "never",
    "new",
    "null",
    "object",
    "or",
    "parent",
    "print",
    "private",
    "protected",
    "public",
    "readonly",
    "require",
    "require_once",
    "return",
    "self",
    "static",
    "string",
    "switch",
    "throw",
    "trait",
    "true",
    "try",
    "unset",
    "use",
    "var",
    "void",
    "while",
    "xor",
    "yield",
];

fn add_name(names: &mut Vec<String>, name: &str) {
    names.push(name.to_string());
}

#[cfg(test)]
mod tests {
    use super::doc_names;

    #[test]
    fn doc_names_keep_each_name_once_resolved_against_the_namespace_and_uses() {
        let source = "<?php\nnamespace App\\Model;\nuse Lib\\Thing as T;\n/** @var T|Helper|T */\n";
        let mut names = doc_names(source);
        names.sort();
        assert_eq!(
            names,
            [
                "app\\model\\helper",
                "app\\model\\t",
                "helper",
                "lib\\thing",
                "t",
            ]
        );
    }

    #[test]
    fn doc_names_leave_out_tags_and_reserved_words_but_not_namespaced_ones() {
        let source =
            "<?php\nnamespace App\\Model;\n/** Describes array and Foo\\Class, see @param Bar */\n";
        let mut names = doc_names(source);
        names.sort();
        assert_eq!(
            names,
            [
                "app\\model\\bar",
                "app\\model\\describes",
                "app\\model\\foo\\class",
                "app\\model\\see",
                "bar",
                "describes",
                "foo\\class",
                "see",
            ]
        );
    }
}
