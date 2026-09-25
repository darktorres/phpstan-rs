use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use crate::ast::{Stmt, StmtKind};
use crate::name_resolution::NameContext;
use crate::token::Span;

/// A symbol's own declaration site, name-keyed the same way `Registry`
/// is, but carrying `(PathBuf, Span)` instead of type
/// information - `Registry` itself has no file-path concept at all (used
/// in many path-agnostic contexts: stubs, tests, `demo`), so this lives
/// here, alongside `Project`'s own per-file bookkeeping, instead.
/// `members` is keyed by `(owner FQN, member name)` - the same bare
/// method/property/const name `Registry::lookup_method`/`lookup_property`/
/// `lookup_const` already key by (no leading `$` on a property, matching
/// `PropertyDecl.name`'s own convention - confirmed against `registry/
/// class_info.rs`'s own `p.name.clone()` map-insertion, not assumed).
#[derive(Default)]
pub(crate) struct DeclLocations {
    pub(crate) classes: FxHashMap<String, (PathBuf, Span)>,
    pub(crate) functions: FxHashMap<String, (PathBuf, Span)>,
    pub(crate) members: FxHashMap<(String, String), (PathBuf, Span)>,
}

impl DeclLocations {
    /// Drops every entry this exact file contributed - mirrors
    /// `Registry::remove_declarations`'s own by-name removal, using the
    /// identical `old.functions`/`old.classes` FQN lists `update_file`
    /// already collects for that call, so the two never disagree about
    /// what a file "declared." A `members` entry is removed whenever its
    /// owner class is in `removed_classes` - a class being reloaded
    /// always gets its members' locations fully repopulated by the
    /// caller's own follow-up `index` call, so a stale-but-not-yet-
    /// replaced gap here is only ever momentary.
    pub(crate) fn remove_declarations(
        &mut self,
        removed_functions: &[String],
        removed_classes: &[String],
    ) {
        for f in removed_functions {
            self.functions.remove(f);
        }
        for c in removed_classes {
            self.classes.remove(c);
        }
        self.members
            .retain(|(owner, _), _| !removed_classes.iter().any(|c| c == owner));
    }

    /// Indexes every class/function/method/property/const declaration
    /// `stmts` contains, exactly mirroring `declared_names_with_context`'s
    /// own namespace/`use`/`if`/`Block` scoping (structured after it, not
    /// sharing code with it, since the two serve different consumers).
    pub(crate) fn index(&mut self, stmts: &[Stmt], ctx: &mut NameContext, path: &Path) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Namespace(name) => ctx.set_namespace(name),
                StmtKind::Use(imports) => ctx.add_uses(imports),
                StmtKind::FunctionDecl(f) => {
                    self.functions.insert(
                        ctx.declared_name(&f.name),
                        (path.to_path_buf(), f.name_span),
                    );
                }
                StmtKind::ClassDecl(c) => {
                    let fqn = ctx.declared_name(&c.name);
                    self.classes
                        .insert(fqn.clone(), (path.to_path_buf(), c.name_span));
                    for m in &c.methods {
                        self.members.insert(
                            (fqn.clone(), m.function.name.to_string()),
                            (path.to_path_buf(), m.function.name_span),
                        );
                    }
                    for p in &c.properties {
                        self.members.insert(
                            (fqn.clone(), p.name.to_string()),
                            (path.to_path_buf(), p.name_span),
                        );
                    }
                    for cst in &c.consts {
                        self.members.insert(
                            (fqn.clone(), cst.name.to_string()),
                            (path.to_path_buf(), cst.name_span),
                        );
                    }
                }
                StmtKind::EnumDecl(e) => {
                    let fqn = ctx.declared_name(&e.name);
                    self.classes
                        .insert(fqn.clone(), (path.to_path_buf(), e.name_span));
                    for m in &e.methods {
                        self.members.insert(
                            (fqn.clone(), m.function.name.to_string()),
                            (path.to_path_buf(), m.function.name_span),
                        );
                    }
                    for cst in &e.consts {
                        self.members.insert(
                            (fqn.clone(), cst.name.to_string()),
                            (path.to_path_buf(), cst.name_span),
                        );
                    }
                    for case in &e.cases {
                        self.members.insert(
                            (fqn.clone(), case.name.to_string()),
                            (path.to_path_buf(), case.name_span),
                        );
                    }
                }
                StmtKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.index(std::slice::from_ref(then_branch), &mut ctx.clone(), path);
                    if let Some(else_branch) = else_branch {
                        self.index(std::slice::from_ref(else_branch), &mut ctx.clone(), path);
                    }
                }
                StmtKind::Block(inner) => self.index(inner, ctx, path),
                _ => {}
            }
        }
    }
}
