use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::ast::{ClassKind, Stmt, StmtKind};
use crate::registry::NameContext;
use crate::token::Span;

/// One top-level class-like (`class`/`interface`/`trait`/`enum`)
/// declaration's own site, name-keyed the same way `Registry`/
/// `DeclLocations` are - mirrors `project::duplicate_fn_sites::FnDeclSite`
/// exactly, with one new field this side needs and the function side
/// doesn't: `is_stub`, whether this site's own file falls under a
/// configured `stub_paths` entry (see `project::duplicate_classes`'s own
/// doc comment for why that distinction exists at all - a designated
/// override for a real corpus-vendored stub file, e.g. `Imagick.stub.
/// php`, must never itself be flagged, and must never count toward
/// flagging the real declaration it's overriding).
pub(crate) struct ClassDeclSite {
    pub(crate) path: PathBuf,
    pub(crate) span: Span,
    pub(crate) kind: ClassKind,
    pub(crate) is_stub: bool,
}

/// Every top-level class-like's own unconditional declaration site(s),
/// across the whole project - a straight port of `DuplicateFunctionSites`
/// to class-likes, plus the `is_stub` field above. Gated behind
/// `Registry::check_duplicate_class_declarations` (opt-in, unlike
/// `DuplicateFunctionSites`, which has no such gate - see that field's own
/// doc comment for why this one needs it and the function-side precedent
/// didn't).
#[derive(Default)]
pub(crate) struct DuplicateClassSites {
    sites: FxHashMap<String, Vec<ClassDeclSite>>,
}

impl DuplicateClassSites {
    /// `conditional` mirrors `DuplicateFunctionSites::index`'s own
    /// identical parameter and `If`/`Block` recursion shape exactly - a
    /// conditionally-reached class-like declaration is dropped entirely,
    /// same false-negative-favoring rationale (a real, if rarer, analogue
    /// of the `function_exists`-guarded polyfill idiom: `if (!class_
    /// exists(Foo::class)) { class Foo {} }`). `is_stub` is this call's
    /// own file-level provenance (every recursive call keeps whatever
    /// its caller passed - a stub file's own conditional class-like
    /// declarations are still dropped by the `conditional` check above,
    /// consistent with the unconditional-only semantics this whole
    /// index shares with the function-side precedent).
    pub(crate) fn index(
        &mut self,
        stmts: &[Stmt],
        ctx: &mut NameContext,
        path: &Path,
        conditional: bool,
        is_stub: bool,
    ) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Namespace(name) => ctx.set_namespace(name),
                StmtKind::Use(imports) => ctx.add_uses(imports),
                StmtKind::ClassDecl(c) => {
                    if conditional {
                        continue;
                    }
                    self.record(
                        ctx.declared_name(&c.name),
                        path,
                        c.name_span,
                        c.kind,
                        is_stub,
                    );
                }
                StmtKind::EnumDecl(e) => {
                    if conditional {
                        continue;
                    }
                    self.record(
                        ctx.declared_name(&e.name),
                        path,
                        e.name_span,
                        ClassKind::Enum,
                        is_stub,
                    );
                }
                StmtKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.index(
                        std::slice::from_ref(then_branch),
                        &mut ctx.clone(),
                        path,
                        true,
                        is_stub,
                    );
                    if let Some(else_branch) = else_branch {
                        self.index(
                            std::slice::from_ref(else_branch),
                            &mut ctx.clone(),
                            path,
                            true,
                            is_stub,
                        );
                    }
                }
                StmtKind::Block(inner) => self.index(inner, ctx, path, conditional, is_stub),
                _ => {}
            }
        }
    }

    fn record(&mut self, fqn: String, path: &Path, span: Span, kind: ClassKind, is_stub: bool) {
        self.sites.entry(fqn).or_default().push(ClassDeclSite {
            path: path.to_path_buf(),
            span,
            kind,
            is_stub,
        });
    }

    /// Mirrors `DuplicateFunctionSites::remove_declarations`'s own shape
    /// exactly.
    pub(crate) fn remove_declarations(&mut self, path: &Path, removed_classes: &[String]) {
        for name in removed_classes {
            if let Some(sites) = self.sites.get_mut(name) {
                sites.retain(|s| s.path != path);
                if sites.is_empty() {
                    self.sites.remove(name);
                }
            }
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &Vec<ClassDeclSite>)> {
        self.sites.iter()
    }

    /// Mirrors `DuplicateFunctionSites::merge`'s own doc comment/shape
    /// exactly.
    pub(crate) fn merge(&mut self, other: DuplicateClassSites) {
        for (name, sites) in other.sites {
            self.sites.entry(name).or_default().extend(sites);
        }
    }
}
