//! What a call may write: the storages the code it runs may change, so the
//! checker forgets what it knew of them after it.
//!
//! A storage is a property of an object (by name, on the callee's own
//! `$this` or on objects that may be of a class), a static property (by the
//! class declaring it), a global variable, or a declaration's static
//! variable. An effect is a set of storages, or anything: what code that is
//! not known may write.
//!
//! A body's effect is what it writes itself and what the calls it makes may
//! write, settled by a fixed point over the call graph that starts from
//! nothing. A call to an overridable method, or through an interface,
//! reaches every implementation the run loads, and a call on a value whose
//! class is not known every loaded method of its name: the program is what
//! the run loads. A builtin writes nothing unless a parameter of it takes
//! something through which it runs code (`CODE_RUNNING_PARAMETER_TYPES`).
//!
//! A closure, an arrow function or a first-class callable a body writes has
//! an effect of its own, settled with the body's, and counts where the body
//! may run it: called, `__invoke` or `call` on a variable holding it, or
//! passed to a builtin that runs it. Its parameters hold what their declared
//! classes admit until it writes them, and what it takes by reference is the
//! body's variable.
//!
//! A body that runs a parameter's argument - calls it, or passes it to a
//! builtin that runs it - holds that as a condition of its effect, as does a
//! body passing its parameter on to a callee that runs it. Each call decides
//! it by what it passes: a closure the caller wrote by that closure's effect,
//! a function or a method a literal names by theirs, a parameter of the
//! caller's by a condition of its own, nothing for a required parameter not
//! passed or a default of `null`, and anything otherwise.
//!
//! An operation PHP runs a value's code for is a call to that code: a
//! property's hook, or a magic method where the scope sees no property, an
//! `ArrayAccess` method for an element, `__toString` for a conversion,
//! `__clone`, an iterator's methods. It reaches what the hierarchy over the
//! value's classes declares, or every loaded declaration of it when the
//! class is not known. A hook's body is a declaration of its own, named
//! after its property (`hook_member`).

use std::collections::BTreeSet;
use std::sync::Arc;

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::{ClassKind, QualifiedName, ResolvedName, Visibility};
use syntax::name::Name;

use crate::registry::bodies::BodySource;
use crate::registry::summaries::{
    Access, ArgAt, Binding, BodySummary, CallWrites, Callable, Callee, ClosureFacts, Made,
    Operation, Origin, PairTarget, Place, Receiver, Root, Step, WriteFacts,
};
use crate::registry::{
    ClassReflection, FunctionReflection, NameSet, PropertyReflection, Registry, is_non_overridable,
};
use types::Type;

/// Storage a write may change.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub(crate) enum Storage {
    /// A property of the callee's own `$this`; `None` for any.
    Own(Option<Name>),
    /// A property of an object that may be of `class`, or of any object;
    /// `None` for any property.
    Property {
        class: Option<Name>,
        prop: Option<Name>,
    },
    /// A static property, by the class declaring it, or of any class; `None`
    /// for any name.
    Static {
        class: Option<Name>,
        prop: Option<Name>,
    },
    /// A global variable; `None` for any.
    Global(Option<Name>),
    /// A static variable of a function, or of a class's method; `None` for
    /// any of its static variables.
    StaticVar {
        class: Option<Name>,
        function: Name,
        var: Option<Name>,
    },
}

/// What code may write: a set of storages, or anything.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Effect(Option<BTreeSet<Storage>>);

impl Default for Effect {
    fn default() -> Effect {
        Effect::nothing()
    }
}

impl Effect {
    pub(crate) fn nothing() -> Effect {
        Effect(Some(BTreeSet::new()))
    }

    pub(crate) fn anything() -> Effect {
        Effect(None)
    }

    pub(crate) fn is_anything(&self) -> bool {
        self.0.is_none()
    }

    pub(crate) fn is_nothing(&self) -> bool {
        self.0.as_ref().is_some_and(BTreeSet::is_empty)
    }

    /// The storages written, when they are known.
    pub(crate) fn storages(&self) -> Option<&BTreeSet<Storage>> {
        self.0.as_ref()
    }

    /// This effect of a method, called on an object that may be of `class`
    /// (of any class, when `None`): its own object's properties are that
    /// object's.
    pub(crate) fn on_object(&self, class: Option<&Name>) -> Effect {
        self.transferred(&Transfer::Other(class.cloned()))
    }

    fn insert(&mut self, storage: Storage) {
        if let Some(storages) = &mut self.0 {
            storages.insert(storage);
        }
    }

    /// Adds what `other` writes; whether that added anything.
    pub(crate) fn join(&mut self, other: &Effect) -> bool {
        match (&mut self.0, &other.0) {
            (None, _) => false,
            (Some(_), None) => {
                self.0 = None;
                true
            }
            (Some(storages), Some(added)) => {
                let before = storages.len();
                storages.extend(added.iter().cloned());
                storages.len() != before
            }
        }
    }

    /// This effect, written by a call whose callee's own `$this` is the
    /// object `transfer` says.
    pub(crate) fn transferred(&self, transfer: &Transfer) -> Effect {
        let Some(storages) = &self.0 else {
            return Effect::anything();
        };
        let mut out = BTreeSet::new();
        for storage in storages {
            match (storage, transfer) {
                (Storage::Own(prop), Transfer::Other(class)) => {
                    out.insert(Storage::Property {
                        class: class.clone(),
                        prop: prop.clone(),
                    });
                }
                (Storage::Own(_), Transfer::Made) => {}
                (Storage::Own(prop), Transfer::Static) => {
                    out.insert(Storage::Property {
                        class: None,
                        prop: prop.clone(),
                    });
                }
                (storage, _) => {
                    out.insert(storage.clone());
                }
            }
        }
        Effect(Some(out))
    }
}

/// What a call's callee's own `$this` is to the caller.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub(crate) enum Transfer {
    /// The caller's own `$this`.
    Same,
    /// Another object, which may be of the class; `None` for any.
    Other(Option<Name>),
    /// An object the caller made, which no one else has seen.
    Made,
    /// No object: a function, or a static method.
    Static,
}

impl Transfer {
    /// This object, said in a callee whose own `$this` is the object
    /// `outer` says to its caller, as the caller says it.
    fn through(&self, outer: &Transfer) -> Transfer {
        match self {
            Transfer::Same => outer.clone(),
            other => other.clone(),
        }
    }
}

/// A parameter whose argument a declaration may run: what that writes is
/// for each call to decide, by what it passes there.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct Condition {
    /// The parameter's position.
    at: u32,
    /// The object the argument runs on when the declaration binds it to one
    /// (`Closure::call`); `None` for the argument's own `$this`.
    this: Option<Transfer>,
}

impl Condition {
    /// This condition of a callee whose own `$this` is what `transfer` says,
    /// as its caller has it.
    fn through(&self, transfer: &Transfer) -> Condition {
        Condition {
            at: self.at,
            this: self.this.as_ref().map(|this| this.through(transfer)),
        }
    }

    /// The `$this` the argument runs on, to the caller of a callee whose own
    /// `$this` is what `transfer` says: the object the callee binds it to,
    /// or `own`, the argument's own.
    fn bound(&self, transfer: &Transfer, own: &Transfer) -> Transfer {
        self.this
            .as_ref()
            .map_or_else(|| own.clone(), |this| this.through(transfer))
    }
}

/// What a call of a declaration may write: what its body and the calls it
/// makes write, and what the arguments it runs write, which each call
/// decides by what it passes.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Settled {
    effect: Effect,
    /// The parameters whose arguments it may run.
    runs: BTreeSet<Condition>,
}

impl Settled {
    fn of(effect: Effect) -> Settled {
        Settled {
            effect,
            runs: BTreeSet::new(),
        }
    }

    /// What a call of it may write when what it passes is not known.
    fn unconditional(&self) -> Effect {
        if self.runs.is_empty() {
            self.effect.clone()
        } else {
            Effect::anything()
        }
    }
}

/// A declaration, keyed case-insensitively: its class and method for a
/// method, its name alone for a function. The names are held lowercased, and
/// a key with them in any case finds the declaration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DeclKey {
    pub(in crate::registry) class: Option<String>,
    pub(in crate::registry) member: String,
}

/// A settled body effect addressed by its declaration, without hashing the
/// declaration's class or member name on every lookup. Synthetic property
/// hooks do not yet have a property slot, so they retain their folded key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum EffectId {
    Function(crate::registry::FnId),
    Method(crate::registry::MethodRef),
    Named(DeclKey),
}

impl EffectId {
    fn is_closure(&self) -> bool {
        matches!(self, EffectId::Named(key) if key.is_closure())
    }

    pub(crate) fn with_generation(self, generation: u32) -> EffectId {
        match self {
            EffectId::Function(id) => EffectId::Function(id.with_generation(generation)),
            EffectId::Method(method) => EffectId::Method(crate::registry::MethodRef::new(
                method.class().with_generation(generation),
                method.slot(),
            )),
            EffectId::Named(key) => EffectId::Named(key),
        }
    }
}

/// The names a `DeclKey` hashes and compares by, case-insensitively.
trait DeclQuery {
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
    fn eq(&self, other: &Self) -> bool {
        (self as &dyn DeclQuery) == (other as &dyn DeclQuery)
    }
}

impl Eq for DeclKey {}

impl<'a> std::borrow::Borrow<dyn DeclQuery + 'a> for DeclKey {
    fn borrow(&self) -> &(dyn DeclQuery + 'a) {
        self
    }
}

impl DeclKey {
    /// The key of the closure, arrow function or first-class callable this
    /// declaration's body lists at `index`: a name no declaration has.
    fn closure(&self, index: u32) -> DeclKey {
        DeclKey {
            class: self.class.clone(),
            member: format!("{}{{closure#{index}}}", self.member),
        }
    }

    /// Whether this is a closure's key (`DeclKey::closure`).
    fn is_closure(&self) -> bool {
        self.member.contains('{')
    }

    /// The name the builtin table knows the declaration by: a function's,
    /// or `class::method`, lowercased.
    fn builtin_name(&self) -> String {
        match &self.class {
            Some(class) => format!("{class}::{}", self.member),
            None => self.member.clone(),
        }
    }
}

fn function_key(name: &str) -> DeclKey {
    DeclKey {
        class: None,
        member: name.to_ascii_lowercase(),
    }
}

fn method_key(class: &str, method: &str) -> DeclKey {
    DeclKey {
        class: Some(class.to_ascii_lowercase()),
        member: method.to_ascii_lowercase(),
    }
}

/// The name a hook of the property `property`, its `set` hook when `set`,
/// is known by among methods: PHP's (`$property::get`), lowercased, with a
/// `^` before each letter that was a capital, so that two properties whose
/// names differ only in case keep apart. `None` for any property's, which
/// only the index of names (`methods_by_name`) lists.
fn hook_member(property: Option<&str>, set: bool) -> String {
    let mut member = String::from("$");
    for c in property.unwrap_or_default().chars() {
        if c.is_ascii_uppercase() {
            member.push('^');
        }
        member.push(c.to_ascii_lowercase());
    }
    member.push_str(if set { "::set" } else { "::get" });
    member
}

/// The declaration of `class`'s hook of `property`, its `set` hook when
/// `set`.
fn hook_key(class: &str, property: &str, set: bool) -> DeclKey {
    DeclKey {
        class: Some(class.to_ascii_lowercase()),
        member: hook_member(Some(property), set),
    }
}

/// Whether code of the class `scope` sees a member `owner` declares with
/// `visibility`.
fn sees(registry: &Registry, owner: &str, visibility: Visibility, scope: Option<&str>) -> bool {
    match visibility {
        Visibility::Public => true,
        Visibility::Protected => scope.is_some_and(|scope| {
            scope.eq_ignore_ascii_case(owner)
                || registry.is_subclass_of(scope, owner)
                || registry.is_subclass_of(owner, scope)
        }),
        Visibility::Private => scope.is_some_and(|scope| scope.eq_ignore_ascii_case(owner)),
    }
}

/// Where a builtin runs code: at a parameter, or through the elements of an
/// array passed there.
#[derive(Debug, Clone, Copy)]
struct Position {
    at: u32,
    elements: bool,
}

/// What a builtin runs through an argument.
#[derive(Debug, Clone, Copy)]
enum BuiltinRun {
    /// It invokes the callable.
    Calls,
    /// It calls this method (lowercased) of the object.
    Method(&'static str),
    /// It runs this stream-wrapper method.
    Wrapper(&'static str),
}

/// What each builtin runs, as `tools/builtin-runs/record.php` found it:
/// the arrays `build.rs` makes of its table, sorted by name, read with no
/// parse.
mod builtin_runs {
    use super::{BuiltinRun, Position};

    include!(concat!(env!("OUT_DIR"), "/builtin_runs.rs"));

    /// Whether a call of the builtin (a function or `class::method`,
    /// lowercased) was reached.
    pub(super) fn probed(name: &str) -> bool {
        PROBED.binary_search(&name).is_ok()
    }

    /// What the builtin was seen to run through its arguments.
    pub(super) fn runs(name: &str) -> &'static [(Position, BuiltinRun)] {
        RUNS.binary_search_by(|&(entry, _)| entry.cmp(name))
            .map_or(&[], |at| RUNS[at].1)
    }

    /// Whether a method of the class (lowercased) ran a callable or an
    /// object's method its object was made with.
    pub(super) fn holding(class: &str) -> bool {
        HOLDING.binary_search(&class).is_ok()
    }

    /// The stream-wrapper methods (lowercased) a method of the class
    /// (lowercased) ran through a URL its object was made with.
    pub(super) fn held_wrappers(class: &str) -> &'static [&'static str] {
        HELD_WRAPPERS
            .binary_search_by(|&(entry, _)| entry.cmp(class))
            .map_or(&[], |at| HELD_WRAPPERS[at].1)
    }
}

/// What a stub signature's effect (`stub_effect`) was the first time it was
/// asked, kept with the signature: the same few thousand builtins are asked
/// about hundreds of thousands of times. It is no part of what a signature
/// is, so equal signatures are equal whether or not it is filled, and a copy
/// of a signature starts without it, since the copy's parameters may yet
/// change.
#[derive(Debug, Default)]
pub(in crate::registry) struct StubEffect(std::sync::OnceLock<bool>);

impl PartialEq for StubEffect {
    fn eq(&self, _: &StubEffect) -> bool {
        true
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

/// The method declarations of every loaded class with a body, by name
/// (lowercased): what a call on a value whose class is not known may reach.
/// The hooks with a body are listed too, each by its name (`hook_member`)
/// and by the name of any property's. Each name's classes are sorted, so
/// what is made of them does not depend on the order the classes were
/// declared in.
fn methods_by_name(registry: &Registry) -> FxHashMap<String, Vec<crate::registry::ClassId>> {
    let mut by_name: FxHashMap<String, Vec<crate::registry::ClassId>> = FxHashMap::default();
    for (class, info) in registry.classes().own_iter() {
        let Some(class_id) = registry.class_ids.get(class).copied() else {
            continue;
        };
        for (method, sig) in info.methods.iter() {
            if !sig.is_stub {
                by_name
                    .entry(method.to_ascii_lowercase())
                    .or_default()
                    .push(class_id);
            }
        }
        for property in registry.property_names(class) {
            let Some(declared) = registry.lookup_property(class, property) else {
                continue;
            };
            for (set, body) in [
                (false, declared.get_hook_body),
                (true, declared.set_hook_body),
            ] {
                if !body {
                    continue;
                }
                for member in [hook_member(Some(property), set), hook_member(None, set)] {
                    let classes = by_name.entry(member).or_default();
                    if classes.last() != Some(&class_id) {
                        classes.push(class_id);
                    }
                }
            }
        }
    }
    for classes in by_name.values_mut() {
        classes.sort_unstable_by(|a, b| {
            let a = registry.class_name_by_id(*a).expect("active class ID");
            let b = registry.class_name_by_id(*b).expect("active class ID");
            (a.lower(), a.as_str()).cmp(&(b.lower(), b.as_str()))
        });
    }
    by_name
}

/// Whether `class`'s `method` has no body of its own to run: a builtin's, an
/// abstract or an interface method, or one a `@method` tag or an enum
/// declares, whose calls run `__call` or PHP's own code.
fn never_runs(class: &ClassReflection, method: &str, sig: &FunctionReflection) -> bool {
    sig.is_stub
        || sig.is_magic
        || class.kind() == ClassKind::Interface
        || class.is_abstract_method(method)
}

/// One body's own writes and the calls it inherits writes from.
#[derive(Debug)]
struct Classification<'r> {
    own: Effect,
    /// Each declaration a call runs, with what its own `$this` is to the
    /// body and, when it takes the call's arguments as its own, the call
    /// (`sites`).
    calls: Vec<(DeclKey, Transfer, Option<u32>)>,
    sites: Vec<Site<'r>>,
    /// The parameters whose argument the body runs itself.
    runs: Vec<Condition>,
}

/// The same classification after resolving all declaration edges to IDs,
/// ready for the fixed-point worklist.
#[derive(Debug)]
struct IdClassification<'r> {
    own: Effect,
    calls: Vec<(EffectId, Transfer, Option<u32>)>,
    sites: Vec<IdSite<'r>>,
    runs: Vec<Condition>,
}

#[derive(Debug)]
struct IdSite<'r> {
    call: &'r CallWrites,
    passed: Vec<(&'r ArgAt, IdPassed)>,
}

#[derive(Debug)]
struct IdPassed {
    runs: Vec<(EffectId, Transfer)>,
    params: Vec<Condition>,
}

impl Classification<'_> {
    fn anything() -> Self {
        Classification {
            own: Effect::anything(),
            calls: Vec::new(),
            sites: Vec::new(),
            runs: Vec::new(),
        }
    }
}

/// A call whose arguments a declaration it runs takes as its own.
#[derive(Debug)]
struct Site<'r> {
    call: &'r CallWrites,
    /// What each argument runs when it is called, where the body knows.
    passed: Vec<(&'r ArgAt, Passed)>,
}

/// What an argument runs when it is called: these declarations, each with
/// what its own `$this` is to the caller, and what the caller is passed at
/// these parameters of its own.
#[derive(Debug)]
struct Passed {
    runs: Vec<(DeclKey, Transfer)>,
    params: Vec<Condition>,
}

/// Where a call passes a parameter of a declaration it runs.
enum Passing<'c> {
    /// The argument passed there.
    Argument(&'c ArgAt),
    /// Nothing: the parameter's default runs these, called; none when the
    /// parameter is required, since the call throws.
    Default(Vec<(DeclKey, Transfer)>),
    /// Not known.
    Unknown,
}

/// Whether the declaration `key` a call of `callee` runs takes the call's
/// arguments as its own: `__call` and `__callStatic` take another method's
/// as an array.
fn passes_arguments(callee: &Callee, key: &DeclKey) -> bool {
    if matches!(callee, Callee::Implicit { .. } | Callee::ParentHook { .. }) {
        return false;
    }
    if key.member != "__call" && key.member != "__callstatic" {
        return true;
    }
    match callee {
        Callee::Method {
            method: Some(method),
            ..
        }
        | Callee::Static {
            method: Some(method),
            ..
        } => method.eq_ignore_ascii_case(&key.member),
        _ => false,
    }
}

/// The function `text` names, called: none when it names no function, since
/// the call throws.
fn function_named(registry: &Registry, text: &str) -> Vec<(DeclKey, Transfer)> {
    match registry.lookup_function_with_name(text.trim_start_matches('\\')) {
        Some((declared, _)) => vec![(function_key(declared), Transfer::Static)],
        None => Vec::new(),
    }
}

/// The magic method PHP calls for `access` to a property it runs one for.
fn property_magic_method(access: Access) -> &'static str {
    match access {
        Access::Read => "__get",
        Access::Write => "__set",
        Access::Isset => "__isset",
        Access::Unset => "__unset",
    }
}

/// The hooks PHP may run for `operation`: those of the property it names
/// (any property when `None`), `set` hooks when `true`, and whether it
/// iterates the object, which reads its properties only when it is not
/// `Traversable`. An unset runs none: a hooked property cannot be unset.
fn hook_access(operation: &Operation) -> Option<(Option<&Name>, bool, bool)> {
    match operation {
        Operation::Property(_, Access::Unset) => None,
        Operation::Property(prop, access) => Some((prop.as_ref(), *access == Access::Write, false)),
        Operation::Iterate { .. } => Some((None, false, true)),
        Operation::Clone { with: true } => Some((None, true, false)),
        Operation::Clone { with: false } | Operation::Element(_) | Operation::String => None,
    }
}

/// The method PHP calls on an `ArrayAccess` for `access` to an element.
fn element_method(access: Access) -> &'static str {
    match access {
        Access::Read => "offsetGet",
        Access::Write => "offsetSet",
        Access::Isset => "offsetExists",
        Access::Unset => "offsetUnset",
    }
}

/// Every method PHP may call for `operation` on an object of some class.
fn implicit_method_names(operation: &Operation) -> Vec<&'static str> {
    match operation {
        Operation::Property(_, access) => vec![property_magic_method(*access)],
        Operation::Element(access) => vec![element_method(*access)],
        Operation::String => vec!["__toString"],
        Operation::Clone { with: false } => vec!["__clone"],
        Operation::Clone { with: true } => vec!["__clone", "__set"],
        Operation::Iterate { keys } => {
            let mut methods = vec!["getIterator", "rewind", "valid", "current", "next"];
            if *keys {
                methods.push("key");
            }
            methods
        }
    }
}

/// Adds `key`, run with its own `$this` what `transfer` says, to `out`
/// once.
fn push_target(out: &mut Vec<(DeclKey, Transfer)>, key: DeclKey, transfer: Transfer) {
    if !out.iter().any(|(k, t)| *k == key && *t == transfer) {
        out.push((key, transfer));
    }
}

fn classifications_by_id<'r>(
    registry: &Registry,
    classified: FxHashMap<DeclKey, Classification<'r>>,
) -> FxHashMap<EffectId, IdClassification<'r>> {
    classified
        .into_iter()
        .map(|(key, classification)| {
            let calls = classification
                .calls
                .into_iter()
                .map(|(key, transfer, site)| (registry.effect_id_for(&key), transfer, site))
                .collect();
            let sites = classification
                .sites
                .into_iter()
                .map(|site| IdSite {
                    call: site.call,
                    passed: site
                        .passed
                        .into_iter()
                        .map(|(at, passed)| {
                            (
                                at,
                                IdPassed {
                                    runs: passed
                                        .runs
                                        .into_iter()
                                        .map(|(key, transfer)| {
                                            (registry.effect_id_for(&key), transfer)
                                        })
                                        .collect(),
                                    params: passed.params,
                                },
                            )
                        })
                        .collect(),
                })
                .collect();
            (
                registry.effect_id_for(&key),
                IdClassification {
                    own: classification.own,
                    calls,
                    sites,
                    runs: classification.runs,
                },
            )
        })
        .collect()
}

impl Registry {
    pub(crate) fn effect_id_for(&self, key: &DeclKey) -> EffectId {
        match key.class.as_deref() {
            None => self
                .function_ids
                .get(key.member.as_str())
                .copied()
                .map_or_else(|| EffectId::Named(key.clone()), EffectId::Function),
            Some(_) if key.member.starts_with('$') => EffectId::Named(key.clone()),
            Some(class) => {
                let method = self.class_ids.get(class).and_then(|id| {
                    self.class_by_id(*id)
                        .and_then(|info| info.member_slots.get(key.member.as_str()))
                        .map(|slot| crate::registry::MethodRef::new(*id, *slot))
                });
                method.map_or_else(|| EffectId::Named(key.clone()), EffectId::Method)
            }
        }
    }

    fn effect_decl_key(&self, id: &EffectId) -> Option<DeclKey> {
        match id {
            EffectId::Function(id) => self
                .function_name_by_id(*id)
                .map(|name| function_key(name.as_str())),
            EffectId::Method(method) => {
                let class = self.class_name_by_id(method.class())?;
                let info = self.class_by_id(method.class())?;
                let member = info.member_order.get(usize::from(method.slot()))?;
                Some(method_key(class.as_str(), member.name.as_str()))
            }
            EffectId::Named(key) => Some(key.clone()),
        }
    }

    pub(crate) fn effects_for_names(&self) -> FxHashMap<DeclKey, Settled> {
        self.effects
            .iter()
            .filter_map(|(id, settled)| self.effect_decl_key(id).map(|key| (key, settled.clone())))
            .collect()
    }

    pub(crate) fn rebuild_effect_ids(&mut self, effects: FxHashMap<DeclKey, Settled>) {
        self.effects = effects
            .into_iter()
            .map(|(key, settled)| (self.effect_id_for(&key), settled))
            .collect();
    }

    /// Settles what every function and method with a body may write. Run with
    /// `resolve_transitive_purity`, after every class is indexed and
    /// flattened. Every declaration is resolved again on each run: what a
    /// call reaches depends on the whole hierarchy, which any file may
    /// change.
    pub(crate) fn resolve_effects(&mut self, bodies: &dyn BodySource) {
        self.methods_by_name = Arc::new(methods_by_name(self));
        let mut unset = crate::registry::bodies::UnsetProperties::default();
        bodies.unset_properties(&mut unset);
        unset.settle();
        self.unset_properties = Arc::new(unset);
        let mut running_code = (*self.base_builtin_methods_running_code).clone();
        for name in self.own_builtin_methods_running_code() {
            running_code.insert(name);
        }
        self.builtin_methods_running_code = running_code;
        let registry: &Registry = self;
        let functions: Vec<(&Name, &FunctionReflection)> = registry
            .functions()
            .own_iter()
            .filter(|(_, sig)| !sig.is_stub)
            .collect();
        let methods: Vec<(&Name, &Name, &FunctionReflection)> = registry
            .classes()
            .own_iter()
            .flat_map(|(class, info)| {
                info.methods
                    .iter()
                    .filter(move |(method, sig)| !never_runs(info, method, sig))
                    .map(move |(method, sig)| (class, method, sig))
            })
            .collect();
        let mut classified: FxHashMap<DeclKey, Classification<'_>> = functions
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .flat_map_iter(|(name, sig)| {
                registry.classify(bodies, None, name, sig, function_key(name))
            })
            .collect();
        classified.par_extend(
            methods
                .par_iter()
                .with_min_len(crate::registry::PASS_CHUNK)
                .flat_map_iter(|(class, method, sig)| {
                    let key = method_key(class, method);
                    registry.classify(bodies, Some(class), method, sig, key)
                }),
        );
        let hooks: Vec<(&Name, &Name, &PropertyReflection, bool)> = registry
            .classes()
            .own_iter()
            .flat_map(|(class, _)| {
                registry.property_names(class).flat_map(move |property| {
                    registry
                        .lookup_property(class, property)
                        .into_iter()
                        .flat_map(move |declared| {
                            [
                                (false, declared.get_hook_body),
                                (true, declared.set_hook_body),
                            ]
                            .into_iter()
                            .filter(|(_, body)| *body)
                            .map(move |(set, _)| (class, property, declared, set))
                        })
                })
            })
            .collect();
        classified.par_extend(
            hooks
                .par_iter()
                .with_min_len(crate::registry::PASS_CHUNK)
                .flat_map_iter(|(class, property, declared, set)| {
                    registry.classify_hook(bodies, class, property, declared, *set)
                }),
        );
        let classified = classifications_by_id(registry, classified);
        let mut effects = settle(registry, &classified, |id| {
            registry.effect_decl_key(id).map_or_else(
                || Settled::of(Effect::anything()),
                |key| Settled::of(registry.declaration_effect(&key)),
            )
        });
        // A closure's effect is taken where it runs; no call names it. Keep
        // its result through settlement, then omit it from the registry.
        effects.retain(|id, _| !matches!(id, EffectId::Named(key) if key.is_closure()));
        self.effects = effects;
    }

    /// What a declaration with no body at hand may write: a stub's by its
    /// parameters, an abstract method or an interface's nothing (it never
    /// runs), anything else's anything.
    fn declaration_effect(&self, key: &DeclKey) -> Effect {
        match &key.class {
            Some(class) => {
                let Some(info) = self.classes.get(class.as_str()) else {
                    return Effect::anything();
                };
                let Some(sig) = info.methods.get(key.member.as_str()) else {
                    return Effect::anything();
                };
                if sig.is_stub {
                    self.stub_effect(key, sig)
                } else if never_runs(info, &key.member, sig) {
                    Effect::nothing()
                } else {
                    Effect::anything()
                }
            }
            None => match self.functions.get(key.member.as_str()) {
                Some(sig) if sig.is_stub => self.stub_effect(key, sig),
                _ => Effect::anything(),
            },
        }
    }

    /// What a call to the declaration `key` may write, as settled.
    fn settled(&self, key: &DeclKey) -> Settled {
        match self.effects.get(&self.effect_id_for(key)) {
            Some(settled) => settled.clone(),
            None => Settled::of(self.declaration_effect(key)),
        }
    }

    /// What a call to the declaration `key` whose arguments are not known
    /// may write, as settled.
    fn settled_effect(&self, key: &DeclKey) -> Effect {
        match self.effects.get(&self.effect_id_for(key)) {
            Some(settled) => settled.unconditional(),
            None => self.declaration_effect(key),
        }
    }

    /// The declaration `key` names, when it is one.
    fn declaration(&self, key: &DeclKey) -> Option<&FunctionReflection> {
        match &key.class {
            Some(class) => self.lookup_method(class, &key.member),
            None => self.functions.get(key.member.as_str()),
        }
    }

    /// Where `call` passes the parameter at `at` of the declaration `key`
    /// it runs, which takes its arguments as its own.
    fn passing<'c>(&self, call: &'c CallWrites, key: &DeclKey, at: u32) -> Passing<'c> {
        let Some(param) = self
            .declaration(key)
            .and_then(|sig| sig.params.get(at as usize))
        else {
            return Passing::Unknown;
        };
        if param.variadic {
            return Passing::Unknown;
        }
        let passed = call.arguments.iter().find(|arg| match arg {
            ArgAt::Position(position) => *position == at,
            ArgAt::Named(name) => name.as_str() == param.name.as_ref(),
            ArgAt::Spread => false,
        });
        if let Some(arg) = passed {
            return Passing::Argument(arg);
        }
        if call
            .arguments
            .iter()
            .any(|arg| matches!(arg, ArgAt::Spread))
        {
            return Passing::Unknown;
        }
        if !param.has_default {
            return Passing::Default(Vec::new());
        }
        let Some(default) = &param.default_ty else {
            return Passing::Unknown;
        };
        if default.is_null().yes() {
            return Passing::Default(Vec::new());
        }
        match default
            .string_literal_value()
            .and_then(|text| text.as_str())
        {
            Some(text) if !text.contains("::") => Passing::Default(function_named(self, text)),
            _ => Passing::Unknown,
        }
    }

    /// What calling `method` on an object that may be of `class` may write:
    /// every implementation it may reach, `class`'s own and each
    /// descendant's, or `__call`'s where there is none.
    pub(crate) fn virtual_method_effect(&self, class: &str, method: &str) -> Effect {
        let mut effect = Effect::nothing();
        for owner in self.implementations(class, method) {
            effect.join(&self.settled_effect(&owner));
            if effect.is_anything() {
                break;
            }
        }
        effect
    }

    /// The declarations a call of `method` on an object that may be of
    /// `class` may run: `class`'s and each descendant's, `__call` where a
    /// class has none, and `class`'s alone when no override can reach it.
    fn implementations(&self, class: &str, method: &str) -> Vec<DeclKey> {
        if let Some((owner, sig)) = self.lookup_method_with_owner(class, method)
            && is_non_overridable(self, class, &owner, method, sig)
        {
            return vec![self.reached(&owner, method, sig)];
        }
        let mut out: Vec<DeclKey> = Vec::new();
        let mut seen: FxHashSet<DeclKey> = FxHashSet::default();
        for candidate in std::iter::once(Name::from(class)).chain(self.subclasses(class)) {
            let key = match self.lookup_method_with_owner(&candidate, method) {
                Some((owner, sig)) => self.reached(&owner, method, sig),
                None => match self.lookup_method_with_owner(&candidate, "__call") {
                    Some((owner, _)) => method_key(&owner, "__call"),
                    None => continue,
                },
            };
            if seen.insert(key.clone()) {
                out.push(key);
            }
        }
        out
    }

    /// The declaration a call to `owner`'s `method` runs: `__call` for a
    /// method only a `@method` tag declares.
    fn reached(&self, owner: &str, method: &str, sig: &FunctionReflection) -> DeclKey {
        if sig.is_magic
            && let Some((call_owner, _)) = self.lookup_method_with_owner(owner, "__call")
        {
            return method_key(&call_owner, "__call");
        }
        method_key(owner, method)
    }

    /// Whether an access to the property `prop` (a computed one when `None`)
    /// of an object of `class`, from code of the class `scope`, runs a
    /// magic method: no property of the name is declared, or only a
    /// `@property` tag, a static one or one the scope cannot see declares
    /// it, or the program unsets a property of the name, which leaves the
    /// declared one to the magic methods. A closure (`rebindable`) may be
    /// bound to any class's scope, so it sees only a public one for
    /// certain.
    fn property_magic(
        &self,
        class: &str,
        prop: Option<&Name>,
        scope: Option<&str>,
        rebindable: bool,
    ) -> bool {
        let Some(prop) = prop else {
            return true;
        };
        let Some((owner, declared)) = self.lookup_property_with_owner(class, prop) else {
            return true;
        };
        if declared.from_tag() || declared.is_static() || self.unset_properties.may_unset(prop) {
            return true;
        }
        match declared.visibility() {
            Visibility::Public => false,
            _ if rebindable => true,
            visibility => !sees(self, &owner, visibility, scope),
        }
    }

    /// The class whose hook runs for an access to `property` of an object
    /// of `class`, its `get` hook, or its `set` hook when `set`: the nearest
    /// declaration of the property, `class`'s own or an ancestor's, that
    /// gives it a hook with a body, since a redeclaration that gives none
    /// keeps its parent's; `None` when no hook with a body runs.
    fn hook_owner(&self, class: &str, property: &str, set: bool) -> Option<Name> {
        let (mut owner, mut declared) = self.lookup_property_with_owner(class, property)?;
        let mut seen: Vec<Name> = Vec::new();
        loop {
            if declared.from_tag() || declared.is_static() {
                return None;
            }
            let (body, hooked) = if set {
                (declared.set_hook_body, declared.has_set_hook)
            } else {
                (declared.get_hook_body, declared.has_get_hook)
            };
            if body {
                return Some(owner);
            }
            if !hooked || seen.iter().any(|class| class.eq_ignore_case(&owner)) {
                return None;
            }
            let parent = self.classes.get(owner.as_str())?.extends().first()?.clone();
            seen.push(owner);
            (owner, declared) = self.lookup_property_with_owner(&parent, property)?;
            if declared.visibility() == Visibility::Private {
                return None;
            }
        }
    }

    /// The name the builtin table knows the builtin `key` by: its own, or,
    /// for a method, its name on the first of its class's ancestors PHP
    /// declares it on; `None` when no call of it was reached.
    fn builtin_entry(&self, key: &DeclKey) -> Option<String> {
        let name = key.builtin_name();
        if builtin_runs::probed(&name) {
            return Some(name);
        }
        let class = key.class.as_deref()?;
        self.all_ancestor_names(class).iter().find_map(|ancestor| {
            let name = format!("{}::{}", ancestor.to_ascii_lowercase(), key.member);
            builtin_runs::probed(&name).then_some(name)
        })
    }

    /// The builtin classes `class` is: itself and its ancestors, lowercased.
    fn builtin_lineage(&self, class: &str) -> Vec<String> {
        std::iter::once(class.to_ascii_lowercase())
            .chain(
                self.all_ancestor_names(class)
                    .iter()
                    .map(|ancestor| ancestor.to_ascii_lowercase()),
            )
            .collect()
    }

    /// Whether an object of the builtin class `class` may run a callable or
    /// an object's method it was made with.
    fn builtin_holds_code(&self, class: &str) -> bool {
        self.builtin_lineage(class)
            .iter()
            .any(|class| builtin_runs::holding(class))
    }

    /// The stream-wrapper methods an object of the builtin class `class` may
    /// run through a URL it was made with.
    fn builtin_held_wrappers(&self, class: &str) -> Vec<&'static str> {
        let mut out = Vec::new();
        for class in self.builtin_lineage(class) {
            for method in builtin_runs::held_wrappers(&class) {
                if !out.contains(method) {
                    out.push(*method);
                }
            }
        }
        out
    }

    /// What a call of the stub `key` (`sig`) whose arguments are not known
    /// may write: nothing when a call of it was seen to run no code through
    /// any argument, its object holds none, and none of its parameters
    /// takes something through which it runs code
    /// (`CODE_RUNNING_PARAMETER_TYPES`); for a stub no call of which was
    /// seen, nothing when its doc declares it pure and it takes no code;
    /// anything otherwise.
    fn stub_effect(&self, key: &DeclKey, sig: &FunctionReflection) -> Effect {
        let runs_code = *sig.stub_effect.0.get_or_init(|| {
            let takes_code = sig
                .params
                .iter()
                .any(|p| p.ty.mentions_any(&CODE_RUNNING_PARAMETER_TYPES));
            let Some(name) = self.builtin_entry(key) else {
                // A stub no run of PHP has seen is taken at its word: one
                // its doc declares pure writes nothing, unless it takes
                // code to run.
                return !sig.declared_pure || takes_code;
            };
            key.class.as_deref().is_some_and(|class| {
                self.builtin_holds_code(class) || !self.builtin_held_wrappers(class).is_empty()
            }) || !builtin_runs::runs(&name).is_empty()
                || takes_code
        });
        if runs_code {
            Effect::anything()
        } else {
            Effect::nothing()
        }
    }

    /// The builtin `key` is, when it is one.
    fn builtin(&self, key: &DeclKey) -> Option<&FunctionReflection> {
        self.declaration(key).filter(|sig| sig.is_stub)
    }

    /// The methods with a body overriding one of the builtin class
    /// `builtin`'s that an object of `receiver` or of a class below it may
    /// run: what a method of the builtin may run on its own object. Each of
    /// the builtin's methods is taken as `receiver` resolves it, which may
    /// be an ancestor's, and from each class below `receiver` declaring it
    /// with a body.
    fn overrides_of_builtin(&self, receiver: &str, builtin: &str) -> Vec<DeclKey> {
        let Some(info) = self.classes.get(builtin) else {
            return Vec::new();
        };
        let below = self.subclasses(receiver);
        let below_lower: FxHashSet<&str> = below.iter().map(Name::lower).collect();
        let mut out = Vec::new();
        for method in info.methods.keys() {
            if let Some((owner, sig)) = self.lookup_method_with_owner(receiver, method)
                && !sig.is_stub
            {
                out.push(method_key(&owner, method));
            }
            let declaring: Vec<&Name> = self.classes_declaring(method).collect();
            if below.len() < declaring.len() {
                out.extend(
                    below
                        .iter()
                        .filter(|class| {
                            self.classes.get(class).is_some_and(|info| {
                                info.methods.get(method).is_some_and(|sig| !sig.is_stub)
                            })
                        })
                        .map(|class| method_key(class, method)),
                );
            } else {
                out.extend(
                    declaring
                        .iter()
                        .filter(|class| below_lower.contains(class.lower()))
                        .map(|class| method_key(class, method)),
                );
            }
        }
        out
    }

    /// The loaded classes declaring `method` with a body.
    fn classes_declaring(&self, method: &str) -> impl Iterator<Item = &Name> {
        self.methods_by_name
            .get(&method.to_ascii_lowercase())
            .into_iter()
            .flatten()
            .filter_map(|id| self.class_name_by_id(*id))
    }

    /// Every loaded declaration of `method` with a body, and every `__call`
    /// and `__callStatic`: what a call on an object or a class not known may
    /// run. `None` when a builtin method of that name runs code passed to
    /// it.
    fn methods_named(&self, method: &str) -> Option<Vec<DeclKey>> {
        if self
            .builtin_methods_running_code()
            .contains(&method.to_ascii_lowercase())
        {
            return None;
        }
        let mut out: Vec<DeclKey> = Vec::new();
        let mut seen: FxHashSet<DeclKey> = FxHashSet::default();
        let declaring = self.classes_declaring(method).map(|class| (class, method));
        let magic = ["__call", "__callStatic"].into_iter().flat_map(|magic| {
            self.classes_declaring(magic)
                .map(move |class| (class, magic))
        });
        for (class, method) in declaring.chain(magic) {
            if let Some((owner, sig)) = self.lookup_method_with_owner(class, method) {
                let key = self.reached(&owner, method, sig);
                if seen.insert(key.clone()) {
                    out.push(key);
                }
            }
        }
        Some(out)
    }

    /// What calling `method` on an object of a class not known may write.
    pub(crate) fn method_effect_by_name(&self, method: &str) -> Effect {
        let Some(keys) = self.methods_named(method) else {
            return Effect::anything();
        };
        let mut effect = Effect::nothing();
        for key in keys {
            effect.join(&self.settled_effect(&key));
            if effect.is_anything() {
                break;
            }
        }
        effect
    }

    /// The names (lowercased) of the builtin methods that run code passed to
    /// them, found once: the stubs do not change.
    fn builtin_methods_running_code(&self) -> &NameSet {
        &self.builtin_methods_running_code
    }

    /// The names, lowercased, of the builtin methods of the classes the
    /// registry holds itself that run code passed to them: a stub method
    /// whose effect is anything.
    pub(crate) fn own_builtin_methods_running_code(&self) -> FxHashSet<String> {
        let mut names = FxHashSet::default();
        for (class, info) in self.classes.own_iter() {
            for (method, sig) in info.methods.iter() {
                if !sig.is_stub {
                    continue;
                }
                let Some((owner, _)) = self.lookup_method_with_owner(class, method) else {
                    continue;
                };
                if self
                    .stub_effect(&method_key(&owner, method), sig)
                    .is_anything()
                {
                    names.insert(method.to_ascii_lowercase());
                }
            }
        }
        names
    }

    /// The classification of the body of `sig`, the declaration `member` of
    /// `class` (a function when `class` is `None`), under `key`, and of each
    /// closure it wrote, under its own.
    fn classify<'r>(
        &'r self,
        bodies: &'r dyn BodySource,
        class: Option<&'r Name>,
        member: &'r Name,
        sig: &'r FunctionReflection,
        key: DeclKey,
    ) -> Vec<(DeclKey, Classification<'r>)> {
        let Some(summary) = bodies.summary(&sig.provenance) else {
            return vec![(key, Classification::anything())];
        };
        let resolve = Resolve {
            registry: self,
            class: class.map(Name::as_str),
            member: member.clone(),
            summary,
            value: None,
        };
        resolve.run(&key, Some(sig))
    }

    /// The classification of the body of `class`'s `get` hook of
    /// `property`, declared as `declared`, or of its `set` hook when `set`,
    /// and of each closure it wrote, under its own.
    fn classify_hook<'r>(
        &'r self,
        bodies: &'r dyn BodySource,
        class: &'r Name,
        property: &'r Name,
        declared: &'r PropertyReflection,
        set: bool,
    ) -> Vec<(DeclKey, Classification<'r>)> {
        let key = hook_key(class, property, set);
        let Some((summary, value)) = bodies.hook_summary(declared.provenance(), set) else {
            return vec![(key, Classification::anything())];
        };
        let ty = declared.set_ty.as_ref().unwrap_or(&declared.ty);
        let resolve = Resolve {
            registry: self,
            class: Some(class.as_str()),
            member: Name::from(format!("${property}::{}", if set { "set" } else { "get" })),
            summary,
            value: value.map(|value| (value.as_str(), ty)),
        };
        resolve.run(&key, None)
    }
}

/// What each declaration `classified` holds may write, settled from what it
/// writes itself: it grows by what each declaration it calls may write, by
/// `outside` for one `classified` does not hold, and, where a callee runs
/// what a call passes it, by what that may write.
fn settle(
    registry: &Registry,
    classified: &FxHashMap<EffectId, IdClassification<'_>>,
    outside: impl Fn(&EffectId) -> Settled,
) -> FxHashMap<EffectId, Settled> {
    let mut settled: FxHashMap<EffectId, Settled> = classified
        .iter()
        .map(|(key, classification)| {
            let runs = classification.runs.iter().cloned().collect();
            let own = Settled {
                effect: classification.own.clone(),
                runs,
            };
            (key.clone(), own)
        })
        .collect();
    // A declaration's effect only ever grows, and grows only when one it is
    // made of does: every declaration is joined once, and again only when a
    // declaration it calls, or one an argument it passes runs, has grown.
    let mut callers: FxHashMap<&EffectId, Vec<&EffectId>> = FxHashMap::default();
    for (key, classification) in classified {
        let caller = key;
        for (callee, _, _) in &classification.calls {
            if classified.contains_key(callee) {
                callers.entry(callee).or_default().push(caller);
            }
        }
    }
    // The declarations an argument runs, which a declaration reads only once
    // a callee runs the argument.
    let mut passed_to: FxHashSet<(&EffectId, &EffectId)> = FxHashSet::default();
    let mut queue: std::collections::VecDeque<&EffectId> = classified.keys().collect();
    let mut queued: FxHashSet<&EffectId> = queue.iter().copied().collect();
    while let Some(key) = queue.pop_front() {
        queued.remove(key);
        let classification = &classified[key];
        let before = &settled[key];
        if before.effect.is_anything() {
            continue;
        }
        let mut now = before.clone();
        let mut read: Vec<EffectId> = Vec::new();
        let get = |callee: &EffectId| match settled.get(callee) {
            Some(found) => found.clone(),
            None => outside(callee),
        };
        for (callee, transfer, site) in &classification.calls {
            let found = get(callee);
            now.effect.join(&found.effect.transferred(transfer));
            for condition in &found.runs {
                if callee.is_closure() {
                    // What a closure the body wrote runs of the body's
                    // parameters is the body's to run.
                    now.runs.insert(condition.through(transfer));
                    continue;
                }
                run_passed(
                    registry,
                    site.map(|site| &classification.sites[site as usize]),
                    callee,
                    transfer,
                    condition,
                    &get,
                    &mut now,
                    &mut read,
                );
                if now.effect.is_anything() {
                    break;
                }
            }
            if now.effect.is_anything() {
                break;
            }
        }
        for found in read {
            if let Some((found, _)) = classified.get_key_value(&found)
                && passed_to.insert((found, key))
            {
                // Keep caller edges borrowed from `classified`'s stable keys.
                callers.entry(found).or_default().push(key);
            }
        }
        if now == settled[key] {
            continue;
        }
        settled.insert(key.clone(), now);
        for caller in callers.get(key).into_iter().flatten() {
            if queued.insert(caller) {
                queue.push_back(caller);
            }
        }
    }
    settled
}

/// Adds to `into` what a callee (`callee`, its own `$this` what `transfer`
/// says) running its argument at the parameter `condition` names may write,
/// as `site`, the call, passes it: `get` gives what a declaration may write,
/// and `read` gets each one read.
#[allow(clippy::too_many_arguments)]
fn run_passed(
    registry: &Registry,
    site: Option<&IdSite<'_>>,
    callee: &EffectId,
    transfer: &Transfer,
    condition: &Condition,
    get: &impl Fn(&EffectId) -> Settled,
    into: &mut Settled,
    read: &mut Vec<EffectId>,
) {
    let Some(site) = site else {
        into.effect = Effect::anything();
        return;
    };
    let Some(callee_key) = registry.effect_decl_key(callee) else {
        into.effect = Effect::anything();
        return;
    };
    let default;
    let (runs, params) = match registry.passing(site.call, &callee_key, condition.at) {
        Passing::Argument(at) => match site.passed.iter().find(|(passed, _)| *passed == at) {
            Some((_, passed)) => (&passed.runs[..], &passed.params[..]),
            None => {
                into.effect = Effect::anything();
                return;
            }
        },
        Passing::Default(runs) => {
            default = runs
                .into_iter()
                .map(|(key, transfer)| (registry.effect_id_for(&key), transfer))
                .collect::<Vec<_>>();
            (&default[..], &[][..])
        }
        Passing::Unknown => {
            into.effect = Effect::anything();
            return;
        }
    };
    for (run, own) in runs {
        let bound = condition.bound(transfer, own);
        let found = get(run);
        into.effect.join(&found.effect.transferred(&bound));
        if run.is_closure() {
            for inner in &found.runs {
                into.runs.insert(inner.through(&bound));
            }
        } else if !found.runs.is_empty() {
            // It runs its own parameters' arguments, which the callee passes.
            into.effect = Effect::anything();
        }
        read.push(run.clone());
        if into.effect.is_anything() {
            return;
        }
    }
    for param in params {
        into.runs.insert(Condition {
            at: param.at,
            this: condition
                .this
                .as_ref()
                .map(|this| this.through(transfer))
                .or_else(|| param.this.clone()),
        });
    }
}

/// The classes `ty` may be an object of: `Some` with each one named, `None`
/// when it may be an object of a class not named (`object`, `mixed`).
pub(crate) fn object_classes(ty: &Type) -> Option<Vec<Name>> {
    let mut classes = Vec::new();
    for member in ty.union_members() {
        if let Some((class, _)) = member.as_object() {
            classes.push(Name::from(class));
        } else if !member.is_object().no() {
            return None;
        }
    }
    Some(classes)
}

/// The object a value may be.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Holder {
    /// An object of one of the classes, or of a descendant of one.
    Classes(Vec<Name>),
    /// An object the caller made, no one else has: of exactly one of the
    /// classes, or of a class not known.
    Made(Option<Vec<Name>>),
    /// An object of any class.
    Unknown,
}

impl Holder {
    /// The object a value either holder describes may be.
    fn join(self, other: Holder) -> Holder {
        match (self, other) {
            (Holder::Classes(none), other) | (other, Holder::Classes(none)) if none.is_empty() => {
                other
            }
            (Holder::Unknown | Holder::Made(None), _)
            | (_, Holder::Unknown | Holder::Made(None)) => Holder::Unknown,
            (Holder::Made(Some(mut a)), Holder::Made(Some(b))) => {
                a.extend(b);
                Holder::Made(Some(sorted_classes(a)))
            }
            (
                Holder::Classes(mut a) | Holder::Made(Some(mut a)),
                Holder::Classes(b) | Holder::Made(Some(b)),
            ) => {
                a.extend(b);
                Holder::Classes(sorted_classes(a))
            }
        }
    }
}

/// `classes` in one order, each once, as PHP compares class names.
fn sorted_classes(mut classes: Vec<Name>) -> Vec<Name> {
    classes.sort_unstable_by(|a, b| a.lower().cmp(b.lower()));
    classes.dedup_by(|a, b| a.eq_ignore_case(b));
    classes
}

/// What the `$this` of code PHP runs for `operation` on an object `holder`
/// says may be, of `class` when one, is to the caller, whose own `$this`
/// the object is when `same`.
fn implicit_transfer(
    holder: &Holder,
    same: bool,
    operation: &Operation,
    class: Option<&Name>,
) -> Transfer {
    if matches!(operation, Operation::Clone { .. }) {
        // `__clone` and the writes `clone` with properties makes run on the
        // copy.
        Transfer::Made
    } else if same {
        Transfer::Same
    } else if matches!(holder, Holder::Made(_)) {
        Transfer::Made
    } else {
        Transfer::Other(class.cloned())
    }
}

/// The holder of a value of type `ty`.
pub(crate) fn holder_of(ty: &Type) -> Holder {
    match object_classes(ty) {
        Some(classes) => Holder::Classes(classes),
        None => Holder::Unknown,
    }
}

/// The holder of a value of the type `ty` declared on a member of `owner`,
/// reached through an object of `class`.
fn declared_holder(registry: &Registry, ty: &Type, class: &str, owner: &str) -> Holder {
    let mut ty = ty.bind_static(class).bind_self(owner);
    if let Some(parent) = registry
        .classes()
        .get(owner)
        .and_then(|info| info.extends().first())
    {
        ty = ty.bind_parent(parent);
    }
    holder_of(&ty)
}

/// The variable `place` is, with no step from it.
fn place_variable(place: &Place) -> Option<&Name> {
    match (&place.root, &*place.steps) {
        (Root::Variable(Some(name)), []) => Some(name),
        _ => None,
    }
}

/// What the code making a call knows of it.
pub(crate) trait CallContext {
    /// The class the calling code belongs to.
    fn class(&self) -> Option<&str>;
    /// The object a method is called on, and whether it is the caller's own
    /// `$this`.
    fn receiver(&self, receiver: &Receiver) -> (Holder, bool);
    /// The object the argument passed `at` is; `place` is what it names,
    /// when it names one.
    fn argument(&self, at: &ArgAt, place: Option<&Place>) -> Holder;
    /// Notes that the call runs what `written` names, its `$this` what
    /// `transfer` says, when the caller knows what that is: only closures,
    /// arrow functions or first-class callables it wrote, or, in a
    /// declaration's body, the argument of a parameter it never writes;
    /// `false` when it may be other code.
    fn runs_written(&self, written: Written<'_>, transfer: &Transfer) -> bool;
    /// The object an operation's operand, a value from `on` (from anywhere
    /// when `None`), is, and whether it is the caller's own `$this`.
    fn operand(&self, on: Option<&[Origin]>) -> (Holder, bool);
    /// Whether the calling code is a closure's, which may be bound to any
    /// class's scope.
    fn rebindable(&self) -> bool;
}

/// What a call may run that its caller may have written.
pub(crate) enum Written<'a> {
    /// The argument passed `at`: the callable it names, when it names one,
    /// and the place it is, when it is one.
    Argument {
        at: &'a ArgAt,
        callable: Option<&'a Callable>,
        place: Option<&'a Place>,
    },
    /// What the variable holds.
    Variable(&'a Name),
    /// The call's callee, which the caller's summary lists at these
    /// indices.
    Callee(&'a [u32]),
}

/// The classes `class` as written names from code of the class `own`: `own`
/// for `self`, its parent for `parent`, `own` and every descendant for
/// `static`, the named one otherwise. `None` when `own` is not known, or has
/// no parent; empty for a name that is no class.
fn named_classes(
    registry: &Registry,
    own: Option<&str>,
    class: &QualifiedName,
) -> Option<Vec<Name>> {
    Some(if class.is_keyword("self") {
        vec![Name::from(own?)]
    } else if class.is_keyword("parent") {
        vec![registry.classes().get(own?)?.extends().first()?.clone()]
    } else if class.is_keyword("static") {
        let own = own?;
        std::iter::once(Name::from(own))
            .chain(registry.subclasses(own))
            .collect()
    } else {
        class.resolved_class().map(Name::from).into_iter().collect()
    })
}

/// Finds the declarations a call runs, with what its caller knows of it.
struct Reach<'r> {
    registry: &'r Registry,
    ctx: &'r dyn CallContext,
}

impl Reach<'_> {
    /// The declarations the argument of `call` passed `at` runs when it is
    /// called, each with what its own `$this` is to the caller, `transfer`
    /// for a closure the caller wrote; `None` when it may run code not
    /// known. What the caller wrote is noted (`CallContext::runs_written`).
    fn argument_runs(
        &self,
        call: &CallWrites,
        at: &ArgAt,
        transfer: &Transfer,
    ) -> Option<Vec<(DeclKey, Transfer)>> {
        // A spread argument is an array of arguments.
        if matches!(at, ArgAt::Spread) {
            return None;
        }
        let callable = call
            .callables
            .iter()
            .find(|(passed, _)| passed == at)
            .map(|(_, callable)| callable);
        let place = call
            .places
            .iter()
            .find(|(passed, _)| passed == at)
            .map(|(_, place)| place);
        match callable {
            Some(Callable::Written(_)) | None => {
                let written = Written::Argument {
                    at,
                    callable,
                    place,
                };
                self.ctx.runs_written(written, transfer).then(Vec::new)
            }
            Some(callable) => self.callable_targets(callable),
        }
    }

    /// The declarations a call of the builtin `key` (`sig`) runs, with the
    /// call's arguments: what the callables passed to it run, the methods it
    /// calls on the objects passed to it, the stream-wrapper methods it runs,
    /// and, for a method, the methods of its object a class extending it
    /// declares. `None` when it may run code not known.
    fn builtin_runs(
        &self,
        key: &DeclKey,
        sig: &FunctionReflection,
        transfer: &Transfer,
        call: &CallWrites,
    ) -> Option<Vec<(DeclKey, Transfer)>> {
        let registry = self.registry;
        let Some(name) = registry.builtin_entry(key) else {
            // A stub no run of PHP has seen runs nothing when it is taken at
            // its word (`Registry::stub_effect`), and anything otherwise.
            return registry.stub_effect(key, sig).is_nothing().then(Vec::new);
        };
        if key
            .class
            .as_deref()
            .is_some_and(|class| registry.builtin_holds_code(class))
        {
            return None;
        }
        let mut out = Vec::new();
        let wrapper_runs = |method: &str, out: &mut Vec<(DeclKey, Transfer)>| {
            for class in registry.classes_declaring(method) {
                if let Some((owner, sig)) = registry.lookup_method_with_owner(class, method) {
                    out.push((registry.reached(&owner, method, sig), Transfer::Made));
                }
            }
        };
        if let Some(class) = &key.class {
            for method in registry.builtin_held_wrappers(class) {
                wrapper_runs(method, &mut out);
            }
        }
        if let Some(owner) = &key.class {
            let receivers: Vec<Name> = match transfer {
                Transfer::Same => self.ctx.class().map(Name::from).into_iter().collect(),
                Transfer::Other(Some(class)) => vec![class.clone()],
                Transfer::Other(None) | Transfer::Made | Transfer::Static => {
                    vec![Name::from(owner.as_str())]
                }
            };
            for receiver in &receivers {
                out.extend(
                    registry
                        .overrides_of_builtin(receiver, owner)
                        .into_iter()
                        .map(|key| (key, transfer.clone())),
                );
            }
        }
        let runs = builtin_runs::runs(&name);
        // Each parameter a callable is passed at runs it.
        for (at, param) in sig.params.iter().enumerate() {
            let at = at as u32;
            let takes_callable = param.ty.mentions_any(&["callable", "Closure"])
                || runs
                    .iter()
                    .any(|(position, run)| position.at == at && matches!(run, BuiltinRun::Calls));
            if !takes_callable {
                continue;
            }
            let passed = |arg: &ArgAt| match arg {
                ArgAt::Position(position) => *position == at || (param.variadic && *position >= at),
                ArgAt::Named(name) => name.as_str() == param.name.as_ref(),
                ArgAt::Spread => true,
            };
            if !call.arguments.iter().any(passed) {
                continue;
            }
            let elements = runs.iter().any(|(position, run)| {
                position.at == at && position.elements && matches!(run, BuiltinRun::Calls)
            });
            if elements {
                return None;
            }
            for arg in call.arguments.iter().filter(|arg| passed(arg)) {
                out.extend(self.argument_runs(call, arg, &Transfer::Same)?);
            }
        }
        for (position, run) in runs {
            let arg_at = |arg: &ArgAt| match arg {
                ArgAt::Position(at) => *at == position.at,
                ArgAt::Named(name) => sig
                    .params
                    .get(position.at as usize)
                    .is_some_and(|p| p.name.as_ref() == name.as_str()),
                ArgAt::Spread => true,
            };
            if !call.arguments.iter().any(arg_at) {
                continue;
            }
            match run {
                BuiltinRun::Calls => {}
                BuiltinRun::Method(method) => {
                    let holder = match call.arguments.iter().find(|arg| arg_at(arg)) {
                        Some(arg) if !position.elements => {
                            let place = call
                                .places
                                .iter()
                                .find(|(at, _)| at == arg)
                                .map(|(_, place)| place);
                            self.ctx.argument(arg, place)
                        }
                        _ => Holder::Unknown,
                    };
                    out.extend(self.method_targets_on(&holder, method)?);
                }
                BuiltinRun::Wrapper(method) => wrapper_runs(method, &mut out),
            }
        }
        Some(out)
    }

    /// The declarations calling `method` on an object `holder` says may be
    /// run, PHP calling it: by the hierarchy over its classes when each has
    /// the method, and every loaded declaration of it otherwise, since the
    /// method may run on an object the first one gave back. Nothing for a
    /// value that is no object. `None` when a builtin method of the name runs
    /// code passed to it.
    fn method_targets_on(&self, holder: &Holder, method: &str) -> Option<Vec<(DeclKey, Transfer)>> {
        let registry = self.registry;
        let has_method = |class: &Name| {
            registry.lookup_method(class, method).is_some()
                || registry.lookup_method(class, "__call").is_some()
        };
        Some(match holder {
            Holder::Classes(classes) if classes.is_empty() => Vec::new(),
            Holder::Classes(classes) if classes.iter().all(has_method) => classes
                .iter()
                .flat_map(|class| {
                    registry
                        .implementations(class, method)
                        .into_iter()
                        .map(|key| (key, Transfer::Other(Some(class.clone()))))
                })
                .collect(),
            Holder::Made(Some(classes)) if classes.iter().all(has_method) => classes
                .iter()
                .filter_map(|class| self.exact_method(class, method))
                .map(|key| (key, Transfer::Made))
                .collect(),
            _ => registry
                .methods_named(method)?
                .into_iter()
                .map(|key| (key, Transfer::Other(None)))
                .collect(),
        })
    }

    /// The declarations a callable the body names runs.
    fn callable_targets(&self, callable: &Callable) -> Option<Vec<(DeclKey, Transfer)>> {
        let registry = self.registry;
        match callable {
            // What the caller wrote is found where it is passed.
            Callable::Written(_) => None,
            Callable::Text(text) => match text.split_once("::") {
                Some((class, method)) => {
                    let class = QualifiedName {
                        written: Name::from(class.trim_start_matches('\\')),
                        resolved: ResolvedName::Exact(Name::from(class.trim_start_matches('\\'))),
                        span: Default::default(),
                    };
                    self.static_targets(&class, Some(&Name::from(method)))
                }
                None => Some(function_named(registry, text)),
            },
            Callable::Pair(PairTarget::Class(class), method) => {
                self.static_targets(class, Some(method))
            }
            Callable::Pair(PairTarget::Object(receiver), method) => self.call_targets(
                &Callee::Method {
                    receiver: receiver.clone(),
                    method: Some(method.clone()),
                },
                None,
            ),
        }
    }

    /// Whether `$name->method(...)` runs only closures the caller wrote,
    /// `$name` holding only those: `__invoke` runs one with its own `$this`,
    /// `call` with the object `call` passes first.
    fn runs_held_closure(&self, name: &Name, method: &str, call: Option<&CallWrites>) -> bool {
        let transfers = if method.eq_ignore_ascii_case("__invoke") {
            vec![Transfer::Same]
        } else if method.eq_ignore_ascii_case("call")
            && let Some(call) = call
        {
            let new_this = call.arguments.iter().find(|at| match at {
                ArgAt::Position(at) => *at == 0,
                ArgAt::Named(name) => name.as_str() == "newThis",
                ArgAt::Spread => true,
            });
            let Some(at) = new_this else {
                return false;
            };
            let place = call
                .places
                .iter()
                .find(|(passed, _)| passed == at)
                .map(|(_, place)| place);
            match self.ctx.argument(at, place) {
                Holder::Classes(classes) => classes
                    .into_iter()
                    .map(|class| Transfer::Other(Some(class)))
                    .collect(),
                Holder::Made(_) => vec![Transfer::Made],
                Holder::Unknown => vec![Transfer::Other(None)],
            }
        } else {
            return false;
        };
        transfers
            .iter()
            .all(|transfer| self.ctx.runs_written(Written::Variable(name), transfer))
    }

    /// The declarations a call may run, each with what its callee's own
    /// `$this` is to the caller; `None` when it may run code not known.
    /// `call` is the call, when the callee is not a callable's.
    fn call_targets(
        &self,
        callee: &Callee,
        call: Option<&CallWrites>,
    ) -> Option<Vec<(DeclKey, Transfer)>> {
        let registry = self.registry;
        Some(match callee {
            Callee::Function(name) => match registry.resolve_function(name) {
                Some((resolved, _)) => {
                    let declared = registry
                        .lookup_function_with_name(resolved)
                        .map_or(resolved, |(declared, _)| declared);
                    vec![(function_key(declared), Transfer::Static)]
                }
                // A call to no function throws.
                None => Vec::new(),
            },
            Callee::Method {
                receiver: Receiver::Variable(name),
                method: Some(method),
            } if self.runs_held_closure(name, method, call) => Vec::new(),
            Callee::Method { receiver, method } => {
                let (holder, same) = self.ctx.receiver(receiver);
                let transfer = |class: Option<&Name>| {
                    if same {
                        Transfer::Same
                    } else if matches!(holder, Holder::Made(_)) {
                        Transfer::Made
                    } else {
                        Transfer::Other(class.cloned())
                    }
                };
                let mut out = Vec::new();
                match (&holder, method) {
                    (Holder::Classes(classes), Some(method)) => {
                        for class in classes {
                            out.extend(
                                registry
                                    .implementations(class, method)
                                    .into_iter()
                                    .map(|key| (key, transfer(Some(class)))),
                            );
                        }
                    }
                    (Holder::Made(Some(classes)), Some(method)) => {
                        for class in classes {
                            out.extend(
                                self.exact_method(class, method)
                                    .map(|key| (key, transfer(Some(class)))),
                            );
                        }
                    }
                    (Holder::Classes(classes) | Holder::Made(Some(classes)), None) => {
                        for class in classes {
                            out.extend(
                                self.every_method(class)
                                    .into_iter()
                                    .map(|key| (key, transfer(Some(class)))),
                            );
                        }
                    }
                    (Holder::Unknown | Holder::Made(None), Some(method)) => {
                        out.extend(
                            self.registry
                                .methods_named(method)?
                                .into_iter()
                                .map(|key| (key, transfer(None))),
                        );
                    }
                    (Holder::Unknown | Holder::Made(None), None) => return None,
                }
                out
            }
            Callee::Static { class, method } => match class {
                Some(class) => self.static_targets(class, method.as_ref())?,
                None => self
                    .registry
                    .methods_named(method.as_ref()?)?
                    .into_iter()
                    .map(|key| (key, Transfer::Static))
                    .collect(),
            },
            Callee::New(class) => {
                let classes = match class {
                    Some(class) => {
                        named_classes(self.registry, self.ctx.class(), class).unwrap_or_default()
                    }
                    None => self
                        .registry
                        .classes_declaring("__construct")
                        .cloned()
                        .collect(),
                };
                classes
                    .iter()
                    .filter_map(|class| registry.lookup_method_with_owner(class, "__construct"))
                    .map(|(owner, _)| (method_key(&owner, "__construct"), Transfer::Made))
                    .collect()
            }
            Callee::Anonymous(name) => name
                .as_ref()
                .and_then(|name| registry.lookup_method_with_owner(name, "__construct"))
                .map(|(owner, _)| (method_key(&owner, "__construct"), Transfer::Made))
                .into_iter()
                .collect(),
            Callee::Value(variable) => {
                let written = Written::Variable(variable.as_ref()?);
                if !self.ctx.runs_written(written, &Transfer::Same) {
                    return None;
                }
                Vec::new()
            }
            Callee::Written(ids) => {
                if !self.ctx.runs_written(Written::Callee(ids), &Transfer::Same) {
                    return None;
                }
                Vec::new()
            }
            Callee::Implicit { on, operation } => {
                let (holder, same) = self.ctx.operand(on.as_deref());
                let mut out = Vec::new();
                if let Some((property, set, iterate)) = hook_access(operation) {
                    self.hook_targets(&holder, same, operation, property, set, iterate, &mut out);
                }
                self.implicit_targets(&holder, same, operation, &mut out, &mut Vec::new())?;
                out
            }
            Callee::ParentHook { property, set } => {
                let parent = registry
                    .classes()
                    .get(self.ctx.class()?)?
                    .extends()
                    .first()?;
                registry
                    .hook_owner(parent, property, *set)
                    .map(|owner| (hook_key(&owner, property, *set), Transfer::Same))
                    .into_iter()
                    .collect()
            }
        })
    }

    /// Adds to `out` the hooks PHP may run for an access to `property` (to
    /// each property when `None`) of an object `holder` says may be, the
    /// caller's own `$this` when `same`, made by `operation`: the `set`
    /// hooks when `set`, and only on an object that is not `Traversable`
    /// when it iterates the object. Each hook with a body runs when the
    /// scope sees its property, and an object of the holder's classes may
    /// have it as the property's hook: one of a class among them or below
    /// them, the hook an object of one of them inherits, or a private
    /// one of the scope's class an object of one of them is.
    #[allow(clippy::too_many_arguments)]
    fn hook_targets(
        &self,
        holder: &Holder,
        same: bool,
        operation: &Operation,
        property: Option<&Name>,
        set: bool,
        iterate: bool,
        out: &mut Vec<(DeclKey, Transfer)>,
    ) {
        let registry = self.registry;
        let transfer = |class: Option<&Name>| implicit_transfer(holder, same, operation, class);
        let (classes, exact) = match holder {
            Holder::Classes(classes) => (Some(classes), false),
            Holder::Made(Some(classes)) => (Some(classes), true),
            Holder::Unknown | Holder::Made(None) => (None, false),
        };
        let member = hook_member(property.map(Name::as_str), set);
        for declaring in registry.classes_declaring(&member) {
            if registry.classes().get(declaring.as_str()).is_none() {
                continue;
            }
            // How an object of each of the holder's classes is one of the
            // declaring class: `Some(true)` when it always is, `Some(false)`
            // when it may be through a class below both.
            let related: Vec<Option<bool>> = classes.map_or_else(Vec::new, |classes| {
                classes
                    .iter()
                    .map(|class| {
                        if iterate && registry.is_subclass_of(class, "Traversable") {
                            None
                        } else if registry.is_subclass_of(class, declaring) {
                            Some(true)
                        } else if !exact
                            && (registry.is_subclass_of(declaring, class)
                                || registry
                                    .subclasses(declaring)
                                    .iter()
                                    .any(|below| registry.is_subclass_of(below, class)))
                        {
                            Some(false)
                        } else {
                            None
                        }
                    })
                    .collect()
            });
            let declarations: Vec<(&Name, &PropertyReflection)> = match property {
                Some(property) => registry
                    .lookup_property(declaring, property)
                    .map(|declared| (property, declared))
                    .into_iter()
                    .collect(),
                None => registry
                    .property_names(declaring)
                    .filter_map(|name| {
                        registry
                            .lookup_property(declaring, name)
                            .map(|declared| (name, declared))
                    })
                    .collect(),
            };
            for (name, declared) in declarations {
                let body = if set {
                    declared.set_hook_body
                } else {
                    declared.get_hook_body
                };
                let visibility = if set {
                    declared.write_visibility()
                } else {
                    declared.visibility()
                };
                if !body
                    || !(self.ctx.rebindable()
                        || sees(registry, declaring, visibility, self.ctx.class()))
                {
                    continue;
                }
                let key = hook_key(declaring, name, set);
                let Some(classes) = classes else {
                    push_target(out, key, transfer(None));
                    continue;
                };
                for (class, related) in classes.iter().zip(&related) {
                    // An object always of the declaring class runs its hook
                    // when that is the nearest its class has, or when the
                    // property is private, the scope's own; one that may be
                    // of a class below both may run it.
                    let reached = match related {
                        Some(true) => {
                            declared.visibility() == Visibility::Private
                                || registry
                                    .hook_owner(class, name, set)
                                    .is_some_and(|owner| owner.eq_ignore_case(declaring))
                        }
                        Some(false) => true,
                        None => false,
                    };
                    if reached {
                        push_target(out, key.clone(), transfer(Some(class)));
                    }
                }
            }
        }
    }

    /// Adds to `out` the declarations PHP runs for `operation` on an object
    /// `holder` says may be, the caller's own `$this` when `same`, each with
    /// what its own `$this` is to the caller: by the hierarchy over the
    /// holder's classes, and every loaded declaration of the methods it may
    /// run when the class is not known. The iterator an `IteratorAggregate`
    /// gives is iterated as its declared type says; `iterators` holds those
    /// already followed. `None` when it may run code not known.
    fn implicit_targets(
        &self,
        holder: &Holder,
        same: bool,
        operation: &Operation,
        out: &mut Vec<(DeclKey, Transfer)>,
        iterators: &mut Vec<Holder>,
    ) -> Option<()> {
        let registry = self.registry;
        let transfer = |class: Option<&Name>| implicit_transfer(holder, same, operation, class);
        let (classes, exact) = match holder {
            Holder::Classes(classes) => (classes, false),
            Holder::Made(Some(classes)) => (classes, true),
            Holder::Unknown | Holder::Made(None) => {
                for method in implicit_method_names(operation) {
                    if self
                        .registry
                        .builtin_methods_running_code()
                        .contains(&method.to_ascii_lowercase())
                    {
                        return None;
                    }
                    for class in registry.classes_declaring(method) {
                        if !self.implicit_methods(class, operation).contains(&method) {
                            continue;
                        }
                        if let Some(key) = self.implicit_method(class, method) {
                            push_target(out, key, transfer(None));
                        }
                    }
                }
                return Some(());
            }
        };
        for class in classes {
            // A property an object of the class has where the scope sees it,
            // an object of a descendant has there too: a redeclaration only
            // widens its visibility, and keeps it an instance property.
            let declared = matches!(operation, Operation::Property(..))
                && self.implicit_methods(class, operation).is_empty();
            let below = if exact || declared {
                Vec::new()
            } else {
                registry.subclasses(class)
            };
            for candidate in std::iter::once(class).chain(below.iter()) {
                for method in self.implicit_methods(candidate, operation) {
                    if let Some(key) = self.implicit_method(candidate, method) {
                        push_target(out, key, transfer(Some(class)));
                    }
                }
                if !matches!(operation, Operation::Iterate { .. })
                    || registry.is_subclass_of(candidate, "Iterator")
                    || !registry.is_subclass_of(candidate, "IteratorAggregate")
                {
                    continue;
                }
                let given = match registry.lookup_method_with_owner(candidate, "getIterator") {
                    Some((owner, sig)) => {
                        declared_holder(registry, sig.return_type(), candidate, &owner)
                    }
                    None => Holder::Unknown,
                };
                if iterators.contains(&given) {
                    continue;
                }
                iterators.push(given.clone());
                // An aggregate giving aggregates this deep is iterated as
                // any object.
                let given = if iterators.len() > 4 {
                    Holder::Unknown
                } else {
                    given
                };
                self.implicit_targets(&given, false, operation, out, iterators)?;
            }
        }
        Some(())
    }

    /// The methods PHP calls for `operation` on an object of exactly
    /// `class`.
    fn implicit_methods(&self, class: &str, operation: &Operation) -> Vec<&'static str> {
        let registry = self.registry;
        match operation {
            Operation::Property(prop, access) => {
                if registry.property_magic(
                    class,
                    prop.as_ref(),
                    self.ctx.class(),
                    self.ctx.rebindable(),
                ) {
                    vec![property_magic_method(*access)]
                } else {
                    Vec::new()
                }
            }
            Operation::Element(access) => {
                if registry.is_subclass_of(class, "ArrayAccess") {
                    vec![element_method(*access)]
                } else {
                    Vec::new()
                }
            }
            Operation::String => vec!["__toString"],
            Operation::Clone { with: false } => vec!["__clone"],
            Operation::Clone { with: true } => vec!["__clone", "__set"],
            Operation::Iterate { keys } => {
                if registry.is_subclass_of(class, "Iterator") {
                    let mut methods = vec!["rewind", "valid", "current", "next"];
                    if *keys {
                        methods.push("key");
                    }
                    methods
                } else if registry.is_subclass_of(class, "IteratorAggregate") {
                    vec!["getIterator"]
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// The declaration PHP runs calling `method` on an object of exactly
    /// `class` itself: none when the class has no body for it, or only a
    /// `@method` tag or an abstract method declares it.
    fn implicit_method(&self, class: &str, method: &str) -> Option<DeclKey> {
        let registry = self.registry;
        let (owner, sig) = registry.lookup_method_with_owner(class, method)?;
        let info = registry.classes().get(owner.as_str())?;
        let abstract_method =
            info.kind() == ClassKind::Interface || info.is_abstract_method(method);
        (!sig.is_magic && !abstract_method).then(|| method_key(&owner, method))
    }

    /// The declaration a call of `method` runs on an object of exactly
    /// `class`: its own, or `__call` when it has none.
    fn exact_method(&self, class: &str, method: &str) -> Option<DeclKey> {
        let registry = self.registry;
        match registry.lookup_method_with_owner(class, method) {
            Some((owner, sig)) => Some(registry.reached(&owner, method, sig)),
            None => registry
                .lookup_method_with_owner(class, "__call")
                .map(|(owner, _)| method_key(&owner, "__call")),
        }
    }

    /// Every method an object of `class`, or of a descendant, has: what a
    /// call with a computed name may run.
    fn every_method(&self, class: &str) -> Vec<DeclKey> {
        let registry = self.registry;
        let mut out = Vec::new();
        let mut seen: FxHashSet<DeclKey> = FxHashSet::default();
        for candidate in std::iter::once(Name::from(class)).chain(registry.subclasses(class)) {
            let Some(owners) = registry.method_owners(candidate.as_str()) else {
                continue;
            };
            for (method, owner) in owners.iter() {
                let key = method_key(owner, method);
                if seen.insert(key.clone()) {
                    out.push(key);
                }
            }
        }
        out
    }

    /// The declarations `class::method()` runs: `self::` and `parent::`
    /// lexically, `static::` over the class and its descendants, a named
    /// class's own, and `__callStatic` or `__call` where there is none. A
    /// non-static method reached from the body's own class or an ancestor
    /// of it runs on the caller's `$this`.
    fn static_targets(
        &self,
        class: &QualifiedName,
        method: Option<&Name>,
    ) -> Option<Vec<(DeclKey, Transfer)>> {
        let registry = self.registry;
        let classes = named_classes(registry, self.ctx.class(), class)?;
        let forwards_from = |target: &str| {
            self.ctx.class().is_some_and(|own| {
                own.eq_ignore_ascii_case(target) || registry.is_subclass_of(own, target)
            })
        };
        let mut out = Vec::new();
        for target in &classes {
            let Some(method) = method else {
                out.extend(
                    self.every_method(target)
                        .into_iter()
                        .map(|key| (key, Transfer::Static)),
                );
                continue;
            };
            match registry.lookup_method_with_owner(target, method) {
                Some((owner, sig)) => {
                    let is_static = registry
                        .classes()
                        .get(owner.as_str())
                        .is_some_and(|info| info.is_static_method(method));
                    let transfer = if !is_static && forwards_from(target) {
                        Transfer::Same
                    } else {
                        Transfer::Static
                    };
                    out.push((registry.reached(&owner, method, sig), transfer));
                }
                None => {
                    for magic in ["__callStatic", "__call"] {
                        if let Some((owner, _)) = registry.lookup_method_with_owner(target, magic) {
                            out.push((method_key(&owner, magic), Transfer::Static));
                        }
                    }
                }
            }
        }
        Some(out)
    }
}

impl Registry {
    /// What running the closure, arrow function or first-class callable a
    /// summary holds (`BodySummary::of_closure`) may write, its `$this` the
    /// caller's, an object of `class`: what it writes itself, what the
    /// closures it runs may write, and what the calls it makes may write, as
    /// settled.
    pub(crate) fn closure_effect(&self, summary: &BodySummary, class: Option<&str>) -> Effect {
        let holder = DeclKey {
            class: class.map(str::to_ascii_lowercase),
            member: String::new(),
        };
        let resolve = Resolve {
            registry: self,
            class,
            member: Name::from("{closure}"),
            summary,
            value: None,
        };
        let classified: FxHashMap<DeclKey, Classification<'_>> =
            resolve.run(&holder, None).into_iter().collect();
        let classified = classifications_by_id(self, classified);
        let effects = settle(self, &classified, |id| {
            self.effect_decl_key(id)
                .map_or_else(|| Settled::of(Effect::anything()), |key| self.settled(&key))
        });
        effects
            .get(&self.effect_id_for(&holder.closure(0)))
            .map_or_else(Effect::anything, Settled::unconditional)
    }

    /// What a call may write, with what its caller knows of it: the
    /// callee's own `$this` stays its own when it is the caller's, and an
    /// argument a callee runs writes what the caller knows it may. What the
    /// closures the caller wrote that it runs may write is the caller's to
    /// add (`CallContext::runs_written`).
    pub(crate) fn call_effect(&self, call: &CallWrites, ctx: &dyn CallContext) -> Effect {
        let reach = Reach {
            registry: self,
            ctx,
        };
        let Some(targets) = reach.call_targets(&call.callee, Some(call)) else {
            return Effect::anything();
        };
        let mut effect = Effect::nothing();
        for (key, transfer) in targets {
            let runs = match self.builtin(&key) {
                Some(sig) => match reach.builtin_runs(&key, sig, &transfer, call) {
                    Some(runs) => runs,
                    None => return Effect::anything(),
                },
                None => {
                    let settled = self.settled(&key);
                    effect.join(&settled.effect.transferred(&transfer));
                    let mut runs = Vec::new();
                    for condition in &settled.runs {
                        let passing = if passes_arguments(&call.callee, &key) {
                            self.passing(call, &key, condition.at)
                        } else {
                            Passing::Unknown
                        };
                        let passed = match passing {
                            Passing::Argument(at) => {
                                let written = condition.bound(&transfer, &Transfer::Same);
                                reach.argument_runs(call, at, &written)
                            }
                            Passing::Default(runs) => Some(runs),
                            Passing::Unknown => None,
                        };
                        let Some(passed) = passed else {
                            return Effect::anything();
                        };
                        runs.extend(passed.into_iter().map(|(run, own)| {
                            let bound = condition.bound(&transfer, &own);
                            (run, bound)
                        }));
                    }
                    runs
                }
            };
            for (run, bound) in runs {
                effect.join(&self.settled_effect(&run).transferred(&bound));
            }
            if effect.is_anything() {
                break;
            }
        }
        effect
    }
}

/// Looks a body's writes and calls up in the registry, and those of each
/// closure it wrote.
struct Resolve<'r> {
    registry: &'r Registry,
    class: Option<&'r str>,
    member: Name,
    summary: &'r BodySummary,
    // A `set` hook's parameter, by its name and the type of what it is
    // passed.
    value: Option<(&'r str, &'r Type)>,
}

/// What each of a scope's calls reaches, and the places its calls may write
/// by reference; `None` when it may write anything.
type Reached<'r> = Option<(Vec<Vec<(DeclKey, Transfer)>>, Vec<&'r Place>)>;

impl<'r> Resolve<'r> {
    /// The classification of the body, under `key`, and of each closure it
    /// wrote, under its own (`DeclKey::closure`); `sig` is the body's
    /// declaration.
    fn run(
        &self,
        key: &DeclKey,
        sig: Option<&'r FunctionReflection>,
    ) -> Vec<(DeclKey, Classification<'r>)> {
        let scopes = 1 + self.summary.closures.len();
        // What each call reaches depends on the variables' types, and which
        // variables a call writes by reference on what it reaches: both are
        // found again until no variable is lost.
        let mut lost: Vec<FxHashSet<&'r str>> = vec![FxHashSet::default(); scopes];
        let (levels, reached) = loop {
            let levels = self.levels(sig, &lost);
            let reached: Vec<Reached<'r>> = levels.iter().map(Level::reach).collect();
            let mut grew = false;
            for (at, found) in reached.iter().enumerate() {
                for place in found.iter().flat_map(|(_, written_args)| written_args) {
                    if let Some(name) = place_variable(place) {
                        grew |= self.lose(&levels, &mut lost, at, name.as_str());
                    }
                }
            }
            if !grew {
                break (levels, reached);
            }
        };
        let mut out = Vec::with_capacity(scopes);
        for (at, (level, found)) in levels.iter().zip(reached).enumerate() {
            let scope_key = match at {
                0 => key.clone(),
                _ => key.closure(at as u32 - 1),
            };
            let classification = match found {
                Some((targets, written_args)) => level.classify(key, targets, written_args),
                None => Classification::anything(),
            };
            out.push((scope_key, classification));
        }
        out
    }

    /// Takes the variable `name` of the scope at `at` as written anything
    /// there, and in each scope around it that shares it by reference;
    /// whether that loses a parameter's declared type or what a body made.
    fn lose(
        &self,
        levels: &[Level<'r>],
        lost: &mut [FxHashSet<&'r str>],
        mut at: usize,
        name: &'r str,
    ) -> bool {
        let mut grew = false;
        loop {
            if lost[at].insert(name) {
                let level = &levels[at];
                grew |= level.params.contains_key(name)
                    || level.made.contains_key(name)
                    || level.origins.contains_key(name)
                    || level.base.contains_key(name);
            }
            let Some(closure) = at.checked_sub(1).map(|index| &self.summary.closures[index]) else {
                return grew;
            };
            let shared = closure
                .captures
                .iter()
                .flatten()
                .any(|(captured, by_ref)| *by_ref && captured.as_str() == name);
            if !shared {
                return grew;
            }
            at = closure.within.map_or(0, |within| within as usize + 1);
        }
    }

    /// Each scope's level: the body's, then each closure's in the order the
    /// summary lists them; the variables `lost` names at a scope are written
    /// anything there.
    fn levels(
        &self,
        sig: Option<&'r FunctionReflection>,
        lost: &[FxHashSet<&'r str>],
    ) -> Vec<Level<'r>> {
        let facts = &self.summary.writes;
        let sig_params = sig.map_or(&[][..], |sig| &sig.params[..]);
        let mut level = self.level(facts, &lost[0], facts.bindings.iter().collect());
        if !facts.computed {
            level.params = sig_params
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    !p.by_ref
                        && !facts.reassigned.iter().any(|name| name == p.name.as_ref())
                        && !lost[0].contains(p.name.as_ref())
                })
                .map(|(at, p)| {
                    let at = (!p.variadic).then_some(at as u32);
                    (p.name.as_ref(), Declared::Param { ty: &p.ty, at })
                })
                .collect();
            // A parameter holds the argument until the body writes it.
            level.made = facts
                .made
                .iter()
                .filter_map(|(name, made)| Some((name.as_str(), made.clone()?)))
                .filter(|(name, _)| {
                    !lost[0].contains(name) && !sig_params.iter().any(|p| p.name.as_ref() == *name)
                })
                .collect();
            level.base = sig_params
                .iter()
                .filter(|p| !lost[0].contains(p.name.as_ref()))
                .map(|p| {
                    let held = if p.by_ref {
                        Holder::Unknown
                    } else {
                        holder_of(&p.ty)
                    };
                    (p.name.as_ref(), held)
                })
                .collect();
            if let Some((name, ty)) = self.value
                && !lost[0].contains(name)
            {
                level.base.insert(name, holder_of(ty));
            }
        }
        let mut levels = vec![level];
        for (index, closure) in self.summary.closures.iter().enumerate() {
            let parent = closure.within.map_or(0, |within| within as usize + 1);
            let level = self.closure_level(closure, &levels[parent], &lost[index + 1]);
            levels.push(level);
        }
        levels
    }

    /// The level of `closure`, written in the scope whose level is `parent`.
    fn closure_level(
        &self,
        closure: &'r ClosureFacts,
        parent: &Level<'r>,
        lost: &FxHashSet<&'r str>,
    ) -> Level<'r> {
        let facts = &closure.facts;
        let reassigned = |name: &str| facts.reassigned.iter().any(|n| n.as_str() == name);
        let is_param = |name: &str| closure.params.iter().any(|p| p.name.as_str() == name);
        let mut params: FxHashMap<&'r str, Declared<'r>> = closure
            .params
            .iter()
            .filter(|p| !p.untyped && !reassigned(&p.name) && !lost.contains(p.name.as_str()))
            .map(|p| (p.name.as_str(), Declared::Classes(p.classes.as_deref())))
            .collect();
        let mut made: FxHashMap<&'r str, Made> = FxHashMap::default();
        let mut base: FxHashMap<&'r str, Holder> = closure
            .params
            .iter()
            .filter(|p| !lost.contains(p.name.as_str()))
            .map(|p| {
                let held = if p.untyped {
                    Holder::Unknown
                } else {
                    parent.declared(&Declared::Classes(p.classes.as_deref()))
                };
                (p.name.as_str(), held)
            })
            .collect();
        let mut bindings: Vec<&'r (Option<Name>, Binding)> = facts.bindings.iter().collect();
        // An arrow function and a first-class callable take every variable
        // but their parameters, so one the code around them holds nothing
        // made in may hold anything in them, even after they write it.
        let captures: Vec<(&'r str, bool)> = match &closure.captures {
            Some(captures) => captures
                .iter()
                .map(|(name, by_ref)| (name.as_str(), *by_ref))
                .collect(),
            None => {
                let mut names: Vec<&'r str> = parent
                    .params
                    .keys()
                    .chain(parent.made.keys())
                    .chain(parent.origins.keys())
                    .chain(parent.base.keys())
                    .copied()
                    .chain(facts.origins.iter().map(|(name, _)| name.as_str()))
                    .chain(facts.reassigned.iter().map(Name::as_str))
                    .filter(|name| !is_param(name))
                    .collect();
                names.sort_unstable();
                names.dedup();
                names.into_iter().map(|name| (name, false)).collect()
            }
        };
        for &(name, by_ref) in &captures {
            if lost.contains(name) {
                continue;
            }
            base.insert(name, parent.variable_holder(&Name::from(name)));
            if let Some(declared) = parent.params.get(name)
                && !reassigned(name)
            {
                params.insert(name, *declared);
            }
            if let Some(held) = parent.made.get(name) {
                let written = facts.made.iter().find(|(n, _)| n.as_str() == name);
                let joined = match written {
                    None => Some(held.clone()),
                    Some((_, Some(own))) => held.join(own),
                    Some((_, None)) => None,
                };
                if let Some(joined) = joined {
                    made.insert(name, joined);
                }
            }
            if by_ref {
                bindings.extend(
                    parent
                        .bindings
                        .iter()
                        .copied()
                        .filter(|(bound, _)| bound.as_ref().is_none_or(|bound| bound == name)),
                );
            }
        }
        // A closure's own variable holds nothing until it writes it.
        if closure.captures.is_some() {
            for (name, held) in facts.made.iter() {
                let name = name.as_str();
                if let Some(held) = held
                    && !is_param(name)
                    && !lost.contains(name)
                    && !captures.iter().any(|&(captured, _)| captured == name)
                {
                    made.insert(name, held.clone());
                }
            }
        }
        let mut level = self.level(facts, lost, bindings);
        level.closure = true;
        if !facts.computed {
            level.params = params;
            level.made = made;
            level.base = base;
        }
        level
    }

    /// The level of a scope whose facts are `facts`, with no variable
    /// known to hold anything but where its writes say what it holds;
    /// `lost` names the variables written anything there.
    fn level(
        &self,
        facts: &'r WriteFacts,
        lost: &FxHashSet<&'r str>,
        bindings: Vec<&'r (Option<Name>, Binding)>,
    ) -> Level<'r> {
        let origins = if facts.computed {
            FxHashMap::default()
        } else {
            facts
                .origins
                .iter()
                .map(|(name, origins)| (name.as_str(), &origins[..]))
                .filter(|(name, _)| !lost.contains(name))
                .collect()
        };
        Level {
            registry: self.registry,
            class: self.class,
            member: self.member.clone(),
            facts,
            params: FxHashMap::default(),
            made: FxHashMap::default(),
            origins,
            base: FxHashMap::default(),
            solved: Default::default(),
            closure: false,
            bindings,
            runs: Default::default(),
            params_run: Default::default(),
        }
    }
}

/// What a parameter is declared to hold.
#[derive(Clone, Copy)]
enum Declared<'r> {
    /// A declaration's parameter, by its type, and the position its
    /// argument is passed at; `None` for a variadic one.
    Param { ty: &'r Type, at: Option<u32> },
    /// A closure's, by the classes its type admits objects of; `None` for
    /// any.
    Classes(Option<&'r [QualifiedName]>),
}

/// One scope of a body, as `Resolve` reads it: the body's own, or a
/// closure's.
struct Level<'r> {
    registry: &'r Registry,
    class: Option<&'r str>,
    member: Name,
    facts: &'r WriteFacts,
    // The parameters whose declared type holds all through the scope:
    // neither assigned nor passed where a parameter is by reference. A
    // closure's are its own and those it takes of the code that wrote it.
    params: FxHashMap<&'r str, Declared<'r>>,
    // The variables that only ever hold what the body made.
    made: FxHashMap<&'r str, Made>,
    // The variables whose every value written comes from where the scope
    // shows, each with where.
    origins: FxHashMap<&'r str, &'r [Origin]>,
    // What a variable holds before the scope writes it: a declaration's
    // parameter its declared type's objects, a closure's parameter or
    // capture what its declared type or the code that wrote it holds.
    base: FxHashMap<&'r str, Holder>,
    // The object each variable of `origins` may hold, found once; filled
    // while it is found.
    solved: std::cell::RefCell<Option<FxHashMap<&'r str, Holder>>>,
    // A closure's scope, which may be bound to any class's.
    closure: bool,
    // What its variables are bound to: its own bindings and, for a variable
    // a closure takes by reference, those of the code that wrote it.
    bindings: Vec<&'r (Option<Name>, Binding)>,
    // The closures its calls run, each with its `$this`.
    runs: std::cell::RefCell<Vec<(u32, Transfer)>>,
    // The declaration's parameters whose arguments its calls run.
    params_run: std::cell::RefCell<Vec<Condition>>,
}

impl CallContext for Level<'_> {
    fn class(&self) -> Option<&str> {
        self.class
    }

    /// The object a method is called on, and whether it is the body's own
    /// `$this`.
    fn receiver(&self, receiver: &Receiver) -> (Holder, bool) {
        let registry = self.registry;
        match receiver {
            Receiver::This => (
                match self.class {
                    Some(class) => Holder::Classes(vec![Name::from(class)]),
                    None => Holder::Unknown,
                },
                true,
            ),
            Receiver::ThisProperty(prop) => (
                self.class
                    .and_then(|class| registry.lookup_property(class, prop))
                    .map_or(Holder::Unknown, |p| holder_of(p.ty())),
                false,
            ),
            Receiver::Variable(name) => (self.variable_holder(name), false),
            Receiver::Static(class, prop) => (self.static_holder(class, prop), false),
            Receiver::Unknown => (Holder::Unknown, false),
        }
    }

    fn argument(&self, _: &ArgAt, place: Option<&Place>) -> Holder {
        match place {
            Some(place) => self.value_holder(&place.root, &place.steps),
            None => Holder::Unknown,
        }
    }

    fn runs_written(&self, written: Written<'_>, transfer: &Transfer) -> bool {
        let held = |name: &Name| match self.made.get(name.as_str()) {
            Some(Made::Closure(ids)) => Some(&ids[..]),
            _ => None,
        };
        let ids = match written {
            Written::Argument {
                callable: Some(Callable::Written(id)),
                ..
            } => std::slice::from_ref(id),
            Written::Argument {
                callable: None,
                place: Some(place),
                ..
            } => match place_variable(place) {
                Some(name) => match held(name) {
                    Some(ids) => ids,
                    None => return self.runs_param(name, transfer),
                },
                None => return false,
            },
            Written::Argument { .. } => return false,
            Written::Variable(name) => match held(name) {
                Some(ids) => ids,
                None => return self.runs_param(name, transfer),
            },
            Written::Callee(ids) => ids,
        };
        self.runs
            .borrow_mut()
            .extend(ids.iter().map(|&id| (id, transfer.clone())));
        true
    }

    fn operand(&self, on: Option<&[Origin]>) -> (Holder, bool) {
        let Some(origins) = on else {
            return (Holder::Unknown, false);
        };
        let mut holder = Holder::Classes(Vec::new());
        let (mut this, mut other) = (false, false);
        for origin in origins {
            let (found, same) = self.origin_holder(origin);
            if same {
                this = true;
            } else if found != Holder::Classes(Vec::new()) {
                other = true;
            }
            holder = holder.join(found);
        }
        (holder, this && !other)
    }

    fn rebindable(&self) -> bool {
        self.closure
    }
}

impl<'r> Level<'r> {
    /// Notes that a call runs the variable `name`, its `$this` what
    /// `transfer` says, when it holds a parameter's argument all through the
    /// scope; whether it does.
    fn runs_param(&self, name: &Name, transfer: &Transfer) -> bool {
        let Some(Declared::Param { at: Some(at), .. }) = self.params.get(name.as_str()) else {
            return false;
        };
        let this = match transfer {
            Transfer::Same => None,
            other => Some(other.clone()),
        };
        self.params_run
            .borrow_mut()
            .push(Condition { at: *at, this });
        true
    }

    /// What each of the scope's calls reaches, and the places they may
    /// write by reference; `None` when a call may run code not known, or the
    /// scope may write anything.
    fn reach(&self) -> Reached<'r> {
        let facts: &'r WriteFacts = self.facts;
        if facts.anything {
            return None;
        }
        let reach = Reach {
            registry: self.registry,
            ctx: self,
        };
        let mut reached = Vec::with_capacity(facts.calls.len());
        let mut written_args: Vec<&'r Place> = Vec::new();
        for call in facts.calls.iter() {
            let ran = (self.runs.borrow().len(), self.params_run.borrow().len());
            let targets = reach.call_targets(&call.callee, Some(call))?;
            // A closure the body wrote or a parameter's argument, called:
            // what it takes by reference is not looked up.
            let unknown_params = (self.runs.borrow().len(), self.params_run.borrow().len()) != ran;
            for (at, place) in call.places.iter() {
                if unknown_params || targets.iter().any(|(key, _)| self.by_ref_at(key, at)) {
                    written_args.push(place);
                }
            }
            reached.push(targets);
        }
        Some((reached, written_args))
    }

    /// The scope's own writes, and the declarations and closures of
    /// `holder` it inherits writes from: what its calls reach, each builtin
    /// among them by what it runs, and the closures they run; and the
    /// parameters whose arguments its calls run.
    fn classify(
        &self,
        holder: &DeclKey,
        reached: Vec<Vec<(DeclKey, Transfer)>>,
        written_args: Vec<&Place>,
    ) -> Classification<'r> {
        let reach = Reach {
            registry: self.registry,
            ctx: self,
        };
        let mut calls: Vec<(DeclKey, Transfer, Option<u32>)> = Vec::new();
        let mut sites: Vec<Site<'r>> = Vec::new();
        // The declarations run with no call's arguments, each once.
        let mut siteless: FxHashSet<(DeclKey, Transfer)> = FxHashSet::default();
        let facts: &'r WriteFacts = self.facts;
        for (call, targets) in facts.calls.iter().zip(reached) {
            let mut site: Option<u32> = None;
            for (key, transfer) in targets {
                // A builtin writes what the code it runs writes: what is
                // passed to it, and what of its object a class extending it
                // declares.
                if let Some(sig) = self.registry.builtin(&key) {
                    let Some(runs) = reach.builtin_runs(&key, sig, &transfer, call) else {
                        return Classification::anything();
                    };
                    for (key, transfer) in runs {
                        if siteless.insert((key.clone(), transfer.clone())) {
                            calls.push((key, transfer, None));
                        }
                    }
                    continue;
                }
                let at = passes_arguments(&call.callee, &key).then(|| {
                    *site.get_or_insert_with(|| {
                        let passed = self.passed(holder, &reach, call);
                        sites.push(Site { call, passed });
                        sites.len() as u32 - 1
                    })
                });
                if at.is_some() || siteless.insert((key.clone(), transfer.clone())) {
                    calls.push((key, transfer, at));
                }
            }
        }
        let mut own = Effect::nothing();
        for place in self.facts.writes.iter().chain(written_args) {
            self.place_storages(place, &mut own, 0);
            if own.is_anything() {
                break;
            }
        }
        calls.extend(
            self.runs
                .take()
                .into_iter()
                .map(|(id, transfer)| (holder.closure(id), transfer, None)),
        );
        Classification {
            own,
            calls,
            sites,
            runs: self.params_run.take(),
        }
    }

    /// What each argument of `call` runs when it is called, where the scope
    /// knows. Whether a declaration the call runs calls it is not known yet,
    /// so none of it is noted as run.
    fn passed(
        &self,
        holder: &DeclKey,
        reach: &Reach<'_>,
        call: &'r CallWrites,
    ) -> Vec<(&'r ArgAt, Passed)> {
        let mut out = Vec::new();
        for at in call.arguments.iter() {
            let ran = (self.runs.borrow().len(), self.params_run.borrow().len());
            let runs = reach.argument_runs(call, at, &Transfer::Same);
            let closures = self.runs.borrow_mut().split_off(ran.0);
            let params = self.params_run.borrow_mut().split_off(ran.1);
            if let Some(mut runs) = runs {
                runs.extend(
                    closures
                        .into_iter()
                        .map(|(id, transfer)| (holder.closure(id), transfer)),
                );
                out.push((at, Passed { runs, params }));
            }
        }
        out
    }

    /// Whether the declaration `key` takes the argument passed `at` by
    /// reference.
    fn by_ref_at(&self, key: &DeclKey, at: &ArgAt) -> bool {
        let Some(sig) = self.registry.declaration(key) else {
            return true;
        };
        match at {
            ArgAt::Position(at) => sig
                .params
                .get(*at as usize)
                .or_else(|| sig.params.last().filter(|p| p.variadic))
                .is_some_and(|p| p.by_ref),
            ArgAt::Named(name) => sig
                .params
                .iter()
                .find(|p| p.name.as_ref() == name.as_str())
                .or_else(|| sig.params.last().filter(|p| p.variadic))
                .is_some_and(|p| p.by_ref),
            ArgAt::Spread => sig.params.iter().any(|p| p.by_ref),
        }
    }

    /// The object the variable `name` holds: what the body made, by its
    /// class; a parameter it never writes, by its declared type; one whose
    /// every value comes from where the scope shows, by what that holds,
    /// with what it held before the scope wrote it; anything else not
    /// known.
    fn variable_holder(&self, name: &Name) -> Holder {
        if let Some(made) = self.made.get(name.as_str()) {
            return match made {
                Made::Object(Some(class)) => Holder::Made(Some(
                    named_classes(self.registry, self.class, class).unwrap_or_default(),
                )),
                Made::Object(None) => Holder::Made(None),
                Made::Closure(_) => Holder::Made(Some(vec![Name::from("Closure")])),
            };
        }
        if let Some(declared) = self.params.get(name.as_str()) {
            return self.declared(declared);
        }
        if self.origins.contains_key(name.as_str()) {
            if self.solved.borrow().is_none() {
                self.solve();
            }
            if let Some(solved) = &*self.solved.borrow() {
                return solved
                    .get(name.as_str())
                    .cloned()
                    .unwrap_or(Holder::Unknown);
            }
        }
        match self.base.get(name.as_str()) {
            Some(held) if !self.facts.reassigned.contains(name) => held.clone(),
            _ => Holder::Unknown,
        }
    }

    /// The object a parameter declared as `declared` holds.
    fn declared(&self, declared: &Declared<'_>) -> Holder {
        match declared {
            Declared::Param { ty, .. } => holder_of(ty),
            Declared::Classes(Some(classes)) => {
                let mut found = Vec::new();
                for class in *classes {
                    match named_classes(self.registry, self.class, class) {
                        Some(named) => found.extend(named),
                        None => return Holder::Unknown,
                    }
                }
                Holder::Classes(found)
            }
            Declared::Classes(None) => Holder::Unknown,
        }
    }

    /// Finds the object each variable of `origins` may hold: what it held
    /// before the scope wrote it and what each value written may be, taken
    /// again until none grows. Each starts as no object, and is read as
    /// found so far while the others are found.
    fn solve(&self) {
        let start: FxHashMap<&'r str, Holder> = self
            .origins
            .keys()
            .map(|&name| (name, Holder::Classes(Vec::new())))
            .collect();
        *self.solved.borrow_mut() = Some(start);
        // A holder grows at most to any object; a chain of variables this
        // long has not settled.
        for _ in 0..16 {
            let mut grew = false;
            for (&name, origins) in &self.origins {
                let mut held = self
                    .base
                    .get(name)
                    .cloned()
                    .unwrap_or(Holder::Classes(Vec::new()));
                for origin in origins.iter() {
                    held = held.join(self.origin_holder(origin).0);
                }
                let held = match held {
                    Holder::Classes(classes) => Holder::Classes(sorted_classes(classes)),
                    Holder::Made(Some(classes)) => Holder::Made(Some(sorted_classes(classes))),
                    other => other,
                };
                let mut solved = self.solved.borrow_mut();
                let Some(solved) = solved.as_mut() else {
                    unreachable!("filled above");
                };
                if solved.get(name) != Some(&held) {
                    solved.insert(name, held);
                    grew = true;
                }
            }
            if !grew {
                return;
            }
        }
        if let Some(solved) = self.solved.borrow_mut().as_mut() {
            for held in solved.values_mut() {
                *held = Holder::Unknown;
            }
        }
    }

    /// The object a value from `origin` may be, and whether it is the
    /// scope's own `$this`.
    fn origin_holder(&self, origin: &Origin) -> (Holder, bool) {
        let registry = self.registry;
        match origin {
            Origin::NoObject | Origin::Arithmetic | Origin::Closure => {
                (Holder::Classes(Vec::new()), false)
            }
            Origin::Place(place) if matches!(place.root, Root::This) && place.steps.is_empty() => {
                (self.value_holder(&place.root, &[]), true)
            }
            Origin::Place(place) => (self.value_holder(&place.root, &place.steps), false),
            Origin::Returned(callee) => (self.returned_holder(callee), false),
            Origin::New(Some(class)) => (
                Holder::Made(Some(
                    named_classes(registry, self.class, class).unwrap_or_default(),
                )),
                false,
            ),
            Origin::New(None) | Origin::Anonymous(None) => (Holder::Made(None), false),
            Origin::Anonymous(Some(name)) => (Holder::Made(Some(vec![name.clone()])), false),
            Origin::Caught(classes) => {
                let mut found = Vec::new();
                for class in classes.iter() {
                    match named_classes(registry, self.class, class) {
                        Some(named) => found.extend(named),
                        None => return (Holder::Unknown, false),
                    }
                }
                (Holder::Classes(found), false)
            }
        }
    }

    /// The object what a call of `callee` returns may be, by the return
    /// types its targets declare.
    fn returned_holder(&self, callee: &Callee) -> Holder {
        let registry = self.registry;
        match callee {
            Callee::Function(name) => match registry.resolve_function(name) {
                Some((_, sig)) => holder_of(sig.return_type()),
                // A call to no function throws.
                None => Holder::Classes(Vec::new()),
            },
            Callee::Method {
                receiver,
                method: Some(method),
            } => {
                let (Holder::Classes(classes) | Holder::Made(Some(classes))) =
                    self.receiver(receiver).0
                else {
                    return Holder::Unknown;
                };
                let mut held = Holder::Classes(Vec::new());
                for class in &classes {
                    let found = match registry.lookup_method_with_owner(class, method) {
                        Some((owner, sig)) if !sig.is_magic => {
                            declared_holder(registry, sig.return_type(), class, &owner)
                        }
                        // `__call`, or a method of a class below.
                        _ => {
                            let mut found = Holder::Classes(Vec::new());
                            for key in registry.implementations(class, method) {
                                let Some((owner, sig)) = key.class.as_deref().and_then(|owner| {
                                    registry.lookup_method_with_owner(owner, &key.member)
                                }) else {
                                    return Holder::Unknown;
                                };
                                found = found.join(declared_holder(
                                    registry,
                                    sig.return_type(),
                                    &owner,
                                    &owner,
                                ));
                            }
                            found
                        }
                    };
                    held = held.join(found);
                }
                held
            }
            Callee::Static {
                class: Some(class),
                method: Some(method),
            } => {
                let Some(targets) = named_classes(registry, self.class, class) else {
                    return Holder::Unknown;
                };
                let mut held = Holder::Classes(Vec::new());
                for target in &targets {
                    match registry.lookup_method_with_owner(target, method) {
                        Some((owner, sig)) if !sig.is_magic => {
                            held = held.join(declared_holder(
                                registry,
                                sig.return_type(),
                                target,
                                &owner,
                            ));
                        }
                        _ => {
                            for magic in ["__callStatic", "__call"] {
                                if let Some((owner, sig)) =
                                    registry.lookup_method_with_owner(target, magic)
                                {
                                    held = held.join(declared_holder(
                                        registry,
                                        sig.return_type(),
                                        target,
                                        &owner,
                                    ));
                                }
                            }
                        }
                    }
                }
                held
            }
            // A `set` hook returns nothing.
            Callee::ParentHook { set: true, .. } => Holder::Classes(Vec::new()),
            Callee::ParentHook {
                property,
                set: false,
            } => {
                let Some(parent) = self
                    .class
                    .and_then(|class| registry.classes().get(class))
                    .and_then(|info| info.extends().first())
                else {
                    return Holder::Unknown;
                };
                match registry.lookup_property_with_owner(parent, property) {
                    Some((owner, declared)) => {
                        declared_holder(registry, declared.ty(), parent, &owner)
                    }
                    None => Holder::Unknown,
                }
            }
            _ => Holder::Unknown,
        }
    }

    /// The object a static property holds, by its declared type.
    fn static_holder(&self, class: &QualifiedName, prop: &Name) -> Holder {
        let mut classes = Vec::new();
        for (owner, _) in self.static_property_owners(class, Some(prop)) {
            let Some(owner) = owner else {
                return Holder::Unknown;
            };
            match self
                .registry
                .lookup_property(&owner, prop)
                .map(|p| holder_of(p.ty()))
            {
                Some(Holder::Classes(found)) => classes.extend(found),
                _ => return Holder::Unknown,
            }
        }
        Holder::Classes(classes)
    }

    /// Each storage `class::$prop` may be: the class declaring the static
    /// property, as seen from each class `class` may be, with its name;
    /// `None` for a class not known.
    fn static_property_owners(
        &self,
        class: &QualifiedName,
        prop: Option<&Name>,
    ) -> Vec<(Option<Name>, Option<Name>)> {
        let Some(classes) = named_classes(self.registry, self.class, class) else {
            return vec![(None, prop.cloned())];
        };
        let mut out: Vec<(Option<Name>, Option<Name>)> = Vec::new();
        for candidate in classes {
            let storage = match prop {
                Some(prop) => match self.registry.lookup_property_with_owner(&candidate, prop) {
                    Some((owner, p)) if p.is_static() => (Some(owner), Some(prop.clone())),
                    // No such static property: the access throws.
                    _ => continue,
                },
                None => (Some(candidate), None),
            };
            if !out.contains(&storage) {
                out.push(storage);
            }
        }
        out
    }

    /// Adds the storages a write to `place` may change to `out`.
    fn place_storages(&self, place: &Place, out: &mut Effect, depth: u32) {
        // A chain of references this long is a cycle.
        if depth > 16 {
            *out = Effect::anything();
            return;
        }
        let last_property = place
            .steps
            .iter()
            .rposition(|step| matches!(step, Step::Property(_)));
        if let Some(at) = last_property {
            let Step::Property(prop) = &place.steps[at] else {
                unreachable!();
            };
            if at == 0 && matches!(place.root, Root::This) {
                out.insert(Storage::Own(prop.clone()));
                return;
            }
            match self.value_holder(&place.root, &place.steps[..at]) {
                Holder::Made(_) => {}
                Holder::Unknown => out.insert(Storage::Property {
                    class: None,
                    prop: prop.clone(),
                }),
                Holder::Classes(classes) => {
                    for class in classes {
                        out.insert(Storage::Property {
                            class: Some(class),
                            prop: prop.clone(),
                        });
                    }
                }
            }
            return;
        }
        match &place.root {
            Root::This | Root::Value => {}
            Root::Variable(name) => self.variable_storages(name.as_ref(), place, out, depth),
            Root::Static(class, prop) => {
                for (owner, prop) in self.static_property_owners(class, prop.as_ref()) {
                    out.insert(Storage::Static { class: owner, prop });
                }
            }
            Root::DynamicStatic(prop) => out.insert(Storage::Static {
                class: None,
                prop: prop.clone(),
            }),
            Root::Global(name) => out.insert(Storage::Global(name.clone())),
        }
    }

    /// Adds what a write to the variable `name` (any, when `None`) may
    /// change by what it is bound to: a global, the declaration's static
    /// variable, or the place a reference was taken of.
    fn variable_storages(&self, name: Option<&Name>, place: &Place, out: &mut Effect, depth: u32) {
        for (bound, binding) in self.bindings.iter().copied() {
            let applies = match (bound, name) {
                (None, _) | (_, None) => true,
                (Some(bound), Some(name)) => bound == name,
            };
            if !applies {
                continue;
            }
            let variable = bound.as_ref().or(name);
            match binding {
                Binding::Global(global) => out.insert(Storage::Global(
                    global.clone().or_else(|| variable.cloned()),
                )),
                Binding::Static => out.insert(Storage::StaticVar {
                    class: self.class.map(Name::from),
                    function: self.member.clone(),
                    var: variable.cloned(),
                }),
                Binding::Reference(target) => {
                    let mut steps = target.steps.to_vec();
                    steps.extend(place.steps.iter().cloned());
                    let through = Place {
                        root: target.root.clone(),
                        steps: steps.into_boxed_slice(),
                    };
                    self.place_storages(&through, out, depth + 1);
                }
            }
        }
    }

    /// The object at `root` and `steps` may be, by the declared types along
    /// the way.
    fn value_holder(&self, root: &Root, steps: &[Step]) -> Holder {
        let start = match root {
            Root::This => match self.class {
                Some(class) => Holder::Classes(vec![Name::from(class)]),
                None => Holder::Unknown,
            },
            Root::Variable(Some(name)) => self.variable_holder(name),
            Root::Static(class, Some(prop)) => self.static_holder(class, prop),
            Root::Variable(None)
            | Root::Static(_, None)
            | Root::DynamicStatic(_)
            | Root::Global(_)
            | Root::Value => Holder::Unknown,
        };
        if steps.is_empty() {
            return start;
        }
        let (Holder::Classes(classes) | Holder::Made(Some(classes))) = start else {
            return Holder::Unknown;
        };
        // The types of the value reached so far, once a step leaves the
        // object the walk starts from.
        let mut reached: Option<Vec<Type>> = None;
        for step in steps {
            reached = Some(match (step, &reached) {
                (Step::Property(Some(prop)), None) => match self.property_types(&classes, prop) {
                    Some(types) => types,
                    None => return Holder::Unknown,
                },
                (Step::Property(Some(prop)), Some(types)) => {
                    let mut holders = Vec::new();
                    for ty in types {
                        match object_classes(ty) {
                            Some(found) => holders.extend(found),
                            None => return Holder::Unknown,
                        }
                    }
                    match self.property_types(&holders, prop) {
                        Some(types) => types,
                        None => return Holder::Unknown,
                    }
                }
                (Step::Element, Some(types)) => {
                    match types.iter().map(Type::array_value_type).collect() {
                        Some(types) => types,
                        None => return Holder::Unknown,
                    }
                }
                (Step::Property(None), _) | (Step::Element, None) => return Holder::Unknown,
            });
        }
        let mut found = Vec::new();
        for ty in reached.into_iter().flatten() {
            match object_classes(&ty) {
                Some(classes) => found.extend(classes),
                None => return Holder::Unknown,
            }
        }
        Holder::Classes(found)
    }

    /// The declared types of the property `prop` on each of `classes`;
    /// `None` when one has no such property.
    fn property_types(&self, classes: &[Name], prop: &str) -> Option<Vec<Type>> {
        classes
            .iter()
            .map(|class| {
                self.registry
                    .lookup_property(class, prop)
                    .map(|p| p.ty().clone())
            })
            .collect()
    }
}

/// `class::member`, or the function's name.
impl syntax::canonical::Canonical for DeclKey {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let DeclKey { class, member } = self;
        if let Some(class) = class {
            p.name(class);
            p.atom("::");
        }
        p.name(member);
    }
}

/// `anything`, or the storages.
impl syntax::canonical::Canonical for Effect {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let Effect(storages) = self;
        match storages {
            Some(storages) => storages.canonical(p),
            None => p.atom("anything"),
        }
    }
}

syntax::canonical_struct!(Settled { effect, runs });

syntax::canonical_struct!(Condition { at, this });

syntax::canonical_enum!(Transfer {
    Same,
    Other(class),
    Made,
    Static,
});

syntax::canonical_enum!(Storage {
    Own(prop),
    Property { class, prop },
    Static { class, prop },
    Global(name),
    StaticVar {
        class,
        function,
        var,
    },
});
