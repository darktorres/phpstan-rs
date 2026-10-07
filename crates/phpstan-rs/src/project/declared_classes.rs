//! The classes a file declares unconditionally, with what the whole-program
//! checks read of their declarations; made once when the file is parsed, so
//! the checks need not keep its tree.

use syntax::ast::{ClassDecl, NodeArenas, Visibility};
use syntax::span::Span;
use types::Name;

/// A class a file declares unconditionally.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DeclaredClass {
    /// Its fully-qualified name, as the registry has it.
    pub(crate) name: String,
    pub(crate) name_span: Span,
    pub(crate) span: Span,
    /// The classes of its attributes, as resolved or as written.
    pub(crate) attributes: Vec<String>,
    /// The names of its own methods, in source order.
    pub(crate) own_methods: Vec<Name>,
    /// How many of them are public and not the constructor.
    pub(crate) public_methods: usize,
    pub(crate) uses_traits: bool,
}

impl DeclaredClass {
    /// The class `c`, declared by the statement at `span` under `name`.
    pub(crate) fn of(
        arenas: &NodeArenas,
        name: String,
        span: Span,
        c: &ClassDecl,
    ) -> DeclaredClass {
        let Some(def) = arenas.class(c.def) else {
            unreachable!("class declaration ID belongs to the file owner")
        };
        DeclaredClass {
            name,
            name_span: c.name.span,
            span,
            attributes: arenas
                .attributes(def.attrs.clone())
                .iter()
                .map(|a| {
                    a.name
                        .resolved_class()
                        .unwrap_or(&a.name.written)
                        .to_string()
                })
                .collect(),
            own_methods: arenas
                .methods(def.methods.clone())
                .iter()
                .map(|m| m.function.name.name.clone())
                .collect(),
            public_methods: arenas
                .methods(def.methods.clone())
                .iter()
                .filter(|m| m.visibility == Visibility::Public && !m.function.is_constructor())
                .count(),
            uses_traits: !def.trait_uses(arenas).is_empty(),
        }
    }
}

syntax::canonical_struct!(DeclaredClass {
    name,
    name_span,
    span,
    attributes,
    own_methods,
    public_methods,
    uses_traits,
});
