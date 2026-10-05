//! The tags that tell which member of a union a record holds.
//!
//! A key is a byte of a record that the program's conditions compare
//! with constants, with the record's unions that may hold a pointer and
//! do not hold that byte: its region (all of a union whose member structs
//! begin with the byte). A function's read of a region is under the
//! values the byte has on every path to the read, from the conditions
//! and the writes of the byte before it (the read's context); a write of
//! a region is under the values the byte is next written with on every
//! path from it, or else has where it is written (the write's part). A
//! read sees no write whose part shares no value with its context.
//!
//! That holds while no write of the byte leaves the region holding what
//! was written under another value: a write of the byte is paired where
//! the region is written before it on every path, with no write of the
//! key between, or after it in its block, and a byte copied from another
//! record with the region in its block is paired by the copy. The two
//! write one instance where they reach it through one register variable,
//! variable or global, or through one pointer computed again while
//! nothing assigns a register it is computed from or writes memory it
//! reads, as C's typing of memory tells which writes may. A call
//! writes the byte where its callee may, through the members of the
//! key's record or a value of a record holding it, itself or through its
//! calls. The values a write of the byte writes are known where they are
//! a few constants: a constant, a register variable holding constants on
//! every path (folded through the operations on them and narrowed by the
//! conditions comparing it with constants), what a call returns whose
//! callee returns only constants, or a byte read where the conditions
//! show its values. A write that is not paired leaves the values it
//! writes, every value where they are not known, in every part of the key
//! in the objects it may write, and in the parts of the cells copied out
//! of them. The byte is taken to be written through the record's members;
//! the writes of narrow integers through pointers that may reach one are
//! counted once the analysis is solved.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::survey::{
    Base, Conversion, FEW, Span, Visit, constant, few, fold_binary, fold_unary, is_comparison,
    locate, strip_casts, walk_expr, walk_function,
};
use super::{COPY, Cons, DefId, FnId, GlobalId, Phase, Pta, StaticOp, TagSetId, gcd};
use crate::cfg::successors;
use crate::ir::{BinOp, BlockId, Callee, Expr, ExprKind, Function, Place, Stmt, Term, UnOp, VarId};
use crate::lower::convert_to;
use crate::types::Shape;

pub(crate) type KeyId = u32;

/// The bit of the values a key's domain does not name.
pub(crate) const OTHER: u64 = 1 << 63;
/// How many values a key's domain names.
pub(crate) const DOMAIN: usize = 63;
/// The key of a call's variable arguments, its values the classes of the
/// `va_arg` types.
pub(crate) const VARIADIC: KeyId = KeyId::MAX;
/// How many blocks the search for a write of a byte reads.
const SEARCH_BLOCKS: usize = 64;

pub(crate) struct Key {
    pub(crate) record: Arc<str>,
    /// Where the byte is in the record.
    pub(crate) tag: i64,
    /// Where the region starts and ends in the record.
    pub(crate) region: (i64, i64),
    pub(crate) size: i64,
    /// The values the bits of a set of values name, in order; any other
    /// is `OTHER`.
    pub(crate) domain: Vec<u8>,
    /// The first writes of the byte that are not paired and whose values
    /// are not known.
    pub(crate) unsplit_by: Vec<String>,
    /// The values the writes of the byte that are not paired write, where
    /// known.
    pub(crate) unclean_values: u64,
    /// The first writes that are not paired of each unclean value, and
    /// whether each is rooted.
    pub(crate) unclean_by: Vec<(u64, String, bool)>,
    pub(crate) counts: KeyCounts,
}

/// What the functions do with a key, for the report.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct KeyCounts {
    pub(crate) tag_writes: usize,
    pub(crate) correlated: usize,
    pub(crate) unpaired_constant: usize,
    pub(crate) unpaired: usize,
    pub(crate) region_writes: usize,
    pub(crate) copies: usize,
    pub(crate) searched: usize,
    pub(crate) parts: usize,
    pub(crate) contexts: usize,
    pub(crate) unrooted: usize,
}

impl Key {
    pub(crate) fn bit(&self, byte: u8) -> u64 {
        match self.domain.binary_search(&byte) {
            Ok(i) => 1 << i,
            Err(_) => OTHER,
        }
    }

    pub(crate) fn full(&self) -> u64 {
        ((1u64 << self.domain.len()) - 1) | OTHER
    }

    /// The values a byte may have where a test holds of it, read as
    /// signed or unsigned.
    fn bits_where(&self, test: impl Fn(i128) -> bool) -> u64 {
        let mut bits = 0;
        for byte in 0..=255u8 {
            if test(i128::from(byte)) || test(i128::from(byte as i8)) {
                bits |= self.bit(byte);
            }
        }
        bits
    }

    /// A set of values as the report writes it.
    pub(crate) fn describe(&self, values: u64) -> String {
        let mut names: Vec<String> = self
            .domain
            .iter()
            .enumerate()
            .filter(|(i, _)| values & (1 << i) != 0)
            .map(|(_, byte)| byte.to_string())
            .collect();
        if values & OTHER != 0 {
            names.push("other".to_owned());
        }
        names.join(",")
    }
}

#[derive(Default)]
pub(crate) struct Model {
    pub(crate) keys: Vec<Key>,
    pub(crate) by_record: FxHashMap<Arc<str>, Vec<KeyId>>,
    /// The records whose members may be in a key's record: the keys'
    /// records and the structs that are members of them.
    keyed: FxHashSet<Arc<str>>,
    /// The keys whose byte each function may write, itself or through
    /// its calls, as bits by key; every key where none.
    pub(crate) writers: Vec<Option<Box<[u64]>>>,
    /// The keys whose byte a value of each record may hold.
    within: FxHashMap<Arc<str>, Box<[u64]>>,
    /// The records each record holds, for the bytes of memory the
    /// functions' paths read.
    nested: Nested,
}

/// Keys as bits, every key where none.
type KeyBits = Option<Box<[u64]>>;

fn add_bits(into: &mut KeyBits, more: &KeyBits) -> bool {
    match (into.as_mut(), more) {
        (None, _) => false,
        (Some(_), None) => {
            *into = None;
            true
        }
        (Some(into), Some(more)) => {
            let mut changed = false;
            for (word, &more) in into.iter_mut().zip(more.iter()) {
                if *word | more != *word {
                    *word |= more;
                    changed = true;
                }
            }
            changed
        }
    }
}

fn bit_keys(bits: &[u64]) -> impl Iterator<Item = KeyId> + '_ {
    bits.iter().enumerate().flat_map(|(i, &word)| {
        (0..64)
            .filter(move |b| word & (1 << b) != 0)
            .map(move |b| (i * 64 + b) as KeyId)
    })
}

/// What the constraints of a function's places are under, by the place
/// (its address in the function's code).
#[derive(Default)]
pub(crate) struct Notes {
    /// The part a write of a region is under.
    pub(crate) parts: FxHashMap<usize, TagSetId>,
    /// The context a read of a region is under.
    pub(crate) ctxs: FxHashMap<usize, TagSetId>,
    /// The writes of a region that copy the bytes another record's region
    /// held, by the variable the bytes were read into: the record's byte
    /// is copied with them.
    pub(crate) copies: FxHashMap<usize, VarId>,
    /// The writes of a byte: each key, and where its byte is in the bytes
    /// written.
    pub(crate) tags: FxHashMap<usize, Vec<(KeyId, u32)>>,
    /// The writes of a byte that are not paired: each key, and the values
    /// they write, every value where not known. The values are in every
    /// part of the key in the objects the writes may write.
    pub(crate) unpaired: FxHashMap<usize, Vec<(KeyId, u64)>>,
}

/// A set of byte values, as 256 bits.
type ByteSet = [u64; 4];

const ALL_BYTES: ByteSet = [u64::MAX; 4];

fn byte_set(bytes: impl IntoIterator<Item = u8>) -> ByteSet {
    let mut set = [0u64; 4];
    for byte in bytes {
        set[usize::from(byte >> 6)] |= 1 << (byte & 63);
    }
    set
}

fn set_bytes(set: ByteSet) -> impl Iterator<Item = u8> {
    (0..=255u8).filter(move |&byte| set[usize::from(byte >> 6)] & (1 << (byte & 63)) != 0)
}

/// The bytes an operation gives on each pair of bytes of two sets.
fn combine(op: BinOp, left: ByteSet, right: ByteSet) -> ByteSet {
    if left == ALL_BYTES && right == ALL_BYTES {
        return ALL_BYTES;
    }
    let mut out = [0u64; 4];
    for a in set_bytes(left) {
        for b in set_bytes(right) {
            let byte = match op {
                BinOp::BitOr => a | b,
                BinOp::BitAnd => a & b,
                _ => a ^ b,
            };
            out[usize::from(byte >> 6)] |= 1 << (byte & 63);
        }
    }
    out
}

/// The byte at `byte` of an integer, in memory's order.
fn byte_of(value: i128, byte: i64) -> u8 {
    match u32::try_from(byte) {
        Ok(byte) if byte < 16 => ((value >> (8 * byte)) & 0xff) as u8,
        _ if value < 0 => 0xff,
        _ => 0,
    }
}

pub(crate) fn place_key(place: &Place) -> usize {
    std::ptr::from_ref(place).addr()
}

/// What a record instance is reached from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Root {
    /// What a register variable points to.
    Reg(VarId),
    /// A variable in memory.
    Mem(VarId),
    Glob(GlobalId),
    /// What a pointer the function computes points to, while nothing it
    /// is computed from changes.
    Path(PathId),
}

/// A record instance: its root, and where it starts from it.
type At = (Root, i64);

type PathId = u32;

/// A variable or a global some bytes are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Named {
    /// A register variable, which no pointer reaches.
    Reg(VarId),
    Mem(VarId),
    Glob(GlobalId),
}

/// The scalar types an access of memory is by, as C's typing of memory
/// tells which may reach the same bytes: a character type's any, an
/// integer's or an enumeration's an integer as wide, a pointer's any
/// pointer, a floating type's one as wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Char,
    Int(u32),
    Float(u32),
    Pointer,
}

/// Where a record's instances are in a record holding it: from `first`,
/// `count` of them `stride` bytes apart, or a superset of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Instances {
    first: i64,
    stride: i64,
    count: i64,
}

impl Instances {
    /// The instances of a record held at these places of an instance of
    /// a record held at `outer`.
    fn within(self, outer: Instances) -> Instances {
        let first = outer.first.saturating_add(self.first);
        if outer.count == 1 {
            return Instances { first, ..self };
        }
        if self.count == 1 {
            return Instances { first, ..outer };
        }
        let stride = gcd(outer.stride as u64, self.stride as u64).max(1) as i64;
        let span = (outer.count - 1)
            .saturating_mul(outer.stride)
            .saturating_add((self.count - 1).saturating_mul(self.stride));
        Instances {
            first,
            stride,
            count: (span / stride).saturating_add(1),
        }
    }

    /// Whether bytes from `start` for `width` may overlap those from
    /// `inner` for `inner_width` in one of the instances.
    fn overlaps(self, start: i64, width: i64, inner: i64, inner_width: i64) -> bool {
        // An instance overlaps where it starts from `low` to `high`.
        let low = start - inner - inner_width + 1;
        let high = start - inner + width - 1;
        if self.count <= 1 || self.stride <= 0 {
            return low <= self.first && self.first <= high;
        }
        let k = if low <= self.first {
            0
        } else {
            (low - self.first + self.stride - 1) / self.stride
        };
        k < self.count && self.first.saturating_add(k.saturating_mul(self.stride)) <= high
    }
}

/// The records each record holds, with where their instances are; none
/// for a record holding one whose members are not known.
type Nested = FxHashMap<Arc<str>, Option<Held>>;

/// The records a record holds, with where their instances are.
type Held = Box<[(Arc<str>, Instances)]>;

/// Bytes of memory read or written.
#[derive(Debug, Clone)]
struct Bytes {
    /// The variable or global they are in, where named.
    named: Option<Named>,
    /// Where they start in it, where known.
    start: Option<i64>,
    width: i64,
    /// The records they are in through the members accessed, outermost
    /// first, each with where they start in its instance, where known.
    chain: Vec<(Arc<str>, Option<i64>)>,
    /// The scalar type they are accessed by; none for a record or a type
    /// not known.
    kind: Option<Kind>,
}

impl Bytes {
    /// Moves the bytes on by a number of bytes; to where not known for
    /// one not known.
    fn shift(&mut self, by: Option<i64>) {
        let shift = |at: &mut Option<i64>| *at = at.zip(by).and_then(|(at, by)| at.checked_add(by));
        shift(&mut self.start);
        for (_, at) in &mut self.chain {
            shift(at);
        }
    }

    /// Whether writing these bytes may change `read`'s, as C's typing of
    /// memory lets an access reach bytes: of one variable or global,
    /// where they overlap, no pointer reaching a register variable; in a
    /// record both are in, where they overlap in its instance, the
    /// instances of a record being apart or one; else where the outermost
    /// record of one holds the other's and they overlap in one of its
    /// instances. Bytes in no record are in a variable or global of their
    /// own type where named, else reach the bytes of a scalar type that
    /// may alias theirs.
    fn may_change(&self, read: &Bytes, nested: &Nested) -> bool {
        let overlap = |a: Option<i64>, b: Option<i64>| match (a, b) {
            (Some(a), Some(b)) => a < b + read.width && b < a + self.width,
            _ => true,
        };
        match (self.named, read.named) {
            (Some(named), Some(other)) => {
                return named == other && overlap(self.start, read.start);
            }
            (Some(Named::Reg(_)), None) | (None, Some(Named::Reg(_))) => return false,
            _ => {}
        }
        let mut common = false;
        for (record, at) in &self.chain {
            for (other, other_at) in &read.chain {
                if record == other {
                    if !overlap(*at, *other_at) {
                        return false;
                    }
                    common = true;
                }
            }
        }
        if common {
            return true;
        }
        match (self.chain.first(), read.chain.first()) {
            (Some((outer, at)), Some((other, other_at))) => {
                holds(
                    nested,
                    (outer, *at, self.width),
                    (other, *other_at, read.width),
                ) || holds(
                    nested,
                    (other, *other_at, read.width),
                    (outer, *at, self.width),
                )
            }
            (Some(_), None) if read.named.is_some() => false,
            (None, Some(_)) if self.named.is_some() => false,
            _ => match (self.kind, read.kind) {
                (Some(kind), Some(other)) => {
                    kind == Kind::Char || other == Kind::Char || kind == other
                }
                _ => true,
            },
        }
    }
}

/// Whether bytes of an instance of a record may overlap bytes of an
/// instance of another record it holds.
fn holds(
    nested: &Nested,
    (outer, at, width): (&Arc<str>, Option<i64>, i64),
    (inner, inner_at, inner_width): (&Arc<str>, Option<i64>, i64),
) -> bool {
    let Some(Some(held)) = nested.get(outer) else {
        return true;
    };
    held.iter()
        .filter(|(record, _)| record == inner)
        .any(|(_, instances)| match (at, inner_at) {
            (Some(at), Some(inner_at)) => instances.overlaps(at, width, inner_at, inner_width),
            _ => true,
        })
}

/// What a path is computed from: register variables, and bytes of
/// memory it reads.
#[derive(Default)]
struct Path {
    regs: Vec<VarId>,
    loads: Vec<Bytes>,
}

/// A function's paths: pointers it computes from register variables and
/// memory, and addresses of elements of arrays indexed by values that
/// are not constant; `None` for one computed from what is not known.
#[derive(Default)]
struct Paths {
    pointers: FxHashMap<Expr, Option<PathId>>,
    elements: FxHashMap<Place, Option<PathId>>,
    paths: Vec<Path>,
}

/// What a statement changes.
#[derive(Debug, Clone)]
enum Event {
    /// A register variable assigned.
    Reassign(VarId),
    /// Bytes of memory written.
    Write(Bytes),
    /// Any memory: a call or assembly.
    Any,
}

impl Paths {
    fn add(&mut self, mut path: Path) -> PathId {
        path.regs.sort_unstable();
        path.regs.dedup();
        self.paths.push(path);
        (self.paths.len() - 1) as PathId
    }

    /// Whether an event may change what a root reaches.
    fn affects(&self, nested: &Nested, event: &Event, root: Root) -> bool {
        match root {
            Root::Reg(var) => matches!(event, Event::Reassign(v) if *v == var),
            Root::Mem(_) | Root::Glob(_) => false,
            Root::Path(path) => {
                let path = &self.paths[path as usize];
                match event {
                    Event::Reassign(var) => path.regs.contains(var),
                    Event::Write(bytes) => {
                        path.loads.iter().any(|load| bytes.may_change(load, nested))
                    }
                    Event::Any => !path.loads.is_empty(),
                }
            }
        }
    }
}

/// A value that is a key's byte of an instance, or holds it in its low
/// byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct View {
    at: At,
    key: KeyId,
    /// Whether the value is the byte alone.
    exact: bool,
}

/// What holds at a point of a function.
#[derive(Clone, Default, PartialEq)]
struct State {
    /// Where each register variable points.
    alias: FxHashMap<VarId, At>,
    /// The values each instance's byte may have; any where absent.
    tags: FxHashMap<(At, KeyId), u64>,
    /// The register variables holding a byte read.
    binds: FxHashMap<VarId, View>,
    /// The constants each register variable may hold.
    consts: FxHashMap<VarId, Vec<i128>>,
    /// The instances whose region was written on every path since the
    /// key was last written.
    written: FxHashSet<(At, KeyId)>,
}

impl State {
    fn meet(&self, other: &State) -> State {
        State {
            alias: self
                .alias
                .iter()
                .filter(|(var, at)| other.alias.get(var) == Some(at))
                .map(|(&var, &at)| (var, at))
                .collect(),
            tags: self
                .tags
                .iter()
                .filter_map(|(&k, &bits)| other.tags.get(&k).map(|&o| (k, bits | o)))
                .collect(),
            binds: self
                .binds
                .iter()
                .filter(|(var, view)| other.binds.get(var) == Some(view))
                .map(|(&var, &view)| (var, view))
                .collect(),
            consts: self
                .consts
                .iter()
                .filter_map(|(&var, values)| {
                    let mut union = values.clone();
                    union.extend_from_slice(other.consts.get(&var)?);
                    union.sort_unstable();
                    union.dedup();
                    (union.len() <= 8).then_some((var, union))
                })
                .collect(),
            written: self.written.intersection(&other.written).copied().collect(),
        }
    }

    fn clear_tags(&mut self) {
        self.tags.clear();
        self.binds.clear();
        self.written.clear();
    }

    fn kill_key(&mut self, key: KeyId) {
        self.tags.retain(|&(_, k), _| k != key);
        self.binds.retain(|_, view| view.key != key);
        self.written.retain(|&(_, k)| k != key);
    }

    /// Forgets what held of the instances of the roots an event made
    /// stale; a register variable pointing into one keeps what held of
    /// its instance as its own.
    fn forget_roots(&mut self, stale: impl Fn(Root) -> bool) {
        let mut moved: Vec<(VarId, At)> = self
            .alias
            .iter()
            .filter(|(_, at)| stale(at.0))
            .map(|(&var, &at)| (var, at))
            .collect();
        moved.sort_unstable();
        for (var, (root, offset)) in moved {
            self.alias.remove(&var);
            let own = |at: At| (Root::Reg(var), at.1 - offset);
            let tags: Vec<((At, KeyId), u64)> = self
                .tags
                .iter()
                .filter(|((at, _), _)| at.0 == root)
                .map(|(&(at, key), &bits)| ((own(at), key), bits))
                .collect();
            self.tags.extend(tags);
            let written: Vec<(At, KeyId)> = self
                .written
                .iter()
                .filter(|(at, _)| at.0 == root)
                .map(|&(at, key)| (own(at), key))
                .collect();
            self.written.extend(written);
            for view in self.binds.values_mut() {
                if view.at.0 == root {
                    view.at = own(view.at);
                }
            }
        }
        self.tags.retain(|&(at, _), _| !stale(at.0));
        self.written.retain(|&(at, _)| !stale(at.0));
        self.binds.retain(|_, view| !stale(view.at.0));
    }
}

/// The keys a place's bytes are in: whose byte they hold (the instance
/// and where the bytes start in it), and whose region without the byte.
#[derive(Default)]
struct Access {
    tags: Vec<(KeyId, Option<At>, i64)>,
    regions: Vec<(KeyId, Option<At>)>,
}

/// A register variable's value read from a record's region or byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Loaded {
    at: At,
    key: KeyId,
    /// The writes of the function's keys made before the read.
    generation: u64,
    rel: i64,
    width: u32,
}

struct TagWrite {
    stmt: usize,
    /// The place written.
    place: usize,
    key: KeyId,
    at: Option<At>,
    /// The values written, where constant.
    bits: Option<u64>,
    paired: bool,
    correlated: bool,
    line: u32,
}

struct RegionWrite {
    block: BlockId,
    stmt: usize,
    key: KeyId,
    at: Option<At>,
    /// The values the byte has where it is written.
    facts: Option<u64>,
    place: usize,
}

/// When in its statement an event happens: before the statement's own
/// write finds its place, or after it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum When {
    Before,
    After,
}

/// The constants a variable may hold, where they are few.
type Consts = Option<Vec<i128>>;

/// A function's analysis.
struct Flow<'f> {
    function: &'f Function,
    slots: Box<[u32]>,
    recording: bool,
    notes: Notes,
    generation: u64,
    paths: RefCell<Paths>,
    tag_writes: Vec<TagWrite>,
    region_writes: Vec<RegionWrite>,
    tag_at: FxHashMap<(BlockId, usize), Vec<usize>>,
    /// What each statement changes, in order.
    events: FxHashMap<(BlockId, usize), Vec<(When, Event)>>,
    region_loaded: FxHashMap<VarId, Loaded>,
    tag_loaded: FxHashMap<VarId, Loaded>,
    pending_regions: Vec<(usize, VarId, Loaded)>,
    pending_tags: Vec<(usize, Loaded)>,
    block_tags: usize,
    block_regions: usize,
}

impl Flow<'_> {
    fn is_register(&self, var: VarId) -> bool {
        self.slots
            .get(var as usize)
            .is_some_and(|&slot| slot & super::MEMORY == 0)
    }

    /// Whether an event of a block from `from` to `to`, both included,
    /// may change what a root reaches.
    fn changed(
        &self,
        nested: &Nested,
        root: Root,
        block: BlockId,
        from: (usize, When),
        to: (usize, When),
    ) -> bool {
        let paths = self.paths.borrow();
        (from.0..=to.0).any(|stmt| {
            self.events
                .get(&(block, stmt))
                .into_iter()
                .flatten()
                .any(|(when, event)| {
                    (stmt, *when) >= from
                        && (stmt, *when) <= to
                        && paths.affects(nested, event, root)
                })
        })
    }
}

/// A byte of a value written: the byte the place already holds, or the
/// values it may have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Byte {
    Kept,
    Values(ByteSet),
}

/// What a write writes: the constants it may be, the register variable
/// it is, and its expression.
struct Written<'e> {
    constants: Option<Vec<i128>>,
    var: Option<VarId>,
    src: Option<&'e Expr>,
}

/// The places a statement reads, with their types.
#[derive(Default)]
struct Reads<'f>(Vec<(&'f Place, &'f Arc<str>)>);

impl<'f> Visit<'f> for Reads<'f> {
    fn expr(&mut self, expr: &'f Expr) {
        if let ExprKind::Use(place) = &expr.kind {
            self.0.push((place, &expr.ty));
        }
    }
}

/// The records a function's places reach members of.
#[derive(Default)]
struct Records<'f>(Vec<&'f Arc<str>>);

impl<'f> Visit<'f> for Records<'f> {
    fn place(&mut self, place: &'f Place) {
        if let Place::Field(_, field) = place {
            self.0.push(&field.record);
        }
    }
}

/// The places within a written place's that its write reads.
fn place_reads<'f>(reads: &mut Reads<'f>, place: &'f Place) {
    match place {
        Place::Deref(pointer) => walk_expr(reads, pointer),
        Place::Field(inner, _) => place_reads(reads, inner),
        Place::Index(inner, bytes) => {
            place_reads(reads, inner);
            walk_expr(reads, bytes);
        }
        Place::Var(_) | Place::Global(_) => {}
    }
}

fn flip(op: BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Le => BinOp::Ge,
        BinOp::Gt => BinOp::Lt,
        BinOp::Ge => BinOp::Le,
        other => other,
    }
}

fn compare(op: BinOp, left: i128, right: i128) -> bool {
    match op {
        BinOp::Eq => left == right,
        BinOp::Ne => left != right,
        BinOp::Lt => left < right,
        BinOp::Le => left <= right,
        BinOp::Gt => left > right,
        BinOp::Ge => left >= right,
        _ => true,
    }
}

/// The comparison that holds where another does not.
fn negate(op: BinOp) -> BinOp {
    match op {
        BinOp::Eq => BinOp::Ne,
        BinOp::Ne => BinOp::Eq,
        BinOp::Lt => BinOp::Ge,
        BinOp::Le => BinOp::Gt,
        BinOp::Gt => BinOp::Le,
        BinOp::Ge => BinOp::Lt,
        other => other,
    }
}

/// The values an integer type holds, from the least to the greatest.
fn bounds(conversion: Option<Conversion>) -> (i128, i128) {
    match conversion {
        Some((bits, true)) if bits < 128 => (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1),
        Some((bits, false)) if bits < 128 => (0, (1i128 << bits) - 1),
        Some((_, false)) => (0, i128::MAX),
        _ => (i128::MIN, i128::MAX),
    }
}

/// Whether a conversion between two integer types keeps every value.
fn preserves(from: Option<Conversion>, to: Option<Conversion>) -> bool {
    match (from, to) {
        (Some((from_bits, from_signed)), Some((to_bits, to_signed))) => {
            (from_signed == to_signed && to_bits >= from_bits)
                || (!from_signed && to_signed && to_bits > from_bits)
        }
        (None, None) => true,
        _ => false,
    }
}

/// The values from `low` to `high` within a type's, where they are few.
fn interval(low: i128, high: i128, range: (i128, i128)) -> Option<Vec<i128>> {
    let (low, high) = (low.max(range.0), high.min(range.1));
    if high < low {
        return Some(Vec::new());
    }
    high.checked_sub(low)
        .is_some_and(|span| span < FEW as i128)
        .then(|| (low..=high).collect())
}

/// The values of a type that a comparison with a constant holds of,
/// where they are few.
fn compared_values(op: BinOp, value: i128, range: (i128, i128)) -> Option<Vec<i128>> {
    match op {
        BinOp::Eq => interval(value, value, range),
        BinOp::Ne => {
            let mut all = interval(range.0, range.1, range)?;
            all.retain(|&v| v != value);
            Some(all)
        }
        BinOp::Lt => interval(range.0, value.checked_sub(1)?, range),
        BinOp::Le => interval(range.0, value, range),
        BinOp::Gt => interval(value.checked_add(1)?, range.1, range),
        BinOp::Ge => interval(value, range.1, range),
        _ => None,
    }
}

/// A register variable a value is, through conversions: the
/// conversions in the order made, whether they keep every value, and the
/// variable's type.
struct Chain {
    var: VarId,
    conversions: Vec<Conversion>,
    preserving: bool,
    own: Option<Conversion>,
}

impl Chain {
    /// A value of the variable as the conversions make it.
    fn convert(&self, value: i128) -> i128 {
        self.conversions
            .iter()
            .fold(value, |value, &(bits, signed)| {
                convert_to(value, bits, signed)
            })
    }
}

/// What the condition taking an edge shows: the values a byte may have,
/// and the constants a register variable holds.
#[derive(Default)]
struct Shown {
    byte: Option<(View, u64)>,
    var: Option<(VarId, Vec<i128>)>,
}

/// The blocks reached from the entry, each before its successors but
/// along back edges.
fn reverse_post_order(function: &Function) -> Vec<BlockId> {
    let count = function.blocks.len();
    if count == 0 {
        return Vec::new();
    }
    let mut seen = vec![false; count];
    let mut order = Vec::with_capacity(count);
    let mut stack: Vec<(BlockId, Vec<BlockId>, usize)> = Vec::new();
    seen[0] = true;
    stack.push((0, successors(&function.blocks[0].term), 0));
    while let Some((block, next, index)) = stack.last_mut() {
        if let Some(&target) = next.get(*index) {
            *index += 1;
            if (target as usize) < count && !seen[target as usize] {
                seen[target as usize] = true;
                let targets = successors(&function.blocks[target as usize].term);
                stack.push((target, targets, 0));
            }
        } else {
            order.push(*block);
            stack.pop();
        }
    }
    order.reverse();
    order
}

impl Pta<'_> {
    /// The keys: each byte the conditions compare, of a record with a
    /// region, with the values written into it.
    pub(crate) fn tag_model(&mut self) {
        let mut pairs: BTreeSet<(Arc<str>, i64)> = BTreeSet::new();
        for (record, tag) in &self.survey.compared {
            pairs.insert((record.clone(), *tag));
            for (union, at) in self.survey.variants.get(record).into_iter().flatten() {
                pairs.insert((union.clone(), tag + at));
            }
        }
        let mut members: FxHashMap<Arc<str>, Vec<(Arc<str>, i64)>> = FxHashMap::default();
        for (member, unions) in &self.survey.variants {
            for (union, at) in unions {
                members
                    .entry(union.clone())
                    .or_default()
                    .push((member.clone(), *at));
            }
        }
        for (record, tag) in pairs {
            let Some(layout) = self.types.layout(&record) else {
                continue;
            };
            let size = layout.size as i64;
            if tag < 0 || tag >= size {
                continue;
            }
            let region = if layout.is_union {
                (0, size)
            } else {
                let mut region: Option<(i64, i64)> = None;
                for field in &layout.fields {
                    let Some(held) = &field.holds else {
                        continue;
                    };
                    let held = self.types.canonical(held);
                    if !self.types.layout(&held).is_some_and(|l| l.is_union)
                        || !self.types.type_holds_pointer(&field.ty)
                    {
                        continue;
                    }
                    let start = field.offset as i64;
                    let end = start + field.size.unwrap_or(0) as i64;
                    if start <= tag && tag < end {
                        continue;
                    }
                    region = Some(match region {
                        None => (start, end),
                        Some((low, high)) => (low.min(start), high.max(end)),
                    });
                }
                match region {
                    Some(region) => region,
                    None => continue,
                }
            };
            let mut bytes = [0u64; 4];
            let mut sources = vec![(record.clone(), tag)];
            for (member, at) in members.get(&record).into_iter().flatten() {
                sources.push((member.clone(), tag - at));
            }
            for source in &sources {
                if let Some(bits) = self.survey.written.get(source) {
                    for (word, bits) in bytes.iter_mut().zip(bits) {
                        *word |= bits;
                    }
                }
            }
            let mut domain = vec![0u8];
            for byte in 1..=255u8 {
                if bytes[usize::from(byte) >> 6] & (1 << (byte & 63)) != 0 {
                    domain.push(byte);
                }
            }
            domain.truncate(DOMAIN);
            let id = self.tags.keys.len() as KeyId;
            self.tags.keys.push(Key {
                record: record.clone(),
                tag,
                region,
                size,
                domain,
                unsplit_by: Vec::new(),
                unclean_values: 0,
                unclean_by: Vec::new(),
                counts: KeyCounts::default(),
            });
            self.tags.by_record.entry(record).or_default().push(id);
        }
        let mut keyed: FxHashSet<Arc<str>> = self.tags.by_record.keys().cloned().collect();
        for (member, unions) in &self.survey.variants {
            if unions
                .iter()
                .any(|(union, _)| self.tags.by_record.contains_key(union))
            {
                keyed.insert(member.clone());
            }
        }
        self.tags.keyed = keyed;
    }

    /// The keys whose byte each function may write: through the members
    /// of the keys' records, by a value of a record holding one, or by its
    /// calls; every key where it runs assembly, calls through a pointer,
    /// or calls a function whose code is not read and whose effects are
    /// not known to write no memory.
    pub(crate) fn find_key_writers(&mut self) {
        let words = self.tags.keys.len().div_ceil(64);
        let count = self.fn_names.len();
        let mut writers: Vec<KeyBits> = vec![Some(vec![0; words].into()); count];
        let mut callers: Vec<Vec<super::FnId>> = vec![Vec::new(); count];
        for f in 0..count {
            let defs = self.fn_defs[f].clone();
            if defs.is_empty() {
                if !self.pure_external(f as super::FnId) {
                    writers[f] = None;
                }
                continue;
            }
            for def in defs {
                let facts = &self.survey.facts[def as usize];
                if facts.indirect || self.defs[def as usize].unread {
                    writers[f] = None;
                }
                for &callee in facts.callees.iter() {
                    callers[callee as usize].push(f as super::FnId);
                }
                if writers[f].is_none() {
                    continue;
                }
                let spans = std::mem::take(&mut self.survey.spans[def as usize]);
                for span in spans.iter() {
                    let keys = self.span_keys(span);
                    add_bits(&mut writers[f], &keys);
                }
                self.survey.spans[def as usize] = spans;
            }
        }
        let mut work: Vec<super::FnId> = (0..count as super::FnId).collect();
        while let Some(callee) = work.pop() {
            let more = writers[callee as usize].clone();
            for &caller in &callers[callee as usize] {
                if add_bits(&mut writers[caller as usize], &more) {
                    work.push(caller);
                }
            }
        }
        self.stats.key_writers = writers
            .iter()
            .filter(|keys| {
                keys.as_ref()
                    .is_some_and(|keys| keys.iter().any(|&w| w != 0))
            })
            .count();
        self.stats.any_key_writers = writers.iter().filter(|keys| keys.is_none()).count();
        self.tags.writers = writers;
    }

    /// The keys whose byte a write's bytes may hold.
    fn span_keys(&mut self, span: &Span) -> KeyBits {
        match span {
            Span::Any => None,
            Span::Whole(ty) => self.keys_within(ty),
            Span::Members(chain, start, width) => {
                let access = self.classify(None, chain, *start, *width);
                let mut bits = vec![0u64; self.tags.keys.len().div_ceil(64)];
                for &(key, ..) in &access.tags {
                    bits[key as usize / 64] |= 1 << (key % 64);
                }
                Some(bits.into())
            }
        }
    }

    /// The keys whose byte a value of a type may hold: those of a record
    /// and of the unions it may be in, and of its members' records; of an
    /// array's elements; every key for a type not known.
    fn keys_within(&mut self, ty: &Arc<str>) -> KeyBits {
        match self.types.shape(ty) {
            Shape::Record(record) => Some(self.record_keys(&record)),
            Shape::Array => match self.types.element(ty) {
                Some(element) => self.keys_within(&element),
                None => None,
            },
            Shape::Unknown => None,
            _ => Some(vec![0u64; self.tags.keys.len().div_ceil(64)].into()),
        }
    }

    fn record_keys(&mut self, record: &Arc<str>) -> Box<[u64]> {
        let record = self.types.canonical(record);
        if let Some(bits) = self.tags.within.get(&record) {
            return bits.clone();
        }
        let words = self.tags.keys.len().div_ceil(64);
        // A record holding itself (through an array) adds no key by that
        // member alone.
        self.tags
            .within
            .insert(record.clone(), vec![0u64; words].into());
        let mut bits = vec![0u64; words];
        let mut keys: Vec<KeyId> = self
            .tags
            .by_record
            .get(&record)
            .cloned()
            .unwrap_or_default();
        for (union, _) in self.survey.variants.get(&record).into_iter().flatten() {
            keys.extend(self.tags.by_record.get(union).into_iter().flatten());
        }
        for key in keys {
            bits[key as usize / 64] |= 1 << (key % 64);
        }
        let members: Vec<Arc<str>> = self
            .types
            .layout(&record)
            .map(|layout| {
                layout
                    .fields
                    .iter()
                    .filter_map(|field| field.holds.clone())
                    .collect()
            })
            .unwrap_or_default();
        for member in members {
            for (word, more) in bits.iter_mut().zip(self.record_keys(&member).iter()) {
                *word |= more;
            }
        }
        let bits: Box<[u64]> = bits.into();
        self.tags.within.insert(record, bits.clone());
        bits
    }

    /// Forgets what held of the keys' bytes a write of a value of a type
    /// may write.
    fn forget_within(&mut self, state: &mut State, ty: &Arc<str>) {
        match self.keys_within(ty) {
            None => state.clear_tags(),
            Some(bits) => {
                for key in bit_keys(&bits) {
                    state.kill_key(key);
                }
            }
        }
    }

    /// Whether a function reaches a member of a record a key may be in.
    fn touches_keys(&self, function: &Function) -> bool {
        let mut records = Records::default();
        walk_function(&mut records, function);
        if records
            .0
            .iter()
            .any(|record| self.tags.keyed.contains(&self.types.canonical(record)))
        {
            return true;
        }
        function
            .blocks
            .iter()
            .flat_map(|b| &b.stmts)
            .any(|stmt| match stmt {
                Stmt::Call {
                    callee: Callee::Direct(name),
                    ..
                } => self
                    .fn_ids
                    .get(name)
                    .and_then(|f| self.survey.accessors.get(f))
                    .is_some_and(|accessor| {
                        accessor
                            .chain
                            .iter()
                            .any(|(record, _)| self.tags.keyed.contains(record))
                    }),
                _ => false,
            })
    }

    /// The parts of a definition's writes of regions and the contexts of
    /// its reads, and the pairing of its writes of bytes.
    pub(crate) fn tag_notes(&mut self, def: DefId, function: &Function) -> Notes {
        if self.tags.keys.is_empty() || !self.touches_keys(function) {
            return Notes::default();
        }
        let mut flow = Flow {
            function,
            slots: self.defs[def as usize].slots.clone(),
            recording: false,
            notes: Notes::default(),
            generation: 0,
            paths: RefCell::new(Paths::default()),
            tag_writes: Vec::new(),
            region_writes: Vec::new(),
            tag_at: FxHashMap::default(),
            events: FxHashMap::default(),
            region_loaded: FxHashMap::default(),
            tag_loaded: FxHashMap::default(),
            pending_regions: Vec::new(),
            pending_tags: Vec::new(),
            block_tags: 0,
            block_regions: 0,
        };
        let order = reverse_post_order(function);
        let mut entry: Vec<Option<State>> = vec![None; function.blocks.len()];
        if entry.is_empty() {
            return Notes::default();
        }
        entry[0] = Some(State::default());
        loop {
            let mut changed = false;
            for &block in &order {
                let Some(mut state) = entry[block as usize].clone() else {
                    continue;
                };
                self.run_block(&mut flow, &mut state, block);
                for (target, shown) in
                    self.edges(&mut flow, &state, &function.blocks[block as usize].term)
                {
                    let mut out = state.clone();
                    if let Some((view, bits)) = shown.byte {
                        let full = self.tags.keys[view.key as usize].full();
                        let now =
                            out.tags.get(&(view.at, view.key)).copied().unwrap_or(full) & bits;
                        if now == 0 {
                            continue;
                        }
                        if now != full {
                            out.tags.insert((view.at, view.key), now);
                        }
                    }
                    if let Some((var, values)) = shown.var {
                        if values.is_empty() {
                            continue;
                        }
                        out.consts.insert(var, values);
                    }
                    let Some(slot) = entry.get_mut(target as usize) else {
                        continue;
                    };
                    let merged = match slot {
                        None => out,
                        Some(old) => old.meet(&out),
                    };
                    if slot.as_ref() != Some(&merged) {
                        *slot = Some(merged);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        flow.recording = true;
        for &block in &order {
            let Some(mut state) = entry[block as usize].clone() else {
                continue;
            };
            flow.region_loaded.clear();
            flow.tag_loaded.clear();
            flow.pending_regions.clear();
            flow.pending_tags.clear();
            flow.block_tags = flow.tag_writes.len();
            flow.block_regions = flow.region_writes.len();
            self.run_block(&mut flow, &mut state, block);
            let mut reads = Reads::default();
            match &function.blocks[block as usize].term {
                Term::Branch { cond, .. } => walk_expr(&mut reads, cond),
                Term::Switch { on, .. } => walk_expr(&mut reads, on),
                Term::Return(Some(value)) => walk_expr(&mut reads, value),
                _ => {}
            }
            self.note_reads(&mut flow, &state, &reads);
            self.end_block(&mut flow, block);
        }
        self.finish_notes(&mut flow);
        flow.notes
    }

    fn run_block(&mut self, flow: &mut Flow<'_>, state: &mut State, block: BlockId) {
        let function = flow.function;
        for (i, stmt) in function.blocks[block as usize].stmts.iter().enumerate() {
            self.step(flow, state, (block, i), stmt);
        }
    }

    fn step<'f>(
        &mut self,
        flow: &mut Flow<'f>,
        state: &mut State,
        at: (BlockId, usize),
        stmt: &'f Stmt,
    ) {
        if flow.recording {
            let mut reads = Reads::default();
            match stmt {
                Stmt::Assign { dst, src, .. } => {
                    place_reads(&mut reads, dst);
                    walk_expr(&mut reads, src);
                }
                Stmt::Call {
                    dst, callee, args, ..
                } => {
                    if let Some(dst) = dst {
                        place_reads(&mut reads, dst);
                    }
                    if let Callee::Indirect(callee) = callee {
                        walk_expr(&mut reads, callee);
                    }
                    for arg in args {
                        walk_expr(&mut reads, arg);
                    }
                }
                Stmt::Asm {
                    writes,
                    reads: read,
                    ..
                } => {
                    for write in writes {
                        place_reads(&mut reads, write);
                    }
                    for read in read {
                        walk_expr(&mut reads, read);
                    }
                }
            }
            self.note_reads(flow, state, &reads);
        }
        match stmt {
            Stmt::Assign { dst, src, line } => {
                if let Place::Var(var) = dst
                    && flow.is_register(*var)
                {
                    let alias = self.pointer_base(flow, state, src);
                    let bind = self.view_of(flow, state, src);
                    let consts = self.fold_values(flow, state, src);
                    let loaded = if flow.recording {
                        self.loaded(flow, state, src)
                    } else {
                        (None, None)
                    };
                    let event = Event::Reassign(*var);
                    let kept = |pta: &Pta<'_>, root: Root| {
                        !flow.paths.borrow().affects(&pta.tags.nested, &event, root)
                    };
                    let alias = alias.filter(|alias| kept(self, alias.0));
                    let bind = bind.filter(|view| kept(self, view.at.0));
                    let loaded = (
                        loaded.0.filter(|loaded| kept(self, loaded.at.0)),
                        loaded.1.filter(|loaded| kept(self, loaded.at.0)),
                    );
                    self.happen(flow, state, at, When::After, event);
                    if let Some(alias) = alias {
                        state.alias.insert(*var, alias);
                    }
                    if let Some(view) = bind {
                        state.binds.insert(*var, view);
                    }
                    if let Some(consts) = consts {
                        state.consts.insert(*var, consts);
                    }
                    if flow.recording {
                        if let Some(loaded) = loaded.0 {
                            flow.region_loaded.insert(*var, loaded);
                        }
                        if let Some(loaded) = loaded.1 {
                            flow.tag_loaded.insert(*var, loaded);
                        }
                    }
                    return;
                }
                if matches!(self.types.shape(&src.ty), Shape::Record(_) | Shape::Array) {
                    self.forget_within(state, &src.ty);
                    flow.generation += 1;
                    let bytes = self.place_bytes(flow, dst, Some(&src.ty));
                    self.happen(flow, state, at, When::After, Event::Write(bytes));
                    return;
                }
                let var = match &strip_casts(src).kind {
                    ExprKind::Use(Place::Var(var)) if flow.is_register(*var) => Some(*var),
                    _ => None,
                };
                let width = self.width_of(&src.ty);
                self.write(
                    flow,
                    state,
                    (at.0, at.1, *line),
                    dst,
                    width,
                    &Written {
                        constants: None,
                        var,
                        src: Some(src),
                    },
                );
                let bytes = self.place_bytes(flow, dst, Some(&src.ty));
                self.happen(flow, state, at, When::After, Event::Write(bytes));
            }
            Stmt::Call {
                dst,
                callee,
                args,
                line,
            } => {
                let pure = match callee {
                    Callee::Direct(name) => {
                        self.fn_ids.get(name).is_some_and(|f| self.pure.contains(f))
                    }
                    Callee::Indirect(_) => false,
                };
                if !pure {
                    let writes = match callee {
                        Callee::Direct(name) => self.fn_ids.get(name).copied(),
                        Callee::Indirect(_) => None,
                    }
                    .and_then(|f| self.tags.writers.get(f as usize));
                    match writes {
                        Some(Some(bits)) => {
                            for key in bit_keys(bits) {
                                state.kill_key(key);
                            }
                        }
                        _ => state.clear_tags(),
                    }
                    flow.generation += 1;
                    self.happen(flow, state, at, When::Before, Event::Any);
                }
                let Some(dst) = dst else {
                    return;
                };
                if let Place::Var(var) = dst
                    && flow.is_register(*var)
                {
                    let bind = match (callee, args.first()) {
                        (Callee::Direct(name), Some(arg)) => {
                            self.accessor_view(flow, state, name, arg)
                        }
                        _ => None,
                    };
                    let consts = self.call_returns(flow, callee, dst);
                    let event = Event::Reassign(*var);
                    let bind = bind.filter(|view| {
                        !flow
                            .paths
                            .borrow()
                            .affects(&self.tags.nested, &event, view.at.0)
                    });
                    self.happen(flow, state, at, When::After, event);
                    if let Some(view) = bind {
                        state.binds.insert(*var, view);
                    }
                    if let Some(consts) = consts {
                        state.consts.insert(*var, consts);
                    }
                    return;
                }
                let ty = self.place_ty(Some(flow.function), dst);
                let shape = ty.as_ref().map(|ty| self.types.shape(ty));
                let bytes = self.place_bytes(flow, dst, ty.as_ref());
                if let Some(ty) = &ty
                    && matches!(shape, Some(Shape::Record(_) | Shape::Array))
                {
                    self.forget_within(state, ty);
                    flow.generation += 1;
                    self.happen(flow, state, at, When::After, Event::Write(bytes));
                    return;
                }
                let width = ty.map_or(8, |ty| self.width_of(&ty));
                let written = Written {
                    constants: self.call_returns(flow, callee, dst),
                    var: None,
                    src: None,
                };
                self.write(flow, state, (at.0, at.1, *line), dst, width, &written);
                self.happen(flow, state, at, When::After, Event::Write(bytes));
            }
            Stmt::Asm { writes, .. } => {
                state.clear_tags();
                flow.generation += 1;
                self.happen(flow, state, at, When::Before, Event::Any);
                for write in writes {
                    if let Place::Var(var) = write
                        && flow.is_register(*var)
                    {
                        self.happen(flow, state, at, When::After, Event::Reassign(*var));
                    }
                }
            }
        }
    }

    /// What a statement changes: what held of the roots it may change no
    /// longer holds, a register variable pointing into one keeping what
    /// held of its instance as its own.
    fn happen(
        &mut self,
        flow: &mut Flow<'_>,
        state: &mut State,
        at: (BlockId, usize),
        when: When,
        event: Event,
    ) {
        if let Event::Reassign(var) = event {
            state.alias.remove(&var);
            state.binds.remove(&var);
            state.consts.remove(&var);
        }
        let nested = &self.tags.nested;
        let paths = flow.paths.borrow();
        let stale = |root: Root| paths.affects(nested, &event, root);
        state.forget_roots(stale);
        if flow.recording {
            if let Event::Reassign(var) = event {
                flow.region_loaded.remove(&var);
                flow.tag_loaded.remove(&var);
            }
            flow.region_loaded.retain(|_, loaded| !stale(loaded.at.0));
            flow.tag_loaded.retain(|_, loaded| !stale(loaded.at.0));
            drop(paths);
            flow.events.entry(at).or_default().push((when, event));
        }
    }

    /// The region and the byte a value reads, for the copies of a region
    /// with its byte.
    fn loaded(
        &mut self,
        flow: &Flow<'_>,
        state: &State,
        src: &Expr,
    ) -> (Option<Loaded>, Option<Loaded>) {
        let value = strip_casts(src);
        let ExprKind::Use(place) = &value.kind else {
            return (None, None);
        };
        if let Place::Var(var) = place
            && flow.is_register(*var)
        {
            return (None, None);
        }
        let width = self.width_of(&value.ty);
        let Some((access, start)) = self.place_access(flow, state, place, width) else {
            return (None, None);
        };
        let region = access.regions.first().and_then(|&(key, at)| {
            Some(Loaded {
                at: at?,
                key,
                generation: flow.generation,
                rel: start - at?.1,
                width,
            })
        });
        let tag = access.tags.first().and_then(|&(key, at, rel)| {
            Some(Loaded {
                at: at?,
                key,
                generation: flow.generation,
                rel,
                width,
            })
        });
        (region, tag)
    }

    /// The constants a value may be, where they are few: folded through
    /// the operations on constants and the register variables holding
    /// them, in the value's type.
    fn fold_values(&mut self, flow: &Flow<'_>, state: &State, expr: &Expr) -> Option<Vec<i128>> {
        let conversion = self.conversion(&expr.ty)?;
        let values = match &expr.kind {
            ExprKind::Int(value) => vec![*value],
            ExprKind::Cast(inner) => self.fold_values(flow, state, inner)?,
            ExprKind::Use(Place::Var(var)) if flow.is_register(*var) => {
                state.consts.get(var)?.clone()
            }
            ExprKind::Unary(op, inner) => fold_unary(*op, &self.fold_values(flow, state, inner)?),
            ExprKind::Binary(op, left, right) => {
                let left = self.fold_values(flow, state, left)?;
                let right = self.fold_values(flow, state, right)?;
                fold_binary(*op, &left, &right)?
            }
            _ => return None,
        };
        match conversion {
            Some((bits, signed)) => few(values
                .into_iter()
                .map(|value| convert_to(value, bits, signed))
                .collect()),
            None => Some(values),
        }
    }

    /// The constants a call's result written to a place may be: what its
    /// callee returns, where that is a few constants, in the place's type.
    fn call_returns(&mut self, flow: &Flow<'_>, callee: &Callee, dst: &Place) -> Option<Vec<i128>> {
        let Callee::Direct(name) = callee else {
            return None;
        };
        let f: FnId = *self.fn_ids.get(name)?;
        let values = self.survey.returns.get(&f)?.clone();
        let ty = self.place_ty(Some(flow.function), dst)?;
        match self.conversion(&ty)? {
            Some((bits, signed)) => few(values
                .into_iter()
                .map(|value| convert_to(value, bits, signed))
                .collect()),
            None => Some(values),
        }
    }

    /// The values a key's byte of an instance may have where a value read
    /// it.
    fn view_bytes(&self, state: &State, view: View) -> ByteSet {
        let key = &self.tags.keys[view.key as usize];
        let bits = state
            .tags
            .get(&(view.at, view.key))
            .copied()
            .unwrap_or_else(|| key.full());
        let mut set = if bits & OTHER == 0 {
            [0; 4]
        } else {
            let named = byte_set(key.domain.iter().copied());
            [!named[0], !named[1], !named[2], !named[3]]
        };
        for (i, &byte) in key.domain.iter().enumerate() {
            if bits & (1 << i) != 0 {
                set[usize::from(byte >> 6)] |= 1 << (byte & 63);
            }
        }
        set
    }

    /// The byte `byte` of a value written to `dst`, the byte `own` of an
    /// instance: the byte it holds, where the value leaves it or is that
    /// byte read, or the values it may have: those of the constants the
    /// value may be, of a byte read where it is that byte, or of the
    /// operations on bytes it is made of.
    fn written_byte(
        &mut self,
        flow: &Flow<'_>,
        state: &State,
        expr: &Expr,
        (dst, own): (&Place, Option<(At, KeyId)>),
        byte: i64,
    ) -> Byte {
        if let ExprKind::Use(place) = &expr.kind
            && place == dst
        {
            return Byte::Kept;
        }
        if let Some(values) = self.fold_values(flow, state, expr) {
            return Byte::Values(byte_set(values.iter().map(|&value| byte_of(value, byte))));
        }
        if byte == 0
            && let Some(view) = self.view_of(flow, state, expr)
        {
            if own == Some((view.at, view.key)) {
                return Byte::Kept;
            }
            return Byte::Values(self.view_bytes(state, view));
        }
        match &expr.kind {
            ExprKind::Cast(inner) => {
                let width = i64::from(self.width_of(&expr.ty));
                let inner_width = i64::from(self.width_of(&inner.ty));
                if byte < width.min(inner_width) {
                    self.written_byte(flow, state, inner, (dst, own), byte)
                } else if byte < width
                    && self
                        .types
                        .integer(&inner.ty)
                        .is_some_and(|(_, signed)| !signed)
                {
                    Byte::Values(byte_set([0]))
                } else {
                    Byte::Values(ALL_BYTES)
                }
            }
            ExprKind::Unary(UnOp::BitNot, inner) => {
                match self.written_byte(flow, state, inner, (dst, own), byte) {
                    Byte::Values(set) => Byte::Values(byte_set(set_bytes(set).map(|b| !b))),
                    Byte::Kept => Byte::Values(ALL_BYTES),
                }
            }
            ExprKind::Binary(op @ (BinOp::BitOr | BinOp::BitAnd | BinOp::BitXor), left, right) => {
                let left = self.written_byte(flow, state, left, (dst, own), byte);
                let right = self.written_byte(flow, state, right, (dst, own), byte);
                let (identity, absorbing) = match op {
                    BinOp::BitAnd => (byte_set([0xff]), Some(byte_set([0]))),
                    BinOp::BitOr => (byte_set([0]), Some(byte_set([0xff]))),
                    _ => (byte_set([0]), None),
                };
                match (left, right) {
                    (Byte::Values(left), Byte::Values(right)) => {
                        Byte::Values(combine(*op, left, right))
                    }
                    (Byte::Kept, Byte::Kept) if *op == BinOp::BitXor => Byte::Values(byte_set([0])),
                    (Byte::Kept, Byte::Kept) => Byte::Kept,
                    (Byte::Kept, Byte::Values(other)) | (Byte::Values(other), Byte::Kept) => {
                        if other == identity {
                            Byte::Kept
                        } else if Some(other) == absorbing {
                            Byte::Values(other)
                        } else {
                            Byte::Values(ALL_BYTES)
                        }
                    }
                }
            }
            _ => Byte::Values(ALL_BYTES),
        }
    }

    fn write(
        &mut self,
        flow: &mut Flow<'_>,
        state: &mut State,
        (block, stmt, line): (BlockId, usize, u32),
        dst: &Place,
        width: u32,
        written: &Written,
    ) {
        let Some(located) = locate(dst) else {
            return;
        };
        if located.chain.is_empty() {
            // A write through a pointer to no member is taken to write no
            // key's byte; those that may are listed once solved.
            if matches!(dst, Place::Deref(_)) {
                flow.generation += 1;
            }
            return;
        }
        let resolved = self.resolve(flow, state, located.base);
        let access = self.classify(resolved, &located.chain, located.start, width);
        if access.tags.is_empty() && access.regions.is_empty() {
            return;
        }
        let place = place_key(dst);
        for &(key, at, rel) in &access.tags {
            let byte = self.tags.keys[key as usize].tag - rel;
            let given = match (written.src, &written.constants) {
                (Some(src), _) => {
                    let own = at.map(|at| (at, key));
                    self.written_byte(flow, state, src, (dst, own), byte)
                }
                (None, Some(values)) => {
                    Byte::Values(byte_set(values.iter().map(|&value| byte_of(value, byte))))
                }
                (None, None) => Byte::Values(ALL_BYTES),
            };
            let bits = match given {
                // The byte is written with what it holds: the tag stays.
                Byte::Kept => continue,
                Byte::Values(set) => {
                    let key = &self.tags.keys[key as usize];
                    let bits = set_bytes(set).fold(0, |bits, value| bits | key.bit(value));
                    (bits != key.full()).then_some(bits)
                }
            };
            let paired = at.is_some_and(|at| state.written.contains(&(at, key)));
            state.kill_key(key);
            if let (Some(at), Some(bits)) = (at, bits) {
                state.tags.insert((at, key), bits);
            }
            if flow.recording {
                flow.notes
                    .tags
                    .entry(place)
                    .or_default()
                    .push((key, byte as u32));
                let id = flow.tag_writes.len();
                flow.tag_writes.push(TagWrite {
                    stmt,
                    place,
                    key,
                    at,
                    bits,
                    paired,
                    correlated: false,
                    line,
                });
                flow.tag_at.entry((block, stmt)).or_default().push(id);
                if let Some(var) = written.var
                    && let Some(&loaded) = flow.tag_loaded.get(&var)
                    && loaded.key == key
                    && loaded.rel == rel
                {
                    flow.pending_tags.push((id, loaded));
                }
            }
        }
        for &(key, at) in &access.regions {
            if flow.recording {
                let id = flow.region_writes.len();
                flow.region_writes.push(RegionWrite {
                    block,
                    stmt,
                    key,
                    at,
                    facts: at.and_then(|at| state.tags.get(&(at, key)).copied()),
                    place,
                });
                if let Some(var) = written.var
                    && let Some(&loaded) = flow.region_loaded.get(&var)
                    && loaded.key == key
                    && loaded.width == width
                {
                    flow.pending_regions.push((id, var, loaded));
                }
            }
            state.written.retain(|&(_, k)| k != key);
            if let Some(at) = at {
                state.written.insert((at, key));
            }
        }
        flow.generation += 1;
    }

    /// Records the contexts of a statement's reads of regions.
    fn note_reads(&mut self, flow: &mut Flow<'_>, state: &State, reads: &Reads<'_>) {
        for &(place, ty) in &reads.0 {
            if let Place::Var(var) = place
                && flow.is_register(*var)
            {
                continue;
            }
            if !self.types.shape(ty).is_scalar() {
                continue;
            }
            let width = self.width_of(ty);
            let Some((access, _)) = self.place_access(flow, state, place, width) else {
                continue;
            };
            let found = access.regions.iter().find_map(|&(key, at)| {
                let bits = *state.tags.get(&(at?, key))?;
                (bits != self.tags.keys[key as usize].full()).then_some((key, bits))
            });
            if let Some((key, bits)) = found {
                let set = self.tag_set(key, bits);
                flow.notes.ctxs.insert(place_key(place), set);
                self.tags.keys[key as usize].counts.contexts += 1;
            }
        }
    }

    /// The copies of a region with its byte in the block just read, and
    /// the writes of a byte its region's write follows in the block.
    fn end_block(&mut self, flow: &mut Flow<'_>, block: BlockId) {
        let nested = &self.tags.nested;
        // Whether the root of a write at a statement holds the instance of
        // another's at another.
        let same = |flow: &Flow<'_>, root: Root, a: usize, b: usize| {
            let (first, last) = (a.min(b), a.max(b));
            !flow.changed(
                nested,
                root,
                block,
                (first, When::After),
                (last, When::Before),
            )
        };
        let pending_regions = std::mem::take(&mut flow.pending_regions);
        let pending_tags = std::mem::take(&mut flow.pending_tags);
        for (region, var, source) in pending_regions {
            let write = &flow.region_writes[region];
            let Some(at) = write.at else {
                continue;
            };
            let found = pending_tags.iter().find(|(tag, tag_source)| {
                let tag = &flow.tag_writes[*tag];
                tag.key == write.key
                    && tag.at == Some(at)
                    && same(flow, at.0, tag.stmt, write.stmt)
                    && tag_source.at == source.at
                    && tag_source.generation == source.generation
            });
            if let Some(&(tag, _)) = found {
                flow.notes.copies.insert(write.place, var);
                flow.tag_writes[tag].correlated = true;
            }
        }
        for tag in flow.block_tags..flow.tag_writes.len() {
            let write = &flow.tag_writes[tag];
            let Some(at) = write.at else {
                continue;
            };
            let follows = flow.region_writes[flow.block_regions..]
                .iter()
                .any(|region| {
                    region.stmt > write.stmt
                        && region.key == write.key
                        && region.at == Some(at)
                        && same(flow, at.0, write.stmt, region.stmt)
                });
            if follows {
                flow.tag_writes[tag].paired = true;
            }
        }
    }

    /// The parts of the function's writes of regions, and the values its
    /// writes of bytes that are not paired leave in every part.
    fn finish_notes(&mut self, flow: &mut Flow<'_>) {
        let mut decided: FxHashMap<usize, (i64, KeyId, u64)> = FxHashMap::default();
        for write in &flow.region_writes {
            let counts = &mut self.tags.keys[write.key as usize].counts;
            counts.region_writes += 1;
            if flow.notes.copies.contains_key(&write.place) {
                counts.copies += 1;
                continue;
            }
            let Some(at) = write.at else {
                counts.unrooted += 1;
                continue;
            };
            let found = search(
                flow,
                &self.tags.nested,
                write.block,
                write.stmt,
                at,
                write.key,
            );
            let bits = match found {
                Some(bits) => {
                    self.tags.keys[write.key as usize].counts.searched += 1;
                    Some(bits)
                }
                None => write.facts,
            };
            let key = &self.tags.keys[write.key as usize];
            let Some(bits) = bits else {
                continue;
            };
            if bits & key.full() == key.full() {
                continue;
            }
            let candidate = (key.tag, write.key, bits);
            decided
                .entry(write.place)
                .and_modify(|best| {
                    if (candidate.0, candidate.1) < (best.0, best.1) {
                        *best = candidate;
                    }
                })
                .or_insert(candidate);
        }
        let mut decided: Vec<(usize, (i64, KeyId, u64))> = decided.into_iter().collect();
        decided.sort_unstable();
        for (place, (_, key, bits)) in decided {
            let set = self.tag_set(key, bits);
            flow.notes.parts.insert(place, set);
            self.tags.keys[key as usize].counts.parts += 1;
        }
        let function = flow.function;
        for write in &flow.tag_writes {
            let key = &mut self.tags.keys[write.key as usize];
            key.counts.tag_writes += 1;
            if write.at.is_none() {
                key.counts.unrooted += 1;
            }
            if write.correlated {
                key.counts.correlated += 1;
                continue;
            }
            if write.paired {
                continue;
            }
            let values = match write.bits {
                Some(bits) => {
                    key.unclean_values |= bits;
                    key.counts.unpaired_constant += 1;
                    for bit in (0..64).map(|i| 1u64 << i).filter(|bit| bits & bit != 0) {
                        if key.unclean_by.iter().filter(|(b, ..)| *b == bit).count() < 6 {
                            key.unclean_by.push((
                                bit,
                                format!("{} at {}:{}", function.name, function.file, write.line),
                                write.at.is_some(),
                            ));
                        }
                    }
                    bits
                }
                None => {
                    key.counts.unpaired += 1;
                    if key.unsplit_by.len() < 12 {
                        key.unsplit_by.push(format!(
                            "{} at {}:{}",
                            function.name, function.file, write.line
                        ));
                    }
                    key.full()
                }
            };
            let keys = flow.notes.unpaired.entry(write.place).or_default();
            match keys.iter_mut().find(|(k, _)| *k == write.key) {
                Some((_, known)) => *known |= values,
                None => keys.push((write.key, values)),
            }
        }
    }

    /// Where a pointer points: a root and an offset from it. A pointer
    /// computed from memory or by a number that is not constant is a
    /// path of its own.
    fn pointer_base(&mut self, flow: &Flow<'_>, state: &State, pointer: &Expr) -> Option<At> {
        match &pointer.kind {
            ExprKind::Cast(inner) => self
                .pointer_base(flow, state, inner)
                .or_else(|| self.pointer_root(flow, pointer)),
            ExprKind::Offset(inner, bytes) => {
                match constant(bytes).and_then(|bytes| i64::try_from(bytes).ok()) {
                    Some(bytes) => {
                        let (root, offset) = self.pointer_base(flow, state, inner)?;
                        Some((root, offset.checked_add(bytes)?))
                    }
                    None => self.pointer_root(flow, pointer),
                }
            }
            ExprKind::Use(Place::Var(var)) if flow.is_register(*var) => Some(
                state
                    .alias
                    .get(var)
                    .copied()
                    .unwrap_or((Root::Reg(*var), 0)),
            ),
            ExprKind::AddrOf(place) => {
                let located = locate(place)?;
                let (root, offset) = self.resolve(flow, state, located.base)?;
                Some((root, offset.checked_add(located.start)?))
            }
            _ => self.pointer_root(flow, pointer),
        }
    }

    /// The path a pointer is, as a root.
    fn pointer_root(&mut self, flow: &Flow<'_>, pointer: &Expr) -> Option<At> {
        if !matches!(
            self.types.shape(&pointer.ty),
            Shape::Pointer { function: false }
        ) {
            return None;
        }
        if let Some(&known) = flow.paths.borrow().pointers.get(pointer) {
            return known.map(|path| (Root::Path(path), 0));
        }
        let mut path = Path::default();
        let made = self
            .expr_deps(flow, pointer, &mut path)
            .map(|()| flow.paths.borrow_mut().add(path));
        flow.paths
            .borrow_mut()
            .pointers
            .insert(pointer.clone(), made);
        made.map(|path| (Root::Path(path), 0))
    }

    /// The path the address of an element of an array indexed by a value
    /// that is not constant is, as a root.
    fn element_root(&mut self, flow: &Flow<'_>, element: &Place) -> Option<At> {
        if let Some(&known) = flow.paths.borrow().elements.get(element) {
            return known.map(|path| (Root::Path(path), 0));
        }
        let mut path = Path::default();
        let made = self
            .address_deps(flow, element, &mut path)
            .map(|()| flow.paths.borrow_mut().add(path));
        flow.paths
            .borrow_mut()
            .elements
            .insert(element.clone(), made);
        made.map(|path| (Root::Path(path), 0))
    }

    /// Adds what a value is computed from to a path: none where it is
    /// what is not known.
    fn expr_deps(&mut self, flow: &Flow<'_>, expr: &Expr, path: &mut Path) -> Option<()> {
        match &expr.kind {
            ExprKind::Use(Place::Var(var)) if flow.is_register(*var) => path.regs.push(*var),
            ExprKind::Use(place) => {
                self.address_deps(flow, place, path)?;
                let load = self.place_bytes(flow, place, Some(&expr.ty));
                match load.named {
                    Some(Named::Reg(var)) => path.regs.push(var),
                    _ => path.loads.push(load),
                }
            }
            ExprKind::AddrOf(place) => self.address_deps(flow, place, path)?,
            ExprKind::Unary(_, inner) | ExprKind::Cast(inner) => {
                self.expr_deps(flow, inner, path)?;
            }
            ExprKind::Binary(_, left, right) | ExprKind::Offset(left, right) => {
                self.expr_deps(flow, left, path)?;
                self.expr_deps(flow, right, path)?;
            }
            ExprKind::Int(_)
            | ExprKind::Float
            | ExprKind::Str(_)
            | ExprKind::Func(_)
            | ExprKind::SizeOf(_)
            | ExprKind::AlignOf(_) => {}
            ExprKind::VaArg(_) | ExprKind::Unknown(_) => return None,
        }
        Some(())
    }

    /// Adds what a place's address is computed from to a path.
    fn address_deps(&mut self, flow: &Flow<'_>, place: &Place, path: &mut Path) -> Option<()> {
        match place {
            Place::Var(_) | Place::Global(_) => Some(()),
            Place::Deref(pointer) => self.expr_deps(flow, pointer, path),
            Place::Field(inner, _) => self.address_deps(flow, inner, path),
            Place::Index(inner, index) => {
                self.address_deps(flow, inner, path)?;
                self.expr_deps(flow, index, path)
            }
        }
    }

    /// The bytes a place's value of a type is in.
    fn place_bytes(&mut self, flow: &Flow<'_>, place: &Place, ty: Option<&Arc<str>>) -> Bytes {
        let mut bytes = self.place_start(flow, place);
        bytes.width = ty.map_or(8, |ty| i64::from(self.width_of(ty)));
        if let Some(ty) = ty {
            if let Shape::Record(record) = self.types.shape(ty) {
                bytes.chain.push((self.types.canonical(&record), Some(0)));
            }
            bytes.kind = self.kind_of(ty);
        }
        if let Some((outer, _)) = bytes.chain.first() {
            let outer = outer.clone();
            self.nest(&outer);
        }
        bytes
    }

    /// Where a place's bytes start: in the variable or global they are
    /// in, where named, and in the records they are in through the
    /// members accessed.
    fn place_start(&mut self, flow: &Flow<'_>, place: &Place) -> Bytes {
        let at = |named| Bytes {
            named,
            start: Some(0),
            width: 0,
            chain: Vec::new(),
            kind: None,
        };
        match place {
            Place::Var(var) if flow.is_register(*var) => at(Some(Named::Reg(*var))),
            Place::Var(var) => at(Some(Named::Mem(*var))),
            Place::Global(name) => at(Some(Named::Glob(self.global_id(name)))),
            Place::Deref(_) => at(None),
            Place::Field(inner, field) => {
                let mut bytes = self.place_start(flow, inner);
                bytes
                    .chain
                    .push((self.types.canonical(&field.record), Some(0)));
                bytes.shift(field.offset.and_then(|offset| i64::try_from(offset).ok()));
                bytes
            }
            Place::Index(inner, index) => {
                let mut bytes = self.place_start(flow, inner);
                bytes.shift(constant(index).and_then(|index| i64::try_from(index).ok()));
                bytes
            }
        }
    }

    /// The scalar type a value of a type is accessed by.
    fn kind_of(&mut self, ty: &Arc<str>) -> Option<Kind> {
        match self.types.shape(ty) {
            Shape::Integer => match self.types.size(ty)? {
                1 => Some(Kind::Char),
                size => Some(Kind::Int(u32::try_from(size).ok()?)),
            },
            Shape::Float => Some(Kind::Float(u32::try_from(self.types.size(ty)?).ok()?)),
            Shape::Pointer { .. } => Some(Kind::Pointer),
            _ => None,
        }
    }

    /// The records a record holds, with where their instances are, made
    /// once. An array at a record's end may run past it.
    fn nest(&mut self, record: &Arc<str>) {
        if self.tags.nested.contains_key(record) {
            return;
        }
        // A record holding itself holds nothing more by that member.
        self.tags
            .nested
            .insert(record.clone(), Some(Box::default()));
        let held = self.held(record);
        self.tags.nested.insert(record.clone(), held);
    }

    fn held(&mut self, record: &Arc<str>) -> Option<Held> {
        let layout = self.types.layout(record)?;
        let end = layout.fields.iter().map(|field| field.offset).max();
        let mut held: Vec<(Arc<str>, Instances)> = Vec::new();
        for field in &layout.fields {
            let Some(inner) = &field.holds else {
                continue;
            };
            let inner = self.types.canonical(inner);
            let size = i64::try_from(self.types.record_size(&inner)?).ok()?.max(1);
            let array = matches!(self.types.shape(&field.ty), Shape::Array);
            let count = match field.size {
                _ if array && Some(field.offset) == end && !layout.is_union => i64::MAX,
                Some(total) => (i64::try_from(total).ok()? / size).max(1),
                None => i64::MAX,
            };
            let instances = Instances {
                first: i64::try_from(field.offset).ok()?,
                stride: size,
                count,
            };
            self.nest(&inner);
            let deeper = self.tags.nested.get(&inner).cloned().flatten()?;
            held.push((inner, instances));
            held.extend(
                deeper
                    .iter()
                    .map(|(record, at)| (record.clone(), at.within(instances))),
            );
        }
        held.sort_unstable_by(|a, b| {
            (&a.0, a.1.first, a.1.stride, a.1.count).cmp(&(&b.0, b.1.first, b.1.stride, b.1.count))
        });
        held.dedup();
        Some(held.into())
    }

    fn resolve(&mut self, flow: &Flow<'_>, state: &State, base: Base<'_>) -> Option<At> {
        match base {
            Base::Var(var) => (!flow.is_register(var)).then_some((Root::Mem(var), 0)),
            Base::Global(name) => Some((Root::Glob(self.global_id(name)), 0)),
            Base::Pointer(pointer) => self.pointer_base(flow, state, pointer),
            Base::Element(element) => self.element_root(flow, element),
        }
    }

    /// The keys a place's bytes are in, and where they start from the
    /// root.
    fn place_access(
        &mut self,
        flow: &Flow<'_>,
        state: &State,
        place: &Place,
        width: u32,
    ) -> Option<(Access, i64)> {
        let located = locate(place)?;
        if located.chain.is_empty() {
            return None;
        }
        let resolved = self.resolve(flow, state, located.base);
        let shift = resolved.map_or(0, |(_, offset)| offset);
        let access = self.classify(resolved, &located.chain, located.start, width);
        Some((access, located.start + shift))
    }

    /// The keys the bytes `start..start + width` are in, of the records in
    /// the chain, all from a root moved by an offset.
    fn classify(
        &self,
        resolved: Option<At>,
        chain: &[(Arc<str>, i64)],
        start: i64,
        width: u32,
    ) -> Access {
        let mut access = Access::default();
        let (root, shift) = match resolved {
            Some((root, shift)) => (Some(root), shift),
            None => (None, 0),
        };
        let start = start + shift;
        let width = i64::from(width);
        let classify_in = |record: &Arc<str>, inst: i64, access: &mut Access| {
            for &key in self.tags.by_record.get(record).into_iter().flatten() {
                let k = &self.tags.keys[key as usize];
                let rel = start - inst;
                let at = root.map(|root| (root, inst));
                if rel <= k.tag && k.tag < rel + width {
                    access.tags.push((key, at, rel));
                } else if rel < k.region.1 && k.region.0 < rel + width {
                    access.regions.push((key, at));
                }
            }
        };
        for (record, inst) in chain {
            let record = self.types.canonical(record);
            let inst = inst + shift;
            classify_in(&record, inst, &mut access);
            for (union, at) in self.survey.variants.get(&record).into_iter().flatten() {
                classify_in(union, inst - at, &mut access);
            }
        }
        let keys = &self.tags.keys;
        access
            .tags
            .sort_by_key(|&(key, at, rel)| (keys[key as usize].tag, key, at, rel));
        access.tags.dedup();
        access
            .regions
            .sort_by_key(|&(key, at)| (keys[key as usize].tag, key, at));
        access.regions.dedup();
        access
    }

    /// The byte a value is, where it is a key's byte read from its
    /// instance or holds it in its low byte: through conversions, and
    /// operations by constants that leave the low byte.
    fn view_of(&mut self, flow: &Flow<'_>, state: &State, value: &Expr) -> Option<View> {
        // Whether the value is the byte alone, and whether an operation
        // outside left other bits beside it.
        let mut exact = false;
        let mut loose = false;
        let mut value = value;
        loop {
            match &value.kind {
                ExprKind::Cast(inner) => {
                    if self.types.size(&value.ty) == Some(1) {
                        exact |= !loose;
                    }
                    value = inner;
                }
                ExprKind::Binary(BinOp::BitAnd, left, right) if constant(right) == Some(0xff) => {
                    exact |= !loose;
                    value = left;
                }
                ExprKind::Binary(BinOp::BitAnd, left, right) if constant(left) == Some(0xff) => {
                    exact |= !loose;
                    value = right;
                }
                ExprKind::Binary(BinOp::BitOr, left, right)
                    if constant(right).is_some_and(|c| c & 0xff == 0) =>
                {
                    loose |= !exact;
                    value = left;
                }
                ExprKind::Binary(BinOp::BitOr, left, right)
                    if constant(left).is_some_and(|c| c & 0xff == 0) =>
                {
                    loose |= !exact;
                    value = right;
                }
                ExprKind::Binary(BinOp::BitAnd, left, right)
                    if constant(right).is_some_and(|c| c & 0xff == 0xff) =>
                {
                    loose |= !exact;
                    value = left;
                }
                ExprKind::Binary(BinOp::BitAnd, left, right)
                    if constant(left).is_some_and(|c| c & 0xff == 0xff) =>
                {
                    loose |= !exact;
                    value = right;
                }
                _ => break,
            }
        }
        match &value.kind {
            ExprKind::Use(Place::Var(var)) if flow.is_register(*var) => {
                state.binds.get(var).map(|view| View {
                    exact: exact || (view.exact && !loose),
                    ..*view
                })
            }
            ExprKind::Use(place) => {
                let width = self.width_of(&value.ty);
                let (access, _) = self.place_access(flow, state, place, width)?;
                let keys = &self.tags.keys;
                let &(key, at, _) = access
                    .tags
                    .iter()
                    .find(|&&(key, at, rel)| at.is_some() && rel == keys[key as usize].tag)?;
                Some(View {
                    at: at?,
                    key,
                    exact: exact || (width == 1 && !loose),
                })
            }
            _ => None,
        }
    }

    /// The byte a call of an accessor gives.
    fn accessor_view(
        &mut self,
        flow: &Flow<'_>,
        state: &State,
        callee: &Arc<str>,
        arg: &Expr,
    ) -> Option<View> {
        let f = *self.fn_ids.get(callee)?;
        let accessor = self.survey.accessors.get(&f)?.clone();
        let base = self.pointer_base(flow, state, arg)?;
        let access = self.classify(Some(base), &accessor.chain, accessor.start, accessor.width);
        let keys = &self.tags.keys;
        let &(key, at, _) = access
            .tags
            .iter()
            .find(|&&(key, at, rel)| at.is_some() && rel == keys[key as usize].tag)?;
        Some(View {
            at: at?,
            key,
            exact: accessor.width == 1,
        })
    }

    /// Each successor of a block, with what the condition taking it
    /// shows: the values it leaves a byte with, and the constants it
    /// leaves a register variable holding.
    fn edges(&mut self, flow: &mut Flow<'_>, state: &State, term: &Term) -> Vec<(BlockId, Shown)> {
        match term {
            Term::Branch { cond, then, els } => {
                if then == els {
                    return vec![(*then, Shown::default())];
                }
                let (byte_yes, byte_no) = match self.condition(flow, state, cond) {
                    Some((view, yes, no)) => (Some((view, yes)), Some((view, no))),
                    None => (None, None),
                };
                let (var_yes, var_no) = match self.var_condition(flow, state, cond) {
                    Some((var, yes, no)) => (yes.map(|yes| (var, yes)), no.map(|no| (var, no))),
                    None => (None, None),
                };
                vec![
                    (
                        *then,
                        Shown {
                            byte: byte_yes,
                            var: var_yes,
                        },
                    ),
                    (
                        *els,
                        Shown {
                            byte: byte_no,
                            var: var_no,
                        },
                    ),
                ]
            }
            Term::Switch { on, cases, default } => {
                let bytes = self.switch_bytes(flow, state, on, cases, *default);
                let values = self.switch_values(flow, state, on, cases, *default);
                let mut targets = successors(term);
                targets.sort_unstable();
                targets.dedup();
                targets
                    .into_iter()
                    .map(|target| {
                        let byte = bytes.as_ref().and_then(|(view, bits)| {
                            let &(_, bits) = bits.iter().find(|(t, _)| *t == target)?;
                            Some((*view, bits))
                        });
                        let var = values.as_ref().and_then(|(var, values)| {
                            let (_, values) = values.iter().find(|(t, _)| *t == target)?;
                            Some((*var, values.clone()?))
                        });
                        (target, Shown { byte, var })
                    })
                    .collect()
            }
            _ => successors(term)
                .into_iter()
                .map(|target| (target, Shown::default()))
                .collect(),
        }
    }

    /// The values a switch on a byte leaves it with on each edge.
    fn switch_bytes(
        &mut self,
        flow: &Flow<'_>,
        state: &State,
        on: &Expr,
        cases: &[(i128, i128, BlockId)],
        default: BlockId,
    ) -> Option<(View, Vec<(BlockId, u64)>)> {
        let view = self.view_of(flow, state, on)?;
        let key = &self.tags.keys[view.key as usize];
        let mut bits: Vec<(BlockId, u64)> = Vec::new();
        let mut add = |target: BlockId, more: u64| match bits.iter_mut().find(|(t, _)| *t == target)
        {
            Some((_, bits)) => *bits |= more,
            None => bits.push((target, more)),
        };
        for &(low, high, target) in cases {
            let more = if view.exact {
                key.bits_where(|v| low <= v && v <= high)
            } else if low == high {
                key.bit((low & 0xff) as u8)
            } else {
                key.full()
            };
            add(target, more);
        }
        let more = if view.exact {
            key.bits_where(|v| !cases.iter().any(|&(low, high, _)| low <= v && v <= high))
        } else {
            key.full()
        };
        add(default, more);
        Some((view, bits))
    }

    /// The constants a switch on a register variable leaves it holding
    /// on each edge, where they are few.
    fn switch_values(
        &mut self,
        flow: &Flow<'_>,
        state: &State,
        on: &Expr,
        cases: &[(i128, i128, BlockId)],
        default: BlockId,
    ) -> Option<(VarId, Vec<(BlockId, Consts)>)> {
        let chain = self.cast_chain(flow, on)?;
        let mut out: Vec<(BlockId, Consts)> = Vec::new();
        let mut add = |target: BlockId, more: Option<Vec<i128>>| match out
            .iter_mut()
            .find(|(t, _)| *t == target)
        {
            Some((_, values)) => {
                *values = match (values.take(), more) {
                    (Some(mut values), Some(more)) => {
                        values.extend(more);
                        few(values)
                    }
                    _ => None,
                };
            }
            None => out.push((target, more)),
        };
        let in_case = |value: i128| {
            cases
                .iter()
                .find(|&&(low, high, _)| low <= value && value <= high)
                .map(|&(_, _, target)| target)
        };
        match state.consts.get(&chain.var) {
            Some(values) => {
                for &(_, _, target) in cases {
                    add(target, Some(Vec::new()));
                }
                add(default, Some(Vec::new()));
                for &value in values {
                    let seen = chain.convert(value);
                    add(in_case(seen).unwrap_or(default), Some(vec![value]));
                }
            }
            None => {
                if !chain.preserving {
                    return None;
                }
                let range = bounds(chain.own);
                for &(low, high, target) in cases {
                    add(target, interval(low, high, range));
                }
                add(default, None);
            }
        }
        Some((chain.var, out))
    }

    /// The register variable a condition compares with a constant, with
    /// the constants it holds where the condition holds and where it does
    /// not, each where they are known to be few.
    fn var_condition(
        &mut self,
        flow: &Flow<'_>,
        state: &State,
        cond: &Expr,
    ) -> Option<(VarId, Consts, Consts)> {
        match &cond.kind {
            ExprKind::Unary(UnOp::Not, inner) => {
                let (var, yes, no) = self.var_condition(flow, state, inner)?;
                Some((var, no, yes))
            }
            ExprKind::Cast(inner)
                if matches!(&inner.kind, ExprKind::Unary(UnOp::Not, _))
                    || matches!(&inner.kind, ExprKind::Binary(op, _, _) if is_comparison(*op)) =>
            {
                self.var_condition(flow, state, inner)
            }
            ExprKind::Binary(op, left, right) if is_comparison(*op) => {
                let single = |values: Option<Vec<i128>>| match values.as_deref() {
                    Some(&[value]) => Some(value),
                    _ => None,
                };
                let right_value = single(self.fold_values(flow, state, right));
                let (operand, value, op) = match right_value {
                    Some(value) => (left, value, *op),
                    None => (
                        right,
                        single(self.fold_values(flow, state, left))?,
                        flip(*op),
                    ),
                };
                self.compared(flow, state, operand, op, value)
            }
            _ => self.compared(flow, state, cond, BinOp::Ne, 0),
        }
    }

    /// The constants a register variable a value is holds where its
    /// comparison with a constant holds and where it does not.
    fn compared(
        &mut self,
        flow: &Flow<'_>,
        state: &State,
        operand: &Expr,
        op: BinOp,
        value: i128,
    ) -> Option<(VarId, Consts, Consts)> {
        let chain = self.cast_chain(flow, operand)?;
        if let Some(values) = state.consts.get(&chain.var) {
            let (yes, no): (Vec<i128>, Vec<i128>) = values
                .iter()
                .partition(|&&v| compare(op, chain.convert(v), value));
            return Some((chain.var, Some(yes), Some(no)));
        }
        if !chain.preserving {
            return None;
        }
        let range = bounds(chain.own);
        Some((
            chain.var,
            compared_values(op, value, range),
            compared_values(negate(op), value, range),
        ))
    }

    /// The register variable an integer value is, through its
    /// conversions.
    fn cast_chain(&mut self, flow: &Flow<'_>, value: &Expr) -> Option<Chain> {
        let mut conversions = Vec::new();
        let mut preserving = true;
        let mut value = value;
        loop {
            match &value.kind {
                ExprKind::Cast(inner) => {
                    let to = self.conversion(&value.ty)?;
                    let from = self.conversion(&inner.ty)?;
                    preserving &= preserves(from, to);
                    conversions.extend(to);
                    value = inner;
                }
                ExprKind::Use(Place::Var(var)) if flow.is_register(*var) => {
                    conversions.reverse();
                    return Some(Chain {
                        var: *var,
                        conversions,
                        preserving,
                        own: self.conversion(&value.ty)?,
                    });
                }
                _ => return None,
            }
        }
    }

    /// The byte a condition tests, with the values it may have where the
    /// condition holds and where it does not.
    fn condition(
        &mut self,
        flow: &Flow<'_>,
        state: &State,
        cond: &Expr,
    ) -> Option<(View, u64, u64)> {
        match &cond.kind {
            ExprKind::Unary(UnOp::Not, inner) => {
                let (view, yes, no) = self.condition(flow, state, inner)?;
                Some((view, no, yes))
            }
            ExprKind::Cast(inner)
                if matches!(&inner.kind, ExprKind::Unary(UnOp::Not, _))
                    || matches!(&inner.kind, ExprKind::Binary(op, _, _) if is_comparison(*op)) =>
            {
                self.condition(flow, state, inner)
            }
            ExprKind::Binary(op, left, right) if is_comparison(*op) => {
                let (operand, value, op) = match constant(right) {
                    Some(value) => (left, value, *op),
                    None => (right, constant(left)?, flip(*op)),
                };
                let view = self.view_of(flow, state, operand)?;
                let key = &self.tags.keys[view.key as usize];
                let low = key.bit((value & 0xff) as u8);
                let (yes, no) = match op {
                    BinOp::Eq if !view.exact => (low, key.full()),
                    BinOp::Ne if !view.exact => (key.full(), low),
                    _ if !view.exact => return None,
                    _ => (
                        key.bits_where(|v| compare(op, v, value)),
                        key.bits_where(|v| !compare(op, v, value)),
                    ),
                };
                Some((view, yes, no))
            }
            _ => {
                let view = self.view_of(flow, state, cond)?;
                let key = &self.tags.keys[view.key as usize];
                let (yes, no) = if view.exact {
                    (key.bits_where(|v| v != 0), key.bits_where(|v| v == 0))
                } else {
                    (key.full(), key.bit(0))
                };
                Some((view, yes, no))
            }
        }
    }

    /// Once every function's notes are made: a context holding every
    /// value is none.
    pub(crate) fn decide_parts(&mut self) {
        let count = self.tag_sets.len();
        self.part_final = vec![0; count];
        self.ctx_final = vec![0; count];
        for id in 1..count {
            let (part, ctx) = self.finals(id as TagSetId);
            self.part_final[id] = part;
            self.ctx_final[id] = ctx;
        }
        let (parts, ctxs) = (&self.part_final, &self.ctx_final);
        for cons in self.cons.values_mut() {
            for cons in cons.iter_mut() {
                match cons {
                    Cons::Load { ctx, .. } => *ctx = ctxs[*ctx as usize],
                    Cons::Store { part, .. } => *part = parts[*part as usize],
                    Cons::Call { .. }
                    | Cons::Gather { .. }
                    | Cons::Scatter { .. }
                    | Cons::Unpaired { .. } => {}
                }
            }
        }
    }

    /// A tag set as a part and as a context: a context holding every value
    /// of its key is none.
    pub(crate) fn finals(&self, id: TagSetId) -> (TagSetId, TagSetId) {
        let (key, values) = self.tag_sets[id as usize];
        let full = if key == VARIADIC {
            self.variadic.full()
        } else {
            self.tags.keys[key as usize].full()
        };
        (id, if values & full == full { 0 } else { id })
    }

    /// Makes the reads and writes put off for the tags, under the decided
    /// parts and contexts, and starts the solving phase.
    pub(crate) fn run_static_ops(&mut self) {
        self.phase = Phase::Solve;
        self.rank = vec![u32::MAX; self.rep.len()];
        self.queued = vec![false; self.rep.len()];
        for op in std::mem::take(&mut self.static_ops) {
            match op {
                StaticOp::Read {
                    loc,
                    width,
                    ctx,
                    dst,
                } => {
                    let (obj, off) = self.locs[loc as usize];
                    let window = self.loc_windows[loc as usize];
                    let ctx = self.ctx_final[ctx as usize];
                    let reader = self.reader(obj, off, ctx, width, window);
                    self.add_edge(reader, dst, COPY);
                }
                StaticOp::Write {
                    loc,
                    width,
                    part,
                    src,
                } => {
                    let (obj, off) = self.locs[loc as usize];
                    let window = self.loc_windows[loc as usize];
                    let part = self.part_final[part as usize];
                    let cell = self.cell(obj, off, part, width, window);
                    self.add_edge(src, cell, COPY);
                }
            }
        }
    }
}

/// The values the next writes of an instance's byte write on every path
/// from a write of its region; none where a path returns, changes what
/// the root reaches, reads too many blocks or writes a value that is not
/// constant first.
fn search(
    flow: &Flow<'_>,
    nested: &Nested,
    block: BlockId,
    from: usize,
    at: At,
    key: KeyId,
) -> Option<u64> {
    let blocks = &flow.function.blocks;
    let changed = |block: BlockId, stmt: usize, when: When| {
        flow.changed(nested, at.0, block, (stmt, when), (stmt, when))
    };
    if changed(block, from, When::After) {
        return None;
    }
    let mut bits = 0u64;
    let mut found = false;
    let mut seen: FxHashSet<BlockId> = FxHashSet::default();
    let mut work = vec![(block, from + 1)];
    while let Some((block, from)) = work.pop() {
        let stmts = blocks.get(block as usize)?.stmts.len();
        let mut ended = false;
        for i in from..stmts {
            if changed(block, i, When::Before) {
                return None;
            }
            for &id in flow.tag_at.get(&(block, i)).into_iter().flatten() {
                let write = &flow.tag_writes[id];
                if write.key == key && write.at == Some(at) {
                    bits |= write.bits?;
                    found = true;
                    ended = true;
                }
            }
            if ended {
                break;
            }
            if changed(block, i, When::After) {
                return None;
            }
        }
        if ended {
            continue;
        }
        let term = &blocks[block as usize].term;
        match term {
            Term::Return(_) => return None,
            Term::Unreachable => {}
            _ => {
                for target in successors(term) {
                    if seen.insert(target) {
                        if seen.len() > SEARCH_BLOCKS {
                            return None;
                        }
                        work.push((target, 0));
                    }
                }
            }
        }
    }
    found.then_some(bits)
}
