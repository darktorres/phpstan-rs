//! `workspace/symbol`: project-wide fuzzy symbol search over the
//! declarations the registry records for project files - classes/enums/
//! interfaces/traits, top-level functions, and class/enum members
//! (methods, properties, consts, enum cases), each at its own name's span.

use std::path::Path;

use lsp_types::SymbolKind;

use crate::project::Project;
use crate::registry::Provenance;
use syntax::span::Span;

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
/// in different namespaces that each declare a same-named member. A
/// member is listed under the class-like declaring it only, not under
/// every class inheriting it or taking it from a trait.
pub(super) fn workspace_symbols<'a>(project: &'a Project, query: &str) -> Vec<Match<'a>> {
    let registry = project.registry();
    let mut matches: Vec<(u8, Match<'a>)> = Vec::new();
    let mut push = |name: &str, kind: SymbolKind, provenance: &'a Provenance, container| {
        let Some(path) = provenance.file() else {
            return;
        };
        let Some(score) = match_score(name, query) else {
            return;
        };
        matches.push((
            score,
            Match {
                name: name.to_string(),
                kind,
                path,
                span: provenance.declaration_span(),
                container_name: container,
            },
        ));
    };

    for (fqn, info) in registry.classes() {
        push(
            short_name(fqn),
            class_kind_symbol_kind(info.kind()),
            info.provenance(),
            namespace_of(fqn),
        );
        let declared_here = |provenance: &Provenance| {
            provenance
                .declaring_class()
                .is_some_and(|c| c.eq_ignore_ascii_case(fqn))
        };
        for (name, sig) in info.methods().iter() {
            if declared_here(sig.provenance()) {
                push(
                    name,
                    SymbolKind::METHOD,
                    sig.provenance(),
                    Some(fqn.to_string()),
                );
            }
        }
        for (name, prop) in info.properties() {
            if declared_here(prop.provenance()) {
                push(
                    name,
                    SymbolKind::PROPERTY,
                    prop.provenance(),
                    Some(fqn.to_string()),
                );
            }
        }
        for (name, cst) in info.consts() {
            if declared_here(cst.provenance()) {
                let kind = if cst.ty().is_enum_case() {
                    SymbolKind::ENUM_MEMBER
                } else {
                    SymbolKind::CONSTANT
                };
                push(name, kind, cst.provenance(), Some(fqn.to_string()));
            }
        }
    }

    for (fqn, sig) in registry.functions().iter() {
        push(
            short_name(fqn),
            SymbolKind::FUNCTION,
            sig.provenance(),
            namespace_of(fqn),
        );
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

fn short_name(fqn: &str) -> &str {
    fqn.rsplit('\\').next().unwrap_or(fqn)
}

fn namespace_of(fqn: &str) -> Option<String> {
    fqn.rfind('\\').map(|i| fqn[..i].to_string())
}

#[cfg(test)]
mod tests;
