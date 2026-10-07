use syntax::name::NameSet;
use types::Name;

use syntax::ast::*;
use syntax::visit::arena::{self, Visitor};

// ---------------------------------------------------------------------
// `Closure::bind`/`->bindTo` rebind targets (see `Checker::
// rebind_target_classes`'s own doc comment)
// ---------------------------------------------------------------------

/// The bit of `FileFacts::mentions` a file that names `bind`, `bindTo` or
/// `call` has: a closure rebind needs one of them.
pub(crate) const MENTIONS_REBIND: u32 = 1 << 1;

/// The spellings that set `MENTIONS_REBIND`.
pub(crate) const WATCH: &[(&str, u32)] = &[
    ("bind", MENTIONS_REBIND),
    ("bindto", MENTIONS_REBIND),
    ("call", MENTIONS_REBIND),
];

/// A whole-program pre-pass that sees into every function, class and
/// closure, since a `Closure::bind` call reaching
/// into class `Foo`'s private members can appear in a completely
/// unrelated top-level function, nowhere near `Foo`'s own declaration.
/// Returns the set of classes a rebind's scope names, plus a coarser
/// "give up entirely" flag for one whose scope this project can't
/// resolve without real type inference. A rebind is `Closure::bind()`,
/// `->bindTo()`, or `->call()` on a closure (a literal, or a variable
/// assigned one), whose scope is its object's class.
pub(crate) fn scan_program_for_rebind_targets(
    arenas: &NodeArenas,
    program: &[StmtId],
) -> (NameSet, bool) {
    let mut closures = ClosureVariables::default();
    arena::walk_stmts(&mut closures, arenas, program);
    let mut rebinds = Rebinds {
        targets: NameSet::default(),
        escape_all: false,
        closure_variables: closures.0,
    };
    arena::walk_stmts(&mut rebinds, arenas, program);
    (rebinds.targets, rebinds.escape_all)
}

/// Every variable name some assignment gives a closure.
#[derive(Default)]
struct ClosureVariables(NameSet);

impl Visitor for ClosureVariables {
    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        if let Some(expr) = arenas.expr(id)
            && let ExprKind::Assign(target, value) = &expr.kind
            && let Some(target) = arenas.expr(*target)
            && let ExprKind::Variable(name) = &target.kind
            && is_closure_expr(arenas, *value)
        {
            self.0.insert(name.clone());
        }
        arena::walk_expr(self, arenas, id);
    }
}

/// A closure or arrow function literal, or a first-class callable.
fn is_closure_expr(arenas: &NodeArenas, id: ExprId) -> bool {
    arenas
        .peeled(id)
        .and_then(|id| arenas.expr(id))
        .is_some_and(|e| {
            matches!(
                &e.kind,
                ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_)
            )
        })
}

struct Rebinds {
    targets: NameSet,
    escape_all: bool,
    closure_variables: NameSet,
}

impl Visitor for Rebinds {
    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(expr) = arenas.expr(id) else { return };
        match &expr.kind {
            // `Closure::bind($closure, $newThis, $scope)`.
            ExprKind::StaticCall(StaticTarget::Name(class), method, args)
                if class
                    .resolved_class()
                    .is_some_and(|c| c.eq_ignore_ascii_case("Closure"))
                    && method.eq_ignore_ascii_case("bind") =>
            {
                record_rebind_scope(arenas, arenas.args(args.clone()).get(2), &mut self.targets, &mut self.escape_all);
            }
            // `$closure->bindTo($newThis, $scope)` - the scope is argument
            // index 1.
            ExprKind::MethodCall(_, method, args, _) if method.eq_ignore_ascii_case("bindTo") => {
                record_rebind_scope(arenas, arenas.args(args.clone()).get(1), &mut self.targets, &mut self.escape_all);
            }
            // `$closure->call($newThis, ...)` runs it with `$newThis`'s
            // class as its scope.
            ExprKind::MethodCall(receiver, method, args, _)
                if method.eq_ignore_ascii_case("call")
                    && (is_closure_expr(arenas, *receiver)
                        || arenas.peeled(*receiver).and_then(|id| arenas.expr(id)).is_some_and(|receiver|
                            matches!(&receiver.kind, ExprKind::Variable(v) if self.closure_variables.contains(v)))) =>
            {
                match arenas.args(args.clone()).first().and_then(|a| arenas.peeled(a.value)).and_then(|id| arenas.expr(id)).map(|e| &e.kind) {
                    Some(ExprKind::New(class, _))
                        if let Some(class) = arenas.expr(*class)
                            && let ExprKind::Name(name) = &class.kind
                            && let Some(class) = arenas.qualified_name(name).and_then(QualifiedName::resolved_class) =>
                    {
                        self.targets.insert(Name::from(class));
                    }
                    _ => self.escape_all = true,
                }
            }
            _ => {}
        }
        arena::walk_expr(self, arenas, id);
    }
}
fn record_rebind_scope(
    arenas: &NodeArenas,
    scope_arg: Option<&Arg>,
    targets: &mut NameSet,
    escape_all: &mut bool,
) {
    match scope_arg
        .and_then(|a| arenas.expr(a.value))
        .map(|e| &e.kind)
    {
        Some(ExprKind::ClassNameFetch(StaticTarget::Name(class_name)))
            if let Some(class) = class_name.resolved_class() =>
        {
            targets.insert(Name::from(class));
        }
        // An omitted scope argument, or one that isn't a literal
        // `Foo::class` naming a class (a variable, `get_class($x)`,
        // `self::class`, ...) - can't rule out
        // it reaches an arbitrary class, so every class's private-member
        // check is skipped for the rest of this file.
        _ => *escape_all = true,
    }
}
