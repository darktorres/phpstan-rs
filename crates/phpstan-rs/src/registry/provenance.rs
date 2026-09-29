//! Where a declaration comes from: the file that declares it and the span
//! of its name there, recorded for every class, function and member as it
//! is indexed.

use std::path::Path;
use std::sync::Arc;
use syntax::name::Name;

use syntax::token::Span;

use crate::registry::Registry;

/// The source a declaration was indexed from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum Origin {
    /// Not recorded: a registry built from bare source.
    #[default]
    Unknown,
    /// A project file, analysed or loaded for context.
    File(Arc<Path>),
    /// A phpstorm-stubs file (its path below the stubs root): a PHP
    /// built-in, of the extension named by its directory.
    Builtin {
        file: &'static str,
        extension: &'static str,
    },
    /// PHPStan's own source, embedded (its path below `src/`).
    Phpstan { file: &'static str },
}

/// A declaration's source, the span of its name there, and for a member,
/// the class-like that declares it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Provenance {
    origin: Origin,
    span: Span,
    declaring_class: Option<Name>,
}

impl Provenance {
    pub fn new(origin: Origin, span: Span) -> Provenance {
        Provenance {
            origin,
            span,
            declaring_class: None,
        }
    }

    /// A member's, declared by `class`.
    pub fn member(origin: Origin, span: Span, class: Name) -> Provenance {
        Provenance {
            origin,
            span,
            declaring_class: Some(class),
        }
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The project file declaring it; `None` for a built-in, PHPStan's
    /// own source, or an unrecorded origin.
    pub fn file(&self) -> Option<&Path> {
        match &self.origin {
            Origin::File(path) => Some(path),
            _ => None,
        }
    }

    /// Whether PHP itself provides it.
    pub fn is_builtin(&self) -> bool {
        matches!(self.origin, Origin::Builtin { .. })
    }

    /// The PHP extension providing a built-in.
    pub fn extension_name(&self) -> Option<&'static str> {
        match self.origin {
            Origin::Builtin { extension, .. } => Some(extension),
            _ => None,
        }
    }

    /// The span of the declaration's name in its file.
    pub fn declaration_span(&self) -> Span {
        self.span
    }

    /// For a member, the class-like declaring it (a trait for one a class
    /// takes from it), as its declaration spells it.
    pub fn declaring_class(&self) -> Option<&str> {
        self.declaring_class.as_deref()
    }

    /// The project file declaring it, and its name's span there.
    fn in_file(&self) -> Option<(&Path, Span)> {
        self.file().map(|file| (file, self.span))
    }
}

impl Registry {
    /// Where a project file declares class-like `fqn`.
    pub fn class_declaration(&self, fqn: &str) -> Option<(&Path, Span)> {
        self.classes.get(fqn)?.provenance.in_file()
    }

    /// Where a project file declares function `fqn`.
    pub fn function_declaration(&self, fqn: &str) -> Option<(&Path, Span)> {
        self.functions.get(fqn)?.provenance.in_file()
    }

    /// Where a project file declares `owner`'s method `name`.
    pub fn method_declaration(&self, owner: &str, name: &str) -> Option<(&Path, Span)> {
        self.classes
            .get(owner)?
            .methods
            .get(name)?
            .provenance
            .in_file()
    }

    /// Where a project file declares `owner`'s property `name`.
    pub fn property_declaration(&self, owner: &str, name: &str) -> Option<(&Path, Span)> {
        self.classes
            .get(owner)?
            .properties
            .get(name)?
            .provenance
            .in_file()
    }

    /// Where a project file declares `owner`'s constant or enum case `name`.
    pub fn constant_declaration(&self, owner: &str, name: &str) -> Option<(&Path, Span)> {
        self.classes
            .get(owner)?
            .consts
            .get(name)?
            .provenance
            .in_file()
    }

    /// Where a project file declares `owner`'s member `name` of unknown
    /// kind: a constant or enum case, else a property, else a method.
    pub fn member_declaration(&self, owner: &str, name: &str) -> Option<(&Path, Span)> {
        self.constant_declaration(owner, name)
            .or_else(|| self.property_declaration(owner, name))
            .or_else(|| self.method_declaration(owner, name))
    }
}
