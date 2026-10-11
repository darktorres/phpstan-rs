//! The `@var` tags of a doc comment: each variable's winning type node,
//! chosen by the prefix precedence PHPStan applies to the tag names. Pure
//! over the doc comment's text, so the reflection and the checker both use
//! it.

use crate::grammar::{self, PhpDocTagValue, TypeNode};

/// Every `@var` tag's winning type node, keyed by its variable name
/// (`None` for a bare `@var Type`), in first-appearance order - the same
/// prefix precedence as `tag_type_nodes`.
///
/// Kept by the doc comment's text: a statement's comment is asked for its
/// `@var` tags by four checks, and selecting the winners clones their
/// type nodes.
pub fn var_tag_nodes(doc: &str) -> std::sync::Arc<Vec<(Option<String>, TypeNode)>> {
    type Tags = Vec<(Option<String>, TypeNode)>;
    static MEMO: std::sync::LazyLock<syntax::memo::Memo<Tags>> =
        std::sync::LazyLock::new(|| syntax::memo::Memo::with_limits(1024, 256 * 1024));
    MEMO.get(doc, select_var_tag_nodes)
}

fn select_var_tag_nodes(doc: &str) -> Vec<(Option<String>, TypeNode)> {
    let node = grammar::parse_shared(doc);
    let mut named: Vec<(Option<String>, TypeNode)> = Vec::new();
    // When any tag is unnamed, only the unnamed tags of the last tag kind
    // (`@var` < `@phan-var` < `@psalm-var` < `@phpstan-var`) that has one
    // count; the named tags are dropped.
    let mut unnamed: Vec<(Option<String>, TypeNode)> = Vec::new();
    for prefix in ["", "phan-", "psalm-", "phpstan-"] {
        let mut group = Vec::new();
        for tag in &node.tags {
            let Some(name) = tag.name.strip_prefix(prefix) else {
                continue;
            };
            if name != "var" || (prefix.is_empty() && has_known_prefix(&tag.name)) {
                continue;
            }
            let PhpDocTagValue::TypedVar {
                ty: Some(ty),
                variable,
                ..
            } = &tag.value
            else {
                continue;
            };
            if !tag.errors.is_empty() {
                continue;
            }
            match variable {
                None => group.push((None, ty.clone())),
                Some(_) => match named.iter_mut().find(|(v, _)| v == variable) {
                    Some(slot) => slot.1 = ty.clone(),
                    None => named.push((variable.clone(), ty.clone())),
                },
            }
        }
        if !group.is_empty() {
            unnamed = group;
        }
    }
    if unnamed.is_empty() { named } else { unnamed }
}

pub fn has_known_prefix(name: &str) -> bool {
    ["phan-", "psalm-", "phpstan-"]
        .iter()
        .any(|p| name.starts_with(p))
}
