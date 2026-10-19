//! Which objects' properties a function or method may write: none that
//! existed before the call, only those of its own `$this`, or any. The
//! checker forgets what it knows about an object's properties after a
//! call that may write them (`Checker::forget_members_a_call_may_change`).
//!
//! Unlike purity (`registry::purity`), output, throwing, exiting or a
//! `void` return don't matter here - only property writes do. A body is
//! classified, when its verdict is asked for, into the property writes it
//! makes itself and the calls whose own writes it inherits; a monotonic
//! search then settles a verdict, a dependency cycle keeping the most
//! conservative one. A call is followed through `$this`, a
//! `$this` property or a parameter never reassigned (by its declared
//! class), `self::`/`static::`/`parent::`, a named class's static
//! method, `new`, and a function by name; any other call may write
//! anything.
//!
//! A builtin (a stub) is taken to write the properties of no object,
//! unless a parameter of it takes something through which it runs code
//! or writes properties (`CODE_RUNNING_PARAMETER_TYPES`).

use rustc_hash::FxHashMap;
#[cfg(debug_assertions)]
use rustc_hash::FxHashSet;
use syntax::name::Name;

use crate::registry::bodies::BodySource;
use crate::registry::name_table::FoldedKey;
use crate::registry::summaries::{CallWrites, Callee, PropertyOf, Receiver, WriteFacts};
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

/// What a stub signature's verdict (`stub_verdict`) was the first time it
/// was asked, kept with the signature: the same few thousand builtins are
/// asked about hundreds of thousands of times. It is no part of what a
/// signature is, so equal signatures are equal whether or not it is
/// filled, and a copy of a signature starts without it, since the copy's
/// parameters may yet change.
#[derive(Debug, Default)]
pub(in crate::registry) struct StubWrites(std::sync::OnceLock<PropertyWrites>);

impl PartialEq for StubWrites {
    fn eq(&self, _: &StubWrites) -> bool {
        true
    }
}

/// A declaration, keyed case-insensitively: its class and method for a
/// method, its name alone for a function. The names are held lowercased, and
/// a declaration is found by names in any case without making them again.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DeclKey {
    pub(in crate::registry) class: Option<String>,
    pub(in crate::registry) member: String,
}

/// What a `DeclKey` is looked up by: the class and member as written.
pub(in crate::registry) struct DeclRef<'a> {
    pub(in crate::registry) class: Option<&'a str>,
    pub(in crate::registry) member: &'a str,
}

/// The two ways of naming a declaration, which hash and compare alike.
pub(in crate::registry) trait DeclQuery {
    fn class(&self) -> Option<&str>;
    fn member(&self) -> &str;
}

impl DeclQuery for DeclKey {
    fn class(&self) -> Option<&str> {
        self.class.as_deref()
    }

    fn member(&self) -> &str {
        &self.member
    }
}

impl DeclQuery for DeclRef<'_> {
    fn class(&self) -> Option<&str> {
        self.class
    }

    fn member(&self) -> &str {
        self.member
    }
}

impl std::hash::Hash for dyn DeclQuery + '_ {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let fold = |text: &str, state: &mut H| {
            for byte in text.bytes() {
                state.write_u8(byte.to_ascii_lowercase());
            }
            state.write_u8(0xff);
        };
        match self.class() {
            Some(class) => {
                state.write_u8(1);
                fold(class, state);
            }
            None => state.write_u8(0),
        }
        fold(self.member(), state);
    }
}

impl PartialEq for dyn DeclQuery + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.member().eq_ignore_ascii_case(other.member())
            && match (self.class(), other.class()) {
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                (None, None) => true,
                _ => false,
            }
    }
}

impl Eq for dyn DeclQuery + '_ {}

impl std::hash::Hash for DeclKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self as &dyn DeclQuery).hash(state);
    }
}

impl PartialEq for DeclKey {
    fn eq(&self, other: &DeclKey) -> bool {
        (self as &dyn DeclQuery) == (other as &dyn DeclQuery)
    }
}

impl Eq for DeclKey {}

impl<'a> std::borrow::Borrow<dyn DeclQuery + 'a> for DeclKey {
    fn borrow(&self) -> &(dyn DeclQuery + 'a) {
        self
    }
}

pub(in crate::registry) fn function_key(name: &str) -> DeclKey {
    DeclKey {
        class: None,
        member: name.to_ascii_lowercase(),
    }
}

pub(in crate::registry) fn method_key(class: &str, method: &str) -> DeclKey {
    DeclKey {
        class: Some(class.to_ascii_lowercase()),
        member: method.to_ascii_lowercase(),
    }
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
#[derive(Debug, Default, Clone, PartialEq)]
pub(in crate::registry) struct Classification {
    writes_own_object: bool,
    deps: Vec<(DeclKey, Need)>,
}

/// What the search for a declaration's verdict finds at it.
enum Settled {
    /// A body to follow the calls of.
    Body(Classification),
    /// Its verdict outright: a builtin's, or `Anything`.
    Verdict(PropertyWrites),
}

/// A declaration the search has entered and not left.
struct Frame {
    key: DeclKey,
    classification: Classification,
    /// The next of the classification's calls to follow.
    next: usize,
    /// The verdicts of the calls followed so far, in their order.
    seen: Vec<PropertyWrites>,
}

impl Frame {
    /// The lowest verdict the declaration's own writes and the verdicts of
    /// its calls allow.
    fn settle(&self) -> PropertyWrites {
        for candidate in [PropertyWrites::Nothing, PropertyWrites::OwnObject] {
            if candidate == PropertyWrites::Nothing && self.classification.writes_own_object {
                continue;
            }
            let satisfied =
                self.classification
                    .deps
                    .iter()
                    .zip(&self.seen)
                    .all(|((_, need), verdict)| match need {
                        Need::Nothing => *verdict == PropertyWrites::Nothing,
                        Need::OwnObject => *verdict <= PropertyWrites::OwnObject,
                        Need::SameAsCaller => *verdict <= candidate,
                    });
            if satisfied {
                return candidate;
            }
        }
        PropertyWrites::Anything
    }
}

impl Registry {
    /// The declaration `key` names, when it is a function or method of the
    /// project with a body: with its declaring class's name for a method.
    fn writing_declaration(&self, key: &DeclKey) -> Option<(Option<&Name>, &FunctionReflection)> {
        match &key.class {
            None => self
                .functions
                .get(key.member.as_str())
                .filter(|sig| !sig.is_stub)
                .map(|sig| (None, sig)),
            Some(class) => {
                let (name, info) = self.classes.get_with_name(&FoldedKey(class.as_str()))?;
                let sig = info
                    .methods
                    .get(key.member.as_str())
                    .filter(|sig| !sig.is_stub)?;
                Some((Some(name), sig))
            }
        }
    }

    /// What a declaration's verdict starts from: its body's classification,
    /// or the verdict a declaration with no body of the project has.
    fn writes_settled(&self, key: &DeclKey, bodies: &dyn BodySource) -> Settled {
        match self.writing_declaration(key) {
            Some((class, sig)) => match classify_summary(bodies, class, sig, self) {
                Some(classification) => Settled::Body(classification),
                None => Settled::Verdict(PropertyWrites::Anything),
            },
            None => Settled::Verdict(self.outside_verdict(key)),
        }
    }

    /// A declaration without a body of the project: a stub's verdict by its
    /// parameters, anything else's `Anything`.
    fn outside_verdict(&self, dep: &DeclKey) -> PropertyWrites {
        let sig = match &dep.class {
            Some(class) => self.lookup_method(class, &dep.member),
            None => self.functions.get(dep.member.as_str()),
        };
        match sig {
            Some(sig) if sig.is_stub => stub_verdict(sig),
            _ => PropertyWrites::Anything,
        }
    }

    /// The verdict of `root`, a declaration with a body, found by following
    /// the calls its body makes and remembered for every declaration the
    /// search settles. A call back into a declaration the search is inside of
    /// is a cycle, whose verdict is `Anything`: a verdict only goes down from
    /// `Anything` once the verdicts it needs have, and none in a cycle can
    /// go first. So a verdict does not depend on where the search began.
    pub(in crate::registry) fn declaration_writes(
        &self,
        root: DeclKey,
        bodies: &dyn BodySource,
    ) -> PropertyWrites {
        // `None` while the search is inside the declaration.
        let mut done: FxHashMap<DeclKey, Option<PropertyWrites>> = FxHashMap::default();
        let mut settled: Vec<(DeclKey, PropertyWrites)> = Vec::new();
        let mut stack: Vec<Frame> = Vec::new();
        let enter = |key: DeclKey,
                     done: &mut FxHashMap<DeclKey, Option<PropertyWrites>>|
         -> Result<PropertyWrites, Frame> {
            if let Some(verdict) = self.demand.writes_of(&key) {
                return Ok(verdict);
            }
            match done.get(&key) {
                Some(Some(verdict)) => return Ok(*verdict),
                Some(None) => return Ok(PropertyWrites::Anything),
                None => {}
            }
            match self.writes_settled(&key, bodies) {
                Settled::Verdict(verdict) => {
                    done.insert(key, Some(verdict));
                    Ok(verdict)
                }
                Settled::Body(classification) => {
                    done.insert(key.clone(), None);
                    Err(Frame {
                        key,
                        classification,
                        next: 0,
                        seen: Vec::new(),
                    })
                }
            }
        };
        match enter(root, &mut done) {
            Ok(verdict) => return verdict,
            Err(frame) => stack.push(frame),
        }
        let mut returned: Option<PropertyWrites> = None;
        let verdict = loop {
            let top = stack
                .last_mut()
                .expect("the search is inside a declaration");
            if let Some(verdict) = returned.take() {
                top.seen.push(verdict);
            }
            // A call that may write anything leaves no verdict below it.
            let ended = if top.seen.contains(&PropertyWrites::Anything) {
                Some(PropertyWrites::Anything)
            } else if top.next == top.classification.deps.len() {
                Some(top.settle())
            } else {
                None
            };
            match ended {
                Some(verdict) => {
                    let frame = stack.pop().expect("the search is inside a declaration");
                    done.insert(frame.key.clone(), Some(verdict));
                    settled.push((frame.key, verdict));
                    if stack.is_empty() {
                        break verdict;
                    }
                    returned = Some(verdict);
                }
                None => {
                    let dep = top.classification.deps[top.next].0.clone();
                    top.next += 1;
                    match enter(dep, &mut done) {
                        Ok(verdict) => returned = Some(verdict),
                        Err(frame) => stack.push(frame),
                    }
                }
            }
        };
        self.demand.record_writes(settled);
        verdict
    }

    /// Whose properties calling function `name` may write.
    pub(crate) fn function_property_writes(
        &self,
        name: &str,
        bodies: &dyn BodySource,
    ) -> PropertyWrites {
        let verdict = match self.functions.get(name) {
            Some(sig) if sig.is_stub => stub_verdict(sig),
            Some(_) => {
                let query = DeclRef {
                    class: None,
                    member: name,
                };
                match self.demand.writes_of(&query) {
                    Some(verdict) => verdict,
                    None => self.declaration_writes(function_key(name), bodies),
                }
            }
            None => PropertyWrites::Anything,
        };
        #[cfg(debug_assertions)]
        self.assert_writes_as_built(&function_key(name), verdict, bodies);
        verdict
    }

    /// Whose properties calling `owner`'s `method` - the declaration a
    /// call reaches, not an override of it - may write.
    pub(crate) fn method_property_writes(
        &self,
        owner: &str,
        method: &str,
        bodies: &dyn BodySource,
    ) -> PropertyWrites {
        let verdict = match self.lookup_method(owner, method) {
            Some(sig) if sig.is_stub => stub_verdict(sig),
            Some(_) => {
                let query = DeclRef {
                    class: Some(owner),
                    member: method,
                };
                match self.demand.writes_of(&query) {
                    Some(verdict) => verdict,
                    None => self.declaration_writes(method_key(owner, method), bodies),
                }
            }
            None => PropertyWrites::Anything,
        };
        #[cfg(debug_assertions)]
        self.assert_writes_as_built(&method_key(owner, method), verdict, bodies);
        verdict
    }

    /// The verdict `key` has among the ones built for every declaration at
    /// once, which a verdict found when asked has to equal.
    #[cfg(debug_assertions)]
    fn assert_writes_as_built(
        &self,
        key: &DeclKey,
        found: PropertyWrites,
        bodies: &dyn BodySource,
    ) {
        let Some(built) = &self.as_built else {
            return;
        };
        // Only where the bodies at hand are those the verdicts were built from.
        let Some((_, sig)) = self.writing_declaration(key) else {
            return;
        };
        if bodies.summary(&sig.provenance).is_none() {
            return;
        }
        let expected = built
            .writes
            .get(key)
            .copied()
            .unwrap_or(PropertyWrites::Anything);
        assert_eq!(
            found, expected,
            "the verdict found when asked and the one built for every declaration differ for {key:?}"
        );
    }

    /// The verdict of every function and method with a body, settled for
    /// all of them at once: the answer the verdicts found when asked are
    /// checked against.
    #[cfg(debug_assertions)]
    pub(in crate::registry) fn writes_of_every_declaration(
        &self,
        bodies: &dyn BodySource,
    ) -> FxHashMap<DeclKey, PropertyWrites> {
        let mut classified: Vec<(DeclKey, Option<Classification>)> = self
            .functions
            .own_iter()
            .filter(|(_, sig)| !sig.is_stub)
            .map(|(name, sig)| {
                (
                    function_key(name),
                    classify_summary(bodies, None, sig, self),
                )
            })
            .collect();
        for (class, info) in self.classes.own_iter() {
            for (method, sig) in info.methods.iter().filter(|(_, sig)| !sig.is_stub) {
                classified.push((
                    method_key(class, method),
                    classify_summary(bodies, Some(class), sig, self),
                ));
            }
        }
        let classified: FxHashMap<DeclKey, Option<Classification>> =
            classified.into_iter().collect();
        let mut verdicts: FxHashMap<DeclKey, PropertyWrites> = classified
            .keys()
            .map(|key| (key.clone(), PropertyWrites::Anything))
            .collect();
        // A verdict only ever goes down, and whether a lower one holds only
        // gets easier as the verdicts it depends on go down. So every
        // declaration is settled once, and again only when a declaration
        // it calls has gone down.
        let mut callers: FxHashMap<&DeclKey, Vec<&DeclKey>> = FxHashMap::default();
        let mut queue: std::collections::VecDeque<&DeclKey> = std::collections::VecDeque::new();
        for (key, classification) in &classified {
            let Some(classification) = classification else {
                continue;
            };
            queue.push_back(key);
            for (dep, _) in &classification.deps {
                if verdicts.contains_key(dep) {
                    callers.entry(dep).or_default().push(key);
                }
            }
        }
        let mut queued: FxHashSet<&DeclKey> = queue.iter().copied().collect();
        while let Some(key) = queue.pop_front() {
            queued.remove(key);
            let Some(Some(classification)) = classified.get(key) else {
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
                    let verdict = match verdicts.get(dep) {
                        Some(verdict) => *verdict,
                        None => self.outside_verdict(dep),
                    };
                    match need {
                        Need::Nothing => verdict == PropertyWrites::Nothing,
                        Need::OwnObject => verdict <= PropertyWrites::OwnObject,
                        Need::SameAsCaller => verdict <= candidate,
                    }
                });
                if satisfied {
                    verdicts.insert(key.clone(), candidate);
                    for caller in callers.get(key).into_iter().flatten() {
                        if queued.insert(caller) {
                            queue.push_back(caller);
                        }
                    }
                    break;
                }
            }
        }
        verdicts
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
    *sig.stub_writes.0.get_or_init(|| {
        let runs_code = sig
            .params
            .iter()
            .any(|p| p.ty.mentions_any(&CODE_RUNNING_PARAMETER_TYPES));
        if runs_code {
            PropertyWrites::Anything
        } else {
            PropertyWrites::Nothing
        }
    })
}

/// The classification of the body of `sig`, from what the passes read of
/// it, or `None` when it may write any object's properties whatever it
/// calls, or when the body is not at hand. `class` is the declaring class of
/// a method.
fn classify_summary(
    bodies: &dyn BodySource,
    class: Option<&Name>,
    sig: &FunctionReflection,
    registry: &Registry,
) -> Option<Classification> {
    let summary = bodies.summary(&sig.provenance)?;
    let classification = classify_facts(&summary.writes, class, sig, registry);
    // The answer from the statements themselves, for the bodies at hand.
    #[cfg(debug_assertions)]
    if let Some(body) = bodies.body(&sig.provenance) {
        assert_eq!(
            classification,
            reference::classify(body, class, sig, registry),
            "the summary of a body and the body disagree on its property writes"
        );
    }
    classification
}

/// The classification of a body that makes the calls `facts` lists.
fn classify_facts(
    facts: &WriteFacts,
    class: Option<&Name>,
    sig: &FunctionReflection,
    registry: &Registry,
) -> Option<Classification> {
    if facts.anything {
        return None;
    }
    let params: FxHashMap<&str, &Type> = sig
        .params
        .iter()
        .filter(|p| !p.by_ref && !facts.reassigned.iter().any(|name| name == p.name.as_ref()))
        .map(|p| (p.name.as_ref(), &p.ty))
        .collect();
    let mut resolve = Resolve {
        class: class.map(|c| c.as_ref()),
        registry,
        params,
        classification: Classification {
            writes_own_object: facts.writes_own_object,
            deps: Vec::new(),
        },
    };
    for call in &facts.calls {
        resolve.call(call)?;
    }
    Some(resolve.classification)
}

/// Looks the calls of a body up in the registry, one by one.
struct Resolve<'r> {
    class: Option<&'r str>,
    registry: &'r Registry,
    params: FxHashMap<&'r str, &'r Type>,
    classification: Classification,
}

impl Resolve<'_> {
    /// The class a receiver holds, by its declared type: `$this`, a
    /// property of `$this`, or a parameter never reassigned.
    fn receiver_class(&self, receiver: &Receiver) -> Option<String> {
        let ty = match receiver {
            Receiver::This => return self.class.map(str::to_string),
            Receiver::Variable(v) => (*self.params.get(v.as_ref())?).clone(),
            Receiver::ThisProperty(prop) => self
                .registry
                .lookup_property(self.class?, prop.as_ref())?
                .ty()
                .clone(),
        };
        let (class, _) = ty.as_object()?;
        Some(class.to_string())
    }

    /// The by-reference parameters `sig` passes a property to among the
    /// arguments that are properties: each a property write. `None` when
    /// one writes another object's.
    fn by_ref_properties(
        &mut self,
        sig: Option<&FunctionReflection>,
        properties: &[(u32, PropertyOf)],
    ) -> Option<()> {
        for (at, whose) in properties {
            let by_ref = match sig {
                Some(sig) => sig
                    .params
                    .get(*at as usize)
                    .or_else(|| sig.params.last().filter(|p| p.variadic))
                    .is_some_and(|p| p.by_ref),
                None => true,
            };
            if by_ref {
                match whose {
                    PropertyOf::This => self.classification.writes_own_object = true,
                    PropertyOf::Other => return None,
                }
            }
        }
        Some(())
    }

    /// `None` when the call may write any object's properties.
    fn call(&mut self, call: &CallWrites) -> Option<()> {
        let registry = self.registry;
        let class = self.class;
        match &call.callee {
            Callee::Function(name) => {
                let (resolved, sig) = registry.resolve_function(name)?;
                self.by_ref_properties(Some(sig), &call.properties)?;
                let declared = registry
                    .lookup_function_with_name(resolved)
                    .map_or(resolved, |(declared, _)| declared);
                self.classification
                    .deps
                    .push((function_key(declared), Need::Nothing));
            }
            Callee::Method { receiver, method } => {
                let receiver_class = self.receiver_class(receiver)?;
                let (owner, sig) = registry.lookup_method_with_owner(&receiver_class, method)?;
                if !is_non_overridable(registry, &receiver_class, &owner, method, sig) {
                    return None;
                }
                let sig = registry.lookup_method(&owner, method);
                self.by_ref_properties(sig, &call.properties)?;
                let need = if matches!(receiver, Receiver::This) {
                    Need::SameAsCaller
                } else {
                    Need::Nothing
                };
                self.classification
                    .deps
                    .push((method_key(&owner, method), need));
            }
            Callee::Static(name, method) => {
                let resolved = match class {
                    Some(class) if name.is_keyword("parent") => registry
                        .lookup_ancestor_method_with_owner(class, method)
                        .map(|(owner, sig)| (owner.to_string(), sig, true)),
                    Some(class) if name.is_keyword("self") || name.is_keyword("static") => registry
                        .lookup_method_with_owner(class, method)
                        .filter(|(owner, sig)| {
                            name.is_keyword("self")
                                || is_non_overridable(registry, class, owner, method, sig)
                        })
                        .map(|(owner, sig)| (owner.to_string(), sig, true)),
                    _ => name.resolved_class().and_then(|class| {
                        registry
                            .lookup_method_with_owner(class, method)
                            .map(|(owner, sig)| (owner.to_string(), sig, false))
                    }),
                };
                let (owner, sig, forwards_this) = resolved?;
                self.by_ref_properties(Some(sig), &call.properties)?;
                let is_static = registry.classes.get(owner.as_str()).is_some_and(|info| {
                    info.static_methods()
                        .iter()
                        .any(|m| m.eq_ignore_ascii_case(method))
                });
                let need = if forwards_this && !is_static {
                    Need::SameAsCaller
                } else {
                    Need::Nothing
                };
                self.classification
                    .deps
                    .push((method_key(&owner, method), need));
            }
            Callee::New(name) => {
                let target = if name.is_keyword("self") || name.is_keyword("static") {
                    class.map(str::to_string)
                } else {
                    name.resolved_class().map(str::to_string)
                };
                let (owner, _) = registry.lookup_class(target.as_deref()?)?;
                if let Some((ctor_owner, sig)) =
                    registry.lookup_method_with_owner(owner, "__construct")
                {
                    self.by_ref_properties(Some(sig), &call.properties)?;
                    self.classification
                        .deps
                        .push((method_key(&ctor_owner, "__construct"), Need::OwnObject));
                }
            }
            Callee::Anonymous(name) => {
                if let Some((owner, sig)) = name.as_ref().and_then(|name| {
                    registry.lookup_method_with_owner(name.as_ref(), "__construct")
                }) {
                    self.by_ref_properties(Some(sig), &call.properties)?;
                    self.classification
                        .deps
                        .push((method_key(&owner, "__construct"), Need::OwnObject));
                }
            }
        }
        Some(())
    }
}

/// The statements' own answer, which the summary of a body has to agree
/// with.
#[cfg(debug_assertions)]
mod reference {
    use super::*;
    use rustc_hash::FxHashSet;
    use syntax::ast::*;
    use syntax::visit::{Visitor, walk_expr, walk_stmt, walk_stmts};

    /// `body`'s classification, from its statements, as it was before it was
    /// read from a summary: what the summary has to agree with.
    pub(super) fn classify(
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
        stack::guard(|| match &target.kind {
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
        })
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
        stack::guard(|| match &target.peeled().kind {
            ExprKind::PropertyAccess(base, _, _) | ExprKind::DynamicPropertyAccess(base, _, _) => {
                Some(matches!(&base.peeled().kind, ExprKind::Variable(v) if **v == *"this"))
            }
            ExprKind::Index(base, _) => property_target_owner(base),
            _ => None,
        })
    }

    impl Scan<'_> {
        fn write(&mut self, target: &Expr) {
            stack::guard(|| match target.peeled().kind {
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
            })
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
            stack::guard(|| {
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
            stack::guard(|| {
                let registry = self.registry;
                match &e.kind {
                    ExprKind::Assign(lhs, _) | ExprKind::CompoundAssign(_, lhs, _) => {
                        self.write(lhs)
                    }
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
                                            || is_non_overridable(
                                                registry, class, owner, method, sig,
                                            )
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
}
