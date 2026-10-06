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

use syntax::FileFacts;
use syntax::ast::Stmt;
use syntax::declarations::{Declared, for_each_declaration_in};
use syntax::name::Name;

use super::declared_classes::DeclaredClass;
use super::duplicate_class_sites::DuplicateClassSites;
use super::duplicate_fn_sites::DuplicateFunctionSites;
use crate::registry::{
    BuiltDecl, FileDecls, FileMember, Origin, anonymous_class_decls, built_declaration,
};

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
    /// File-grouped declaration references, used by the id store as it
    /// moves off per-declaration name-table values.
    pub(super) file_decls: FileDecls,
    pub(super) duplicate_fn_sites: DuplicateFunctionSites,
    pub(super) duplicate_class_sites: DuplicateClassSites,
    /// The classes it declares unconditionally.
    pub(super) declared_classes: Vec<DeclaredClass>,
}

/// The bit of `FileFacts::mentions` a file naming `define`, in any case,
/// however it imports it, has: the only call that declares a constant.
pub(crate) const MENTIONS_DEFINE: u32 = 1 << 3;

/// The spellings that set `MENTIONS_DEFINE`.
pub(crate) const WATCH: &[(&str, u32)] = &[("define", MENTIONS_DEFINE)];

/// Whether the declarations of a file can be inside its expressions: a
/// `define()` call, or a declaration in the body of a closure or an
/// anonymous class. If not, the walk for them leaves its expressions
/// alone.
pub(crate) fn declares_in_expressions(facts: &FileFacts) -> bool {
    facts.mentions & MENTIONS_DEFINE != 0 || facts.has_nested_declaration
}

/// The declarations of `ast`, the file at `path`. `is_stub` is whether the
/// file falls under a configured stub path. The facts of the file say
/// whether it has an anonymous class, which is all its walk for them
/// (`anonymous_class_decls`) can find, and whether its declarations can be
/// inside expressions.
pub(super) fn collect(
    ast: &[Stmt],
    path: &Path,
    is_stub: bool,
    origin: &Origin,
    facts: &FileFacts,
) -> FileDeclarations {
    let has_anonymous_class = facts.has_anonymous_class;
    let in_expressions = declares_in_expressions(facts);
    let mut found = FileDeclarations {
        functions: Vec::new(),
        classes: Vec::new(),
        constants: Vec::new(),
        built_decls: Vec::new(),
        file_decls: FileDecls::new(),
        duplicate_fn_sites: DuplicateFunctionSites::default(),
        duplicate_class_sites: DuplicateClassSites::default(),
        declared_classes: Vec::new(),
    };
    for_each_declaration_in(ast, in_expressions, |declaration, ctx, guards| {
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
        built_declaration(declaration, ctx, guards, origin, &mut found.built_decls);
    });
    if has_anonymous_class {
        found.built_decls.extend(anonymous_class_decls(ast, origin));
    }
    for decl in &found.built_decls {
        match decl {
            BuiltDecl::Class { info, .. } => {
                let members = info.methods().iter().map(|(name, declaration)| FileMember {
                    name: name.clone(),
                    declaration: declaration.clone(),
                });
                found.file_decls.add_class(
                    std::sync::Arc::clone(info),
                    members,
                    info.properties().values().map(std::sync::Arc::clone),
                    info.consts().values().map(std::sync::Arc::clone),
                    info.extends()
                        .iter()
                        .chain(info.implements())
                        .chain(info.uses())
                        .cloned(),
                );
            }
            BuiltDecl::Function { sig, .. } => {
                found
                    .file_decls
                    .add_function(std::sync::Arc::clone(sig), sig.params().iter().cloned());
            }
            BuiltDecl::GlobalConst { .. } => {}
        }
    }
    found
}
