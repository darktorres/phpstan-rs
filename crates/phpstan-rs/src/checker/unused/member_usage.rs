use syntax::name::NameSet;
use types::Name;

use syntax::ast::*;
use syntax::visit::arena::{self, Visitor};

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
#[derive(Default, Clone)]
pub(crate) struct MemberUsage {
    pub(crate) consts: NameSet,
    /// Lowercased (`method_key`).
    pub(crate) methods: NameSet,
    pub(crate) reads: NameSet,
    pub(crate) writes: NameSet,
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
    pub(crate) aliases: NameSet,
}

impl MemberUsage {
    /// Runs `scan` over a body twice: the second time every alias the first
    /// found is known from the start. Aliases are only ever added, so a
    /// first scan that found none ran with the alias set the second would
    /// have, and the second would find what the first did.
    pub(crate) fn scan_twice(&mut self, mut scan: impl FnMut(&mut MemberUsage)) {
        let known = self.aliases.len();
        scan(self);
        if self.aliases.len() != known {
            scan(self);
            return;
        }
        #[cfg(debug_assertions)]
        {
            let mut again = self.clone();
            scan(&mut again);
            assert_eq!(
                (
                    again.consts.len(),
                    again.methods.len(),
                    again.reads.len(),
                    again.writes.len(),
                    again.aliases.len(),
                    again.escaped
                ),
                (
                    self.consts.len(),
                    self.methods.len(),
                    self.reads.len(),
                    self.writes.len(),
                    self.aliases.len(),
                    self.escaped
                ),
                "a second scan of a body with no alias found something the first did not"
            );
        }
    }
}

/// Whether `class_ref` (the class name half of a `Foo::bar`/`self::bar`/
/// `static::bar` expression) refers to the class currently being
/// scanned - `self`/`static` always do (a private member is only ever
/// reachable from inside its own declaring class to begin with, so late
/// static binding through `static::` can't actually reach a *different*
/// class's private member); an explicit same-class reference (`Foo::BAR`
/// written inside `Foo` itself, unusual but legal) does too.
fn class_ref_is_self(class_ref: &QualifiedName, fqn: &str) -> bool {
    class_ref.is_keyword("self")
        || class_ref.is_keyword("static")
        || class_ref
            .resolved_class()
            .is_some_and(|resolved| resolved.eq_ignore_ascii_case(fqn))
}

fn target_is_self(target: &StaticTarget, fqn: &str) -> bool {
    target
        .as_name()
        .is_some_and(|class_ref| class_ref_is_self(class_ref, fqn))
}

/// A method name as `MemberUsage::methods` holds it: PHP method names are
/// case-insensitive.
pub(crate) fn method_key(name: &str) -> Name {
    Name::from(name.to_ascii_lowercase())
}

fn is_this(arenas: &NodeArenas, id: ExprId) -> bool {
    matches!(arenas.expr(id).map(|e| &e.kind), Some(ExprKind::Variable(name)) if **name == *"this")
}

/// `$this`, or a local variable syntactically proven (`usage.aliases`) to
/// hold an instance of the class currently being scanned - e.g.
/// `$sub = new self(); ...; $sub->privateMethod();`, or
/// `if ($x instanceof self) { ...; $x->privateProp; }`. See
/// `MemberUsage::aliases`'s own doc comment for how that set is built.
fn is_this_or_aliased(arenas: &NodeArenas, id: ExprId, usage: &MemberUsage) -> bool {
    is_this(arenas, id)
        || matches!(arenas.expr(id).map(|e| &e.kind), Some(ExprKind::Variable(name)) if usage.aliases.contains(name.as_ref()))
}

/// `new self(...)`/`new static(...)`/`new <SameClass>(...)` - the
/// declaration-based half of `MemberUsage::aliases`'s detection, paired
/// with the `Assign` arm below.
fn is_new_self_instance(arenas: &NodeArenas, id: ExprId, fqn: &str) -> bool {
    let Some(expr) = arenas.expr(id) else {
        return false;
    };
    let ExprKind::New(callee, _) = &expr.kind else {
        return false;
    };
    matches!(arenas.expr(*callee).map(|callee| &callee.kind), Some(ExprKind::Name(class_ref)) if arenas.qualified_name(class_ref).is_some_and(|class_ref| class_ref_is_self(class_ref, fqn)))
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
    arenas: &NodeArenas,
    id: ExprId,
    fqn: &str,
    usage: &mut MemberUsage,
    also_read: bool,
) -> bool {
    let Some(left) = arenas.expr(id) else {
        return false;
    };
    match &left.kind {
        ExprKind::PropertyAccess(base, name, _) if is_this_or_aliased(arenas, *base, usage) => {
            usage.writes.insert(Name::from(&name.name));
            if also_read {
                usage.reads.insert(Name::from(&name.name));
            }
            true
        }
        ExprKind::StaticPropertyFetch(target, prop) if target_is_self(target, fqn) => {
            usage.writes.insert(Name::from(&prop.name));
            if also_read {
                usage.reads.insert(Name::from(&prop.name));
            }
            true
        }
        _ => false,
    }
}

/// Records `usage` of the class `fqn`'s own members within one body. A
/// nested named function or class declaration is not entered, nor an
/// anonymous class's body: each has its own `$this` and `self`, and this
/// per-class syntactic walk has no scope-threading to keep them apart -
/// entering one would risk marking an outer member "used" because of an
/// unrelated same-named member access inside it. An anonymous class's
/// constructor arguments run in the enclosing scope and are walked.
struct Members<'u> {
    fqn: &'u str,
    usage: &'u mut MemberUsage,
}

impl Visitor for Members<'_> {
    fn visit_function_decl(&mut self, _: &NodeArenas, _: &FunctionDecl) {}

    fn visit_class_def(&mut self, _: &NodeArenas, _: &ClassDef) {}

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(expr) = arenas.expr(id) else {
            return;
        };
        syntax::stack::guard(|| {
            let (fqn, usage) = (self.fqn, &mut *self.usage);
            match &expr.kind {
                // A pure write target - `$this->x = ...` - is recorded as a
                // write only, not also as a read the way walking it as an
                // expression would.
                ExprKind::Assign(l, r) | ExprKind::AssignRef(l, r) => {
                    self.visit_expr(arenas, *r);
                    // `$var = new self()`/`new static()`/`new <SameClass>()`
                    // - see `MemberUsage::aliases`'s own doc comment.
                    if let Some(ExprKind::Variable(name)) = arenas.expr(*l).map(|left| &left.kind)
                        && is_new_self_instance(arenas, *r, self.fqn)
                    {
                        self.usage.aliases.insert(name.clone());
                    }
                    if !mark_write_target(arenas, *l, self.fqn, self.usage, false) {
                        self.visit_expr(arenas, *l);
                    }
                    return;
                }
                ExprKind::CompoundAssign(_, l, r) => {
                    self.visit_expr(arenas, *r);
                    if !mark_write_target(arenas, *l, self.fqn, self.usage, true) {
                        self.visit_expr(arenas, *l);
                    }
                    return;
                }
                ExprKind::PropertyAccess(base, name, _)
                    if is_this_or_aliased(arenas, *base, usage) =>
                {
                    usage.reads.insert(name.name.clone());
                }
                // `$var instanceof self`/`static`/`<SameClass>`, either
                // polarity, anywhere in the body - see `MemberUsage::
                // aliases`'s own doc comment. Deliberately not narrowed to
                // "only inside the branch this proves true in" (this whole
                // walker is flow-insensitive by design, e.g. `usage.reads`/
                // `usage.writes` are plain, unconditional sets too) - the
                // safe direction here, matching this project's "favor false
                // negatives [on `unused`] over false positives" default.
                ExprKind::Instanceof(inner, InstanceofTarget::Name(class_ref)) => {
                    if let Some(ExprKind::Variable(name)) =
                        arenas.expr(*inner).map(|inner| &inner.kind)
                        && arenas
                            .qualified_name(class_ref)
                            .is_some_and(|class_ref| class_ref_is_self(class_ref, fqn))
                    {
                        usage.aliases.insert(name.clone());
                    }
                }
                // `Foo::${$expr}` / `Foo::$$name`: can't know which property
                // this reaches, so the whole class's private-member checks
                // are skipped (see `MemberUsage`'s own doc comment).
                ExprKind::DynamicStaticPropertyFetch(..) => usage.escaped = true,
                ExprKind::StaticPropertyFetch(target, prop) => {
                    if target_is_self(target, fqn) {
                        usage.reads.insert(prop.name.clone());
                    }
                }
                ExprKind::ClassConstFetch(target, member) => {
                    if target_is_self(target, fqn) {
                        usage.consts.insert(member.name.clone());
                    }
                }
                ExprKind::DynamicClassConstFetch(target, name) => {
                    match arenas.expr(*name).map(|name| &name.kind) {
                        Some(ExprKind::Literal(Literal::String(member))) => {
                            if target_is_self(target, fqn) {
                                if let Some(bytes) = arenas.string_bytes(member) {
                                    usage.consts.insert(Name::from(
                                        String::from_utf8_lossy(bytes).into_owned(),
                                    ));
                                } else {
                                    usage.escaped = true;
                                }
                            }
                        }
                        _ => usage.escaped = true,
                    }
                }
                ExprKind::MethodCall(recv, name, ..)
                    if is_this_or_aliased(arenas, *recv, usage) =>
                {
                    usage.methods.insert(method_key(&name.name));
                }
                ExprKind::StaticCall(target, name, _) if target_is_self(target, fqn) => {
                    usage.methods.insert(method_key(&name.name));
                }
                // `$obj->{$expr}` / `$obj->$name(...)` - a fully dynamic
                // member access this walker can't attribute to any specific
                // member.
                ExprKind::DynamicPropertyAccess(..)
                | ExprKind::DynamicMethodCall(..)
                | ExprKind::DynamicStaticCall(..) => {
                    usage.escaped = true;
                }
                // `[$this, 'methodName']` - PHP's array-callable convention
                // (`usort($arr, [$this, 'compare'])`, `array_map([$this,
                // 'fmt'], ...)`, ...), common enough that skipping it would
                // be a real false-positive source for exactly the kind of
                // private helper method this check is meant to protect.
                ExprKind::ArrayLiteral(items) => {
                    let items = arenas.array_items(items.clone());
                    if let [a, b] = items
                        && a.key.is_none()
                        && b.key.is_none()
                        && !a.spread
                        && !b.spread
                        && (is_this_or_aliased(arenas, a.value, usage)
                            || matches!(arenas.expr(a.value).map(|e| &e.kind), Some(ExprKind::ClassNameFetch(target))
                                if target_is_self(target, fqn)))
                        && let Some(ExprKind::Literal(Literal::String(name))) =
                            arenas.expr(b.value).map(|e| &e.kind)
                    {
                        if let Some(bytes) = arenas.string_bytes(name) {
                            usage
                                .methods
                                .insert(method_key(&String::from_utf8_lossy(bytes)));
                        } else {
                            usage.escaped = true;
                        }
                    }
                }
                _ => {}
            }
            arena::walk_expr(self, arenas, id);
        })
    }
}

pub(crate) fn walk_expr_for_members(
    arenas: &NodeArenas,
    expr: ExprId,
    fqn: &str,
    usage: &mut MemberUsage,
) {
    Members { fqn, usage }.visit_expr(arenas, expr);
}

pub(crate) fn walk_property_hooks_for_members(
    arenas: &NodeArenas,
    hooks: &PropertyHooks,
    fqn: &str,
    usage: &mut MemberUsage,
) {
    arena::walk_property_hooks(&mut Members { fqn, usage }, arenas, hooks);
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
    arenas: &NodeArenas,
    body: &[StmtId],
    fqn: &str,
    usage: &mut MemberUsage,
) {
    arena::walk_stmts(&mut Members { fqn, usage }, arenas, body);
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
    arenas: &NodeArenas,
    params: &[Param],
    fqn: &str,
    usage: &mut MemberUsage,
) {
    for p in params {
        let is_self = p.type_hint.as_ref().is_some_and(|hint| match &hint.kind {
            TypeHintKind::Keyword(NativeType::SelfType | NativeType::Static, _) => true,
            TypeHintKind::Name(name) => arenas
                .qualified_names(name)
                .first()
                .is_some_and(|name| class_ref_is_self(name, fqn)),
            _ => false,
        });
        if is_self {
            usage.aliases.insert(p.name.name.clone());
        }
    }
}
