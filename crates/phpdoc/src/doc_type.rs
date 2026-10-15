//! A type written in a doc comment, with the `Type` it denotes.

use std::sync::{Arc, OnceLock};

use types::Type;

use crate::grammar::TypeNode;

/// A type written in a doc comment: the node the grammar read, and, once
/// someone asks, the `Type` it denotes as written (names unresolved,
/// template and alias references not yet marked) and whether it spells
/// out `mixed`. The docblock holding it is read once for each distinct doc
/// text, so each distinct written type is lowered once and not once for
/// each use.
///
/// Cloning one is cloning a pointer: the same written type is shared by
/// every docblock that has it, which is what lets a tag read once be
/// applied to many.
#[derive(Debug, Clone)]
pub struct DocType(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    node: TypeNode,
    lowered: OnceLock<Type>,
}

impl DocType {
    pub fn new(node: TypeNode) -> DocType {
        DocType(Arc::new(Inner {
            node,
            lowered: OnceLock::new(),
        }))
    }

    /// The node the grammar read.
    pub fn node(&self) -> &TypeNode {
        &self.0.node
    }

    /// The node, cloned.
    pub fn into_node(self) -> TypeNode {
        match Arc::try_unwrap(self.0) {
            Ok(inner) => inner.node,
            Err(shared) => shared.node.clone(),
        }
    }

    /// The `Type` the node denotes as written.
    pub fn ty(&self) -> &Type {
        self.0
            .lowered
            .get_or_init(|| types::parse_written_type(&self.0.node.to_source_string()))
    }
}

/// What a written type denotes: by reading it again, for a node alone, and
/// from where it was kept, for a `DocType`.
pub trait WrittenType {
    /// The `Type` the written type denotes as written: names unresolved,
    /// template and alias references not yet marked.
    fn written_type(&self) -> Type;
}

impl WrittenType for TypeNode {
    fn written_type(&self) -> Type {
        types::parse_written_type(&self.to_source_string())
    }
}

impl WrittenType for DocType {
    fn written_type(&self) -> Type {
        self.ty().clone()
    }
}

impl From<TypeNode> for DocType {
    fn from(node: TypeNode) -> DocType {
        DocType::new(node)
    }
}

impl std::ops::Deref for DocType {
    type Target = TypeNode;

    fn deref(&self) -> &TypeNode {
        &self.0.node
    }
}

impl PartialEq for DocType {
    fn eq(&self, other: &DocType) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.node == other.0.node
    }
}

impl serde::Serialize for DocType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.node.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for DocType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<DocType, D::Error> {
        TypeNode::deserialize(deserializer).map(DocType::new)
    }
}
