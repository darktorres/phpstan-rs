//! What a closure's captures by reference hold. A captured variable is one
//! storage with the enclosing scope's: each time the closure runs, it holds
//! what the scope, and the closure's own earlier runs, left in it. The
//! closure's body is entered with that, and the scope holds afterwards what
//! the closure may leave, from every place its body returns or ends.

use syntax::ast::ClosureUse;
use syntax::name::{Name, NameMap};
use syntax::span::Span;
use types::Type;
use types::UnionAll as _;

use crate::checker::{Checker, Locals};

impl Checker<'_> {
    /// What a closure made at `at` is entered with in the variable `name` it
    /// captures by reference, which holds `outer` where it is made: that,
    /// what the scope holds in it later, as a pass over the scope found
    /// (`check_capturing_scope`).
    pub(crate) fn capture_entry(&self, at: Span, name: &Name, outer: Type) -> Type {
        match self.capture_widening.get(&(at.start, name.clone())) {
            Some(wider) => self.relation().union_of(vec![outer, wider.clone()]),
            None => outer,
        }
    }

    /// Adds to `pass_locals`, a pass of a closure's body as it ends, what
    /// each of `names`, captured by reference, held where the body returned
    /// (`current_function_by_ref_exit_snapshots`): the closure's next run
    /// starts from those as much as from its end.
    pub(crate) fn fold_returned_captures(&self, names: &[Name], pass_locals: &mut Locals) {
        for name in names {
            let Some(returned) = self.current_function_by_ref_exit_snapshots.get(name) else {
                continue;
            };
            let held = returned
                .iter()
                .cloned()
                .chain(pass_locals.get_var(name).cloned())
                .union_all_with(&self.relation())
                .unwrap_or_else(Type::mixed);
            pass_locals.insert_var(name, held);
        }
    }

    /// The enclosing scope's `locals` after a closure made at `at`: each
    /// variable of `uses` captured by reference holds what the closure may
    /// leave in it (`snapshots`, taken where its body returns and ends, or
    /// its end state `closure_locals`). Records, for the scope's later
    /// statements, what the closure was entered with (`entries`) and what it
    /// may write.
    pub(crate) fn write_back_captures(
        &mut self,
        at: Span,
        uses: &[ClosureUse],
        entries: &[(Name, Type)],
        snapshots: &NameMap<Vec<Type>>,
        closure_locals: &Locals,
        locals: &mut Locals,
    ) {
        for u in uses.iter().filter(|u| u.by_ref) {
            let left = snapshots
                .get(&u.name.name)
                .and_then(|snapshots| snapshots.iter().cloned().union_all_with(&self.relation()))
                .unwrap_or_else(|| {
                    closure_locals
                        .get_var(&u.name)
                        .cloned()
                        .unwrap_or(Type::mixed())
                });
            if !locals.contains_var(&u.name) {
                // PHP makes `$x` in the enclosing scope the moment
                // `use (&$x)` captures it undefined, as a by-reference
                // argument does: its declaration is the capture.
                locals.declare_var(&u.name, left.clone(), u.name.span);
                if self.probe_span.get() == Some(u.name.span) {
                    *self.probe_var_provenance.borrow_mut() =
                        Some([u.name.span].into_iter().collect());
                }
            } else {
                locals.insert_var(&u.name, left.clone());
                // The closure may write anything to it.
                locals.clear_native_var(&u.name);
            }
            let written = match self.captured_types.remove(&u.name.name) {
                Some(earlier) => self.relation().union_of(vec![earlier, left]),
                None => left,
            };
            let entered = entries
                .iter()
                .find(|(name, _)| *name == u.name.name)
                .map_or_else(Type::mixed, |(_, ty)| ty.clone());
            let entered = self.relation().union_of(vec![entered, written.clone()]);
            self.capture_entries
                .push((at.start, u.name.name.clone(), entered));
            self.captured_types.insert(u.name.name.clone(), written);
        }
    }
}
