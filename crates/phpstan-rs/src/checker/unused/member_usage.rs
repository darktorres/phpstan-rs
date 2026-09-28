use rustc_hash::FxHashSet;
use std::sync::Arc;

use syntax::ast::*;
use syntax::intern::intern;
use syntax::name_resolution::NameContext;
use syntax::visit::{any_child_expr, any_child_stmt};

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
pub(crate) struct MemberUsage {
    pub(crate) consts: FxHashSet<Arc<str>>,
    /// Lowercased (`method_key`).
    pub(crate) methods: FxHashSet<Arc<str>>,
    pub(crate) reads: FxHashSet<Arc<str>>,
    pub(crate) writes: FxHashSet<Arc<str>>,
    pub(crate) escaped: bool,
    /// Local variable names, scoped to the one function/hook body
    /// currently being scanned, syntactically proven (never via real type
    /// inference - this stays a flat, flow-insensitive "was this name
    /// ever established as self-typed anywhere in this body" set, at the
    /// same precision level the rest of this file already uses) to hold
    /// an instance of the class being scanned - see
    /// `is_this_or_aliased`/`is_new_self_instance`/
    /// `seed_self_typed_param_aliases`. Variable names are per-scope, not
    /// class-wide, so the caller (`check_unused_private_members`) clears
    /// this before each function/hook body's own scan rather than letting
    /// it accumulate across bodies.
    pub(crate) aliases: FxHashSet<Arc<str>>,
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

fn target_is_self(target: &StaticTarget, fqn: &str, ctx: &NameContext) -> bool {
    target
        .as_name()
        .is_some_and(|class_ref| class_ref_is_self(class_ref, fqn, ctx))
}

/// A method name as `MemberUsage::methods` holds it: PHP method names are
/// case-insensitive.
pub(crate) fn method_key(name: &str) -> Arc<str> {
    Arc::from(name.to_ascii_lowercase())
}

fn is_this(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Variable(name) if **name == *"this")
}

/// `$this`, or a local variable syntactically proven (`usage.aliases`) to
/// hold an instance of the class currently being scanned - e.g.
/// `$sub = new self(); ...; $sub->privateMethod();`, or
/// `if ($x instanceof self) { ...; $x->privateProp; }`. See
/// `MemberUsage::aliases`'s own doc comment for how that set is built.
fn is_this_or_aliased(e: &Expr, usage: &MemberUsage) -> bool {
    is_this(e)
        || matches!(&e.kind, ExprKind::Variable(name) if usage.aliases.contains(name.as_ref()))
}

/// `new self(...)`/`new static(...)`/`new <SameClass>(...)` - the
/// declaration-based half of `MemberUsage::aliases`'s detection, paired
/// with the `Assign` arm below.
fn is_new_self_instance(e: &Expr, fqn: &str, ctx: &NameContext) -> bool {
    matches!(&e.kind, ExprKind::New(callee, _)
        if matches!(&callee.kind, ExprKind::Name(class_ref) if class_ref_is_self(class_ref, fqn, ctx)))
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
        ExprKind::PropertyAccess(base, name, _) if is_this_or_aliased(base, usage) => {
            usage.writes.insert(intern(&name.name));
            if also_read {
                usage.reads.insert(intern(&name.name));
            }
            true
        }
        ExprKind::StaticPropertyFetch(target, prop) if target_is_self(target, fqn, ctx) => {
            usage.writes.insert(intern(&prop.name));
            if also_read {
                usage.reads.insert(intern(&prop.name));
            }
            true
        }
        _ => false,
    }
}

fn walk_stmt_for_members(stmt: &Stmt, fqn: &str, ctx: &NameContext, usage: &mut MemberUsage) {
    syntax::stack::guard(|| {
        match &stmt.kind {
            StmtKind::Expression(e) | StmtKind::Throw(e) => {
                walk_expr_for_members(e, fqn, ctx, usage)
            }
            StmtKind::Return(Some(e)) => walk_expr_for_members(e, fqn, ctx, usage),
            StmtKind::Echo(exprs) | StmtKind::Unset(exprs) => {
                for e in exprs {
                    walk_expr_for_members(e, fqn, ctx, usage);
                }
            }
            StmtKind::If(if_stmt) => {
                for cond in if_stmt.conditions() {
                    walk_expr_for_members(cond, fqn, ctx, usage);
                }
            }
            StmtKind::While { cond, .. } | StmtKind::DoWhile { cond, .. } => {
                walk_expr_for_members(cond, fqn, ctx, usage)
            }
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
        any_child_stmt(stmt, |child| {
            walk_stmt_for_members(child, fqn, ctx, usage);
            false
        });
    })
}

pub(crate) fn walk_expr_for_members(
    expr: &Expr,
    fqn: &str,
    ctx: &NameContext,
    usage: &mut MemberUsage,
) {
    syntax::stack::guard(|| {
        match &expr.kind {
            // Handled explicitly (and `return`s below, skipping the generic
            // `any_child_expr` recursion at the bottom of this function)
            // specifically so a pure write target - `$this->x = ...` - is
            // recorded as a write only, not *also* as a read the way falling
            // through to the generic `PropertyAccess`/`ClassConstFetch`/`StaticPropertyFetch` arms
            // further down would.
            ExprKind::Assign(l, r) | ExprKind::AssignRef(l, r) => {
                walk_expr_for_members(r, fqn, ctx, usage);
                // `$var = new self()`/`new static()`/`new <SameClass>()` -
                // see `MemberUsage::aliases`'s own doc comment. Moot for
                // `AssignRef` in practice (real PHP rejects `=& new ...`),
                // but harmless to still check.
                if let ExprKind::Variable(name) = &l.kind
                    && is_new_self_instance(r, fqn, ctx)
                {
                    usage.aliases.insert(name.clone());
                }
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
            ExprKind::PropertyAccess(base, name, _) if is_this_or_aliased(base, usage) => {
                usage.reads.insert(intern(&name.name));
            }
            // `$var instanceof self`/`static`/`<SameClass>`, either polarity,
            // anywhere in the body - see `MemberUsage::aliases`'s own doc
            // comment. Deliberately not narrowed to "only inside the branch
            // this proves true in" (this whole walker is flow-insensitive by
            // design already, e.g. `usage.reads`/`usage.writes` are plain,
            // unconditional sets too) - the safe direction here, matching
            // this project's "favor false negatives [on `unused`] over false
            // positives" default.
            ExprKind::Instanceof(inner, InstanceofTarget::Name(class_ref, _)) => {
                if let ExprKind::Variable(name) = &inner.kind
                    && class_ref_is_self(class_ref, fqn, ctx)
                {
                    usage.aliases.insert(name.clone());
                }
            }
            // `Foo::${$expr}` / `Foo::$$name`: can't know which property
            // this reaches, so the whole class's private-member checks are
            // skipped (see `MemberUsage`'s own doc comment).
            ExprKind::DynamicStaticPropertyFetch(..) => usage.escaped = true,
            ExprKind::StaticPropertyFetch(target, prop) => {
                if target_is_self(target, fqn, ctx) {
                    usage.reads.insert(intern(&prop.name));
                }
            }
            ExprKind::ClassConstFetch(target, member) => {
                if target_is_self(target, fqn, ctx) {
                    usage.consts.insert(intern(&member.name));
                }
            }
            ExprKind::DynamicClassConstFetch(target, name) => match &name.kind {
                ExprKind::Literal(Literal::String(member)) => {
                    if target_is_self(target, fqn, ctx) {
                        usage.consts.insert(Arc::from(&*member.to_str_lossy()));
                    }
                }
                _ => usage.escaped = true,
            },
            ExprKind::MethodCall(recv, name, ..) if is_this_or_aliased(recv, usage) => {
                usage.methods.insert(method_key(&name.name));
            }
            ExprKind::StaticCall(target, name, _) if target_is_self(target, fqn, ctx) => {
                usage.methods.insert(method_key(&name.name));
            }
            // `$obj->{$expr}` / `$obj->$name(...)` - a fully dynamic member
            // access this walker can't attribute to any specific member.
            ExprKind::DynamicPropertyAccess(..)
            | ExprKind::DynamicMethodCall(..)
            | ExprKind::DynamicStaticCall(..) => {
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
                    && (is_this_or_aliased(&a.value, usage)
                        || matches!(&a.value.kind, ExprKind::ClassNameFetch(target)
                            if target_is_self(target, fqn, ctx)))
                    && let ExprKind::Literal(Literal::String(name)) = &b.value.kind
                {
                    usage.methods.insert(method_key(&name.to_str_lossy()));
                }
            }
            // `any_child_expr` deliberately returns no children for these
            // three (see `visit`'s own module doc comment - each consumer
            // needs its own explicit handling since they don't fit the
            // generic `Vec<&Expr>` shape). Without these arms, a private
            // member referenced only from inside a closure/arrow function
            // (`array_map`/`usort`/`preg_replace_callback`/
            // `register_shutdown_function`/a transactional closure, ...)
            // would otherwise be invisible to this walker and wrongly
            // reported unused.
            ExprKind::Closure { params, body, .. } => {
                for p in params {
                    if let Some(d) = &p.default {
                        walk_expr_for_members(d, fqn, ctx, usage);
                    }
                }
                walk_stmt_body_for_members(body, fqn, ctx, usage);
                return;
            }
            ExprKind::ArrowFn { params, body, .. } => {
                for p in params {
                    if let Some(d) = &p.default {
                        walk_expr_for_members(d, fqn, ctx, usage);
                    }
                }
                walk_expr_for_members(body, fqn, ctx, usage);
                return;
            }
            // The anonymous class's own constructor `args` genuinely run in
            // the *enclosing* scope (so `$this->x`/`self::CONST` there really
            // is outer-class usage) and are walked. Its own class body is a
            // deliberate exclusion: that body has its own, different
            // `$this`/`self` binding, and this per-class syntactic walker has
            // no scope-threading to keep the two apart safely - walking it
            // would risk the wrong-direction bug (marking an outer member
            // "used" because of an unrelated same-named member access inside
            // the anonymous class).
            ExprKind::NewAnonymousClass(_, args) => {
                for a in args {
                    walk_expr_for_members(&a.value, fqn, ctx, usage);
                }
                return;
            }
            _ => {}
        }
        any_child_expr(expr, |child| {
            walk_expr_for_members(child, fqn, ctx, usage);
            false
        });
    })
}

pub(crate) fn walk_property_hooks_for_members(
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
        PropertyHookBody::Abstract => {}
    }
}

/// A `get`-only property hook (`{ get => ...; }`, no `set`) can't be
/// assigned through the property syntax at all, so "never written"
/// deserves its own wording (`property.neverWritten`-shaped, mirroring
/// real PHPStan) rather than the generic "only read" one; symmetrically
/// for a `set`-only hook and "never read". Neither is a property this
/// checker's parser can't represent - see `ast::PropertyHooks`.
pub(crate) fn hook_read_write_shape(hooks: Option<&PropertyHooks>) -> (bool, bool) {
    match hooks {
        Some(h) => (
            h.get.is_some() && h.set.is_none(),
            h.set.is_some() && h.get.is_none(),
        ),
        None => (false, false),
    }
}

pub(crate) fn walk_stmt_body_for_members(
    body: &[Stmt],
    fqn: &str,
    ctx: &NameContext,
    usage: &mut MemberUsage,
) {
    for stmt in body {
        walk_stmt_for_members(stmt, fqn, ctx, usage);
    }
}

/// The declaration-based half of `MemberUsage::aliases`'s detection: a
/// parameter explicitly typed `self`/`static`/the declaring class itself
/// (e.g. a comparison/merge/copy method taking a same-typed argument) is
/// as much a same-class instance as one proven via `new self()`/
/// `instanceof self`. Deliberately only the exact-name case - a nullable
/// (`?self`) or union (`self|null`) type hint is skipped, matching this
/// project's existing "conservative when the shape gets more complex"
/// direction elsewhere in this same file.
pub(crate) fn seed_self_typed_param_aliases(
    params: &[Param],
    fqn: &str,
    ctx: &NameContext,
    usage: &mut MemberUsage,
) {
    for p in params {
        if p.type_hint
            .as_ref()
            .map(TypeHint::to_string)
            .as_deref()
            .is_some_and(|t| class_ref_is_self(t, fqn, ctx))
        {
            usage.aliases.insert(p.name.clone());
        }
    }
}
