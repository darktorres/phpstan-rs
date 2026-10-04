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

use std::collections::BTreeSet;
use std::sync::Arc;

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::{ClassKind, QualifiedName, ResolvedName};
use syntax::name::Name;

use crate::registry::bodies::BodySource;
use crate::registry::summaries::{
    ArgAt, Binding, CallWrites, Callable, Callee, Made, PairTarget, Place, Receiver, Root, Step,
    WriteFacts,
};
use crate::registry::{ClassReflection, FunctionReflection, Registry, is_non_overridable};
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

    /// Whether it may write a property of the callee's own `$this`.
    pub(crate) fn writes_own_object(&self) -> bool {
        self.0.as_ref().is_none_or(|storages| {
            storages
                .iter()
                .any(|storage| matches!(storage, Storage::Own(_)))
        })
    }

    /// Whether it may write a property of an object other than the
    /// callee's own `$this`.
    pub(crate) fn writes_other_objects(&self) -> bool {
        self.0.as_ref().is_none_or(|storages| {
            storages
                .iter()
                .any(|storage| matches!(storage, Storage::Property { .. }))
        })
    }

    /// This effect, written by a call whose callee's own `$this` is the
    /// object `transfer` says.
    fn transferred(&self, transfer: &Transfer) -> Effect {
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Transfer {
    /// The caller's own `$this`.
    Same,
    /// Another object, which may be of the class; `None` for any.
    Other(Option<Name>),
    /// An object the caller made, which no one else has seen.
    Made,
    /// No object: a function, or a static method.
    Static,
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
struct DeclRef<'a> {
    class: Option<&'a str>,
    member: &'a str,
}

/// The two ways of naming a declaration, which hash and compare alike.
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

/// What each builtin runs, as `tools/builtin-runs/record.php` found it.
struct BuiltinTable {
    /// The builtins a call reached: functions and `class::method`,
    /// lowercased.
    probed: FxHashSet<&'static str>,
    runs: FxHashMap<&'static str, Vec<(Position, BuiltinRun)>>,
    /// The classes (lowercased) a method of which ran a callable or an
    /// object's method its object was made with.
    holding: FxHashSet<&'static str>,
    /// The stream-wrapper methods (lowercased) a method of each class
    /// (lowercased) ran through a URL its object was made with.
    held_wrappers: FxHashMap<&'static str, Vec<&'static str>>,
}

fn builtin_table() -> &'static BuiltinTable {
    static TABLE: std::sync::LazyLock<BuiltinTable> =
        std::sync::LazyLock::new(|| BuiltinTable::parse(include_str!("data/builtin-runs-8.5.tsv")));
    &TABLE
}

impl BuiltinTable {
    fn parse(text: &'static str) -> BuiltinTable {
        let mut table = BuiltinTable {
            probed: FxHashSet::default(),
            runs: FxHashMap::default(),
            holding: FxHashSet::default(),
            held_wrappers: FxHashMap::default(),
        };
        for line in text.lines() {
            let mut fields = line.split('\t');
            let (Some(builtin), Some(place), Some(kind)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let detail = fields.next().unwrap_or("");
            if kind == "probed" {
                table.probed.insert(builtin);
                continue;
            }
            if place.starts_with("new:") {
                if let Some((class, _)) = builtin.split_once("::") {
                    if kind == "wrapper" {
                        let held = table.held_wrappers.entry(class).or_default();
                        if !held.contains(&detail) {
                            held.push(detail);
                        }
                    } else {
                        table.holding.insert(class);
                    }
                }
                continue;
            }
            let (at, elements) = match place.strip_suffix("[]") {
                Some(at) => (at, true),
                None => (place, false),
            };
            let Ok(at) = at.parse() else {
                continue;
            };
            let run = match kind {
                "calls" => BuiltinRun::Calls,
                "method" => BuiltinRun::Method(detail),
                "wrapper" => BuiltinRun::Wrapper(detail),
                _ => continue,
            };
            table
                .runs
                .entry(builtin)
                .or_default()
                .push((Position { at, elements }, run));
        }
        table
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
#[derive(Debug, Default, Clone)]
struct Classification {
    own: Effect,
    calls: Vec<(DeclKey, Transfer)>,
}

impl Registry {
    /// Settles what every function and method with a body may write. Run with
    /// `resolve_transitive_purity`, after every class is indexed and
    /// flattened. Every declaration is resolved again on each run: what a
    /// call reaches depends on the whole hierarchy, which any file may
    /// change.
    pub(crate) fn resolve_effects(&mut self, bodies: &dyn BodySource) {
        self.methods_by_name = Arc::new(methods_by_name(self));
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
        let mut classified: FxHashMap<DeclKey, Classification> = functions
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .map(|(name, sig)| {
                let key = function_key(name);
                let resolved = registry.classify(bodies, None, name, sig);
                (key, resolved)
            })
            .collect();
        classified.par_extend(
            methods
                .par_iter()
                .with_min_len(crate::registry::PASS_CHUNK)
                .map(|(class, method, sig)| {
                    let key = method_key(class, method);
                    let resolved = registry.classify(bodies, Some(class), method, sig);
                    (key, resolved)
                }),
        );
        let mut effects: FxHashMap<DeclKey, Effect> = classified
            .iter()
            .map(|(key, classification)| (key.clone(), classification.own.clone()))
            .collect();
        // An effect only ever grows, and grows only when one it is made of
        // does: every declaration is joined once, and again only when a
        // declaration it calls has grown.
        let mut callers: FxHashMap<&DeclKey, Vec<&DeclKey>> = FxHashMap::default();
        for (key, classification) in &classified {
            for (callee, _) in &classification.calls {
                if classified.contains_key(callee) {
                    callers.entry(callee).or_default().push(key);
                }
            }
        }
        let mut queue: std::collections::VecDeque<&DeclKey> = classified.keys().collect();
        let mut queued: FxHashSet<&DeclKey> = queue.iter().copied().collect();
        while let Some(key) = queue.pop_front() {
            queued.remove(key);
            let classification = &classified[key];
            let mut effect = effects[key].clone();
            if effect.is_anything() {
                continue;
            }
            let mut grew = false;
            for (callee, transfer) in &classification.calls {
                let callee_effect = match effects.get(callee) {
                    Some(effect) => effect.clone(),
                    None => self.declaration_effect(callee),
                };
                grew |= effect.join(&callee_effect.transferred(transfer));
                if effect.is_anything() {
                    break;
                }
            }
            if grew {
                effects.insert(key.clone(), effect);
                for caller in callers.get(key).into_iter().flatten() {
                    if queued.insert(caller) {
                        queue.push_back(caller);
                    }
                }
            }
        }
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
    fn settled_effect(&self, key: &DeclKey) -> Effect {
        let query = DeclRef {
            class: key.class.as_deref(),
            member: &key.member,
        };
        match self.effects.get(&query as &dyn DeclQuery) {
            Some(effect) => effect.clone(),
            None => self.declaration_effect(key),
        }
    }

    /// What calling the function `name` may write.
    pub(crate) fn function_effect(&self, name: &str) -> Effect {
        let declared = self
            .lookup_function_with_name(name)
            .map_or(name, |(declared, _)| declared);
        self.settled_effect(&function_key(declared))
    }

    /// What calling `owner`'s `method` - the declaration a call reaches, not
    /// an override of it - may write.
    pub(crate) fn method_effect(&self, owner: &str, method: &str) -> Effect {
        self.settled_effect(&method_key(owner, method))
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
        let table = builtin_table();
        let name = key.builtin_name();
        if table.probed.contains(name.as_str()) {
            return Some(name);
        }
        let class = key.class.as_deref()?;
        self.all_ancestor_names(class).iter().find_map(|ancestor| {
            let name = format!("{}::{}", ancestor.to_ascii_lowercase(), key.member);
            table.probed.contains(name.as_str()).then_some(name)
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
        let table = builtin_table();
        self.builtin_lineage(class)
            .iter()
            .any(|class| table.holding.contains(class.as_str()))
    }

    /// The stream-wrapper methods an object of the builtin class `class` may
    /// run through a URL it was made with.
    fn builtin_held_wrappers(&self, class: &str) -> Vec<&'static str> {
        let table = builtin_table();
        let mut out = Vec::new();
        for class in self.builtin_lineage(class) {
            for method in table
                .held_wrappers
                .get(class.as_str())
                .into_iter()
                .flatten()
            {
                if !out.contains(method) {
                    out.push(*method);
                }
            }
        }
        out
    }

    /// What a call of the builtin `key` (`sig`) whose arguments are not
    /// known may write: nothing when a call of it was seen to run no code
    /// through any argument, its object holds none, and none of its
    /// parameters takes something through which it runs code
    /// (`CODE_RUNNING_PARAMETER_TYPES`); anything otherwise.
    fn stub_effect(&self, key: &DeclKey, sig: &FunctionReflection) -> Effect {
        let runs_code = *sig.stub_effect.0.get_or_init(|| {
            let table = builtin_table();
            let Some(name) = self.builtin_entry(key) else {
                return true;
            };
            key.class.as_deref().is_some_and(|class| {
                self.builtin_holds_code(class) || !self.builtin_held_wrappers(class).is_empty()
            }) || table.runs.contains_key(name.as_str())
                || sig
                    .params
                    .iter()
                    .any(|p| p.ty.mentions_any(&CODE_RUNNING_PARAMETER_TYPES))
        });
        if runs_code {
            Effect::anything()
        } else {
            Effect::nothing()
        }
    }

    /// The builtin `key` is, when it is one.
    fn builtin(&self, key: &DeclKey) -> Option<&FunctionReflection> {
        let sig = match &key.class {
            Some(class) => self.lookup_method(class, &key.member),
            None => self.functions.get(key.member.as_str()),
        }?;
        sig.is_stub.then_some(sig)
    }

    /// The methods `class` has with a body that override one of the builtin
    /// class `builtin`'s: what a method of the builtin may run on its own
    /// object.
    fn overrides_of_builtin(&self, class: &str, builtin: &str) -> Vec<DeclKey> {
        let Some(info) = self.classes.get(builtin) else {
            return Vec::new();
        };
        info.methods
            .iter()
            .filter_map(|(method, _)| {
                let (owner, sig) = self.lookup_method_with_owner(class, method)?;
                (!sig.is_stub).then(|| method_key(&owner, method))
            })
            .collect()
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
    fn builtin_methods_running_code(&self) -> &FxHashSet<String> {
        self.builtin_methods_running_code.get_or_init(|| {
            let mut names = FxHashSet::default();
            for (class, info) in self.classes.iter() {
                for (method, sig) in info.methods.iter() {
                    let Some((owner, _)) = self.lookup_method_with_owner(class, method) else {
                        continue;
                    };
                    let key = method_key(&owner, method);
                    if sig.is_stub && self.stub_effect(&key, sig).is_anything() {
                        names.insert(method.to_ascii_lowercase());
                    }
                }
            }
            names
        })
    }

    /// The classification of the body of `sig`, the declaration `member` of
    /// `class` (a function when `class` is `None`).
    fn classify(
        &self,
        bodies: &dyn BodySource,
        class: Option<&Name>,
        member: &Name,
        sig: &FunctionReflection,
    ) -> Classification {
        let Some(summary) = bodies.summary(&sig.provenance) else {
            return Classification {
                own: Effect::anything(),
                calls: Vec::new(),
            };
        };
        let mut resolve = Resolve {
            registry: self,
            class: class.map(Name::as_str),
            member,
            facts: &summary.writes,
            params: FxHashMap::default(),
            made: FxHashMap::default(),
            classification: Classification::default(),
        };
        resolve.run(sig);
        resolve.classification
    }
}

/// The classes `ty` may be an object of: `Some` with each one named, `None`
/// when it may be an object of a class not named (`object`, `mixed`).
fn object_classes(ty: &Type) -> Option<Vec<Name>> {
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
enum Holder {
    /// An object of one of the classes, or of a descendant of one.
    Classes(Vec<Name>),
    /// An object the body made, no one else has: of exactly one of the
    /// classes, or of a class not known.
    Made(Option<Vec<Name>>),
    /// An object of any class.
    Unknown,
}

/// The holder of a value of type `ty`.
fn holder_of(ty: &Type) -> Holder {
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

/// Looks a body's writes and calls up in the registry.
struct Resolve<'r> {
    registry: &'r Registry,
    class: Option<&'r str>,
    member: &'r Name,
    facts: &'r WriteFacts,
    // The parameters whose declared type holds all through the body: neither
    // assigned nor passed where a parameter is by reference.
    params: FxHashMap<&'r str, &'r Type>,
    // The variables that only ever hold what the body made.
    made: FxHashMap<&'r str, &'r Made>,
    classification: Classification,
}

impl<'r> Resolve<'r> {
    fn run(&mut self, sig: &'r FunctionReflection) {
        if self.facts.anything {
            self.classification.own = Effect::anything();
            return;
        }
        self.params = sig
            .params
            .iter()
            .filter(|p| {
                !p.by_ref
                    && !self
                        .facts
                        .reassigned
                        .iter()
                        .any(|name| name == p.name.as_ref())
            })
            .map(|p| (p.name.as_ref(), &p.ty))
            .collect();
        self.made = self
            .facts
            .made
            .iter()
            .map(|(name, made)| (name.as_str(), made))
            .collect();
        // What each call reaches depends on the variables' types, and which
        // variables a call writes by reference on what it reaches: both are
        // found again until no variable is lost.
        let (reached, written_args) = loop {
            let mut reached: Vec<Vec<(DeclKey, Transfer)>> = Vec::new();
            let mut written_args: Vec<&'r Place> = Vec::new();
            for call in &self.facts.calls {
                let Some(targets) = self.call_targets(&call.callee) else {
                    self.classification.own = Effect::anything();
                    return;
                };
                // A closure the body wrote, called: what it takes by
                // reference is not looked up.
                let unknown_params = matches!(call.callee, Callee::Value(_));
                for (at, place) in &call.places {
                    if unknown_params || targets.iter().any(|(key, _)| self.by_ref_at(key, at)) {
                        written_args.push(place);
                    }
                }
                reached.push(targets);
            }
            let mut lost = false;
            for place in &written_args {
                if let Some(name) = place_variable(place) {
                    lost |= self.params.remove(name.as_str()).is_some();
                    lost |= self.made.remove(name.as_str()).is_some();
                }
            }
            if !lost {
                break (reached, written_args);
            }
        };
        // A builtin writes what the code it runs writes: what is passed to
        // it, and what of its object a class extending it declares.
        let mut calls: Vec<(DeclKey, Transfer)> = Vec::new();
        for (call, targets) in self.facts.calls.iter().zip(reached) {
            for (key, transfer) in targets {
                match self.registry.builtin(&key) {
                    Some(sig) => match self.builtin_runs(&key, sig, &transfer, call) {
                        Some(runs) => calls.extend(runs),
                        None => {
                            self.classification.own = Effect::anything();
                            return;
                        }
                    },
                    None => calls.push((key, transfer)),
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
        self.classification.own = own;
        self.classification.calls = calls;
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
        let table = builtin_table();
        let name = registry.builtin_entry(key)?;
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
                Transfer::Same => self.class.map(Name::from).into_iter().collect(),
                Transfer::Other(Some(class)) => vec![class.clone()],
                Transfer::Other(None) | Transfer::Made | Transfer::Static => {
                    vec![Name::from(owner.as_str())]
                }
            };
            for receiver in &receivers {
                for candidate in
                    std::iter::once(receiver.clone()).chain(registry.subclasses(receiver))
                {
                    out.extend(
                        registry
                            .overrides_of_builtin(&candidate, owner)
                            .into_iter()
                            .map(|key| (key, transfer.clone())),
                    );
                }
            }
        }
        let runs = table.runs.get(name.as_str()).map_or(&[][..], Vec::as_slice);
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
            let callable = call
                .callables
                .iter()
                .find(|(arg, _)| passed(arg))
                .map(|(_, callable)| callable);
            match callable {
                Some(callable) => out.extend(self.callable_targets(callable)?),
                // A closure the body wrote, held in a variable.
                None => match call.places.iter().find(|(arg, _)| passed(arg)) {
                    Some((_, place))
                        if place_variable(place)
                            .and_then(|name| self.made.get(name.as_str()))
                            .is_some_and(|made| matches!(made, Made::Closure)) => {}
                    _ => return None,
                },
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
                    let holder = match call.places.iter().find(|(arg, _)| arg_at(arg)) {
                        Some((_, place)) if !position.elements => {
                            self.value_holder(&place.root, &place.steps)
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
            Callable::Written => Some(Vec::new()),
            Callable::Text(text) => match text.split_once("::") {
                Some((class, method)) => {
                    let class = QualifiedName {
                        written: Name::from(class.trim_start_matches('\\')),
                        resolved: ResolvedName::Exact(Name::from(class.trim_start_matches('\\'))),
                        span: Default::default(),
                    };
                    self.static_targets(&class, Some(&Name::from(method)))
                }
                None => {
                    let name = text.trim_start_matches('\\');
                    Some(match registry.lookup_function_with_name(name) {
                        Some((declared, _)) => vec![(function_key(declared), Transfer::Static)],
                        None => Vec::new(),
                    })
                }
            },
            Callable::Pair(PairTarget::Class(class), method) => {
                self.static_targets(class, Some(method))
            }
            Callable::Pair(PairTarget::Object(receiver), method) => {
                self.call_targets(&Callee::Method {
                    receiver: receiver.clone(),
                    method: Some(method.clone()),
                })
            }
        }
    }

    /// Whether the declaration `key` takes the argument passed `at` by
    /// reference.
    fn by_ref_at(&self, key: &DeclKey, at: &ArgAt) -> bool {
        let sig = match &key.class {
            Some(class) => self.registry.lookup_method(class, &key.member),
            None => self.registry.functions.get(key.member.as_str()),
        };
        let Some(sig) = sig else {
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

    /// The declarations a call may run, each with what its callee's own
    /// `$this` is to the caller; `None` when it may run code not known.
    fn call_targets(&self, callee: &Callee) -> Option<Vec<(DeclKey, Transfer)>> {
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
            Callee::Method { receiver, method } => {
                let (holder, same) = self.receiver(receiver);
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
                    Some(class) => self.instantiated(class),
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
            Callee::Value(variable) => match variable
                .as_ref()
                .and_then(|name| self.made.get(name.as_str()))
            {
                // A closure the body wrote: its statements are the body's.
                Some(Made::Closure) => Vec::new(),
                _ => return None,
            },
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
        let classes = self.named_classes(class)?;
        let forwards_from = |target: &str| {
            self.class.is_some_and(|own| {
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

    /// The classes `class` as written names from the body: the body's own
    /// for `self`, its parent for `parent`, the body's own and every
    /// descendant for `static`, the named one otherwise. `None` when the
    /// body's class is not known, or has no parent; empty for a name that
    /// is no class.
    fn named_classes(&self, class: &QualifiedName) -> Option<Vec<Name>> {
        let registry = self.registry;
        Some(if class.is_keyword("self") {
            vec![Name::from(self.class?)]
        } else if class.is_keyword("parent") {
            vec![
                registry
                    .classes
                    .get(self.class?)?
                    .extends()
                    .first()?
                    .clone(),
            ]
        } else if class.is_keyword("static") {
            let own = self.class?;
            std::iter::once(Name::from(own))
                .chain(registry.subclasses(own))
                .collect()
        } else {
            class.resolved_class().map(Name::from).into_iter().collect()
        })
    }

    /// The classes `new class` may make an object of.
    fn instantiated(&self, class: &QualifiedName) -> Vec<Name> {
        self.named_classes(class).unwrap_or_default()
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

    /// The object the variable `name` holds: what the body made, by its
    /// class; a parameter, by its declared type; anything else not known.
    fn variable_holder(&self, name: &Name) -> Holder {
        if let Some(made) = self.made.get(name.as_str()) {
            return match made {
                Made::Object(Some(class)) => Holder::Made(Some(self.instantiated(class))),
                Made::Object(None) => Holder::Made(None),
                Made::Closure => Holder::Made(Some(vec![Name::from("Closure")])),
            };
        }
        match self.params.get(name.as_str()) {
            Some(ty) => holder_of(ty),
            None => Holder::Unknown,
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
        let Some(classes) = self.named_classes(class) else {
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
        for (bound, binding) in self.facts.bindings.iter() {
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
