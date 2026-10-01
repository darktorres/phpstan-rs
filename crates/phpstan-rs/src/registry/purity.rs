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

use crate::registry::bodies::BodySource;
use rayon::prelude::*;
use syntax::name::Name;

use crate::registry::{FunctionReflection, Registry};
use syntax::ast::*;
use syntax::token::Span;
use syntax::visit::{Visitor, walk_expr, walk_stmt, walk_stmts};

/// One real, resolved call target a declaration's own effect-freedom is
/// conditional on - `Some(vec![])` (no dependencies at all) is the base
/// case a fixed point grows outward from; `None` (not this type, see
/// `FunctionReflection::purity_dependencies`) means "can never be proven
/// effect-free, no matter what". Both variants store a lower-cased key
/// (matching real PHPStan's own `functionKey`/`methodKey`, which
/// `strtolower`s both parts) - so the same declaration referenced under
/// different call-site casing always resolves to the same graph node.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum PurityDependency {
    Function(Name),
    Method(Name, Name),
}

impl Registry {
    /// Wired into `Registry::build` immediately after
    /// `resolve_deferred_value_of` (and into `Project`'s own incremental
    /// single-file-reload path, the same precedent) - re-runs from
    /// scratch on every reload rather than incrementally recomputing,
    /// prioritizing correctness over performance, the same tradeoff
    /// `resolve_deferred_value_of` already made. Each body comes from
    /// `bodies`, found by its declaration's provenance.
    #[hotpath::measure]
    pub(crate) fn resolve_transitive_purity(&mut self, bodies: &dyn BodySource) {
        self.classify_purity(bodies);
        self.run_purity_fixed_point();
        self.resolve_property_writes(bodies);
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
    fn classify_purity(&mut self, bodies: &dyn BodySource) {
        let function_names: Vec<Name> = self.scoped_function_names();
        let registry: &Registry = self;
        let function_deps: Vec<(Name, Option<Vec<PurityDependency>>)> = function_names
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .filter_map(|name| {
                let sig = &registry.functions[name];
                if !purity_gate_passes(sig, false) {
                    return None;
                }
                let body = bodies.body(&sig.provenance)?;
                let deps = classify_body(body, None, registry);
                Some((name.clone(), deps))
            })
            .collect();
        for (name, deps) in function_deps {
            self.functions.get_mut(&name).unwrap().purity_dependencies = deps;
        }

        let class_names: Vec<Name> = self.scoped_class_names();
        let registry: &Registry = self;
        let method_deps: Vec<(Name, Name, Option<Vec<PurityDependency>>)> = class_names
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .flat_map(|class_name| {
                let method_names: Vec<Name> = registry.classes[class_name]
                    .methods
                    .keys()
                    .cloned()
                    .collect();
                let mut per_class = Vec::new();
                for method_name in method_names {
                    let sig = &registry.classes[class_name].methods[&method_name];
                    if !purity_gate_passes(sig, syntax::ast::is_constructor_name(&method_name)) {
                        continue;
                    }
                    let Some(body) = bodies.body(&sig.provenance) else {
                        continue;
                    };
                    let deps = classify_body(body, Some(class_name), registry);
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
        let function_names: Vec<Name> = self.scoped_function_names();
        let class_names: Vec<Name> = self.scoped_class_names();
        loop {
            let mut changed = false;
            for name in &function_names {
                let sig = &self.functions[name];
                if sig.is_transitively_pure {
                    continue;
                }
                let Some(deps) = &sig.purity_dependencies else {
                    continue;
                };
                if deps.iter().all(|d| self.dependency_is_pure(d)) {
                    self.functions.get_mut(name).unwrap().is_transitively_pure = true;
                    changed = true;
                }
            }
            for class_name in &class_names {
                let method_names: Vec<Name> =
                    self.classes[class_name].methods.keys().cloned().collect();
                for method_name in method_names {
                    let sig = &self.classes[class_name].methods[&method_name];
                    if sig.is_transitively_pure {
                        continue;
                    }
                    let Some(deps) = &sig.purity_dependencies else {
                        continue;
                    };
                    if deps.iter().all(|d| self.dependency_is_pure(d)) {
                        self.classes
                            .get_mut(class_name)
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
/// one of this project's own `FunctionReflection`s already carries exactly the
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
fn purity_gate_passes(sig: &FunctionReflection, is_constructor: bool) -> bool {
    // See `FunctionReflection::is_stub`'s own doc comment - a vendored stub's
    // own body is always empty and never real; walking it would wrongly
    // "prove" every PHP builtin pure. Checked first, not that it matters
    // for correctness (the other checks would eventually reject some of
    // them too, but not all - `header()`'s own stub signature has no
    // by-ref param, no assert tag, no declared throw, so without this
    // check it would sail straight through every other gate).
    if sig.is_stub {
        return false;
    }
    if !is_constructor && sig.return_type.is_void() {
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
    let mut search = Disqualifier {
        declaring_class,
        registry,
        deps: Vec::new(),
        found: false,
    };
    walk_stmts(&mut search, body);
    if search.found {
        return None;
    }
    let mut deps = search.deps;
    deps.sort();
    deps.dedup();
    Some(deps)
}

/// The class `new <name>(...)` instantiates: the declaring class for
/// `self`/`static`, else the name as resolved.
fn new_target_class(name: &QualifiedName, declaring_class: Option<&str>) -> Option<String> {
    if name.is_keyword("self") || name.is_keyword("static") {
        return declaring_class.map(str::to_string);
    }
    name.resolved_class().map(str::to_string)
}

/// Searches one body for a construct that disqualifies it from purity,
/// collecting the calls its purity depends on along the way. A closure,
/// arrow function or class defined in the body doesn't run by being
/// defined, so none is entered (an anonymous class's constructor
/// arguments are).
struct Disqualifier<'r> {
    declaring_class: Option<&'r str>,
    registry: &'r Registry,
    deps: Vec<PurityDependency>,
    found: bool,
}

impl<'ast> Visitor<'ast> for Disqualifier<'_> {
    fn visit_function_decl(&mut self, _: &'ast FunctionDecl) {}

    fn visit_class_def(&mut self, _: &'ast ClassDef) {}

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if self.found {
            return;
        }
        syntax::stack::guard(|| {
            self.found = match &stmt.kind {
                // `echo`/`die`/`exit`/`print` are `betweenPhpTags`'s own
                // "output" siblings - real PHPStan's own separate `echo`
                // identifier for the statement form specifically.
                StmtKind::Echo(_) => true,
                // An *explicit* `throw` - matches `resolveDependencies`'s
                // own `$throwPoint->isExplicit()` check exactly (an implicit
                // throw from a called function is a different, unmodeled
                // concern, same as real PHPStan's own treatment).
                StmtKind::Throw(_) => true,
                StmtKind::Global(_) => true,
                // `static $x = ...;` - a function-local variable persisting
                // across calls, real PHPStan's own `'static'` identifier.
                StmtKind::StaticVar(_) => true,
                // Real output between PHP tags - `betweenPhpTags`.
                StmtKind::InlineHtml(_) => true,
                // Unsetting a variable changes nothing outside the function;
                // unsetting a property does (`propertyUnset`).
                StmtKind::Unset(targets) => targets.iter().any(property_write_target),
                _ => false,
            };
            if !self.found {
                walk_stmt(self, stmt);
            }
        })
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        if self.found {
            return;
        }
        syntax::stack::guard(|| {
            let (class, registry) = (self.declaring_class, self.registry);
            let disqualifies = match &e.kind {
                // Any static property access at all - real PHPStan's own
                // `staticPropertyAccess` fires even for a bare read of
                // shared static state, not just a write.
                ExprKind::ClassConstFetch(..)
                | ExprKind::ClassNameFetch(_)
                | ExprKind::StaticPropertyFetch(..)
                | ExprKind::DynamicStaticPropertyFetch(..) => true,
                ExprKind::Variable(name) => is_superglobal(name),
                ExprKind::Yield(..) | ExprKind::YieldFrom(_) => true,
                // Runs a shell command.
                ExprKind::ShellExec(_) => true,
                ExprKind::Print(_)
                | ExprKind::Exit(..)
                | ExprKind::Eval(_)
                | ExprKind::Include(..) => true,
                ExprKind::Assign(lhs, _)
                | ExprKind::AssignRef(lhs, _)
                | ExprKind::CompoundAssign(_, lhs, _) => property_write_target(lhs),
                // The declared name, not a lowercased one - see
                // `resolve_this_method_call`'s own doc comment for why a
                // lowercased key would break `dependency_is_pure`'s later
                // exact lookup.
                ExprKind::Call(callee, _) => match &callee.kind {
                    ExprKind::Name(name) => match registry.resolve_function(name) {
                        Some((resolved, _)) => {
                            let declared = registry
                                .lookup_function_with_name(resolved)
                                .map_or(resolved, |(declared, _)| declared);
                            self.deps
                                .push(PurityDependency::Function(Name::from(declared)));
                            false
                        }
                        None => true,
                    },
                    _ => true,
                },
                ExprKind::MethodCall(recv, name, _, _) => {
                    !matches!(&recv.kind, ExprKind::Variable(v) if *v == "this")
                        || class.is_none_or(|class| {
                            resolve_this_method_call(class, name, registry, &mut self.deps)
                        })
                }
                // A dynamic receiver is never one of the resolvable
                // pseudo-keywords: unresolvable, so disqualifying.
                ExprKind::StaticCall(target, name, _) => match (class, target.as_name()) {
                    (Some(class), Some(target)) => {
                        resolve_static_call(class, target, name, registry, &mut self.deps)
                    }
                    _ => true,
                },
                // A computed method name: any method, so unresolvable.
                ExprKind::DynamicMethodCall(..) | ExprKind::DynamicStaticCall(..) => true,
                ExprKind::New(callee, _) => match &callee.kind {
                    ExprKind::Name(name) => match new_target_class(name, class) {
                        Some(target) => resolve_new(&target, registry, &mut self.deps),
                        None => true,
                    },
                    _ => true,
                },
                ExprKind::Closure { .. } | ExprKind::ArrowFn { .. } => return,
                _ => false,
            };
            if disqualifies {
                self.found = true;
            } else {
                walk_expr(self, e);
            }
        })
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
    syntax::stack::guard(|| match &e.kind {
        ExprKind::PropertyAccess(..) | ExprKind::DynamicPropertyAccess(..) => true,
        ExprKind::Index(base, _) => property_write_target(base),
        _ => false,
    })
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
/// lookup consistently), this project's `self.classes`/`ClassReflection.
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
    deps.push(PurityDependency::Method(owner.clone(), Name::from(method)));
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
    sig: &FunctionReflection,
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
    deps.push(PurityDependency::Method(owner, Name::from(method)));
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
        Name::from(owner),
        Name::from("__construct"),
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
///
/// `implicit_calls` are the offsets in `body` the checker found calling an
/// `ArrayAccess` method, which the walk can't tell from an array's.
pub(crate) fn collect_impure_points(
    body: &[Stmt],
    declaring_class: Option<&str>,
    registry: &Registry,
    implicit_calls: &[Span],
) -> Vec<ImpurePointOccurrence> {
    let mut points = ImpurePoints {
        declaring_class,
        registry,
        out: Vec::new(),
    };
    walk_stmts(&mut points, body);
    for span in implicit_calls {
        points.push(*span, "methodCall", "call to unknown method", false);
    }
    points.out
}

/// Every impure point in one body, in source order; the same scope as
/// `Disqualifier`.
struct ImpurePoints<'r> {
    declaring_class: Option<&'r str>,
    registry: &'r Registry,
    out: Vec<ImpurePointOccurrence>,
}

impl ImpurePoints<'_> {
    fn push(
        &mut self,
        span: Span,
        identifier: &'static str,
        description: &'static str,
        certain: bool,
    ) {
        self.out.push(ImpurePointOccurrence {
            span,
            identifier,
            description,
            certain,
        });
    }

    /// Whether a call of `name` on the class the body is declared in (or,
    /// for `parent::`, its parent) resolves.
    fn static_call_resolves(&self, receiver: &StaticTarget, name: &str) -> bool {
        let (Some(class), Some(receiver)) = (self.declaring_class, receiver.as_name()) else {
            return false;
        };
        let target = if receiver.is_keyword("parent") {
            self.registry.lookup_ancestor_method_with_owner(class, name)
        } else if receiver.is_keyword("self") || receiver.is_keyword("static") {
            self.registry.lookup_method_with_owner(class, name)
        } else {
            None
        };
        target.is_some()
    }
}

impl<'ast> Visitor<'ast> for ImpurePoints<'_> {
    fn visit_function_decl(&mut self, _: &'ast FunctionDecl) {}

    fn visit_class_def(&mut self, _: &'ast ClassDef) {}

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        syntax::stack::guard(|| {
            match &stmt.kind {
                StmtKind::Echo(_) => self.push(stmt.span, "echo", "echo", true),
                StmtKind::Global(_) => self.push(stmt.span, "global", "global variable", true),
                StmtKind::StaticVar(_) => self.push(stmt.span, "static", "static variable", true),
                StmtKind::InlineHtml(_) => self.push(
                    stmt.span,
                    "betweenPhpTags",
                    "output between PHP opening and closing tags",
                    true,
                ),
                StmtKind::Unset(targets) => {
                    for _ in targets.iter().filter(|t| property_write_target(t)) {
                        self.push(stmt.span, "propertyUnset", "property unset", true);
                    }
                }
                _ => {}
            }
            walk_stmt(self, stmt);
        })
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        syntax::stack::guard(|| {
            match &e.kind {
                // Only a static property read is mutable shared state; a
                // class constant or enum case is immutable (real PHPStan's
                // own `NodeScopeResolver` never emits an `ImpurePoint` for a
                // `ClassConstFetch`). `Disqualifier` still treats every
                // static fetch as impure - a disclosed, narrower divergence
                // in the "fewer false positives" direction.
                ExprKind::StaticPropertyFetch(..) => {
                    self.push(
                        e.span,
                        "staticPropertyAccess",
                        "static property access",
                        true,
                    );
                }
                ExprKind::Variable(name) if is_superglobal(name) => {
                    self.push(
                        e.span,
                        "superglobal",
                        "access to superglobal variable",
                        true,
                    );
                }
                ExprKind::Yield(..) => self.push(e.span, "yield", "yield", true),
                ExprKind::YieldFrom(_) => self.push(e.span, "yieldFrom", "yield from", true),
                ExprKind::Exit(..) => self.push(e.span, "exit", "exit", true),
                ExprKind::Print(_) => self.push(e.span, "print", "print", true),
                ExprKind::Eval(_) => self.push(e.span, "eval", "eval", true),
                ExprKind::Include(..) => self.push(e.span, "include", "include", true),
                ExprKind::Assign(lhs, _) | ExprKind::CompoundAssign(_, lhs, _)
                    if property_write_target(lhs) =>
                {
                    self.push(e.span, "propertyAssign", "property assignment", true);
                }
                ExprKind::Call(callee, _) => {
                    let known = matches!(&callee.kind, ExprKind::Name(name)
                        if self.registry.resolve_function(name).is_some());
                    if !known {
                        self.push(e.span, "functionCall", "call to unknown function", false);
                    }
                }
                ExprKind::MethodCall(recv, name, _, _) => {
                    let resolved = matches!(&recv.kind, ExprKind::Variable(v) if *v == "this")
                        && self.declaring_class.is_some_and(|class| {
                            self.registry
                                .lookup_method_with_owner(class, name)
                                .is_some_and(|(owner, sig)| {
                                    is_non_overridable(self.registry, class, &owner, name, sig)
                                })
                        });
                    if !resolved {
                        self.push(e.span, "methodCall", "call to unknown method", false);
                    }
                }
                ExprKind::DynamicStaticCall(..) | ExprKind::DynamicMethodCall(..) => {
                    self.push(e.span, "methodCall", "call to unknown method", false);
                }
                ExprKind::StaticCall(receiver, name, _) => {
                    if !self.static_call_resolves(receiver, name) {
                        self.push(e.span, "methodCall", "call to unknown method", false);
                    }
                }
                ExprKind::New(callee, _) => {
                    let resolved = match &callee.kind {
                        ExprKind::Name(name) => new_target_class(name, self.declaring_class)
                            .is_some_and(|class| {
                                self.registry.lookup_class(&class).is_some_and(|(_, info)| {
                                    info.methods.contains_key("__construct")
                                })
                            }),
                        _ => false,
                    };
                    if !resolved {
                        self.push(e.span, "new", "instantiation of unknown class", false);
                    }
                }
                ExprKind::Closure { .. } | ExprKind::ArrowFn { .. } => return,
                _ => {}
            }
            walk_expr(self, e);
        })
    }
}
