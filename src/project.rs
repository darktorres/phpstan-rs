use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::Stmt;
use crate::checker::{Checker, Diagnostic};
use crate::deps;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::php_stubs;
use crate::registry::{NameContext, Registry};
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
/// changed, plus the *transitive* closure of every file that references a
/// symbol it declares, or references a symbol declared by one of those
/// files, and so on (see `transitive_affected_files`) - every other file's
/// parse tree and registry entries are untouched.
///
/// What this does NOT do: narrow by *which* aspect of a symbol changed -
/// any file reachable through that chain gets re-checked, even if the
/// specific thing it uses didn't change. That's a safe over-approximation
/// (it costs an occasional extra re-check, never a missed one) and
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
        registry.merge_stub_classes(php_stubs::generated_stub_classes());
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
            changed_symbols.extend(old.functions.iter().map(|s| bare_name(s).to_string()));
            changed_symbols.extend(old.classes.iter().map(|s| bare_name(s).to_string()));
            unregister_references(&mut self.dependents, path, &old.references);
        }

        let ast = parse(new_source);
        let (functions, classes) = declared_names(&ast);
        let references = deps::referenced_names(&ast);
        // `changed_symbols` seeds transitive_affected_files' BFS, which
        // looks names up in `self.dependents` - keyed by the *bare* names
        // deps::referenced_names collects (a deliberate simplification,
        // see that function's own doc comment), not the fully-qualified
        // ones `functions`/`classes` carry (needed for correct
        // Registry removal above, Phase 9.5). Feeding FQNs into a
        // bare-keyed lookup silently found nothing for any namespaced
        // class - confirmed as a real regression this uncovered:
        // editing a widely-depended-on namespaced class stopped
        // cascading to any of its real dependents at all.
        changed_symbols.extend(functions.iter().map(|s| bare_name(s).to_string()));
        changed_symbols.extend(classes.iter().map(|s| bare_name(s).to_string()));

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

        self.transitive_affected_files(path, changed_symbols)
            .into_iter()
            .map(|p| {
                let diagnostics = self.check_file(&p);
                (p, diagnostics)
            })
            .collect()
    }

    /// The full, transitive closure of files that need re-checking after
    /// `changed_file`'s declared symbols became `seed_symbols` (its own
    /// old+new declared names) - not just the files that directly
    /// reference one of those symbols, but the fixed point of "declares ->
    /// is referenced by -> declares -> is referenced by -> ...".
    ///
    /// This is what makes invalidation correct across inheritance chains:
    /// if file A references class B (`function f(B $b) { ... $b->x ... }`)
    /// and B `extends` class C declared in a third file, a change to C's
    /// shape can change what A's use of `$b->x` type-checks to, even
    /// though A's own source never mentions C by name. A one-hop lookup
    /// from C's dependents would find B but stop there and miss A; this
    /// keeps expanding the frontier through every newly-affected file's
    /// own declarations until nothing new turns up.
    fn transitive_affected_files(
        &self,
        changed_file: &Path,
        seed_symbols: HashSet<String>,
    ) -> HashSet<PathBuf> {
        let mut affected = HashSet::new();
        affected.insert(changed_file.to_path_buf());

        let mut seen_symbols = HashSet::new();
        let mut frontier: Vec<String> = Vec::new();
        for symbol in seed_symbols {
            if seen_symbols.insert(symbol.clone()) {
                frontier.push(symbol);
            }
        }

        while let Some(symbol) = frontier.pop() {
            let Some(files) = self.dependents.get(&symbol) else {
                continue;
            };
            for file in files {
                if !affected.insert(file.clone()) {
                    continue;
                }
                // `file` is newly affected - its own effective shape may
                // have just changed too (e.g. it `extends` the symbol that
                // changed), so anything depending on what *it* declares
                // needs to be considered next.
                let Some(state) = self.files.get(file) else {
                    continue;
                };
                // Bare, not FQN - see update_file's identical note on
                // changed_symbols; this frontier feeds the same
                // bare-keyed `self.dependents` lookup above.
                for name in state.functions.iter().chain(state.classes.iter()) {
                    let bare = bare_name(name).to_string();
                    if seen_symbols.insert(bare.clone()) {
                        frontier.push(bare);
                    }
                }
            }
        }

        affected
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

    /// Runs the checker over every currently-loaded file, not just one -
    /// the whole-project counterpart to `check_file`. Used by
    /// `--check-project` and by tests/tooling that need to see every
    /// diagnostic across a real corpus at once, rather than one file (and
    /// its transitively-affected dependents) at a time the way
    /// `update_file` does.
    pub fn check_all(&self) -> HashMap<PathBuf, Vec<Diagnostic>> {
        self.files
            .keys()
            .map(|path| (path.clone(), self.check_file(path)))
            .collect()
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

/// The last segment of a fully-qualified name (`"App\Foo\Bar"` becomes
/// `"Bar"`). `declared_names` returns FQNs (needed to match
/// `Registry::index`'s namespace-aware keys, for correct removal), but
/// the invalidation graph (`dependents`, built from
/// `deps::referenced_names`) is deliberately keyed by bare names, so
/// anything feeding that graph's BFS needs converting back down to bare
/// first.
fn bare_name(fqn: &str) -> &str {
    fqn.rsplit('\\').next().unwrap_or(fqn)
}

/// The top-level function/class/enum names one file's parse tree declares -
/// exactly what a registry entry needs to be identified by to be removed
/// again later. Enums share the registry's `classes` map (see
/// `Registry::index`), so they're collected into the same list here.
/// Must produce exactly the same names `Registry::index` actually keys
/// its entries under - otherwise an incremental update's removal step
/// would look up the wrong key and leave a stale entry behind. Mirrors
/// `Registry::index_with_context`'s structure (including its positional,
/// mutated-in-place `NameContext` tracking - see that method's own doc
/// comment on why a namespace can't just be computed once up front for
/// the whole file) for exactly that reason.
fn declared_names(stmts: &[Stmt]) -> (Vec<String>, Vec<String>) {
    let mut ctx = NameContext::default();
    declared_names_with_context(stmts, &mut ctx)
}

fn declared_names_with_context(
    stmts: &[Stmt],
    ctx: &mut NameContext,
) -> (Vec<String>, Vec<String>) {
    let mut functions = Vec::new();
    let mut classes = Vec::new();
    for stmt in stmts {
        match stmt {
            Stmt::Namespace(name) => ctx.set_namespace(name),
            Stmt::Use(imports) => ctx.add_uses(imports),
            Stmt::FunctionDecl(f) => functions.push(ctx.declared_name(&f.name)),
            Stmt::ClassDecl(c) => classes.push(ctx.declared_name(&c.name)),
            Stmt::EnumDecl(e) => classes.push(ctx.declared_name(&e.name)),
            // Must mirror Registry::index's recursion into `if`/block
            // bodies (the `if (!function_exists(...)) { function foo()
            // {} }` polyfill idiom) - otherwise an incremental update
            // wouldn't know to remove a conditionally-declared symbol
            // that index() itself did pick up, leaving a stale entry
            // behind after the declaring file changes or is removed.
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                let (f, c) = declared_names_with_context(
                    std::slice::from_ref(then_branch),
                    &mut ctx.clone(),
                );
                functions.extend(f);
                classes.extend(c);
                if let Some(else_branch) = else_branch {
                    let (f, c) = declared_names_with_context(
                        std::slice::from_ref(else_branch),
                        &mut ctx.clone(),
                    );
                    functions.extend(f);
                    classes.extend(c);
                }
            }
            Stmt::Block(inner) => {
                let (f, c) = declared_names_with_context(inner, ctx);
                functions.extend(f);
                classes.extend(c);
            }
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
    fn cross_file_use_import_and_method_calls_resolve_correctly() {
        // Every other cross-file test in this module uses bare,
        // non-namespaced classes with no `use` import at all - the exact
        // combination (separate namespaces + a `use` import + a method
        // call checked against the imported class, all across a real
        // multi-file Project rather than a single Checker) that a
        // regression in NameContext resolution, FQN-keyed lookups, or
        // undefined-method detection could plausibly break without any
        // existing test noticing.
        let mut project = Project::new();
        project.load_file(
            PathBuf::from("user.php"),
            "<?php namespace App\\Models; class User { public function getName(): string { return 'x'; } }",
        );
        project.load_file(
            PathBuf::from("greeter.php"),
            "<?php namespace App\\Services;\nuse App\\Models\\User;\nfunction greet(User $u): string { return $u->getName(); }",
        );
        project.finish_loading();

        let diagnostics = project.check_file(&PathBuf::from("greeter.php"));
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics for a real cross-namespace method call: {diagnostics:?}"
        );

        // Same setup, but calling a method User genuinely doesn't have -
        // proves undefined-method detection also works through this same
        // cross-file, use-imported path, not just same-file/same-namespace.
        let mut bad_project = Project::new();
        bad_project.load_file(
            PathBuf::from("user.php"),
            "<?php namespace App\\Models; class User { public function getName(): string { return 'x'; } }",
        );
        bad_project.load_file(
            PathBuf::from("greeter.php"),
            "<?php namespace App\\Services;\nuse App\\Models\\User;\nfunction greet(User $u): string { return $u->totallyMadeUp(); }",
        );
        bad_project.finish_loading();
        let bad_diagnostics = bad_project.check_file(&PathBuf::from("greeter.php"));
        assert!(
            bad_diagnostics
                .iter()
                .any(|d| d.message.contains("totallyMadeUp")),
            "expected an undefined-method diagnostic, got: {bad_diagnostics:?}"
        );
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

    #[test]
    fn invalidation_is_transitive_across_an_inheritance_chain() {
        let mut project = Project::new();
        // C declares $x. B extends C (so B's *effective* shape includes
        // $x, without B's own source ever mentioning it). A references B
        // by name (a type hint) but never mentions C at all.
        project.load_file(
            PathBuf::from("c.php"),
            "<?php class C { public int $x = 1; }",
        );
        project.load_file(PathBuf::from("b.php"), "<?php class B extends C {}");
        project.load_file(
            PathBuf::from("a.php"),
            "<?php function useB(B $b) { return $b->x; }",
        );
        project.finish_loading();

        // Changing C ripples through B into A, even though a.php's own
        // parse tree never references "C" - a one-hop lookup from C's
        // direct dependents (just b.php) would miss a.php entirely.
        let result = project.update_file(
            &PathBuf::from("c.php"),
            "<?php class C { public string $x = 'a'; }",
        );

        assert!(result.contains_key(&PathBuf::from("c.php")));
        assert!(result.contains_key(&PathBuf::from("b.php")));
        assert!(result.contains_key(&PathBuf::from("a.php")));
    }

    #[test]
    fn invalidation_still_works_for_namespaced_classes() {
        // Same shape as invalidation_is_transitive_across_an_inheritance_chain,
        // but every class lives in a namespace - real code, unlike almost
        // every other test in this file. Phase 9.5 made declared_names
        // return fully-qualified names (to match Registry::index's now
        // namespace-aware keys, for correct incremental *removal*), but
        // deps::referenced_names deliberately still collects bare names
        // (an intentional, documented approximation - see its own doc
        // comment) - changed_symbols (fed from declared_names) and
        // `dependents`'s keys (fed from referenced_names) must actually
        // agree for transitive_affected_files' lookup to find anything at
        // all, and after Phase 9.5 they silently didn't for any namespaced
        // class: `dependents.get("App\\C")` found nothing under a
        // "App\\C" key that was never inserted (only the bare "C" key
        // was), so a.php was never re-checked even though it genuinely
        // depends on the changed class.
        let mut project = Project::new();
        project.load_file(
            PathBuf::from("c.php"),
            "<?php namespace App; class C { public int $x = 1; }",
        );
        project.load_file(
            PathBuf::from("b.php"),
            "<?php namespace App; class B extends C {}",
        );
        project.load_file(
            PathBuf::from("a.php"),
            "<?php namespace App; function useB(B $b) { return $b->x; }",
        );
        project.finish_loading();

        let result = project.update_file(
            &PathBuf::from("c.php"),
            "<?php namespace App; class C { public string $x = 'a'; }",
        );

        assert!(result.contains_key(&PathBuf::from("c.php")));
        assert!(
            result.contains_key(&PathBuf::from("b.php")),
            "b.php (extends C) should have been re-checked, got: {:?}",
            result.keys().collect::<Vec<_>>()
        );
        assert!(
            result.contains_key(&PathBuf::from("a.php")),
            "a.php (references B) should have been re-checked, got: {:?}",
            result.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_class_referenced_only_inside_a_property_hook_body_is_a_real_dependency() {
        // Coverage measurement found deps.rs's collect_property_hooks/
        // collect_hook_body had zero test executions - PHP 8.4 property
        // hooks are real, newer syntax, and a class mentioned only inside
        // a get/set hook body (not a plain method) is exactly the kind of
        // reference the earlier Phase 7 regression already proved this
        // codebase can silently miss.
        let mut project = Project::new();
        project.load_file(
            PathBuf::from("dep.php"),
            "<?php class Dep { public static function make(): Dep { return new Dep(); } }",
        );
        project.load_file(
            PathBuf::from("holder.php"),
            r#"<?php class Holder {
                public $prop {
                    get => Dep::make();
                }
            }"#,
        );
        project.finish_loading();

        let result = project.update_file(
            &PathBuf::from("dep.php"),
            "<?php class Dep { public static function make(): int { return 1; } }",
        );
        assert!(
            result.contains_key(&PathBuf::from("holder.php")),
            "holder.php (references Dep only inside a property hook body) should have been re-checked, got: {:?}",
            result.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_class_referenced_only_via_an_enum_is_a_real_dependency() {
        // Same coverage finding for collect_enum: an enum's `implements`
        // and its methods' bodies are real references that must feed the
        // same invalidation graph a class's `implements`/methods already
        // do - untested before this.
        let mut project = Project::new();
        project.load_file(PathBuf::from("dep.php"), "<?php interface Dep {}");
        project.load_file(
            PathBuf::from("status.php"),
            "<?php enum Status implements Dep { case Active; }",
        );
        project.finish_loading();

        let result = project.update_file(
            &PathBuf::from("dep.php"),
            "<?php interface Dep { public function f(): void; }",
        );
        assert!(
            result.contains_key(&PathBuf::from("status.php")),
            "status.php (enum implements Dep) should have been re-checked, got: {:?}",
            result.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn unrelated_files_are_not_swept_into_transitive_invalidation() {
        let mut project = Project::new();
        project.load_file(
            PathBuf::from("c.php"),
            "<?php class C { public int $x = 1; }",
        );
        project.load_file(PathBuf::from("b.php"), "<?php class B extends C {}");
        project.load_file(
            PathBuf::from("a.php"),
            "<?php function useB(B $b) { return $b->x; }",
        );
        project.load_file(
            PathBuf::from("unrelated.php"),
            "<?php function unrelated(): int { return 1; }",
        );
        project.finish_loading();

        let result = project.update_file(
            &PathBuf::from("c.php"),
            "<?php class C { public string $x = 'a'; }",
        );

        assert!(!result.contains_key(&PathBuf::from("unrelated.php")));
    }
}
