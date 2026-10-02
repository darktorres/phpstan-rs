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

use super::duplicate_class_sites::DuplicateClassSites;
use super::duplicate_fn_sites::DuplicateFunctionSites;
use crate::registry::{BuiltDecl, Origin, anonymous_class_decls, built_declaration};

/// `collect`'s result.
pub(super) struct FileDeclarations {
    pub(super) functions: Vec<String>,
    pub(super) classes: Vec<String>,
    /// The global constants (`const`, `define()`), as
    /// `Registry::global_constants` holds them.
    pub(super) constants: Vec<String>,
    /// In walk order, then the anonymous classes, as `build_declarations`
    /// returns them.
    pub(super) built_decls: Vec<BuiltDecl>,
    pub(super) duplicate_fn_sites: DuplicateFunctionSites,
    pub(super) duplicate_class_sites: DuplicateClassSites,
}

/// The declarations of `ast`, the file at `path`. `is_stub` is whether the
/// file falls under a configured stub path.
pub(super) fn collect(
    ast: &[Stmt],
    path: &Path,
    is_stub: bool,
    origin: &Origin,
) -> FileDeclarations {
    let mut found = FileDeclarations {
        functions: Vec::new(),
        classes: Vec::new(),
        constants: Vec::new(),
        built_decls: Vec::new(),
        duplicate_fn_sites: DuplicateFunctionSites::default(),
        duplicate_class_sites: DuplicateClassSites::default(),
    };
    for_each_declaration(ast, |declaration, ctx| {
        let name = declaration.name();
        match declaration.declared {
            Declared::Function(f) => {
                if !declaration.conditional {
                    found.duplicate_fn_sites.record(name.clone(), path, f);
                }
                found.functions.push(name);
            }
            Declared::Class(c) => {
                if !declaration.conditional {
                    found.duplicate_class_sites.record(
                        name.clone(),
                        path,
                        c.name.span,
                        c.def.kind,
                        is_stub,
                    );
                }
                found.classes.push(name);
            }
            Declared::Constant(_) | Declared::Define { .. } => {
                found
                    .constants
                    .push(name.trim_start_matches('\\').to_string());
            }
        }
        built_declaration(declaration, ctx, origin, None, &mut found.built_decls);
    });
    found.built_decls.extend(anonymous_class_decls(ast, origin));
    found
}
