//! `RequireFileExistsRule`: an `include`/`require` (and `_once`) whose
//! path resolves to known strings, none of which name an existing file.
//! The strings come from `include_values`, once for each world a path is
//! read in, and are looked up where a run can find them (`include_lookup`).
//! A path the file also tests with `file_exists()`/`is_file()`/
//! `is_readable()` is left alone.

use rustc_hash::FxHashSet;
use std::path::Path;

use crate::checker::include_lookup::World;
use crate::checker::include_values::{IncludeCall, IncludeCalls};
use crate::checker::{Checker, Locals};
use syntax::ast::*;
use syntax::span::Span;
use syntax::visit::walk_stmts;

/// `(construct as written, identifier prefix)` for an include kind.
pub(crate) fn include_construct(kind: IncludeKind) -> (&'static str, &'static str) {
    let prefix = match kind {
        IncludeKind::Require => "require",
        IncludeKind::RequireOnce => "requireOnce",
        IncludeKind::Include => "include",
        IncludeKind::IncludeOnce => "includeOnce",
    };
    (kind.as_str(), prefix)
}

impl<'a> Checker<'a> {
    /// Whether the file tests `path` with `GUARD_FUNCTIONS`.
    fn is_guarded(&self, path: &str) -> bool {
        self.include_guarded_paths
            .contains(&*self.settings.path_case.key(path))
    }

    pub(crate) fn check_include_path(
        &mut self,
        (construct, identifier_prefix): (&'static str, &'static str),
        path_expr: &Expr,
        locals: &Locals,
        span: Span,
    ) {
        if self.current_file.is_none() {
            return;
        }
        let Some(env) = self.include_env else {
            return;
        };
        let Some([unix, windows]) = self.include_values(path_expr, locals) else {
            return;
        };
        let includer_dir = self.current_dir_of_file();
        let place = env.place(includer_dir.as_deref());
        let is_file = |path: &Path| path.is_file();
        for (unix_text, windows_text) in unix.iter().zip(&windows) {
            let values = [(World::Unix, unix_text), (World::Windows, windows_text)];
            if values.iter().any(|&(_, text)| self.is_guarded(text))
                || values
                    .iter()
                    .any(|&(world, text)| place.is_found(world, text, &is_file))
            {
                continue;
            }
            self.report(
                format!(
                    "Path in {construct}() \"{unix_text}\" is not a file or it does not exist."
                ),
                span,
                0,
                format!("{identifier_prefix}.fileNotFound"),
            );
        }
    }

    /// Every path `program` tests with a `GUARD_FUNCTIONS` call, in each
    /// world.
    pub(crate) fn collect_include_guards(&self, program: &[Stmt]) -> FxHashSet<String> {
        let locals = self.new_locals();
        let mut guarded = FxHashSet::default();
        let mut on_call = |call: IncludeCall| {
            let IncludeCall::Guard(arg) = call else {
                return;
            };
            for world in World::ALL {
                if let Some(paths) = self.resolve_include_paths(arg, &locals, world) {
                    // Keyed as `is_guarded` looks them up: a guard names the
                    // file whatever the case of its letters where the root's
                    // directory folds case.
                    guarded.extend(
                        paths
                            .iter()
                            .map(|path| self.settings.path_case.key(path).into_owned()),
                    );
                }
            }
        };
        walk_stmts(&mut IncludeCalls(&mut on_call), program);
        guarded
    }
}
