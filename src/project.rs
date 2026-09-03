use rustc_hash::FxHashMap;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{Stmt, StmtKind};
use crate::checker::{Checker, Diagnostic};
use crate::deps;
use crate::lexer::Lexer;
use crate::parser::{ParseError, Parser};
use crate::pest_config::PestConfig;
use crate::php_stubs;
use crate::registry::{NameContext, Registry};

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
    files: FxHashMap<PathBuf, FileState>,
    dependents: FxHashMap<String, HashSet<PathBuf>>,
    registry: Registry,
    // Directory-scoped Pest `$this`-binding config, rebuilt from every
    // currently-loaded `Pest.php` file whenever the loaded file set could
    // have changed (`finish_loading`/`update_file`) - see `pest_config`'s
    // own module doc comment for the full resolution algorithm.
    pest_config: PestConfig,
}

impl Project {
    pub fn new() -> Self {
        let mut registry = Registry::default();
        registry.merge_stubs(php_stubs::generated_stubs());
        registry.merge_stub_classes(php_stubs::generated_stub_classes());
        Project {
            files: FxHashMap::default(),
            dependents: FxHashMap::default(),
            registry,
            pest_config: PestConfig::default(),
        }
    }

    /// Rebuilds `pest_config` from every currently-loaded `Pest.php` file
    /// (`PestConfig::build` itself filters for that filename) - cheap
    /// regardless of corpus size, since it's proportional only to the
    /// small number of `Pest.php` files a real project has, never the
    /// whole loaded file set. Called unconditionally from both
    /// `finish_loading` and `update_file`, rather than only when the
    /// specific edited file is itself a `Pest.php`, to keep the LSP
    /// incremental-edit path correct if a `Pest.php` config itself is
    /// what just changed.
    fn rebuild_pest_config(&mut self) {
        self.pest_config = PestConfig::build(
            self.files
                .iter()
                .map(|(path, state)| (path.as_path(), state.ast.as_slice())),
        );
    }

    /// Opts the whole project's checking into Phase 11 Stage 5's
    /// strictness parameter (`registry::Registry::strict_mixed_arrays`) -
    /// a narrow, purpose-built setter rather than exposing a general
    /// `&mut Registry` accessor, which would let a caller bypass
    /// `load_file`/`update_file`'s own bookkeeping around what's
    /// currently indexed.
    pub fn set_strict_mixed_arrays(&mut self, strict: bool) {
        self.registry.strict_mixed_arrays = strict;
    }

    /// Parses one file and indexes its declarations into the shared
    /// registry. Call `finish_loading` once after the initial batch of
    /// `load_file` calls - trait flattening is a global pass over every
    /// class, so it's wasteful to re-run it after each individual file
    /// during a bulk load the way `update_file` has to for a single
    /// incremental change.
    #[hotpath::measure]
    pub fn load_file(&mut self, path: PathBuf, source: &str) -> Result<(), ParseError> {
        let ast = parse(source)?;
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
        Ok(())
    }

    /// Finalizes an initial batch of `load_file` calls (trait flattening).
    #[hotpath::measure]
    pub fn finish_loading(&mut self) {
        self.registry.flatten_traits();
        self.rebuild_pest_config();
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
    ) -> Result<HashMap<PathBuf, Vec<Diagnostic>>, ParseError> {
        let mut changed_symbols: HashSet<String> = HashSet::new();

        if let Some(old) = self.files.get(path) {
            self.registry
                .remove_declarations(&old.functions, &old.classes);
            changed_symbols.extend(old.functions.iter().map(|s| bare_name(s).to_string()));
            changed_symbols.extend(old.classes.iter().map(|s| bare_name(s).to_string()));
            unregister_references(&mut self.dependents, path, &old.references);
        }

        let ast = parse(new_source)?;
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
        self.rebuild_pest_config();

        Ok(self
            .transitive_affected_files(path, changed_symbols)
            .into_iter()
            .map(|p| {
                let diagnostics = self.check_file(&p);
                (p, diagnostics)
            })
            .collect())
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
    #[hotpath::measure]
    pub fn check_file(&self, path: &Path) -> Vec<Diagnostic> {
        let Some(file) = self.files.get(path) else {
            return Vec::new();
        };
        let mut checker = Checker::new(&self.registry);
        checker.set_pest_this_type(self.pest_config.resolve_this_type(path, &self.registry));
        checker.set_pest_hook_properties(self.pest_config.resolve_hook_properties(path));
        checker.check_program(&file.ast);
        checker.into_diagnostics()
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Runs the checker over every currently-loaded file, not just one -
    /// the whole-project counterpart to `check_file`. `--check-project`
    /// no longer uses this directly (it needs to check only the files it
    /// was asked to analyze, not every file loaded for context - see
    /// `main.rs::run_check_project`), but it's still the right tool for
    /// tests/tooling that need to see every diagnostic across a real
    /// corpus at once, rather than one file (and its
    /// transitively-affected dependents) at a time the way `update_file`
    /// does.
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

fn parse(source: &str) -> Result<Vec<Stmt>, ParseError> {
    let tokens = Lexer::new(source).tokenize_with_spans();
    Parser::new(tokens).parse_program()
}

/// Recursively collects every `.php` file under `path` - shared by the CLI
/// (`main.rs`'s own subcommands) and the LSP's own workspace-loading path
/// (`lsp.rs`'s `load_workspace`), which is why this lives in the library
/// rather than as CLI-only glue in `main.rs`.
pub fn collect_php_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            collect_php_files(&entry.path(), out);
        }
    } else if path.extension().is_some_and(|e| e == "php") {
        out.push(path.to_path_buf());
    }
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
        match &stmt.kind {
            StmtKind::Namespace(name) => ctx.set_namespace(name),
            StmtKind::Use(imports) => ctx.add_uses(imports),
            StmtKind::FunctionDecl(f) => functions.push(ctx.declared_name(&f.name)),
            StmtKind::ClassDecl(c) => classes.push(ctx.declared_name(&c.name)),
            StmtKind::EnumDecl(e) => classes.push(ctx.declared_name(&e.name)),
            // Must mirror Registry::index's recursion into `if`/block
            // bodies (the `if (!function_exists(...)) { function foo()
            // {} }` polyfill idiom) - otherwise an incremental update
            // wouldn't know to remove a conditionally-declared symbol
            // that index() itself did pick up, leaving a stale entry
            // behind after the declaring file changes or is removed.
            StmtKind::If {
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
            StmtKind::Block(inner) => {
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
    dependents: &mut FxHashMap<String, HashSet<PathBuf>>,
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
    dependents: &mut FxHashMap<String, HashSet<PathBuf>>,
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
mod tests;
