//! Which objects' properties a function or method may write: none that
//! existed before the call, only those of its own `$this`, or any. The
//! checker forgets what it knows about an object's properties after a
//! call that may write them (`Checker::forget_members_a_call_may_change`).
//!
//! Unlike purity (`registry::purity`), output, throwing, exiting or a
//! `void` return don't matter here - only property writes do. A body is
//! classified once, at registry build time, into the property writes it
//! makes itself and the calls whose own writes it inherits; a monotonic
//! fixed point then settles every verdict, a dependency cycle keeping
//! the most conservative one. A call is followed through `$this`, a
//! `$this` property or a parameter never reassigned (by its declared
//! class), `self::`/`static::`/`parent::`, a named class's static
//! method, `new`, and a function by name; any other call may write
//! anything.
//!
//! A builtin (a stub) is taken to write the properties of no object,
//! unless a parameter of it takes something through which it runs code
//! or writes properties (`CODE_RUNNING_PARAMETER_TYPES`).

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::*;
use syntax::name::Name;
use syntax::visit::{Visitor, walk_expr, walk_stmt, walk_stmts};

use crate::registry::bodies::BodySource;
use crate::registry::{FunctionReflection, Registry, is_non_overridable};
use types::Type;

/// Whose properties a call may write, in increasing order of reach.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub(crate) enum PropertyWrites {
    /// No object's that existed before the call.
    Nothing,
    /// Only its own `$this`'s.
    OwnObject,
    /// Any object's.
    Anything,
}

/// A declaration, keyed case-insensitively: `(class, method)` for a
/// method, `(None, function)` for a function.
type DeclKey = (Option<String>, String);

fn function_key(name: &str) -> DeclKey {
    (None, name.to_ascii_lowercase())
}

fn method_key(class: &str, method: &str) -> DeclKey {
    (
        Some(class.to_ascii_lowercase()),
        method.to_ascii_lowercase(),
    )
}

/// What a called declaration's verdict must be for the caller to keep
/// its candidate one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Need {
    /// It writes no object's properties: a call on another object, or
    /// a function.
    Nothing,
    /// It writes at most its own object's: a constructor through `new`,
    /// whose object is new.
    OwnObject,
    /// It writes no more than the caller may: a call on the caller's own
    /// `$this`.
    SameAsCaller,
}

/// One body's own property writes and the calls it inherits writes from.
#[derive(Debug, Default)]
struct Classification {
    writes_own_object: bool,
    deps: Vec<(DeclKey, Need)>,
}

impl Registry {
    /// Classifies every function and method with a body, then settles
    /// their verdicts. Run with `resolve_transitive_purity`, after every
    /// class is indexed and flattened.
    pub(crate) fn resolve_property_writes(&mut self, bodies: &dyn BodySource) {
        let mut classified: FxHashMap<DeclKey, Option<Classification>> = FxHashMap::default();
        // A shared base holds only built-in stubs, which have no body.
        for (name, sig) in self.functions.own_iter() {
            if sig.is_stub {
                continue;
            }
            let body = bodies.body(&sig.provenance);
            classified.insert(
                function_key(name),
                body.and_then(|body| classify(body, None, sig, self)),
            );
        }
        for (class, info) in self.classes.own_iter() {
            for (method, sig) in &info.methods {
                if sig.is_stub {
                    continue;
                }
                let body = bodies.body(&sig.provenance);
                classified.insert(
                    method_key(class, method),
                    body.and_then(|body| classify(body, Some(class), sig, self)),
                );
            }
        }
        let mut verdicts: FxHashMap<DeclKey, PropertyWrites> = classified
            .keys()
            .map(|key| (key.clone(), PropertyWrites::Anything))
            .collect();
        loop {
            let mut changed = false;
            for (key, classification) in &classified {
                let Some(classification) = classification else {
                    continue;
                };
                let current = verdicts[key];
                for candidate in [PropertyWrites::Nothing, PropertyWrites::OwnObject] {
                    if candidate >= current {
                        break;
                    }
                    if candidate == PropertyWrites::Nothing && classification.writes_own_object {
                        continue;
                    }
                    let satisfied = classification.deps.iter().all(|(dep, need)| {
                        let verdict = self.dependency_verdict(dep, &verdicts);
                        match need {
                            Need::Nothing => verdict == PropertyWrites::Nothing,
                            Need::OwnObject => verdict <= PropertyWrites::OwnObject,
                            Need::SameAsCaller => verdict <= candidate,
                        }
                    });
                    if satisfied {
                        verdicts.insert(key.clone(), candidate);
                        changed = true;
                        break;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        self.property_writes = verdicts;
    }

    /// A dependency's verdict: a stub's by its parameters, a classified
    /// declaration's as settled so far, anything else's `Anything`.
    fn dependency_verdict(
        &self,
        dep: &DeclKey,
        verdicts: &FxHashMap<DeclKey, PropertyWrites>,
    ) -> PropertyWrites {
        if let Some(verdict) = verdicts.get(dep) {
            return *verdict;
        }
        let sig = match &dep.0 {
            Some(class) => self.lookup_method(class, &dep.1),
            None => self.functions.get(dep.1.as_str()),
        };
        match sig {
            Some(sig) if sig.is_stub => stub_verdict(sig),
            _ => PropertyWrites::Anything,
        }
    }

    /// Whose properties calling function `name` may write.
    pub(crate) fn function_property_writes(&self, name: &str) -> PropertyWrites {
        match self.functions.get(name) {
            Some(sig) if sig.is_stub => stub_verdict(sig),
            Some(_) => self
                .property_writes
                .get(&function_key(name))
                .copied()
                .unwrap_or(PropertyWrites::Anything),
            None => PropertyWrites::Anything,
        }
    }

    /// Whose properties calling `owner`'s `method` - the declaration a
    /// call reaches, not an override of it - may write.
    pub(crate) fn method_property_writes(&self, owner: &str, method: &str) -> PropertyWrites {
        match self.lookup_method(owner, method) {
            Some(sig) if sig.is_stub => stub_verdict(sig),
            Some(_) => self
                .property_writes
                .get(&method_key(owner, method))
                .copied()
                .unwrap_or(PropertyWrites::Anything),
            None => PropertyWrites::Anything,
        }
    }
}

/// The parameter types through which a builtin runs code an object
/// brings, or writes the object's properties itself: a callable, an
/// untyped object (`ReflectionProperty::setValue`), and the interfaces a
/// builtin calls the methods of (`count()`, iterating, `json_encode()`,
/// a string conversion, offset access, `serialize()`).
const CODE_RUNNING_PARAMETER_TYPES: [&str; 11] = [
    "callable",
    "Closure",
    "object",
    "Countable",
    "Traversable",
    "Iterator",
    "IteratorAggregate",
    "JsonSerializable",
    "Stringable",
    "ArrayAccess",
    "Serializable",
];

/// A builtin writes no object's properties unless a parameter of it takes
/// one of `CODE_RUNNING_PARAMETER_TYPES`.
fn stub_verdict(sig: &FunctionReflection) -> PropertyWrites {
    let runs_code = sig.params.iter().any(|p| {
        CODE_RUNNING_PARAMETER_TYPES
            .iter()
            .any(|name| p.ty.mentions(name))
    });
    if runs_code {
        PropertyWrites::Anything
    } else {
        PropertyWrites::Nothing
    }
}

/// `body`'s classification, or `None` when it may write any object's
/// properties whatever it calls. `class` is the declaring class of a
/// method, `sig` its signature.
fn classify(
    body: &[Stmt],
    class: Option<&Name>,
    sig: &FunctionReflection,
    registry: &Registry,
) -> Option<Classification> {
    let mut reassigned = Reassigned::default();
    walk_stmts(&mut reassigned, body);
    let params: FxHashMap<&str, &Type> = sig
        .params
        .iter()
        .filter(|p| !p.by_ref && !reassigned.0.contains(p.name.as_ref()))
        .map(|p| (p.name.as_ref(), &p.ty))
        .collect();
    let mut scan = Scan {
        class: class.map(|c| c.as_ref()),
        registry,
        params,
        classification: Classification::default(),
        anything: false,
    };
    walk_stmts(&mut scan, body);
    (!scan.anything).then_some(scan.classification)
}

/// The variables a body assigns, whose declared types then no longer
/// hold.
#[derive(Default)]
struct Reassigned(FxHashSet<String>);

impl<'ast> Visitor<'ast> for Reassigned {
    fn visit_expr(&mut self, e: &'ast Expr) {
        if let ExprKind::Assign(lhs, _)
        | ExprKind::AssignRef(lhs, _)
        | ExprKind::CompoundAssign(_, lhs, _) = &e.kind
        {
            collect_assigned_variables(lhs, &mut self.0);
        }
        walk_expr(self, e);
    }

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if let StmtKind::Foreach { key, value, .. } = &stmt.kind {
            for target in key.iter().chain([value]) {
                collect_assigned_variables(target, &mut self.0);
            }
        }
        walk_stmt(self, stmt);
    }
}

fn collect_assigned_variables(target: &Expr, out: &mut FxHashSet<String>) {
    match &target.kind {
        ExprKind::Variable(name) => {
            out.insert(name.to_string());
        }
        ExprKind::Destructure(items, _) => {
            for item in items {
                if let Some(target) = &item.target {
                    collect_assigned_variables(target, out);
                }
            }
        }
        _ => {}
    }
}

struct Scan<'r> {
    class: Option<&'r str>,
    registry: &'r Registry,
    params: FxHashMap<&'r str, &'r Type>,
    classification: Classification,
    anything: bool,
}

/// Whose property a write to `target` writes: `Some(true)` its own
/// `$this`'s, `Some(false)` another object's, `None` no property's.
fn property_target_owner(target: &Expr) -> Option<bool> {
    match &target.peeled().kind {
        ExprKind::PropertyAccess(base, _, _) | ExprKind::DynamicPropertyAccess(base, _, _) => {
            Some(matches!(&base.peeled().kind, ExprKind::Variable(v) if **v == *"this"))
        }
        ExprKind::Index(base, _) => property_target_owner(base),
        _ => None,
    }
}

impl Scan<'_> {
    fn write(&mut self, target: &Expr) {
        match target.peeled().kind {
            ExprKind::Destructure(ref items, _) => {
                for item in items {
                    if let Some(target) = &item.target {
                        self.write(target);
                    }
                }
            }
            _ => match property_target_owner(target) {
                Some(true) => self.classification.writes_own_object = true,
                Some(false) => self.anything = true,
                None => {}
            },
        }
    }

    fn depend(&mut self, key: DeclKey, need: Need) {
        self.classification.deps.push((key, need));
    }

    /// The class a receiver expression holds, by its declared type:
    /// `$this`, a property of `$this`, or a parameter never reassigned.
    fn receiver_class(&self, recv: &Expr) -> Option<String> {
        let ty = match &recv.peeled().kind {
            ExprKind::Variable(v) if **v == *"this" => return self.class.map(str::to_string),
            ExprKind::Variable(v) => (*self.params.get(v.as_ref())?).clone(),
            ExprKind::PropertyAccess(base, prop, _) if matches!(&base.peeled().kind, ExprKind::Variable(v) if **v == *"this") => {
                self.registry
                    .lookup_property(self.class?, prop.as_ref())?
                    .ty()
                    .clone()
            }
            _ => return None,
        };
        let (class, _) = ty.as_object()?;
        Some(class.to_string())
    }

    /// The declaration `$recv->method()` surely reaches.
    fn resolve_method_call(&self, recv: &Expr, method: &str) -> Option<(String, bool)> {
        let class = self.receiver_class(recv)?;
        let (owner, sig) = self.registry.lookup_method_with_owner(&class, method)?;
        if !is_non_overridable(self.registry, &class, &owner, method, sig) {
            return None;
        }
        let on_this = matches!(&recv.peeled().kind, ExprKind::Variable(v) if **v == *"this");
        Some((owner.to_string(), on_this))
    }

    /// The by-reference parameters `sig` passes a property to among
    /// `args`: each a property write.
    fn by_ref_property_args(&mut self, sig: Option<&FunctionReflection>, args: &[Arg]) {
        for (i, arg) in args.iter().enumerate() {
            let by_ref = match sig {
                Some(sig) => sig
                    .params
                    .get(i)
                    .or_else(|| sig.params.last().filter(|p| p.variadic))
                    .is_some_and(|p| p.by_ref),
                None => true,
            };
            if by_ref {
                self.write(&arg.value);
            }
        }
    }
}

impl<'ast> Visitor<'ast> for Scan<'_> {
    fn visit_function_decl(&mut self, _: &'ast FunctionDecl) {}

    fn visit_class_def(&mut self, _: &'ast ClassDef) {}

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if self.anything {
            return;
        }
        syntax::stack::guard(|| {
            match &stmt.kind {
                StmtKind::Unset(targets) => {
                    for target in targets {
                        self.write(target);
                    }
                }
                StmtKind::Foreach {
                    subject,
                    value,
                    by_ref,
                    ..
                } => {
                    self.write(value);
                    if *by_ref {
                        self.write(subject);
                    }
                }
                _ => {}
            }
            walk_stmt(self, stmt);
        })
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        if self.anything {
            return;
        }
        syntax::stack::guard(|| {
            let registry = self.registry;
            match &e.kind {
                ExprKind::Assign(lhs, _) | ExprKind::CompoundAssign(_, lhs, _) => self.write(lhs),
                ExprKind::AssignRef(lhs, rhs) => {
                    self.write(lhs);
                    self.write(rhs);
                }
                ExprKind::Unary(
                    UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                    target,
                ) => self.write(target),
                ExprKind::Eval(_) | ExprKind::Include(..) => self.anything = true,
                ExprKind::Call(callee, args) => match &callee.kind {
                    ExprKind::Name(name) => match registry.resolve_function(name) {
                        Some((resolved, sig)) => {
                            self.by_ref_property_args(Some(sig), args);
                            let declared = registry
                                .lookup_function_with_name(resolved)
                                .map_or(resolved, |(declared, _)| declared);
                            self.depend(function_key(declared), Need::Nothing);
                        }
                        None => self.anything = true,
                    },
                    _ => self.anything = true,
                },
                ExprKind::MethodCall(recv, method, args, _) => {
                    match self.resolve_method_call(recv, method) {
                        Some((owner, on_this)) => {
                            let sig = registry.lookup_method(&owner, method);
                            self.by_ref_property_args(sig, args);
                            let need = if on_this {
                                Need::SameAsCaller
                            } else {
                                Need::Nothing
                            };
                            self.depend(method_key(&owner, method), need);
                        }
                        None => self.anything = true,
                    }
                }
                ExprKind::StaticCall(target, method, args) => {
                    let resolved = match (self.class, target.as_name()) {
                        (Some(class), Some(name)) if name.is_keyword("parent") => registry
                            .lookup_ancestor_method_with_owner(class, method)
                            .map(|(owner, sig)| (owner.to_string(), sig, true)),
                        (Some(class), Some(name))
                            if name.is_keyword("self") || name.is_keyword("static") =>
                        {
                            registry
                                .lookup_method_with_owner(class, method)
                                .filter(|(owner, sig)| {
                                    name.is_keyword("self")
                                        || is_non_overridable(registry, class, owner, method, sig)
                                })
                                .map(|(owner, sig)| (owner.to_string(), sig, true))
                        }
                        (_, Some(name)) => name.resolved_class().and_then(|class| {
                            registry
                                .lookup_method_with_owner(class, method)
                                .map(|(owner, sig)| (owner.to_string(), sig, false))
                        }),
                        _ => None,
                    };
                    match resolved {
                        Some((owner, sig, forwards_this)) => {
                            self.by_ref_property_args(Some(sig), args);
                            let is_static =
                                registry.classes.get(owner.as_str()).is_some_and(|info| {
                                    info.static_methods()
                                        .iter()
                                        .any(|m| m.eq_ignore_ascii_case(method))
                                });
                            let need = if forwards_this && !is_static {
                                Need::SameAsCaller
                            } else {
                                Need::Nothing
                            };
                            self.depend(method_key(&owner, method), need);
                        }
                        None => self.anything = true,
                    }
                }
                ExprKind::DynamicMethodCall(..) | ExprKind::DynamicStaticCall(..) => {
                    self.anything = true;
                }
                ExprKind::New(callee, args) => match &callee.kind {
                    ExprKind::Name(name) => {
                        let target = if name.is_keyword("self") || name.is_keyword("static") {
                            self.class.map(str::to_string)
                        } else {
                            name.resolved_class().map(str::to_string)
                        };
                        match target.as_deref().and_then(|t| registry.lookup_class(t)) {
                            Some((owner, _)) => {
                                if let Some((ctor_owner, sig)) =
                                    registry.lookup_method_with_owner(owner, "__construct")
                                {
                                    self.by_ref_property_args(Some(sig), args);
                                    self.depend(
                                        method_key(&ctor_owner, "__construct"),
                                        Need::OwnObject,
                                    );
                                }
                            }
                            None => self.anything = true,
                        }
                    }
                    _ => self.anything = true,
                },
                ExprKind::NewAnonymousClass(class) => {
                    if let Some((owner, sig)) = class.name.as_ref().and_then(|name| {
                        registry.lookup_method_with_owner(name.as_ref(), "__construct")
                    }) {
                        self.by_ref_property_args(Some(sig), &class.args);
                        self.depend(method_key(&owner, "__construct"), Need::OwnObject);
                    }
                }
                _ => {}
            }
            if !self.anything {
                walk_expr(self, e);
            }
        })
    }
}
