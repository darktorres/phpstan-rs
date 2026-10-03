//! Hash-consing: a type is built once per exact form, so two types of the
//! same exact form are one node, and comparing them is comparing pointers.
//!
//! "Exact" is stricter than type equality (`PartialEq for Type`): it keeps
//! what equality ignores - a union's member order, a class name's letter
//! case, whether a `mixed` was written, a shape's next append index - so a
//! shared node never changes how a type prints or behaves. Children are
//! compared by pointer, being interned themselves.
//!
//! The table holds weak references: a type nothing uses any more is freed,
//! and its entry is dropped the next time its bucket or shard is visited.

use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex, OnceLock, Weak};

use rustc_hash::{FxHashMap, FxHasher};

use crate::{CallableSig, Node, Repr, Shape, TemplateParam, Type};

const SHARDS: usize = 64;

struct Shard {
    entries: FxHashMap<u64, Vec<Weak<Node>>>,
    // The entry count after the last sweep of dead entries; the next
    // sweep waits until the shard has doubled.
    swept_at: usize,
}

static TABLE: LazyLock<Vec<Mutex<Shard>>> = LazyLock::new(|| {
    (0..SHARDS)
        .map(|_| {
            Mutex::new(Shard {
                entries: FxHashMap::default(),
                swept_at: 1024,
            })
        })
        .collect()
});

/// The node for `repr`: the existing one of its exact form, or a new one
/// made from `repr` itself.
pub(crate) fn intern(repr: Repr, semantic_hash: fn(&Repr) -> u64) -> Arc<Node> {
    if let Some(constant) = Constant::of(&repr) {
        return constant.node(semantic_hash);
    }
    insert(Form::Owned(repr), semantic_hash)
}

/// The node for the form `repr` borrows: the existing one, or, when there is
/// none, a new one made from a copy of it. A form that is found is never
/// built: its parts are not copied, nor dropped after.
pub(crate) fn intern_ref(repr: ReprRef<'_>, semantic_hash: fn(&Repr) -> u64) -> Arc<Node> {
    insert(Form::Borrowed(repr), semantic_hash)
}

/// A form of type being looked for or made: one that is owned, to be moved
/// into the node if there is none, or one that borrows its parts.
enum Form<'a> {
    Owned(Repr),
    Borrowed(ReprRef<'a>),
}

impl Form<'_> {
    fn exact_hash(&self) -> u64 {
        match self {
            Form::Owned(repr) => exact_hash(&repr.as_ref()),
            Form::Borrowed(repr) => exact_hash(repr),
        }
    }

    fn is_form_of(&self, stored: &Repr) -> bool {
        match self {
            Form::Owned(repr) => exact_eq(stored, &repr.as_ref()),
            Form::Borrowed(repr) => exact_eq(stored, repr),
        }
    }

    fn into_repr(self) -> Repr {
        match self {
            Form::Owned(repr) => repr,
            Form::Borrowed(repr) => repr.to_owned(),
        }
    }
}

/// A form of type with its parts borrowed, for the kinds a caller usually
/// builds from parts it holds, and the owned form of every other. One
/// definition of the exact hash and of exact equality serves both: an owned
/// `Repr` is hashed and compared through its `ReprRef` (`Repr::as_ref`).
#[derive(Clone, Copy)]
pub(crate) enum ReprRef<'a> {
    StringLiteral(&'a crate::PhpString),
    Object(&'a crate::Name, &'a [Type]),
    EnumCase(&'a crate::Name, &'a crate::Name),
    ClassString(&'a Type),
    Union(&'a [Type]),
    BenevolentUnion(&'a [Type]),
    Intersection(&'a [Type]),
    /// Any other form, as it is owned.
    Owned(&'a Repr),
}

impl Repr {
    pub(crate) fn as_ref(&self) -> ReprRef<'_> {
        match self {
            Repr::StringLiteral(s) => ReprRef::StringLiteral(s),
            Repr::Object(name, args) => ReprRef::Object(name, args),
            Repr::EnumCase(class, case) => ReprRef::EnumCase(class, case),
            Repr::ClassString(inner) => ReprRef::ClassString(inner),
            Repr::Union(members) => ReprRef::Union(members),
            Repr::BenevolentUnion(members) => ReprRef::BenevolentUnion(members),
            Repr::Intersection(members) => ReprRef::Intersection(members),
            other => ReprRef::Owned(other),
        }
    }
}

impl<'a> ReprRef<'a> {
    /// The borrowed form an owned one has, when it has one: a `Repr` of a
    /// borrowable kind is never looked for as `Owned`, so the exact hash and
    /// exact equality have one case for each form.
    fn canonical(self) -> ReprRef<'a> {
        match self {
            ReprRef::Owned(repr) => repr.as_ref(),
            borrowed => borrowed,
        }
    }

    /// The owned form, copying the parts.
    pub(crate) fn to_owned(self) -> Repr {
        match self {
            ReprRef::StringLiteral(s) => Repr::StringLiteral(s.clone()),
            ReprRef::Object(name, args) => Repr::Object(name.clone(), args.to_vec()),
            ReprRef::EnumCase(class, case) => Repr::EnumCase(class.clone(), case.clone()),
            ReprRef::ClassString(inner) => Repr::ClassString(inner.clone()),
            ReprRef::Union(members) => Repr::Union(members.to_vec()),
            ReprRef::BenevolentUnion(members) => Repr::BenevolentUnion(members.to_vec()),
            ReprRef::Intersection(members) => Repr::Intersection(members.to_vec()),
            ReprRef::Owned(repr) => repr.clone(),
        }
    }
}

/// A type with nothing in it (`int`, `null`, `mixed`, ...): its node is
/// built once and kept, so the most common types skip the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Constant {
    Mixed,
    ExplicitMixed,
    Error,
    Never,
    Void,
    Null,
    Bool,
    True,
    False,
    Int,
    Float,
    String,
    ObjectWithoutClass,
    ParentRef,
    This,
}

impl Constant {
    #[cfg(test)]
    pub(crate) const ALL: [Constant; 15] = [
        Constant::Mixed,
        Constant::ExplicitMixed,
        Constant::Error,
        Constant::Never,
        Constant::Void,
        Constant::Null,
        Constant::Bool,
        Constant::True,
        Constant::False,
        Constant::Int,
        Constant::Float,
        Constant::String,
        Constant::ObjectWithoutClass,
        Constant::ParentRef,
        Constant::This,
    ];

    /// The constant `repr` is, if it is one.
    pub(crate) fn of(repr: &Repr) -> Option<Constant> {
        Some(match repr {
            Repr::Mixed(false, None) => Constant::Mixed,
            Repr::Mixed(true, None) => Constant::ExplicitMixed,
            Repr::Error => Constant::Error,
            Repr::Never => Constant::Never,
            Repr::Void => Constant::Void,
            Repr::Null => Constant::Null,
            Repr::Bool => Constant::Bool,
            Repr::True => Constant::True,
            Repr::False => Constant::False,
            Repr::Int => Constant::Int,
            Repr::Float => Constant::Float,
            Repr::String => Constant::String,
            Repr::ObjectWithoutClass => Constant::ObjectWithoutClass,
            Repr::ParentRef => Constant::ParentRef,
            Repr::This => Constant::This,
            _ => return None,
        })
    }

    pub(crate) fn repr(self) -> Repr {
        match self {
            Constant::Mixed => Repr::Mixed(false, None),
            Constant::ExplicitMixed => Repr::Mixed(true, None),
            Constant::Error => Repr::Error,
            Constant::Never => Repr::Never,
            Constant::Void => Repr::Void,
            Constant::Null => Repr::Null,
            Constant::Bool => Repr::Bool,
            Constant::True => Repr::True,
            Constant::False => Repr::False,
            Constant::Int => Repr::Int,
            Constant::Float => Repr::Float,
            Constant::String => Repr::String,
            Constant::ObjectWithoutClass => Repr::ObjectWithoutClass,
            Constant::ParentRef => Repr::ParentRef,
            Constant::This => Repr::This,
        }
    }

    /// The node every type of this constant shares.
    pub(crate) fn node(self, semantic_hash: fn(&Repr) -> u64) -> Arc<Node> {
        static NODES: OnceLock<[Arc<Node>; 15]> = OnceLock::new();
        let nodes = NODES.get_or_init(|| {
            [
                Constant::Mixed,
                Constant::ExplicitMixed,
                Constant::Error,
                Constant::Never,
                Constant::Void,
                Constant::Null,
                Constant::Bool,
                Constant::True,
                Constant::False,
                Constant::Int,
                Constant::Float,
                Constant::String,
                Constant::ObjectWithoutClass,
                Constant::ParentRef,
                Constant::This,
            ]
            .map(|constant| insert(Form::Owned(constant.repr()), semantic_hash))
        });
        nodes[self as usize].clone()
    }
}

/// How many nodes a thread keeps close at hand (`FRONT`).
const FRONT_SLOTS: usize = 4096;

thread_local! {
    /// The nodes this thread found or made most lately, each in the slot
    /// its exact hash picks (a newer node takes the slot of an older one):
    /// a type looked for again is found here by comparing it with one
    /// node, without the shard's lock or a weak reference to upgrade. A
    /// node is held strongly, so a few thousand per thread outlive their
    /// users; each is the node the table has for its form.
    static FRONT: std::cell::RefCell<Vec<Option<Arc<Node>>>> =
        std::cell::RefCell::new(vec![None; FRONT_SLOTS]);
}

fn insert(form: Form<'_>, semantic_hash: fn(&Repr) -> u64) -> Arc<Node> {
    let exact = form.exact_hash();
    let slot = (exact as usize) % FRONT_SLOTS;
    let near = FRONT.with(|front| {
        let front = front.borrow();
        match &front[slot] {
            Some(node) if node.exact == exact && form.is_form_of(&node.repr) => {
                Some(Arc::clone(node))
            }
            _ => None,
        }
    });
    if let Some(node) = near {
        return node;
    }
    let node = insert_shared(form, semantic_hash, exact);
    FRONT.with(|front| front.borrow_mut()[slot] = Some(Arc::clone(&node)));
    node
}

fn insert_shared(form: Form<'_>, semantic_hash: fn(&Repr) -> u64, exact: u64) -> Arc<Node> {
    let shard = &TABLE[(exact as usize) % SHARDS];
    let mut shard = shard
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(bucket) = shard.entries.get_mut(&exact) {
        // A dead entry upgrades to nothing; they are dropped from the
        // bucket where a node is added to it.
        for weak in bucket.iter() {
            if let Some(node) = weak.upgrade()
                && form.is_form_of(&node.repr)
            {
                return node;
            }
        }
        bucket.retain(|weak| weak.strong_count() > 0);
    }
    let repr = form.into_repr();
    let hash = semantic_hash(&repr);
    let flags = crate::reference_flags(&repr);
    let node = Arc::new(Node {
        repr,
        hash,
        exact,
        flags,
        refined: std::sync::atomic::AtomicU64::new(0),
    });
    shard
        .entries
        .entry(exact)
        .or_default()
        .push(Arc::downgrade(&node));
    if shard.entries.len() > 2 * shard.swept_at {
        shard.entries.retain(|_, bucket| {
            bucket.retain(|weak| weak.strong_count() > 0);
            !bucket.is_empty()
        });
        shard.swept_at = shard.entries.len().max(1024);
    }
    node
}

fn same(a: &Type, b: &Type) -> bool {
    Arc::ptr_eq(&a.0, &b.0)
}

fn same_all(a: &[Type], b: &[Type]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(x, y))
}

fn same_option(a: &Option<Type>, b: &Option<Type>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => same(a, b),
        _ => false,
    }
}

fn same_shape(a: &Shape, b: &Shape) -> bool {
    a.fields().len() == b.fields().len()
        && a.fields()
            .iter()
            .zip(b.fields())
            .all(|(x, y)| x.key == y.key && same(&x.ty, &y.ty) && x.optional == y.optional)
        && match (&a.unsealed, &b.unsealed) {
            (None, None) => true,
            (Some(a), Some(b)) => same(&a.0, &b.0) && same(&a.1, &b.1),
            _ => false,
        }
        && a.next_index == b.next_index
}

fn same_template(a: &TemplateParam, b: &TemplateParam) -> bool {
    a.name == b.name
        && same(&a.bound, &b.bound)
        && a.variance == b.variance
        && a.scope == b.scope
        && match (&a.default, &b.default) {
            (None, None) => true,
            (Some(a), Some(b)) => same(a, b),
            _ => false,
        }
}

fn same_callable(a: &CallableSig, b: &CallableSig) -> bool {
    a.params.len() == b.params.len()
        && a.params.iter().zip(&b.params).all(|(x, y)| {
            same(&x.ty, &y.ty)
                && x.variadic == y.variadic
                && x.optional == y.optional
                && x.by_ref == y.by_ref
        })
        && same(&a.return_type, &b.return_type)
        && a.templates.len() == b.templates.len()
        && a.templates
            .iter()
            .zip(&b.templates)
            .all(|(x, y)| same_template(x, y))
}

/// Whether `a` and `b` are the same exact form, for the forms that are not
/// borrowed (`ReprRef`).
fn exact_eq_owned(a: &Repr, b: &Repr) -> bool {
    match (a, b) {
        (Repr::Mixed(aw, a), Repr::Mixed(bw, b)) => aw == bw && same_option(a, b),
        (Repr::Error, Repr::Error)
        | (Repr::Never, Repr::Never)
        | (Repr::Void, Repr::Void)
        | (Repr::Null, Repr::Null)
        | (Repr::Bool, Repr::Bool)
        | (Repr::True, Repr::True)
        | (Repr::False, Repr::False)
        | (Repr::Int, Repr::Int)
        | (Repr::Float, Repr::Float)
        | (Repr::String, Repr::String)
        | (Repr::ObjectWithoutClass, Repr::ObjectWithoutClass)
        | (Repr::ParentRef, Repr::ParentRef)
        | (Repr::This, Repr::This) => true,
        (Repr::IntLiteral(a), Repr::IntLiteral(b)) => a == b,
        (Repr::Array(ak, av), Repr::Array(bk, bv))
        | (Repr::OffsetAccess(ak, av), Repr::OffsetAccess(bk, bv)) => same(ak, bk) && same(av, bv),
        (Repr::ValueOf(a), Repr::ValueOf(b)) | (Repr::KeyOf(a), Repr::KeyOf(b)) => same(a, b),
        (Repr::ClassKindString(a), Repr::ClassKindString(b)) => a == b,
        (Repr::ArrayShape(a), Repr::ArrayShape(b))
        | (Repr::ObjectShape(a), Repr::ObjectShape(b)) => Arc::ptr_eq(a, b) || same_shape(a, b),
        (Repr::Resource { closed: a }, Repr::Resource { closed: b }) => a == b,
        (Repr::Callable(a), Repr::Callable(b)) => match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b) || same_callable(a, b),
            _ => false,
        },
        (Repr::SelfRef(a), Repr::SelfRef(b)) | (Repr::StaticRef(a), Repr::StaticRef(b)) => {
            same_all(a, b)
        }
        (Repr::TemplateParam(a), Repr::TemplateParam(b)) => same_template(a, b),
        (Repr::VarianceArg(av, a), Repr::VarianceArg(bv, b)) => av == bv && same(a, b),
        (Repr::IntRange(amin, amax), Repr::IntRange(bmin, bmax)) => amin == bmin && amax == bmax,
        (Repr::FloatLiteral(a), Repr::FloatLiteral(b)) => a.to_bits() == b.to_bits(),
        (Repr::Accessory(a), Repr::Accessory(b)) => a == b,
        (Repr::HasOffsetValue(ak, av), Repr::HasOffsetValue(bk, bv)) => ak == bk && same(av, bv),
        (Repr::HasOffset(a), Repr::HasOffset(b)) => a == b,
        (Repr::HasMethod(a), Repr::HasMethod(b)) | (Repr::HasProperty(a), Repr::HasProperty(b)) => {
            a == b
        }
        (Repr::ReflectedMember(ao, an), Repr::ReflectedMember(bo, bn)) => same(ao, bo) && an == bn,
        (
            Repr::ClassConstant {
                class: ac,
                pattern: ap,
            },
            Repr::ClassConstant {
                class: bc,
                pattern: bp,
            },
        ) => same(ac, bc) && ap == bp,
        (Repr::Extension(a), Repr::Extension(b)) => Arc::ptr_eq(a, b) || a.eq_data(&**b),
        (
            Repr::Conditional {
                subject: a_subject,
                target: a_target,
                if_true: a_if_true,
                if_false: a_if_false,
                negated: a_negated,
            },
            Repr::Conditional {
                subject: b_subject,
                target: b_target,
                if_true: b_if_true,
                if_false: b_if_false,
                negated: b_negated,
            },
        ) => {
            a_subject == b_subject
                && same(a_target, b_target)
                && same(a_if_true, b_if_true)
                && same(a_if_false, b_if_false)
                && a_negated == b_negated
        }
        _ => false,
    }
}

/// Whether the node form `stored` is the exact form `form` borrows or owns.
fn exact_eq(stored: &Repr, form: &ReprRef<'_>) -> bool {
    match form.canonical() {
        ReprRef::StringLiteral(b) => matches!(stored, Repr::StringLiteral(a) if a == b),
        ReprRef::Object(bn, ba) => {
            matches!(stored, Repr::Object(an, aa) if an == bn && same_all(aa, ba))
        }
        ReprRef::EnumCase(bc, bk) => {
            matches!(stored, Repr::EnumCase(ac, ak) if ac == bc && ak == bk)
        }
        ReprRef::ClassString(b) => matches!(stored, Repr::ClassString(a) if same(a, b)),
        ReprRef::Union(b) => matches!(stored, Repr::Union(a) if same_all(a, b)),
        ReprRef::BenevolentUnion(b) => {
            matches!(stored, Repr::BenevolentUnion(a) if same_all(a, b))
        }
        ReprRef::Intersection(b) => matches!(stored, Repr::Intersection(a) if same_all(a, b)),
        ReprRef::Owned(b) => exact_eq_owned(stored, b),
    }
}

fn hash_type(ty: &Type, state: &mut FxHasher) {
    state.write_u64(ty.0.exact);
}

fn hash_template(tp: &TemplateParam, state: &mut FxHasher) {
    tp.name.hash(state);
    hash_type(&tp.bound, state);
    tp.variance.hash(state);
    tp.scope.hash(state);
    if let Some(default) = &tp.default {
        hash_type(default, state);
    }
}

/// The hash of the exact form `repr` is, for the forms that are not borrowed
/// (`ReprRef`), from its children's.
fn exact_hash_owned(repr: &Repr) -> u64 {
    let mut hasher = FxHasher::default();
    let state = &mut hasher;
    std::mem::discriminant(repr).hash(state);
    match repr {
        Repr::Mixed(written, subtracted) => {
            written.hash(state);
            if let Some(subtracted) = subtracted {
                hash_type(subtracted, state);
            }
        }
        Repr::Error
        | Repr::Never
        | Repr::Void
        | Repr::Null
        | Repr::Bool
        | Repr::True
        | Repr::False
        | Repr::Int
        | Repr::Float
        | Repr::String
        | Repr::ObjectWithoutClass
        | Repr::ParentRef
        | Repr::This => {}
        Repr::IntLiteral(n) => n.hash(state),
        Repr::Array(a, b) | Repr::OffsetAccess(a, b) => {
            hash_type(a, state);
            hash_type(b, state);
        }
        Repr::ValueOf(inner) | Repr::KeyOf(inner) => hash_type(inner, state),
        Repr::ClassKindString(kind) => kind.hash(state),
        Repr::ArrayShape(shape) | Repr::ObjectShape(shape) => {
            for field in shape.fields() {
                field.key.hash(state);
                hash_type(&field.ty, state);
                field.optional.hash(state);
            }
            if let Some(tail) = &shape.unsealed {
                hash_type(&tail.0, state);
                hash_type(&tail.1, state);
            }
        }
        Repr::Resource { closed } => closed.hash(state),
        Repr::Callable(sig) => {
            if let Some(sig) = sig {
                for param in &sig.params {
                    hash_type(&param.ty, state);
                    (param.variadic, param.optional, param.by_ref).hash(state);
                }
                hash_type(&sig.return_type, state);
                for tp in &sig.templates {
                    hash_template(tp, state);
                }
            }
        }
        Repr::SelfRef(args) | Repr::StaticRef(args) => {
            for arg in args {
                hash_type(arg, state);
            }
        }
        Repr::TemplateParam(tp) => hash_template(tp, state),
        Repr::VarianceArg(variance, inner) => {
            variance.hash(state);
            hash_type(inner, state);
        }
        Repr::IntRange(min, max) => {
            min.hash(state);
            max.hash(state);
        }
        Repr::FloatLiteral(f) => f.to_bits().hash(state),
        Repr::Accessory(kind) => kind.hash(state),
        Repr::HasOffsetValue(key, value) => {
            key.hash(state);
            hash_type(value, state);
        }
        Repr::HasOffset(key) => key.hash(state),
        Repr::HasMethod(name) | Repr::HasProperty(name) => name.hash(state),
        Repr::ReflectedMember(owner, member) => {
            hash_type(owner, state);
            member.hash(state);
        }
        Repr::ClassConstant { class, pattern } => {
            hash_type(class, state);
            pattern.hash(state);
        }
        Repr::Extension(data) => {
            let state: &mut dyn Hasher = state;
            data.hash_data(state);
        }
        Repr::Conditional {
            subject,
            target,
            if_true,
            if_false,
            negated,
        } => {
            subject.hash(state);
            hash_type(target, state);
            hash_type(if_true, state);
            hash_type(if_false, state);
            negated.hash(state);
        }
        // Borrowed forms: `exact_hash` hashes these through `ReprRef`.
        Repr::StringLiteral(_)
        | Repr::Object(..)
        | Repr::EnumCase(..)
        | Repr::ClassString(_)
        | Repr::Union(_)
        | Repr::BenevolentUnion(_)
        | Repr::Intersection(_) => unreachable!("hashed as a ReprRef"),
    }
    hasher.finish()
}

/// The hash of an exact form, from its children's: the one definition, for
/// a form owned and a form borrowed.
fn exact_hash(form: &ReprRef<'_>) -> u64 {
    let mut hasher = FxHasher::default();
    let state = &mut hasher;
    let members = |tag: u8, members: &[Type], state: &mut FxHasher| {
        tag.hash(state);
        for member in members {
            hash_type(member, state);
        }
    };
    match form.canonical() {
        ReprRef::StringLiteral(s) => {
            0xa0u8.hash(state);
            s.hash(state);
        }
        ReprRef::Object(name, args) => {
            0xa1u8.hash(state);
            name.hash(state);
            for arg in args {
                hash_type(arg, state);
            }
        }
        ReprRef::EnumCase(class, case) => {
            0xa2u8.hash(state);
            class.hash(state);
            case.hash(state);
        }
        ReprRef::ClassString(inner) => {
            0xa3u8.hash(state);
            hash_type(inner, state);
        }
        ReprRef::Union(args) => members(0xa4, args, state),
        ReprRef::BenevolentUnion(args) => members(0xa5, args, state),
        ReprRef::Intersection(args) => members(0xa6, args, state),
        ReprRef::Owned(repr) => return exact_hash_owned(repr),
    }
    hasher.finish()
}

/// Which operation a memoized result is of.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Operation {
    Union,
    Without,
    Intersect,
}

// A memo entry keeps its arguments alive, so their nodes (and addresses)
// stay the ones its key names; they are held, and never read.
#[allow(dead_code)]
enum Kept {
    // Every operation but an intersection has two arguments.
    Two(Type, Type),
    Many(Vec<Type>),
}

type MemoEntry = (Kept, Type);

/// A memo key: the operation and its arguments' node addresses, the first
/// two inline (every operation but an intersection has two).
#[derive(PartialEq, Eq, Hash)]
struct MemoKey {
    operation: Operation,
    first: usize,
    second: usize,
    rest: Box<[usize]>,
}

const MEMO_CAPACITY: usize = 1 << 16;

thread_local! {
    static MEMO: std::cell::RefCell<FxHashMap<MemoKey, MemoEntry>> =
        std::cell::RefCell::new(FxHashMap::default());
}

fn address(ty: &Type) -> usize {
    Arc::as_ptr(&ty.0) as usize
}

fn remembered(key: &MemoKey) -> Option<Type> {
    MEMO.with(|memo| memo.borrow().get(key).map(|(_, result)| result.clone()))
}

fn remember(key: MemoKey, kept: Kept, result: &Type) {
    MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        if memo.len() >= MEMO_CAPACITY {
            memo.clear();
        }
        memo.insert(key, (kept, result.clone()));
    });
}

/// `compute`'s result for `operation` over `a` and `b`, computed once per
/// pair on each thread: the arguments are interned, so the same pair is
/// the same nodes. Only for an operation that asks nothing but the types
/// (no class hierarchy), whose result `compute` alone decides. A hit
/// copies nothing; a miss copies the pair once, to keep it alive.
pub(crate) fn memoized_pair(
    operation: Operation,
    a: &Type,
    b: &Type,
    compute: impl FnOnce() -> Type,
) -> Type {
    let key = MemoKey {
        operation,
        first: address(a),
        second: address(b),
        rest: Box::default(),
    };
    if let Some(result) = remembered(&key) {
        return result;
    }
    let result = compute();
    remember(key, Kept::Two(a.clone(), b.clone()), &result);
    result
}

/// `memoized_pair` for a `compute` that takes its two arguments by value.
pub(crate) fn memoized_owned(
    operation: Operation,
    a: Type,
    b: Type,
    compute: impl FnOnce(Type, Type) -> Type,
) -> Type {
    let key = MemoKey {
        operation,
        first: address(&a),
        second: address(&b),
        rest: Box::default(),
    };
    if let Some(result) = remembered(&key) {
        return result;
    }
    let kept = Kept::Two(a.clone(), b.clone());
    let result = compute(a, b);
    remember(key, kept, &result);
    result
}

/// `memoized_pair` over any number of arguments (an intersection's members).
pub(crate) fn memoized_many(
    operation: Operation,
    arguments: &[Type],
    compute: impl FnOnce() -> Type,
) -> Type {
    let key = MemoKey {
        operation,
        first: arguments.first().map_or(0, address),
        second: arguments.get(1).map_or(0, address),
        rest: arguments.iter().skip(2).map(address).collect(),
    };
    if let Some(result) = remembered(&key) {
        return result;
    }
    let result = compute();
    remember(key, Kept::Many(arguments.to_vec()), &result);
    result
}
