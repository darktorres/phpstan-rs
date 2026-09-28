//! Generates the stub symbol index (`crates/stub-sources/index.tsv`, its
//! format documented in that crate) from the embedded stub sources, with
//! the parser alone. Shared by `cargo xtask stub-index`, which writes it,
//! and the package's `stub_index` test, which regenerates and compares.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use syntax::ast::{
    Arg, Attribute, ClassKind, Expr, ExprKind, Literal, ResolvedName, Stmt, StmtKind,
};
use syntax::lexer::Lexer;
use syntax::parser::Parser;

/// One embedded corpus: its tag in the index, and each file's path, source
/// and (for phpstorm-stubs) extension.
pub(crate) struct Corpus<'a> {
    pub(crate) tag: &'a str,
    pub(crate) files: Vec<(&'a str, &'a str, Option<&'a str>)>,
}

#[derive(Default)]
struct Entry {
    name: String,
    case_insensitive: bool,
    declarations: Vec<String>,
    extension: Option<String>,
    parent: Option<String>,
    interfaces: Vec<String>,
    values: Vec<String>,
}

/// The index text for `corpora`, in priority order.
pub(crate) fn generate(corpora: &[Corpus]) -> String {
    let mut entries: BTreeMap<(&'static str, String), Entry> = BTreeMap::new();
    for corpus in corpora {
        for (path, source, extension) in &corpus.files {
            let tokens = Lexer::new(source).tokenize_with_spans();
            let Ok(mut stmts) = Parser::new(tokens).parse_program() else {
                continue;
            };
            syntax::name_resolution::resolve_names(&mut stmts);
            let file = format!("{}:{path}", corpus.tag);
            let mut walk = Walk {
                file: &file,
                extension: *extension,
                entries: &mut entries,
            };
            walk.stmts(&stmts);
        }
    }
    let mut out = String::from(
        "kind\tkey\tname\tflags\tdeclarations\textension\tparent\tinterfaces\tvalue\n",
    );
    for ((kind, key), e) in &entries {
        let value = match e.values.split_first() {
            None => String::new(),
            Some((first, rest)) if rest.iter().all(|v| v == first) => format!("={first}"),
            Some(_) => "*".to_string(),
        };
        writeln!(
            out,
            "{kind}\t{key}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            e.name,
            if e.case_insensitive { "ci" } else { "" },
            e.declarations.join(","),
            e.extension.as_deref().unwrap_or(""),
            e.parent.as_deref().unwrap_or(""),
            e.interfaces.join(","),
            escape(&value),
        )
        .unwrap();
    }
    out
}

/// A declaration's name with its namespace, which name resolution filled in.
fn declared(namespaced_name: &Option<std::sync::Arc<str>>) -> String {
    namespaced_name
        .as_deref()
        .expect("name resolution names every declaration statement")
        .to_string()
}

struct Walk<'a, 'e> {
    file: &'a str,
    extension: Option<&'a str>,
    entries: &'e mut BTreeMap<(&'static str, String), Entry>,
}

impl Walk<'_, '_> {
    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Block(inner)
                | StmtKind::Declare {
                    body: Some(inner), ..
                } => self.stmts(inner),
                StmtKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.stmts(std::slice::from_ref(then_branch));
                    if let Some(e) = else_branch {
                        self.stmts(std::slice::from_ref(e));
                    }
                }
                StmtKind::ClassDecl(c) => {
                    let kind = match c.kind {
                        ClassKind::Interface => "interface",
                        ClassKind::Trait => "trait",
                        ClassKind::Enum => "enum",
                        ClassKind::Class => "class",
                    };
                    let name = declared(&c.namespaced_name);
                    let parent = (c.kind == ClassKind::Class)
                        .then(|| c.extends.first().map(resolved))
                        .flatten();
                    let interfaces = if c.kind == ClassKind::Interface {
                        c.extends.iter().map(resolved).collect()
                    } else {
                        c.implements.iter().map(resolved).collect()
                    };
                    let e = self.entry(kind, name.to_ascii_lowercase(), &name, &c.attrs);
                    if e.declarations.len() == 1 {
                        e.parent = parent;
                        e.interfaces = interfaces;
                    }
                }
                StmtKind::EnumDecl(en) => {
                    let name = declared(&en.namespaced_name);
                    let interfaces = en.implements.iter().map(resolved).collect();
                    let e = self.entry("enum", name.to_ascii_lowercase(), &name, &[]);
                    if e.declarations.len() == 1 {
                        e.interfaces = interfaces;
                    }
                }
                StmtKind::FunctionDecl(f) => {
                    let name = declared(&f.namespaced_name);
                    self.entry("function", name.to_ascii_lowercase(), &name, &f.attrs);
                }
                StmtKind::GlobalConst(consts) => {
                    for c in consts {
                        let name = declared(&c.namespaced_name);
                        let key = constant_key(&name);
                        let value = syntax::printer::print_expr(&c.value);
                        let e = self.entry("constant", key, &name, &c.attrs);
                        e.values.push(value);
                    }
                }
                StmtKind::Expression(e) => self.define(e),
                _ => {}
            }
        }
    }

    /// A `define('NAME', value[, true])` call: a global constant, declared
    /// case-insensitive by a `true` third argument.
    fn define(&mut self, e: &Expr) {
        let ExprKind::Call(callee, args) = &e.kind else {
            return;
        };
        let ExprKind::Name(function) = &callee.kind else {
            return;
        };
        if !function
            .trim_start_matches('\\')
            .eq_ignore_ascii_case("define")
        {
            return;
        }
        let Some(name) = args.first().and_then(|a| string_literal(&a.value)) else {
            return;
        };
        let Some(value) = args.get(1) else {
            return;
        };
        let name = name.trim_start_matches('\\').to_string();
        let case_insensitive = args.get(2).is_some_and(|a| match &a.value.kind {
            ExprKind::Literal(Literal::Bool(b)) => *b,
            ExprKind::Name(n) => n.eq_ignore_ascii_case("true"),
            _ => false,
        });
        let key = if case_insensitive {
            name.to_ascii_lowercase()
        } else {
            constant_key(&name)
        };
        let value = syntax::printer::print_expr(&value.value);
        let e = self.entry("constant", key, &name, &[]);
        e.case_insensitive |= case_insensitive;
        e.values.push(value);
    }

    fn entry(
        &mut self,
        kind: &'static str,
        key: String,
        name: &str,
        attrs: &[Attribute],
    ) -> &mut Entry {
        let declaration = match version_gate(attrs) {
            Some(gate) => format!("{}@{gate}", self.file),
            None => self.file.to_string(),
        };
        let extension = self.extension;
        let e = self.entries.entry((kind, key)).or_default();
        if e.declarations.is_empty() {
            e.name = name.to_string();
            e.extension = extension.map(str::to_string);
        }
        e.declarations.push(declaration);
        e
    }
}

/// A constant's lookup key: its namespace lowercased, its own name exact.
fn constant_key(name: &str) -> String {
    match name.rsplit_once('\\') {
        Some((namespace, own)) => format!("{}\\{own}", namespace.to_ascii_lowercase()),
        None => name.to_string(),
    }
}

/// A declaration's `#[PhpStormStubsElementAvailable(from, to)]` range as
/// `from-to`, either side empty when open.
fn version_gate(attrs: &[Attribute]) -> Option<String> {
    let attr = attrs.iter().find(|a| {
        let name = match &a.name.resolved {
            ResolvedName::Exact(n) => n.as_ref(),
            _ => &a.name.written,
        };
        name.rsplit('\\').next() == Some("PhpStormStubsElementAvailable")
    })?;
    let arg = |label: &str, position: usize| -> Option<String> {
        let by_name = attr
            .args
            .iter()
            .find(|a: &&Arg| a.name.as_deref() == Some(label));
        let positional = attr.args.iter().filter(|a| a.name.is_none()).nth(position);
        by_name
            .or(positional)
            .and_then(|a| string_literal(&a.value))
    };
    Some(format!(
        "{}-{}",
        arg("from", 0).unwrap_or_default(),
        arg("to", 1).unwrap_or_default()
    ))
}

fn string_literal(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn resolved(name: &syntax::ast::QualifiedName) -> String {
    match &name.resolved {
        ResolvedName::Exact(n) | ResolvedName::WithGlobalFallback(n) => n.to_string(),
        ResolvedName::Unresolved => name.written.trim_start_matches('\\').to_string(),
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}
