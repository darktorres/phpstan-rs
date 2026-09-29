//! Resolution of a parsed PHPDoc type to the `Type` it denotes.

use phpdoc::grammar::TypeNode;
use types::Type;

/// The `Type` a PHPDoc type denotes as written: names still unresolved,
/// template and alias references not yet marked.
pub(crate) fn parse_type_node(node: &TypeNode) -> Type {
    types::parse_type_string(&node.to_source_string())
}

/// Whether a PHPDoc type spells out `mixed` anywhere, nested included.
pub(crate) fn mentions_mixed(node: &TypeNode) -> bool {
    types::text_mentions_mixed(&node.to_source_string())
}
