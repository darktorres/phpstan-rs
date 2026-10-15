//! Resolution of a parsed PHPDoc type to the `Type` it denotes.

use phpdoc::WrittenType;
use types::Type;

/// The `Type` a PHPDoc type denotes as written: names still unresolved,
/// template and alias references not yet marked.
pub(crate) fn parse_type_node<T: WrittenType + ?Sized>(node: &T) -> Type {
    node.written_type()
}
