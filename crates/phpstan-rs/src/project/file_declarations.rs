//! What one parse of a file finds in its declarations, in one walk of
//! them: the names it declares, the registry entries they build, and its
//! sites among the declarations of a name that several files make.
//!
//! These were five walks of the same tree (`declared_names`,
//! `declared_constants`, `build_declarations`, and the two duplicate-site
//! indexes), each visiting every expression of every body to find the
//! `define()` calls and the declarations nested in expressions, and each
//! making the same name of a declaration again.

use syntax::FileFacts;
use syntax::ast::Stmt;
use syntax::declarations::{Declared, for_each_declaration_with};
use syntax::name::Name;

use super::declared_classes::DeclaredClass;
use super::duplicate_class_sites::ClassDeclSite;
use super::duplicate_fn_sites::FnDeclSite;
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
    /// Each unconditional function declaration's name and site, in walk order.
    pub(super) duplicate_fn_sites: Vec<(String, FnDeclSite)>,
    /// Each unconditional class-like declaration's name and site, in walk order.
    pub(super) duplicate_class_sites: Vec<(String, ClassDeclSite)>,
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

/// The declarations of `ast`. `is_stub` is whether the file falls under a
/// configured stub path. `facts` limits the walk: expressions are entered only
/// where the file can declare in one, and bodies only where they can hold a
/// declaration.
pub(super) fn collect(
    ast: &[Stmt],
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
        duplicate_fn_sites: Vec::new(),
        duplicate_class_sites: Vec::new(),
        declared_classes: Vec::new(),
    };
    // A body is entered only where it can hold a declaration.
    let in_bodies = in_expressions || facts.has_declaration_in_body;
    for_each_declaration_with(
        ast,
        in_expressions,
        in_bodies,
        |declaration, ctx, guards| {
            let name = declaration.name();
            match declaration.declared {
                Declared::Function(f) => {
                    if !declaration.conditional {
                        found
                            .duplicate_fn_sites
                            .push((name.clone(), FnDeclSite::of(f)));
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
                        found.duplicate_class_sites.push((
                            name.clone(),
                            ClassDeclSite {
                                span: c.name.span,
                                kind: c.def.kind,
                                is_stub,
                            },
                        ));
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
        },
    );
    if has_anonymous_class {
        found.built_decls.extend(anonymous_class_decls(ast, origin));
    }
    // What a walk of the expressions too finds.
    #[cfg(debug_assertions)]
    if !in_expressions {
        let mut declared = 0;
        syntax::declarations::for_each_declaration(ast, |_, _, _| declared += 1);
        assert_eq!(
            declared,
            found.functions.len() + found.classes.len() + found.constants.len(),
            "a file whose facts say it declares nothing in an expression does"
        );
    }
    found
}
