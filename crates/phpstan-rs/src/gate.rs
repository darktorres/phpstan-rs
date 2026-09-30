//! Reading what the gate records back: the templates in scope where a
//! type was dumped or asserted, so a displayed template name reads as
//! that template rather than a class.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustc_hash::FxHashMap;
use syntax::ast::{ClassDecl, FunctionDecl, Stmt, StmtKind};
use syntax::token::Position;
use syntax::visit::{self, Visitor};
use types::Type;

use crate::Registry;

/// The templates in scope at a point, by name, as the checker binds them:
/// the enclosing class's, then the enclosing function's or method's own,
/// which shadow a class template of the same name.
#[derive(Debug, Clone, Default)]
pub struct TemplateScope {
    templates: FxHashMap<String, Type>,
}

impl TemplateScope {
    /// The template `name` names here, if any.
    pub fn resolve(&self, name: &str) -> Option<Type> {
        self.templates.get(name).cloned()
    }
}

/// The templates in scope at `line`:`col` of a file whose statements are
/// `stmts`, with class templates from `registry`.
pub fn template_scope_at(
    registry: &Registry,
    stmts: &[Stmt],
    line: u32,
    col: u32,
) -> TemplateScope {
    let mut finder = Enclosing {
        at: Position { line, col },
        class: None,
        function: None,
    };
    visit::walk_stmts(&mut finder, stmts);
    let mut templates = FxHashMap::default();
    if let Some(class) = finder.class
        && let Some(info) = registry.classes.get(class.fq_name())
    {
        for (name, arg) in info
            .templates()
            .iter()
            .zip(info.own_template_args(class.fq_name()))
        {
            templates.insert(name.to_string(), arg);
        }
    }
    if let Some(function) = finder.function
        && let Some(doc) = function.doc_comment.as_deref()
    {
        let ctx = syntax::name_resolution::context_at(stmts, function.span.start);
        let own = crate::registry::build_template_params(
            &phpdoc::docblock::parse(doc),
            &function.name,
            &ctx,
        );
        for (name, param) in own {
            templates.insert(name, Type::template_param(param));
        }
    }
    TemplateScope { templates }
}

/// The innermost class and function or method declarations containing a
/// position.
struct Enclosing<'ast> {
    at: Position,
    class: Option<&'ast ClassDecl>,
    function: Option<&'ast FunctionDecl>,
}

impl Enclosing<'_> {
    fn contains(&self, start: Position, end: Position) -> bool {
        (start.line, start.col) <= (self.at.line, self.at.col)
            && (self.at.line, self.at.col) <= (end.line, end.col)
    }
}

impl<'ast> Visitor<'ast> for Enclosing<'ast> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if !self.contains(stmt.span.start, stmt.span.end) {
            return;
        }
        if let StmtKind::ClassDecl(class) = &stmt.kind {
            self.class = Some(class);
            self.function = None;
        }
        visit::walk_stmt(self, stmt);
    }

    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
        if self.contains(function.span.start, function.span.end) {
            self.function = Some(function);
            visit::walk_function_decl(self, function);
        }
    }
}

/// `template_scope_at` over files read and parsed once each.
pub struct TemplateScopes<'r> {
    registry: &'r Registry,
    interner: Arc<syntax::name::Interner>,
    files: FxHashMap<PathBuf, Option<Vec<Stmt>>>,
}

impl<'r> TemplateScopes<'r> {
    pub fn new(registry: &'r Registry) -> TemplateScopes<'r> {
        TemplateScopes {
            registry,
            interner: Arc::new(syntax::name::Interner::new()),
            files: FxHashMap::default(),
        }
    }

    /// The templates in scope at `line`:`col` of `path`; none when the
    /// file can't be read or parsed.
    pub fn at(&mut self, path: &Path, line: u32, col: u32) -> TemplateScope {
        let interner = &self.interner;
        let stmts = self.files.entry(path.to_path_buf()).or_insert_with(|| {
            let source = std::fs::read(path).ok()?;
            syntax::parse(&source, interner)
                .ok()
                .map(|parsed| parsed.stmts)
        });
        match stmts {
            Some(stmts) => template_scope_at(self.registry, stmts, line, col),
            None => TemplateScope::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "<?php
namespace App;
/** @template T of \\Countable */
class Box {
    /**
     * @template U of int
     * @template T
     */
    public function map(): void {
        $a = 1;
    }
    public function get(): void {
        $b = 1;
    }
}
function free(): void {
    $c = 1;
}
";

    fn scope_at(line: u32) -> TemplateScope {
        let interner = Arc::new(syntax::name::Interner::new());
        let parsed = syntax::parse(SOURCE.as_bytes(), &interner).unwrap();
        let registry = Registry::build(&parsed.stmts);
        template_scope_at(&registry, &parsed.stmts, line, 9)
    }

    fn bound(scope: &TemplateScope, name: &str) -> Option<String> {
        scope
            .resolve(name)
            .and_then(|t| t.template_bound().map(Type::to_string))
    }

    #[test]
    fn a_method_sees_its_class_templates_and_its_own() {
        let scope = scope_at(13);
        assert_eq!(bound(&scope, "T").as_deref(), Some("Countable"));
        assert_eq!(bound(&scope, "U"), None);
    }

    #[test]
    fn a_method_template_shadows_a_class_template_of_the_same_name() {
        let scope = scope_at(10);
        assert_eq!(bound(&scope, "U").as_deref(), Some("int"));
        assert_eq!(bound(&scope, "T").as_deref(), Some("mixed"));
    }

    #[test]
    fn outside_the_class_its_templates_are_not_in_scope() {
        let outside = scope_at(17);
        assert_eq!(outside.resolve("T"), None);
        let inside = scope_at(13);
        let read = |scope: &TemplateScope| {
            types::try_parse_displayed_with("list<T>", &|n| scope.resolve(n)).unwrap()
        };
        assert!(read(&inside).references_template());
        assert!(!read(&outside).references_template());
    }
}
