use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use syntax::ast::ClassKind;
#[cfg(test)]
use syntax::ast::Stmt;
#[cfg(test)]
use syntax::declarations::Declared;
use syntax::name::Name;
use syntax::span::Span;

use crate::shard::{join_map, split_map};

/// One class-like (`class`/`interface`/`trait`/`enum`) declaration's own
/// site, name-keyed the same way `Registry` is - mirrors `project::duplicate_fn_sites::FnDeclSite`
/// exactly, with one new field this side needs and the function side
/// doesn't: `is_stub`, whether this site's own file falls under a
/// configured `stub_paths` entry (see `project::duplicate_classes`'s own
/// doc comment for why that distinction exists at all - a designated
/// override for a real corpus-vendored stub file, e.g. `Imagick.stub.
/// php`, must never itself be flagged, and must never count toward
/// flagging the real declaration it's overriding).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ClassDeclSite {
    pub(crate) path: PathBuf,
    pub(crate) span: Span,
    pub(crate) kind: ClassKind,
    pub(crate) is_stub: bool,
}

/// Every class-like's own unconditional declaration site(s),
/// across the whole project - a straight port of `DuplicateFunctionSites`
/// to class-likes, plus the `is_stub` field above. Gated behind
/// `RulesSettings::check_duplicate_class_declarations` (opt-in, unlike
/// `DuplicateFunctionSites`, which has no such gate - see that field's own
/// doc comment for why this one needs it and the function-side precedent
/// didn't).
#[derive(Default, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Clone)]
pub(crate) struct DuplicateClassSites {
    sites: FxHashMap<String, Vec<ClassDeclSite>>,
}

impl DuplicateClassSites {
    /// Records `stmts`' unconditional class-like declarations, dropping
    /// a conditional one as `DuplicateFunctionSites::index` does, for the
    /// same false-negative-favoring reason (a real, if rarer, analogue of
    /// the `function_exists`-guarded polyfill idiom: `if (!class_exists(
    /// Foo::class)) { class Foo {} }`). `is_stub` is the file's
    /// provenance.
    #[cfg(test)]
    pub(crate) fn index(&mut self, stmts: &[Stmt], path: &Path, is_stub: bool) {
        syntax::declarations::for_each_declaration(stmts, |declaration, _, _| {
            if let Declared::Class(c) = declaration.declared
                && !declaration.conditional
            {
                self.record(declaration.name(), path, c.name.span, c.def.kind, is_stub);
            }
        });
    }

    pub(crate) fn record(
        &mut self,
        fqn: String,
        path: &Path,
        span: Span,
        kind: ClassKind,
        is_stub: bool,
    ) {
        self.sites.entry(fqn).or_default().push(ClassDeclSite {
            path: path.to_path_buf(),
            span,
            kind,
            is_stub,
        });
    }

    /// Mirrors `DuplicateFunctionSites::remove_declarations`'s own shape
    /// exactly.
    pub(crate) fn remove_declarations(&mut self, path: &Path, removed_classes: &[Name]) {
        for name in removed_classes {
            if let Some(sites) = self.sites.get_mut(name.as_str()) {
                sites.retain(|s| s.path != path);
                if sites.is_empty() {
                    self.sites.remove(name.as_str());
                }
            }
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &Vec<ClassDeclSite>)> {
        self.sites.iter()
    }

    /// Mirrors `DuplicateFunctionSites::merge`'s own doc comment/shape
    /// exactly.
    pub(crate) fn merge(&mut self, other: DuplicateClassSites) {
        for (name, sites) in other.into_names() {
            self.merge_name(name, sites);
        }
    }

    /// Adds `sites`, the sites one file declares `name` at, after the sites
    /// `self` holds for it.
    pub(crate) fn merge_name(&mut self, name: String, sites: Vec<ClassDeclSite>) {
        self.sites.entry(name).or_default().extend(sites);
    }

    /// The names of the table, each with its sites, for a merge that routes
    /// each name to its own shard.
    pub(crate) fn into_names(self) -> impl Iterator<Item = (String, Vec<ClassDeclSite>)> {
        self.sites.into_iter()
    }

    /// Takes the table apart into `SHARDS` tables, each holding the names
    /// `shard_of_key` routes to it. `join_shards` puts them back together.
    pub(crate) fn split_shards(self) -> Vec<DuplicateClassSites> {
        split_map(self.sites)
            .into_iter()
            .map(|sites| DuplicateClassSites { sites })
            .collect()
    }

    /// Joins the tables `split_shards` made back into one.
    pub(crate) fn join_shards(shards: Vec<DuplicateClassSites>) -> DuplicateClassSites {
        DuplicateClassSites {
            sites: join_map(shards.into_iter().map(|shard| shard.sites).collect()),
        }
    }
}
