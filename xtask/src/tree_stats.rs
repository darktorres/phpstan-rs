//! `tree-stats`: what the parser's tree holds in the PHP files under some
//! directories, for the census that sizes the flat tree (SYN-1.11). It counts
//! the live nodes only, those a walk from each file's statements reaches: the
//! expression and statement nodes by kind, the tokens and the tokens per node,
//! the lengths of the lists it counts (call arguments, array items and
//! statement lists) as a histogram, the arguments that are named, the
//! parameters that have attributes, the functions with a doc comment, and the
//! bytes of the node records at the layout the tree has now. The heap a node
//! points into is not counted.
//!
//! Each directory gets one summary. `--files` adds a line per file before the
//! summaries: its path, its tokens, its expression and statement nodes, and its
//! tokens per node.

use std::collections::BTreeMap;
use std::fmt::{self, Debug, Write as _};
use std::path::Path;

use syntax::ast::{Arg, Expr, ExprKind, FunctionDecl, Stmt, StmtKind};
use syntax::lexer::Lexer;
use syntax::visit::{Visitor, walk_expr, walk_function_decl, walk_stmt, walk_stmts};

use crate::front_end::php_files;

/// The census recurses through the tree the way the checker does, on a thread
/// with room for a deep one.
const STACK: usize = 256 << 20;

/// The labels of a list-length histogram's buckets, in order.
const BUCKETS: [&str; 8] = ["0", "1", "2", "3-4", "5-8", "9-16", "17-32", "33+"];

/// The bucket a list of `len` items falls in.
fn bucket(len: usize) -> usize {
    match len {
        0 => 0,
        1 => 1,
        2 => 2,
        3..=4 => 3,
        5..=8 => 4,
        9..=16 => 5,
        17..=32 => 6,
        _ => 7,
    }
}

/// The name of a node's kind: the first thing its derived `Debug` writes is the
/// variant's name, so the writer stops there and the fields are never formatted.
fn variant_name(kind: &impl Debug) -> String {
    struct FirstWrite(String);
    impl fmt::Write for FirstWrite {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            self.0.push_str(text);
            Err(fmt::Error)
        }
    }
    let mut first = FirstWrite(String::new());
    let _ = write!(first, "{kind:?}");
    first.0
}

/// The nodes, lists and fields counted so far.
#[derive(Default)]
struct Census {
    exprs: BTreeMap<String, u64>,
    stmts: BTreeMap<String, u64>,
    lists: BTreeMap<&'static str, [u64; 8]>,
    /// Call arguments, and those that are named.
    args: u64,
    named_args: u64,
    /// Parameters of named functions and methods, and those with attributes.
    params: u64,
    params_with_attrs: u64,
    /// Named functions and methods, and those with a doc comment.
    functions: u64,
    functions_with_doc: u64,
}

impl Census {
    fn list(&mut self, name: &'static str, len: usize) {
        self.lists.entry(name).or_default()[bucket(len)] += 1;
    }

    fn arguments(&mut self, args: &[Arg]) {
        self.args += args.len() as u64;
        self.named_args += args.iter().filter(|arg| arg.name.is_some()).count() as u64;
        self.list("call arguments", args.len());
    }

    fn expr_count(&self) -> u64 {
        self.exprs.values().sum()
    }

    fn stmt_count(&self) -> u64 {
        self.stmts.values().sum()
    }
}

impl<'ast> Visitor<'ast> for Census {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        *self.stmts.entry(variant_name(&stmt.kind)).or_default() += 1;
        if let StmtKind::Block(stmts) = &stmt.kind {
            self.list("statement lists", stmts.len());
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        *self.exprs.entry(variant_name(&expr.kind)).or_default() += 1;
        match &expr.kind {
            ExprKind::Call(_, args)
            | ExprKind::New(_, args)
            | ExprKind::MethodCall(_, _, args, _)
            | ExprKind::DynamicMethodCall(_, _, args, _)
            | ExprKind::StaticCall(_, _, args)
            | ExprKind::DynamicStaticCall(_, _, args) => self.arguments(args),
            ExprKind::ArrayLiteral(items) => self.list("array items", items.len()),
            _ => {}
        }
        walk_expr(self, expr);
    }

    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
        self.functions += 1;
        if function.doc_comment.is_some() {
            self.functions_with_doc += 1;
        }
        self.params += function.params.len() as u64;
        self.params_with_attrs += function
            .params
            .iter()
            .filter(|param| !param.attrs.is_empty())
            .count() as u64;
        if let Some(body) = &function.body {
            self.list("statement lists", body.stmts.len());
        }
        walk_function_decl(self, function);
    }
}

/// What one directory's files come to.
#[derive(Default)]
struct Totals {
    files: u64,
    failed: u64,
    tokens: u64,
    census: Census,
}

impl Totals {
    fn add(&mut self, path: &Path, source: &[u8], per_file: bool) {
        self.files += 1;
        let tokens = Lexer::from_bytes(source).tokenize().len() as u64;
        self.tokens += tokens;
        let parsed = match syntax::parse(source) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.failed += 1;
                if per_file {
                    println!("{}\terror {error:?}", path.display());
                }
                return;
            }
        };
        let (exprs, stmts) = (self.census.expr_count(), self.census.stmt_count());
        self.census.list("statement lists", parsed.stmts.len());
        walk_stmts(&mut self.census, &parsed.stmts);
        if per_file {
            let (exprs, stmts) = (
                self.census.expr_count() - exprs,
                self.census.stmt_count() - stmts,
            );
            let nodes = exprs + stmts;
            let ratio = match nodes {
                0 => "-".to_string(),
                _ => format!("{:.2}", tokens as f64 / nodes as f64),
            };
            println!("{}\t{tokens}\t{exprs}\t{stmts}\t{ratio}", path.display());
        }
    }

    fn print(&self, dir: &str) {
        let (expr_nodes, stmt_nodes) = (self.census.expr_count(), self.census.stmt_count());
        let nodes = expr_nodes + stmt_nodes;
        let record_bytes = expr_nodes * std::mem::size_of::<Expr>() as u64
            + stmt_nodes * std::mem::size_of::<Stmt>() as u64;
        println!("# {dir}");
        println!(
            "files {} (parse errors {}), tokens {}, nodes {}, tokens per node {:.2}",
            self.files,
            self.failed,
            self.tokens,
            nodes,
            self.tokens as f64 / nodes.max(1) as f64,
        );
        println!(
            "record bytes {record_bytes} (expression {expr_nodes} at {} each, statement {stmt_nodes} at {} each)",
            std::mem::size_of::<Expr>(),
            std::mem::size_of::<Stmt>(),
        );
        let census = &self.census;
        println!(
            "arguments {} ({} named), parameters {} ({} with attributes), functions {} ({} with a doc comment)",
            census.args,
            census.named_args,
            census.params,
            census.params_with_attrs,
            census.functions,
            census.functions_with_doc,
        );
        print_kinds("expression kinds", &self.census.exprs, expr_nodes);
        print_kinds("statement kinds", &self.census.stmts, stmt_nodes);
        println!("list lengths (lists per bucket: {})", BUCKETS.join(" "));
        for (name, counts) in &self.census.lists {
            let cells: Vec<String> = counts.iter().map(u64::to_string).collect();
            println!("  {name}: {}", cells.join(" "));
        }
        println!();
    }
}

/// The kinds, most frequent first, with their share of `total`.
fn print_kinds(title: &str, kinds: &BTreeMap<String, u64>, total: u64) {
    println!("{title}:");
    let mut sorted: Vec<(&String, &u64)> = kinds.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (kind, count) in sorted {
        let share = *count as f64 * 100.0 / total.max(1) as f64;
        println!("  {kind} {count} ({share:.2}%)");
    }
}

pub(crate) fn tree_stats(args: &[String]) -> Result<(), String> {
    let per_file = args.iter().any(|arg| arg == "--files");
    let dirs: Vec<String> = args
        .iter()
        .filter(|arg| *arg != "--files")
        .cloned()
        .collect();
    if dirs.is_empty() {
        return Err("tree-stats needs at least one directory".to_string());
    }
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || -> Result<(), String> {
            for dir in &dirs {
                let mut files = Vec::new();
                php_files(Path::new(dir), &mut files);
                files.sort();
                let mut totals = Totals::default();
                for path in &files {
                    let source =
                        std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
                    totals.add(path, &source, per_file);
                }
                totals.print(dir);
            }
            Ok(())
        })
        .map_err(|e| e.to_string())?
        .join()
        .unwrap_or_else(|_| Err("tree-stats panicked".to_string()))
}
