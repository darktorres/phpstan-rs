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

use crate::registry::TableKey;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::{SmallVec, smallvec};

use super::effect_table::{ByRefParams, EffectIx, TransferIx, TransferTable};
use super::reach_memo::{ReturnedKey, SharedTargets, StaticTarget};
use syntax::ast::{ClassKind, QualifiedName, ResolvedName, Visibility};
use syntax::name::Name;
use syntax::stats::{self, Counter};

use crate::file_id::FileId;
use crate::registry::bodies::BodySource;
use crate::registry::summaries::{
    Access, ArgAt, Binding, BodySummary, CallWrites, Callable, Callee, ClosureFacts, Made,
    Narrowing, Operation, Origin, PairTarget, Place, Receiver, Root, Step, WriteFacts,
};
use crate::registry::{
    FunctionReflection, NameMap, PropertyReflection, Registry, is_non_overridable,
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
    /// A variable a closure captures by reference, shared with the scope
    /// that wrote the closure: the closure by the id of its expression in
    /// that scope's file, and the variable.
    Captured { closure: u32, var: Name },
    /// Whatever code of the loaded program a call the analysis cannot
    /// resolve may run.
    Unknown,
}

/// What code may write: a set of storages, or anything.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Effect(Option<Arc<BTreeSet<Storage>>>);

impl Default for Effect {
    fn default() -> Effect {
        Effect::nothing()
    }
}

impl Effect {
    pub(crate) fn nothing() -> Effect {
        Effect(Some(Arc::default()))
    }

    pub(crate) fn anything() -> Effect {
        Effect(None)
    }

    /// What a call may write that runs code the analysis cannot resolve: any
    /// code of the loaded program.
    pub(crate) fn unknown() -> Effect {
        Effect(Some(Arc::new(BTreeSet::from([Storage::Unknown]))))
    }

    /// Whether it may write anything the code of the loaded program may, or
    /// anything at all.
    pub(crate) fn is_anything(&self) -> bool {
        self.0
            .as_ref()
            .is_none_or(|storages| storages.contains(&Storage::Unknown))
    }

    /// Whether it may write anything at all: it runs code that is not in the
    /// loaded program (`eval`, an include of a file not known).
    pub(crate) fn is_arbitrary(&self) -> bool {
        self.0.is_none()
    }

    /// What running the closure `closure` writes of the variables it
    /// captures by reference.
    pub(crate) fn captured(closure: u32, vars: impl IntoIterator<Item = Name>) -> Effect {
        Effect(Some(Arc::new(
            vars.into_iter()
                .map(|var| Storage::Captured { closure, var })
                .collect(),
        )))
    }

    pub(crate) fn is_nothing(&self) -> bool {
        self.0.as_ref().is_some_and(|storages| storages.is_empty())
    }

    /// The storages written, when they are known.
    pub(crate) fn storages(&self) -> Option<&BTreeSet<Storage>> {
        self.0.as_deref()
    }

    /// This effect of a method, called on an object that may be of `class`
    /// (of any class, when `None`): its own object's properties are that
    /// object's.
    pub(crate) fn on_object(&self, class: Option<&Name>) -> Effect {
        self.transferred(&Transfer::Other(class.cloned()))
    }

    fn insert(&mut self, storage: Storage) {
        if let Some(storages) = &mut self.0 {
            Arc::make_mut(storages).insert(storage);
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
                if Arc::ptr_eq(storages, added) || added.iter().all(|s| storages.contains(s)) {
                    return false;
                }
                Arc::make_mut(storages).extend(added.iter().cloned());
                true
            }
        }
    }

    /// Adds what `other` writes when called with `transfer`, as
    /// `self.join(&other.transferred(transfer))` does, without building the
    /// transferred effect.
    pub(crate) fn join_transferred(&mut self, other: &Effect, transfer: &Transfer) -> bool {
        let Some(added) = &other.0 else {
            return self.join(&Effect::anything());
        };
        let Some(storages) = &mut self.0 else {
            return false;
        };
        let mut changed = false;
        for storage in added.iter() {
            let produced = match (storage, transfer) {
                (Storage::Own(prop), Transfer::Other(class)) => Cow::Owned(Storage::Property {
                    class: class.clone(),
                    prop: prop.clone(),
                }),
                (Storage::Own(_), Transfer::Made) => continue,
                (Storage::Own(prop), Transfer::Static) => Cow::Owned(Storage::Property {
                    class: None,
                    prop: prop.clone(),
                }),
                (storage, _) => Cow::Borrowed(storage),
            };
            if !storages.contains(&*produced) {
                Arc::make_mut(storages).insert(produced.into_owned());
                changed = true;
            }
        }
        changed
    }

    /// This effect, written by a call whose callee's own `$this` is the
    /// object `transfer` says.
    pub(crate) fn transferred(&self, transfer: &Transfer) -> Effect {
        let Some(storages) = &self.0 else {
            return Effect::anything();
        };
        if matches!(transfer, Transfer::Same)
            || !storages.iter().any(|s| matches!(s, Storage::Own(_)))
        {
            return self.clone();
        }
        let mut out = BTreeSet::new();
        for storage in storages.iter() {
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
        Effect(Some(Arc::new(out)))
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
    /// The argument is not called but destroyed, the declaration writing
    /// over the by-reference parameter it is passed at.
    destroys: bool,
}

impl Condition {
    /// This condition of a callee whose own `$this` is what `transfer` says,
    /// as its caller has it.
    fn through(&self, transfer: &Transfer) -> Condition {
        Condition {
            at: self.at,
            this: self.this.as_ref().map(|this| this.through(transfer)),
            destroys: self.destroys,
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
            Effect::unknown()
        }
    }
}

/// A declaration, keyed case-insensitively: its class and method for a
/// method, its name alone for a function. The names are held lowercased, and
/// a key with them in any case finds the declaration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(from = "DeclKeyNames", into = "DeclKeyNames")]
pub(crate) struct DeclKey {
    pub(in crate::registry) class: Option<Name>,
    pub(in crate::registry) member: Name,
    /// `decl_hash` of the names, made when the key was.
    hash: u64,
}

/// What a `DeclKey` is stored as.
#[derive(serde::Serialize, serde::Deserialize)]
struct DeclKeyNames {
    class: Option<Arc<str>>,
    member: Arc<str>,
}

impl From<DeclKeyNames> for DeclKey {
    fn from(names: DeclKeyNames) -> DeclKey {
        DeclKey::new(
            names.class.map(|class| Name::from(&*class)),
            Name::from(&*names.member),
        )
    }
}

impl From<DeclKey> for DeclKeyNames {
    fn from(key: DeclKey) -> DeclKeyNames {
        DeclKeyNames {
            class: key.class.map(|class| Arc::from(class.as_str())),
            member: Arc::from(key.member.as_str()),
        }
    }
}

/// The hash a declaration's names file it under, the same in any case.
fn decl_hash(class: &Option<Name>, member: &Name) -> u64 {
    mix_decl_hash(class.as_ref().map(Name::folded_hash), member.folded_hash())
}

/// `decl_hash` from the folded hashes of the names.
fn mix_decl_hash(class: Option<u64>, member: u64) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let class = class.map_or(0, |hash| hash | 1);
    (class.rotate_left(5) ^ member).wrapping_mul(K)
}

/// `decl_hash` of names given as text.
fn decl_text_hash(class: Option<&str>, member: &str) -> u64 {
    mix_decl_hash(
        class.map(|class| syntax::name::folded_hash(class.as_bytes())),
        syntax::name::folded_hash(member.as_bytes()),
    )
}

/// The kind of synthetic property hook represented by a property slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PropertyHookKind {
    Get,
    Set,
}

/// A settled body effect addressed by its declaration, without hashing the
/// declaration's class or member name on every lookup. Property hooks use
/// the class's exact-case `PropertyRef` topology.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum EffectId {
    Function(crate::registry::FnId),
    Method(crate::registry::MethodRef),
    PropertyHook {
        property: crate::registry::PropertyRef,
        kind: PropertyHookKind,
    },
    Closure {
        caller: Box<EffectId>,
        ordinal: u32,
    },
    /// The top level of a file: what including it may write.
    File(FileId),
    Named(DeclKey),
}

impl EffectId {
    pub(super) fn is_closure(&self) -> bool {
        matches!(self, EffectId::Closure { .. })
    }

    pub(crate) fn with_generation(self, generation: u32) -> EffectId {
        match self {
            EffectId::Function(id) => EffectId::Function(id.with_generation(generation)),
            EffectId::Method(method) => EffectId::Method(crate::registry::MethodRef::new(
                method.class().with_generation(generation),
                method.slot(),
            )),
            EffectId::PropertyHook { property, kind } => EffectId::PropertyHook {
                property: property.with_generation(generation),
                kind,
            },
            EffectId::Closure { caller, ordinal } => EffectId::Closure {
                caller: Box::new(caller.with_generation(generation)),
                ordinal,
            },
            EffectId::File(file) => EffectId::File(file),
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
        state.write_u64(decl_text_hash(self.class(), self.member()));
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
        state.write_u64(self.hash);
    }
}

impl PartialEq for DeclKey {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.member == other.member && self.class == other.class
    }
}

impl Eq for DeclKey {}

impl<'a> std::borrow::Borrow<dyn DeclQuery + 'a> for DeclKey {
    fn borrow(&self) -> &(dyn DeclQuery + 'a) {
        self
    }
}

/// The class of the keys that name the top level of a file: a name no class
/// has.
const FILE_CLASS: &str = "\0file";

impl DeclKey {
    /// The key of the declaration of `member`, in `class` for a method; the
    /// names are taken in lowercase.
    pub(in crate::registry) fn new(class: Option<Name>, member: Name) -> DeclKey {
        let class = class.map(|class| class.lowered().clone());
        let member = member.lowered().clone();
        let hash = decl_hash(&class, &member);
        DeclKey {
            class,
            member,
            hash,
        }
    }

    /// The key of the top level of `file`.
    fn file(file: FileId) -> DeclKey {
        DeclKey::new(
            Some(Name::from(FILE_CLASS)),
            Name::from(file.index().to_string()),
        )
    }

    /// The key of the top level of a file that is not known: code of
    /// arbitrary content.
    fn unresolved_include() -> DeclKey {
        DeclKey::new(Some(Name::from(FILE_CLASS)), Name::from("unresolved"))
    }

    /// The file whose top level the key names, when it names one.
    fn file_id(&self) -> Option<FileId> {
        if self.class.as_deref() != Some(FILE_CLASS) {
            return None;
        }
        self.member.parse().ok().map(FileId::new)
    }

    /// The key of the closure, arrow function or first-class callable this
    /// declaration's body lists at `index`: a name no declaration has.
    fn closure(&self, index: u32) -> DeclKey {
        DeclKey::new(
            self.class.clone(),
            Name::from(format!("{}{{closure#{index}}}", self.member)),
        )
    }

    /// The name the builtin table knows the declaration by: a function's,
    /// or `class::method`, lowercased.
    fn builtin_name(&self) -> String {
        match &self.class {
            Some(class) => format!("{class}::{}", self.member),
            None => self.member.to_string(),
        }
    }
}

/// Splits a synthetic closure name into the caller declaration and the
/// body's ordinal within it. Closure names are created only by `closure`.
fn closure_parent(key: &DeclKey) -> Option<(DeclKey, u32)> {
    if !key.member.ends_with('}') {
        return None;
    }
    let start = key.member.rfind("{closure#")?;
    let ordinal = key.member[start + 9..].strip_suffix('}')?.parse().ok()?;
    Some((
        DeclKey::new(key.class.clone(), Name::from(&key.member[..start])),
        ordinal,
    ))
}

pub(crate) fn function_key(name: &(impl KeyName + ?Sized)) -> DeclKey {
    DeclKey::new(None, name.lowered_name())
}

pub(crate) fn method_key(
    class: &(impl KeyName + ?Sized),
    method: &(impl KeyName + ?Sized),
) -> DeclKey {
    DeclKey::new(Some(class.lowered_name()), method.lowered_name())
}

/// A name that makes the lowercase `Name` a key holds: a `Name` has it
/// already, text is lowercased and interned.
pub(crate) trait KeyName {
    fn lowered_name(&self) -> Name;
    fn text(&self) -> &str;
}

impl KeyName for Name {
    #[inline]
    fn lowered_name(&self) -> Name {
        self.lowered().clone()
    }

    #[inline]
    fn text(&self) -> &str {
        self.as_str()
    }
}

impl KeyName for str {
    fn text(&self) -> &str {
        self
    }

    fn lowered_name(&self) -> Name {
        const STACK: usize = 128;
        if !self.bytes().any(|byte| byte.is_ascii_uppercase()) {
            Name::from(self)
        } else if self.len() <= STACK {
            let mut stack = [0u8; STACK];
            let bytes = &mut stack[..self.len()];
            bytes.copy_from_slice(self.as_bytes());
            bytes.make_ascii_lowercase();
            Name::from(std::str::from_utf8(bytes).expect("lowercasing ASCII keeps UTF-8"))
        } else {
            Name::from(self.to_ascii_lowercase())
        }
    }
}

impl KeyName for String {
    fn lowered_name(&self) -> Name {
        self.as_str().lowered_name()
    }

    fn text(&self) -> &str {
        self
    }
}

impl<T: KeyName + ?Sized> KeyName for &T {
    fn lowered_name(&self) -> Name {
        (**self).lowered_name()
    }

    fn text(&self) -> &str {
        (**self).text()
    }
}

/// The names of the methods the analysis asks for by name, interned once.
pub(crate) struct WellKnownNames {
    pub(crate) call: Name,
    pub(crate) call_static: Name,
    pub(crate) construct: Name,
    pub(crate) destruct: Name,
    pub(crate) clone: Name,
    pub(crate) to_string: Name,
    pub(crate) get: Name,
    pub(crate) set: Name,
    pub(crate) isset: Name,
    pub(crate) unset: Name,
    pub(crate) offset_get: Name,
    pub(crate) offset_set: Name,
    pub(crate) offset_exists: Name,
    pub(crate) offset_unset: Name,
    pub(crate) get_iterator: Name,
    pub(crate) rewind: Name,
    pub(crate) valid: Name,
    pub(crate) current: Name,
    pub(crate) next: Name,
    pub(crate) key: Name,
}

pub(crate) fn well_known() -> &'static WellKnownNames {
    static NAMES: std::sync::LazyLock<WellKnownNames> = std::sync::LazyLock::new(|| {
        // Each name's lowercase form is made at once, so a key built from it
        // finds it.
        let name = |text: &str| {
            let name = Name::from(text);
            name.lowered();
            name
        };
        WellKnownNames {
            call: name("__call"),
            call_static: name("__callStatic"),
            construct: name("__construct"),
            destruct: name("__destruct"),
            clone: name("__clone"),
            to_string: name("__toString"),
            get: name("__get"),
            set: name("__set"),
            isset: name("__isset"),
            unset: name("__unset"),
            offset_get: name("offsetGet"),
            offset_set: name("offsetSet"),
            offset_exists: name("offsetExists"),
            offset_unset: name("offsetUnset"),
            get_iterator: name("getIterator"),
            rewind: name("rewind"),
            valid: name("valid"),
            current: name("current"),
            next: name("next"),
            key: name("key"),
        }
    });
    &NAMES
}

/// The name a hook of the property `property`, its `set` hook when `set`,
/// is known by among methods: PHP's (`$property::get`), lowercased, with a
/// `^` before each letter that was a capital, so that two properties whose
/// names differ only in case keep apart. `None` for any property's, which
/// only the index of names (`methods_by_name`) lists.
fn hook_member(property: Option<&str>, set: bool) -> String {
    let property = property.unwrap_or_default();
    let mut member = String::with_capacity(property.len() + 8);
    member.push('$');
    for c in property.chars() {
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
fn hook_key(class: &(impl KeyName + ?Sized), property: &str, set: bool) -> DeclKey {
    DeclKey::new(
        Some(class.lowered_name()),
        Name::from(hook_member(Some(property), set)),
    )
}

/// Whether code of the class `scope` sees a member `owner` declares with
/// `visibility`.
/// A property of a class with a hook body, as its hook's callers need it.
#[derive(Debug, Clone)]
pub(crate) struct HookDeclaration {
    name: Name,
    /// Who may reach the hook.
    visibility: Visibility,
    /// Whether the property is private.
    private: bool,
}

impl Registry {
    /// The properties of `class` named `property` (all when none) that have
    /// a hook body for `set`, in the order the class declares them.
    fn hook_declarations(
        &self,
        class: &Name,
        property: Option<&Name>,
        set: bool,
    ) -> Vec<HookDeclaration> {
        let declared: Vec<(&Name, &PropertyReflection)> = match property {
            Some(property) => self
                .lookup_property(class, property)
                .map(|declared| (property, declared))
                .into_iter()
                .collect(),
            None => self
                .property_names(class)
                .filter_map(|name| {
                    self.lookup_property(class, name)
                        .map(|declared| (name, declared))
                })
                .collect(),
        };
        declared
            .into_iter()
            .filter(|(_, declared)| {
                if set {
                    declared.set_hook_body
                } else {
                    declared.get_hook_body
                }
            })
            .map(|(name, declared)| HookDeclaration {
                name: name.clone(),
                visibility: if set {
                    declared.write_visibility()
                } else {
                    declared.visibility()
                },
                private: declared.visibility() == Visibility::Private,
            })
            .collect()
    }
}

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
pub(super) struct Position {
    pub(super) at: u32,
    pub(super) elements: bool,
}

/// What a call of a builtin runs whatever its arguments are.
pub(super) struct BuiltinBase {
    runs: Arc<[(Position, BuiltinRun)]>,
    prefix: Arc<[Run]>,
    /// Per parameter: whether a callable passed there is run.
    takes_callable: Arc<[bool]>,
}

/// What a call of a builtin runs: what it runs whatever its arguments are,
/// shared by the calls that ask the same, and what its arguments add.
pub(super) struct BuiltinRuns {
    prefix: Arc<[Run]>,
    added: RunList,
}

impl BuiltinRuns {
    fn of(added: RunList) -> Self {
        BuiltinRuns {
            prefix: Arc::from([]),
            added,
        }
    }

    fn iter(&self) -> impl Iterator<Item = Run> + '_ {
        self.prefix.iter().chain(&self.added).copied()
    }
}

/// What the code around a closure knows of the object each variable it
/// captures holds.
pub(crate) type CapturedHolders<'a> = &'a dyn Fn(&str) -> Option<Holder>;

/// What a builtin runs through an argument.
#[derive(Debug, Clone, Copy)]
pub(super) enum BuiltinRun {
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

    /// Whether a probed method has this name (lowercased), on any class.
    pub(super) fn probed_member(member: &str) -> bool {
        static MEMBERS: std::sync::LazyLock<rustc_hash::FxHashSet<&'static str>> =
            std::sync::LazyLock::new(|| {
                PROBED
                    .iter()
                    .filter_map(|name| name.split_once("::").map(|(_, member)| member))
                    .collect()
            });
        MEMBERS.contains(member)
    }

    /// What the builtin was seen to run through its arguments.
    pub(super) fn runs(name: &str) -> &'static [(Position, BuiltinRun)] {
        RUNS.binary_search_by(|&(entry, _)| entry.cmp(name))
            .map_or(&[], |at| RUNS[at].1)
    }

    /// Whether the function (lowercased) kept none of a callable given as
    /// its argument `at` once it returned.
    pub(super) fn releases(name: &str, at: u32) -> bool {
        RELEASES
            .binary_search_by(|&(entry, _)| entry.cmp(name))
            .is_ok_and(|found| RELEASES[found].1.contains(&at))
    }

    /// Whether the method (`class::method`, lowercased) ran a callable or
    /// an object's method its object was made with, or given by another
    /// method.
    pub(super) fn holding(name: &str) -> bool {
        HOLDING.binary_search(&name).is_ok()
    }

    /// The stream-wrapper methods (lowercased) the method (`class::method`,
    /// lowercased) ran through a URL its object was made with, or given by
    /// another method.
    pub(super) fn held_wrappers(name: &str) -> &'static [&'static str] {
        HELD_WRAPPERS
            .binary_search_by(|&(entry, _)| entry.cmp(name))
            .map_or(&[], |at| HELD_WRAPPERS[at].1)
    }
}

/// Whether the builtin function `name` (lowercased) was seen to keep none of
/// a callable it was given as its argument `at` once it returned.
pub(crate) fn builtin_releases(name: &str, at: u32) -> bool {
    builtin_runs::releases(name, at)
}

/// The methods PHP calls on an object it is given where it takes an
/// iterator, a countable, an array-like, a serialisable or any object.
const ITERATION_HOOKS: [&str; 6] = ["getiterator", "valid", "current", "key", "next", "rewind"];
const OFFSET_HOOKS: [&str; 4] = ["offsetget", "offsetset", "offsetexists", "offsetunset"];
const SERIALIZATION_HOOKS: [&str; 6] = [
    "serialize",
    "unserialize",
    "__serialize",
    "__unserialize",
    "__sleep",
    "__wakeup",
];
const STREAM_WRAPPER_METHODS: [&str; 19] = [
    "stream_open",
    "stream_read",
    "stream_write",
    "stream_close",
    "stream_eof",
    "stream_tell",
    "stream_seek",
    "stream_flush",
    "stream_stat",
    "stream_lock",
    "stream_truncate",
    "stream_set_option",
    "stream_cast",
    "stream_metadata",
    "url_stat",
    "dir_opendir",
    "dir_readdir",
    "dir_closedir",
    "dir_rewinddir",
];
const PATH_PARAMETER_NAMES: [&str; 8] = [
    "filename",
    "file",
    "path",
    "filepath",
    "url",
    "uri",
    "directory",
    "dir",
];

/// What a stub that no run of PHP has seen may run through its parameters,
/// read from their types the way `tools/builtin-runs/record.php` measures
/// them: a string is converted, an array's elements are converted, an
/// iterator iterated, a countable counted, any object or `mixed` may be any
/// of these, and a path or a stream may be a registered wrapper's.
fn derived_runs(sig: &FunctionReflection) -> Vec<(Position, BuiltinRun)> {
    let mut runs = Vec::new();
    for (at, p) in sig.params.iter().enumerate() {
        let at = at as u32;
        let any = p.ty.mentions_any(&["mixed", "object"]);
        let on = |names: &[&str]| any || p.ty.mentions_any(names);
        let mut row = |elements: bool, methods: &[&'static str]| {
            for &method in methods {
                runs.push((Position { at, elements }, BuiltinRun::Method(method)));
            }
        };
        let is_string = on(&["string", "Stringable"]);
        if is_string {
            row(false, &["__tostring"]);
        }
        if p.ty.mentions_any(&["array", "iterable"]) || any {
            row(true, &["__tostring"]);
        }
        if on(&["Traversable", "IteratorAggregate", "Iterator", "iterable"]) {
            row(false, &ITERATION_HOOKS);
        }
        if on(&["Countable"]) {
            row(false, &["count"]);
        }
        if on(&["ArrayAccess"]) {
            row(false, &OFFSET_HOOKS);
        }
        if on(&["JsonSerializable"]) {
            row(false, &["jsonserialize"]);
        }
        if on(&["Serializable"]) {
            row(false, &SERIALIZATION_HOOKS);
        }
        if any {
            row(false, &["__debuginfo"]);
        }
        let names_a_path = PATH_PARAMETER_NAMES.contains(&p.name.as_ref());
        if (is_string && names_a_path) || p.ty.mentions_any(&["resource"]) {
            for method in STREAM_WRAPPER_METHODS {
                runs.push((
                    Position {
                        at,
                        elements: false,
                    },
                    BuiltinRun::Wrapper(method),
                ));
            }
        }
    }
    runs
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
fn methods_by_name(registry: &Registry) -> FxHashMap<Name, Vec<crate::registry::ClassId>> {
    let mut by_name: FxHashMap<Name, Vec<crate::registry::ClassId>> = FxHashMap::default();
    for (class, class_id) in registry.class_ids.iter() {
        for method in registry.method_names(class) {
            let Some(sig) = registry.lookup_own_method(class, method) else {
                continue;
            };
            if !sig.is_stub {
                by_name
                    .entry(method.lowered().clone())
                    .or_default()
                    .push(*class_id);
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
                    let classes = by_name.entry(Name::from(member)).or_default();
                    if classes.last() != Some(class_id) {
                        classes.push(*class_id);
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
fn never_runs(registry: &Registry, class: &str, method: &str, sig: &FunctionReflection) -> bool {
    sig.is_stub
        || sig.is_magic
        || registry
            .class(class)
            .is_some_and(|(_, _, info)| info.kind() == ClassKind::Interface)
        || registry.method_is_abstract(class, method)
}

/// One body's own writes and the calls it inherits writes from.
#[derive(Debug)]
struct Classification<'r> {
    own: Effect,
    /// Each declaration a call runs, with what its own `$this` is to the
    /// body and, when it takes the call's arguments as its own, the call
    /// (`sites`).
    calls: Vec<(EffectIx, TransferIx, Option<u32>)>,
    /// The declarations calls run that no call's arguments reach, found
    /// once for every body that asks the same.
    sets: Vec<IdTargets>,
    sites: Vec<Site<'r>>,
    /// The parameters whose argument the body runs itself.
    runs: Vec<Condition>,
    /// The properties the body writes other than as a constructor does.
    property_writes: PropertyWrites,
}

/// The properties code writes after the object is made: by name, since a
/// write through a variable, a reference or a callback does not say which
/// class it is on. A write by `$this` in a constructor or `__clone` is not
/// among them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct PropertyWrites {
    // Sorted, each once, once settled.
    names: Vec<Name>,
    /// Some write is to a property not known, or from code not known.
    all: bool,
}

impl PropertyWrites {
    fn everything() -> Self {
        PropertyWrites {
            names: Vec::new(),
            all: true,
        }
    }

    fn note(&mut self, prop: Option<&Name>) {
        match prop {
            Some(prop) => self.names.push(prop.clone()),
            None => self.all = true,
        }
    }

    fn merge(&mut self, other: &PropertyWrites) {
        self.all |= other.all;
        self.names.extend(other.names.iter().cloned());
    }

    fn settle(&mut self) {
        self.names.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
        self.names.dedup();
    }

    /// Whether no code of the loaded program writes `prop` but a
    /// constructor of the class, or `__clone`, on its `$this`.
    pub(crate) fn never_written_after_construction(&self, prop: &str) -> bool {
        !self.all
            && self
                .names
                .binary_search_by(|name| name.as_str().cmp(prop))
                .is_err()
    }
}

/// The properties code of the loaded program writes after the object is
/// made, and whether a file names something that writes one without a write
/// in the code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PropertyStability {
    writes: PropertyWrites,
    external_writers: bool,
}

/// Before the effects are settled nothing is known to be stable.
impl Default for PropertyStability {
    fn default() -> Self {
        PropertyStability {
            writes: PropertyWrites::everything(),
            external_writers: true,
        }
    }
}

impl syntax::canonical::Canonical for PropertyStability {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let PropertyStability {
            writes,
            external_writers,
        } = self;
        p.node(
            "PropertyStability",
            &[("writes", writes), ("external_writers", external_writers)],
        );
    }
}

impl syntax::canonical::Canonical for PropertyWrites {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let PropertyWrites { names, all } = self;
        p.node("PropertyWrites", &[("names", names), ("all", all)]);
    }
}

/// A set of declarations, by id, each with what its own `$this` is to the
/// caller.
pub(in crate::registry) type IdTargets = Arc<IdTargetSet>;

/// The declarations of a set by id, and what the set adds to a caller as of
/// the wave of `settle` that last asked.
#[derive(Debug)]
pub(in crate::registry) struct IdTargetSet {
    entries: Vec<(EffectIx, TransferIx)>,
    joined: Mutex<(u64, Option<Arc<SetJoin>>)>,
}

impl IdTargetSet {
    fn new(entries: Vec<(EffectIx, TransferIx)>) -> Self {
        IdTargetSet {
            entries,
            joined: Mutex::new((0, None)),
        }
    }

    /// What the set adds to a caller, read from the settled snapshot of the
    /// wave `wave` (a number no other wave has) once for every caller.
    fn joined<'g>(
        &self,
        wave: u64,
        transfers: &TransferTable,
        get: &impl Fn(EffectIx) -> std::borrow::Cow<'g, Settled>,
    ) -> Arc<SetJoin> {
        let mut held = self
            .joined
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if held.0 == wave
            && let Some(joined) = &held.1
        {
            return Arc::clone(joined);
        }
        let joined = Arc::new(SetJoin::of(&self.entries, transfers, get));
        *held = (wave, Some(Arc::clone(&joined)));
        joined
    }
}

/// The numbers of the waves of every `settle`.
static WAVES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Classification<'_> {
    fn anything() -> Self {
        let mut classification = Classification::of(Effect::anything());
        classification.property_writes = PropertyWrites::everything();
        classification
    }

    fn unknown() -> Self {
        Classification::of(Effect::unknown())
    }

    fn of(own: Effect) -> Self {
        Classification {
            own,
            calls: Vec::new(),
            sets: Vec::new(),
            sites: Vec::new(),
            runs: Vec::new(),
            property_writes: PropertyWrites::default(),
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
    runs: RunList,
    params: Vec<Condition>,
    /// What dies if the declaration writes over the parameter.
    destroys: Option<RunList>,
}

/// Where a call passes a parameter of a declaration it runs.
enum Passing<'c> {
    /// The argument passed there.
    Argument(&'c ArgAt),
    /// Nothing: the parameter's default runs these, called; none when the
    /// parameter is required, since the call throws.
    Default(RunList),
    /// Not known.
    Unknown,
}

/// Whether the declaration `key` a call of `callee` runs takes the call's
/// arguments as its own: `__call` and `__callStatic` take another method's
/// as an array.
fn passes_arguments(callee: &Callee, key: &DeclKey) -> bool {
    if matches!(
        callee,
        Callee::Implicit { .. } | Callee::ParentHook { .. } | Callee::Include(_)
    ) {
        return false;
    }
    if &*key.member != "__call" && &*key.member != "__callstatic" {
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
fn function_named(registry: &Registry, text: &str) -> RunList {
    match registry.lookup_function_with_name(text.trim_start_matches('\\')) {
        Some((declared, _)) => smallvec![(
            registry.effect_ix(&function_key(declared)),
            TransferTable::STATIC,
        )],
        None => RunList::new(),
    }
}

/// The magic method PHP calls for `access` to a property it runs one for.
fn property_magic_method(access: Access) -> &'static Name {
    let known = well_known();
    match access {
        Access::Read => &known.get,
        Access::Write => &known.set,
        Access::Isset => &known.isset,
        Access::Unset => &known.unset,
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
        Operation::Clone { with: false }
        | Operation::Element(_)
        | Operation::String
        | Operation::Destroy => None,
    }
}

/// The method PHP calls on an `ArrayAccess` for `access` to an element.
fn element_method(access: Access) -> &'static Name {
    let known = well_known();
    match access {
        Access::Read => &known.offset_get,
        Access::Write => &known.offset_set,
        Access::Isset => &known.offset_exists,
        Access::Unset => &known.offset_unset,
    }
}

/// An operation, as a key to remember what it runs by.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct OperationKey(u8, u8, Option<Name>);

impl OperationKey {
    fn of(operation: &Operation) -> OperationKey {
        let access = |access: &Access| match access {
            Access::Read => 0,
            Access::Write => 1,
            Access::Isset => 2,
            Access::Unset => 3,
        };
        match operation {
            Operation::Property(name, a) => OperationKey(0, access(a), name.clone()),
            Operation::Element(a) => OperationKey(1, access(a), None),
            Operation::String => OperationKey(2, 0, None),
            Operation::Destroy => OperationKey(3, 0, None),
            Operation::Clone { with } => OperationKey(4, u8::from(*with), None),
            Operation::Iterate { keys } => OperationKey(5, u8::from(*keys), None),
        }
    }
}

/// What an operation's targets depend on: the object, the scope that
/// asks, and the operation.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct ImplicitKey {
    holder: Holder,
    same: bool,
    operation: OperationKey,
    /// Whether it is the hooks alone that are asked about.
    hooks: Option<bool>,
    scope: Option<Name>,
    rebindable: bool,
}

/// Every method PHP may call for `operation` on an object of some class.
fn implicit_method_names(operation: &Operation) -> Vec<&'static Name> {
    let known = well_known();
    match operation {
        Operation::Property(_, access) => vec![property_magic_method(*access)],
        Operation::Element(access) => vec![element_method(*access)],
        Operation::String => vec![&known.to_string],
        Operation::Destroy => vec![&known.destruct],
        Operation::Clone { with: false } => vec![&known.clone],
        Operation::Clone { with: true } => vec![&known.clone, &known.set],
        Operation::Iterate { keys } => {
            let mut methods = vec![
                &known.get_iterator,
                &known.rewind,
                &known.valid,
                &known.current,
                &known.next,
            ];
            if *keys {
                methods.push(&known.key);
            }
            methods
        }
    }
}

/// A declaration run with what its own `$this` is to the caller, by handle.
pub(super) type Run = (EffectIx, TransferIx);

/// The runs of one call: most reach one to three declarations.
pub(super) type RunList = SmallVec<[Run; 4]>;

/// The classes of a holder before it is shared; most name one.
pub(crate) type NameList = SmallVec<[Name; 4]>;

/// The declarations a call runs, each with what its own `$this` is to the
/// caller, in the order they were first added and once each.
#[derive(Default)]
pub(super) struct TargetSet {
    list: RunList,
    /// Every entry of `list`, kept once the list is long enough for a scan
    /// to cost more than a hash.
    seen: FxHashSet<Run>,
}

impl TargetSet {
    /// Entries a scan is cheaper than a hash for.
    const SCANNED: usize = 12;

    /// Adds `run` once.
    pub(super) fn push(&mut self, run: Run) {
        if self.list.len() < Self::SCANNED {
            if !self.list.contains(&run) {
                self.list.push(run);
            }
            return;
        }
        if self.seen.is_empty() {
            self.seen.extend(self.list.iter().copied());
        }
        if self.seen.insert(run) {
            self.list.push(run);
        }
    }

    pub(super) fn into_list(self) -> RunList {
        self.list
    }

    pub(super) fn into_shared(self) -> SharedTargets {
        Arc::from(&self.list[..])
    }
}

fn classification_by_id<'r>(
    registry: &Registry,
    key: DeclKey,
    classification: Classification<'r>,
) -> (EffectIx, Classification<'r>) {
    (registry.effect_ix(&key), classification)
}

impl Registry {
    /// The handle the declaration `key` stands for in the effects pass.
    pub(crate) fn effect_ix(&self, key: &DeclKey) -> EffectIx {
        let memo = &self.reach_memo;
        let ix = memo
            .effect_ixs
            .get_or_compute_ref(key, || memo.effects.intern(&self.effect_id_for(key), key));
        debug_assert_eq!(*memo.effects.id(ix), self.effect_id_for(key));
        ix
    }

    /// The declaration the handle `ix` stands for.
    pub(crate) fn effect_key(&self, ix: EffectIx) -> &DeclKey {
        self.reach_memo.effects.key(ix)
    }

    /// The transfer the handle `ix` stands for.
    pub(crate) fn transfer_of(&self, ix: TransferIx) -> &Transfer {
        self.reach_memo.transfers.get(ix)
    }

    /// The handles of a declaration run with its own `$this` what
    /// `transfer` says.
    pub(crate) fn effect_run(&self, key: &DeclKey, transfer: &Transfer) -> Run {
        (
            self.effect_ix(key),
            self.reach_memo.transfers.intern(transfer),
        )
    }

    /// The settled effect of the declaration `key`, when the pass found one.
    fn settled_of(&self, ix: EffectIx) -> Option<&Settled> {
        self.effects.get(self.reach_memo.effects.id(ix))
    }

    pub(crate) fn effect_id_for(&self, key: &DeclKey) -> EffectId {
        if let Some(file) = key.file_id() {
            return EffectId::File(file);
        }
        if let Some((caller, ordinal)) = closure_parent(key) {
            return EffectId::Closure {
                caller: Box::new(self.effect_id_for(&caller)),
                ordinal,
            };
        }
        match key.class.as_ref() {
            None => self
                .function_ids
                .get(&key.member)
                .copied()
                .map_or_else(|| EffectId::Named(key.clone()), EffectId::Function),
            Some(class) if key.member.starts_with('$') => {
                let hook = self.property_names(class).find_map(|property| {
                    [
                        (false, PropertyHookKind::Get),
                        (true, PropertyHookKind::Set),
                    ]
                    .into_iter()
                    .find(|(set, _)| hook_member(Some(property), *set) == *key.member)
                    .map(|(_, kind)| (property, kind))
                });
                let Some((property, kind)) = hook else {
                    return EffectId::Named(key.clone());
                };
                let Some(class_id) = self.class_ids.get(class).copied() else {
                    return EffectId::Named(key.clone());
                };
                let Some(property) = self.property_ref_by_name(class_id, property) else {
                    return EffectId::Named(key.clone());
                };
                EffectId::PropertyHook { property, kind }
            }
            Some(class) => {
                let method = self.class_ids.get(class).and_then(|id| {
                    self.member_slot(class, &key.member)
                        .map(|slot| crate::registry::MethodRef::new(*id, slot))
                });
                method.map_or_else(|| EffectId::Named(key.clone()), EffectId::Method)
            }
        }
    }

    pub(crate) fn effect_decl_key(&self, id: &EffectId) -> Option<DeclKey> {
        match id {
            EffectId::Function(id) => self.function_name_by_id(*id).map(function_key),
            EffectId::Method(method) => {
                let class = self.class_name_by_id(method.class())?;
                let member = self.member_entry_by_ref(*method)?;
                Some(method_key(class, &member.name))
            }
            EffectId::PropertyHook { property, kind } => {
                let class_name = self.class_name_by_id(property.class())?;
                let property = self.property_name_by_ref(*property)?;
                Some(hook_key(
                    class_name,
                    property.as_str(),
                    *kind == PropertyHookKind::Set,
                ))
            }
            // A file's number is no part of what is kept between runs.
            EffectId::Closure { .. } | EffectId::File(_) => None,
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
        self.reach_memo.transferred = Default::default();
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
        let loaded = bodies.files();
        let facts: Vec<_> = loaded
            .iter()
            .map(|file| (file.id, file.path, file.global_facts))
            .collect();
        self.resolve_includes(&facts);
        self.methods_by_name = Arc::new(methods_by_name(self));
        self.prepare_stub_effect_results();
        let mut unset = crate::registry::bodies::UnsetProperties::default();
        bodies.unset_properties(&mut unset);
        unset.settle();
        self.unset_properties = Arc::new(unset);
        let mut wrappers = crate::registry::bodies::StreamWrappers::default();
        bodies.stream_wrappers(&mut wrappers);
        wrappers.settle();
        self.stream_wrappers = Arc::new(wrappers);
        let mut running_code = (*self.base_builtin_methods_running_code).clone();
        for (name, mut owners) in self.own_builtin_methods_running_code() {
            if let Some(known) = running_code.get(name.as_str()) {
                owners.extend(known.iter().cloned());
                owners.sort_unstable();
                owners.dedup();
            }
            running_code.insert(Name::from(name), owners);
        }
        self.builtin_methods_running_code = running_code;
        self.reach_memo.reset();
        let registry: &Registry = self;
        let functions: Vec<(&Name, &FunctionReflection)> = registry
            .own_functions_by_id()
            .filter_map(|(_, name, sig)| (!sig.is_stub).then_some((name, sig)))
            .collect();
        let methods: Vec<(&Name, &Name, &FunctionReflection)> = registry
            .own_classes_by_id()
            .flat_map(|(id, class, _)| {
                registry
                    .members_with_slots(id)
                    .filter_map(move |(at, entry)| {
                        let method = &entry.name;
                        let sig = registry.method_by_ref(at)?;
                        (!never_runs(registry, class, method, sig)).then_some((class, method, sig))
                    })
            })
            .collect();
        let mut classified: Vec<(EffectIx, Classification<'_>)> = functions
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .flat_map_iter(|(name, sig)| {
                registry
                    .classify(bodies, None, name, sig, function_key(name))
                    .into_iter()
                    .map(|(key, classification)| {
                        classification_by_id(registry, key, classification)
                    })
            })
            .collect();
        classified.par_extend(
            methods
                .par_iter()
                .with_min_len(crate::registry::PASS_CHUNK)
                .flat_map_iter(|(class, method, sig)| {
                    let key = method_key(class, method);
                    registry
                        .classify(bodies, Some(class), method, sig, key)
                        .into_iter()
                        .map(|(key, classification)| {
                            classification_by_id(registry, key, classification)
                        })
                }),
        );
        classified.par_extend(
            loaded
                .par_iter()
                .with_min_len(crate::registry::PASS_CHUNK)
                .flat_map_iter(|file| {
                    registry
                        .classify_file(file.id, file.top_level)
                        .into_iter()
                        .map(|(key, classification)| {
                            classification_by_id(registry, key, classification)
                        })
                }),
        );
        let hooks: Vec<(&Name, &Name, &PropertyReflection, bool)> = registry
            .own_classes_by_id()
            .flat_map(|(_, class, _)| {
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
                    registry
                        .classify_hook(bodies, class, property, declared, *set)
                        .into_iter()
                        .map(|(key, classification)| {
                            classification_by_id(registry, key, classification)
                        })
                }),
        );
        let settlements = settle(registry, &classified, true, |ix| {
            match registry.reach_memo.effects.id(ix) {
                // A file the program does not hold, or holds no summary of.
                EffectId::File(_) => Settled::of(Effect::anything()),
                id => registry.effect_decl_key(id).map_or_else(
                    || Settled::of(Effect::unknown()),
                    |_| Settled::of(registry.declaration_effect(ix)),
                ),
            }
        });
        let mut stability = PropertyStability {
            writes: PropertyWrites::default(),
            external_writers: bodies.names_property_writer(),
        };
        for (_, classification) in &classified {
            stability.writes.merge(&classification.property_writes);
        }
        stability.writes.settle();
        // A closure's effect is taken where it runs; no call names it. Keep
        // its result through settlement, then omit it from the registry.
        let effects = settlements.into_effects(registry);
        self.property_stability = Arc::new(stability);
        self.reach_memo.transferred = Default::default();
        self.effects = effects;
    }

    /// What a declaration with no body at hand may write: a stub's by its
    /// parameters, an abstract method or an interface's nothing (it never
    /// runs), anything else's anything.
    fn declaration_effect(&self, ix: EffectIx) -> Effect {
        let key = self.effect_key(ix);
        match &key.class {
            Some(class) => {
                if self.class(class).is_none() {
                    return Effect::anything();
                }
                let Some(sig) = self.lookup_own_method(class, &key.member) else {
                    return Effect::anything();
                };
                if sig.is_stub {
                    self.stub_effect(ix, sig)
                } else if never_runs(self, class, &key.member, sig) {
                    Effect::nothing()
                } else {
                    Effect::anything()
                }
            }
            None => match self.lookup_function(&key.member) {
                Some(sig) if sig.is_stub => self.stub_effect(ix, sig),
                _ => Effect::anything(),
            },
        }
    }

    /// What a call to the declaration `key` may write, as settled.
    fn settled(&self, key: &DeclKey) -> Settled {
        self.settled_by_ix(self.effect_ix(key))
    }

    /// `settled` for the declaration `ix` stands for.
    fn settled_by_ix(&self, ix: EffectIx) -> Settled {
        match self.settled_of(ix) {
            Some(settled) => settled.clone(),
            None => Settled::of(self.declaration_effect(ix)),
        }
    }

    /// What a call to the declaration `key` whose arguments are not known
    /// may write, as settled.
    fn settled_effect(&self, key: &DeclKey) -> Effect {
        self.settled_effect_by_ix(self.effect_ix(key))
    }

    /// What a call of the declaration `ix` stands for writes when its own
    /// `$this` is what `transfer` says: its own effect when `own`, else
    /// `settled_effect_by_ix`'s.
    fn transferred_effect(&self, ix: EffectIx, transfer: TransferIx, own: bool) -> Effect {
        let compute = || {
            let base = if own {
                match self.settled_of(ix) {
                    Some(settled) => settled.effect.clone(),
                    None => self.declaration_effect(ix),
                }
            } else {
                self.settled_effect_by_ix(ix)
            };
            base.transferred(self.transfer_of(transfer))
        };
        let effect = self
            .reach_memo
            .transferred
            .get_or_compute((ix, transfer, own), compute);
        debug_assert_eq!(effect, compute());
        effect
    }

    /// `settled_effect` for the declaration `ix` stands for.
    fn settled_effect_by_ix(&self, ix: EffectIx) -> Effect {
        match self.settled_of(ix) {
            Some(settled) => settled.unconditional(),
            None => self.declaration_effect(ix),
        }
    }

    /// The declaration `key` names, when it is one.
    fn declaration(&self, key: &DeclKey) -> Option<&FunctionReflection> {
        match &key.class {
            Some(class) => self.lookup_method(class, &key.member),
            None => self.lookup_function(&key.member),
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
            return Passing::Default(RunList::new());
        }
        let Some(default) = &param.default_ty else {
            return Passing::Unknown;
        };
        if default.is_null().yes() {
            return Passing::Default(RunList::new());
        }
        match default
            .string_literal_value()
            .and_then(|text| text.as_str())
        {
            Some(text) if !text.contains("::") => Passing::Default(function_named(self, text)),
            _ => Passing::Unknown,
        }
    }

    /// `passing` for a parameter the declaration writes over: an argument
    /// left out passes its default, which holds no object when its type says
    /// so.
    fn passing_destroyed<'c>(&self, call: &'c CallWrites, key: &DeclKey, at: u32) -> Passing<'c> {
        match self.passing(call, key, at) {
            Passing::Unknown => {
                let objectless = self
                    .declaration(key)
                    .and_then(|sig| sig.params.get(at as usize))
                    .filter(|param| {
                        !param.variadic
                            && param.has_default
                            && !call.arguments.iter().any(|arg| match arg {
                                ArgAt::Position(position) => *position == at,
                                ArgAt::Named(name) => name.as_str() == param.name.as_ref(),
                                ArgAt::Spread => true,
                            })
                    })
                    .and_then(|param| param.default_ty.as_ref())
                    .and_then(element_classes)
                    .is_some_and(|classes| classes.is_empty());
                if objectless {
                    Passing::Default(RunList::new())
                } else {
                    Passing::Unknown
                }
            }
            passing => passing,
        }
    }

    /// What calling `method` on an object that may be of `class` may write:
    /// every implementation it may reach, `class`'s own and each
    /// descendant's, or `__call`'s where there is none.
    pub(crate) fn virtual_method_effect(&self, class: &str, method: &str) -> Effect {
        let mut effect = Effect::nothing();
        for owner in self.implementations(class, method).iter() {
            effect.join(&self.settled_effect(owner));
            if effect.is_anything() {
                break;
            }
        }
        effect
    }

    /// The declarations a call of `method` on an object that may be of
    /// `class` may run: `class`'s and each descendant's, `__call` where a
    /// class has none, and `class`'s alone when no override can reach it.
    fn implementations(
        &self,
        class: &(impl KeyName + ?Sized),
        method: &(impl KeyName + ?Sized),
    ) -> Arc<[DeclKey]> {
        self.reach_memo.implementations.get_or_compute(
            (class.lowered_name(), method.lowered_name()),
            || {
                self.compute_implementations(class.text(), method.text())
                    .into()
            },
        )
    }

    /// `implementations`, by handle.
    fn implementation_ixs(
        &self,
        class: &(impl KeyName + ?Sized),
        method: &(impl KeyName + ?Sized),
    ) -> Arc<[EffectIx]> {
        self.reach_memo.implementation_ixs.get_or_compute(
            (class.lowered_name(), method.lowered_name()),
            || {
                self.implementations(class, method)
                    .iter()
                    .map(|key| self.effect_ix(key))
                    .collect()
            },
        )
    }

    fn compute_implementations(&self, class: &str, method: &str) -> Vec<DeclKey> {
        if let Some((owner, sig)) = self.lookup_method_with_owner(class, method)
            && is_non_overridable(self, class, &owner, method, sig)
        {
            return vec![self.reached(&owner, method, sig)];
        }
        let mut out: Vec<DeclKey> = Vec::new();
        let mut seen: FxHashSet<DeclKey> = FxHashSet::default();
        for candidate in
            std::iter::once(Name::from(class)).chain(self.subclasses_shared(class).iter().cloned())
        {
            let key = match self.lookup_method_with_owner(&candidate, method) {
                Some((owner, sig)) => self.reached(&owner, method, sig),
                None => match self.lookup_method_with_owner(&candidate, &well_known().call) {
                    Some((owner, _)) => method_key(&owner, &well_known().call),
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
    fn reached(
        &self,
        owner: &(impl KeyName + TableKey + ?Sized),
        method: &(impl KeyName + ?Sized),
        sig: &FunctionReflection,
    ) -> DeclKey {
        if sig.is_magic
            && let Some((call_owner, _)) = self.lookup_method_with_owner(owner, &well_known().call)
        {
            return method_key(&call_owner, &well_known().call);
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
    fn property_magic<K: TableKey + ?Sized>(
        &self,
        class: &K,
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

    /// Whether the property `prop` of an object of `class` holds, once the
    /// object is made, what the constructor gave it: no code of the loaded
    /// program writes a property of that name but a constructor, or
    /// `__clone`, on its `$this`, and no file names something that writes
    /// one without a write in its code. `assume_encapsulated` leaves out
    /// that second condition for a property that is not public.
    pub(crate) fn property_is_stable<K: TableKey + ?Sized>(
        &self,
        class: &K,
        prop: &str,
        assume_encapsulated: bool,
    ) -> bool {
        let Some((owner, declared)) = self.lookup_property_with_owner(class, prop) else {
            return false;
        };
        if declared.from_tag()
            || declared.is_static()
            || self.unset_properties.may_unset(prop)
            || self.is_builtin_class(&owner)
        {
            return false;
        }
        let stability = &*self.property_stability;
        (!stability.external_writers
            || (assume_encapsulated && declared.visibility() != Visibility::Public))
            && stability.writes.never_written_after_construction(prop)
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
            let parent = self
                .class_references_by_name(&owner)?
                .extends
                .first()?
                .clone();
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
    fn builtin_entry(&self, key: &DeclKey) -> Option<Arc<str>> {
        self.reach_memo
            .builtin_entries
            .get_or_compute_ref(key, || self.compute_builtin_entry(key))
    }

    fn compute_builtin_entry(&self, key: &DeclKey) -> Option<Arc<str>> {
        let name = key.builtin_name();
        if builtin_runs::probed(&name) {
            return Some(name.into());
        }
        let class = key.class.as_ref()?;
        if !builtin_runs::probed_member(&key.member) {
            return None;
        }
        self.all_ancestor_names_of(class)
            .iter()
            .find_map(|ancestor| {
                let name = format!("{}::{}", ancestor.to_ascii_lowercase(), key.member);
                builtin_runs::probed(&name).then(|| name.into())
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
    fn stub_holds_code(&self, class: &str) -> bool {
        self.reach_memo
            .stub_holds_code
            .get_or_compute(class.to_ascii_lowercase(), || {
                self.builtin_lineage(class)
                    .iter()
                    .any(|class| self.stub_holding_classes.contains(class))
            })
    }

    /// What a call of the stub `key` (`sig`) whose arguments are not known
    /// may write: nothing when a call of it was seen to run no code through
    /// any argument, its object holds none, and none of its parameters
    /// takes something through which it runs code
    /// (`CODE_RUNNING_PARAMETER_TYPES`); for a stub no call of which was
    /// seen, nothing when its doc declares it pure and it takes no code;
    /// anything otherwise.
    fn stub_effect(&self, ix: EffectIx, sig: &FunctionReflection) -> Effect {
        let runs_code = self
            .reach_memo
            .stub_runs
            .get_or_compute(ix, || self.stub_runs_code(self.effect_key(ix), sig));
        if runs_code {
            Effect::unknown()
        } else {
            Effect::nothing()
        }
    }

    /// Rebuilds the stub classes whose objects may run what they hold,
    /// after declaration and hierarchy results are ready. The set is
    /// immutable while effect settlement runs in parallel.
    fn prepare_stub_effect_results(&mut self) {
        let mut holding = FxHashSet::default();
        for (class, _) in self.class_ids.iter() {
            for method in self.method_names(class) {
                let Some(sig) = self.lookup_own_method(class, method) else {
                    continue;
                };
                if sig.is_stub
                    && sig
                        .params
                        .iter()
                        .any(|p| p.ty.mentions_any(&CODE_RUNNING_PARAMETER_TYPES))
                    && self.builtin_entry(&method_key(class, method)).is_none()
                {
                    holding.insert(class.to_ascii_lowercase());
                }
            }
        }
        self.stub_holding_classes = holding;
    }

    pub(crate) fn stub_runs_code(&self, key: &DeclKey, sig: &FunctionReflection) -> bool {
        let takes_code = sig
            .params
            .iter()
            .any(|p| p.ty.mentions_any(&CODE_RUNNING_PARAMETER_TYPES));
        let Some(name) = self.builtin_entry(key) else {
            // A stub no run of PHP has seen is taken at its word: one
            // its doc declares pure writes nothing, unless it takes code.
            return !sig.declared_pure || takes_code;
        };
        builtin_runs::holding(&name)
            || !builtin_runs::held_wrappers(&name).is_empty()
            || !builtin_runs::runs(&name).is_empty()
            || takes_code
    }

    /// The constructor of every class that has one, as the class resolves it.
    fn every_constructor(&self) -> Arc<[DeclKey]> {
        self.reach_memo
            .constructors
            .get_or_init(|| {
                self.classes_declaring(&well_known().construct)
                    .filter_map(|class| {
                        self.lookup_method_with_owner(class, &well_known().construct)
                    })
                    .map(|(owner, _)| method_key(&owner, &well_known().construct))
                    .collect()
            })
            .clone()
    }

    /// The builtin `key` is, when it is one.
    fn builtin(&self, key: &DeclKey) -> Option<&FunctionReflection> {
        self.declaration(key).filter(|sig| sig.is_stub)
    }

    /// `builtin` for the declaration `ix` stands for, with the question
    /// whether it is one asked once.
    fn builtin_of(&self, ix: EffectIx) -> Option<&FunctionReflection> {
        let key = self.effect_key(ix);
        if self
            .reach_memo
            .effects
            .is_builtin(ix, || self.builtin(key).is_some())
        {
            self.builtin(key)
        } else {
            None
        }
    }

    /// The stream-wrapper methods named `method` a class that may be
    /// registered as a wrapper declares, run as the wrapper's own object.
    fn wrapper_runs(&self, method: &'static str) -> Arc<[Run]> {
        self.reach_memo.wrapper_runs.get_or_compute(method, || {
            let mut out = RunList::new();
            for class in self
                .classes_declaring(method)
                .filter(|class| self.may_be_registered_wrapper(class))
            {
                if let Some((owner, sig)) = self.lookup_method_with_owner(class, method) {
                    out.push((
                        self.effect_ix(&self.reached(&owner, method, sig)),
                        TransferTable::MADE,
                    ));
                }
            }
            Arc::from(&out[..])
        })
    }

    /// The methods with a body overriding one of the builtin class
    /// `builtin`'s that an object of `receiver` or of a class below it may
    /// run: what a method of the builtin may run on its own object. Each of
    /// the builtin's methods is taken as `receiver` resolves it, which may
    /// be an ancestor's, and from each class below `receiver` declaring it
    /// with a body.
    fn overrides_of_builtin(&self, receiver: &str, builtin: &str) -> Arc<[DeclKey]> {
        self.reach_memo.overrides_of_builtin.get_or_compute(
            (receiver.to_ascii_lowercase(), builtin.to_ascii_lowercase()),
            || self.compute_overrides_of_builtin(receiver, builtin).into(),
        )
    }

    fn compute_overrides_of_builtin(&self, receiver: &str, builtin: &str) -> Vec<DeclKey> {
        if self.class(builtin).is_none() {
            return Vec::new();
        }
        let below = self.subclasses_shared(receiver);
        let below_lower: FxHashSet<&str> = below.iter().map(Name::lower).collect();
        let mut out = Vec::new();
        for method in self.method_names(builtin) {
            if let Some((owner, sig)) = self.lookup_method_with_owner(receiver, method)
                && !sig.is_stub
            {
                out.push(method_key(&owner, method));
            }
            if below.len() < self.declaring_ids(method).len() {
                out.extend(
                    below
                        .iter()
                        .filter(|class| {
                            self.lookup_own_method(class, method)
                                .is_some_and(|sig| !sig.is_stub)
                        })
                        .map(|class| method_key(class, method)),
                );
            } else {
                out.extend(
                    self.classes_declaring(method)
                        .filter(|class| below_lower.contains(class.lower()))
                        .map(|class| method_key(class, method)),
                );
            }
        }
        out
    }

    /// The ids of the loaded classes declaring `method` with a body.
    fn declaring_ids(&self, method: &(impl KeyName + ?Sized)) -> &[crate::registry::ClassId] {
        self.methods_by_name
            .get(&method.lowered_name())
            .map_or(&[], Vec::as_slice)
    }

    /// The loaded classes declaring `method` with a body.
    fn classes_declaring(&self, method: &(impl KeyName + ?Sized)) -> impl Iterator<Item = &Name> {
        self.methods_by_name
            .get(&method.lowered_name())
            .into_iter()
            .flatten()
            .filter_map(|id| self.class_name_by_id(*id))
    }

    /// Whether `class` may be registered as a stream wrapper: a registration
    /// names it, or one names a class not known.
    fn may_be_registered_wrapper(&self, class: &str) -> bool {
        let lower = class.to_ascii_lowercase();
        self.stream_wrappers
            .may_be_registered(&lower, |base| self.is_subclass_of(class, base))
    }

    /// Every loaded declaration of `method` with a body, and every `__call`
    /// and `__callStatic`, and the builtin ones that run code passed to
    /// them: what a call on an object or a class not known may run.
    fn methods_named(&self, method: &(impl KeyName + ?Sized)) -> Arc<[DeclKey]> {
        self.reach_memo
            .methods_named
            .get_or_compute(method.lowered_name(), || {
                self.compute_methods_named(method.text()).into()
            })
    }

    /// `methods_named`, by handle.
    fn methods_named_ixs(&self, method: &(impl KeyName + ?Sized)) -> Arc<[EffectIx]> {
        self.reach_memo
            .methods_named_ixs
            .get_or_compute(method.lowered_name(), || {
                self.methods_named(method)
                    .iter()
                    .map(|key| self.effect_ix(key))
                    .collect()
            })
    }

    fn compute_methods_named(&self, method: &str) -> Vec<DeclKey> {
        let mut out: Vec<DeclKey> = Vec::new();
        let mut seen: FxHashSet<DeclKey> = FxHashSet::default();
        let method_name = Name::from(method);
        let declaring = self
            .classes_declaring(method)
            .chain(self.builtin_classes_running_code(method))
            .map(|class| (class, &method_name));
        let magic = [&well_known().call, &well_known().call_static]
            .into_iter()
            .flat_map(|magic| {
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
        out
    }

    /// The builtin classes declaring `method`, which runs code passed to
    /// it.
    fn builtin_classes_running_code(&self, method: &str) -> impl Iterator<Item = &Name> {
        self.builtin_methods_running_code()
            .get(method.to_ascii_lowercase().as_str())
            .into_iter()
            .flatten()
    }

    /// What calling `method` on an object of a class not known may write.
    pub(crate) fn method_effect_by_name(&self, method: &str) -> Effect {
        let mut effect = Effect::nothing();
        for key in self.methods_named(method).iter() {
            effect.join(&self.settled_effect(key));
            if effect.is_anything() {
                break;
            }
        }
        effect
    }

    pub(crate) fn rebuild_methods_by_name(&mut self) {
        self.methods_by_name = Arc::new(methods_by_name(self));
        self.reach_memo.reset();
    }

    /// The names (lowercased) of the builtin methods that run code passed to
    /// them, found once: the stubs do not change.
    fn builtin_methods_running_code(&self) -> &NameMap<Vec<Name>> {
        &self.builtin_methods_running_code
    }

    /// The builtin methods of the classes the registry holds itself that
    /// run code passed to them, a stub method whose effect is anything: by
    /// name, lowercased, with the classes declaring each.
    pub(crate) fn own_builtin_methods_running_code(&self) -> FxHashMap<String, Vec<Name>> {
        let mut names: FxHashMap<String, Vec<Name>> = FxHashMap::default();
        for (class, _) in self.class_ids.iter() {
            for method in self.method_names(class) {
                let Some(sig) = self.lookup_own_method(class, method) else {
                    continue;
                };
                if !sig.is_stub {
                    continue;
                }
                let Some((owner, _)) = self.lookup_method_with_owner(class, method) else {
                    continue;
                };
                if self
                    .stub_effect(self.effect_ix(&method_key(&owner, method)), sig)
                    .is_anything()
                {
                    let owners = names.entry(method.to_ascii_lowercase()).or_default();
                    if !owners.contains(&owner) {
                        owners.push(owner);
                    }
                }
            }
        }
        for owners in names.values_mut() {
            owners.sort_unstable();
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
            file: sig.provenance.file_id(),
            file_scope: false,
            summary,
            value: None,
            captured: None,
        };
        resolve.run(&key, Some(sig))
    }

    /// The classification of the top level of `file`, summarised as
    /// `summary`, and of each closure it wrote, under its own.
    fn classify_file<'r>(
        &'r self,
        file: FileId,
        summary: &'r BodySummary,
    ) -> Vec<(DeclKey, Classification<'r>)> {
        let resolve = Resolve {
            registry: self,
            class: None,
            member: Name::from(format!("{{file#{}}}", file.index())),
            file: Some(file),
            file_scope: true,
            summary,
            value: None,
            captured: None,
        };
        resolve.run(&DeclKey::file(file), None)
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
            file: declared.provenance().file_id(),
            file_scope: false,
            summary,
            value: value.map(|value| (value.as_str(), ty)),
            captured: None,
        };
        resolve.run(&key, None)
    }
}

/// Where each declaration of a classified list sits in it, by handle.
enum Slots {
    /// A position plus one by handle number, 0 for a declaration not held.
    Dense(Vec<u32>),
    Sparse(FxHashMap<EffectIx, u32>),
}

impl Slots {
    /// The slots of `classified`, a declaration listed twice at the later
    /// position. `dense` is the number of handles when a table of that
    /// size is worth it.
    fn of(classified: &[(EffectIx, Classification<'_>)], dense: Option<usize>) -> Slots {
        match dense {
            Some(len) => {
                let mut at = vec![0u32; len];
                for (pos, (ix, _)) in classified.iter().enumerate() {
                    at[ix.index()] = pos as u32 + 1;
                }
                Slots::Dense(at)
            }
            None => Slots::Sparse(
                classified
                    .iter()
                    .enumerate()
                    .map(|(pos, (ix, _))| (*ix, pos as u32))
                    .collect(),
            ),
        }
    }

    fn get(&self, ix: EffectIx) -> Option<usize> {
        match self {
            Slots::Dense(at) => match at.get(ix.index()) {
                Some(&at) if at != 0 => Some(at as usize - 1),
                _ => None,
            },
            Slots::Sparse(at) => at.get(&ix).map(|&at| at as usize),
        }
    }
}

/// What `settle` found for the declarations it was given.
struct Settlements {
    slots: Slots,
    /// The handle of each position of the list `settle` was given.
    ixs: Vec<EffectIx>,
    /// What each position settled to; none for a declaration listed again
    /// further on.
    settled: Vec<Option<Settled>>,
}

impl Settlements {
    fn get(&self, ix: EffectIx) -> Option<&Settled> {
        self.slots
            .get(ix)
            .and_then(|pos| self.settled[pos].as_ref())
    }

    /// The settled effects by id, but for the closures' (a closure's effect
    /// is taken where it runs; no call names it).
    fn into_effects(self, registry: &Registry) -> FxHashMap<EffectId, Settled> {
        let table = &registry.reach_memo.effects;
        let mut out = FxHashMap::with_capacity_and_hasher(self.ixs.len(), Default::default());
        for (ix, settled) in self.ixs.into_iter().zip(self.settled) {
            if let Some(settled) = settled
                && !ix.is_closure()
            {
                out.insert(table.id(ix).clone(), settled);
            }
        }
        out
    }
}

/// What each declaration `classified` holds may write, settled from what it
/// writes itself: it grows by what each declaration it calls may write, by
/// `outside` for one `classified` does not hold, and, where a callee runs
/// what a call passes it, by what that may write. `shared` says the list is
/// the whole of the registry's handles, so its slots are a table by handle.
fn settle(
    registry: &Registry,
    classified: &[(EffectIx, Classification<'_>)],
    shared: bool,
    outside: impl Fn(EffectIx) -> Settled + Sync,
) -> Settlements {
    let table = &registry.reach_memo.effects;
    let transfers = &registry.reach_memo.transfers;
    let slots = Slots::of(classified, shared.then(|| table.len()));
    let live = |pos: usize| slots.get(classified[pos].0) == Some(pos);
    let mut settled: Vec<Option<Settled>> = classified
        .iter()
        .enumerate()
        .map(|(pos, (_, classification))| {
            live(pos).then(|| Settled {
                effect: classification.own.clone(),
                runs: classification.runs.iter().cloned().collect(),
            })
        })
        .collect();
    // What a declaration `classified` does not hold settles to, once.
    let outside_cache: Vec<std::sync::OnceLock<Settled>> = if shared {
        (0..table.len())
            .map(|_| std::sync::OnceLock::new())
            .collect()
    } else {
        Vec::new()
    };
    // A declaration's effect only ever grows, and grows only when one it is
    // made of does: every declaration is joined once, and again only when a
    // declaration it calls, or one an argument it passes runs, has grown.
    let callees_of = |classification: &'_ Classification<'_>| {
        let set_callees = classification
            .sets
            .iter()
            .flat_map(|set| set.entries.iter())
            .map(|(callee, _)| *callee);
        classification
            .calls
            .iter()
            .map(|(callee, _, _)| *callee)
            .chain(set_callees)
            .filter_map(|callee| slots.get(callee))
            .collect::<SmallVec<[usize; 8]>>()
    };
    let mut callers: Vec<Vec<u32>> = vec![Vec::new(); classified.len()];
    {
        // Each list is sized once, from the callers counted in a first pass
        // that skips a caller's repeats the way the second does.
        let mut counts = vec![0u32; classified.len()];
        let mut last = vec![u32::MAX; classified.len()];
        for (pos, (_, classification)) in classified.iter().enumerate() {
            if !live(pos) {
                continue;
            }
            for callee in callees_of(classification) {
                if last[callee] != pos as u32 {
                    last[callee] = pos as u32;
                    counts[callee] += 1;
                }
            }
        }
        for (list, count) in callers.iter_mut().zip(counts) {
            list.reserve_exact(count as usize);
        }
    }
    for (pos, (_, classification)) in classified.iter().enumerate() {
        if !live(pos) {
            continue;
        }
        for callee in callees_of(classification) {
            let list = &mut callers[callee];
            if list.last() != Some(&(pos as u32)) {
                list.push(pos as u32);
            }
        }
    }
    // The declarations an argument runs, which a declaration reads only once
    // a callee runs the argument.
    let mut passed_to: FxHashSet<(u32, u32)> = FxHashSet::default();
    let mut queue: std::collections::VecDeque<u32> = (0..classified.len())
        .filter(|pos| live(*pos))
        .map(|pos| pos as u32)
        .collect();
    let mut queued: Vec<bool> = (0..classified.len()).map(live).collect();
    while !queue.is_empty() {
        // Every member of a wave reads the same settled snapshot. Its own
        // result and newly discovered argument edges are collected locally;
        // the merge below is serial, so no two workers write shared state.
        let wave: Vec<u32> = queue.drain(..).collect();
        for pos in &wave {
            queued[*pos as usize] = false;
        }
        let wave_number = WAVES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let updates: Vec<(u32, Settled, SmallVec<[EffectIx; 4]>)> = {
            let get = |callee: EffectIx| match slots
                .get(callee)
                .and_then(|pos| settled[pos].as_ref())
            {
                Some(found) => std::borrow::Cow::Borrowed(found),
                None => match outside_cache.get(callee.index()) {
                    Some(cell) => std::borrow::Cow::Borrowed(cell.get_or_init(|| outside(callee))),
                    None => std::borrow::Cow::Owned(outside(callee)),
                },
            };
            wave.par_iter()
                .with_min_len(crate::registry::PASS_CHUNK)
                .filter_map(|&pos| {
                    let classification = &classified[pos as usize].1;
                    let before = settled[pos as usize].as_ref()?;
                    if before.effect.is_anything() {
                        return None;
                    }
                    let mut now = before.clone();
                    let mut read: SmallVec<[EffectIx; 4]> = SmallVec::new();
                    for &(callee, transfer, site) in &classification.calls {
                        let found = get(callee);
                        let transfer = transfers.get(transfer);
                        now.effect.join_transferred(&found.effect, transfer);
                        for condition in &found.runs {
                            if callee.is_closure() {
                                // What a closure the body wrote runs of the
                                // body's parameters is the body's to run.
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
                    for set in &classification.sets {
                        if now.effect.is_anything() {
                            break;
                        }
                        let joined = set.joined(wave_number, transfers, &get);
                        now.effect.join(&joined.effect);
                        now.runs.extend(joined.runs.iter().cloned());
                        if joined.unknown {
                            now.effect = Effect::unknown();
                        }
                    }
                    Some((pos, now, read))
                })
                .collect()
        };

        for (pos, now, read) in updates {
            for found in read {
                if let Some(found) = slots.get(found)
                    && passed_to.insert((found as u32, pos))
                {
                    callers[found].push(pos);
                    // The callee may have advanced earlier in this same
                    // wave, after this caller read the wave's old snapshot.
                    // Revisit the caller once the new edge is visible.
                    if !std::mem::replace(&mut queued[pos as usize], true) {
                        queue.push_back(pos);
                    }
                }
            }
            if settled[pos as usize].as_ref() == Some(&now) {
                continue;
            }
            settled[pos as usize] = Some(now);
            for &caller in &callers[pos as usize] {
                if !std::mem::replace(&mut queued[caller as usize], true) {
                    queue.push_back(caller);
                }
            }
        }
    }
    Settlements {
        slots,
        ixs: classified.iter().map(|(ix, _)| *ix).collect(),
        settled,
    }
}

/// What a caller holding a set of declarations adds to its own effect: what
/// each may write, as its own `$this` is to the caller, and the conditions
/// of the closures among them; and whether one runs a parameter's argument,
/// which no call passes.
#[derive(Debug)]
struct SetJoin {
    effect: Effect,
    runs: BTreeSet<Condition>,
    unknown: bool,
}

impl SetJoin {
    fn of<'g>(
        set: &[(EffectIx, TransferIx)],
        transfers: &TransferTable,
        get: &impl Fn(EffectIx) -> std::borrow::Cow<'g, Settled>,
    ) -> SetJoin {
        let mut joined = SetJoin {
            effect: Effect::nothing(),
            runs: BTreeSet::new(),
            unknown: false,
        };
        for &(callee, transfer) in set {
            let transfer = transfers.get(transfer);
            let found = get(callee);
            joined.effect.join_transferred(&found.effect, transfer);
            for condition in &found.runs {
                if callee.is_closure() {
                    joined.runs.insert(condition.through(transfer));
                } else {
                    joined.unknown = true;
                }
            }
            if joined.effect.is_anything() {
                break;
            }
        }
        joined
    }
}

/// Adds to `into` what a callee (`callee`, its own `$this` what `transfer`
/// says) running its argument at the parameter `condition` names may write,
/// as `site`, the call, passes it: `get` gives what a declaration may write,
/// and `read` gets each one read.
#[allow(clippy::too_many_arguments)]
fn run_passed<'g>(
    registry: &Registry,
    site: Option<&Site<'_>>,
    callee: EffectIx,
    transfer: &Transfer,
    condition: &Condition,
    get: &impl Fn(EffectIx) -> std::borrow::Cow<'g, Settled>,
    into: &mut Settled,
    read: &mut SmallVec<[EffectIx; 4]>,
) {
    let Some(site) = site else {
        into.effect = Effect::unknown();
        return;
    };
    let Some(callee_key) = registry.effect_decl_key(registry.reach_memo.effects.id(callee)) else {
        into.effect = Effect::unknown();
        return;
    };
    let default;
    let passing = if condition.destroys {
        registry.passing_destroyed(site.call, &callee_key, condition.at)
    } else {
        registry.passing(site.call, &callee_key, condition.at)
    };
    let (runs, params) = match passing {
        Passing::Argument(at) => match site.passed.iter().find(|(passed, _)| *passed == at) {
            Some((_, passed)) if condition.destroys => match &passed.destroys {
                Some(destroys) => (&destroys[..], &[][..]),
                None => {
                    into.effect = Effect::unknown();
                    return;
                }
            },
            Some((_, passed)) => (&passed.runs[..], &passed.params[..]),
            None => {
                into.effect = Effect::unknown();
                return;
            }
        },
        Passing::Default(runs) => {
            // A default is no value the caller holds.
            default = if condition.destroys {
                RunList::new()
            } else {
                runs
            };
            (&default[..], &[][..])
        }
        Passing::Unknown => {
            into.effect = Effect::unknown();
            return;
        }
    };
    let transfers = &registry.reach_memo.transfers;
    for &(run, own) in runs {
        let bound = condition.bound(transfer, transfers.get(own));
        let found = get(run);
        into.effect.join_transferred(&found.effect, &bound);
        if run.is_closure() {
            for inner in &found.runs {
                into.runs.insert(inner.through(&bound));
            }
        } else if !found.runs.is_empty() {
            // It runs its own parameters' arguments, which the callee passes.
            into.effect = Effect::unknown();
        }
        read.push(run);
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
            destroys: false,
        });
    }
}

/// How many arrays deep the elements of a place are followed for the objects
/// they hold.
const ELEMENT_DEPTH: u32 = 8;

/// The classes `ty` may be an object of: `Some` with each one named, `None`
/// when it may be an object of a class not named (`object`, `mixed`).
pub(crate) fn object_classes(ty: &Type) -> Option<NameList> {
    let members = ty.union_members();
    let mut classes = NameList::new();
    for member in members {
        if let Some((class, _)) = member.as_object() {
            classes.push(Name::from(class));
        } else if !member.is_object().no() {
            return None;
        }
    }
    Some(classes)
}

/// The classes of the objects a value of type `ty` may hold, in the arrays
/// it is or holds to any depth: `Some` with each one named, `None` when one
/// may be of a class not named (`object`, `mixed`, an iterable).
pub(crate) fn element_classes(ty: &Type) -> Option<NameList> {
    fn collect(ty: &Type, out: &mut NameList, depth: u32) -> Option<()> {
        if depth > 8 {
            return None;
        }
        for member in ty.union_members() {
            if member.is_array().yes() {
                collect(&member.array_value_type()?, out, depth + 1)?;
            } else if let Some((class, _)) = member.as_object() {
                out.push(Name::from(class));
            } else if !member.is_object().no() {
                return None;
            }
        }
        Some(())
    }
    let mut classes = NameList::new();
    collect(ty, &mut classes, 0)?;
    Some(classes)
}

/// The holder of the objects a value of type `ty` holds in its elements.
pub(crate) fn elements_holder_of(ty: &Type) -> Holder {
    match element_classes(ty) {
        Some(classes) => Holder::Classes(sorted_classes(classes)),
        None => Holder::Unknown,
    }
}

/// The object a value may be.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Holder {
    /// An object of one of the classes, or of a descendant of one.
    Classes(Arc<[Name]>),
    /// An object the caller made, no one else has: of exactly one of the
    /// classes, or of a class not known.
    Made(Option<Arc<[Name]>>),
    /// An object of any class.
    Unknown,
}

impl Holder {
    /// The holder of no object.
    pub(crate) fn none() -> Holder {
        Holder::Classes(Holder::none_classes())
    }

    fn none_classes() -> Arc<[Name]> {
        static NONE: std::sync::LazyLock<Arc<[Name]>> = std::sync::LazyLock::new(|| Arc::from([]));
        Arc::clone(&NONE)
    }

    /// The holder of an object of one of `classes`, in the order given.
    pub(crate) fn listed(classes: NameList) -> Holder {
        Holder::Classes(shared_names(classes))
    }

    /// The holder of an object of `class`.
    pub(crate) fn single(class: Name) -> Holder {
        Holder::Classes(Arc::from([class]))
    }

    /// The object a value either holder describes may be.
    pub(crate) fn join(self, other: Holder) -> Holder {
        match (self, other) {
            (Holder::Classes(none), other) | (other, Holder::Classes(none)) if none.is_empty() => {
                other
            }
            (Holder::Unknown | Holder::Made(None), _)
            | (_, Holder::Unknown | Holder::Made(None)) => Holder::Unknown,
            (Holder::Made(Some(a)), Holder::Made(Some(b))) => {
                Holder::Made(Some(merged_classes(a, b)))
            }
            (
                Holder::Classes(a) | Holder::Made(Some(a)),
                Holder::Classes(b) | Holder::Made(Some(b)),
            ) => Holder::Classes(merged_classes(a, b)),
        }
    }
}

/// `classes` in one order, each once, as PHP compares class names.
fn sorted_classes(mut classes: NameList) -> Arc<[Name]> {
    classes.sort_unstable_by(|a, b| a.lower().cmp(b.lower()));
    classes.dedup_by(|a, b| a.eq_ignore_case(b));
    shared_names(classes)
}

/// `classes` as a shared list: the list of no class is one allocation for
/// all, and a list of one or two moves its names in without a vector.
fn shared_names(classes: NameList) -> Arc<[Name]> {
    let mut names = classes.into_iter();
    match (names.next(), names.next()) {
        (None, _) => Holder::none_classes(),
        (Some(a), None) => Arc::from([a]),
        (Some(a), Some(b)) => {
            let rest = names.len();
            if rest == 0 {
                Arc::from([a, b])
            } else {
                let mut all = Vec::with_capacity(2 + rest);
                all.push(a);
                all.push(b);
                all.extend(names);
                all.into()
            }
        }
    }
}

/// The classes of two lists in one, as `sorted_classes` has them; a list
/// already in that order that holds the other's is returned as it is.
fn merged_classes(a: Arc<[Name]>, b: Arc<[Name]>) -> Arc<[Name]> {
    let holds = |all: &[Name], some: &[Name]| {
        some.iter()
            .all(|class| all.iter().any(|have| have.eq_ignore_case(class)))
    };
    let ordered = |all: &[Name]| all.windows(2).all(|pair| pair[0].lower() < pair[1].lower());
    if Arc::ptr_eq(&a, &b) && ordered(&a) {
        return a;
    }
    if ordered(&a) && holds(&a, &b) {
        return a;
    }
    if ordered(&b) && holds(&b, &a) {
        return b;
    }
    let mut all = NameList::with_capacity(a.len() + b.len());
    all.extend(a.iter().cloned());
    all.extend(b.iter().cloned());
    sorted_classes(all)
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
        Some(classes) => Holder::listed(classes),
        None => Holder::Unknown,
    }
}

/// The holder of a value of the type `ty` declared on a member of `owner`,
/// reached through an object of `class`.
fn declared_holder(
    registry: &Registry,
    ty: &Type,
    class: &str,
    owner: &Name,
    of: fn(&Type) -> Holder,
) -> Holder {
    let mut ty = ty.bind_static(class).bind_self(owner);
    if let Some(parent) = registry
        .class_references_by_name(owner)
        .and_then(|references| references.extends.first())
    {
        ty = ty.bind_parent(parent);
    }
    of(&ty)
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
    /// The file the calling code is in, when it is known.
    fn file(&self) -> Option<FileId>;
    /// The object a method is called on, and whether it is the caller's own
    /// `$this`.
    fn receiver(&self, receiver: &Receiver) -> (Holder, bool);
    /// The object the argument passed `at` is; `place` is what it names,
    /// when it names one.
    fn argument(&self, at: &ArgAt, place: Option<&Place>) -> Holder;
    /// The objects the elements of the argument passed `at` may be, at any
    /// depth of arrays; `Unknown` when the caller cannot tell.
    fn elements(&self, _at: &ArgAt, _place: Option<&Place>) -> Holder {
        Holder::Unknown
    }
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
    /// The name of the class the calling code belongs to.
    fn own_class(&self) -> Option<Name> {
        self.class().map(Name::from)
    }
    /// The lowercase name of the class the calling code belongs to.
    fn scope_name(&self) -> Option<Name> {
        self.class()
            .map(|class| Name::from(class).lowered().clone())
    }
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
    own: Option<&Name>,
    class: &QualifiedName,
) -> Option<NameList> {
    Some(if class.is_keyword("self") {
        smallvec![own?.clone()]
    } else if class.is_keyword("parent") {
        smallvec![
            registry
                .class_references_by_name(own?.as_str())?
                .extends
                .first()?
                .clone(),
        ]
    } else if class.is_keyword("static") {
        let own = own?;
        std::iter::once(own.clone())
            .chain(registry.subclasses(own.as_str()))
            .collect()
    } else {
        class.resolved_class_name().cloned().into_iter().collect()
    })
}

/// Finds the declarations a call runs, with what its caller knows of it.
struct Reach<'r> {
    registry: &'r Registry,
    ctx: &'r dyn CallContext,
}

impl Reach<'_> {
    /// The destructors that run if the declaration a call reaches writes
    /// over the by-reference parameter its argument `at` is passed at: those
    /// of the objects the argument and its elements hold; `None` when they
    /// may be code not known.
    fn argument_destroys(&self, call: &CallWrites, at: &ArgAt) -> Option<RunList> {
        let Some((_, place)) = call.places.iter().find(|(passed, _)| passed == at) else {
            return Some(RunList::new());
        };
        let holder = self
            .ctx
            .argument(at, Some(place))
            .join(self.ctx.elements(at, Some(place)));
        if holder == Holder::none() {
            return Some(RunList::new());
        }
        let targets = self.implicit_holder_targets(holder, false, &Operation::Destroy)?;
        Some(targets.into_list())
    }

    /// The declarations the argument of `call` passed `at` runs when it is
    /// called, each with what its own `$this` is to the caller, `transfer`
    /// for a closure the caller wrote; `None` when it may run code not
    /// known. What the caller wrote is noted (`CallContext::runs_written`).
    fn argument_runs(&self, call: &CallWrites, at: &ArgAt, transfer: &Transfer) -> Option<RunList> {
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
                self.ctx.runs_written(written, transfer).then(RunList::new)
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
    ) -> Option<BuiltinRuns> {
        let registry = self.registry;
        let scope = match transfer {
            Transfer::Same => self.ctx.own_class(),
            _ => None,
        };
        let base = registry
            .reach_memo
            .builtin_base
            .get_or_compute((key.clone(), transfer.clone(), scope), || {
                self.compute_builtin_base(key, sig, transfer)
            });
        #[cfg(debug_assertions)]
        {
            let again = self.compute_builtin_base(key, sig, transfer);
            debug_assert_eq!(
                base.as_ref().map(|b| (&b.prefix, &b.takes_callable)),
                again.as_ref().map(|b| (&b.prefix, &b.takes_callable)),
            );
        }
        let base = base?;
        let runs: &[(Position, BuiltinRun)] = &base.runs;
        let mut out = RunList::new();
        // Each parameter a callable is passed at runs it.
        for (at, param) in sig.params.iter().enumerate() {
            let at = at as u32;
            if !base
                .takes_callable
                .get(at as usize)
                .copied()
                .unwrap_or(false)
            {
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
                        Some(arg) => {
                            let place = call
                                .places
                                .iter()
                                .find(|(at, _)| at == arg)
                                .map(|(_, place)| place);
                            if place.is_none() && call.objectless.contains(arg) {
                                Holder::none()
                            } else if position.elements {
                                self.ctx.elements(arg, place)
                            } else {
                                self.ctx.argument(arg, place)
                            }
                        }
                        None => Holder::Unknown,
                    };
                    out.extend(self.method_targets_on(&holder, method));
                }
                BuiltinRun::Wrapper(method) => {
                    out.extend(registry.wrapper_runs(method).iter().copied())
                }
            }
        }
        Some(BuiltinRuns {
            prefix: Arc::clone(&base.prefix),
            added: out,
        })
    }

    /// What a call of the builtin `key` runs whatever its arguments are:
    /// the table's runs, the stream-wrapper methods it holds, and the
    /// methods a class extending it declares. `None` when it may run code
    /// not known.
    fn compute_builtin_base(
        &self,
        key: &DeclKey,
        sig: &FunctionReflection,
        transfer: &Transfer,
    ) -> Option<Arc<BuiltinBase>> {
        let registry = self.registry;
        let entry = registry.builtin_entry(key);
        // A stub no run of PHP has seen runs what its parameters let it run.
        let runs: Arc<[(Position, BuiltinRun)]> = match &entry {
            Some(name) => builtin_runs::runs(name).into(),
            None => {
                let takes_code = sig
                    .params
                    .iter()
                    .any(|p| p.ty.mentions_any(&CODE_RUNNING_PARAMETER_TYPES));
                if sig.declared_pure && !takes_code {
                    return Some(Arc::new(BuiltinBase {
                        runs: Arc::from([]),
                        prefix: Arc::from([]),
                        takes_callable: Arc::from([]),
                    }));
                }
                derived_runs(sig).into()
            }
        };
        // A method of a class whose objects run what they were made with may
        // run anything; the constructor that is making one runs what the
        // table says it runs, and nothing of what it keeps.
        if &*key.member != "__construct"
            && match &entry {
                Some(name) => builtin_runs::holding(name),
                None => key
                    .class
                    .as_deref()
                    .is_some_and(|class| registry.stub_holds_code(class)),
            }
        {
            return None;
        }
        let mut out = Vec::new();
        if let Some(name) = &entry {
            for method in builtin_runs::held_wrappers(name) {
                out.extend(registry.wrapper_runs(method).iter());
            }
        }
        if let Some(owner) = &key.class {
            let receivers: Vec<Name> = match transfer {
                Transfer::Same => self.ctx.own_class().into_iter().collect(),
                Transfer::Other(Some(class)) => vec![class.clone()],
                Transfer::Other(None) | Transfer::Made | Transfer::Static => {
                    vec![owner.clone()]
                }
            };
            for receiver in &receivers {
                out.extend(
                    registry
                        .overrides_of_builtin(receiver, owner)
                        .iter()
                        .map(|key| registry.effect_run(key, transfer)),
                );
            }
        }
        let takes_callable = sig
            .params
            .iter()
            .enumerate()
            .map(|(at, param)| {
                param.ty.mentions_callable()
                    || runs.iter().any(|(position, run)| {
                        position.at == at as u32 && matches!(run, BuiltinRun::Calls)
                    })
            })
            .collect();
        Some(Arc::new(BuiltinBase {
            runs,
            prefix: out.into(),
            takes_callable,
        }))
    }

    /// The declarations calling `method` on an object `holder` says may be
    /// run, PHP calling it: by the hierarchy over its classes when each has
    /// the method, and every loaded declaration of it otherwise, since the
    /// method may run on an object the first one gave back. Nothing for a
    /// value that is no object.
    fn method_targets_on(&self, holder: &Holder, method: &str) -> RunList {
        let registry = self.registry;
        let transfers = &registry.reach_memo.transfers;
        let has_method = |class: &Name| {
            registry.lookup_method(class, method).is_some()
                || registry.lookup_method(class, &well_known().call).is_some()
        };
        match holder {
            Holder::Classes(classes) if classes.is_empty() => RunList::new(),
            Holder::Classes(classes) if classes.iter().all(has_method) => {
                let mut out = RunList::new();
                for class in classes.iter() {
                    let transfer = transfers.intern(&Transfer::Other(Some(class.clone())));
                    out.extend(
                        registry
                            .implementation_ixs(class, method)
                            .iter()
                            .map(|&ix| (ix, transfer)),
                    );
                }
                out
            }
            Holder::Made(Some(classes)) if classes.iter().all(has_method) => classes
                .iter()
                .filter_map(|class| self.exact_method(class, method))
                .map(|key| (registry.effect_ix(&key), TransferTable::MADE))
                .collect(),
            _ => registry
                .methods_named_ixs(method)
                .iter()
                .map(|&ix| (ix, TransferTable::ANY))
                .collect(),
        }
    }

    /// The declarations a callable the body names runs.
    fn callable_targets(&self, callable: &Callable) -> Option<RunList> {
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
                    .iter()
                    .map(|class| Transfer::Other(Some(class.clone())))
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
    fn call_targets(&self, callee: &Callee, call: Option<&CallWrites>) -> Option<RunList> {
        let registry = self.registry;
        let transfers = &registry.reach_memo.transfers;
        Some(match callee {
            Callee::Function(name) => match registry.resolve_function(name) {
                Some((resolved, _)) => {
                    let declared = registry
                        .lookup_function_with_name(resolved)
                        .map_or(resolved, |(declared, _)| declared);
                    smallvec![(
                        registry.effect_ix(&function_key(declared)),
                        TransferTable::STATIC,
                    )]
                }
                // A call to no function throws.
                None => RunList::new(),
            },
            Callee::Method {
                receiver: Receiver::Variable(name),
                method: Some(method),
            } if self.runs_held_closure(name, method, call) => RunList::new(),
            Callee::Method { receiver, method } => {
                let (holder, same) = self.ctx.receiver(receiver);
                let transfer = |class: Option<&Name>| {
                    transfers.intern(&if same {
                        Transfer::Same
                    } else if matches!(holder, Holder::Made(_)) {
                        Transfer::Made
                    } else {
                        Transfer::Other(class.cloned())
                    })
                };
                let mut out = RunList::new();
                match (&holder, method) {
                    (Holder::Classes(classes), Some(method)) => {
                        for class in classes.iter() {
                            let transfer = transfer(Some(class));
                            out.extend(
                                registry
                                    .implementation_ixs(class, method)
                                    .iter()
                                    .map(|&ix| (ix, transfer)),
                            );
                        }
                    }
                    (Holder::Made(Some(classes)), Some(method)) => {
                        for class in classes.iter() {
                            if let Some(key) = self.exact_method(class, method) {
                                out.push((registry.effect_ix(&key), transfer(Some(class))));
                            }
                        }
                    }
                    (Holder::Classes(classes) | Holder::Made(Some(classes)), None) => {
                        for class in classes.iter() {
                            let transfer = transfer(Some(class));
                            out.extend(
                                self.every_method_ixs(class)
                                    .iter()
                                    .map(|&ix| (ix, transfer)),
                            );
                        }
                    }
                    (Holder::Unknown | Holder::Made(None), Some(method)) => {
                        let transfer = transfer(None);
                        out.extend(
                            registry
                                .methods_named_ixs(method)
                                .iter()
                                .map(|&ix| (ix, transfer)),
                        );
                    }
                    (Holder::Unknown | Holder::Made(None), None) => return None,
                }
                out
            }
            Callee::Static { class, method } => match class {
                Some(class) => self.static_targets(class, method.as_ref())?,
                None => registry
                    .methods_named_ixs(method.as_ref()?)
                    .iter()
                    .map(|&ix| (ix, TransferTable::STATIC))
                    .collect(),
            },
            Callee::New(None) => registry
                .every_constructor()
                .iter()
                .map(|key| (registry.effect_ix(key), TransferTable::MADE))
                .collect(),
            Callee::New(class) => {
                let classes = match class {
                    Some(class) => {
                        named_classes(self.registry, self.ctx.own_class().as_ref(), class)
                            .unwrap_or_default()
                    }
                    None => NameList::new(),
                };
                classes
                    .iter()
                    .filter_map(|class| {
                        registry.lookup_method_with_owner(class, &well_known().construct)
                    })
                    .map(|(owner, _)| {
                        (
                            registry.effect_ix(&method_key(&owner, &well_known().construct)),
                            TransferTable::MADE,
                        )
                    })
                    .collect()
            }
            Callee::Anonymous(name) => name
                .as_ref()
                .and_then(|name| registry.lookup_method_with_owner(name, &well_known().construct))
                .map(|(owner, _)| {
                    (
                        registry.effect_ix(&method_key(&owner, &well_known().construct)),
                        TransferTable::MADE,
                    )
                })
                .into_iter()
                .collect(),
            Callee::Value(variable) => {
                let written = Written::Variable(variable.as_ref()?);
                if !self.ctx.runs_written(written, &Transfer::Same) {
                    return None;
                }
                RunList::new()
            }
            Callee::Written(ids) => {
                if !self.ctx.runs_written(Written::Callee(ids), &Transfer::Same) {
                    return None;
                }
                RunList::new()
            }
            Callee::Implicit { on, operation } => self
                .implicit_call_targets(on.as_deref(), operation)?
                .into_list(),
            // The file's top level runs in the including code's scope, as its
            // own `$this`; a file that is not known is code of any content.
            Callee::Include(at) => {
                let key = match self
                    .ctx
                    .file()
                    .and_then(|file| registry.include_target(file, *at))
                    .flatten()
                {
                    Some(target) => DeclKey::file(target),
                    None => DeclKey::unresolved_include(),
                };
                smallvec![(registry.effect_ix(&key), TransferTable::SAME)]
            }
            Callee::ParentHook { property, set } => {
                let parent = registry
                    .class_references_by_name(self.ctx.class()?)?
                    .extends
                    .first()?;
                registry
                    .hook_owner(parent, property, *set)
                    .map(|owner| {
                        (
                            registry.effect_ix(&hook_key(&owner, property, *set)),
                            TransferTable::SAME,
                        )
                    })
                    .into_iter()
                    .collect()
            }
        })
    }

    /// The declarations PHP runs for `operation` on the operand `on`, found
    /// once for each holder, operation and, where it decides what is seen,
    /// scope asked about.
    fn implicit_call_targets(
        &self,
        on: Option<&[Origin]>,
        operation: &Operation,
    ) -> Option<Targets> {
        let (holder, same) = self.ctx.operand(on);
        self.implicit_holder_targets(holder, same, operation)
    }

    /// `implicit_call_targets` for an operand already found to be `holder`.
    fn implicit_holder_targets(
        &self,
        holder: Holder,
        same: bool,
        operation: &Operation,
    ) -> Option<Targets> {
        let (scope, rebindable) = (self.ctx.scope_name(), self.ctx.rebindable());
        let scoped = |hooks: Option<bool>| ImplicitKey {
            holder: holder.clone(),
            same,
            operation: OperationKey::of(operation),
            hooks,
            scope: scope.clone(),
            rebindable,
        };
        let memo = &self.registry.reach_memo.implicit;
        // The hooks the scope sees come first, and are found alone: most
        // programs have none, and then the rest does not depend on the
        // scope but for a property's.
        let hooks = match hook_access(operation) {
            Some((property, set, iterate)) => memo
                .get_or_compute(scoped(Some(true)), || {
                    let mut out = TargetSet::default();
                    self.hook_targets(&holder, same, operation, property, set, iterate, &mut out);
                    Some(out.into_shared())
                })
                .unwrap_or_default(),
            None => SharedTargets::default(),
        };
        let mut key = scoped(Some(false));
        if hooks.is_empty() && !matches!(operation, Operation::Property(..)) {
            key.scope = None;
            key.rebindable = false;
        }
        let list = memo.get_or_compute(key, || {
            if hooks.is_empty() && matches!(holder, Holder::Unknown | Holder::Made(None)) {
                let shared = self.unknown_targets(&holder, same, operation);
                #[cfg(debug_assertions)]
                {
                    let mut pushed = TargetSet::default();
                    self.implicit_targets(&holder, same, operation, &mut pushed, &mut Vec::new());
                    debug_assert_eq!(pushed.into_list()[..], shared[..]);
                }
                return Some(shared);
            }
            let mut out = TargetSet::default();
            for run in hooks.iter() {
                out.push(*run);
            }
            self.implicit_targets(&holder, same, operation, &mut out, &mut Vec::new())?;
            Some(out.into_shared())
        })?;
        Some(Targets::Shared(list))
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
        out: &mut TargetSet,
    ) {
        let registry = self.registry;
        let transfer = |class: Option<&Name>| implicit_transfer(holder, same, operation, class);
        let (classes, exact) = match holder {
            Holder::Classes(classes) => (Some(classes), false),
            Holder::Made(Some(classes)) => (Some(classes), true),
            Holder::Unknown | Holder::Made(None) => (None, false),
        };
        let member = registry
            .reach_memo
            .hook_members
            .get_or_compute((property.cloned(), set), || {
                Name::from(hook_member(property.map(Name::as_str), set))
            });
        for declaring in registry.classes_declaring(member.as_str()) {
            if registry.class(declaring).is_none() {
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
            let declarations = registry
                .reach_memo
                .hook_declarations
                .get_or_compute((declaring.clone(), property.cloned(), set), || {
                    registry.hook_declarations(declaring, property, set).into()
                });
            for hook in declarations.iter() {
                let (name, visibility, private) = (&hook.name, hook.visibility, hook.private);
                if !(self.ctx.rebindable()
                    || sees(registry, declaring, visibility, self.ctx.class()))
                {
                    continue;
                }
                let key = hook_key(declaring, name, set);
                let ix = registry.effect_ix(&key);
                let Some(classes) = classes else {
                    out.push((ix, registry.reach_memo.transfers.intern(&transfer(None))));
                    continue;
                };
                for (class, related) in classes.iter().zip(&related) {
                    // An object always of the declaring class runs its hook
                    // when that is the nearest its class has, or when the
                    // property is private, the scope's own; one that may be
                    // of a class below both may run it.
                    let reached = match related {
                        Some(true) => {
                            private
                                || registry
                                    .hook_owner(class, name, set)
                                    .is_some_and(|owner| owner.eq_ignore_case(declaring))
                        }
                        Some(false) => true,
                        None => false,
                    };
                    if reached {
                        out.push((
                            ix,
                            registry.reach_memo.transfers.intern(&transfer(Some(class))),
                        ));
                    }
                }
            }
        }
    }

    /// The declarations PHP runs for `operation` on an object of any class,
    /// each once.
    fn unknown_implicit_ixs(&self, operation: &Operation) -> Arc<[EffectIx]> {
        self.registry.reach_memo.unknown_implicit.get_or_compute(
            self.unknown_scope(operation),
            || {
                self.compute_unknown_implicit_keys(operation)
                    .iter()
                    .map(|key| self.registry.effect_ix(key))
                    .collect()
            },
        )
    }

    /// What the declarations PHP runs for `operation` on an object of any
    /// class depend on: the operation and, for a property's, which the scope
    /// sees.
    fn unknown_scope(&self, operation: &Operation) -> (OperationKey, Option<Name>, bool) {
        if matches!(operation, Operation::Property(..)) {
            (
                OperationKey::of(operation),
                self.ctx.scope_name(),
                self.ctx.rebindable(),
            )
        } else {
            (OperationKey::of(operation), None, false)
        }
    }

    /// The declarations PHP runs for `operation` on an object of any class,
    /// each with the `$this` the holder says, found once for the calls that
    /// ask the same.
    fn unknown_targets(&self, holder: &Holder, same: bool, operation: &Operation) -> SharedTargets {
        let transfer = implicit_transfer(holder, same, operation, None);
        let (operation_key, scope, rebindable) = self.unknown_scope(operation);
        self.registry.reach_memo.unknown_targets.get_or_compute(
            (operation_key, scope, rebindable, transfer.clone()),
            || {
                let transfer = self.registry.reach_memo.transfers.intern(&transfer);
                self.unknown_implicit_ixs(operation)
                    .iter()
                    .map(|&ix| (ix, transfer))
                    .collect()
            },
        )
    }

    fn compute_unknown_implicit_keys(&self, operation: &Operation) -> Vec<DeclKey> {
        let registry = self.registry;
        let mut out: Vec<DeclKey> = Vec::new();
        let mut seen: FxHashSet<DeclKey> = FxHashSet::default();
        for method in implicit_method_names(operation) {
            for class in registry
                .classes_declaring(method)
                .chain(registry.builtin_classes_running_code(method))
            {
                if !self.implicit_methods(class, operation).contains(&method) {
                    continue;
                }
                // A builtin destructor frees what its object holds
                // and runs no code of its own.
                if matches!(operation, Operation::Destroy) && registry.is_builtin_class(class) {
                    continue;
                }
                if let Some(key) = self.implicit_method(class, method)
                    && seen.insert(key.clone())
                {
                    out.push(key);
                }
            }
        }
        out
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
        out: &mut TargetSet,
        iterators: &mut Vec<Holder>,
    ) -> Option<()> {
        let registry = self.registry;
        let transfer = |class: Option<&Name>| implicit_transfer(holder, same, operation, class);
        let (classes, exact) = match holder {
            Holder::Classes(classes) => (classes, false),
            Holder::Made(Some(classes)) => (classes, true),
            Holder::Unknown | Holder::Made(None) => {
                let transfer = registry.reach_memo.transfers.intern(&transfer(None));
                for &ix in self.unknown_implicit_ixs(operation).iter() {
                    out.push((ix, transfer));
                }
                return Some(());
            }
        };
        for class in classes.iter() {
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
                for &method in self.implicit_methods(candidate, operation).iter() {
                    if let Some(key) = self.implicit_method(candidate, method) {
                        out.push(registry.effect_run(&key, &transfer(Some(class))));
                    }
                }
                if matches!(operation, Operation::Destroy) {
                    let held = self.destroy_held(candidate);
                    if matches!(&held, Holder::Classes(classes) if classes.is_empty())
                        || iterators.contains(&held)
                    {
                        continue;
                    }
                    iterators.push(held.clone());
                    // Objects holding objects this deep drop as any object.
                    let held = if iterators.len() > 16 {
                        Holder::Unknown
                    } else {
                        held
                    };
                    self.implicit_targets(&held, false, operation, out, iterators)?;
                    continue;
                }
                if !matches!(operation, Operation::Iterate { .. })
                    || registry.is_subclass_of(candidate, "Iterator")
                    || !registry.is_subclass_of(candidate, "IteratorAggregate")
                {
                    continue;
                }
                let given = match registry
                    .lookup_method_with_owner(candidate, &well_known().get_iterator)
                {
                    Some((owner, sig)) => {
                        declared_holder(registry, sig.return_type(), candidate, &owner, holder_of)
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

    /// The objects an object of exactly `class` holds in its instance
    /// properties, by their declared types, which die with it when nothing
    /// else holds them.
    fn destroy_held(&self, class: &Name) -> Holder {
        let registry = self.registry;
        let mut found = NameList::new();
        let names: Vec<&Name> = match registry.property_owners(class) {
            Some(owners) => owners.keys().collect(),
            None => registry.property_names(class).collect(),
        };
        for property in names {
            let Some((owner, declared)) = registry.lookup_property_with_owner(class, property)
            else {
                return Holder::Unknown;
            };
            if declared.is_static() {
                continue;
            }
            match declared_holder(registry, declared.ty(), class, &owner, elements_holder_of) {
                Holder::Classes(held) => found.extend(held.iter().cloned()),
                _ => return Holder::Unknown,
            }
        }
        Holder::listed(found)
    }

    /// The methods PHP calls for `operation` on an object of exactly
    /// `class`.
    fn implicit_methods(&self, class: &Name, operation: &Operation) -> Arc<[&'static Name]> {
        // Only a property's depends on the scope that asks.
        if matches!(operation, Operation::Property(..)) {
            return self.compute_implicit_methods(class, operation).into();
        }
        self.registry.reach_memo.implicit_methods.get_or_compute(
            (class.lowered().clone(), OperationKey::of(operation)),
            || self.compute_implicit_methods(class, operation).into(),
        )
    }

    fn compute_implicit_methods(&self, class: &Name, operation: &Operation) -> Vec<&'static Name> {
        let registry = self.registry;
        let known = well_known();
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
            Operation::String => vec![&known.to_string],
            Operation::Destroy => vec![&known.destruct],
            Operation::Clone { with: false } => vec![&known.clone],
            Operation::Clone { with: true } => vec![&known.clone, &known.set],
            Operation::Iterate { keys } => {
                if registry.is_subclass_of(class, "Iterator") {
                    let mut methods =
                        vec![&known.rewind, &known.valid, &known.current, &known.next];
                    if *keys {
                        methods.push(&known.key);
                    }
                    methods
                } else if registry.is_subclass_of(class, "IteratorAggregate") {
                    vec![&known.get_iterator]
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// The declaration PHP runs calling `method` on an object of exactly
    /// `class` itself: none when the class has no body for it, or only a
    /// `@method` tag or an abstract method declares it.
    fn implicit_method(&self, class: &Name, method: &Name) -> Option<DeclKey> {
        let mut computed = false;
        let found = self.registry.reach_memo.implicit_method.get_or_compute(
            (class.lowered().clone(), method.lowered().clone()),
            || {
                computed = true;
                self.compute_implicit_method(class, method)
            },
        );
        stats::count(if computed {
            Counter::ImplicitMethodMiss
        } else {
            Counter::ImplicitMethodHit
        });
        found
    }

    fn compute_implicit_method(&self, class: &Name, method: &Name) -> Option<DeclKey> {
        let registry = self.registry;
        let (owner, sig) = registry.lookup_method_with_owner(class, method)?;
        let (_, _, info) = registry.class(&owner)?;
        let abstract_method =
            info.kind() == ClassKind::Interface || registry.method_is_abstract(&owner, method);
        (!sig.is_magic && !abstract_method).then(|| method_key(&owner, method))
    }

    /// The declaration a call of `method` runs on an object of exactly
    /// `class`: its own, or `__call` when it has none.
    fn exact_method(&self, class: &str, method: &(impl KeyName + ?Sized)) -> Option<DeclKey> {
        let registry = self.registry;
        match registry.lookup_method_with_owner(class, method.text()) {
            Some((owner, sig)) => Some(registry.reached(&owner, method, sig)),
            None => registry
                .lookup_method_with_owner(class, &well_known().call)
                .map(|(owner, _)| method_key(&owner, &well_known().call)),
        }
    }

    /// Every method an object of `class`, or of a descendant, has: what a
    /// call with a computed name may run.
    fn every_method(&self, class: &(impl KeyName + ?Sized)) -> Arc<[DeclKey]> {
        self.registry
            .reach_memo
            .every_method
            .get_or_compute(class.lowered_name(), || {
                self.compute_every_method(class.text()).into()
            })
    }

    /// `every_method`, by handle.
    fn every_method_ixs(&self, class: &(impl KeyName + ?Sized)) -> Arc<[EffectIx]> {
        self.registry
            .reach_memo
            .every_method_ixs
            .get_or_compute(class.lowered_name(), || {
                self.every_method(class)
                    .iter()
                    .map(|key| self.registry.effect_ix(key))
                    .collect()
            })
    }

    fn compute_every_method(&self, class: &str) -> Vec<DeclKey> {
        let registry = self.registry;
        let mut out = Vec::new();
        let mut seen: FxHashSet<DeclKey> = FxHashSet::default();
        for candidate in std::iter::once(Name::from(class))
            .chain(registry.subclasses_shared(class).iter().cloned())
        {
            let Some(owners) = registry.method_owners(&candidate) else {
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
    fn static_targets(&self, class: &QualifiedName, method: Option<&Name>) -> Option<RunList> {
        let registry = self.registry;
        let classes = named_classes(registry, self.ctx.own_class().as_ref(), class)?;
        let forwards_from = |target: &str| {
            self.ctx.class().is_some_and(|own| {
                own.eq_ignore_ascii_case(target) || registry.is_subclass_of(own, target)
            })
        };
        let mut out = RunList::new();
        for target in &classes {
            let Some(method) = method else {
                out.extend(
                    self.every_method_ixs(target)
                        .iter()
                        .map(|&ix| (ix, TransferTable::STATIC)),
                );
                continue;
            };
            match registry.lookup_method_with_owner(target, method) {
                Some((owner, sig)) => {
                    let is_static = registry.method_is_static(&owner, method);
                    let transfer = if !is_static && forwards_from(target) {
                        TransferTable::SAME
                    } else {
                        TransferTable::STATIC
                    };
                    out.push((
                        registry.effect_ix(&registry.reached(&owner, method, sig)),
                        transfer,
                    ));
                }
                None => {
                    for magic in [&well_known().call_static, &well_known().call] {
                        if let Some((owner, _)) = registry.lookup_method_with_owner(target, magic) {
                            out.push((
                                registry.effect_ix(&method_key(&owner, magic)),
                                TransferTable::STATIC,
                            ));
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
    pub(crate) fn closure_effect(
        &self,
        summary: &BodySummary,
        class: Option<&str>,
        file: Option<FileId>,
        captured: Option<CapturedHolders<'_>>,
    ) -> Effect {
        let holder = DeclKey::new(
            class.map(|class| class.to_ascii_lowercase().into()),
            "".into(),
        );
        let resolve = Resolve {
            registry: self,
            class,
            member: Name::from("{closure}"),
            file,
            file_scope: false,
            summary,
            value: None,
            captured,
        };
        let classified: Vec<(EffectIx, Classification<'_>)> = resolve
            .run(&holder, None)
            .into_iter()
            .map(|(key, classification)| classification_by_id(self, key, classification))
            .collect();
        let settlements = settle(self, &classified, false, |ix| {
            self.effect_decl_key(self.reach_memo.effects.id(ix))
                .map_or_else(|| Settled::of(Effect::unknown()), |key| self.settled(&key))
        });
        settlements
            .get(self.effect_ix(&holder.closure(0)))
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
            return Effect::unknown();
        };
        let mut effect = Effect::nothing();
        for (ix, bound) in targets {
            let key = self.effect_key(ix);
            let transfer = self.transfer_of(bound);
            let runs = match self.builtin_of(ix) {
                Some(sig) => match reach.builtin_runs(key, sig, transfer, call) {
                    Some(runs) => runs,
                    None => return Effect::unknown(),
                },
                None => {
                    let fallback;
                    let settled = match self.settled_of(ix) {
                        Some(settled) => settled,
                        None => {
                            fallback = Settled::of(self.declaration_effect(ix));
                            &fallback
                        }
                    };
                    effect.join(&self.transferred_effect(ix, bound, true));
                    let mut runs = RunList::new();
                    for condition in &settled.runs {
                        let passing = if !passes_arguments(&call.callee, key) {
                            Passing::Unknown
                        } else if condition.destroys {
                            self.passing_destroyed(call, key, condition.at)
                        } else {
                            self.passing(call, key, condition.at)
                        };
                        let passed = match passing {
                            Passing::Argument(at) if condition.destroys => {
                                reach.argument_destroys(call, at)
                            }
                            Passing::Argument(at) => {
                                let written = condition.bound(transfer, &Transfer::Same);
                                reach.argument_runs(call, at, &written)
                            }
                            Passing::Default(_) if condition.destroys => Some(RunList::new()),
                            Passing::Default(runs) => Some(runs),
                            Passing::Unknown => None,
                        };
                        let Some(passed) = passed else {
                            return Effect::unknown();
                        };
                        runs.extend(passed.into_iter().map(|(run, own)| {
                            let bound = condition.bound(transfer, self.transfer_of(own));
                            (run, self.reach_memo.transfers.intern(&bound))
                        }));
                    }
                    BuiltinRuns::of(runs)
                }
            };
            for (run, bound) in runs.iter() {
                effect.join(&self.transferred_effect(run, bound, false));
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
    // The file the body is in, which says what its includes name; `None`
    // when it is not known.
    file: Option<FileId>,
    // Whether the body is a file's top level, whose variables are those of
    // the scope that includes it.
    file_scope: bool,
    summary: &'r BodySummary,
    // A `set` hook's parameter, by its name and the type of what it is
    // passed.
    value: Option<(&'r str, &'r Type)>,
    // What the code that wrote the closure this summary is of knows of the
    // object a variable it captures by value holds.
    captured: Option<CapturedHolders<'r>>,
}

/// What each of a scope's calls reaches, and the places its calls may write
/// by reference; `None` when it may write anything.
type Reached<'r> = Option<(Vec<Targets>, Vec<&'r Place>)>;

/// What one call reaches.
enum Targets {
    List(RunList),
    /// What an operation runs, found once for every call asking the same.
    Shared(SharedTargets),
}

impl Targets {
    fn ixs(&self) -> impl Iterator<Item = EffectIx> + '_ {
        let list = match self {
            Targets::List(list) => &list[..],
            Targets::Shared(list) => list,
        };
        list.iter().map(|(ix, _)| *ix)
    }

    fn into_list(self) -> RunList {
        match self {
            Targets::List(list) => list,
            Targets::Shared(list) => RunList::from_slice(&list),
        }
    }
}

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
        let mut lost: Vec<Rc<FxHashSet<&'r str>>> = vec![Rc::default(); scopes];
        // What each parameter is declared to hold does not depend on what a
        // round loses.
        let param_holders: Vec<Holder> = match sig {
            Some(sig) if !self.summary.writes.computed => {
                sig.params.iter().map(|p| holder_of(&p.ty)).collect()
            }
            _ => Vec::new(),
        };
        let aliased = Rc::new(self.aliased(sig.map_or(&[][..], |sig| &sig.params[..])));
        // A level depends on its scope's lost variables and on its parent's
        // level, so a round builds again only the scopes whose lost variables
        // grew in the one before, and the closures inside them.
        let mut levels: Vec<Level<'r>> = Vec::with_capacity(scopes);
        let mut reached: Vec<Reached<'r>> = Vec::with_capacity(scopes);
        let mut dirty = vec![true; scopes];
        let (levels, reached) = loop {
            for at in 0..scopes {
                if at > 0 {
                    let parent = self.summary.closures[at - 1]
                        .within
                        .map_or(0, |within| within as usize + 1);
                    dirty[at] |= dirty[parent];
                }
                if !dirty[at] {
                    continue;
                }
                let level = self.level_at(at, sig, &lost, &param_holders, &aliased, &levels);
                let found = level.reach();
                if at < levels.len() {
                    levels[at] = level;
                    reached[at] = found;
                } else {
                    levels.push(level);
                    reached.push(found);
                }
            }
            stats::count(Counter::ResolveRounds);
            stats::add(
                Counter::ResolveLevelsBuilt,
                dirty.iter().filter(|dirty| **dirty).count() as u64,
            );
            stats::add(
                Counter::ResolveLevelsReused,
                dirty.iter().filter(|dirty| !**dirty).count() as u64,
            );
            dirty.fill(false);
            let mut grew = false;
            for (at, found) in reached.iter().enumerate() {
                for place in found.iter().flat_map(|(_, written_args)| written_args) {
                    if let Some(name) = place_variable(place) {
                        grew |= self.lose(&levels, &mut lost, &mut dirty, at, name.as_str());
                    }
                }
            }
            if !grew {
                break (levels, reached);
            }
        };
        stats::count(Counter::ResolveRuns);
        #[cfg(debug_assertions)]
        self.assert_rounds_agree(sig, &param_holders, &lost);
        let mut out = Vec::with_capacity(scopes);
        for (at, (level, found)) in levels.iter().zip(reached).enumerate() {
            let scope_key = match at {
                0 => key.clone(),
                _ => key.closure(at as u32 - 1),
            };
            let constructs = at == 0
                && sig.is_some()
                && self.class.is_some()
                && ["__construct", "__clone"]
                    .iter()
                    .any(|name| self.member.as_str().eq_ignore_ascii_case(name));
            let property_writes = level.property_writes(
                found.as_ref().map(|(_, written)| &written[..]),
                constructs,
            );
            let mut classification = match found {
                Some((targets, written_args)) => level.classify(key, targets, written_args),
                None if level.facts.anything => Classification::anything(),
                None => Classification::unknown(),
            };
            #[cfg(debug_assertions)]
            Self::assert_writes_cover_effect(&classification.own, &property_writes, constructs);
            classification.property_writes = property_writes;
            out.push((scope_key, classification));
        }
        out
    }

    /// Every property the scope's own effect names is among the properties
    /// its writes name, but for the constructor's own.
    #[cfg(debug_assertions)]
    fn assert_writes_cover_effect(own: &Effect, writes: &PropertyWrites, constructs: bool) {
        if writes.all {
            return;
        }
        for storage in own.storages().into_iter().flatten() {
            let prop = match storage {
                Storage::Own(Some(prop)) if !constructs => prop,
                Storage::Property {
                    prop: Some(prop), ..
                } => prop,
                _ => continue,
            };
            assert!(
                writes.names.iter().any(|name| name == prop),
                "a write to {prop} is in the effect but not among the property writes"
            );
        }
    }

    /// The variables lost at each scope when every level is built again in
    /// every round, which the rounds that build only the changed ones must
    /// agree with.
    #[cfg(debug_assertions)]
    fn assert_rounds_agree(
        &self,
        sig: Option<&'r FunctionReflection>,
        param_holders: &[Holder],
        found: &[Rc<FxHashSet<&'r str>>],
    ) {
        let scopes = 1 + self.summary.closures.len();
        let aliased = Rc::new(self.aliased(sig.map_or(&[][..], |sig| &sig.params[..])));
        let mut lost: Vec<Rc<FxHashSet<&'r str>>> = vec![Rc::default(); scopes];
        loop {
            let mut levels: Vec<Level<'r>> = Vec::with_capacity(scopes);
            for at in 0..scopes {
                let level = self.level_at(at, sig, &lost, param_holders, &aliased, &levels);
                levels.push(level);
            }
            let reached: Vec<Reached<'r>> = levels.iter().map(Level::reach).collect();
            let mut dirty = vec![false; scopes];
            let mut grew = false;
            for (at, found) in reached.iter().enumerate() {
                for place in found.iter().flat_map(|(_, written_args)| written_args) {
                    if let Some(name) = place_variable(place) {
                        grew |= self.lose(&levels, &mut lost, &mut dirty, at, name.as_str());
                    }
                }
            }
            if !grew {
                break;
            }
        }
        assert!(
            lost.iter().zip(found).all(|(a, b)| **a == **b),
            "building only the changed levels lost the variables that building all of them does"
        );
    }

    /// Takes the variable `name` of the scope at `at` as written anything
    /// there, and in each scope around it that shares it by reference;
    /// whether that loses a parameter's declared type or what a body made.
    fn lose(
        &self,
        levels: &[Level<'r>],
        lost: &mut [Rc<FxHashSet<&'r str>>],
        dirty: &mut [bool],
        mut at: usize,
        name: &'r str,
    ) -> bool {
        let mut grew = false;
        loop {
            if !lost[at].contains(name) {
                Rc::make_mut(&mut lost[at]).insert(name);
                dirty[at] = true;
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

    /// The variables that may be written while a scope of the body runs, by
    /// another: the names of the by-reference parameters and of the
    /// variables a closure takes by reference.
    fn aliased(
        &self,
        params: &'r [crate::registry::function_sig::ParameterReflection],
    ) -> FxHashSet<&'r str> {
        let mut names: FxHashSet<&'r str> = params
            .iter()
            .filter(|p| p.by_ref)
            .map(|p| p.name.as_ref())
            .collect();
        for closure in self.summary.closures.iter() {
            for (name, by_ref) in closure.captures.iter().flatten() {
                if *by_ref {
                    names.insert(name.as_str());
                }
            }
        }
        names
    }

    /// The level of the scope at `at`: the body's, or a closure's in the
    /// order the summary lists them; the variables `lost` names at a scope
    /// are written anything there. `levels` holds the levels of the scopes
    /// before it.
    fn level_at(
        &self,
        at: usize,
        sig: Option<&'r FunctionReflection>,
        lost: &[Rc<FxHashSet<&'r str>>],
        param_holders: &[Holder],
        aliased: &Rc<FxHashSet<&'r str>>,
        levels: &[Level<'r>],
    ) -> Level<'r> {
        if let Some(index) = at.checked_sub(1) {
            let closure = &self.summary.closures[index];
            let parent = closure.within.map_or(0, |within| within as usize + 1);
            let mut level = self.closure_level(closure, &levels[parent], &lost[at], aliased);
            if index == 0 && closure.captures.is_none() {
                level.outer = self.captured;
            }
            return level;
        }
        let facts = &self.summary.writes;
        let sig_params = sig.map_or(&[][..], |sig| &sig.params[..]);
        let mut level = self.level(facts, &lost[0], aliased, facts.bindings.iter().collect());
        level.overwrites = sig_params
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.by_ref
                    && (facts.computed || facts.overwritten.iter().any(|n| n == p.name.as_ref()))
            })
            .map(|(at, p)| (!p.variadic).then_some(at as u32))
            .collect();
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
                .zip(param_holders)
                .filter(|(p, _)| !lost[0].contains(p.name.as_ref()))
                .map(|(p, held)| {
                    // What a reference refers to may be written an object by
                    // an alias, unless the declared type holds none.
                    let held = if p.by_ref && !matches!(held, Holder::Classes(c) if c.is_empty()) {
                        Holder::Unknown
                    } else {
                        held.clone()
                    };
                    (p.name.as_ref(), held)
                })
                .collect();
            if let Some((name, ty)) = self.value
                && !lost[0].contains(name)
            {
                level.base.insert(name, holder_of(ty));
            }
            if let Some(held) = self.captured
                && let Some(closure) = self.summary.closures.first()
                && let Some(captures) = &closure.captures
            {
                for (name, _) in captures.iter().filter(|(_, by_ref)| !by_ref) {
                    if !lost[0].contains(name.as_str())
                        && let Some(holder) = held(name)
                    {
                        level.base.insert(name.as_str(), holder);
                    }
                }
            }
        }
        level
    }

    /// The level of `closure`, written in the scope whose level is `parent`.
    fn closure_level(
        &self,
        closure: &'r ClosureFacts,
        parent: &Level<'r>,
        lost: &Rc<FxHashSet<&'r str>>,
        aliased: &Rc<FxHashSet<&'r str>>,
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
        let captures: Vec<(&'r str, bool, Option<&'r Name>)> = match &closure.captures {
            Some(captures) => captures
                .iter()
                .map(|(name, by_ref)| (name.as_str(), *by_ref, Some(name)))
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
                names.into_iter().map(|name| (name, false, None)).collect()
            }
        };
        base.reserve(captures.len());
        for &(name, by_ref, named) in &captures {
            if lost.contains(name) {
                continue;
            }
            let interned;
            let variable = match named {
                Some(named) => named,
                None => {
                    interned = Name::from(name);
                    &interned
                }
            };
            base.insert(name, parent.variable_holder(variable));
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
                    && !captures.iter().any(|&(captured, ..)| captured == name)
                {
                    made.insert(name, held.clone());
                }
            }
        }
        let mut level = self.level(facts, lost, aliased, bindings);
        level.closure = true;
        // What a closure writes of a variable it shares by reference with a
        // file's top level, the file's scope has.
        level.file_scope = false;
        level.shared_with_file = captures
            .iter()
            .filter(|&&(name, by_ref, _)| {
                by_ref && (parent.file_scope || parent.shared_with_file.contains(&name))
            })
            .map(|&(name, ..)| name)
            .collect();
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
        lost: &Rc<FxHashSet<&'r str>>,
        aliased: &Rc<FxHashSet<&'r str>>,
        bindings: Vec<&'r (Option<Name>, Binding)>,
    ) -> Level<'r> {
        let origins = if facts.computed {
            FxHashMap::default()
        } else {
            let mut origins =
                FxHashMap::with_capacity_and_hasher(facts.origins.len(), Default::default());
            origins.extend(
                facts
                    .origins
                    .iter()
                    .map(|(name, origins)| (name.as_str(), &origins[..]))
                    .filter(|(name, _)| !lost.contains(name)),
            );
            origins
        };
        Level {
            registry: self.registry,
            class: self.class,
            member: self.member.clone(),
            file: self.file,
            file_scope: self.file_scope,
            shared_with_file: Vec::new(),
            facts,
            params: FxHashMap::default(),
            made: FxHashMap::default(),
            origins,
            closures: &self.summary.closures,
            base: FxHashMap::default(),
            solved: Default::default(),
            origin_holders: Default::default(),
            closure: false,
            bindings,
            lost: Rc::clone(lost),
            aliased: Rc::clone(aliased),
            runs: Default::default(),
            params_run: Default::default(),
            overwrites: Vec::new(),
            outer: None,
            class_name: std::cell::OnceCell::new(),
            this_holder: std::cell::OnceCell::new(),
        }
    }
}

/// Whether the object a value from `origin` holds depends on what a variable
/// of the scope holds.
fn reads_variables(origin: &Origin) -> bool {
    match origin {
        Origin::Elements(inner) => reads_variables(inner),
        Origin::Taken(_, from, held) => from.iter().chain(held.iter()).any(reads_variables),
        Origin::Written(_) => true,
        Origin::Place(place) => matches!(place.root, Root::Variable(Some(_))),
        Origin::Returned(callee) => matches!(
            &**callee,
            Callee::Method {
                receiver: Receiver::Variable(_) | Receiver::Origins(_),
                ..
            }
        ),
        _ => false,
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
    file: Option<FileId>,
    // A file's top level, whose variables belong to the scope including it.
    file_scope: bool,
    // The variables of a file's top level a closure written there shares
    // with it by reference.
    shared_with_file: Vec<&'r str>,
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
    // What each origin the scope's calls ask about holds, by where the origin
    // is: the scope does not change while its calls are read.
    origin_holders: std::cell::RefCell<FxHashMap<*const Origin, (Holder, bool)>>,
    // A closure's scope, which may be bound to any class's.
    closure: bool,
    // What its variables are bound to: its own bindings and, for a variable
    // a closure takes by reference, those of the code that wrote it.
    bindings: Vec<&'r (Option<Name>, Binding)>,
    // The variables written anything here, by what the calls reach.
    lost: Rc<FxHashSet<&'r str>>,
    // The variables another scope may write while this one runs: those taken
    // by reference, by a parameter or a closure.
    aliased: Rc<FxHashSet<&'r str>>,
    // The closures its calls run, each with its `$this`.
    runs: std::cell::RefCell<Vec<(u32, Transfer)>>,
    // The declaration's parameters whose arguments its calls run.
    params_run: std::cell::RefCell<Vec<Condition>>,
    // The positions of the parameters taken by reference the body writes
    // over, whose arguments then die; `None` for a variadic one.
    overwrites: Vec<Option<u32>>,
    // For an arrow function or a first-class callable, which take every
    // variable around them by value: what the code that wrote it knows of
    // the object a variable it does not write holds.
    outer: Option<CapturedHolders<'r>>,
    // The name of the class and the holder of its `$this`, made when first
    // asked.
    class_name: std::cell::OnceCell<Option<Name>>,
    this_holder: std::cell::OnceCell<Holder>,
    // The closures the body wrote.
    closures: &'r [ClosureFacts],
}

impl CallContext for Level<'_> {
    fn class(&self) -> Option<&str> {
        self.class
    }

    fn own_class(&self) -> Option<Name> {
        self.class_name().cloned()
    }

    fn scope_name(&self) -> Option<Name> {
        self.class_name().map(|class| class.lowered().clone())
    }

    fn file(&self) -> Option<FileId> {
        self.file
    }

    /// The object a method is called on, and whether it is the body's own
    /// `$this`.
    fn receiver(&self, receiver: &Receiver) -> (Holder, bool) {
        let registry = self.registry;
        match receiver {
            Receiver::This => (self.this_holder(), true),
            Receiver::ThisProperty(prop) => (
                self.class
                    .and_then(|class| registry.lookup_property(class, prop))
                    .map_or(Holder::Unknown, |p| holder_of(p.ty())),
                false,
            ),
            Receiver::Variable(name) => (self.variable_holder(name), false),
            Receiver::Static(class, prop) => (self.static_holder(class, prop), false),
            Receiver::Origins(origins) => self.operand(Some(origins)),
            Receiver::Unknown => (Holder::Unknown, false),
        }
    }

    fn argument(&self, _: &ArgAt, place: Option<&Place>) -> Holder {
        match place {
            Some(place) => self.value_holder(&place.root, place.narrowing.as_ref(), &place.steps),
            None => Holder::Unknown,
        }
    }

    fn elements(&self, at: &ArgAt, place: Option<&Place>) -> Holder {
        if let Some(place) = place
            && let Some(name) = place_variable(place)
            && !self.made.contains_key(name.as_str())
            && let Some(Declared::Param { ty, .. }) = self.params.get(name.as_str())
        {
            return elements_holder_of(ty);
        }
        match self.argument(at, place) {
            none @ Holder::Classes(_) if matches!(&none, Holder::Classes(c) if c.is_empty()) => {
                none
            }
            _ => Holder::Unknown,
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
        let mut holder = Holder::none();
        let (mut this, mut other) = (false, false);
        for origin in origins {
            let (found, same) = self.operand_origin(origin);
            if same {
                this = true;
            } else if found != Holder::none() {
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
    /// The name of the class the scope belongs to.
    fn class_name(&self) -> Option<&Name> {
        self.class_name
            .get_or_init(|| self.class.map(Name::from))
            .as_ref()
    }

    /// The holder of the scope's own `$this`.
    fn this_holder(&self) -> Holder {
        self.this_holder
            .get_or_init(|| match self.class_name() {
                Some(class) => Holder::single(class.clone()),
                None => Holder::Unknown,
            })
            .clone()
    }

    /// What `origin`, one of a call's operand, holds, found once: the solved
    /// variables it reads do not change after the scope is solved.
    fn operand_origin(&self, origin: &Origin) -> (Holder, bool) {
        if matches!(
            origin,
            Origin::NoObject | Origin::Arithmetic | Origin::Closure
        ) {
            return (Holder::none(), false);
        }
        let at: *const Origin = origin;
        if let Some(found) = self.origin_holders.borrow().get(&at) {
            return found.clone();
        }
        let found = self.origin_holder(origin);
        let mut holders = self.origin_holders.borrow_mut();
        if holders.is_empty() {
            holders.reserve(self.origin_count());
        }
        holders.insert(at, found.clone());
        found
    }

    /// How many origins the scope's variables and calls name: the most
    /// `operand_origin` can be asked about.
    fn origin_count(&self) -> usize {
        let calls: usize = self
            .facts
            .calls
            .iter()
            .map(|call| match &call.callee {
                Callee::Implicit { on, .. } => on.as_deref().map_or(0, <[Origin]>::len),
                Callee::Method {
                    receiver: Receiver::Origins(origins),
                    ..
                } => origins.len(),
                _ => 0,
            })
            .sum();
        calls
            + self
                .origins
                .values()
                .map(|origins| origins.len())
                .sum::<usize>()
    }

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
        self.params_run.borrow_mut().push(Condition {
            at: *at,
            this,
            destroys: false,
        });
        true
    }

    /// Whether `ix` is a builtin whose by-reference writes the summary
    /// applied to the variables (see `summaries::filler`).
    fn is_filler(&self, ix: EffectIx) -> bool {
        let key = self.registry.effect_key(ix);
        key.class.is_none() && crate::registry::summaries::filler(key.member.as_str()).is_some()
    }

    /// The properties the scope writes other than as a constructor does, in
    /// the sense of `PropertyWrites`: `constructs` when the scope is the
    /// body of a constructor or `__clone`. `written` is what its calls may
    /// write by reference, when that is known; else every place they are
    /// passed.
    fn property_writes(&self, written: Option<&[&Place]>, constructs: bool) -> PropertyWrites {
        let facts: &'r WriteFacts = self.facts;
        if facts.anything {
            return PropertyWrites::everything();
        }
        let mut out = PropertyWrites::default();
        let passed: Vec<&Place> = match written {
            Some(written) => written.to_vec(),
            None => facts
                .calls
                .iter()
                .flat_map(|call| call.places.iter().map(|(_, place)| place))
                .collect(),
        };
        for place in facts.writes.iter().chain(passed) {
            let Some(at) = place
                .steps
                .iter()
                .rposition(|step| matches!(step, Step::Property(_)))
            else {
                continue;
            };
            let Step::Property(prop) = &place.steps[at] else {
                unreachable!();
            };
            if constructs && at == 0 && matches!(place.root, Root::This) {
                continue;
            }
            out.note(prop.as_ref());
        }
        // A constructor run again on an object that exists writes what it
        // wrote. Inside a constructor, `parent::__construct()` is the chain.
        let initializer = |method: &Option<Name>| {
            method.as_ref().is_some_and(|method| {
                method.as_str().eq_ignore_ascii_case("__construct")
                    || method.as_str().eq_ignore_ascii_case("__clone")
            })
        };
        for call in facts.calls.iter() {
            let again = match &call.callee {
                Callee::Method { method, .. } => initializer(method),
                Callee::Static { class, method } => {
                    initializer(method)
                        && !(constructs
                            && class.as_ref().is_some_and(|class| {
                                class.written.as_str().eq_ignore_ascii_case("parent")
                            }))
                }
                _ => false,
            };
            if again {
                out.all = true;
            }
        }
        // A reference to a property, or to an element of an object being
        // iterated, is written wherever it goes.
        for (_, binding) in self.bindings.iter().copied() {
            let Binding::Reference(target) = binding else {
                continue;
            };
            if let Some(Step::Property(prop)) = target
                .steps
                .iter()
                .rev()
                .find(|step| matches!(step, Step::Property(_)))
            {
                out.note(prop.as_ref());
            } else if let [rest @ .., Step::Element] = &target.steps[..] {
                match self.value_holder(&target.root, target.narrowing.as_ref(), rest) {
                    Holder::Classes(classes) if classes.is_empty() => {}
                    _ => out.all = true,
                }
            }
        }
        out
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
            let targets = match &call.callee {
                Callee::Implicit { on, operation } => {
                    reach.implicit_call_targets(on.as_deref(), operation)?
                }
                callee => Targets::List(reach.call_targets(callee, Some(call))?),
            };
            // A closure the body wrote or a parameter's argument, called:
            // what it takes by reference is not looked up.
            let unknown_params = (self.runs.borrow().len(), self.params_run.borrow().len()) != ran;
            for (at, place) in call.places.iter() {
                let filled = call.fills.contains(at);
                if unknown_params
                    || targets
                        .ixs()
                        .any(|ix| !(filled && self.is_filler(ix)) && self.by_ref_at(ix, at))
                {
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
        reached: Vec<Targets>,
        written_args: Vec<&Place>,
    ) -> Classification<'r> {
        let reach = Reach {
            registry: self.registry,
            ctx: self,
        };
        let listed: usize = reached
            .iter()
            .map(|targets| match targets {
                Targets::List(list) => list.len(),
                Targets::Shared(_) => 0,
            })
            .sum();
        let mut calls: Vec<(EffectIx, TransferIx, Option<u32>)> = Vec::with_capacity(listed);
        let mut sets: Vec<IdTargets> = Vec::new();
        let mut sites: Vec<Site<'r>> = Vec::new();
        // The declarations run with no call's arguments, each once.
        let mut siteless: FxHashSet<(EffectIx, TransferIx)> =
            FxHashSet::with_capacity_and_hasher(listed, Default::default());
        let facts: &'r WriteFacts = self.facts;
        // The shared sets of declarations already taken.
        let mut taken: FxHashSet<usize> = FxHashSet::default();
        for (call, targets) in facts.calls.iter().zip(reached) {
            let mut site: Option<u32> = None;
            let entries = match targets {
                Targets::List(list) => list,
                Targets::Shared(list) => {
                    debug_assert!(call.arguments.is_empty() && call.places.is_empty());
                    if !taken.insert(Arc::as_ptr(&list).cast::<Run>() as usize) {
                        continue;
                    }
                    let Some(set) = self.shared_runs(&reach, list, call) else {
                        return Classification::unknown();
                    };
                    if !set.entries.is_empty() {
                        sets.push(set);
                    }
                    continue;
                }
            };
            for run in entries {
                let (ix, transfer) = run;
                let key = self.registry.effect_key(ix);
                // A builtin writes what the code it runs writes: what is
                // passed to it, and what of its object a class extending it
                // declares.
                if let Some(sig) = self.registry.builtin_of(ix) {
                    let transfer = self.registry.transfer_of(transfer);
                    let Some(runs) = reach.builtin_runs(key, sig, transfer, call) else {
                        return Classification::unknown();
                    };
                    for run in runs.iter() {
                        if siteless.insert(run) {
                            calls.push((run.0, run.1, None));
                        }
                    }
                    continue;
                }
                let at = passes_arguments(&call.callee, key).then(|| {
                    *site.get_or_insert_with(|| {
                        let passed = self.passed(holder, &reach, call);
                        sites.push(Site { call, passed });
                        sites.len() as u32 - 1
                    })
                });
                if at.is_some() {
                    calls.push((run.0, run.1, at));
                    continue;
                }
                if siteless.insert(run) {
                    calls.push((run.0, run.1, None));
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
        for (id, transfer) in self.runs.take() {
            let (run, transfer) = self.registry.effect_run(&holder.closure(id), &transfer);
            calls.push((run, transfer, None));
        }
        let mut runs = self.params_run.take();
        for &at in &self.overwrites {
            match at {
                Some(at) => runs.push(Condition {
                    at,
                    this: None,
                    destroys: true,
                }),
                None => own = Effect::unknown(),
            }
        }
        Classification {
            own,
            calls,
            sets,
            sites,
            runs,
            property_writes: PropertyWrites::default(),
        }
    }

    /// What the shared set `list`, run by calls that pass no arguments,
    /// runs once each builtin in it is replaced by what it runs, by id;
    /// found once for every scope that asks the same.
    fn shared_runs(
        &self,
        reach: &Reach<'_>,
        list: SharedTargets,
        call: &CallWrites,
    ) -> Option<IdTargets> {
        let registry = self.registry;
        let memo = &registry.reach_memo;
        let address = Arc::as_ptr(&list).cast::<Run>() as usize;
        let (_, scoped) = memo.shared_scoped.get_or_compute(address, || {
            let scoped = list
                .iter()
                .any(|(_, transfer)| *transfer == TransferTable::SAME);
            (Arc::clone(&list), scoped)
        });
        let class = if scoped {
            self.class().map(str::to_owned)
        } else {
            None
        };
        let (_, runs) = memo.shared_runs.get_or_compute((address, class), || {
            let runs = (|| {
                let mut out: Vec<(EffectIx, TransferIx)> = Vec::new();
                let mut seen: FxHashSet<(EffectIx, TransferIx)> = FxHashSet::default();
                for &(ix, transfer) in list.iter() {
                    if let Some(sig) = registry.builtin_of(ix) {
                        let runs = reach.builtin_runs(
                            registry.effect_key(ix),
                            sig,
                            registry.transfer_of(transfer),
                            call,
                        )?;
                        for run in runs.iter() {
                            if seen.insert(run) {
                                out.push(run);
                            }
                        }
                    } else if seen.insert((ix, transfer)) {
                        out.push((ix, transfer));
                    }
                }
                Some(IdTargets::new(IdTargetSet::new(out)))
            })();
            (Arc::clone(&list), runs)
        });
        runs
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
                runs.extend(closures.into_iter().map(|(id, transfer)| {
                    self.registry.effect_run(&holder.closure(id), &transfer)
                }));
                let destroys = reach.argument_destroys(call, at);
                out.push((
                    at,
                    Passed {
                        runs,
                        params,
                        destroys,
                    },
                ));
            }
        }
        out
    }

    /// Whether the declaration `ix` stands for takes the argument passed
    /// `at` by reference.
    fn by_ref_at(&self, ix: EffectIx, at: &ArgAt) -> bool {
        let registry = self.registry;
        let Some(params) = registry.reach_memo.effects.by_ref_params(ix, || {
            registry.declaration(registry.effect_key(ix)).map(|sig| {
                ByRefParams::new(sig.params.iter().map(|p| (&p.name, p.by_ref, p.variadic)))
            })
        }) else {
            return true;
        };
        match at {
            ArgAt::Position(at) => params.at_position(*at as usize),
            ArgAt::Named(name) => params.named(name.as_str()),
            ArgAt::Spread => params.any(),
        }
    }

    /// The object made by `new` of one of `classes`: one nothing else holds,
    /// unless a constructor lets code reach it.
    fn made_holder(&self, classes: NameList) -> Holder {
        if classes
            .iter()
            .any(|class| self.registry.class_constructor_leaks(class.as_str()))
        {
            Holder::Classes(sorted_classes(classes))
        } else {
            Holder::Made(Some(shared_names(classes)))
        }
    }

    /// The object made by `clone`, `new $c` or `new class`, of a class not
    /// known.
    fn made_unknown_holder(&self) -> Holder {
        if self.registry.unknown_object_may_leak() {
            Holder::Unknown
        } else {
            Holder::Made(None)
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
                Made::Object(Some(class)) => self.made_holder(
                    named_classes(self.registry, self.class_name(), class).unwrap_or_default(),
                ),
                Made::Object(None) => self.made_unknown_holder(),
                Made::Closure(_) => Holder::Made(Some(vec![Name::from("Closure")].into())),
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
        if self.facts.reassigned.contains(name) {
            return Holder::Unknown;
        }
        match self.base.get(name.as_str()) {
            Some(held) => held.clone(),
            None => self
                .outer
                .and_then(|outer| outer(name.as_str()))
                .unwrap_or(Holder::Unknown),
        }
    }

    /// The object the variable `name` holds of what the scope writes it.
    fn written_holder(&self, name: &Name) -> Holder {
        if self.lost.contains(name.as_str()) || self.facts.computed {
            return Holder::Unknown;
        }
        match self.origins.get(name.as_str()) {
            Some(origins) => origins.iter().fold(Holder::none(), |held, origin| {
                held.join(self.operand_origin(origin).0)
            }),
            None if self.facts.reassigned.contains(name) => Holder::Unknown,
            None => Holder::none(),
        }
    }

    /// Whether nothing but the code of this scope, in the order it is
    /// written, writes the variable `name`: a guard on it holds until the
    /// scope writes it.
    fn stable(&self, name: &Name) -> bool {
        let name = name.as_str();
        !self.facts.computed
            && !self.file_scope
            && !self.lost.contains(name)
            && !self.aliased.contains(name)
            && !self.shared_with_file.contains(&name)
            && !self.bindings.iter().any(|(bound, binding)| {
                bound.as_ref().is_none_or(|bound| bound.as_str() == name)
                    || matches!(binding, Binding::Reference(place)
                        if matches!(&place.root, Root::Variable(shared)
                            if shared.as_ref().is_none_or(|shared| shared.as_str() == name)))
            })
    }

    /// Whether a value of the type `ty`, as an inline `@var` gives it, holds
    /// objects of classes known only: it can be taken as written.
    fn known_type(&self, ty: &Type) -> bool {
        element_classes(ty).is_some_and(|classes| {
            classes
                .iter()
                .all(|class| self.registry.lookup_class(class).is_some())
        })
    }

    /// The object the variable `name` holds where a guard shows
    /// `narrowing` of it.
    fn narrowed_holder(&self, name: &Name, narrowing: Option<&Narrowing>) -> Holder {
        let held = self.variable_holder(name);
        let Some(narrowing) = narrowing.filter(|_| self.stable(name)) else {
            return held;
        };
        match narrowing {
            Narrowing::NoObject => Holder::none(),
            Narrowing::Declared(ty) if self.known_type(ty) => holder_of(ty),
            Narrowing::Declared(_) => held,
            Narrowing::Instance(classes) => {
                let mut named = NameList::new();
                for class in classes.iter() {
                    match named_classes(self.registry, self.class_name(), class) {
                        Some(found) => named.extend(found),
                        None => return held,
                    }
                }
                match held {
                    Holder::Made(_) => held,
                    Holder::Classes(own) if !own.is_empty() => {
                        let mut found = NameList::new();
                        for own in own.iter() {
                            for class in &named {
                                found.push(if self.registry.is_subclass_of(own, class) {
                                    own.clone()
                                } else {
                                    class.clone()
                                });
                            }
                        }
                        Holder::Classes(sorted_classes(found))
                    }
                    _ => Holder::Classes(sorted_classes(named)),
                }
            }
        }
    }

    /// The object a parameter declared as `declared` holds.
    fn declared(&self, declared: &Declared<'_>) -> Holder {
        match declared {
            Declared::Param { ty, .. } => holder_of(ty),
            Declared::Classes(Some(classes)) => {
                let mut found = NameList::new();
                for class in *classes {
                    match named_classes(self.registry, self.class_name(), class) {
                        Some(named) => found.extend(named),
                        None => return Holder::Unknown,
                    }
                }
                Holder::listed(found)
            }
            Declared::Classes(None) => Holder::Unknown,
        }
    }

    /// Finds the object each variable of `origins` may hold: what it held
    /// before the scope wrote it and what each value written may be, taken
    /// again until none grows. Each starts as no object, and is read as
    /// found so far while the others are found.
    fn solve(&self) {
        #[cfg(debug_assertions)]
        let reference = {
            self.solve_rounds(false);
            self.solved.borrow_mut().take()
        };
        self.solve_rounds(true);
        #[cfg(debug_assertions)]
        debug_assert_eq!(reference, *self.solved.borrow());
    }

    /// `solve`; `split` finds once what the origins that read no variable
    /// hold, and takes again only the others in each round.
    fn solve_rounds(&self, split: bool) {
        let start: FxHashMap<&'r str, Holder> = self
            .origins
            .keys()
            .map(|&name| (name, Holder::none()))
            .collect();
        *self.solved.borrow_mut() = Some(start);
        // Per variable: what it held before the scope wrote it joined with
        // what the origins that read no variable hold, and the others.
        let parts: Vec<(&'r str, Holder, Vec<&'r Origin>)> = self
            .origins
            .iter()
            .map(|(&name, origins)| {
                let mut fixed = self.base.get(name).cloned().unwrap_or(Holder::none());
                let mut variable = Vec::new();
                for origin in origins.iter() {
                    if split && !reads_variables(origin) {
                        fixed = fixed.join(self.origin_holder(origin).0);
                    } else {
                        variable.push(origin);
                    }
                }
                (name, fixed, variable)
            })
            .collect();
        // A holder grows at most to any object; a chain of variables this
        // long has not settled.
        for _ in 0..16 {
            let mut grew = false;
            for (name, fixed, variable) in &parts {
                let mut held = fixed.clone();
                for origin in variable {
                    held = held.join(self.origin_holder(origin).0);
                }
                let held = match held {
                    Holder::Classes(classes) => {
                        Holder::Classes(sorted_classes(classes.iter().cloned().collect()))
                    }
                    Holder::Made(Some(classes)) => {
                        Holder::Made(Some(sorted_classes(classes.iter().cloned().collect())))
                    }
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
            Origin::NoObject | Origin::Arithmetic | Origin::Closure => (Holder::none(), false),
            Origin::Declared(ty) if self.known_type(ty) => (holder_of(ty), false),
            Origin::Declared(_) => (Holder::Unknown, false),
            Origin::Place(place) if matches!(place.root, Root::This) && place.steps.is_empty() => {
                (self.value_holder(&place.root, None, &[]), true)
            }
            Origin::Place(place) => (
                self.value_holder(&place.root, place.narrowing.as_ref(), &place.steps),
                false,
            ),
            Origin::Returned(callee) => (self.returned_holder(callee), false),
            Origin::Elements(inner) => (self.elements_holder(inner), false),
            Origin::Taken(callee, from, held) => (self.taken_holder(callee, from, held), false),
            Origin::Written(name) => (self.written_holder(name), false),
            Origin::New(Some(class)) => (
                self.made_holder(
                    named_classes(registry, self.class_name(), class).unwrap_or_default(),
                ),
                false,
            ),
            Origin::New(None) | Origin::Anonymous(None) => (self.made_unknown_holder(), false),
            Origin::Anonymous(Some(name)) => (self.made_holder(smallvec![name.clone()]), false),
            Origin::Constant(name) => {
                let (primary, fallback) = name.candidates();
                let found = registry
                    .global_constants()
                    .get(primary)
                    .or_else(|| fallback.and_then(|name| registry.global_constants().get(name)));
                (found.map_or(Holder::Unknown, holder_of), false)
            }
            Origin::ClassConstant(class, constant) => {
                let Some(classes) = named_classes(registry, self.class_name(), class) else {
                    return (Holder::Unknown, false);
                };
                let mut found = NameList::new();
                for class in classes.iter() {
                    let Some(ty) = registry.lookup_const_type(class, constant) else {
                        return (Holder::Unknown, false);
                    };
                    match holder_of(ty) {
                        Holder::Classes(held) => found.extend(held.iter().cloned()),
                        _ => return (Holder::Unknown, false),
                    }
                }
                (Holder::listed(found), false)
            }
            Origin::Caught(classes) => {
                let mut found = NameList::new();
                for class in classes.iter() {
                    match named_classes(registry, self.class_name(), class) {
                        Some(named) => found.extend(named),
                        None => return (Holder::Unknown, false),
                    }
                }
                (Holder::listed(found), false)
            }
        }
    }

    /// The objects held in the elements, at any depth, of an array from
    /// `origin`: those a declared type names, and any object when none does.
    fn elements_holder(&self, origin: &Origin) -> Holder {
        match origin {
            Origin::Place(place) => {
                // An object is no array; what it holds is not followed.
                match self.value_holder(&place.root, place.narrowing.as_ref(), &place.steps) {
                    Holder::Classes(classes) | Holder::Made(Some(classes))
                        if !classes.is_empty() =>
                    {
                        return Holder::none();
                    }
                    _ => {}
                }
                let mut steps = place.steps.to_vec();
                let mut found = Holder::none();
                for _ in 0..ELEMENT_DEPTH {
                    steps.push(Step::Element);
                    found = found.join(self.value_holder(
                        &place.root,
                        place.narrowing.as_ref(),
                        &steps,
                    ));
                    if found == Holder::Unknown {
                        break;
                    }
                }
                found
            }
            Origin::Returned(callee) => self.returned_holder_of(callee, elements_holder_of),
            Origin::Declared(ty) if self.known_type(ty) => elements_holder_of(ty),
            Origin::Constant(name) => {
                let (primary, fallback) = name.candidates();
                self.registry
                    .global_constants()
                    .get(primary)
                    .or_else(|| {
                        fallback.and_then(|name| self.registry.global_constants().get(name))
                    })
                    .map_or(Holder::Unknown, elements_holder_of)
            }
            Origin::Elements(inner) => self.elements_holder(inner),
            Origin::Taken(callee, from, held) => self.taken_holder(callee, from, held),
            _ => Holder::Unknown,
        }
    }

    /// What a call of `callee` takes out of the arrays from `from`: their
    /// elements when it is a builtin that does so, else what it returns.
    fn taken_holder(&self, callee: &Callee, from: &[Origin], held: &[Origin]) -> Holder {
        let takes = match callee {
            Callee::Function(name) => self
                .registry
                .resolve_function(name)
                .is_some_and(|(resolved, _)| crate::registry::summaries::takes_elements(resolved)),
            _ => false,
        };
        if !takes {
            return self.returned_holder(callee);
        }
        let found = held.iter().fold(Holder::none(), |all, origin| {
            all.join(self.origin_holder(origin).0)
        });
        from.iter()
            .fold(found, |all, origin| all.join(self.elements_holder(origin)))
    }

    /// The object what a call of `callee` returns may be, by the return
    /// types its targets declare.
    fn returned_holder(&self, callee: &Callee) -> Holder {
        let Some(key) = self.returned_key(callee) else {
            return self.compute_returned_holder(callee);
        };
        let mut computed = false;
        let held = self.registry.reach_memo.returned.get_or_compute(key, || {
            computed = true;
            self.compute_returned_holder(callee)
        });
        stats::count(if computed {
            Counter::ReturnedHolderMiss
        } else {
            Counter::ReturnedHolderHit
        });
        debug_assert_eq!(held, self.compute_returned_holder(callee));
        held
    }

    /// What the holder a call of `callee` returns depends on, when that is
    /// no more than the registry and the scope's class: not the call's
    /// place in the code, nor a variable.
    fn returned_key(&self, callee: &Callee) -> Option<ReturnedKey> {
        match callee {
            Callee::Method {
                receiver: Receiver::This,
                method: Some(method),
            } => Some(ReturnedKey::ThisMethod {
                class: self.class_name()?.clone(),
                method: method.clone(),
            }),
            Callee::Method {
                receiver: Receiver::ThisProperty(property),
                method: Some(method),
            } => Some(ReturnedKey::ThisPropertyMethod {
                class: self.class_name()?.clone(),
                property: property.clone(),
                method: method.clone(),
            }),
            Callee::Static {
                class: Some(class),
                method: Some(method),
            } => {
                let (target, own) = if class.is_keyword("self") {
                    (StaticTarget::SelfKeyword, self.class_name().cloned())
                } else if class.is_keyword("parent") {
                    (StaticTarget::Parent, self.class_name().cloned())
                } else if class.is_keyword("static") {
                    (StaticTarget::StaticKeyword, self.class_name().cloned())
                } else {
                    (
                        StaticTarget::Named(class.resolved_class_name().cloned()),
                        None,
                    )
                };
                Some(ReturnedKey::Static {
                    target,
                    method: method.clone(),
                    own,
                })
            }
            Callee::ParentHook {
                property,
                set: false,
            } => Some(ReturnedKey::ParentHook {
                property: property.clone(),
                class: self.class_name()?.clone(),
            }),
            _ => None,
        }
    }

    fn compute_returned_holder(&self, callee: &Callee) -> Holder {
        self.returned_holder_of(callee, holder_of)
    }

    /// The holder `of` gives the type a call of `callee` returns, joined over
    /// its targets.
    fn returned_holder_of(&self, callee: &Callee, of: fn(&Type) -> Holder) -> Holder {
        let registry = self.registry;
        match callee {
            Callee::Function(name) => match registry.resolve_function(name) {
                // They return strings, numbers and null, whatever their
                // declared type says.
                Some((resolved, _))
                    if resolved.eq_ignore_ascii_case("sscanf")
                        || resolved.eq_ignore_ascii_case("fscanf") =>
                {
                    Holder::none()
                }
                Some((_, sig)) => of(sig.return_type()),
                // A call to no function throws.
                None => Holder::none(),
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
                let mut held = Holder::none();
                for class in classes.iter() {
                    let found = match registry.lookup_method_with_owner(class, method) {
                        Some((owner, sig)) if !sig.is_magic => {
                            declared_holder(registry, sig.return_type(), class, &owner, of)
                        }
                        // `__call`, or a method of a class below.
                        _ => {
                            let mut found = Holder::none();
                            for key in registry.implementations(class, method).iter() {
                                let Some((owner, sig)) = key.class.as_ref().and_then(|owner| {
                                    registry.lookup_method_with_owner(owner, &key.member)
                                }) else {
                                    return Holder::Unknown;
                                };
                                found = found.join(declared_holder(
                                    registry,
                                    sig.return_type(),
                                    &owner,
                                    &owner,
                                    of,
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
                let Some(targets) = named_classes(registry, self.class_name(), class) else {
                    return Holder::Unknown;
                };
                let mut held = Holder::none();
                for target in &targets {
                    match registry.lookup_method_with_owner(target, method) {
                        Some((owner, sig)) if !sig.is_magic => {
                            held = held.join(declared_holder(
                                registry,
                                sig.return_type(),
                                target,
                                &owner,
                                of,
                            ));
                        }
                        _ => {
                            for magic in [&well_known().call_static, &well_known().call] {
                                if let Some((owner, sig)) =
                                    registry.lookup_method_with_owner(target, magic)
                                {
                                    held = held.join(declared_holder(
                                        registry,
                                        sig.return_type(),
                                        target,
                                        &owner,
                                        of,
                                    ));
                                }
                            }
                        }
                    }
                }
                held
            }
            // What the closures a variable only ever holds are declared to
            // return.
            Callee::Value(Some(name)) => {
                let Some(Made::Closure(ids)) = self.made.get(name.as_str()) else {
                    return Holder::Unknown;
                };
                let mut held = Holder::none();
                for id in ids.iter() {
                    let Some(returns) = self
                        .closures
                        .get(*id as usize)
                        .and_then(|closure| closure.returns.as_deref())
                    else {
                        return Holder::Unknown;
                    };
                    if returns.is_empty() {
                        held = held.join(of(&Type::null()));
                    }
                    for class in returns {
                        held = held.join(of(&Type::object(class.clone(), Vec::new())));
                    }
                }
                held
            }
            // A `set` hook returns nothing.
            Callee::ParentHook { set: true, .. } => Holder::none(),
            Callee::ParentHook {
                property,
                set: false,
            } => {
                let Some(parent) = self
                    .class
                    .and_then(|class| registry.class_references_by_name(class))
                    .and_then(|references| references.extends.first())
                else {
                    return Holder::Unknown;
                };
                match registry.lookup_property_with_owner(parent, property) {
                    Some((owner, declared)) => {
                        declared_holder(registry, declared.ty(), parent, &owner, of)
                    }
                    None => Holder::Unknown,
                }
            }
            _ => Holder::Unknown,
        }
    }

    /// The object a static property holds, by its declared type.
    fn static_holder(&self, class: &QualifiedName, prop: &Name) -> Holder {
        let mut classes = NameList::new();
        for (owner, _) in self.static_property_owners(class, Some(prop)) {
            let Some(owner) = owner else {
                return Holder::Unknown;
            };
            match self
                .registry
                .lookup_property(&owner, prop)
                .map(|p| holder_of(p.ty()))
            {
                Some(Holder::Classes(found)) => classes.extend(found.iter().cloned()),
                _ => return Holder::Unknown,
            }
        }
        Holder::listed(classes)
    }

    /// The declared types of the static property `class::$prop`, as seen
    /// from each class `class` may be; `None` when one is not known.
    fn static_types(&self, class: &QualifiedName, prop: &Name) -> Option<Vec<Type>> {
        let mut types = Vec::new();
        for (owner, _) in self.static_property_owners(class, Some(prop)) {
            types.push(self.registry.lookup_property(&owner?, prop)?.ty().clone());
        }
        Some(types)
    }

    /// Each storage `class::$prop` may be: the class declaring the static
    /// property, as seen from each class `class` may be, with its name;
    /// `None` for a class not known.
    fn static_property_owners(
        &self,
        class: &QualifiedName,
        prop: Option<&Name>,
    ) -> SmallVec<[(Option<Name>, Option<Name>); 2]> {
        let Some(classes) = named_classes(self.registry, self.class_name(), class) else {
            return smallvec![(None, prop.cloned())];
        };
        let mut out: SmallVec<[(Option<Name>, Option<Name>); 2]> = SmallVec::new();
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
            *out = Effect::unknown();
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
            match self.value_holder(&place.root, place.narrowing.as_ref(), &place.steps[..at]) {
                Holder::Made(_) => {}
                Holder::Unknown => out.insert(Storage::Property {
                    class: None,
                    prop: prop.clone(),
                }),
                Holder::Classes(classes) => {
                    for class in classes.iter() {
                        out.insert(Storage::Property {
                            class: Some(class.clone()),
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
        let mut bound_here = false;
        for (bound, binding) in self.bindings.iter().copied() {
            let applies = match (bound, name) {
                (None, _) | (_, None) => true,
                (Some(bound), Some(name)) => bound == name,
            };
            if !applies {
                continue;
            }
            bound_here = true;
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
                        narrowing: None,
                    };
                    self.place_storages(&through, out, depth + 1);
                }
            }
        }
        // A variable of a file's top level is the including scope's: a
        // global when that is the top level of a script.
        let shared = match name {
            Some(name) => self.shared_with_file.contains(&name.as_str()),
            None => !self.shared_with_file.is_empty(),
        };
        if !bound_here && (self.file_scope || shared) {
            out.insert(Storage::Global(name.cloned()));
        }
    }

    /// The object at `root` and `steps` may be, by the declared types along
    /// the way.
    fn value_holder(&self, root: &Root, narrowing: Option<&Narrowing>, steps: &[Step]) -> Holder {
        // The declared types of the value the place starts from, when it
        // has them: an array of objects holds objects its own holder, the
        // object it is, does not name.
        let typed: Option<Vec<Type>> = match root {
            Root::Variable(Some(name)) => match narrowing.filter(|_| self.stable(name)) {
                Some(Narrowing::Declared(ty)) if self.known_type(ty) => Some(vec![ty.clone()]),
                Some(Narrowing::Instance(_)) => None,
                _ if !self.made.contains_key(name.as_str()) => {
                    match self.params.get(name.as_str()) {
                        Some(Declared::Param { ty, .. }) => Some(vec![(*ty).clone()]),
                        _ => None,
                    }
                }
                _ => None,
            },
            Root::Static(class, Some(prop)) => self.static_types(class, prop),
            _ => None,
        };
        let start = match root {
            Root::This => self.this_holder(),
            Root::Variable(Some(name)) => self.narrowed_holder(name, narrowing),
            Root::Static(class, Some(prop)) => self.static_holder(class, prop),
            // A superglobal is an array, no object; what its elements hold is
            // not known, since any code may store an object in one.
            Root::Global(Some(name)) if syntax::ast::is_superglobal(name) => Holder::none(),
            Root::Variable(None)
            | Root::Static(_, None)
            | Root::DynamicStatic(_)
            | Root::Global(_)
            | Root::Value => Holder::Unknown,
        };
        if steps.is_empty() {
            return start;
        }
        // The types of the value reached so far, once a step leaves the
        // object the walk starts from.
        let mut reached: Option<Vec<Type>> = typed;
        let classes = match (&reached, start) {
            (Some(_), _) => Holder::none_classes(),
            (None, Holder::Classes(classes) | Holder::Made(Some(classes))) => classes,
            (None, _) => return Holder::Unknown,
        };
        for step in steps {
            reached = Some(match (step, &reached) {
                // A value that is no object has no property.
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
        let mut found = NameList::new();
        for ty in reached.into_iter().flatten() {
            match object_classes(&ty) {
                Some(classes) => found.extend(classes),
                None => return Holder::Unknown,
            }
        }
        Holder::listed(found)
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
        let DeclKey { class, member, .. } = self;
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

syntax::canonical_struct!(Condition { at, this, destroys });

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
    Captured { closure, var },
    Unknown,
});

#[cfg(test)]
mod hierarchy_queries;
#[cfg(test)]
mod reference;

#[cfg(test)]
mod join_transferred_tests {
    use super::*;

    #[test]
    fn joining_a_transferred_effect_is_joining_the_effect_transferred() {
        let name = |text: &str| Some(Name::from(text));
        let storages = Effect(Some(Arc::new(BTreeSet::from([
            Storage::Own(name("a")),
            Storage::Own(None),
            Storage::Global(name("g")),
            Storage::Static {
                class: name("c"),
                prop: name("p"),
            },
        ]))));
        let bases = [
            Effect::nothing(),
            Effect(Some(Arc::new(BTreeSet::from([Storage::Global(name("h"))])))),
            Effect::anything(),
        ];
        let transfers = [
            Transfer::Same,
            Transfer::Other(name("k")),
            Transfer::Other(None),
            Transfer::Made,
            Transfer::Static,
        ];
        for other in [&storages, &Effect::nothing(), &Effect::anything()] {
            for base in &bases {
                for transfer in &transfers {
                    let mut expected = base.clone();
                    let expected_grew = expected.join(&other.transferred(transfer));
                    let mut got = base.clone();
                    let grew = got.join_transferred(other, transfer);
                    assert_eq!(got, expected);
                    assert_eq!(grew, expected_grew);
                }
            }
        }
    }

    fn shared(effect: &Effect, other: &Effect) -> bool {
        match (&effect.0, &other.0) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    #[test]
    fn a_join_that_adds_nothing_leaves_a_shared_effect_shared() {
        let name = |text: &str| Some(Name::from(text));
        let held = Effect(Some(Arc::new(BTreeSet::from([
            Storage::Global(name("g")),
            Storage::Own(name("a")),
        ]))));
        let subset = Effect(Some(Arc::new(BTreeSet::from([Storage::Global(name("g"))]))));

        let mut copy = held.clone();
        assert!(shared(&copy, &held));
        assert!(!copy.join(&subset));
        assert!(!copy.join(&Effect::nothing()));
        assert!(!copy.join_transferred(&subset, &Transfer::Same));
        assert!(!copy.join_transferred(&held, &Transfer::Same));
        assert!(shared(&copy, &held));
    }

    #[test]
    fn a_join_that_adds_unshares_and_leaves_the_original_alone() {
        let name = |text: &str| Some(Name::from(text));
        let held = Effect(Some(Arc::new(BTreeSet::from([Storage::Global(name("g"))]))));
        let extra = Effect(Some(Arc::new(BTreeSet::from([Storage::Global(name("h"))]))));

        let mut copy = held.clone();
        assert!(copy.join(&extra));
        assert!(!shared(&copy, &held));
        assert_eq!(held.storages().map(BTreeSet::len), Some(1));
        assert_eq!(copy.storages().map(BTreeSet::len), Some(2));

        let mut moved = held.clone();
        assert!(moved.join_transferred(
            &Effect(Some(Arc::new(BTreeSet::from([Storage::Own(name("p"))])))),
            &Transfer::Other(name("k")),
        ));
        assert_eq!(held.storages().map(BTreeSet::len), Some(1));
        assert!(moved.storages().unwrap().contains(&Storage::Property {
            class: name("k"),
            prop: name("p"),
        }));
    }
}

#[cfg(test)]
mod effect_id_tests {
    use super::*;
    use crate::registry::container::{Reader, Writer};
    use syntax::{lexer::Lexer, parser::Parser};

    fn registry(source: &str) -> Registry {
        let tokens = Lexer::new(source).tokenize_with_spans();
        let program = Parser::new(tokens).parse_program().unwrap();
        Registry::build(&program.arenas, program.statements())
    }

    fn cache_round_trip(registry: &Registry) -> Registry {
        let mut writer = Writer::new();
        registry.store(&mut writer).expect("stores");
        let reader = match Reader::new(writer.finish()) {
            Ok(reader) => reader,
            Err(_) => panic!("stored registry is a whole file"),
        };
        let mut back = Registry::load(&reader).expect("reads back");
        back.check_stored().expect("stored tables are ordered");
        back.freeze();
        back
    }

    #[test]
    fn closures_are_addressed_by_their_caller_id_and_ordinal() {
        let registry = registry(
            r#"<?php
                function work(): void {}
                class Worker { public function run(): void {} }
            "#,
        );
        let function = function_key("work");
        let first = registry.effect_id_for(&function.closure(0));
        let second = registry.effect_id_for(&function.closure(1));
        assert_eq!(
            first,
            EffectId::Closure {
                caller: Box::new(registry.effect_id_for(&function)),
                ordinal: 0,
            }
        );
        assert_ne!(first, second);

        let method = method_key("Worker", "run");
        let nested = registry.effect_id_for(&method.closure(3).closure(1));
        assert_eq!(
            nested,
            EffectId::Closure {
                caller: Box::new(EffectId::Closure {
                    caller: Box::new(registry.effect_id_for(&method)),
                    ordinal: 3,
                }),
                ordinal: 1,
            }
        );
    }

    #[test]
    fn property_hook_effect_ids_use_property_refs_and_round_trip_names() {
        let mut registry = registry(
            r#"<?php
                class Hooks {
                    public int $z { get {} }
                    public int $plain;
                    public int $a { get {} set {} }
                }
            "#,
        );
        let get_a = hook_key("Hooks", "a", false);
        let set_a = hook_key("Hooks", "a", true);
        let get_z = hook_key("Hooks", "z", false);
        let get_a_id = registry.effect_id_for(&get_a);
        let set_a_id = registry.effect_id_for(&set_a);
        let get_z_id = registry.effect_id_for(&get_z);
        assert_eq!(
            get_a_id,
            EffectId::PropertyHook {
                property: crate::registry::PropertyRef::new(registry.class_ids["Hooks"], 0),
                kind: PropertyHookKind::Get,
            }
        );
        assert_eq!(
            set_a_id,
            EffectId::PropertyHook {
                property: crate::registry::PropertyRef::new(registry.class_ids["Hooks"], 0),
                kind: PropertyHookKind::Set,
            }
        );
        assert_eq!(
            get_z_id,
            EffectId::PropertyHook {
                property: crate::registry::PropertyRef::new(registry.class_ids["Hooks"], 2),
                kind: PropertyHookKind::Get,
            }
        );

        registry
            .effects
            .insert(get_a_id.clone(), Settled::of(Effect::nothing()));
        let names = registry.effects_for_names();
        assert!(names.contains_key(&get_a));
        registry.rebuild_effect_ids(names);
        assert_eq!(
            registry.effects.get(&get_a_id),
            Some(&Settled::of(Effect::nothing()))
        );
        assert_eq!(
            get_a_id.clone().with_generation(77),
            EffectId::PropertyHook {
                property: crate::registry::PropertyRef::new(
                    registry.class_ids["Hooks"].with_generation(77),
                    0,
                ),
                kind: PropertyHookKind::Get,
            }
        );

        let back = cache_round_trip(&registry);
        let back_id = back.effect_id_for(&get_a);
        assert_eq!(
            back_id,
            EffectId::PropertyHook {
                property: crate::registry::PropertyRef::new(back.class_ids["Hooks"], 0),
                kind: PropertyHookKind::Get,
            }
        );
        assert_eq!(back.effect_decl_key(&back_id), Some(get_a));
    }
}
