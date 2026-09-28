use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use syntax::ast::{FunctionDecl, Stmt, StmtKind, TypeHint};
use syntax::name_resolution::NameContext;
use syntax::token::Span;

/// One top-level `function` declaration's own site, name-keyed the same
/// way `Registry`/`DeclLocations` are - but every unconditional
/// declaration of a name is kept (not just the winning one), since
/// `check_duplicate_function_declarations` needs to see every occurrence
/// to build its own full location list, exactly mirroring real PHPStan's
/// `DuplicateFunctionDeclarationRule`.
pub(crate) struct FnDeclSite {
    pub(crate) path: PathBuf,
    pub(crate) span: Span,
    // The declaration's native shape at index time: each parameter's
    // native hint and the native return hint.
    pub(crate) param_hints: Vec<Option<String>>,
    pub(crate) return_hint: Option<String>,
}

/// Every top-level function's own unconditional declaration site(s),
/// across the whole project - always populated (unlike `DeclLocations`,
/// which is LSP-only and opt-in), since `check_duplicate_function_
/// declarations` needs to run by default for `check-project`, matching
/// real PHPStan's own always-on `function.duplicate` rule. A new,
/// purpose-built walker (not a generalization of `DeclLocations::index` or
/// `registry::build_declarations`) for the same reason `DeclLocations`
/// itself gives for being its own walker: this one's `conditional`
/// handling is load-bearing for correctness (an `if (!function_exists(...))
/// { function foo() {} }` polyfill guard must never be tracked at all -
/// see `index`'s own doc comment), unlike `DeclLocations`'s deliberately
/// looser last-wins convenience lookup.
#[derive(Default)]
pub(crate) struct DuplicateFunctionSites {
    sites: FxHashMap<String, Vec<FnDeclSite>>,
}

impl DuplicateFunctionSites {
    /// `conditional` mirrors `registry::build_declarations`'s own
    /// identical parameter and `If`/`Block` recursion shape - but where
    /// that function still *records* a conditional declaration (just with
    /// weaker "first unconditional wins" semantics), this one drops a
    /// conditionally-reached `FunctionDecl` entirely: real PHPStan's own
    /// rule has no such guard and is a known, disclosed false-positive
    /// source for the common `symfony/polyfill-*`-style `if (!
    /// function_exists('foo')) { function foo() {} }` idiom (two
    /// unrelated packages independently guarding the identical polyfill
    /// name) - a deliberate, stricter departure from literal real-PHPStan
    /// behavior, matching this project's own "favor false negatives over
    /// false positives" precedent.
    pub(crate) fn index(
        &mut self,
        stmts: &[Stmt],
        ctx: &mut NameContext,
        path: &Path,
        conditional: bool,
    ) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Namespace(name) => ctx.set_namespace(name),
                StmtKind::Use(imports) => ctx.add_uses(imports),
                StmtKind::FunctionDecl(f) => {
                    if conditional {
                        continue;
                    }
                    self.record(ctx.declared_name(&f.name), path, f);
                }
                StmtKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.index(
                        std::slice::from_ref(then_branch),
                        &mut ctx.clone(),
                        path,
                        true,
                    );
                    if let Some(else_branch) = else_branch {
                        self.index(
                            std::slice::from_ref(else_branch),
                            &mut ctx.clone(),
                            path,
                            true,
                        );
                    }
                }
                StmtKind::Block(inner)
                | StmtKind::Declare {
                    body: Some(inner), ..
                } => self.index(inner, ctx, path, conditional),
                _ => {}
            }
        }
    }

    fn record(&mut self, fqn: String, path: &Path, f: &FunctionDecl) {
        self.sites.entry(fqn).or_default().push(FnDeclSite {
            path: path.to_path_buf(),
            span: f.name_span,
            param_hints: f
                .params
                .iter()
                .map(|p| p.type_hint.as_ref().map(TypeHint::to_string))
                .collect(),
            return_hint: f.return_type.as_ref().map(TypeHint::to_string),
        });
    }

    /// Mirrors `DeclLocations::remove_declarations`'s own shape, but
    /// filters each name's own `Vec` down by `path` rather than a flat
    /// `.remove()` - unlike `DeclLocations` (one winning site per name),
    /// one function name here can legitimately have sites in several
    /// different files at once, so removing `path`'s own contribution
    /// must leave every other file's site(s) for that same name intact.
    pub(crate) fn remove_declarations(&mut self, path: &Path, removed_functions: &[String]) {
        for name in removed_functions {
            if let Some(sites) = self.sites.get_mut(name) {
                sites.retain(|s| s.path != path);
                if sites.is_empty() {
                    self.sites.remove(name);
                }
            }
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &Vec<FnDeclSite>)> {
        self.sites.iter()
    }

    /// Absorbs `other`'s own sites into `self` - correct regardless of
    /// which file's own per-file `DuplicateFunctionSites` `self`/`other`
    /// each hold, since every site already carries its own `path` and
    /// `record`'s own append-only `Vec` per name never depends on
    /// insertion order (unlike `Registry::insert_built_decl`'s FQN-
    /// collision resolution - there is no such collision here, every
    /// unconditional declaration is kept, not just a winner). Lets
    /// `check_project.rs`'s parallel parse stage build one
    /// `DuplicateFunctionSites` per file concurrently and merge them all
    /// sequentially afterward, instead of calling `index` once per file
    /// inside the sequential insert loop.
    pub(crate) fn merge(&mut self, other: DuplicateFunctionSites) {
        for (name, sites) in other.sites {
            self.sites.entry(name).or_default().extend(sites);
        }
    }
}
