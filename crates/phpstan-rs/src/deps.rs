use rustc_hash::FxHashSet;

use syntax::ast::*;
use syntax::name::Name;
use syntax::name_resolution::NameContext;
pub use syntax::references::RefKey;
use syntax::visit::arena::{self, Visitor};

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
pub fn referenced_names(arenas: &NodeArenas, stmts: &[StmtId]) -> FxHashSet<RefKey> {
    let mut collector = Collector::new(false);
    arena::walk_stmts(&mut collector, arenas, stmts);
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
    let found: Vec<(bool, DocSiteKeys)> = doc_site_references(&references)
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
        let DocSiteKeys { types, see } = keys;
        for key in types {
            if is_outside {
                outside.insert(key.clone());
            }
            names.insert(key);
        }
        names.extend(see);
    }
    (names, outside)
}

/// What the names in one doc site's tags refer to.
#[derive(Debug, Default)]
pub(crate) struct DocSiteKeys {
    /// The names in its tags' types, each resolved where the site is and
    /// as written: what the declaration it documents can take its shape
    /// from.
    pub(crate) types: Vec<RefKey>,
    /// The classes its `@see` tags name, resolved where the site is, which
    /// only the check of the tag reads.
    pub(crate) see: Vec<RefKey>,
}

/// Each doc site of `references`, in the order of the file, with what the
/// names in its tags refer to, the templates the site and its enclosing
/// functions and classes declare left out of the names in types.
pub(crate) fn doc_site_references(
    references: &syntax::references::FileReferences,
) -> Vec<(&syntax::references::DocSite, DocSiteKeys)> {
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
        let mut keys = DocSiteKeys::default();
        for token in &parsed.tokens {
            if parsed.declared.contains(token)
                || enclosing
                    .iter()
                    .any(|frame| doc_names_of(&frame.doc).declared.contains(token))
            {
                continue;
            }
            keys.types
                .push(RefKey::class_text(&site.scope.resolve(token)));
            keys.types.push(RefKey::class_text(token));
        }
        for class in &parsed.see {
            keys.see
                .push(RefKey::class_text(&site.scope.resolve(class)));
        }
        out.push((site, keys));
    }
    out
}

/// `referenced_names` and `declaration_referenced_names`, in one walk.
pub fn both_referenced_names(
    arenas: &NodeArenas,
    stmts: &[StmtId],
) -> (FxHashSet<RefKey>, FxHashSet<RefKey>) {
    let mut collector = Collector::new(true);
    arena::walk_stmts(&mut collector, arenas, stmts);
    (collector.names, collector.outside)
}

/// Collects every name a file mentions, resolved against the `namespace`
/// and `use` statements in effect, anywhere in the file (nested scopes
/// included).
struct Collector {
    names: FxHashSet<RefKey>,
    // With `track_outside`, the names found anywhere but in a function or
    // method body: what a file's declarations can take their shape from
    // (signatures, hierarchies, PHPDoc, constant values).
    outside: FxHashSet<RefKey>,
    track_outside: bool,
    // How many function and method bodies the walk is inside.
    body_depth: usize,
    ctx: NameContext,
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
    /// The classes its `@see` tags name, as written: read as classes
    /// whatever templates are in scope, as the check of the tag reads them.
    see: Vec<String>,
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
            see: block
                .see
                .iter()
                .filter_map(|target| phpdoc::docblock::see_reference(target))
                .map(|reference| reference.class.to_string())
                .collect(),
        }
    })
}

impl Collector {
    fn new(track_outside: bool) -> Collector {
        Collector {
            names: FxHashSet::default(),
            outside: FxHashSet::default(),
            track_outside,
            body_depth: 0,
            ctx: NameContext::default(),
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

    /// The names a doc comment's tags mention: those in the types of its
    /// tags, each resolved as a class and also taken as written (a
    /// constant-named type's global fallback), a template parameter in
    /// scope not being a class; and the classes its `@see` tags name,
    /// which no declaration takes its shape from.
    fn doc_names(&mut self, arenas: &NodeArenas, doc: Option<&DocCommentRanges>) {
        let Some(doc) = doc.and_then(|ranges| arenas.doc_comment_text(ranges)) else {
            return;
        };
        self.doc_names_text(&doc);
    }

    fn doc_names_text(&mut self, doc: &str) {
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
        for class in &parsed.see {
            self.names
                .insert(RefKey::class_text(&self.ctx.resolve(class)));
        }
    }

    /// `doc`'s names, for the template parameters it declares.
    fn templates_of(
        arenas: &NodeArenas,
        doc: Option<&DocCommentRanges>,
    ) -> Option<std::sync::Arc<DocNames>> {
        doc.and_then(|ranges| arenas.doc_comment_text(ranges))
            .as_deref()
            .map(doc_names_of)
    }
}

impl Visitor for Collector {
    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else { return };
        match &stmt.kind {
            StmtKind::Namespace(name) => self.ctx.set_namespace(name),
            StmtKind::Use(imports) => {
                let imports = arenas.use_imports(imports);
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
                for name in arenas
                    .global_vars(vars.clone())
                    .iter()
                    .filter_map(GlobalVar::name)
                {
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
            StmtKind::FunctionDecl(function) => arenas
                .function(*function)
                .and_then(|function| function.doc_comment.as_ref()),
            StmtKind::ClassDecl(class) => arenas
                .class(arenas.class_decl(class.clone()).def)
                .and_then(|def| def.doc_comment.as_ref()),
            _ => None,
        };
        if let Some(site) = arenas
            .stmt_span(id)
            .and_then(|span| arenas.doc_site(span.start))
        {
            let declaration_doc =
                declaration_doc.and_then(|ranges| arenas.doc_comment_text(ranges));
            if declaration_doc.as_deref() != Some(site.text.as_ref()) {
                self.doc_names_text(&site.text);
            }
        }
        arena::walk_stmt(self, arenas, id);
    }

    fn visit_function_decl(&mut self, arenas: &NodeArenas, function: &FunctionDecl) {
        self.doc_names(arenas, function.doc_comment.as_ref());
        self.templates.push(Collector::templates_of(
            arenas,
            function.doc_comment.as_ref(),
        ));
        if self.track_outside {
            // The parts that are not the body count as outside it.
            for attr in arenas.attributes(function.attrs.clone()) {
                self.visit_attribute(arenas, attr);
            }
            for param in arenas.params(function.params.clone()) {
                self.visit_param(arenas, param);
            }
            if let Some(hint) = &function.return_type {
                self.visit_type_hint(arenas, hint);
            }
            self.body_depth += 1;
            arena::walk_stmts(self, arenas, function.body_stmts(arenas));
            self.body_depth -= 1;
        } else {
            arena::walk_function_decl(self, arenas, function);
        }
        self.templates.pop();
    }

    fn visit_class_def(&mut self, arenas: &NodeArenas, class: &ClassDef) {
        self.doc_names(arenas, class.doc_comment.as_ref());
        self.templates
            .push(Collector::templates_of(arenas, class.doc_comment.as_ref()));
        for trait_use in class.trait_uses(arenas) {
            self.doc_names(arenas, trait_use.doc_comment.as_ref());
        }
        arena::walk_class_def(self, arenas, class);
        self.templates.pop();
    }

    fn visit_property_decl(&mut self, arenas: &NodeArenas, property: &PropertyDecl) {
        self.doc_names(arenas, property.doc_comment.as_ref());
        arena::walk_property_decl(self, arenas, property);
    }

    fn visit_class_const(&mut self, arenas: &NodeArenas, constant: &ClassConst) {
        self.doc_names(arenas, constant.doc_comment.as_ref());
        arena::walk_class_const(self, arenas, constant);
    }

    fn visit_param(&mut self, arenas: &NodeArenas, param: &Param) {
        if let Some(site) = arenas.doc_site(param.span.start) {
            self.doc_names_text(&site.text);
        }
        arena::walk_param(self, arenas, param);
    }

    fn visit_property_hooks(&mut self, arenas: &NodeArenas, hooks: &PropertyHooks) {
        for hook in hooks.iter() {
            self.doc_names(arenas, hook.doc.as_ref());
        }
        arena::walk_property_hooks(self, arenas, hooks);
    }

    /// The names in an expression: a constant, the callee of a call, and
    /// every other name - the class of a `new`, a static access, an
    /// `instanceof` - as what each is.
    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(expr) = arenas.expr(id) else { return };
        match &expr.kind {
            ExprKind::Name(name) => {
                if let Some(name) = arenas.qualified_name(name) {
                    self.add_name(name, NameKind::Constant);
                }
            }
            ExprKind::Call(callee, args)
                if matches!(
                    arenas.expr(*callee).map(|e| &e.kind),
                    Some(ExprKind::Name(_))
                ) =>
            {
                if let Some(ExprNode {
                    kind: ExprKind::Name(name),
                    ..
                }) = arenas.expr(*callee)
                    && let Some(name) = arenas.qualified_name(name) {
                        self.add_name(name, NameKind::Function);
                    }
                for arg in arenas.args(args.clone()) {
                    arena::walk_arg(self, arenas, arg);
                }
            }
            ExprKind::New(class, args) => {
                match arenas.expr(*class).map(|e| &e.kind) {
                    Some(ExprKind::Name(name)) => {
                        if let Some(name) = arenas.qualified_name(name) {
                            self.add_name(name, NameKind::Class);
                        }
                    }
                    _ => self.visit_expr(arenas, *class),
                }
                for arg in arenas.args(args.clone()) {
                    arena::walk_arg(self, arenas, arg);
                }
            }
            _ => arena::walk_expr(self, arenas, id),
        }
    }

    /// Every class reference in declarations' `extends`/`implements`/`use`,
    /// catch clauses, attributes and type declarations, and in the places
    /// of an expression the arms above leave to it.
    fn visit_name(&mut self, name: &QualifiedName) {
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
