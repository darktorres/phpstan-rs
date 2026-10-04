use rustc_hash::FxHashSet;

use syntax::ast::*;
use syntax::name::Name;
use syntax::name_resolution::NameContext;
pub use syntax::references::RefKey;
use syntax::visit::{
    Visitor, walk_args, walk_class_const, walk_class_def, walk_expr, walk_function_decl,
    walk_param, walk_property_decl, walk_property_hooks, walk_stmt, walk_stmts,
};

/// The key a declaration's or reference's fully-qualified name (with or
/// without its leading `\`) is tracked under in the registry: PHP matches
/// class and function names case-insensitively.
pub fn dependency_key(name: &str) -> String {
    name.trim_start_matches('\\').to_ascii_lowercase()
}

/// The key a `global $name;` statement is tracked under in the registry,
/// for the files whose checking reads the project-wide type inferred for
/// `$name`. A `$` keeps it apart from every class, function and constant
/// key.
pub fn global_variable_key(name: &str) -> String {
    format!("${name}")
}

/// Every class, function and constant one file's parse tree could
/// reference, as the keys of what each is - used purely to decide which
/// files might need re-checking when a declaration changes. Missing a
/// real reference would let a stale diagnostic survive an edit elsewhere,
/// while an extra name costs one harmless re-check, so this errs toward
/// over-collecting: an unqualified function or constant name in a
/// namespace yields both the namespaced and the global name, and the
/// names in PHPDoc tags are taken from the tag text without parsing it.
pub fn referenced_names(stmts: &[Stmt]) -> FxHashSet<RefKey> {
    let mut collector = Collector::new(true, false);
    walk_stmts(&mut collector, stmts);
    collector.names
}

/// What a file refers to, read from what its parse recorded: every name,
/// and those outside the bodies of functions and methods. The names in doc
/// comments are read from the comments the parser recorded with the names in
/// effect where each is, with the templates its enclosing functions and
/// classes declare left out.
pub fn recorded_references(
    references: syntax::references::FileReferences,
) -> (FxHashSet<RefKey>, FxHashSet<RefKey>) {
    let found: Vec<(bool, Vec<RefKey>)> = doc_site_references(&references)
        .into_iter()
        .map(|(site, keys)| (site.outside, keys))
        .collect();
    let syntax::references::FileReferences {
        mut names,
        mut outside,
        doc_sites: _,
        doc_frames: _,
    } = references;
    for (is_outside, keys) in found {
        for key in keys {
            if is_outside {
                outside.insert(key.clone());
            }
            names.insert(key);
        }
    }
    (names, outside)
}

/// Each doc site of `references`, in the order of the file, with what the
/// names in its tags refer to: each name resolved where the site is and as
/// written, the templates the site and its enclosing functions and classes
/// declare left out.
pub(crate) fn doc_site_references(
    references: &syntax::references::FileReferences,
) -> Vec<(&syntax::references::DocSite, Vec<RefKey>)> {
    let mut sites: Vec<&syntax::references::DocSite> = references.doc_sites.iter().collect();
    sites.sort_by_key(|site| site.pos);
    let mut frames: Vec<&syntax::references::DocFrame> = references.doc_frames.iter().collect();
    frames.sort_by_key(|frame| frame.start);
    let mut frames = frames.into_iter().peekable();
    // The frames the site being read is within, outermost first.
    let mut enclosing: Vec<&syntax::references::DocFrame> = Vec::new();
    let mut out = Vec::with_capacity(sites.len());
    for site in sites {
        while let Some(frame) = frames.next_if(|frame| frame.start < site.pos) {
            while enclosing.last().is_some_and(|last| last.end <= frame.start) {
                enclosing.pop();
            }
            enclosing.push(frame);
        }
        while enclosing.last().is_some_and(|last| last.end <= site.pos) {
            enclosing.pop();
        }
        let parsed = doc_names_of(&site.doc);
        let mut keys = Vec::new();
        for token in &parsed.tokens {
            if parsed.declared.contains(token)
                || enclosing
                    .iter()
                    .any(|frame| doc_names_of(&frame.doc).declared.contains(token))
            {
                continue;
            }
            keys.push(RefKey::class_text(&site.scope.resolve(token)));
            keys.push(RefKey::class_text(token));
        }
        out.push((site, keys));
    }
    out
}

/// `referenced_names` and `declaration_referenced_names`, in one walk.
pub fn both_referenced_names(stmts: &[Stmt]) -> (FxHashSet<RefKey>, FxHashSet<RefKey>) {
    let mut collector = Collector::new(true, true);
    walk_stmts(&mut collector, stmts);
    (collector.names, collector.outside)
}

/// `referenced_names` without what only function and method bodies
/// mention: the names a file's declarations can take their shape from
/// (signatures, hierarchies, PHPDoc, constant values).
pub fn declaration_referenced_names(stmts: &[Stmt]) -> FxHashSet<RefKey> {
    let mut collector = Collector::new(false, false);
    walk_stmts(&mut collector, stmts);
    collector.names
}

/// Collects every name a file mentions, resolved against the `namespace`
/// and `use` statements in effect, anywhere in the file (nested scopes
/// included).
struct Collector {
    names: FxHashSet<RefKey>,
    // With `track_outside`, the names found anywhere but in a function or
    // method body: what `declaration_referenced_names` finds, from the same
    // walk.
    outside: FxHashSet<RefKey>,
    track_outside: bool,
    // How many function and method bodies the walk is inside.
    body_depth: usize,
    ctx: NameContext,
    // Whether function and method bodies are walked.
    bodies: bool,
    // The doc comments of the classes and functions being walked,
    // innermost last: their template parameters are names that are not
    // classes.
    templates: Vec<Option<std::sync::Arc<DocNames>>>,
}

/// What a doc comment's text alone gives the collector, kept by the text:
/// the same comments are read by each walk, at the statement and at the
/// declaration, and the names in the types of their tags do not depend on
/// where they are read.
struct DocNames {
    /// The names its tags' types can make a class's, as written.
    tokens: Vec<String>,
    /// The template parameters its `@template` tags declare.
    declared: FxHashSet<String>,
}

/// `doc`'s `DocNames`: what its docblock holds.
fn doc_names_of(doc: &str) -> std::sync::Arc<DocNames> {
    static MEMO: std::sync::LazyLock<syntax::memo::Memo<DocNames>> =
        std::sync::LazyLock::new(|| syntax::memo::Memo::with_limits(1024, 256 * 1024));
    MEMO.get(doc, |doc| {
        let block = phpdoc::docblock::parse_shared(doc);
        let mut tokens = Vec::new();
        block.for_each_class_name(&mut |name| tokens.push(name.to_string()));
        DocNames {
            tokens,
            declared: block.templates.iter().cloned().collect(),
        }
    })
}

impl Collector {
    fn new(bodies: bool, track_outside: bool) -> Collector {
        Collector {
            names: FxHashSet::default(),
            outside: FxHashSet::default(),
            track_outside,
            body_depth: 0,
            ctx: NameContext::default(),
            bodies,
            templates: Vec::new(),
        }
    }

    /// Records `key` as referenced.
    fn add(&mut self, key: RefKey) {
        if self.track_outside && self.body_depth == 0 {
            self.outside.insert(key.clone());
        }
        self.names.insert(key);
    }

    /// The names a doc comment's tags mention, each resolved as a class
    /// and also taken as written (a constant-named type's global
    /// fallback). Only the types of a tag are read, not its description,
    /// and a template parameter in scope isn't a class.
    fn doc_names(&mut self, doc: Option<&str>) {
        let Some(doc) = doc else { return };
        let parsed = doc_names_of(doc);
        for token in &parsed.tokens {
            if parsed.declared.contains(token)
                || self
                    .templates
                    .iter()
                    .flatten()
                    .any(|t| t.declared.contains(token))
            {
                continue;
            }
            let resolved = RefKey::class_text(&self.ctx.resolve(token));
            self.add(resolved);
            self.add(RefKey::class_text(token));
        }
    }

    /// `doc`'s names, for the template parameters it declares.
    fn templates_of(doc: Option<&str>) -> Option<std::sync::Arc<DocNames>> {
        doc.map(doc_names_of)
    }
}

impl<'ast> Visitor<'ast> for Collector {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::Namespace(name) => self.ctx.set_namespace(name),
            StmtKind::Use(imports) => {
                self.ctx.add_uses(imports);
                for import in imports {
                    let path = &import.path;
                    self.add(match import.kind {
                        UseKind::Class => RefKey::class(path),
                        UseKind::Function => RefKey::function(path),
                        UseKind::Const => RefKey::constant(path),
                    });
                }
            }
            StmtKind::Global(vars) => {
                for name in vars.iter().filter_map(|v| v.name()) {
                    self.add(RefKey::global_variable(name));
                }
            }
            _ => {}
        }
        // A function's or a class's own doc comment is read again when the
        // walk reaches the declaration, by `visit_function_decl` or
        // `visit_class_def`, to the same names: only a comment the
        // declaration does not have is read here.
        let declaration_doc = match &stmt.kind {
            StmtKind::FunctionDecl(function) => Some(function.doc_comment.as_deref()),
            StmtKind::ClassDecl(class) => Some(class.def.doc_comment.as_deref()),
            _ => None,
        };
        if declaration_doc != Some(stmt.doc_comment.as_deref()) {
            self.doc_names(stmt.doc_comment.as_deref());
        }
        walk_stmt(self, stmt);
    }

    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
        self.doc_names(function.doc_comment.as_deref());
        self.templates
            .push(Collector::templates_of(function.doc_comment.as_deref()));
        if self.bodies && self.track_outside {
            // The parts that are not the body count as outside it.
            for attr in &function.attrs {
                self.visit_attribute(attr);
            }
            for param in &function.params {
                self.visit_param(param);
            }
            if let Some(hint) = &function.return_type {
                self.visit_type_hint(hint);
            }
            self.body_depth += 1;
            walk_stmts(self, function.body_stmts());
            self.body_depth -= 1;
        } else if self.bodies {
            walk_function_decl(self, function);
        } else {
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
        self.templates.pop();
    }

    fn visit_class_def(&mut self, class: &'ast ClassDef) {
        self.doc_names(class.doc_comment.as_deref());
        self.templates
            .push(Collector::templates_of(class.doc_comment.as_deref()));
        for trait_use in &class.trait_uses {
            self.doc_names(trait_use.doc_comment.as_deref());
        }
        walk_class_def(self, class);
        self.templates.pop();
    }

    fn visit_property_decl(&mut self, property: &'ast PropertyDecl) {
        self.doc_names(property.doc_comment.as_deref());
        walk_property_decl(self, property);
    }

    fn visit_class_const(&mut self, constant: &'ast ClassConst) {
        self.doc_names(constant.doc_comment.as_deref());
        walk_class_const(self, constant);
    }

    fn visit_param(&mut self, param: &'ast Param) {
        self.doc_names(param.doc_comment.as_deref());
        walk_param(self, param);
    }

    fn visit_property_hooks(&mut self, hooks: &'ast PropertyHooks) {
        for hook in hooks.iter() {
            self.doc_names(hook.doc.as_deref());
        }
        walk_property_hooks(self, hooks);
    }

    /// The names in an expression: a constant, the callee of a call, and
    /// every other name - the class of a `new`, a static access, an
    /// `instanceof` - as what each is.
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            ExprKind::Name(name) => self.add_name(name, NameKind::Constant),
            ExprKind::Call(callee, args) if matches!(callee.kind, ExprKind::Name(_)) => {
                if let ExprKind::Name(name) = &callee.kind {
                    self.add_name(name, NameKind::Function);
                }
                walk_args(self, args);
            }
            ExprKind::New(class, args) => {
                match &class.kind {
                    ExprKind::Name(name) => self.add_name(name, NameKind::Class),
                    _ => self.visit_expr(class),
                }
                walk_args(self, args);
            }
            _ => walk_expr(self, expr),
        }
    }

    /// Every class reference in declarations' `extends`/`implements`/`use`,
    /// catch clauses, attributes and type declarations, and in the places
    /// of an expression the arms above leave to it.
    fn visit_name(&mut self, name: &'ast QualifiedName) {
        self.add_name(name, NameKind::Class);
    }
}

/// What a name is read as.
#[derive(Clone, Copy)]
enum NameKind {
    Class,
    Function,
    Constant,
}

impl Collector {
    /// Records a resolved name as a reference to what `kind` says it is:
    /// the one name it resolved to, and for an unqualified function or
    /// constant in a namespace the global one it falls back to; a name
    /// that is not resolved is taken as written, unless it is a keyword.
    fn add_name(&mut self, name: &QualifiedName, kind: NameKind) {
        let key = |name: &Name| match kind {
            NameKind::Class => RefKey::class(name),
            NameKind::Function => RefKey::function(name),
            NameKind::Constant => RefKey::constant(name),
        };
        match &name.resolved {
            ResolvedName::Exact(resolved) => self.add(key(resolved)),
            ResolvedName::WithGlobalFallback(resolved) => {
                self.add(key(resolved));
                self.add(key(&name.written));
            }
            ResolvedName::Unresolved => {
                if !["self", "parent", "static"]
                    .iter()
                    .any(|keyword| name.is_keyword(keyword))
                {
                    self.add(key(&name.written));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
