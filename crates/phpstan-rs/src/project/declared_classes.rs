//! The classes a file declares unconditionally, with what the whole-program
//! checks read of their declarations; made once when the file is parsed, so
//! the checks need not keep its tree.

#[cfg(debug_assertions)]
use syntax::ast::Stmt;
use syntax::ast::{ClassDecl, Visibility};
#[cfg(debug_assertions)]
use syntax::declarations::{Declared, for_each_declaration};
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
    pub(crate) fn of(name: String, span: Span, c: &ClassDecl) -> DeclaredClass {
        DeclaredClass {
            name,
            name_span: c.name.span,
            span,
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
        }
    }
}

/// The classes `program` declares unconditionally, by a walk of its own, which
/// the debug build compares with the classes found alongside the declarations.
#[cfg(debug_assertions)]
pub(crate) fn declared_classes(program: &[Stmt]) -> Vec<DeclaredClass> {
    let mut classes = Vec::new();
    for_each_declaration(program, |declaration, _, _| {
        if let Declared::Class(c) = declaration.declared
            && !declaration.conditional
        {
            classes.push(DeclaredClass::of(declaration.name(), declaration.span, c));
        }
    });
    classes
}
