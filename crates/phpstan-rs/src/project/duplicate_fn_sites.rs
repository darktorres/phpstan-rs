use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

#[cfg(test)]
use syntax::ast::Stmt;
use syntax::ast::{FunctionDecl, TypeHint};
#[cfg(test)]
use syntax::declarations::Declared;
use syntax::name::Name;
use syntax::span::Span;

use crate::shard::{join_map, split_map};

/// One `function` declaration's own site, name-keyed the same way
/// `Registry` is - but every unconditional
/// declaration of a name is kept (not just the winning one), since
/// `check_duplicate_function_declarations` needs to see every occurrence
/// to build its own full location list, exactly mirroring real PHPStan's
/// `DuplicateFunctionDeclarationRule`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct FnDeclSite {
    pub(crate) path: PathBuf,
    pub(crate) span: Span,
    // The declaration's native shape at index time: each parameter's
    // native hint and the native return hint.
    pub(crate) param_hints: Vec<Option<String>>,
    pub(crate) return_hint: Option<String>,
}

/// Every function's own unconditional declaration site(s), across the
/// whole project - always populated, since `check_duplicate_function_
/// declarations` needs to run by default for `check-project`, matching
/// real PHPStan's own always-on `function.duplicate` rule. A conditional
/// declaration is never tracked (see `index`).
#[derive(Default, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Clone)]
pub(crate) struct DuplicateFunctionSites {
    sites: FxHashMap<String, Vec<FnDeclSite>>,
}

impl DuplicateFunctionSites {
    /// Records `stmts`' unconditional function declarations. Where
    /// `registry::build_declarations` still records a conditional one
    /// (with weaker "first unconditional wins" semantics), this drops it
    /// entirely: real PHPStan's own rule has no such guard and is a known,
    /// disclosed false-positive source for the common
    /// `symfony/polyfill-*`-style `if (!function_exists('foo')) { function
    /// foo() {} }` idiom (two unrelated packages independently guarding
    /// the identical polyfill name) - a deliberate, stricter departure
    /// from literal real-PHPStan behavior, matching this project's own
    /// "favor false negatives over false positives" precedent.
    #[cfg(test)]
    pub(crate) fn index(&mut self, stmts: &[Stmt], path: &Path) {
        syntax::declarations::for_each_declaration(stmts, |declaration, _, _| {
            if let Declared::Function(f) = declaration.declared
                && !declaration.conditional
            {
                self.record(declaration.name(), path, f);
            }
        });
    }

    pub(crate) fn record(&mut self, fqn: String, path: &Path, f: &FunctionDecl) {
        self.sites.entry(fqn).or_default().push(FnDeclSite {
            path: path.to_path_buf(),
            span: f.name.span,
            param_hints: f
                .params
                .iter()
                .map(|p| p.type_hint.as_ref().map(TypeHint::to_string))
                .collect(),
            return_hint: f.return_type.as_ref().map(TypeHint::to_string),
        });
    }

    /// Filters each name's own `Vec` down by `path` rather than a flat
    /// `.remove()` - one function name here can legitimately have sites in several
    /// different files at once, so removing `path`'s own contribution
    /// must leave every other file's site(s) for that same name intact.
    pub(crate) fn remove_declarations(&mut self, path: &Path, removed_functions: &[Name]) {
        for name in removed_functions {
            if let Some(sites) = self.sites.get_mut(name.as_str()) {
                sites.retain(|s| s.path != path);
                if sites.is_empty() {
                    self.sites.remove(name.as_str());
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
        for (name, sites) in other.into_names() {
            self.merge_name(name, sites);
        }
    }

    /// Adds `sites`, the sites one file declares `name` at, after the sites
    /// `self` holds for it.
    pub(crate) fn merge_name(&mut self, name: String, sites: Vec<FnDeclSite>) {
        self.sites.entry(name).or_default().extend(sites);
    }

    /// The names of the table, each with its sites, for a merge that routes
    /// each name to its own shard.
    pub(crate) fn into_names(self) -> impl Iterator<Item = (String, Vec<FnDeclSite>)> {
        self.sites.into_iter()
    }

    /// Takes the table apart into `SHARDS` tables, each holding the names
    /// `shard_of_key` routes to it. `join_shards` puts them back together.
    pub(crate) fn split_shards(self) -> Vec<DuplicateFunctionSites> {
        split_map(self.sites)
            .into_iter()
            .map(|sites| DuplicateFunctionSites { sites })
            .collect()
    }

    /// Joins the tables `split_shards` made back into one.
    pub(crate) fn join_shards(shards: Vec<DuplicateFunctionSites>) -> DuplicateFunctionSites {
        DuplicateFunctionSites {
            sites: join_map(shards.into_iter().map(|shard| shard.sites).collect()),
        }
    }
}
