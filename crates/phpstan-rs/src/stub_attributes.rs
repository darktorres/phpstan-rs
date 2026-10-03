//! phpstorm-stubs' version attributes, applied to a parsed file for the
//! newest PHP version and then removed:
//!
//! - `#[PhpStormStubsElementAvailable(from: 'X', to: 'Y')]` with a closed
//!   range (`to:`) marks a declaration or parameter that no longer exists.
//!   A function or method whose last attribute it is is dropped; a
//!   parameter is dropped when it has no default or when a neighbouring
//!   parameter redeclares its name (`array_map`'s superseded `$arrays`).
//! - `#[LanguageLevelTypeAware(['8.1' => 'T', ...], default: 'D')]` (also
//!   spelled `PhpVersionAware`) gives the newest version's type. Directly
//!   on an untyped parameter it becomes the parameter's native type (and an
//!   `@param` override in the nearest doc comment); anywhere else it
//!   becomes an `@return` override in the declaration's doc comment.
//!
//! A file may import either attribute under an alias
//! (`use JetBrains\PhpStorm\Internal\LanguageLevelTypeAware as Alias;`);
//! each is recognized by the class its name resolves to.

use std::sync::Arc;
use types::Name;

use syntax::ast::*;
use syntax::lexer::Lexer;
use syntax::parser::Parser;
use syntax::span::Span;
use syntax::token::Token;
use syntax::visit::{VisitorMut, walk_class_def_mut, walk_function_decl_mut, walk_stmt_mut};

#[derive(Clone, Copy, PartialEq, Eq)]
enum StubAttribute {
    Availability,
    TypeAware,
}

/// Which of the two attributes `attr` is, by the class it names, however
/// the file imports it.
fn stub_attribute(attr: &Attribute) -> Option<StubAttribute> {
    let name = attr.name.resolved_class().unwrap_or(&attr.name.written);
    match name.rsplit('\\').next().unwrap_or(name) {
        "PhpStormStubsElementAvailable" => Some(StubAttribute::Availability),
        "LanguageLevelTypeAware" | "PhpVersionAware" => Some(StubAttribute::TypeAware),
        _ => None,
    }
}

/// Whether an availability attribute gives a closed range (`to:`).
fn is_closed_range(attr: &Attribute) -> bool {
    attr.args.iter().any(|a| a.name.as_deref() == Some("to"))
}

/// Whether an attribute list ends with a closed-range availability
/// attribute, so the declaration it gates no longer exists.
fn ends_superseded(attrs: &[Attribute]) -> bool {
    attrs.last().is_some_and(|a| {
        stub_attribute(a) == Some(StubAttribute::Availability) && is_closed_range(a)
    })
}

fn string_literal(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => s.as_str(),
        _ => None,
    }
}

/// `major.minor`, compared numerically (`8.10` after `8.9`).
fn parse_version(s: &str) -> Option<(u32, u32)> {
    let (major, minor) = s.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// The type a type-aware attribute gives the newest PHP version: the
/// highest version key's, else `default:`'s.
fn newest_type(attr: &Attribute) -> Option<String> {
    let mut best: Option<((u32, u32), &str)> = None;
    if let Some(ExprKind::ArrayLiteral(items)) = attr
        .args
        .iter()
        .find(|a| a.name.is_none())
        .map(|a| &a.value.kind)
    {
        for item in items {
            let (Some(key), Some(ty)) = (
                item.key.as_ref().and_then(string_literal),
                string_literal(&item.value),
            ) else {
                break;
            };
            if let Some(version) = parse_version(key)
                && best.is_none_or(|(v, _)| version > v)
            {
                best = Some((version, ty));
            }
        }
    }
    best.map(|(_, ty)| ty.to_string()).or_else(|| {
        attr.args
            .iter()
            .find(|a| a.name.as_deref() == Some("default"))
            .and_then(|a| string_literal(&a.value))
            .map(str::to_string)
    })
}

/// Appends `line` (a `@return`/`@param` tag) before a doc comment's
/// closing `*/`; the doc parser keeps the last tag of a kind, so it wins
/// over an older one.
fn append_tag(doc: &str, line: &str) -> Arc<str> {
    match doc.rfind("*/") {
        Some(idx) => format!("{}\n * {line}\n {}", &doc[..idx], &doc[idx..]).into(),
        None => doc.into(),
    }
}

fn append_to(doc: &mut Option<Arc<str>>, line: &str) {
    if let Some(text) = doc {
        *text = append_tag(text, line);
    }
}

/// Parses a type attribute value as a native type declaration placed at
/// `at`.
fn parse_native_type(text: &str, at: Span) -> Option<TypeHint> {
    let tokens: Vec<_> = Lexer::new_in_php(text)
        .tokenize_with_spans()
        .into_iter()
        .filter(|(t, _, _)| *t != Token::Eof)
        .map(|(t, _, raw)| (t, at, raw))
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let mut hint = Parser::new(tokens).parse_type_hint().ok()??;
    set_spans(&mut hint, at);
    Some(hint)
}

fn set_spans(hint: &mut TypeHint, at: Span) {
    syntax::stack::guard(|| {
        hint.span = at;
        match &mut hint.kind {
            TypeHintKind::Keyword(..) | TypeHintKind::Name(_) => {}
            TypeHintKind::Nullable(inner) => set_spans(inner, at),
            TypeHintKind::Union(members) | TypeHintKind::Intersection(members) => {
                for m in members {
                    set_spans(m, at);
                }
            }
        }
    })
}

/// The `@return` overrides a declaration's type-aware attributes give, to
/// append to its doc comment.
fn return_overrides(attrs: &[Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|a| stub_attribute(a) == Some(StubAttribute::TypeAware))
        .filter_map(newest_type)
        .collect()
}

/// The bit of `FileFacts::mentions` a file naming one of the stub
/// attributes, in any case, however it imports them, has.
pub(crate) const MENTIONS: u32 = 1 << 0;

/// The spellings that set `MENTIONS`.
pub(crate) const WATCH: &[(&str, u32)] = &[
    ("phpversionaware", MENTIONS),
    ("languageleveltypeaware", MENTIONS),
    ("phpstormstubselementavailable", MENTIONS),
];

/// Applies and removes the version attributes in a parsed file, then
/// resolves the names of any type it added. Whether the tree changed.
pub(crate) fn apply_stub_attributes(stmts: &mut Vec<Stmt>) -> bool {
    if !mentions_stub_attributes(stmts) {
        return false;
    }
    let mut pass = Pass;
    pass.retain_stmts(stmts);
    for stmt in stmts.iter_mut() {
        pass.visit_stmt(stmt);
    }
    syntax::name_resolution::resolve_names(stmts);
    true
}

/// Whether any attribute in the file is one of the two.
fn mentions_stub_attributes(stmts: &[Stmt]) -> bool {
    struct Find {
        found: bool,
    }
    impl<'ast> syntax::visit::Visitor<'ast> for Find {
        fn visit_attribute(&mut self, attr: &'ast Attribute) {
            if stub_attribute(attr).is_some() {
                self.found = true;
            }
            syntax::visit::walk_attribute(self, attr);
        }
    }
    let mut find = Find { found: false };
    syntax::visit::walk_stmts(&mut find, stmts);
    find.found
}

struct Pass;

impl Pass {
    fn kind(&self, attr: &Attribute) -> Option<StubAttribute> {
        stub_attribute(attr)
    }

    fn strip(&self, attrs: &mut Vec<Attribute>) {
        attrs.retain(|a| self.kind(a).is_none());
    }

    fn return_overrides(&self, attrs: &[Attribute]) -> Vec<String> {
        return_overrides(attrs)
    }

    /// Drops the function declarations a closed range supersedes.
    fn retain_stmts(&self, stmts: &mut Vec<Stmt>) {
        stmts
            .retain(|s| !matches!(&s.kind, StmtKind::FunctionDecl(f) if ends_superseded(&f.attrs)));
    }

    /// Applies the parameter rules to one parameter list.
    fn params(&mut self, params: &mut Vec<Param>, function_doc: &mut Option<Arc<str>>) {
        let names: Vec<Name> = params.iter().map(|p| p.name.name.clone()).collect();
        let mut keep = vec![true; params.len()];
        for i in 0..params.len() {
            let superseded_here = params[i]
                .attrs
                .iter()
                .any(|a| self.kind(a) == Some(StubAttribute::Availability) && is_closed_range(a));
            if superseded_here {
                let name = &names[i];
                let redeclared = names.get(i + 1) == Some(name) || names[..i].contains(name);
                if params[i].default.is_none() || redeclared {
                    keep[i] = false;
                    continue;
                }
            }
            // A type-aware attribute directly before an untyped `$name`
            // types the parameter.
            if let Some(last) = params[i].attrs.last()
                && self.kind(last) == Some(StubAttribute::TypeAware)
                && params[i].type_hint.is_none()
                && let Some(ty) = newest_type(last)
                && !ty.contains(['[', ']'])
            {
                let at = Span {
                    start: params[i].name.span.start,
                    end: params[i].name.span.start,
                };
                if let Some(hint) = parse_native_type(&ty, at) {
                    params[i].type_hint = Some(hint);
                    let line = format!("@param {ty} ${}", names[i]);
                    // The nearest doc comment before it: an earlier
                    // parameter's, else the function's.
                    match params[..i]
                        .iter_mut()
                        .rev()
                        .find(|p| p.doc_comment.is_some())
                    {
                        Some(p) => append_to(&mut p.doc_comment, &line),
                        None => append_to(function_doc, &line),
                    }
                }
            }
        }
        let mut index = 0;
        params.retain(|_| {
            index += 1;
            keep[index - 1]
        });
        for p in params {
            self.strip(&mut p.attrs);
        }
    }

    fn function(&mut self, f: &mut FunctionDecl, stmt_doc: Option<&mut Option<Arc<str>>>) {
        let overrides = self.return_overrides(&f.attrs);
        for ty in &overrides {
            append_to(&mut f.doc_comment, &format!("@return {ty}"));
        }
        if let Some(stmt_doc) = stmt_doc {
            for ty in &overrides {
                append_to(stmt_doc, &format!("@return {ty}"));
            }
        }
        self.strip(&mut f.attrs);
        let mut doc = f.doc_comment.take();
        self.params(&mut f.params, &mut doc);
        f.doc_comment = doc;
        walk_function_decl_mut(self, f);
    }

    fn methods(&mut self, methods: &mut Vec<MethodDecl>) {
        methods.retain(|m| !ends_superseded(&m.function.attrs));
    }
}

impl VisitorMut for Pass {
    fn visit_stmt(&mut self, stmt: &mut Stmt) {
        match &mut stmt.kind {
            StmtKind::Block(stmts)
            | StmtKind::Declare {
                body: Some(stmts), ..
            } => {
                self.retain_stmts(stmts);
                walk_stmt_mut(self, stmt);
            }
            StmtKind::FunctionDecl(f) => self.function(f, Some(&mut stmt.doc_comment)),
            StmtKind::ClassDecl(c) => {
                for ty in self.return_overrides(&c.def.attrs) {
                    append_to(&mut c.def.doc_comment, &format!("@return {ty}"));
                    append_to(&mut stmt.doc_comment, &format!("@return {ty}"));
                }
                self.visit_class_decl(c);
            }
            _ => walk_stmt_mut(self, stmt),
        }
    }

    fn visit_function_decl(&mut self, f: &mut FunctionDecl) {
        self.function(f, None);
    }

    fn visit_class_def(&mut self, c: &mut ClassDef) {
        self.strip(&mut c.attrs);
        self.methods(&mut c.methods);
        for p in &mut c.properties {
            for ty in self.return_overrides(&p.attrs) {
                append_to(&mut p.doc_comment, &format!("@return {ty}"));
            }
            self.strip(&mut p.attrs);
        }
        for k in &mut c.consts {
            for ty in self.return_overrides(&k.attrs) {
                append_to(&mut k.doc_comment, &format!("@return {ty}"));
            }
            self.strip(&mut k.attrs);
        }
        for case in &mut c.enum_cases {
            self.strip(&mut case.attrs);
        }
        walk_class_def_mut(self, c);
    }

    fn visit_expr(&mut self, expr: &mut Expr) {
        match &mut expr.kind {
            ExprKind::Closure(closure) => {
                self.strip(&mut closure.attrs);
                self.params(&mut closure.params, &mut None);
            }
            ExprKind::ArrowFn(arrow) => {
                self.strip(&mut arrow.attrs);
                self.params(&mut arrow.params, &mut None);
            }
            _ => {}
        }
        syntax::visit::walk_expr_mut(self, expr);
    }
}

#[cfg(test)]
mod tests;
