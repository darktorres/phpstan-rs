use rustc_hash::FxHashMap;

use syntax::ast::ClassKind;
#[cfg(test)]
use syntax::ast::Stmt;
#[cfg(test)]
use syntax::declarations::Declared;
use syntax::name::Name;
use syntax::span::Span;

use crate::file_id::FileId;
use crate::shard::{join_map, split_map};

/// One class-like (`class`/`interface`/`trait`/`enum`) declaration's own site
/// in its file, mirroring `project::duplicate_fn_sites::FnDeclSite` with one
/// field the function side doesn't need: `is_stub`, whether the site's own
/// file falls under a configured `stub_paths` entry. A site tagged `is_stub`
/// is never flagged and never counts toward flagging another site.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ClassDeclSite {
    pub(crate) span: Span,
    pub(crate) kind: ClassKind,
    pub(crate) is_stub: bool,
}

/// Every class-like's own unconditional declaration site(s), across the whole
/// project, each with the file it is in - a straight port of
/// `DuplicateFunctionSites` to class-likes, plus the `is_stub` field above.
/// Gated behind `RulesSettings::check_duplicate_class_declarations`, which is
/// off by default. `DuplicateFunctionSites` has no such gate.
#[derive(Default, Debug, PartialEq, Eq, Clone)]
pub(crate) struct DuplicateClassSites {
    sites: FxHashMap<String, Vec<(FileId, ClassDeclSite)>>,
}

impl DuplicateClassSites {
    /// Adds the site of a class-like `file` declares unconditionally, under its
    /// name, after the sites the name already has. Files are added in load
    /// order and each file's sites in walk order, as
    /// `DuplicateFunctionSites::add` adds them.
    pub(crate) fn add(&mut self, name: String, file: FileId, site: ClassDeclSite) {
        self.sites.entry(name).or_default().push((file, site));
    }

    /// Mirrors `DuplicateFunctionSites::remove_declarations`'s own shape
    /// exactly.
    pub(crate) fn remove_declarations(&mut self, file: FileId, removed_classes: &[Name]) {
        for name in removed_classes {
            if let Some(sites) = self.sites.get_mut(name.as_str()) {
                sites.retain(|(site_file, _)| *site_file != file);
                if sites.is_empty() {
                    self.sites.remove(name.as_str());
                }
            }
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &Vec<(FileId, ClassDeclSite)>)> {
        self.sites.iter()
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

/// The unconditional class-like declarations of `stmts`, by name, in walk
/// order, as a file's walk records them. `is_stub` is the file's provenance.
#[cfg(test)]
pub(crate) fn decl_sites(stmts: &[Stmt], is_stub: bool) -> Vec<(String, ClassDeclSite)> {
    let mut sites = Vec::new();
    syntax::declarations::for_each_declaration(stmts, |declaration, _, _| {
        if let Declared::Class(c) = declaration.declared
            && !declaration.conditional
        {
            sites.push((
                declaration.name(),
                ClassDeclSite {
                    span: c.name.span,
                    kind: c.def.kind,
                    is_stub,
                },
            ));
        }
    });
    sites
}
