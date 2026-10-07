use crate::checker::{Checker, Locals};
use syntax::ast::*;

impl<'a> Checker<'a> {
    /// A `new SomeException(...)` expression sitting on its own line as a
    /// whole statement - the object is constructed and immediately
    /// discarded, never actually thrown. A real, common "forgot the
    /// `throw` keyword" typo, not a deliberate no-op (unlike an ordinary
    /// `new Foo();` for a class with a constructor side effect, which
    /// this deliberately never touches - only a genuine `Throwable`
    /// qualifies).
    pub(crate) fn check_new_exception_discarded(&mut self, e: &ExprNode, locals: &Locals) {
        if !self.settings.extras.check_new_exception_discarded {
            return;
        }
        let Some(owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = owner.arenas();
        let Some(inner) = arenas.peeled(e.id).and_then(|id| arenas.expr(id)) else {
            return;
        };
        if !matches!(inner.kind, ExprKind::New(..)) {
            return;
        }
        let ty = self.infer_expr(inner, locals);
        if let Some((class, _)) = ty.as_object()
            && self.class_name_exists(class)
            && self.registry.is_subclass_of(class, "Throwable")
        {
            self.report(
                format!("Construction of {class} is discarded - did you forget the throw keyword?"),
                self.expr_span(e.id),
                5,
                "new.exceptionNotThrown",
            );
        }
    }
}
