use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ast::Stmt;
use crate::checker::{Checker, Diagnostic};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::php_stubs;
use crate::registry::Registry;
use crate::stubs;

/// One file's parsed state, kept around so `Project::update_file` can undo
/// exactly what that file previously contributed to the shared registry
/// before re-indexing its new parse - the difference between an
/// incremental update and a full reload.
struct FileState {
    ast: Vec<Stmt>,
    functions: Vec<String>,
    classes: Vec<String>,
}

/// A multi-file PHP project: every loaded file's own parse tree, plus one
/// registry merged across all of them (so a class in one file can extend a
/// class declared in another). `load_file` is for the initial, full load of
/// a codebase; `update_file` is the incremental path - re-parsing and
/// re-checking only the one file that changed, touching every other file's
/// parse tree and registry entries not at all.
///
/// What incrementality does NOT yet cover: a changed function's signature
/// can invalidate diagnostics in *other* files that call it, and this
/// doesn't re-check those callers - only the changed file itself is
/// re-checked. Real cross-file invalidation (a dependency graph from
/// "function X is called in files A, B, C") is future work, not something
/// this first version does.
pub struct Project {
    files: HashMap<PathBuf, FileState>,
    registry: Registry,
}

impl Project {
    pub fn new() -> Self {
        let mut registry = Registry::default();
        registry.merge_stubs(php_stubs::generated_stubs());
        registry.merge_stubs(stubs::builtin_stubs());
        Project {
            files: HashMap::new(),
            registry,
        }
    }

    /// Parses one file and indexes its declarations into the shared
    /// registry. Call `finish_loading` once after the initial batch of
    /// `load_file` calls - trait flattening is a global pass over every
    /// class, so it's wasteful to re-run it after each individual file
    /// during a bulk load the way `update_file` has to for a single
    /// incremental change.
    pub fn load_file(&mut self, path: PathBuf, source: &str) {
        let ast = parse(source);
        let (functions, classes) = declared_names(&ast);
        self.registry.index(&ast);
        self.files.insert(
            path,
            FileState {
                ast,
                functions,
                classes,
            },
        );
    }

    /// Finalizes an initial batch of `load_file` calls (trait flattening).
    pub fn finish_loading(&mut self) {
        self.registry.flatten_traits();
    }

    /// Re-parses one already-loaded file and patches just its own
    /// registry contributions - the incremental path. Every other file's
    /// AST and registry entries are left completely untouched, which is
    /// what makes this cheaper than `load_file` + `finish_loading` over
    /// the whole project: the cost is proportional to this one file's
    /// size, not the project's.
    pub fn update_file(&mut self, path: &Path, new_source: &str) {
        if let Some(old) = self.files.get(path) {
            self.registry
                .remove_declarations(&old.functions, &old.classes);
        }
        let ast = parse(new_source);
        let (functions, classes) = declared_names(&ast);
        self.registry.index(&ast);
        // A changed class's `use TraitName;` list (added, removed, or a
        // trait's own members changed in this same file) means the whole
        // registry's trait flattening can't be assumed still correct -
        // re-run it. Still far cheaper than a full reload: a HashMap scan
        // over already-parsed ClassInfo, not a re-parse of anything.
        self.registry.flatten_traits();
        self.files.insert(
            path.to_path_buf(),
            FileState {
                ast,
                functions,
                classes,
            },
        );
    }

    /// Type-checks one already-loaded file against the project's current
    /// registry, returning its diagnostics.
    pub fn check_file(&self, path: &Path) -> Vec<Diagnostic> {
        let Some(file) = self.files.get(path) else {
            return Vec::new();
        };
        let mut checker = Checker::new(&self.registry);
        checker.check_program(&file.ast);
        checker.into_diagnostics()
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

fn parse(source: &str) -> Vec<Stmt> {
    let tokens = Lexer::new(source).tokenize();
    Parser::new(tokens).parse_program()
}

/// The top-level function/class/enum names one file's parse tree declares -
/// exactly what a registry entry needs to be identified by to be removed
/// again later. Enums share the registry's `classes` map (see
/// `Registry::index`), so they're collected into the same list here.
fn declared_names(stmts: &[Stmt]) -> (Vec<String>, Vec<String>) {
    let mut functions = Vec::new();
    let mut classes = Vec::new();
    for stmt in stmts {
        match stmt {
            Stmt::FunctionDecl(f) => functions.push(f.name.clone()),
            Stmt::ClassDecl(c) => classes.push(c.name.clone()),
            Stmt::EnumDecl(e) => classes.push(e.name.clone()),
            _ => {}
        }
    }
    (functions, classes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn updating_one_file_does_not_disturb_another_files_registry_entries() {
        let mut project = Project::new();
        project.load_file(
            PathBuf::from("a.php"),
            "<?php function helperA(): int { return 1; }",
        );
        project.load_file(
            PathBuf::from("b.php"),
            "<?php function helperB(): int { return helperA(); }",
        );
        project.finish_loading();

        assert!(project.registry().functions.contains_key("helperA"));
        assert!(project.registry().functions.contains_key("helperB"));

        // Change b.php's return type - a.php's own registry entry and
        // parse tree must be untouched by this.
        project.update_file(
            &PathBuf::from("b.php"),
            "<?php function helperB(): string { return 'x'; }",
        );

        assert!(project.registry().functions.contains_key("helperA"));
        let helper_b = &project.registry().functions["helperB"];
        assert_eq!(helper_b.return_type.to_string(), "string");
    }

    #[test]
    fn removing_a_declaration_on_update_drops_it_from_the_registry() {
        let mut project = Project::new();
        project.load_file(
            PathBuf::from("a.php"),
            "<?php function helperA(): int { return 1; } class Foo {}",
        );
        project.finish_loading();
        assert!(project.registry().classes.contains_key("Foo"));

        // The new version of the file no longer declares Foo at all.
        project.update_file(
            &PathBuf::from("a.php"),
            "<?php function helperA(): int { return 1; }",
        );
        assert!(!project.registry().classes.contains_key("Foo"));
        assert!(project.registry().functions.contains_key("helperA"));
    }

    #[test]
    fn incremental_update_still_resolves_cross_file_inheritance() {
        let mut project = Project::new();
        project.load_file(
            PathBuf::from("base.php"),
            "<?php class Base { public int $x = 1; }",
        );
        project.load_file(
            PathBuf::from("child.php"),
            "<?php class Child extends Base {}",
        );
        project.finish_loading();
        assert!(project.registry().is_subclass_of("Child", "Base"));

        // Touching an unrelated file shouldn't break that cross-file
        // relationship.
        project.update_file(
            &PathBuf::from("child.php"),
            "<?php class Child extends Base {}",
        );
        assert!(project.registry().is_subclass_of("Child", "Base"));
    }
}
