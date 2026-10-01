use rustc_hash::FxHashSet;

use syntax::ast::*;
use syntax::name_resolution::NameContext;
use syntax::visit::{
    Visitor, walk_class_const, walk_class_def, walk_function_decl, walk_param, walk_property_decl,
    walk_property_hooks, walk_stmt, walk_stmts,
};

/// Every class, function and constant one file's parse tree could
/// reference, as the keys `dependency_key` gives - used purely to decide
/// which files might need re-checking when a declaration changes. Missing
/// a real reference would let a stale diagnostic survive an edit
/// elsewhere, while an extra name costs one harmless re-check, so this
/// errs toward over-collecting: an unqualified function or constant name
/// in a namespace yields both the namespaced and the global name, and the
/// names in PHPDoc tags are taken from the tag text without parsing it.
pub fn referenced_names(stmts: &[Stmt]) -> FxHashSet<String> {
    let mut collector = Collector {
        names: FxHashSet::default(),
        ctx: NameContext::default(),
        bodies: true,
    };
    walk_stmts(&mut collector, stmts);
    collector.names
}

/// `referenced_names` without what only function and method bodies
/// mention: the names a file's declarations can take their shape from
/// (signatures, hierarchies, PHPDoc, constant values).
pub fn declaration_referenced_names(stmts: &[Stmt]) -> FxHashSet<String> {
    let mut collector = Collector {
        names: FxHashSet::default(),
        ctx: NameContext::default(),
        bodies: false,
    };
    walk_stmts(&mut collector, stmts);
    collector.names
}

/// The key a declaration's or reference's fully-qualified name (with or
/// without its leading `\`) is tracked under: PHP matches class and
/// function names case-insensitively.
pub fn dependency_key(name: &str) -> String {
    name.trim_start_matches('\\').to_ascii_lowercase()
}

/// The key a `global $name;` statement is tracked under, for the files
/// whose checking reads the project-wide type inferred for `$name`. A `$`
/// keeps it apart from every class, function and constant key.
pub fn global_variable_key(name: &str) -> String {
    format!("${name}")
}

/// Collects every name a file mentions, resolved against the `namespace`
/// and `use` statements in effect, anywhere in the file (nested scopes
/// included).
struct Collector {
    names: FxHashSet<String>,
    ctx: NameContext,
    // Whether function and method bodies are walked.
    bodies: bool,
}

impl Collector {
    /// The names a doc comment's tags mention, each resolved as a class
    /// and also taken as written (a constant-named type's global
    /// fallback). A tag's own name and `$variables` are skipped.
    fn doc_names(&mut self, doc: Option<&str>) {
        let Some(doc) = doc else { return };
        for line in doc.lines() {
            let line = line
                .trim_start()
                .trim_start_matches("/**")
                .trim_start_matches('*')
                .trim_start();
            let Some(tag) = line.strip_prefix('@') else {
                continue;
            };
            let rest = tag.trim_start_matches(|c: char| !c.is_whitespace() && c != '(');
            for token in name_tokens(rest) {
                self.names.insert(dependency_key(&self.ctx.resolve(token)));
                self.names.insert(dependency_key(token));
            }
        }
    }
}

/// The name-shaped tokens of `text`: runs of identifier characters and
/// `\` starting with a letter, `_` or `\`, not preceded by `$`.
fn name_tokens(text: &str) -> impl Iterator<Item = &str> {
    let is_name_char =
        |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '\\' || !c.is_ascii();
    let mut tokens = Vec::new();
    let mut start = None;
    let mut prev = ' ';
    for (i, c) in text.char_indices() {
        match start {
            Some(_) if is_name_char(c) => {}
            Some(s) => {
                tokens.push(&text[s..i]);
                start = None;
            }
            None if is_name_char(c) && !c.is_ascii_digit() && prev != '$' => start = Some(i),
            None => {}
        }
        prev = c;
    }
    if let Some(s) = start {
        tokens.push(&text[s..]);
    }
    tokens
        .into_iter()
        .map(|t| t.trim_end_matches('\\'))
        .filter(|t| !t.trim_start_matches('\\').is_empty())
}

impl<'ast> Visitor<'ast> for Collector {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::Namespace(name) => self.ctx.set_namespace(name),
            StmtKind::Use(imports) => {
                self.ctx.add_uses(imports);
                self.names
                    .extend(imports.iter().map(|import| dependency_key(&import.path)));
            }
            StmtKind::Global(vars) => self.names.extend(
                vars.iter()
                    .filter_map(|v| v.name())
                    .map(|n| global_variable_key(n)),
            ),
            _ => {}
        }
        self.doc_names(stmt.doc_comment.as_deref());
        walk_stmt(self, stmt);
    }

    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
        self.doc_names(function.doc_comment.as_deref());
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

    fn visit_class_def(&mut self, class: &'ast ClassDef) {
        self.doc_names(class.doc_comment.as_deref());
        for trait_use in &class.trait_uses {
            self.doc_names(trait_use.doc_comment.as_deref());
        }
        walk_class_def(self, class);
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

    /// Every class, function and constant reference - in expressions,
    /// declarations' `extends`/`implements`/`use`, catch clauses,
    /// attributes and type declarations.
    fn visit_name(&mut self, name: &'ast QualifiedName) {
        match &name.resolved {
            ResolvedName::Exact(resolved) => {
                self.names.insert(dependency_key(resolved));
            }
            ResolvedName::WithGlobalFallback(resolved) => {
                self.names.insert(dependency_key(resolved));
                self.names.insert(dependency_key(&name.written));
            }
            ResolvedName::Unresolved => {
                if !["self", "parent", "static"]
                    .iter()
                    .any(|keyword| name.is_keyword(keyword))
                {
                    self.names.insert(dependency_key(&name.written));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
