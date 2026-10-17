use rustc_hash::FxHashMap;

#[cfg(test)]
use syntax::ast::Stmt;
use syntax::ast::{FunctionDecl, TypeHint};
#[cfg(test)]
use syntax::declarations::Declared;
use syntax::name::Name;
use syntax::span::Span;

use crate::file_id::FileId;
use crate::shard::{join_map, split_map};

/// One `function` declaration's own site in its file: where its name is, and
/// its native shape at index time - each parameter's native hint and the
/// native return hint. The file is not named here; the table holds it beside
/// the site.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct FnDeclSite {
    pub(crate) span: Span,
    // The declaration's native shape at index time: each parameter's
    // native hint and the native return hint.
    pub(crate) param_hints: Vec<Option<String>>,
    pub(crate) return_hint: Option<String>,
}

impl FnDeclSite {
    /// The site of `f`, a declaration a file's walk finds.
    pub(crate) fn of(f: &FunctionDecl) -> FnDeclSite {
        FnDeclSite {
            span: f.name.span,
            param_hints: f
                .params
                .iter()
                .map(|p| p.type_hint.as_ref().map(TypeHint::to_string))
                .collect(),
            return_hint: f.return_type.as_ref().map(TypeHint::to_string),
        }
    }
}

/// Every function's own unconditional declaration site(s), across the whole
/// project, each with the file it is in - always populated, since
/// `check_duplicate_function_declarations` needs to run by default for
/// `check-project`, matching real PHPStan's own always-on `function.duplicate`
/// rule. Every unconditional declaration of a name is kept (not just the
/// winning one), since the check needs to see every occurrence to build its
/// own full location list, exactly mirroring real PHPStan's
/// `DuplicateFunctionDeclarationRule`. A conditional declaration is never
/// tracked: where `registry::build_declarations` still records one (with
/// weaker "first unconditional wins" semantics), the walk of its file drops
/// it entirely. Real PHPStan's own rule has no such guard and is a known,
/// disclosed false-positive source for the common
/// `symfony/polyfill-*`-style `if (!function_exists('foo')) { function
/// foo() {} }` idiom (two unrelated packages independently guarding the
/// identical polyfill name) - a deliberate, stricter departure from literal
/// real-PHPStan behavior, matching this project's own "favor false negatives
/// over false positives" precedent.
#[derive(Default, Debug, PartialEq, Eq, Clone)]
pub(crate) struct DuplicateFunctionSites {
    sites: FxHashMap<String, Vec<(FileId, FnDeclSite)>>,
}

impl DuplicateFunctionSites {
    /// Adds the site of a function `file` declares unconditionally, under its
    /// name, after the sites the name already has. The files are added in
    /// load order, and each file's sites in the order its walk finds them, so
    /// a name's sites are in that order too.
    pub(crate) fn add(&mut self, name: String, file: FileId, site: FnDeclSite) {
        self.sites.entry(name).or_default().push((file, site));
    }

    /// Filters each name's own `Vec` down by `file` rather than a flat
    /// `.remove()` - one function name here can legitimately have sites in
    /// several different files at once, so removing `file`'s own contribution
    /// must leave every other file's site(s) for that same name intact.
    pub(crate) fn remove_declarations(&mut self, file: FileId, removed_functions: &[Name]) {
        for name in removed_functions {
            if let Some(sites) = self.sites.get_mut(name.as_str()) {
                sites.retain(|(site_file, _)| *site_file != file);
                if sites.is_empty() {
                    self.sites.remove(name.as_str());
                }
            }
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &Vec<(FileId, FnDeclSite)>)> {
        self.sites.iter()
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

/// The unconditional function declarations of `stmts`, by name, in walk order,
/// as a file's walk records them.
#[cfg(test)]
pub(crate) fn decl_sites(stmts: &[Stmt]) -> Vec<(String, FnDeclSite)> {
    let mut sites = Vec::new();
    syntax::declarations::for_each_declaration(stmts, |declaration, _, _| {
        if let Declared::Function(f) = declaration.declared
            && !declaration.conditional
        {
            sites.push((declaration.name(), FnDeclSite::of(f)));
        }
    });
    sites
}
