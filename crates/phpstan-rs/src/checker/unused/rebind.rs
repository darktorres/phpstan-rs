use rustc_hash::FxHashSet;
use std::sync::Arc;

use syntax::ast::*;
use syntax::visit::{Visitor, walk_expr, walk_stmts};

// ---------------------------------------------------------------------
// `Closure::bind`/`->bindTo` rebind targets (see `Checker::
// rebind_target_classes`'s own doc comment)
// ---------------------------------------------------------------------

/// A whole-program pre-pass that sees into every function, class and
/// closure, since a `Closure::bind` call reaching
/// into class `Foo`'s private members can appear in a completely
/// unrelated top-level function, nowhere near `Foo`'s own declaration.
/// Returns the set of classes a rebind's scope names, plus a coarser
/// "give up entirely" flag for one whose scope this project can't
/// resolve without real type inference. A rebind is `Closure::bind()`,
/// `->bindTo()`, or `->call()` on a closure (a literal, or a variable
/// assigned one), whose scope is its object's class.
pub(crate) fn scan_program_for_rebind_targets(program: &[Stmt]) -> (FxHashSet<Arc<str>>, bool) {
    let mut closures = ClosureVariables::default();
    walk_stmts(&mut closures, program);
    let mut rebinds = Rebinds {
        targets: FxHashSet::default(),
        escape_all: false,
        closure_variables: closures.0,
    };
    walk_stmts(&mut rebinds, program);
    (rebinds.targets, rebinds.escape_all)
}

/// Every variable name some assignment gives a closure.
#[derive(Default)]
struct ClosureVariables(FxHashSet<Arc<str>>);

impl<'ast> Visitor<'ast> for ClosureVariables {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let ExprKind::Assign(target, value) = &expr.kind
            && let ExprKind::Variable(name) = &target.kind
            && is_closure_expr(value)
        {
            self.0.insert(name.clone());
        }
        walk_expr(self, expr);
    }
}

/// A closure or arrow function literal, or a first-class callable.
fn is_closure_expr(e: &Expr) -> bool {
    matches!(
        &e.peeled().kind,
        ExprKind::Closure { .. } | ExprKind::ArrowFn { .. } | ExprKind::FirstClassCallable(_)
    )
}

struct Rebinds {
    targets: FxHashSet<Arc<str>>,
    escape_all: bool,
    closure_variables: FxHashSet<Arc<str>>,
}

impl<'ast> Visitor<'ast> for Rebinds {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            // `Closure::bind($closure, $newThis, $scope)`.
            ExprKind::StaticCall(StaticTarget::Name(class, _), method, args)
                if class
                    .resolved_class()
                    .is_some_and(|c| c.eq_ignore_ascii_case("Closure"))
                    && method.eq_ignore_ascii_case("bind") =>
            {
                record_rebind_scope(args.get(2), &mut self.targets, &mut self.escape_all);
            }
            // `$closure->bindTo($newThis, $scope)` - the scope is argument
            // index 1.
            ExprKind::MethodCall(_, method, args, _) if method.eq_ignore_ascii_case("bindTo") => {
                record_rebind_scope(args.get(1), &mut self.targets, &mut self.escape_all);
            }
            // `$closure->call($newThis, ...)` runs it with `$newThis`'s
            // class as its scope.
            ExprKind::MethodCall(receiver, method, args, _)
                if method.eq_ignore_ascii_case("call")
                    && (is_closure_expr(receiver)
                        || matches!(&receiver.peeled().kind,
                            ExprKind::Variable(v) if self.closure_variables.contains(v))) =>
            {
                match args.first().map(|a| &a.value.peeled().kind) {
                    Some(ExprKind::New(class, _))
                        if let ExprKind::Name(name) = &class.kind
                            && let Some(class) = name.resolved_class() =>
                    {
                        self.targets.insert(Arc::from(class));
                    }
                    _ => self.escape_all = true,
                }
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}
fn record_rebind_scope(
    scope_arg: Option<&Arg>,
    targets: &mut FxHashSet<Arc<str>>,
    escape_all: &mut bool,
) {
    match scope_arg.map(|a| &a.value.kind) {
        Some(ExprKind::ClassNameFetch(StaticTarget::Name(class_name, _)))
            if let Some(class) = class_name.resolved_class() =>
        {
            targets.insert(Arc::from(class));
        }
        // An omitted scope argument, or one that isn't a literal
        // `Foo::class` naming a class (a variable, `get_class($x)`,
        // `self::class`, ...) - can't rule out
        // it reaches an arbitrary class, so every class's private-member
        // check is skipped for the rest of this file.
        _ => *escape_all = true,
    }
}
