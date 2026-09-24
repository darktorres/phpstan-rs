use crate::ast::*;
use crate::checker::{Checker, Locals};
use crate::types::Type;

impl<'a> Checker<'a> {
    /// A `new SomeException(...)` expression sitting on its own line as a
    /// whole statement - the object is constructed and immediately
    /// discarded, never actually thrown. A real, common "forgot the
    /// `throw` keyword" typo, not a deliberate no-op (unlike an ordinary
    /// `new Foo();` for a class with a constructor side effect, which
    /// this deliberately never touches - only a genuine `Throwable`
    /// qualifies).
    pub(crate) fn check_new_exception_discarded(&mut self, e: &Expr, locals: &Locals, label: &str) {
        if !self.registry.check_new_exception_discarded {
            return;
        }
        let inner = unwrap_grouping(e);
        if !matches!(inner.kind, ExprKind::New(..)) {
            return;
        }
        let ty = self.infer_expr(inner, locals);
        if let Type::Object(class, _) = &ty
            && self.class_name_exists(class)
            && self.registry.is_subclass_of(class, "Throwable")
        {
            self.report(
                label.to_string(),
                format!("Construction of {class} is discarded - did you forget the throw keyword?"),
                e.span,
                5,
                "new.exceptionNotThrown",
            );
        }
    }
}
