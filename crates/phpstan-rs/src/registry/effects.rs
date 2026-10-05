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

use std::collections::BTreeSet;
use std::sync::Arc;

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::{ClassKind, QualifiedName, ResolvedName};
use syntax::name::Name;

use crate::registry::bodies::BodySource;
use crate::registry::summaries::{
    ArgAt, Binding, BodySummary, CallWrites, Callable, Callee, ClosureFacts, Made, PairTarget,
    Place, Receiver, Root, Step, WriteFacts,
};
use crate::registry::{ClassReflection, FunctionReflection, NameSet, Registry, is_non_overridable};
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
/// Each name's classes are sorted, so what is made of them does not depend
/// on the order the classes were declared in.
fn methods_by_name(registry: &Registry) -> FxHashMap<String, Vec<Name>> {
    let mut by_name: FxHashMap<String, Vec<Name>> = FxHashMap::default();
    for (class, info) in registry.classes.own_iter() {
        for (method, sig) in info.methods.iter() {
            if !sig.is_stub {
                by_name
                    .entry(method.to_ascii_lowercase())
                    .or_default()
                    .push(class.clone());
            }
        }
    }
    for classes in by_name.values_mut() {
        classes.sort_unstable_by(|a, b| (a.lower(), a.as_str()).cmp(&(b.lower(), b.as_str())));
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
        || class
            .abstract_methods()
            .iter()
            .any(|m| m.eq_ignore_ascii_case(method))
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

impl Registry {
    /// Settles what every function and method with a body may write. Run with
    /// `resolve_transitive_purity`, after every class is indexed and
    /// flattened. Every declaration is resolved again on each run: what a
    /// call reaches depends on the whole hierarchy, which any file may
    /// change.
    pub(crate) fn resolve_effects(&mut self, bodies: &dyn BodySource) {
        self.methods_by_name = Arc::new(methods_by_name(self));
        let mut running_code = (*self.base_builtin_methods_running_code).clone();
        for name in self.own_builtin_methods_running_code() {
            running_code.insert(name);
        }
        self.builtin_methods_running_code = running_code;
        let registry: &Registry = self;
        let functions: Vec<(&Name, &FunctionReflection)> = registry
            .functions
            .own_iter()
            .filter(|(_, sig)| !sig.is_stub)
            .collect();
        let methods: Vec<(&Name, &Name, &FunctionReflection)> = registry
            .classes
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
        let mut effects = settle(registry, &classified, |key| {
            Settled::of(registry.declaration_effect(key))
        });
        // A closure's effect is taken where it runs; no call names it.
        effects.retain(|key, _| !key.is_closure());
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
        match self.effects.get(key) {
            Some(settled) => settled.clone(),
            None => Settled::of(self.declaration_effect(key)),
        }
    }

    /// What a call to the declaration `key` whose arguments are not known
    /// may write, as settled.
    fn settled_effect(&self, key: &DeclKey) -> Effect {
        match self.effects.get(key) {
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
            let declaring = self.classes_declaring(method);
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
    fn classes_declaring(&self, method: &str) -> &[Name] {
        self.methods_by_name
            .get(&method.to_ascii_lowercase())
            .map_or(&[], Vec::as_slice)
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
        let declaring = self
            .classes_declaring(method)
            .iter()
            .map(|class| (class, method));
        let magic = ["__call", "__callStatic"].into_iter().flat_map(|magic| {
            self.classes_declaring(magic)
                .iter()
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
            member,
            summary,
        };
        resolve.run(&key, Some(sig))
    }
}

/// What each declaration `classified` holds may write, settled from what it
/// writes itself: it grows by what each declaration it calls may write, by
/// `outside` for one `classified` does not hold, and, where a callee runs
/// what a call passes it, by what that may write.
fn settle(
    registry: &Registry,
    classified: &FxHashMap<DeclKey, Classification<'_>>,
    outside: impl Fn(&DeclKey) -> Settled,
) -> FxHashMap<DeclKey, Settled> {
    let mut settled: FxHashMap<DeclKey, Settled> = classified
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
    let mut callers: FxHashMap<&DeclKey, Vec<&DeclKey>> = FxHashMap::default();
    for (key, classification) in classified {
        for (callee, _, _) in &classification.calls {
            if classified.contains_key(callee) {
                callers.entry(callee).or_default().push(key);
            }
        }
    }
    // The declarations an argument runs, which a declaration reads only once
    // a callee runs the argument.
    let mut passed_to: FxHashSet<(&DeclKey, &DeclKey)> = FxHashSet::default();
    let mut queue: std::collections::VecDeque<&DeclKey> = classified.keys().collect();
    let mut queued: FxHashSet<&DeclKey> = queue.iter().copied().collect();
    while let Some(key) = queue.pop_front() {
        queued.remove(key);
        let classification = &classified[key];
        let before = &settled[key];
        if before.effect.is_anything() {
            continue;
        }
        let mut now = before.clone();
        let mut read: Vec<DeclKey> = Vec::new();
        let get = |callee: &DeclKey| match settled.get(callee) {
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
                let site = site.map(|site| &classification.sites[site as usize]);
                run_passed(
                    registry, site, callee, transfer, condition, &get, &mut now, &mut read,
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
    site: Option<&Site<'_>>,
    callee: &DeclKey,
    transfer: &Transfer,
    condition: &Condition,
    get: &impl Fn(&DeclKey) -> Settled,
    into: &mut Settled,
    read: &mut Vec<DeclKey>,
) {
    let Some(site) = site else {
        into.effect = Effect::anything();
        return;
    };
    let default;
    let (runs, params) = match registry.passing(site.call, callee, condition.at) {
        Passing::Argument(at) => match site.passed.iter().find(|(passed, _)| *passed == at) {
            Some((_, passed)) => (&passed.runs[..], &passed.params[..]),
            None => {
                into.effect = Effect::anything();
                return;
            }
        },
        Passing::Default(runs) => {
            default = runs;
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
#[derive(Clone)]
pub(crate) enum Holder {
    /// An object of one of the classes, or of a descendant of one.
    Classes(Vec<Name>),
    /// An object the caller made, no one else has: of exactly one of the
    /// classes, or of a class not known.
    Made(Option<Vec<Name>>),
    /// An object of any class.
    Unknown,
}

/// The holder of a value of type `ty`.
pub(crate) fn holder_of(ty: &Type) -> Holder {
    match object_classes(ty) {
        Some(classes) => Holder::Classes(classes),
        None => Holder::Unknown,
    }
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
        vec![registry.classes.get(own?)?.extends().first()?.clone()]
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
                    None => self.registry.classes_declaring("__construct").to_vec(),
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
        })
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
                    let is_static = registry.classes.get(owner.as_str()).is_some_and(|info| {
                        info.static_methods()
                            .iter()
                            .any(|m| m.eq_ignore_ascii_case(method))
                    });
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
        let member = Name::from("{closure}");
        let resolve = Resolve {
            registry: self,
            class,
            member: &member,
            summary,
        };
        let classified: FxHashMap<DeclKey, Classification<'_>> =
            resolve.run(&holder, None).into_iter().collect();
        let effects = settle(self, &classified, |key| self.settled(key));
        effects
            .get(&holder.closure(0))
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
    member: &'r Name,
    summary: &'r BodySummary,
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
                grew |= levels[at].params.contains_key(name) || levels[at].made.contains_key(name);
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
        let params = sig_params
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
        let made = facts
            .made
            .iter()
            .filter_map(|(name, made)| Some((name.as_str(), made.clone()?)))
            .filter(|(name, _)| {
                !lost[0].contains(name) && !sig_params.iter().any(|p| p.name.as_ref() == *name)
            })
            .collect();
        let mut levels = vec![self.level(facts, params, made, facts.bindings.iter().collect())];
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
        let mut bindings: Vec<&'r (Option<Name>, Binding)> = facts.bindings.iter().collect();
        // An arrow function and a first-class callable take every variable
        // but their parameters, so one the code around them holds nothing
        // made in may hold anything in them, even after they write it.
        let captures: Vec<(&'r str, bool)> = match &closure.captures {
            Some(captures) => captures
                .iter()
                .map(|(name, by_ref)| (name.as_str(), *by_ref))
                .collect(),
            None => parent
                .params
                .keys()
                .chain(parent.made.keys())
                .copied()
                .filter(|name| !is_param(name))
                .map(|name| (name, false))
                .collect(),
        };
        for &(name, by_ref) in &captures {
            if lost.contains(name) {
                continue;
            }
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
        self.level(facts, params, made, bindings)
    }

    fn level(
        &self,
        facts: &'r WriteFacts,
        params: FxHashMap<&'r str, Declared<'r>>,
        made: FxHashMap<&'r str, Made>,
        bindings: Vec<&'r (Option<Name>, Binding)>,
    ) -> Level<'r> {
        Level {
            registry: self.registry,
            class: self.class,
            member: self.member,
            facts,
            params,
            made,
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
    member: &'r Name,
    facts: &'r WriteFacts,
    // The parameters whose declared type holds all through the scope:
    // neither assigned nor passed where a parameter is by reference. A
    // closure's are its own and those it takes of the code that wrote it.
    params: FxHashMap<&'r str, Declared<'r>>,
    // The variables that only ever hold what the body made.
    made: FxHashMap<&'r str, Made>,
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
                    calls.extend(
                        runs.into_iter()
                            .map(|(key, transfer)| (key, transfer, None)),
                    );
                    continue;
                }
                let at = passes_arguments(&call.callee, &key).then(|| {
                    *site.get_or_insert_with(|| {
                        let passed = self.passed(holder, &reach, call);
                        sites.push(Site { call, passed });
                        sites.len() as u32 - 1
                    })
                });
                calls.push((key, transfer, at));
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
    /// class; a parameter, by its declared type; anything else not known.
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
        match self.params.get(name.as_str()) {
            Some(Declared::Param { ty, .. }) => holder_of(ty),
            Some(Declared::Classes(Some(classes))) => {
                let mut found = Vec::new();
                for class in *classes {
                    match named_classes(self.registry, self.class, class) {
                        Some(named) => found.extend(named),
                        None => return Holder::Unknown,
                    }
                }
                Holder::Classes(found)
            }
            Some(Declared::Classes(None)) | None => Holder::Unknown,
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
