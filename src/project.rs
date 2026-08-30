use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::Stmt;
use crate::checker::{Checker, Diagnostic};
use crate::deps;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::php_stubs;
use crate::registry::Registry;
use crate::stubs;

/// One file's parsed state, kept around so `Project::update_file` can undo
/// exactly what that file previously contributed - both to the shared
/// registry and to `dependents` below - before re-indexing its new parse.
/// That undo/redo pair is the difference between an incremental update and
/// a full reload.
struct FileState {
    ast: Vec<Stmt>,
    functions: Vec<String>,
    classes: Vec<String>,
    references: HashSet<String>,
}

/// A multi-file PHP project: every loaded file's own parse tree, one
/// registry merged across all of them (so a class in one file can extend a
/// class declared in another), and a reverse index - `dependents[name]` is
/// every file that *references* `name` - used to find which other files
/// need re-checking when a file's declarations change.
///
/// `load_file` is for the initial, full load of a codebase; `update_file`
/// is the incremental path: re-parsing and re-checking only the file that
/// changed, plus (new) every file that references a symbol it declares -
/// every other file's parse tree and registry entries are untouched.
///
/// What this does NOT do: chase invalidation transitively (if A depends on
/// B and B depends on C, changing C re-checks B but not A, even though A's
/// diagnostics could in principle depend on C through B) or narrow by
/// *which* aspect of a symbol changed (any file referencing a changed
/// symbol at all gets re-checked, even if the specific thing it uses
/// didn't change) - both are safe over-approximations in the direction
/// that costs an occasional extra re-check rather than a missed one, and
/// straightforward to tighten later if the extra re-checking ever matters
/// in practice.
pub struct Project {
    files: HashMap<PathBuf, FileState>,
    dependents: HashMap<String, HashSet<PathBuf>>,
    registry: Registry,
}

impl Project {
    pub fn new() -> Self {
        let mut registry = Registry::default();
        registry.merge_stubs(php_stubs::generated_stubs());
        registry.merge_stubs(stubs::builtin_stubs());
        Project {
            files: HashMap::new(),
            dependents: HashMap::new(),
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
        let references = deps::referenced_names(&ast);
        self.registry.index(&ast);
        register_references(&mut self.dependents, &path, &references);
        self.files.insert(
            path,
            FileState {
                ast,
                functions,
                classes,
                references,
            },
        );
    }

    /// Finalizes an initial batch of `load_file` calls (trait flattening).
    pub fn finish_loading(&mut self) {
        self.registry.flatten_traits();
    }

    /// Re-parses one already-loaded file, patches just its own registry
    /// contributions and `dependents` entries, and re-checks it plus every
    /// other file that references a symbol it declared (before or after
    /// the change - a symbol the new version dropped still needs its old
    /// dependents re-checked, since they'll now see it as undefined).
    /// Returns every re-checked file's fresh diagnostics, keyed by path -
    /// the changed file is always included, even with zero diagnostics.
    pub fn update_file(
        &mut self,
        path: &Path,
        new_source: &str,
    ) -> HashMap<PathBuf, Vec<Diagnostic>> {
        let mut changed_symbols: HashSet<String> = HashSet::new();

        if let Some(old) = self.files.get(path) {
            self.registry
                .remove_declarations(&old.functions, &old.classes);
            changed_symbols.extend(old.functions.iter().cloned());
            changed_symbols.extend(old.classes.iter().cloned());
            unregister_references(&mut self.dependents, path, &old.references);
        }

        let ast = parse(new_source);
        let (functions, classes) = declared_names(&ast);
        let references = deps::referenced_names(&ast);
        changed_symbols.extend(functions.iter().cloned());
        changed_symbols.extend(classes.iter().cloned());

        self.registry.index(&ast);
        // A changed class's `use TraitName;` list (added, removed, or a
        // trait's own members changed in this same file) means the whole
        // registry's trait flattening can't be assumed still correct -
        // re-run it. Still far cheaper than a full reload: a HashMap scan
        // over already-parsed ClassInfo, not a re-parse of anything.
        self.registry.flatten_traits();
        register_references(&mut self.dependents, path, &references);

        self.files.insert(
            path.to_path_buf(),
            FileState {
                ast,
                functions,
                classes,
                references,
            },
        );

        let mut affected: HashSet<PathBuf> = HashSet::new();
        affected.insert(path.to_path_buf());
        for symbol in &changed_symbols {
            if let Some(files) = self.dependents.get(symbol) {
                affected.extend(files.iter().cloned());
            }
        }

        affected
            .into_iter()
            .map(|p| {
                let diagnostics = self.check_file(&p);
                (p, diagnostics)
            })
            .collect()
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

fn register_references(
    dependents: &mut HashMap<String, HashSet<PathBuf>>,
    path: &Path,
    references: &HashSet<String>,
) {
    for name in references {
        dependents
            .entry(name.clone())
            .or_default()
            .insert(path.to_path_buf());
    }
}

fn unregister_references(
    dependents: &mut HashMap<String, HashSet<PathBuf>>,
    path: &Path,
    references: &HashSet<String>,
) {
    for name in references {
        if let Some(files) = dependents.get_mut(name) {
            files.remove(path);
            if files.is_empty() {
                dependents.remove(name);
            }
        }
    }
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

    #[test]
    fn changing_a_declaration_re_checks_files_that_reference_it() {
        let mut project = Project::new();
        project.load_file(
            PathBuf::from("base.php"),
            "<?php class Base { public int $x = 1; }",
        );
        project.load_file(
            PathBuf::from("caller.php"),
            "<?php function useIt(Base $b) { return $b->x + 1; }",
        );
        project.load_file(
            PathBuf::from("unrelated.php"),
            "<?php function unrelated(): int { return 1; }",
        );
        project.finish_loading();

        // Base.php's own edit doesn't itself introduce a diagnostic here,
        // but caller.php (which references Base by name in a type hint)
        // must be among the re-checked files - unrelated.php must not be.
        let result = project.update_file(
            &PathBuf::from("base.php"),
            "<?php class Base { public int $x = 1; public function extra(): void {} }",
        );

        assert!(result.contains_key(&PathBuf::from("base.php")));
        assert!(result.contains_key(&PathBuf::from("caller.php")));
        assert!(!result.contains_key(&PathBuf::from("unrelated.php")));
    }

    #[test]
    fn removing_a_referenced_declaration_still_flags_its_old_dependents() {
        let mut project = Project::new();
        project.load_file(
            PathBuf::from("thing.php"),
            "<?php function thing(): int { return 1; }",
        );
        project.load_file(
            PathBuf::from("caller.php"),
            "<?php function useThing() { return thing(); }",
        );
        project.finish_loading();

        // thing.php no longer declares `thing` at all - caller.php, which
        // calls it by name, still needs to be re-checked (it'll now see
        // an unresolvable call), even though nothing in caller.php itself
        // changed.
        let result = project.update_file(&PathBuf::from("thing.php"), "<?php // now empty");

        assert!(result.contains_key(&PathBuf::from("thing.php")));
        assert!(result.contains_key(&PathBuf::from("caller.php")));
        assert!(!project.registry().functions.contains_key("thing"));
    }
}
