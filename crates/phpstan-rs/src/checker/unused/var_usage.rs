use syntax::name::NameSet;
use types::Name;

use syntax::ast::*;
use syntax::visit::arena::{self, Visitor};

// ---------------------------------------------------------------------
// Variable-name usage (constructor parameters / closure `use(...)`)
// ---------------------------------------------------------------------

/// The result of walking one function/closure body for "which variable
/// names does it reference anywhere" - any occurrence (read, write, or a
/// nested closure's own `use(...)` capture of the same name), not just a
/// provably-read one: a false "unused" report is treated as much worse
/// than a missed one, and a parameter that's merely reassigned without
/// ever being read again is a subtler bug than an unused-parameter check
/// is meant to catch.
pub(crate) enum VarUsage {
    Names(NameSet),
    /// Something that reads variables by a name this walk can't resolve
    /// statically was seen anywhere in the body - `func_get_args()`,
    /// `compact(...)` with a non-literal argument, `get_defined_vars()`,
    /// `include`/`require` (which can define arbitrary new variables in
    /// the current scope, real PHP's own reason `UnusedFunctionParametersCheck`
    /// treats them the same way), or a `${expr}`/`$$x` dynamic variable
    /// whose name isn't a literal string. Every candidate parameter/use
    /// should be treated as used once this is set - this project's own
    /// `extract()`/`eval()` detection already discards a whole function
    /// body's diagnostics outright (`Checker::saw_dynamic_var_construct`),
    /// so those two aren't duplicated here.
    Escaped,
}

impl VarUsage {
    pub(crate) fn contains(&self, name: &str) -> bool {
        match self {
            VarUsage::Names(names) => names.contains(name),
            VarUsage::Escaped => true,
        }
    }
}

pub(crate) fn collect_var_usage(arenas: &NodeArenas, stmts: &[StmtId]) -> VarUsage {
    let mut vars = Vars {
        names: NameSet::default(),
        escaped: false,
    };
    arena::walk_stmts(&mut vars, arenas, stmts);
    if vars.escaped {
        VarUsage::Escaped
    } else {
        VarUsage::Names(vars.names)
    }
}

/// Whether `callee` names the global function `target`, as an unqualified
/// call in a namespace falls back to.
fn is_call_to(arenas: &NodeArenas, callee: ExprId, target: &str) -> bool {
    matches!(arenas.expr(callee).map(|node| &node.kind), Some(ExprKind::Name(n)) if {
        let Some(n) = arenas.qualified_name(n) else { return false };
        let (primary, fallback) = n.candidates();
        fallback.unwrap_or(primary).eq_ignore_ascii_case(target)
    })
}

/// The variable names one body references. A nested named function or
/// class declaration has its own scope and is not entered; an anonymous
/// class's constructor arguments run in this one.
struct Vars {
    names: NameSet,
    escaped: bool,
}

impl Visitor for Vars {
    fn visit_function_decl(&mut self, _: &NodeArenas, _: &FunctionDecl) {}

    fn visit_class_def(&mut self, _: &NodeArenas, _: &ClassDef) {}

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        syntax::stack::guard(|| {
            let Some(expr) = arenas.expr(id) else { return };
            match &expr.kind {
                ExprKind::Variable(name) => {
                    if **name != *"this" {
                        self.names.insert(name.clone());
                    }
                }
                ExprKind::DynamicVariable(inner) => {
                    let Some(inner) = arenas.expr(*inner) else {
                        return;
                    };
                    if let ExprKind::Literal(Literal::String(s)) = &inner.kind {
                        if let Some(bytes) = arenas.string_bytes(s) {
                            self.names
                                .insert(Name::from(String::from_utf8_lossy(bytes).into_owned()));
                        } else {
                            self.escaped = true;
                        }
                    } else {
                        self.escaped = true;
                    }
                }
                // The included file or evaluated code can read any variable.
                ExprKind::Include(..) | ExprKind::Eval(_) => self.escaped = true,
                ExprKind::Call(callee, args) => {
                    if is_call_to(arenas, *callee, "func_get_args")
                        || is_call_to(arenas, *callee, "get_defined_vars")
                    {
                        self.escaped = true;
                    } else if is_call_to(arenas, *callee, "compact") {
                        for arg in arenas.args(args.clone()) {
                            collect_compact_names(
                                arenas,
                                arg.value,
                                &mut self.names,
                                &mut self.escaped,
                            );
                        }
                    }
                }
                // Mirrors real PHPStan's own `UnusedFunctionParametersCheck::
                // getUsedVariables`: only a closure's own `use(...)` list
                // counts as a reference to the enclosing scope's variable of
                // the same name - its parameters and body have their own,
                // independent scope, where a same-named but unrelated inner
                // variable would otherwise read as a use of the outer one.
                // An arrow function implicitly captures every outer variable
                // it references by value, so its body is walked.
                ExprKind::Closure(closure_id) => {
                    let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure_id) else {
                        self.escaped = true;
                        return;
                    };
                    let uses = arenas.closure_uses(&closure.uses);
                    for u in uses {
                        self.names.insert(u.name.name.clone());
                    }
                    return;
                }
                _ => {}
            }
            arena::walk_expr(self, arenas, id);
        })
    }
}

/// `compact('a', 'b')` / `compact(['a', 'b'])` reads the named variables
/// by string, dynamically - a literal string (nested one level into an
/// array literal, PHP allows both forms) resolves to a real, precise name;
/// anything else (a variable holding the name, a function call, `...$x`
/// spread, ...) has no static name to resolve here, so it escalates to the
/// same blanket "everything is used" fallback as `func_get_args()`/
/// `get_defined_vars()` rather than risk a false "unused" report.
fn collect_compact_names(arenas: &NodeArenas, id: ExprId, names: &mut NameSet, escaped: &mut bool) {
    let Some(e) = arenas.expr(id) else {
        *escaped = true;
        return;
    };
    syntax::stack::guard(|| match &e.kind {
        ExprKind::Literal(Literal::String(s)) => {
            if let Some(bytes) = arenas.string_bytes(s) {
                names.insert(Name::from(String::from_utf8_lossy(bytes).into_owned()));
            } else {
                *escaped = true;
            }
        }
        ExprKind::ArrayLiteral(items) => {
            for item in arenas.array_items(items.clone()) {
                if item.spread {
                    *escaped = true;
                    continue;
                }
                collect_compact_names(arenas, item.value, names, escaped);
            }
        }
        _ => *escaped = true,
    })
}
