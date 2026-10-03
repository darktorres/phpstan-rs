//! What one parse of a file finds in its declarations, in one walk of
//! them: the names it declares, the registry entries they build, and its
//! sites among the declarations of a name that several files make.
//!
//! These were five walks of the same tree (`declared_names`,
//! `declared_constants`, `build_declarations`, and the two duplicate-site
//! indexes), each visiting every expression of every body to find the
//! `define()` calls and the declarations nested in expressions, and each
//! making the same name of a declaration again.

use std::path::Path;

use syntax::ast::Stmt;
use syntax::declarations::{Declared, for_each_declaration};
use syntax::name::Name;

use super::declared_classes::DeclaredClass;
use super::duplicate_class_sites::DuplicateClassSites;
use super::duplicate_fn_sites::DuplicateFunctionSites;
use crate::registry::{BuiltDecl, Origin, anonymous_class_decls, built_declaration};

/// `collect`'s result.
pub(super) struct FileDeclarations {
    pub(super) functions: Vec<Name>,
    pub(super) classes: Vec<Name>,
    /// The global constants (`const`, `define()`), as
    /// `Registry::global_constants` holds them.
    pub(super) constants: Vec<Name>,
    /// In walk order, then the anonymous classes, as `build_declarations`
    /// returns them.
    pub(super) built_decls: Vec<BuiltDecl>,
    pub(super) duplicate_fn_sites: DuplicateFunctionSites,
    pub(super) duplicate_class_sites: DuplicateClassSites,
    /// The classes it declares unconditionally.
    pub(super) declared_classes: Vec<DeclaredClass>,
}

/// The declarations of `ast`, the file at `path`. `is_stub` is whether the
/// file falls under a configured stub path. `has_anonymous_class` is
/// whether it has an anonymous class, which is all its walk for them
/// (`anonymous_class_decls`) can find.
pub(super) fn collect(
    ast: &[Stmt],
    path: &Path,
    is_stub: bool,
    origin: &Origin,
    has_anonymous_class: bool,
) -> FileDeclarations {
    let mut found = FileDeclarations {
        functions: Vec::new(),
        classes: Vec::new(),
        constants: Vec::new(),
        built_decls: Vec::new(),
        duplicate_fn_sites: DuplicateFunctionSites::default(),
        duplicate_class_sites: DuplicateClassSites::default(),
        declared_classes: Vec::new(),
    };
    for_each_declaration(ast, |declaration, ctx, guards| {
        let name = declaration.name();
        match declaration.declared {
            Declared::Function(f) => {
                if !declaration.conditional {
                    found.duplicate_fn_sites.record(name.clone(), path, f);
                }
                found.functions.push(Name::from(name));
            }
            Declared::Class(c) => {
                if !declaration.conditional {
                    found.declared_classes.push(DeclaredClass::of(
                        name.clone(),
                        declaration.span,
                        c,
                    ));
                    found.duplicate_class_sites.record(
                        name.clone(),
                        path,
                        c.name.span,
                        c.def.kind,
                        is_stub,
                    );
                }
                found.classes.push(Name::from(name));
            }
            Declared::Constant(_) | Declared::Define { .. } => {
                found
                    .constants
                    .push(Name::from(name.trim_start_matches('\\')));
            }
        }
        built_declaration(
            declaration,
            ctx,
            guards,
            origin,
            None,
            &mut found.built_decls,
        );
    });
    if has_anonymous_class {
        found.built_decls.extend(anonymous_class_decls(ast, origin));
    }
    found
}
