//! Phase 59: the unused-symbol detection subsystem - one variable/member-
//! usage walker, applied at four call sites (constructor parameters,
//! closure `use(...)` captures, and private class consts/methods/
//! properties) to back five diagnostics: `UnusedConstructorParametersRule`-
//! style, `UnusedClosureUsesRule`-style, `UnusedPrivateConstantRule`,
//! `UnusedPrivateMethodRule`, and `UnusedPrivatePropertyRule` (this last
//! one with real PHPStan's own five distinct read/write message shapes -
//! see `PrivatePropertyState::message` below).
//!
//! Deliberately scoped down from the roadmap phase text in one place: real
//! PHPStan's own `UnusedConstructorParametersRule` (the one rule this
//! project's Phase 59 phase text actually names) checks *only*
//! `__construct` parameters, never an arbitrary function/method's - a
//! general "this parameter is never read" rule is far noisier in real code
//! (an interface implementation's parameter, an overridden method's
//! parameter, an `array_map`/`usort` callback's parameter, ... all
//! routinely go unused without being a bug) and isn't what real PHPStan
//! ships at all. This module follows the named rule, not the broader
//! "function/method parameters" phrase earlier in the same paragraph.
//!
//! Grounded directly against the real rule sources this project doesn't
//! vendor by default (`vendor/phpstan-src/src/Rules/{Classes,Functions,
//! DeadCode}/Unused*.php` and their shared `UnusedFunctionParametersCheck`)
//! rather than guessed from the phase text alone - message wording is
//! adapted to this project's own lowercase, un-punctuated diagnostic style
//! (see `checker/oop.rs`'s existing messages), not copied verbatim.

use std::collections::HashSet;

use crate::ast::*;
use crate::ast_walk::{expr_children, stmt_children};
use crate::checker::Checker;
use crate::registry::NameContext;
use crate::token::Span;

// ---------------------------------------------------------------------
// Variable-name usage (constructor parameters / closure `use(...)`)
// ---------------------------------------------------------------------

/// The result of walking one function/closure body for "which variable
/// names does it reference anywhere" - deliberately *any* occurrence
/// (read, write, or a nested closure's own `use(...)` capture of the same
/// name), not just a provably-read one: this project treats a false
/// "unused" report as much worse than a missed one (see the project's own
/// conventions), and a parameter that's merely reassigned without ever
/// being read again is a real but much subtler bug than an unused-
/// parameter check is meant to catch.
pub(crate) enum VarUsage {
    Names(HashSet<String>),
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

pub(crate) fn collect_var_usage(stmts: &[Stmt]) -> VarUsage {
    let mut names = HashSet::new();
    let mut escaped = false;
    for stmt in stmts {
        walk_stmt_for_vars(stmt, &mut names, &mut escaped);
    }
    if escaped {
        VarUsage::Escaped
    } else {
        VarUsage::Names(names)
    }
}

fn is_call_to(callee: &Expr, target: &str) -> bool {
    matches!(&callee.kind, ExprKind::Name(n) if n.trim_start_matches('\\').eq_ignore_ascii_case(target))
}

fn walk_stmt_for_vars(stmt: &Stmt, names: &mut HashSet<String>, escaped: &mut bool) {
    match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => walk_expr_for_vars(e, names, escaped),
        StmtKind::Return(Some(e)) => walk_expr_for_vars(e, names, escaped),
        StmtKind::Echo(exprs) => {
            for e in exprs {
                walk_expr_for_vars(e, names, escaped);
            }
        }
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => walk_expr_for_vars(cond, names, escaped),
        StmtKind::For {
            init, cond, step, ..
        } => {
            for e in init.iter().chain(cond).chain(step) {
                walk_expr_for_vars(e, names, escaped);
            }
        }
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            walk_expr_for_vars(subject, names, escaped);
            if let Some(k) = key {
                walk_expr_for_vars(k, names, escaped);
            }
            walk_expr_for_vars(value, names, escaped);
        }
        StmtKind::GlobalConst(consts) => {
            for c in consts {
                walk_expr_for_vars(&c.value, names, escaped);
            }
        }
        StmtKind::StaticVar(vars) => {
            for (_, d) in vars {
                if let Some(e) = d {
                    walk_expr_for_vars(e, names, escaped);
                }
            }
        }
        StmtKind::Switch { subject, cases } => {
            walk_expr_for_vars(subject, names, escaped);
            for case in cases {
                if let Some(t) = &case.test {
                    walk_expr_for_vars(t, names, escaped);
                }
            }
        }
        _ => {}
    }
    for child in stmt_children(stmt) {
        walk_stmt_for_vars(child, names, escaped);
    }
}

fn walk_expr_for_vars(expr: &Expr, names: &mut HashSet<String>, escaped: &mut bool) {
    match &expr.kind {
        ExprKind::Variable(name) => {
            if name != "this" {
                names.insert(name.clone());
            }
        }
        ExprKind::DynamicVariable(inner) => {
            if let ExprKind::Literal(Literal::String(s)) = &inner.kind {
                names.insert(s.clone());
            } else {
                *escaped = true;
            }
        }
        ExprKind::Call(callee, args) => {
            if is_call_to(callee, "func_get_args")
                || is_call_to(callee, "get_defined_vars")
                || is_call_to(callee, "include")
                || is_call_to(callee, "include_once")
                || is_call_to(callee, "require")
                || is_call_to(callee, "require_once")
            {
                *escaped = true;
            } else if is_call_to(callee, "compact") {
                for arg in args {
                    collect_compact_names(&arg.value, names, escaped);
                }
            }
        }
        // Mirrors real PHPStan's own `UnusedFunctionParametersCheck::
        // getUsedVariables`: only a closure's own `use(...)` list counts
        // as a reference to the *enclosing* scope's variable of the same
        // name - its `params`/`body` have their own, independent scope,
        // and naively recursing into them risks reading a same-named but
        // unrelated inner parameter/local as if it were a use of the
        // outer one. `expr_children` already excludes `Closure`'s
        // children entirely (see `ast_walk`'s own module doc comment), so
        // this is the only place its variables are ever collected from.
        ExprKind::Closure { uses, .. } => {
            for u in uses {
                names.insert(u.name.clone());
            }
        }
        // Unlike `Closure`, an arrow function implicitly captures every
        // outer variable it references by value - so, unlike above, its
        // `params`/`body` genuinely do need a normal recursive walk here.
        // `expr_children` excludes `ArrowFn`'s children too, so this is
        // the only place they're walked from.
        ExprKind::ArrowFn { params, body, .. } => {
            for p in params {
                if let Some(d) = &p.default {
                    walk_expr_for_vars(d, names, escaped);
                }
            }
            walk_expr_for_vars(body, names, escaped);
        }
        // Real PHP anonymous classes don't implicitly capture the
        // enclosing scope's variables at all - only the constructor
        // arguments (evaluated in the enclosing scope before being passed
        // in) can reference an outer variable. `expr_children` excludes
        // `NewAnonymousClass`'s children entirely, so the constructor args
        // need an explicit walk here; the class's own method bodies
        // deliberately aren't recursed into (a separate, unrelated scope).
        ExprKind::NewAnonymousClass(_, args) => {
            for a in args {
                walk_expr_for_vars(&a.value, names, escaped);
            }
        }
        _ => {}
    }
    for child in expr_children(expr) {
        walk_expr_for_vars(child, names, escaped);
    }
}

/// `compact('a', 'b')` / `compact(['a', 'b'])` reads the named variables
/// by string, dynamically - a literal string (nested one level into an
/// array literal, PHP allows both forms) resolves to a real, precise name;
/// anything else (a variable holding the name, a function call, `...$x`
/// spread, ...) has no static name to resolve here, so it escalates to the
/// same blanket "everything is used" fallback as `func_get_args()`/
/// `get_defined_vars()` rather than risk a false "unused" report.
fn collect_compact_names(e: &Expr, names: &mut HashSet<String>, escaped: &mut bool) {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => {
            names.insert(s.clone());
        }
        ExprKind::ArrayLiteral(items) => {
            for item in items {
                if item.spread {
                    *escaped = true;
                    continue;
                }
                collect_compact_names(&item.value, names, escaped);
            }
        }
        _ => *escaped = true,
    }
}

// ---------------------------------------------------------------------
// Private-member usage (consts / methods / properties)
// ---------------------------------------------------------------------

/// Every private const/method/property name referenced anywhere across one
/// class's own bodies (method bodies, property/const/promoted-parameter
/// default values, property-hook bodies) - built once per class, then
/// consulted once per private candidate member. `escaped` is set by any
/// fully dynamic member access (`$obj->{$expr}`, `$obj->$name(...)`, or a
/// `Foo::{$expr}`/`Foo::$$name` static access) found anywhere in the
/// class - once set, this project has no way to know which member such an
/// access might reach, so every private-member check for the *whole*
/// class is skipped outright rather than risk a false "unused" report on
/// a member that dynamic code actually does reach.
#[derive(Default)]
struct MemberUsage {
    consts: HashSet<String>,
    methods: HashSet<String>,
    reads: HashSet<String>,
    writes: HashSet<String>,
    escaped: bool,
}

/// Whether `class_ref` (the class name half of a `Foo::bar`/`self::bar`/
/// `static::bar` expression, exactly as written in source) refers to the
/// class currently being scanned - `self`/`static` always do (a private
/// member is only ever reachable from inside its own declaring class to
/// begin with, so late static binding through `static::` can't actually
/// reach a *different* class's private member); anything else is resolved
/// through the same file-local `use`-import table every other class/
/// function reference in this checker goes through, so an explicit
/// same-class self-reference (`Foo::BAR` written inside `Foo` itself,
/// unusual but legal) is still recognized.
fn class_ref_is_self(class_ref: &str, fqn: &str, ctx: &NameContext) -> bool {
    class_ref == "self" || class_ref == "static" || ctx.resolve(class_ref) == fqn
}

fn is_this(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Variable(name) if name == "this")
}

/// Marks `l` - the left-hand side of an `Assign`/`CompoundAssign` - as a
/// write (and, for a compound assignment, also a read) when it's exactly
/// `$this->prop` or `self::$prop`/`static::$prop`; returns `false` for
/// anything more complex (`$this->arr[$i] = ...`, destructuring, ...),
/// which the caller then falls back to walking like any other expression -
/// conservative in the direction this project always prefers: a property
/// only ever mutated in place might go unreported as "never written", but
/// this can never *cause* a false "unused"/"only written" report, since
/// falling back to a plain read only ever adds usage, never removes it.
fn mark_write_target(
    l: &Expr,
    fqn: &str,
    ctx: &NameContext,
    usage: &mut MemberUsage,
    also_read: bool,
) -> bool {
    match &l.kind {
        ExprKind::PropertyAccess(base, name, _) if is_this(base) => {
            usage.writes.insert(name.clone());
            if also_read {
                usage.reads.insert(name.clone());
            }
            true
        }
        ExprKind::StaticAccess(class_ref, member) if class_ref_is_self(class_ref, fqn, ctx) => {
            match member.strip_prefix('$') {
                Some(prop) => {
                    usage.writes.insert(prop.to_string());
                    if also_read {
                        usage.reads.insert(prop.to_string());
                    }
                    true
                }
                None => false,
            }
        }
        _ => false,
    }
}

fn walk_stmt_for_members(stmt: &Stmt, fqn: &str, ctx: &NameContext, usage: &mut MemberUsage) {
    match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => walk_expr_for_members(e, fqn, ctx, usage),
        StmtKind::Return(Some(e)) => walk_expr_for_members(e, fqn, ctx, usage),
        StmtKind::Echo(exprs) => {
            for e in exprs {
                walk_expr_for_members(e, fqn, ctx, usage);
            }
        }
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => walk_expr_for_members(cond, fqn, ctx, usage),
        StmtKind::For {
            init, cond, step, ..
        } => {
            for e in init.iter().chain(cond).chain(step) {
                walk_expr_for_members(e, fqn, ctx, usage);
            }
        }
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            walk_expr_for_members(subject, fqn, ctx, usage);
            if let Some(k) = key {
                walk_expr_for_members(k, fqn, ctx, usage);
            }
            walk_expr_for_members(value, fqn, ctx, usage);
        }
        StmtKind::GlobalConst(consts) => {
            for c in consts {
                walk_expr_for_members(&c.value, fqn, ctx, usage);
            }
        }
        StmtKind::StaticVar(vars) => {
            for (_, d) in vars {
                if let Some(e) = d {
                    walk_expr_for_members(e, fqn, ctx, usage);
                }
            }
        }
        StmtKind::Switch { subject, cases } => {
            walk_expr_for_members(subject, fqn, ctx, usage);
            for case in cases {
                if let Some(t) = &case.test {
                    walk_expr_for_members(t, fqn, ctx, usage);
                }
            }
        }
        _ => {}
    }
    for child in stmt_children(stmt) {
        walk_stmt_for_members(child, fqn, ctx, usage);
    }
}

fn walk_expr_for_members(expr: &Expr, fqn: &str, ctx: &NameContext, usage: &mut MemberUsage) {
    match &expr.kind {
        // Handled explicitly (and `return`s below, skipping the generic
        // `expr_children` recursion at the bottom of this function)
        // specifically so a pure write target - `$this->x = ...` - is
        // recorded as a write only, not *also* as a read the way falling
        // through to the generic `PropertyAccess`/`StaticAccess` arms
        // further down would.
        ExprKind::Assign(l, r) => {
            walk_expr_for_members(r, fqn, ctx, usage);
            if !mark_write_target(l, fqn, ctx, usage, false) {
                walk_expr_for_members(l, fqn, ctx, usage);
            }
            return;
        }
        ExprKind::CompoundAssign(_, l, r) => {
            walk_expr_for_members(r, fqn, ctx, usage);
            if !mark_write_target(l, fqn, ctx, usage, true) {
                walk_expr_for_members(l, fqn, ctx, usage);
            }
            return;
        }
        ExprKind::PropertyAccess(base, name, _) if is_this(base) => {
            usage.reads.insert(name.clone());
        }
        ExprKind::StaticAccess(class_ref, member) => {
            // `Foo::{$expr}` / `Foo::$$name` - the parser's own sentinel
            // for a fully dynamic static member name (`parser/postfix.rs`).
            // Can't know which const/property this reaches, so the whole
            // class's private-member checks are skipped (see `MemberUsage`'s
            // own doc comment).
            if member == "{dynamic}" {
                usage.escaped = true;
            } else if let Some(prop) = member.strip_prefix('$') {
                if class_ref_is_self(class_ref, fqn, ctx) {
                    usage.reads.insert(prop.to_string());
                }
            } else if class_ref_is_self(class_ref, fqn, ctx) {
                usage.consts.insert(member.clone());
            }
        }
        ExprKind::MethodCall(recv, name, ..) if is_this(recv) => {
            usage.methods.insert(name.clone());
        }
        ExprKind::StaticCall(class_ref, name, _) if class_ref_is_self(class_ref, fqn, ctx) => {
            usage.methods.insert(name.clone());
        }
        // `$obj->{$expr}` / `$obj->$name(...)` - the roadmap's own
        // called-out dynamic-member-access escape hatch.
        ExprKind::DynamicPropertyAccess(..) | ExprKind::DynamicMethodCall(..) => {
            usage.escaped = true;
        }
        // `[$this, 'methodName']` - PHP's array-callable convention
        // (`usort($arr, [$this, 'compare'])`, `array_map([$this, 'fmt'],
        // ...)`, ...), common enough that skipping it would be a real
        // false-positive source for exactly the kind of private helper
        // method this check is meant to protect, not flag.
        ExprKind::ArrayLiteral(items) => {
            if let [a, b] = items.as_slice()
                && a.key.is_none()
                && b.key.is_none()
                && !a.spread
                && !b.spread
                && is_this(&a.value)
                && let ExprKind::Literal(Literal::String(name)) = &b.value.kind
            {
                usage.methods.insert(name.clone());
            }
        }
        _ => {}
    }
    for child in expr_children(expr) {
        walk_expr_for_members(child, fqn, ctx, usage);
    }
}

fn walk_property_hooks_for_members(
    hooks: &PropertyHooks,
    fqn: &str,
    ctx: &NameContext,
    usage: &mut MemberUsage,
) {
    if let Some(body) = &hooks.get {
        walk_hook_body_for_members(body, fqn, ctx, usage);
    }
    if let Some((_, body)) = &hooks.set {
        walk_hook_body_for_members(body, fqn, ctx, usage);
    }
}

fn walk_hook_body_for_members(
    body: &PropertyHookBody,
    fqn: &str,
    ctx: &NameContext,
    usage: &mut MemberUsage,
) {
    match body {
        PropertyHookBody::Arrow(e) => walk_expr_for_members(e, fqn, ctx, usage),
        PropertyHookBody::Block(stmts) => {
            for s in stmts {
                walk_stmt_for_members(s, fqn, ctx, usage);
            }
        }
    }
}

/// A `get`-only property hook (`{ get => ...; }`, no `set`) can't be
/// assigned through the property syntax at all, so "never written"
/// deserves its own wording (`property.neverWritten`-shaped, mirroring
/// real PHPStan) rather than the generic "only read" one; symmetrically
/// for a `set`-only hook and "never read". Neither is a property this
/// checker's parser can't represent - see `ast::PropertyHooks`.
fn hook_read_write_shape(hooks: Option<&PropertyHooks>) -> (bool, bool) {
    match hooks {
        Some(h) => (
            h.get.is_some() && h.set.is_none(),
            h.set.is_some() && h.get.is_none(),
        ),
        None => (false, false),
    }
}

// ---------------------------------------------------------------------
// `Closure::bind`/`->bindTo` rebind targets (see `Checker::
// rebind_target_classes`'s own doc comment)
// ---------------------------------------------------------------------

/// A whole-program pre-pass (unlike every other walker in this module,
/// which deliberately stops at a nested function/class declaration - this
/// one needs to see into every one of them, since a `Closure::bind` call
/// reaching into class `Foo`'s private members can appear in a completely
/// unrelated top-level function, nowhere near `Foo`'s own declaration).
/// Returns the set of classes a *literal* `Foo::class` scope argument
/// names, plus a coarser "give up entirely" flag for a bind/bindTo call
/// whose scope this project can't resolve without real type inference (an
/// omitted scope, or an arbitrary expression) - see the two call sites'
/// own doc comments for why either one short-circuits
/// `check_unused_private_members` rather than risk a false report.
pub(crate) fn scan_program_for_rebind_targets(
    program: &[Stmt],
    ctx: &NameContext,
) -> (HashSet<String>, bool) {
    let mut targets = HashSet::new();
    let mut escape_all = false;
    for stmt in program {
        scan_stmt_for_rebinds(stmt, ctx, &mut targets, &mut escape_all);
    }
    (targets, escape_all)
}

fn scan_stmts_for_rebinds(
    body: &[Stmt],
    ctx: &NameContext,
    targets: &mut HashSet<String>,
    escape_all: &mut bool,
) {
    for s in body {
        scan_stmt_for_rebinds(s, ctx, targets, escape_all);
    }
}

fn scan_stmt_for_rebinds(
    stmt: &Stmt,
    ctx: &NameContext,
    targets: &mut HashSet<String>,
    escape_all: &mut bool,
) {
    match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => {
            scan_expr_for_rebinds(e, ctx, targets, escape_all)
        }
        StmtKind::Return(Some(e)) => scan_expr_for_rebinds(e, ctx, targets, escape_all),
        StmtKind::Echo(exprs) => {
            for e in exprs {
                scan_expr_for_rebinds(e, ctx, targets, escape_all);
            }
        }
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => scan_expr_for_rebinds(cond, ctx, targets, escape_all),
        StmtKind::For {
            init, cond, step, ..
        } => {
            for e in init.iter().chain(cond).chain(step) {
                scan_expr_for_rebinds(e, ctx, targets, escape_all);
            }
        }
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            scan_expr_for_rebinds(subject, ctx, targets, escape_all);
            if let Some(k) = key {
                scan_expr_for_rebinds(k, ctx, targets, escape_all);
            }
            scan_expr_for_rebinds(value, ctx, targets, escape_all);
        }
        StmtKind::GlobalConst(consts) => {
            for c in consts {
                scan_expr_for_rebinds(&c.value, ctx, targets, escape_all);
            }
        }
        StmtKind::StaticVar(vars) => {
            for (_, d) in vars {
                if let Some(e) = d {
                    scan_expr_for_rebinds(e, ctx, targets, escape_all);
                }
            }
        }
        StmtKind::Switch { subject, cases } => {
            scan_expr_for_rebinds(subject, ctx, targets, escape_all);
            for case in cases {
                if let Some(t) = &case.test {
                    scan_expr_for_rebinds(t, ctx, targets, escape_all);
                }
            }
        }
        // Unlike every other walker in this module, these three ARE
        // recursed into here - a nested declaration can still contain a
        // `Closure::bind`/`bindTo` call reaching into some other class
        // entirely.
        StmtKind::FunctionDecl(f) => scan_stmts_for_rebinds(&f.body, ctx, targets, escape_all),
        StmtKind::ClassDecl(c) => {
            for m in &c.methods {
                scan_stmts_for_rebinds(&m.function.body, ctx, targets, escape_all);
            }
            for p in &c.properties {
                if let Some(d) = &p.default {
                    scan_expr_for_rebinds(d, ctx, targets, escape_all);
                }
            }
            for cst in &c.consts {
                scan_expr_for_rebinds(&cst.value, ctx, targets, escape_all);
            }
        }
        StmtKind::EnumDecl(e) => {
            for m in &e.methods {
                scan_stmts_for_rebinds(&m.function.body, ctx, targets, escape_all);
            }
            for cst in &e.consts {
                scan_expr_for_rebinds(&cst.value, ctx, targets, escape_all);
            }
        }
        _ => {}
    }
    for child in stmt_children(stmt) {
        scan_stmt_for_rebinds(child, ctx, targets, escape_all);
    }
}

fn scan_expr_for_rebinds(
    expr: &Expr,
    ctx: &NameContext,
    targets: &mut HashSet<String>,
    escape_all: &mut bool,
) {
    match &expr.kind {
        // `Closure::bind($closure, $newThis, $scope)` - real, idiomatic
        // scope-widening (see `checker/expr.rs`'s own Finding 49 handling
        // of the identical call, which this mirrors).
        ExprKind::StaticCall(class, method, args) if class == "Closure" && method == "bind" => {
            record_rebind_scope(args.get(2), ctx, targets, escape_all);
        }
        // `$closure->bindTo($newThis, $scope)` - same idea, on the
        // receiver instead of the first argument, so the scope is
        // argument index 1, not 2.
        ExprKind::MethodCall(_, method, args, _) if method == "bindTo" => {
            record_rebind_scope(args.get(1), ctx, targets, escape_all);
        }
        // `expr_children` deliberately excludes these three (see
        // `ast_walk`'s own module doc comment) - walked explicitly here
        // since, unlike this module's other walkers, a rebind call can
        // legitimately live inside any one of them.
        ExprKind::Closure { body, .. } => scan_stmts_for_rebinds(body, ctx, targets, escape_all),
        ExprKind::ArrowFn { body, .. } => scan_expr_for_rebinds(body, ctx, targets, escape_all),
        ExprKind::NewAnonymousClass(decl, args) => {
            for m in &decl.methods {
                scan_stmts_for_rebinds(&m.function.body, ctx, targets, escape_all);
            }
            for a in args {
                scan_expr_for_rebinds(&a.value, ctx, targets, escape_all);
            }
        }
        _ => {}
    }
    for child in expr_children(expr) {
        scan_expr_for_rebinds(child, ctx, targets, escape_all);
    }
}

fn record_rebind_scope(
    scope_arg: Option<&Arg>,
    ctx: &NameContext,
    targets: &mut HashSet<String>,
    escape_all: &mut bool,
) {
    match scope_arg.map(|a| &a.value.kind) {
        Some(ExprKind::StaticAccess(class_name, member)) if member == "class" => {
            targets.insert(ctx.resolve(class_name));
        }
        // An omitted scope argument, or one that isn't a literal
        // `Foo::class` this project can resolve without real type
        // inference (a variable, `get_class($x)`, ...) - can't rule out
        // it reaches an arbitrary class, so every class's private-member
        // check is skipped for the rest of this file.
        _ => *escape_all = true,
    }
}

impl<'a> Checker<'a> {
    /// Phase 59: flags a `__construct` parameter never referenced anywhere
    /// in the constructor's own body - deliberately scoped to constructors
    /// only (real PHPStan's own `UnusedConstructorParametersRule`, not a
    /// general "unused parameter" rule - see this module's own doc comment
    /// for why). Constructor property promotion (`private int $id`) is
    /// exempt: the promotion itself is an implicit `$this->id = $id`
    /// assignment, a real use, even when the body never reads `$id` again.
    pub(crate) fn check_unused_constructor_parameters(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        label: &str,
    ) {
        let Some(fqn) = class_name else {
            return;
        };
        if f.name != "__construct" || f.body.is_empty() {
            return;
        }
        let candidates: Vec<&Param> = f
            .params
            .iter()
            .filter(|p| p.promoted_visibility.is_none())
            .collect();
        if candidates.is_empty() {
            return;
        }
        let usage = collect_var_usage(&f.body);
        for p in candidates {
            if usage.contains(&p.name) {
                continue;
            }
            self.report(
                label.to_string(),
                format!("constructor of {fqn} has an unused parameter ${}", p.name),
                p.span,
                1,
                "parameter.unused",
            );
        }
    }

    /// Phase 59: flags a closure `use ($x)`/`use (&$x)` capture never
    /// referenced anywhere in the closure's own body - real PHPStan's own
    /// `UnusedClosureUsesRule`. Applies to every closure regardless of
    /// where it appears (unlike constructor parameters, an explicit
    /// `use(...)` clause is always the closure author's own deliberate
    /// choice, never dictated by an interface/override signature, so this
    /// carries none of the false-positive risk a general unused-parameter
    /// check would).
    pub(crate) fn check_unused_closure_uses(
        &mut self,
        uses: &[ClosureUse],
        body: &[Stmt],
        label: &str,
        span: Span,
    ) {
        if uses.is_empty() {
            return;
        }
        let usage = collect_var_usage(body);
        for u in uses {
            if usage.contains(&u.name) {
                continue;
            }
            self.report(
                label.to_string(),
                format!("closure has an unused use (${})", u.name),
                span,
                1,
                "closure.unusedUse",
            );
        }
    }

    /// Phase 59: flags a private const/method/property never referenced
    /// anywhere in this class's own declared bodies - real PHPStan's
    /// `UnusedPrivateConstantRule`/`UnusedPrivateMethodRule`/
    /// `UnusedPrivatePropertyRule`. Scanning only `methods`/`properties`/
    /// `consts` exactly as declared on this one `ClassDecl`/`EnumDecl`
    /// (never the registry's own trait-flattened `ClassInfo`) means a
    /// trait-provided member is never a candidate here at all - matching
    /// real PHPStan's explicit `isDeclaredInTrait()` exclusion, for free.
    pub(crate) fn check_unused_private_members(
        &mut self,
        fqn: &str,
        kind: ClassKind,
        methods: &[MethodDecl],
        properties: &[PropertyDecl],
        consts: &[ClassConst],
    ) {
        if kind == ClassKind::Trait || kind == ClassKind::Interface {
            return;
        }
        // See `rebind_target_classes`'s own doc comment - a class reached
        // by a `Closure::bind`/`->bindTo` call from outside its own body
        // can have its private members referenced from a closure this
        // per-class scan below never sees.
        if self.rebind_escape_all || self.rebind_target_classes.contains(fqn) {
            return;
        }

        let mut usage = MemberUsage::default();
        let ctor = methods.iter().find(|m| m.function.name == "__construct");
        for m in methods {
            walk_stmt_body_for_members(&m.function.body, fqn, &self.name_ctx, &mut usage);
        }
        if let Some(ctor) = ctor {
            for p in &ctor.function.params {
                if let Some(d) = &p.default {
                    walk_expr_for_members(d, fqn, &self.name_ctx, &mut usage);
                }
                if let Some(hooks) = &p.hooks {
                    walk_property_hooks_for_members(hooks, fqn, &self.name_ctx, &mut usage);
                }
            }
        }
        for p in properties {
            if let Some(d) = &p.default {
                walk_expr_for_members(d, fqn, &self.name_ctx, &mut usage);
            }
            if let Some(hooks) = &p.hooks {
                walk_property_hooks_for_members(hooks, fqn, &self.name_ctx, &mut usage);
            }
        }
        for c in consts {
            walk_expr_for_members(&c.value, fqn, &self.name_ctx, &mut usage);
        }

        // See `MemberUsage::escaped`'s own doc comment - a fully dynamic
        // member access anywhere in the class means none of these three
        // checks can trust their own "never referenced" conclusion.
        if usage.escaped {
            return;
        }

        for c in consts {
            if c.visibility != Visibility::Private || usage.consts.contains(&c.name) {
                continue;
            }
            self.report(
                fqn.to_string(),
                format!("private constant {fqn}::{} is unused", c.name),
                c.span,
                4,
                "classConstant.unused",
            );
        }

        for m in methods {
            if m.visibility != Visibility::Private
                || m.function.name == "__construct"
                || m.function.name.eq_ignore_ascii_case("__clone")
                || usage.methods.contains(&m.function.name)
            {
                continue;
            }
            let kind_word = if m.is_static {
                "private static method"
            } else {
                "private method"
            };
            self.report(
                fqn.to_string(),
                format!("{kind_word} {fqn}::{}() is unused", m.function.name),
                m.function.span,
                4,
                "method.unused",
            );
        }

        for p in properties {
            if p.visibility != Visibility::Private {
                continue;
            }
            let (only_readable, only_writable) = hook_read_write_shape(p.hooks.as_ref());
            self.report_unused_property(
                fqn,
                &p.name,
                p.is_static,
                p.default.is_some(),
                only_readable,
                only_writable,
                p.doc_comment.as_deref(),
                p.span,
                &usage,
            );
        }
        if let Some(ctor) = ctor {
            for param in &ctor.function.params {
                if param.promoted_visibility != Some(Visibility::Private) {
                    continue;
                }
                let (only_readable, only_writable) = hook_read_write_shape(param.hooks.as_deref());
                // Promotion is itself an implicit `$this->x = $x` write -
                // see `check_unused_constructor_parameters`'s own doc
                // comment for the same reasoning applied to the parameter
                // side of this same construct.
                self.report_unused_property(
                    fqn,
                    &param.name,
                    false,
                    true,
                    only_readable,
                    only_writable,
                    None,
                    param.span,
                    &usage,
                );
            }
        }
    }

    /// The shared read/write-state-to-message logic behind
    /// `UnusedPrivatePropertyRule`'s five distinct shapes - see this
    /// module's own doc comment. `@phpstan-always-read`/`@phpstan-always-
    /// written` (checked directly against the raw doc-comment text, the
    /// same substring check real PHPStan's own configurable tag list
    /// uses) let a property genuinely read/written only through
    /// reflection, serialization, or a magic method suppress a false
    /// report the same way real PHPStan's extension point does.
    #[allow(clippy::too_many_arguments)]
    fn report_unused_property(
        &mut self,
        fqn: &str,
        name: &str,
        is_static: bool,
        seed_written: bool,
        only_readable: bool,
        only_writable: bool,
        doc_comment: Option<&str>,
        span: Span,
        usage: &MemberUsage,
    ) {
        let tag_always_read = doc_comment.is_some_and(|d| d.contains("@phpstan-always-read"));
        let tag_always_written = doc_comment.is_some_and(|d| d.contains("@phpstan-always-written"));

        let read = tag_always_read || usage.reads.contains(name);
        let written = tag_always_written || seed_written || usage.writes.contains(name);

        let kind_word = if is_static {
            "static property"
        } else {
            "property"
        };
        // Identifiers match real PHPStan's own UnusedPrivatePropertyRule
        // one-for-one (property.unused/.neverRead/.onlyWritten/
        // .neverWritten/.onlyRead - vendor/phpstan-src's own five
        // ->identifier(...) calls).
        let (message, identifier) = if !read {
            if !written {
                (
                    format!("private {kind_word} {fqn}::${name} is unused"),
                    "property.unused",
                )
            } else if only_readable {
                (
                    format!("private read-only {kind_word} {fqn}::${name} is never read"),
                    "property.neverRead",
                )
            } else {
                (
                    format!("private {kind_word} {fqn}::${name} is never read, only written"),
                    "property.onlyWritten",
                )
            }
        } else if !written {
            if only_writable {
                (
                    format!("private write-only {kind_word} {fqn}::${name} is never written"),
                    "property.neverWritten",
                )
            } else {
                (
                    format!("private {kind_word} {fqn}::${name} is never written, only read"),
                    "property.onlyRead",
                )
            }
        } else {
            return;
        };
        self.report(fqn.to_string(), message, span, 4, identifier);
    }
}

fn walk_stmt_body_for_members(
    body: &[Stmt],
    fqn: &str,
    ctx: &NameContext,
    usage: &mut MemberUsage,
) {
    for stmt in body {
        walk_stmt_for_members(stmt, fqn, ctx, usage);
    }
}
