//! The transitive-purity half: a registry-build-time fixed point over
//! the whole project's own call graph, deciding which
//! functions/methods/constructors are provably effect-free - either
//! directly (an empty/pure-arithmetic body) or transitively (every call
//! they make resolves to something *else* already proven effect-free).
//!
//! **The real architecture**: a plain, monotonic fixed point over a
//! dependency graph, run once, immediately after every class is
//! indexed - the same placement `flatten_phpdoc_inheritance`/
//! `resolve_deferred_value_of` already established for "needs the whole
//! project visible first" passes.
//!
//! **A disclosed precision restriction**: real call-target resolution
//! for `MethodCall`/`StaticCall` depends on a live, fully-inferred
//! `Scope` - machinery that belongs to this project's own per-file
//! `Checker`, which doesn't exist yet at `Registry::build` time. So this
//! module only ever resolves a call whose target is knowable without
//! type inference: `$this->method()` (virtual dispatch against the
//! enclosing class), `self::`/`static::`/`parent::method()`
//! (lexical/virtual resolution against the enclosing class - the exact
//! same shapes `checker/uninitialized_dataflow.rs` already resolves,
//! for the identical reason), `new self()`/`new static()`/`new
//! LiteralClassName()`, and a plain function call by name. Any other
//! call shape (through a local variable, a property, a parameter,
//! another call's own return value) is treated as unconditionally
//! unresolvable, disqualifying the whole declaration - a precision loss
//! (a legitimately pure method reached only through a non-`$this`
//! receiver is missed), never a soundness one, matching this project's
//! "favor false negatives over false positives" direction throughout.
//!
//! Every disqualifying-construct trigger condition and identifier below
//! (`echo`/`die`/`exit`/`propertyAssign`/`propertyAssignByRef`/
//! `propertyUnset`/`include`/`require`/`print`/`eval`/`superglobal`/
//! `yield`/`yieldFrom`/`static`/`global`/`betweenPhpTags`/
//! `staticPropertyAccess`) mirrors real PHPStan's own impure-point
//! taxonomy.

use rayon::prelude::*;
use std::sync::Arc;

use crate::ast::*;
use crate::intern::intern;
use crate::registry::{FunctionSig, Registry};
use crate::token::Span;
use crate::types::Type;
use crate::visit::{child_exprs, child_stmts};

/// One real, resolved call target a declaration's own effect-freedom is
/// conditional on - `Some(vec![])` (no dependencies at all) is the base
/// case a fixed point grows outward from; `None` (not this type, see
/// `FunctionSig::purity_dependencies`) means "can never be proven
/// effect-free, no matter what". Both variants store a lower-cased key
/// (matching real PHPStan's own `functionKey`/`methodKey`, which
/// `strtolower`s both parts) - so the same declaration referenced under
/// different call-site casing always resolves to the same graph node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PurityDependency {
    Function(Arc<str>),
    Method(Arc<str>, Arc<str>),
}

impl Registry {
    /// Wired into `Registry::build` immediately after
    /// `resolve_deferred_value_of` (and into `Project`'s own incremental
    /// single-file-reload path, the same precedent) - re-runs from
    /// scratch on every reload rather than incrementally recomputing,
    /// prioritizing correctness over performance, the same tradeoff
    /// `resolve_deferred_value_of` already made.
    #[hotpath::measure]
    pub(crate) fn resolve_transitive_purity(&mut self) {
        self.classify_purity();
        self.run_purity_fixed_point();
    }

    /// Populates `purity_dependencies` on every function/method this
    /// project's own registry has a real body for - gated the same way
    /// real PHPStan's own `*WithoutImpurePointsCollector`s gate before
    /// ever calling `resolveDependencies`: no by-reference parameter (a
    /// disclosed simplification of real PHPStan's own
    /// `createsNewVariable()` distinction - see the gating check below
    /// for why treating every by-ref parameter as disqualifying is the
    /// safe direction here), no `@phpstan-assert`-family tag, and no
    /// declared throw. This project has no `@phpstan-pure`/
    /// `@phpstan-impure`/`hasSideEffects` annotation surface at all, so
    /// unlike real PHPStan's own gate, nothing here is ever
    /// pre-disqualified by an existing annotation - every declaration
    /// starts "maybe pure" until its own body says otherwise.
    /// `classify_body` only ever *reads* the registry (resolving
    /// `$this`/`self`/`parent`/`static`/plain-function call targets, see
    /// its own doc comment) - the write
    /// (`...purity_dependencies = deps`) happens after, per function/
    /// method, to a disjoint key, so this parallelizes with the same
    /// reborrow-as-`&Registry` technique `flatten_traits_one_pass`/
    /// `flatten_phpdoc_inheritance_one_pass` already use. Two separate
    /// parallel passes (functions, then class methods) rather than one
    /// combined one, matching the two loops this replaces - each needs
    /// its own fresh reborrow since the sequential apply loop in between
    /// needs `&mut self` again.
    #[allow(clippy::type_complexity)]
    fn classify_purity(&mut self) {
        let function_names: Vec<Arc<str>> = self.functions.keys().cloned().collect();
        let registry: &Registry = self;
        let function_deps: Vec<(Arc<str>, Option<Vec<PurityDependency>>)> = function_names
            .par_iter()
            .filter_map(|name| {
                let sig = &registry.functions[name];
                if !purity_gate_passes(sig, false) {
                    return None;
                }
                let body = sig.body.clone()?;
                let deps = classify_body(&body, None, registry);
                Some((name.clone(), deps))
            })
            .collect();
        for (name, deps) in function_deps {
            self.functions.get_mut(&name).unwrap().purity_dependencies = deps;
        }

        let class_names: Vec<Arc<str>> = self.classes.keys().cloned().collect();
        let registry: &Registry = self;
        let method_deps: Vec<(Arc<str>, Arc<str>, Option<Vec<PurityDependency>>)> = class_names
            .par_iter()
            .flat_map(|class_name| {
                let method_names: Vec<Arc<str>> = registry.classes[class_name]
                    .methods
                    .keys()
                    .cloned()
                    .collect();
                let mut per_class = Vec::new();
                for method_name in method_names {
                    let sig = &registry.classes[class_name].methods[&method_name];
                    if !purity_gate_passes(sig, method_name.eq_ignore_ascii_case("__construct")) {
                        continue;
                    }
                    let Some(body) = sig.body.clone() else {
                        continue;
                    };
                    let deps = classify_body(&body, Some(class_name), registry);
                    per_class.push((class_name.clone(), method_name, deps));
                }
                per_class
            })
            .collect();
        for (class_name, method_name, deps) in method_deps {
            self.classes
                .get_mut(&class_name)
                .unwrap()
                .methods
                .get_mut(&method_name)
                .unwrap()
                .purity_dependencies = deps;
        }
    }

    /// A plain, monotonic fixed point (bounded by declaration count, no
    /// `MAX_PASSES` cap needed - this is a monotonically growing set
    /// over a finite graph, not approximating a possibly-nonconvergent
    /// narrowing fixed point): mark a declaration pure once every one of
    /// its own dependencies is already marked pure, repeat until one
    /// full pass makes no change. A dependency cycle with no external
    /// pure base case is correctly never marked pure, matching real
    /// PHPStan's own identical behavior for the same shape.
    fn run_purity_fixed_point(&mut self) {
        loop {
            let mut changed = false;
            let function_names: Vec<Arc<str>> = self.functions.keys().cloned().collect();
            for name in function_names {
                let sig = &self.functions[&name];
                if sig.is_transitively_pure {
                    continue;
                }
                let Some(deps) = &sig.purity_dependencies else {
                    continue;
                };
                if deps.iter().all(|d| self.dependency_is_pure(d)) {
                    self.functions.get_mut(&name).unwrap().is_transitively_pure = true;
                    changed = true;
                }
            }
            let class_names: Vec<Arc<str>> = self.classes.keys().cloned().collect();
            for class_name in class_names {
                let method_names: Vec<Arc<str>> =
                    self.classes[&class_name].methods.keys().cloned().collect();
                for method_name in method_names {
                    let sig = &self.classes[&class_name].methods[&method_name];
                    if sig.is_transitively_pure {
                        continue;
                    }
                    let Some(deps) = &sig.purity_dependencies else {
                        continue;
                    };
                    if deps.iter().all(|d| self.dependency_is_pure(d)) {
                        self.classes
                            .get_mut(&class_name)
                            .unwrap()
                            .methods
                            .get_mut(&method_name)
                            .unwrap()
                            .is_transitively_pure = true;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn dependency_is_pure(&self, dep: &PurityDependency) -> bool {
        match dep {
            PurityDependency::Function(name) => self
                .functions
                .get(name)
                .is_some_and(|sig| sig.is_transitively_pure),
            PurityDependency::Method(class, method) => self
                .classes
                .get(class)
                .and_then(|info| info.methods.get(method))
                .is_some_and(|sig| sig.is_transitively_pure),
        }
    }
}

/// Real PHPStan's own pre-`resolveDependencies` gate: no by-reference
/// parameter, no `@phpstan-assert`-family tag, no declared throw. Every
/// one of this project's own `FunctionSig`s already carries exactly the
/// fields needed to check this without re-deriving anything.
///
/// Real PHPStan's own `hasSideEffects()` unconditionally returns
/// "definitely has side effects" for any non-constructor declaration
/// with a `void` return type, regardless of its actual body. This is
/// deliberate design, not an oversight: a `void` declaration's only
/// possible reason to exist is its side effects, so PHPStan treats an
/// empty-bodied `void` method as presumed-intentional future-extension
/// scaffolding, never a "this does nothing" finding. Without an
/// equivalent exclusion, `is_transitively_pure` would wrongly mark
/// every void, empty/pure-bodied method/function pure - a real
/// false-positive source, e.g. `void`-returning stub/hook methods like
/// deliberately-empty boot-sequence placeholders, or `parent::setUp()`/
/// `tearDown()`/`tearDownAfterClass()` calls into PHPUnit's own
/// currently-empty base hooks. `is_constructor` mirrors the real
/// gate's own explicit `getName() !== '__construct'` exemption
/// (constructors can't syntactically declare a `void` return type, but
/// a stray `@return void` PHPDoc tag on one is a real possibility this
/// project's own `return_type` field doesn't otherwise guard against).
fn purity_gate_passes(sig: &FunctionSig, is_constructor: bool) -> bool {
    // See `FunctionSig::is_stub`'s own doc comment - a vendored stub's
    // own body is always empty and never real; walking it would wrongly
    // "prove" every PHP builtin pure. Checked first, not that it matters
    // for correctness (the other checks would eventually reject some of
    // them too, but not all - `header()`'s own stub signature has no
    // by-ref param, no assert tag, no declared throw, so without this
    // check it would sail straight through every other gate).
    if sig.is_stub {
        return false;
    }
    if !is_constructor && sig.return_type == Type::Void {
        return false;
    }
    if sig.params.iter().any(|p| p.by_ref) {
        return false;
    }
    if !sig.assert_if_true.is_empty() || !sig.assert_if_false.is_empty() || !sig.assert.is_empty() {
        return false;
    }
    if sig.throws.is_some() {
        return false;
    }
    true
}

/// Walks a declaration's own body once, returning its dependency list
/// (`Some(deps)`, possibly empty) or `None` the moment any disqualifying
/// construct or unresolvable call is found - matches real PHPStan's own
/// "one impure point/one unresolvable call disqualifies the whole
/// declaration" rule exactly, not a per-statement partial result.
/// `declaring_class` is `None` for a plain top-level function, `Some`
/// for a method/constructor - needed to resolve `$this->`/`self::`/
/// `static::`/`parent::`/`new self()`/`new static()`.
fn classify_body(
    body: &[Stmt],
    declaring_class: Option<&str>,
    registry: &Registry,
) -> Option<Vec<PurityDependency>> {
    let mut deps: Vec<PurityDependency> = Vec::new();
    if body
        .iter()
        .any(|s| stmt_disqualifies(s, declaring_class, registry, &mut deps))
    {
        return None;
    }
    deps.sort();
    deps.dedup();
    Some(deps)
}

fn stmt_disqualifies(
    stmt: &Stmt,
    declaring_class: Option<&str>,
    registry: &Registry,
    deps: &mut Vec<PurityDependency>,
) -> bool {
    let own = match &stmt.kind {
        // `echo`/`die`/`exit`/`print` are `betweenPhpTags`'s own
        // "output" siblings - real PHPStan's own separate `echo`
        // identifier for the statement form specifically.
        StmtKind::Echo(_) => true,
        // An *explicit* `throw` - matches `resolveDependencies`'s own
        // `$throwPoint->isExplicit()` check exactly (an implicit throw
        // from a called function is a different, unmodeled concern,
        // same as real PHPStan's own treatment).
        StmtKind::Throw(_) => true,
        StmtKind::Global(_) => true,
        // `static $x = ...;` - a function-local variable persisting
        // across calls, real PHPStan's own `'static'` identifier
        // (distinct from `self`/`static::` class-name resolution,
        // which never touches this statement kind at all).
        StmtKind::StaticVar(_) => true,
        // Real output between PHP tags - `betweenPhpTags`.
        StmtKind::InlineHtml(_) => true,
        StmtKind::Expression(e) => expr_disqualifies(e, declaring_class, registry, deps),
        StmtKind::Return(Some(e)) => expr_disqualifies(e, declaring_class, registry, deps),
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => {
            expr_disqualifies(cond, declaring_class, registry, deps)
        }
        StmtKind::For {
            init, cond, step, ..
        } => init
            .iter()
            .chain(cond)
            .chain(step)
            .any(|e| expr_disqualifies(e, declaring_class, registry, deps)),
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            expr_disqualifies(subject, declaring_class, registry, deps)
                || key
                    .as_ref()
                    .is_some_and(|k| expr_disqualifies(k, declaring_class, registry, deps))
                || expr_disqualifies(value, declaring_class, registry, deps)
        }
        StmtKind::Switch { subject, cases } => {
            expr_disqualifies(subject, declaring_class, registry, deps)
                || cases.iter().any(|c| {
                    c.test
                        .as_ref()
                        .is_some_and(|t| expr_disqualifies(t, declaring_class, registry, deps))
                })
        }
        StmtKind::GlobalConst(consts) => consts
            .iter()
            .any(|c| expr_disqualifies(&c.value, declaring_class, registry, deps)),
        _ => false,
    };
    own || child_stmts(stmt)
        .into_iter()
        .any(|s| stmt_disqualifies(s, declaring_class, registry, deps))
}

fn expr_disqualifies(
    e: &Expr,
    declaring_class: Option<&str>,
    registry: &Registry,
    deps: &mut Vec<PurityDependency>,
) -> bool {
    match &e.kind {
        // Any static property access at all - real PHPStan's own
        // `staticPropertyAccess` fires even for a bare read of shared
        // static state, not just a write.
        ExprKind::ClassConstFetch(..)
        | ExprKind::ClassNameFetch(_)
        | ExprKind::StaticPropertyFetch(..)
        | ExprKind::DynamicStaticPropertyFetch(..) => true,
        ExprKind::Variable(name) if is_superglobal(name) => true,
        ExprKind::Yield(..) | ExprKind::YieldFrom(_) => true,
        ExprKind::Assign(lhs, rhs) | ExprKind::AssignRef(lhs, rhs) => {
            property_write_target(lhs)
                || expr_disqualifies(lhs, declaring_class, registry, deps)
                || expr_disqualifies(rhs, declaring_class, registry, deps)
        }
        ExprKind::CompoundAssign(_, lhs, rhs) => {
            property_write_target(lhs)
                || expr_disqualifies(lhs, declaring_class, registry, deps)
                || expr_disqualifies(rhs, declaring_class, registry, deps)
        }
        ExprKind::Call(callee, args) => {
            if let ExprKind::Name(name) = &callee.kind {
                let lname = name.trim_start_matches('\\').to_ascii_lowercase();
                match lname.as_str() {
                    // Never disqualifying on their own - `unset` only
                    // disqualifies when it targets a property
                    // (`propertyUnset`); a plain-variable `unset` has no
                    // impure point in real PHPStan's own model at all.
                    "isset" | "empty" => {
                        return args
                            .iter()
                            .any(|a| expr_disqualifies(&a.value, declaring_class, registry, deps));
                    }
                    "unset" => {
                        return args.iter().any(|a| {
                            property_write_target(&a.value)
                                || expr_disqualifies(&a.value, declaring_class, registry, deps)
                        });
                    }
                    "die" | "exit" | "print" | "eval" | "include" | "include_once" | "require"
                    | "require_once" => return true,
                    _ => {}
                }
                if args
                    .iter()
                    .any(|a| expr_disqualifies(&a.value, declaring_class, registry, deps))
                {
                    return true;
                }
                // Canonical declared name, not `lname` - see
                // `resolve_this_method_call`'s own doc comment for why
                // storing a lowercased key here would silently break
                // `dependency_is_pure`'s later exact lookup for any
                // function whose real declared name isn't already
                // all-lowercase.
                let Some((canonical_name, _)) = registry.lookup_function_with_name(name) else {
                    return true;
                };
                deps.push(PurityDependency::Function(intern(canonical_name)));
                return false;
            }
            true
        }
        ExprKind::MethodCall(recv, name, args, _) => {
            if args
                .iter()
                .any(|a| expr_disqualifies(&a.value, declaring_class, registry, deps))
            {
                return true;
            }
            if !matches!(&recv.kind, ExprKind::Variable(v) if **v == *"this") {
                return true;
            }
            let Some(class) = declaring_class else {
                return true;
            };
            resolve_this_method_call(class, name, registry, deps)
        }
        ExprKind::StaticCall(target, name, args) => {
            if args
                .iter()
                .any(|a| expr_disqualifies(&a.value, declaring_class, registry, deps))
            {
                return true;
            }
            let Some(class) = declaring_class else {
                return true;
            };
            // A dynamic receiver is never one of the resolvable
            // pseudo-keywords: unresolvable, so disqualifying.
            let Some(target) = target.as_name() else {
                return true;
            };
            resolve_static_call(class, target, name, registry, deps)
        }
        // A computed method name: any method, so unresolvable.
        ExprKind::DynamicStaticCall(..) => true,
        ExprKind::New(callee, args) => {
            if args
                .iter()
                .any(|a| expr_disqualifies(&a.value, declaring_class, registry, deps))
            {
                return true;
            }
            let ExprKind::Name(name) = &callee.kind else {
                return true;
            };
            let target_class = match name.trim_start_matches('\\') {
                n if n.eq_ignore_ascii_case("self") || n.eq_ignore_ascii_case("static") => {
                    let Some(class) = declaring_class else {
                        return true;
                    };
                    class.to_string()
                }
                n => n.to_string(),
            };
            resolve_new(&target_class, registry, deps)
        }
        _ => child_exprs(e)
            .into_iter()
            .any(|c| expr_disqualifies(c, declaring_class, registry, deps)),
    }
}

/// `$obj->prop = ...`/`unset($obj->prop)` - a property write/unset on
/// *any* receiver, not just `$this`, matches real PHPStan's own
/// `propertyAssign`/`propertyUnset` exactly (mutating any object's own
/// property is an observable side effect regardless of which object).
/// Also recognizes a property reached through any depth of array-index
/// nesting (`$this->arr[] = $x;`/`$this->arr[$k] = $x;`/
/// `$this->matrix[$i][$j] = $x;`): the underlying property is still
/// being mutated, through the exact same reference, whether the
/// assignment target is written as a bare `PropertyAccess` or an
/// `Index` chain ending in one.
fn property_write_target(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::PropertyAccess(..) | ExprKind::DynamicPropertyAccess(..) => true,
        ExprKind::Index(base, _) => property_write_target(base),
        _ => false,
    }
}

/// `$this->method(...)` - virtual dispatch against `class` (the
/// enclosing declaration's own class), matching real PHPStan's own
/// `MethodCall` handling: usable only when the resolved method is
/// non-overridable (private, final, on a final class, or `class` itself
/// - `$this`'s own static type - is final).
///
/// `owner`/`method` must be pushed exactly as returned by
/// `lookup_method_with_owner`, without lowercasing: unlike real
/// PHPStan's own key encoding (which lowercases both construction and
/// lookup consistently), this project's `self.classes`/`ClassInfo.
/// methods` are keyed by whatever case each declaration actually used
/// (PascalCase by PHP convention). Lowercasing before storing would
/// break every multi-hop dependency chain through any class name
/// containing an uppercase letter - `dependency_is_pure`'s later
/// `self.classes.get(dep_class)` would silently miss, permanently
/// stalling the fixed point for that dependency.
fn resolve_this_method_call(
    class: &str,
    method: &str,
    registry: &Registry,
    deps: &mut Vec<PurityDependency>,
) -> bool {
    let Some((owner, sig)) = registry.lookup_method_with_owner(class, method) else {
        return true;
    };
    if !is_non_overridable(registry, class, &owner, method, sig) {
        return true;
    }
    deps.push(PurityDependency::Method(owner.clone(), intern(method)));
    false
}

/// Whether a call targeting `sig` (declared on `owner`, reached through
/// a receiver whose own static type is `calling_class`) is safe to
/// treat as calling *exactly* `sig` - private, final, on a final class,
/// or `calling_class` itself final (in which case no subclass could
/// exist to override anything). `pub(crate)`: also used by
/// `checker::noop::check_pure_call_without_effect`, the *consuming*
/// diagnostic - real PHPStan's own `PossiblyPureMethodCallCollector`
/// (the thing that decides whether a bare `$obj->method();` statement
/// is even a *candidate* worth checking against the pure set) applies
/// this exact same overridability gate a second time, independently of
/// whatever `resolveCall` already required to classify `sig` itself as
/// pure - the two questions are genuinely different (one is "can this
/// declaration ever be proven effect-free", the other is "does *this*
/// call site definitely reach that exact declaration, not some
/// overriding one") and both must hold.
pub(crate) fn is_non_overridable(
    registry: &Registry,
    calling_class: &str,
    owner: &str,
    method: &str,
    sig: &FunctionSig,
) -> bool {
    if sig.visibility == Visibility::Private {
        return true;
    }
    if registry
        .classes
        .get(owner)
        .is_some_and(|info| info.final_methods.contains(method) || info.is_final)
    {
        return true;
    }
    registry
        .classes
        .get(calling_class)
        .is_some_and(|info| info.is_final)
}

/// `self::`/`static::`/`parent::method(...)` - lexically (`self`/
/// `parent`) or virtually (`static`) resolved against `class`, matching
/// real PHPStan's own `StaticCall` handling exactly: unlike `MethodCall`
/// above, no overridability check at all (read directly from source,
/// not assumed) - a `self::`/`parent::` call is lexically bound to one
/// exact declaration, never ambiguous the way an arbitrary receiver's
/// method call can be.
fn resolve_static_call(
    class: &str,
    class_name: &str,
    method: &str,
    registry: &Registry,
    deps: &mut Vec<PurityDependency>,
) -> bool {
    let resolved = if class_name.eq_ignore_ascii_case("parent") {
        registry.lookup_ancestor_method_with_owner(class, method)
    } else if class_name.eq_ignore_ascii_case("self") || class_name.eq_ignore_ascii_case("static") {
        registry.lookup_method_with_owner(class, method)
    } else {
        None
    };
    let Some((owner, _)) = resolved else {
        return true;
    };
    deps.push(PurityDependency::Method(owner, intern(method)));
    false
}

/// `new self()`/`new static()`/`new LiteralClassName()` - matches real
/// PHPStan's own `New_` handling: usable only when the target class has
/// a real, declared `__construct` of its own (an implicit default
/// constructor disqualifies too, matching `resolveCall`'s own
/// `!$classReflection->hasConstructor()` check exactly, however blunt
/// that looks - read directly from source, not softened). No
/// overridability check - a `new` expression always targets exactly one
/// concrete class, never ambiguous.
fn resolve_new(target_class: &str, registry: &Registry, deps: &mut Vec<PurityDependency>) -> bool {
    // `lookup_class`, not a plain `self.classes.get` - `target_class` is
    // either an already-canonical `declaring_class` (the `self`/`static`
    // case) or a literal name lifted verbatim from the AST (`new Foo()`),
    // whose case may not match the declaring key exactly. Same fix
    // rationale as `resolve_this_method_call`'s own doc comment.
    let Some((owner, info)) = registry.lookup_class(target_class) else {
        return true;
    };
    if !info.methods.contains_key("__construct") {
        return true;
    }
    deps.push(PurityDependency::Method(
        intern(owner),
        intern("__construct"),
    ));
    false
}

/// One real, resolved `ImpurePoint` occurrence, mirroring real
/// PHPStan's own `identifier`/`description`/`isCertain()` shape - the
/// diagnostic-only sibling of `stmt_disqualifies`/`expr_disqualifies`
/// above (see `collect_impure_points`'s own doc comment for why it's a
/// genuinely separate, non-short-circuiting walk rather than a
/// repurposing of those two).
pub(crate) struct ImpurePointOccurrence {
    pub(crate) span: Span,
    pub(crate) identifier: &'static str,
    pub(crate) description: &'static str,
    pub(crate) certain: bool,
}

/// A second, **non-short-circuiting** walk over the identical body `stmt_
/// disqualifies`/`expr_disqualifies` already classify, for `checker::
/// noop::check_purity_impure_points`'s own diagnostic use - that walk
/// stops at the *first* disqualifying construct and records only "can
/// never be proven pure, no matter what" (`purity_dependencies = None`),
/// correct for the fixed point (one impure point already means "not
/// pure," a second one adds nothing) but wrong for reporting every real
/// occurrence a `@phpstan-pure`-tagged-but-actually-impure declaration
/// contains - real PHPStan's own `FunctionPurityCheck` reports every one
/// of them, not just the first. Deliberately does **not** modify `stmt_
/// disqualifies`/`expr_disqualifies` themselves (regression risk to the
/// already-working fixed point) - this mirrors their own match arms
/// exactly, one call site at a time, but always keeps walking every
/// child regardless of what it just found.
///
/// A few real, disclosed narrowings relative to the full `ImpurePoint`
/// catalog, all in the "favor false negatives" direction this project
/// has held throughout:
/// - An *explicit* `throw` isn't included at all - real PHPStan's own
///   `ImpurePoint` family has no `throw` identifier; a declared/explicit
///   throw is a *throw point*, a separate concept `purity_gate_passes`
///   already excludes any `@throws`-declared/thrown-bodied function from
///   reaching this walk's own caller in the first place.
/// - A property write (`propertyAssign`, certain) and a by-reference one
///   (`propertyAssignByRef`, *not* certain) are real PHPStan's own two
///   distinct identifiers - this project's own `expr_disqualifies` never
///   distinguished a plain assignment target from a by-ref one for this
///   purpose, so both report as the common, dominant `propertyAssign`
///   case here too.
/// - Superglobal *read* (`'access to superglobal variable'`) and *write*
///   (`'assign to superglobal variable'`) share real PHPStan's own
///   identical `superglobal` identifier but different description text -
///   this project's own `expr_disqualifies` doesn't distinguish the two
///   either, so this always reports the read wording.
/// - `new`'s own three disqualifying shapes (a dynamic target, a
///   genuinely unknown class, a real class with no declared constructor
///   at all) all report uniformly as `new`/`"instantiation of unknown
///   class"` - real PHPStan's own construction sites use varying
///   description text per shape, but always the same `new` identifier;
///   this project's own `resolve_new` doesn't itself distinguish which
///   of the three specifically failed.
///
/// A precision gap on heavy `@pure`-tag usage: `possiblyImpure.
/// functionCall`/`.methodCall`/`.new` fire for any call this walk's own
/// `resolved` check can't prove targets a known declaration - which,
/// exactly like `resolve_static_call`/`resolve_this_method_call`/
/// `resolve_new` above (deliberately reused as-is, not widened, per this
/// doc comment's own "don't touch the boolean classifier" rule), only
/// recognizes `self::`/`static::`/`parent::` and `$this->` receivers,
/// never a literal class name (`Color::ansi256(...)`) or an arbitrary
/// object receiver - this project has no "resolve arbitrary target, then
/// ask if pure" step. A `namespace\foo()`-relative function call
/// compounds this further, a pre-existing, unrelated name-resolution gap
/// that also defeats the plain `registry.lookup_function` check in the
/// `Call` arm below. None of this is a regression - the exact same
/// resolution narrowness already silently capped `is_transitively_pure`;
/// this walk just gives it a voice for once-invisible cases. Real
/// follow-up, not attempted here: widen call-target resolution (both
/// here and in `resolve_static_call`/`resolve_this_method_call`, to stay
/// consistent) to a literal class name and an arbitrary receiver type,
/// and fix `namespace\`-relative call-name resolution generally.
pub(crate) fn collect_impure_points(
    body: &[Stmt],
    declaring_class: Option<&str>,
    registry: &Registry,
) -> Vec<ImpurePointOccurrence> {
    let mut out = Vec::new();
    for stmt in body {
        collect_stmt_impure_points(stmt, declaring_class, registry, &mut out);
    }
    out
}

fn collect_stmt_impure_points(
    stmt: &Stmt,
    declaring_class: Option<&str>,
    registry: &Registry,
    out: &mut Vec<ImpurePointOccurrence>,
) {
    match &stmt.kind {
        StmtKind::Echo(_) => out.push(ImpurePointOccurrence {
            span: stmt.span,
            identifier: "echo",
            description: "echo",
            certain: true,
        }),
        StmtKind::Global(_) => out.push(ImpurePointOccurrence {
            span: stmt.span,
            identifier: "global",
            description: "global variable",
            certain: true,
        }),
        StmtKind::StaticVar(_) => out.push(ImpurePointOccurrence {
            span: stmt.span,
            identifier: "static",
            description: "static variable",
            certain: true,
        }),
        StmtKind::InlineHtml(_) => out.push(ImpurePointOccurrence {
            span: stmt.span,
            identifier: "betweenPhpTags",
            description: "output between PHP opening and closing tags",
            certain: true,
        }),
        StmtKind::Expression(e) => collect_expr_impure_points(e, declaring_class, registry, out),
        StmtKind::Return(Some(e)) => collect_expr_impure_points(e, declaring_class, registry, out),
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => {
            collect_expr_impure_points(cond, declaring_class, registry, out);
        }
        StmtKind::For {
            init, cond, step, ..
        } => {
            for e in init.iter().chain(cond).chain(step) {
                collect_expr_impure_points(e, declaring_class, registry, out);
            }
        }
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            collect_expr_impure_points(subject, declaring_class, registry, out);
            if let Some(k) = key {
                collect_expr_impure_points(k, declaring_class, registry, out);
            }
            collect_expr_impure_points(value, declaring_class, registry, out);
        }
        StmtKind::Switch { subject, cases } => {
            collect_expr_impure_points(subject, declaring_class, registry, out);
            for c in cases {
                if let Some(t) = &c.test {
                    collect_expr_impure_points(t, declaring_class, registry, out);
                }
            }
        }
        StmtKind::GlobalConst(consts) => {
            for c in consts {
                collect_expr_impure_points(&c.value, declaring_class, registry, out);
            }
        }
        _ => {}
    }
    for child in child_stmts(stmt) {
        collect_stmt_impure_points(child, declaring_class, registry, out);
    }
}

fn collect_expr_impure_points(
    e: &Expr,
    declaring_class: Option<&str>,
    registry: &Registry,
    out: &mut Vec<ImpurePointOccurrence>,
) {
    match &e.kind {
        // Only a static property read is mutable shared state; a class
        // constant or enum case is immutable (real PHPStan's own
        // `NodeScopeResolver` never emits an `ImpurePoint` for a
        // `ClassConstFetch`). `expr_disqualifies`'s boolean classifier
        // still treats every static fetch as impure - a disclosed,
        // narrower divergence in the "fewer false positives" direction.
        ExprKind::StaticPropertyFetch(..) => out.push(ImpurePointOccurrence {
            span: e.span,
            identifier: "staticPropertyAccess",
            description: "static property access",
            certain: true,
        }),
        ExprKind::Variable(name) if is_superglobal(name) => out.push(ImpurePointOccurrence {
            span: e.span,
            identifier: "superglobal",
            description: "access to superglobal variable",
            certain: true,
        }),
        ExprKind::Yield(..) => out.push(ImpurePointOccurrence {
            span: e.span,
            identifier: "yield",
            description: "yield",
            certain: true,
        }),
        ExprKind::YieldFrom(_) => out.push(ImpurePointOccurrence {
            span: e.span,
            identifier: "yieldFrom",
            description: "yield from",
            certain: true,
        }),
        ExprKind::Assign(lhs, rhs) | ExprKind::CompoundAssign(_, lhs, rhs) => {
            if property_write_target(lhs) {
                out.push(ImpurePointOccurrence {
                    span: e.span,
                    identifier: "propertyAssign",
                    description: "property assignment",
                    certain: true,
                });
            }
            collect_expr_impure_points(lhs, declaring_class, registry, out);
            collect_expr_impure_points(rhs, declaring_class, registry, out);
        }
        ExprKind::Call(callee, args) => {
            if let ExprKind::Name(name) = &callee.kind {
                let lname = name.trim_start_matches('\\').to_ascii_lowercase();
                match lname.as_str() {
                    "isset" | "empty" => {}
                    "unset" => {
                        for a in args {
                            if property_write_target(&a.value) {
                                out.push(ImpurePointOccurrence {
                                    span: e.span,
                                    identifier: "propertyUnset",
                                    description: "property unset",
                                    certain: true,
                                });
                            }
                        }
                    }
                    "die" | "exit" => out.push(ImpurePointOccurrence {
                        span: e.span,
                        identifier: "exit",
                        description: "exit",
                        certain: true,
                    }),
                    "print" => out.push(ImpurePointOccurrence {
                        span: e.span,
                        identifier: "print",
                        description: "print",
                        certain: true,
                    }),
                    "eval" => out.push(ImpurePointOccurrence {
                        span: e.span,
                        identifier: "eval",
                        description: "eval",
                        certain: true,
                    }),
                    "include" | "include_once" | "require" | "require_once" => {
                        out.push(ImpurePointOccurrence {
                            span: e.span,
                            identifier: "include",
                            description: "include",
                            certain: true,
                        });
                    }
                    _ => {
                        if registry.lookup_function(name).is_none() {
                            out.push(ImpurePointOccurrence {
                                span: e.span,
                                identifier: "functionCall",
                                description: "call to unknown function",
                                certain: false,
                            });
                        }
                    }
                }
            } else {
                out.push(ImpurePointOccurrence {
                    span: e.span,
                    identifier: "functionCall",
                    description: "call to unknown function",
                    certain: false,
                });
            }
            for a in args {
                collect_expr_impure_points(&a.value, declaring_class, registry, out);
            }
        }
        ExprKind::MethodCall(recv, name, args, _) => {
            let resolved =
                matches!(&recv.kind, ExprKind::Variable(v) if **v == *"this")
                    && declaring_class.is_some_and(|class| {
                        registry.lookup_method_with_owner(class, name).is_some_and(
                            |(owner, sig)| is_non_overridable(registry, class, &owner, name, sig),
                        )
                    });
            if !resolved {
                out.push(ImpurePointOccurrence {
                    span: e.span,
                    identifier: "methodCall",
                    description: "call to unknown method",
                    certain: false,
                });
            }
            collect_expr_impure_points(recv, declaring_class, registry, out);
            for a in args {
                collect_expr_impure_points(&a.value, declaring_class, registry, out);
            }
        }
        ExprKind::DynamicStaticCall(_, _, args) => {
            out.push(ImpurePointOccurrence {
                span: e.span,
                identifier: "methodCall",
                description: "call to unknown method",
                certain: false,
            });
            for a in args {
                collect_expr_impure_points(&a.value, declaring_class, registry, out);
            }
        }
        ExprKind::StaticCall(receiver, name, args) => {
            let resolved = declaring_class.is_some_and(|class| {
                let is_receiver = |kw: &str| {
                    receiver
                        .as_name()
                        .is_some_and(|n| n.eq_ignore_ascii_case(kw))
                };
                let target = if is_receiver("parent") {
                    registry.lookup_ancestor_method_with_owner(class, name)
                } else if is_receiver("self") || is_receiver("static") {
                    registry.lookup_method_with_owner(class, name)
                } else {
                    None
                };
                target.is_some()
            });
            if !resolved {
                out.push(ImpurePointOccurrence {
                    span: e.span,
                    identifier: "methodCall",
                    description: "call to unknown method",
                    certain: false,
                });
            }
            for a in args {
                collect_expr_impure_points(&a.value, declaring_class, registry, out);
            }
        }
        ExprKind::New(callee, args) => {
            let resolved = if let ExprKind::Name(name) = &callee.kind {
                let target_class = match name.trim_start_matches('\\') {
                    n if n.eq_ignore_ascii_case("self") || n.eq_ignore_ascii_case("static") => {
                        declaring_class.map(str::to_string)
                    }
                    n => Some(n.to_string()),
                };
                target_class.is_some_and(|class| {
                    registry
                        .classes
                        .get(class.as_str())
                        .is_some_and(|info| info.methods.contains_key("__construct"))
                })
            } else {
                false
            };
            if !resolved {
                out.push(ImpurePointOccurrence {
                    span: e.span,
                    identifier: "new",
                    description: "instantiation of unknown class",
                    certain: false,
                });
            }
            for a in args {
                collect_expr_impure_points(&a.value, declaring_class, registry, out);
            }
        }
        // Every arm above already recurses into its own real children
        // explicitly (matching `expr_disqualifies`'s identical structure,
        // not just visually similar to it) - falling through to `expr_
        // children` unconditionally after the match, the way `stmt_
        // children` safely does in `collect_stmt_impure_points` just
        // above, would double-visit (and so double-report) every one of
        // those same children here, since `child_exprs` returns a
        // matched arm's own sub-expressions too, not just an unhandled
        // arm's. Only reachable for anything *not* explicitly matched
        // above (`BinaryOp`, `Ternary`, ...), exactly like `expr_
        // disqualifies`'s own `_` arm.
        _ => {
            for child in child_exprs(e) {
                collect_expr_impure_points(child, declaring_class, registry, out);
            }
        }
    }
}
