//! The classes a file declares unconditionally, with what the whole-program
//! checks read of their declarations; made once when the file is parsed, so
//! the checks need not keep its tree.

use syntax::ast::{Stmt, Visibility};
use syntax::declarations::{Declared, for_each_declaration};
use syntax::span::Span;
use types::Name;

/// A class a file declares unconditionally.
#[derive(Debug, Clone)]
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

/// The classes `program` declares unconditionally.
pub(crate) fn declared_classes(program: &[Stmt]) -> Vec<DeclaredClass> {
    let mut classes = Vec::new();
    for_each_declaration(program, |declaration, _, _| {
        if let Declared::Class(c) = declaration.declared
            && !declaration.conditional
        {
            classes.push(DeclaredClass {
                name: declaration.name(),
                name_span: c.name.span,
                span: declaration.span,
                attributes: c
                    .def
                    .attrs
                    .iter()
                    .map(|a| {
                        a.name
                            .resolved_class()
                            .unwrap_or(&a.name.written)
                            .to_string()
                    })
                    .collect(),
                own_methods: c
                    .def
                    .methods
                    .iter()
                    .map(|m| m.function.name.name.clone())
                    .collect(),
                public_methods: c
                    .def
                    .methods
                    .iter()
                    .filter(|m| m.visibility == Visibility::Public && !m.function.is_constructor())
                    .count(),
                uses_traits: !c.def.trait_uses.is_empty(),
            });
        }
    });
    classes
}
