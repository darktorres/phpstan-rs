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
/// Returns the set of classes a literal `Foo::class` scope argument
/// names, plus a coarser "give up entirely" flag for a bind/bindTo call
/// whose scope this project can't resolve without real type inference
/// (an omitted scope, or an arbitrary expression).
pub(crate) fn scan_program_for_rebind_targets(program: &[Stmt]) -> (FxHashSet<Arc<str>>, bool) {
    let mut rebinds = Rebinds {
        targets: FxHashSet::default(),
        escape_all: false,
    };
    walk_stmts(&mut rebinds, program);
    (rebinds.targets, rebinds.escape_all)
}

struct Rebinds {
    targets: FxHashSet<Arc<str>>,
    escape_all: bool,
}

impl<'ast> Visitor<'ast> for Rebinds {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            // `Closure::bind($closure, $newThis, $scope)` - real, idiomatic
            // scope-widening (see `checker/expr.rs`'s own handling of the
            // identical call, which this mirrors).
            ExprKind::StaticCall(target, method, args)
                if target.as_name() == Some("Closure") && method == "bind" =>
            {
                record_rebind_scope(args.get(2), &mut self.targets, &mut self.escape_all);
            }
            // `$closure->bindTo($newThis, $scope)` - same idea, on the
            // receiver instead of the first argument, so the scope is
            // argument index 1, not 2.
            ExprKind::MethodCall(_, method, args, _) if **method == *"bindTo" => {
                record_rebind_scope(args.get(1), &mut self.targets, &mut self.escape_all);
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
