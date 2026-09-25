//! `workspace/symbol`: project-wide fuzzy symbol search over
//! `Project::decl_locations()` (already kept live by `update_file` - no
//! new index needed) - classes/enums/interfaces/traits, top-level
//! functions, and class/enum members (methods, properties, consts, enum
//! cases). Indexing is gated by `Project::retain_source`, which
//! `Server::new` turns on unconditionally before any request can arrive,
//! so by the time a `workspace/symbol` request is possible, the index is
//! already populated.

use std::path::Path;

use lsp_types::SymbolKind;

use crate::project::Project;
use crate::registry::Registry;
use crate::types::Type;
use syntax::token::Span;

use super::document_symbol::class_kind_symbol_kind;

const MAX_RESULTS: usize = 256;

pub(super) struct Match<'a> {
    pub(super) name: String,
    pub(super) kind: SymbolKind,
    pub(super) path: &'a Path,
    pub(super) span: Span,
    pub(super) container_name: Option<String>,
}

/// Every declared class/enum/interface/trait, top-level function, and
/// class/enum member whose short name fuzzy-matches `query`, ranked
/// best-match-first and capped at `MAX_RESULTS`. An empty `query`
/// matches everything at the best rank - a real client (VS Code's "Go to
/// Symbol in Workspace") can send this request before the user types
/// anything, despite the spec's own text calling `query` "non-empty" (a
/// client-authoring guideline, not a server-side guarantee).
///
/// A class/enum member's `container_name` is the owner's full FQN, not
/// its short name - needed to disambiguate two identically-named classes
/// in different namespaces that each declare a same-named member.
///
/// Known limitation, not fixed here: `DeclLocations::members` collapses
/// same-named method/property/const declarations on one class into a
/// single flat `(owner, name)` key (property beats method, const beats
/// property, by insertion order in `DeclLocations::index`) - a class
/// declaring both a `$bar` property and a `bar()` method will surface
/// only one of them in every search that would otherwise match "bar".
/// This is shared with `navigation::resolve_at`/hover/goto-definition,
/// which discard member-kind information the same way before ever
/// reaching this same map; a real fix means kind-qualifying that map's
/// key and threading a member kind through every existing consumer - a
/// cross-cutting change to already-shipped features, properly scoped as
/// its own separate follow-up rather than folded into this read-only,
/// additive request handler.
pub(super) fn workspace_symbols<'a>(project: &'a Project, query: &str) -> Vec<Match<'a>> {
    let locations = project.decl_locations();
    let registry = project.registry();
    let mut matches: Vec<(u8, Match<'a>)> = Vec::new();

    for (fqn, (path, span)) in &locations.classes {
        let Some(score) = match_score(short_name(fqn), query) else {
            continue;
        };
        // `DeclLocations::classes` carries no `ClassKind` (classes,
        // interfaces, traits, and enums all share this one map) - the
        // registry is the only source for it. `None` here means a
        // decl_locations/registry desync (shouldn't happen, both are
        // populated together) - skipped defensively, not panicked on.
        let Some(info) = registry.classes.get(fqn.as_str()) else {
            continue;
        };
        matches.push((
            score,
            Match {
                name: short_name(fqn).to_string(),
                kind: class_kind_symbol_kind(info.kind),
                path: path.as_path(),
                span: *span,
                container_name: namespace_of(fqn),
            },
        ));
    }

    for (fqn, (path, span)) in &locations.functions {
        let Some(score) = match_score(short_name(fqn), query) else {
            continue;
        };
        matches.push((
            score,
            Match {
                name: short_name(fqn).to_string(),
                kind: SymbolKind::FUNCTION,
                path: path.as_path(),
                span: *span,
                container_name: namespace_of(fqn),
            },
        ));
    }

    for ((owner, name), (path, span)) in &locations.members {
        let Some(score) = match_score(name, query) else {
            continue;
        };
        let Some(kind) = member_symbol_kind(registry, owner, name) else {
            continue;
        };
        matches.push((
            score,
            Match {
                name: name.clone(),
                kind,
                path: path.as_path(),
                span: *span,
                container_name: Some(owner.clone()),
            },
        ));
    }

    matches.sort_by(|(score_a, a), (score_b, b)| {
        score_a
            .cmp(score_b)
            .then_with(|| a.name.len().cmp(&b.name.len()))
            .then_with(|| a.name.cmp(&b.name))
    });
    matches.truncate(MAX_RESULTS);
    matches.into_iter().map(|(_, m)| m).collect()
}

/// Ranks `name` against `query`, ASCII-case-insensitively (matching this
/// codebase's existing PHP-identifier convention, e.g. `hierarchy.rs`'s
/// `eq_ignore_ascii_case` fallback): `0` exact, `1` prefix, `2`
/// substring, `3` subsequence, `None` no match at all. An empty `query`
/// always matches at rank `0`.
fn match_score(name: &str, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(0);
    }
    let name = name.to_ascii_lowercase();
    let query = query.to_ascii_lowercase();
    if name == query {
        Some(0)
    } else if name.starts_with(&query) {
        Some(1)
    } else if name.contains(&query) {
        Some(2)
    } else if is_subsequence(&name, &query) {
        Some(3)
    } else {
        None
    }
}

fn is_subsequence(haystack: &str, needle: &str) -> bool {
    let mut needle_chars = needle.chars();
    let Some(mut want) = needle_chars.next() else {
        return true;
    };
    for c in haystack.chars() {
        if c == want {
            match needle_chars.next() {
                Some(next) => want = next,
                None => return true,
            }
        }
    }
    false
}

/// A class/enum member's `SymbolKind` - `None` on a `decl_locations`/
/// `registry` desync (shouldn't happen, both populated together in
/// `update_file`).
///
/// Checked in the SAME order `DeclLocations::index` inserts into its own
/// flat `(owner, name)` map (methods, then properties, then consts/cases)
/// but REVERSED, since a later insert there silently overwrites an
/// earlier same-named one: the location this function's caller reports
/// for `name` is always whichever declaration inserted last, so the kind
/// reported here must match that same winner, not just "whichever kind
/// happens to exist" in registry declaration order.
fn member_symbol_kind(registry: &Registry, owner: &str, name: &str) -> Option<SymbolKind> {
    let info = registry.classes.get(owner)?;
    if let Some(cst) = info.consts.get(name) {
        if matches!(cst.ty, Type::EnumCase(..)) {
            Some(SymbolKind::ENUM_MEMBER)
        } else {
            Some(SymbolKind::CONSTANT)
        }
    } else if info.properties.contains_key(name) {
        Some(SymbolKind::PROPERTY)
    } else if info.methods.contains_key(name) {
        Some(SymbolKind::METHOD)
    } else {
        None
    }
}

fn short_name(fqn: &str) -> &str {
    fqn.rsplit('\\').next().unwrap_or(fqn)
}

fn namespace_of(fqn: &str) -> Option<String> {
    fqn.rfind('\\').map(|i| fqn[..i].to_string())
}

#[cfg(test)]
mod tests;
