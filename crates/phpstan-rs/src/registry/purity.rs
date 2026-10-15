//! The transitive-purity half: which functions/methods/constructors are
//! provably effect-free - either directly (an empty/pure-arithmetic body)
//! or transitively (every call they make resolves to something *else*
//! already proven effect-free).
//!
//! **The real architecture**: a plain, monotonic search over a dependency
//! graph, run for a declaration when something asks whether it is pure
//! (`Registry::function_is_pure`/`method_is_pure`) and remembered
//! (`registry::demand`). Few declarations are ever asked about, and none is
//! settled that nothing asks about. A debug build also settles every
//! declaration at once, the way the registry did before it was asked, and
//! checks each answer against that.
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
use crate::registry::name_table::FoldedKey;
use crate::registry::property_writes::{DeclKey, DeclRef, function_key, method_key};
use rustc_hash::FxHashMap;
use syntax::name::Name;

use crate::registry::summaries::PurityCall;
use crate::registry::{FunctionReflection, Registry};
use syntax::ast::*;
use syntax::span::Span;
use syntax::visit::{Visitor, walk_expr, walk_stmt, walk_stmts};

/// One real, resolved call target a declaration's own effect-freedom is
/// conditional on - `Some(vec![])` (no dependencies at all) is the base
/// case a fixed point grows outward from; `None` (not this type, see
/// `Registry::purity_dependencies_of`) means "can never be proven
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

/// A declaration the search has entered and not left.
struct Frame {
    key: DeclKey,
    /// The declarations the purity of this one is conditional on.
    deps: Vec<DeclKey>,
    /// The next of them to follow.
    next: usize,
}

/// The declaration a dependency names.
fn dependency_key(dep: &PurityDependency) -> DeclKey {
    match dep {
        PurityDependency::Function(name) => function_key(name),
        PurityDependency::Method(class, method) => method_key(class, method),
    }
}

impl Registry {
    /// What the purity of the declaration `key` names is conditional on:
    /// the declarations its body calls, none for a body that calls none.
    /// `None` when it can never be proven effect-free: it is no function or
    /// method of the project, fails the gate, has no body at hand, or has a
    /// construct or a call that decides against it.
    ///
    /// Gated the same way real PHPStan's own `*WithoutImpurePointsCollector`s
    /// gate before ever calling `resolveDependencies` (`purity_gate_passes`).
    /// This project has no `@phpstan-pure`/`@phpstan-impure`/
    /// `hasSideEffects` annotation surface at all, so unlike real PHPStan's
    /// own gate, nothing here is ever pre-disqualified by an existing
    /// annotation - every declaration starts "maybe pure" until its own
    /// body says otherwise.
    fn purity_dependencies_of(
        &self,
        key: &DeclKey,
        bodies: &dyn BodySource,
    ) -> Option<Vec<DeclKey>> {
        let (class, sig, is_constructor) = match &key.class {
            None => (None, self.functions.get(key.member.as_str())?, false),
            Some(class) => {
                let (name, info) = self.classes.get_with_name(&FoldedKey(class.as_str()))?;
                let sig = info.methods.get(key.member.as_str())?;
                (
                    Some(name),
                    sig,
                    syntax::ast::is_constructor_name(&key.member),
                )
            }
        };
        if !purity_gate_passes(sig, is_constructor) {
            return None;
        }
        let deps = classify_summary(
            bodies,
            &sig.provenance,
            class.map(|name| name.as_ref()),
            self,
        )??;
        Some(deps.iter().map(dependency_key).collect())
    }

    /// Whether `root` is provably effect-free, found by following the
    /// calls its body makes and remembered for every declaration the search
    /// settles. A declaration is pure once every one of its dependencies is:
    /// plain, monotonic, and a dependency cycle with no external pure base
    /// case is correctly never pure, matching real PHPStan's own identical
    /// behavior for the same shape. So a call back into a declaration the
    /// search is inside of is impure, and so is every declaration on the way
    /// back to it, as each is in the cycle or reaches it. A declaration's
    /// answer therefore does not depend on where the search began.
    pub(in crate::registry) fn declaration_is_pure(
        &self,
        root: DeclKey,
        bodies: &dyn BodySource,
    ) -> bool {
        // `None` while the search is inside the declaration.
        let mut done: FxHashMap<DeclKey, Option<bool>> = FxHashMap::default();
        let mut stack: Vec<Frame> = Vec::new();
        let enter =
            |key: DeclKey, done: &mut FxHashMap<DeclKey, Option<bool>>| -> Result<bool, Frame> {
                if let Some(pure) = self.demand.pure_of(&key) {
                    return Ok(pure);
                }
                match done.get(&key) {
                    Some(Some(pure)) => return Ok(*pure),
                    Some(None) => return Ok(false),
                    None => {}
                }
                match self.purity_dependencies_of(&key, bodies) {
                    None => {
                        done.insert(key, Some(false));
                        Ok(false)
                    }
                    Some(deps) => {
                        done.insert(key.clone(), None);
                        Err(Frame { key, deps, next: 0 })
                    }
                }
            };
        match enter(root, &mut done) {
            Ok(pure) => return pure,
            Err(frame) => stack.push(frame),
        }
        let mut returned: Option<bool> = None;
        let pure = loop {
            let top = stack
                .last_mut()
                .expect("the search is inside a declaration");
            // A dependency that is not pure leaves no purity below it.
            let ended = match returned.take() {
                Some(false) => Some(false),
                _ if top.next == top.deps.len() => Some(true),
                _ => None,
            };
            match ended {
                Some(pure) => {
                    let frame = stack.pop().expect("the search is inside a declaration");
                    done.insert(frame.key, Some(pure));
                    if stack.is_empty() {
                        break pure;
                    }
                    returned = Some(pure);
                }
                None => {
                    let dep = top.deps[top.next].clone();
                    top.next += 1;
                    match enter(dep, &mut done) {
                        Ok(pure) => returned = Some(pure),
                        Err(frame) => stack.push(frame),
                    }
                }
            }
        };
        self.demand.record_pure(
            done.into_iter()
                .filter_map(|(key, pure)| Some((key, pure?)))
                .collect(),
        );
        pure
    }

    /// Whether calling function `name` is provably effect-free.
    pub(crate) fn function_is_pure(&self, name: &str, bodies: &dyn BodySource) -> bool {
        let query = DeclRef {
            class: None,
            member: name,
        };
        let pure = match self.demand.pure_of(&query) {
            Some(pure) => pure,
            None => self.declaration_is_pure(function_key(name), bodies),
        };
        #[cfg(debug_assertions)]
        self.assert_pure_as_built(&function_key(name), pure, bodies);
        pure
    }

    /// Whether calling `class`'s `method` is provably effect-free: `class`
    /// is the one that declares it (`lookup_method_with_owner`'s).
    pub(crate) fn method_is_pure(
        &self,
        class: &str,
        method: &str,
        bodies: &dyn BodySource,
    ) -> bool {
        let query = DeclRef {
            class: Some(class),
            member: method,
        };
        let pure = match self.demand.pure_of(&query) {
            Some(pure) => pure,
            None => self.declaration_is_pure(method_key(class, method), bodies),
        };
        #[cfg(debug_assertions)]
        self.assert_pure_as_built(&method_key(class, method), pure, bodies);
        pure
    }

    /// Whether `key` is among the declarations found pure for every
    /// declaration at once, which an answer found when asked has to agree
    /// with.
    #[cfg(debug_assertions)]
    fn assert_pure_as_built(&self, key: &DeclKey, found: bool, bodies: &dyn BodySource) {
        let Some(built) = &self.as_built else {
            return;
        };
        // Only where the bodies at hand are those the answers were built from.
        let sig = match &key.class {
            None => self.functions.get(key.member.as_str()),
            Some(class) => self
                .classes
                .get(class.as_str())
                .and_then(|info| info.methods.get(key.member.as_str())),
        };
        let Some(sig) = sig else {
            return;
        };
        if bodies.summary(&sig.provenance).is_none() {
            return;
        }
        assert_eq!(
            found,
            built.pure.contains(key),
            "the purity found when asked and the one built for every declaration differ for {key:?}"
        );
    }

    /// The functions and methods found pure for every declaration with a
    /// body at once: the answers the ones found when asked are checked
    /// against.
    #[cfg(debug_assertions)]
    pub(in crate::registry) fn pure_among_every_declaration(
        &self,
        bodies: &dyn BodySource,
    ) -> rustc_hash::FxHashSet<DeclKey> {
        let mut candidates: FxHashMap<DeclKey, Vec<DeclKey>> = FxHashMap::default();
        for (name, _) in self.functions.own_iter() {
            let key = function_key(name);
            if let Some(deps) = self.purity_dependencies_of(&key, bodies) {
                candidates.insert(key, deps);
            }
        }
        for (class, info) in self.classes.own_iter() {
            for (method, _) in info.methods.iter() {
                let key = method_key(class, method);
                if let Some(deps) = self.purity_dependencies_of(&key, bodies) {
                    candidates.insert(key, deps);
                }
            }
        }
        // A candidate is pure once every dependency is; one that is no
        // candidate never is.
        let mut pure: rustc_hash::FxHashSet<DeclKey> = rustc_hash::FxHashSet::default();
        loop {
            let found: Vec<DeclKey> = candidates
                .iter()
                .filter(|(key, deps)| {
                    !pure.contains(*key) && deps.iter().all(|dep| pure.contains(dep))
                })
                .map(|(key, _)| key.clone())
                .collect();
            if found.is_empty() {
                return pure;
            }
            pure.extend(found);
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

/// The dependency list of the body the provenance finds, from what the
/// passes read of it (`Some(deps)`, possibly empty) or `None` when a call
/// does not resolve or a construct makes it impure; the outer `None` when
/// the body is not at hand. Resolving the body's calls in the order it made
/// them stops at the first that does not.
fn classify_summary(
    bodies: &dyn BodySource,
    provenance: &crate::registry::Provenance,
    declaring_class: Option<&str>,
    registry: &Registry,
) -> Option<Option<Vec<PurityDependency>>> {
    let summary = bodies.summary(provenance)?;
    let deps = summary
        .purity
        .as_deref()
        .and_then(|calls| resolve_purity_calls(calls, declaring_class, registry));
    // The answer from the statements themselves, for the bodies at hand.
    #[cfg(debug_assertions)]
    if let Some(body) = bodies.body(provenance) {
        assert_eq!(
            deps,
            classify_body(body, declaring_class, registry),
            "the summary of a body and the body disagree on its purity"
        );
    }
    Some(deps)
}

/// The dependencies of a body whose calls are `calls`, or `None` when one
/// does not resolve. `declaring_class` is `None` for a plain function, `Some`
/// for a method or constructor.
fn resolve_purity_calls(
    calls: &[PurityCall],
    declaring_class: Option<&str>,
    registry: &Registry,
) -> Option<Vec<PurityDependency>> {
    let mut deps = Vec::new();
    for call in calls {
        let unresolved = match call {
            PurityCall::Function(name) => match registry.resolve_function(name) {
                Some((resolved, _)) => {
                    let declared = registry
                        .lookup_function_with_name(resolved)
                        .map_or(resolved, |(declared, _)| declared);
                    deps.push(PurityDependency::Function(Name::from(declared)));
                    false
                }
                None => true,
            },
            PurityCall::ThisMethod(name) => declaring_class
                .is_none_or(|class| resolve_this_method_call(class, name, registry, &mut deps)),
            PurityCall::Static(target, name) => match declaring_class {
                Some(class) => resolve_static_call(class, target, name, registry, &mut deps),
                None => true,
            },
            PurityCall::New(name) => match new_target_class(name, declaring_class) {
                Some(target) => resolve_new(&target, registry, &mut deps),
                None => true,
            },
        };
        if unresolved {
            return None;
        }
    }
    deps.sort();
    deps.dedup();
    Some(deps)
}

/// Walks a declaration's own body once, returning its dependency list
/// (`Some(deps)`, possibly empty) or `None` the moment any disqualifying
/// construct or unresolvable call is found - matches real PHPStan's own
/// "one impure point/one unresolvable call disqualifies the whole
/// declaration" rule exactly, not a per-statement partial result.
/// `declaring_class` is `None` for a plain top-level function, `Some`
/// for a method/constructor - needed to resolve `$this->`/`self::`/
/// `static::`/`parent::`/`new self()`/`new static()`.
#[cfg(debug_assertions)]
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
#[cfg(debug_assertions)]
struct Disqualifier<'r> {
    declaring_class: Option<&'r str>,
    registry: &'r Registry,
    deps: Vec<PurityDependency>,
    found: bool,
}

#[cfg(debug_assertions)]
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
