//! Generates the stub symbol index (`crates/stub-sources/index.tsv`, its
//! format documented in that crate) from the embedded stub sources, with
//! the parser alone. Shared by `cargo xtask stub-index`, which writes it,
//! and the package's `stub_index` test, which regenerates and compares.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use syntax::ParsedFile;
use syntax::ast::{
    Attribute, ClassKind, ExprId, ExprKind, Literal, ResolvedName, StmtId, StmtKind,
};

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
            let Ok(parsed) = syntax::parse(source.as_bytes()) else {
                continue;
            };
            let file = format!("{}:{path}", corpus.tag);
            let mut walk = Walk {
                file: &file,
                extension: *extension,
                parsed: &parsed,
                entries: &mut entries,
            };
            let stmts = parsed.statements().to_vec();
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
    parsed: &'a ParsedFile,
    entries: &'e mut BTreeMap<(&'static str, String), Entry>,
}

impl Walk<'_, '_> {
    fn stmts(&mut self, stmts: &[StmtId]) {
        for stmt_id in stmts {
            let Some(stmt) = self.parsed.stmt(*stmt_id) else {
                continue;
            };
            let kind = stmt.kind.clone();
            match &kind {
                StmtKind::Block(inner) => {
                    let inner = self.parsed.arenas.stmt_ids(inner).to_vec();
                    self.stmts(&inner);
                }
                StmtKind::Declare {
                    body: Some(inner), ..
                } => {
                    let inner = self.parsed.arenas.stmt_ids(inner).to_vec();
                    self.stmts(&inner);
                }
                StmtKind::If(if_stmt) => {
                    let bodies = if_stmt.bodies(&self.parsed.arenas).collect::<Vec<_>>();
                    for body in bodies {
                        self.stmts(std::slice::from_ref(&body));
                    }
                }
                StmtKind::ClassDecl(c) => {
                    let decl = self.parsed.arenas.class_decl(c.clone());
                    let Some(def) = self.parsed.arenas.class(decl.def) else {
                        continue;
                    };
                    let kind = match def.kind {
                        ClassKind::Interface => "interface",
                        ClassKind::Trait => "trait",
                        ClassKind::Enum => "enum",
                        ClassKind::Class => "class",
                    };
                    let name = decl.fq_name().to_string();
                    let extends = self.parsed.arenas.qualified_names(&def.extends);
                    let implements = self.parsed.arenas.qualified_names(&def.implements);
                    let parent = (def.kind == ClassKind::Class)
                        .then(|| extends.first().map(resolved))
                        .flatten();
                    let interfaces = if def.kind == ClassKind::Interface {
                        extends.iter().map(resolved).collect()
                    } else {
                        implements.iter().map(resolved).collect()
                    };
                    let attrs = self.parsed.arenas.attributes(def.attrs.clone()).to_vec();
                    let e = self.entry(kind, name.to_ascii_lowercase(), &name, &attrs);
                    if e.declarations.len() == 1 {
                        e.parent = parent;
                        e.interfaces = interfaces;
                    }
                }
                StmtKind::FunctionDecl(id) => {
                    let Some(f) = self.parsed.arenas.function(*id) else {
                        continue;
                    };
                    let name = declared(&f.namespaced_name);
                    let attrs = self.parsed.arenas.attributes(f.attrs.clone()).to_vec();
                    self.entry("function", name.to_ascii_lowercase(), &name, &attrs);
                }
                StmtKind::GlobalConst(consts) => {
                    let consts = self.parsed.arenas.class_consts(consts).to_vec();
                    for c in &consts {
                        let name = declared(&c.namespaced_name);
                        let key = constant_key(&name);
                        let value = syntax::printer::print_expr(&self.parsed.arenas, c.value);
                        let attrs = self.parsed.arenas.attributes(c.attrs.clone()).to_vec();
                        let e = self.entry("constant", key, &name, &attrs);
                        e.values.push(value);
                    }
                }
                StmtKind::Expression(e) => self.define(*e),
                _ => {}
            }
        }
    }

    /// A `define('NAME', value[, true])` call: a global constant, declared
    /// case-insensitive by a `true` third argument.
    fn define(&mut self, expression: ExprId) {
        let Some(e) = self.parsed.expr(expression) else {
            return;
        };
        let ExprKind::Call(callee, args) = &e.kind else {
            return;
        };
        let Some(callee) = self.parsed.expr(*callee) else {
            return;
        };
        let ExprKind::Name(function) = &callee.kind else {
            return;
        };
        let Some(function) = self.parsed.arenas.qualified_name(function) else {
            return;
        };
        if !function
            .written
            .trim_start_matches('\\')
            .eq_ignore_ascii_case("define")
        {
            return;
        }
        let args = self.parsed.arenas.args(args.clone());
        let Some(name) = args
            .first()
            .and_then(|a| string_literal(self.parsed, a.value))
        else {
            return;
        };
        let Some(value) = args.get(1) else {
            return;
        };
        let name = name.trim_start_matches('\\').to_string();
        let case_insensitive = args.get(2).is_some_and(|a| {
            self.parsed
                .expr(a.value)
                .is_some_and(|expr| match &expr.kind {
                    ExprKind::Literal(Literal::Bool(b)) => *b,
                    ExprKind::Name(n) => self
                        .parsed
                        .arenas
                        .qualified_name(n)
                        .is_some_and(|n| n.written.eq_ignore_ascii_case("true")),
                    _ => false,
                })
        });
        let key = if case_insensitive {
            name.to_ascii_lowercase()
        } else {
            constant_key(&name)
        };
        let value = syntax::printer::print_expr(&self.parsed.arenas, value.value);
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
        let declaration = match version_gate(attrs, self.parsed) {
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
fn version_gate(attrs: &[Attribute], parsed: &ParsedFile) -> Option<String> {
    let attr = attrs.iter().find(|a| {
        let name = match &a.name.resolved {
            ResolvedName::Exact(n) => n.as_ref(),
            _ => &a.name.written,
        };
        name.rsplit('\\').next() == Some("PhpStormStubsElementAvailable")
    })?;
    let arg = |label: &str, position: usize| -> Option<String> {
        let args = parsed.arenas.args(attr.args.clone());
        let by_name = args.iter().find(|a| a.name.as_deref() == Some(label));
        let positional = args.iter().filter(|a| a.name.is_none()).nth(position);
        by_name
            .or(positional)
            .and_then(|a| string_literal(parsed, a.value))
    };
    Some(format!(
        "{}-{}",
        arg("from", 0).unwrap_or_default(),
        arg("to", 1).unwrap_or_default()
    ))
}

fn string_literal(parsed: &ParsedFile, id: ExprId) -> Option<String> {
    let ExprKind::Literal(Literal::String(payload)) = &parsed.expr(id)?.kind else {
        return None;
    };
    parsed
        .arenas
        .owned_php_string(payload)
        .map(|value| value.to_str_lossy().into_owned())
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
