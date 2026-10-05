//! Where each pointer may point, and so which functions each call through
//! a pointer may reach: an inclusion-based analysis of the whole program,
//! blind to the order of statements, over memory as bytes.
//!
//! - An object is its bytes. A location is an object and an offset into
//!   it: a constant offset, one of the offsets a stride apart, or any.
//!   What a store writes at a location is a cell of its object, by the
//!   offset, the store's width and the tag part it was written under; a
//!   load reads each cell its bytes overlap, the cell's pointers whole
//!   where the two agree on offset and width, else at any offset of
//!   their objects. A copy of bytes between objects is read by the
//!   destination's readers, each through a reader of the bytes the copy
//!   took from the source (`memory`).
//! - A value of any type and width may hold a pointer: arithmetic by a
//!   constant moves it, an index by a stride, any other arithmetic loses
//!   its offset but not its object. A pointer made in an array (an array
//!   member indexed or decayed, the outermost where arrays nest) or at a
//!   member (an array of it alone) keeps to it when moved by a stride, as
//!   C keeps pointer arithmetic inside the array, until a constant move
//!   takes it out; an access naming an array inside it narrows it.
//! - The objects: each function's code, each variable whose address is
//!   taken or whose members are read (the rest are values), each global,
//!   the memory each allocating call gives (a function returning only
//!   memory it allocates is analysed again for each call of it), the
//!   string literals, the code of each library the build has no source
//!   of and its own memory of each class, and the bytes from outside the
//!   process, whose pointers point outside again.
//! - A library is taken by its types: it holds each pointer it is given
//!   by its class, gives it back only where its class may go, reads and
//!   writes through it at its type, and calls the functions it holds
//!   (`library`). The C library and the compiler's builtins are read
//!   function by function besides (`externals`), and the allocators by
//!   their contract.
//! - A union whose member a tag tells (`zval`'s type) is read apart by
//!   the tag's values where the conditions of a function show them, the
//!   values written under each kept apart where every write of the tag is
//!   paired with a write of the union (`tags`).
//! - The constraints are reduced before they are solved: the nodes that
//!   point to the same objects are found offline and merged (`hvn`),
//!   cycles are merged as they form, the sets are hash-consed bitmaps, and
//!   each node passes on only what it gained (`solve`).
//! - The functions' code stays in the store: it is read once to survey the
//!   program and once to generate the constraints, and the call graph's
//!   components are loaded one by one, callees first (`report`).

pub(crate) mod bitmap;
mod blame;
mod constraints;
mod hvn;
mod library;
mod memory;
mod report;
mod solve;
mod survey;
mod tags;
mod trace;
mod variadic;

pub(crate) use trace::Asked;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::clang::Platform;
use crate::externals::Later;
use crate::ir::{BlockId, FnName, Function, GlobalName, VarId};
use crate::libraries::{LibId, Libraries};
use crate::program::Program;
use crate::store::{Hash, Store};
use crate::types::Types;

use bitmap::{EMPTY, SetId, Sets};
use memory::{FULL, Slot, Window};

pub(crate) type NodeId = u32;
pub(crate) type ObjId = u32;
pub(crate) type LocId = u32;
pub(crate) type TrId = u32;
/// A tag part or context: a key and a set of its values, 0 for none.
pub(crate) type TagSetId = u32;
pub(crate) type SiteId = u32;
pub(crate) type DefId = u32;
pub(crate) type InstId = u32;
pub(crate) type FnId = u32;
pub(crate) type GlobalId = u32;
/// A type, interned.
pub(crate) type TyId = u32;

/// No node, object or location.
pub(crate) const NONE: u32 = u32::MAX;
/// The transform that keeps an offset.
pub(crate) const COPY: TrId = 0;
/// The transform to any offset.
pub(crate) const TO_ANY: TrId = 1;
/// The bytes a copy reaches when it runs to the end of its object.
pub(crate) const TO_END: u64 = u64::MAX;

/// An offset into an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Off {
    Exact(i64),
    /// Any of the offsets `rem` plus a multiple of `stride`.
    Stride {
        rem: u32,
        stride: u32,
    },
    Any,
}

/// How a pointer's offset moves: by `add`, then by any multiple of
/// `stride` (none where it is 0), or to any offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Transform {
    pub(crate) add: i64,
    pub(crate) stride: u32,
    pub(crate) any: bool,
    /// The array or member the access moving it names, from the offset
    /// moved: the bytes a pointer at a constant offset keeps to, of those
    /// it kept to.
    pub(crate) bound: Option<(i64, i64)>,
}

impl Transform {
    pub(crate) const IDENTITY: Transform = Transform {
        add: 0,
        stride: 0,
        any: false,
        bound: None,
    };
    pub(crate) const ANY: Transform = Transform {
        add: 0,
        stride: 0,
        any: true,
        bound: None,
    };

    /// The transform moving by `add` and any multiple of `stride`, a
    /// stride of 1 being any offset.
    pub(crate) fn linear(add: i64, stride: u64) -> Transform {
        match stride {
            0 => Transform {
                add,
                stride: 0,
                any: false,
                bound: None,
            },
            1 => Transform::ANY,
            _ if stride > u64::from(u32::MAX) => Transform::ANY,
            _ => Transform {
                add: add.rem_euclid(stride as i64),
                stride: stride as u32,
                any: false,
                bound: None,
            },
        }
    }

    /// The same move, in the array from `low` to `high` of the offset
    /// moved.
    pub(crate) fn within(self, low: i64, high: i64) -> Transform {
        Transform {
            bound: (!self.any).then_some((low, high)),
            ..self
        }
    }

    /// This transform, then `then`: in this one's array, narrowed to
    /// `then`'s where this one moves by a constant.
    pub(crate) fn then(self, then: Transform) -> Transform {
        if self.any || then.any {
            return Transform::ANY;
        }
        let stride = gcd(u64::from(self.stride), u64::from(then.stride));
        let moved = Transform::linear(self.add.saturating_add(then.add), stride);
        let bound = match (self.bound, then.bound) {
            (bound, Some((low, high))) if self.stride == 0 => {
                let named = (low.saturating_add(self.add), high.saturating_add(self.add));
                Some(narrow(bound.unwrap_or(FULL), named))
            }
            (bound, _) => bound,
        };
        Transform { bound, ..moved }
    }

    pub(crate) fn apply(self, off: Off) -> Off {
        if self.any {
            return Off::Any;
        }
        match off {
            Off::Any => Off::Any,
            Off::Exact(at) => stride_off(at.saturating_add(self.add), u64::from(self.stride)),
            Off::Stride { rem, stride } => {
                let stride = gcd(u64::from(stride), u64::from(self.stride));
                stride_off(i64::from(rem).saturating_add(self.add), stride)
            }
        }
    }

    /// Moves an offset in the array its pointer keeps to, and gives the
    /// array it keeps to after: narrowed to this transform's where the
    /// pointer is at a constant offset, none once a constant move takes
    /// it out of its array or its offset is lost.
    pub(crate) fn apply_in(self, off: Off, window: Window) -> (Off, Window) {
        let moved = self.apply(off);
        let window = match (off, self.bound) {
            (Off::Exact(at), Some((low, high))) => {
                narrow(window, (at.saturating_add(low), at.saturating_add(high)))
            }
            _ => window,
        };
        if window == FULL || moved == Off::Any {
            return (moved, FULL);
        }
        if self.stride != 0 {
            return (moved, window);
        }
        // A constant move: the pointer stays in the array where every
        // offset it may have, moved, is in it or just past its end.
        let (first, last) = match off {
            Off::Exact(at) => (at, Some(at)),
            Off::Stride { rem, stride } => {
                let stride = i64::from(stride);
                let rem = i64::from(rem);
                let first = window.0.saturating_add((rem - window.0).rem_euclid(stride));
                let last = (window.1 != i64::MAX)
                    .then(|| window.1 - 1 - (window.1 - 1 - rem).rem_euclid(stride));
                (first, last)
            }
            Off::Any => return (moved, FULL),
        };
        let inside = first.saturating_add(self.add) >= window.0
            && last.is_none_or(|last| last.saturating_add(self.add) <= window.1);
        (moved, if inside { window } else { FULL })
    }
}

/// The bytes of an array a pointer keeps to that an access inside it
/// names: those the two share, or the access's where they share none, the
/// pointer having left its array.
fn narrow(window: Window, named: Window) -> Window {
    let shared = memory::meet(window, named);
    if shared.0 < shared.1 { shared } else { named }
}

pub(crate) fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// The offsets `at` plus a multiple of `stride`: `at` alone for none, any
/// offset for 1.
pub(crate) fn stride_off(at: i64, stride: u64) -> Off {
    match stride {
        0 => Off::Exact(at),
        1 => Off::Any,
        _ if stride > u64::from(u32::MAX) => Off::Any,
        _ => Off::Stride {
            rem: at.rem_euclid(stride as i64) as u32,
            stride: stride as u32,
        },
    }
}

/// An offset as a base and a stride (0 for one offset), none for any.
pub(crate) fn off_parts(off: Off) -> Option<(i64, u64)> {
    match off {
        Off::Exact(at) => Some((at, 0)),
        Off::Stride { rem, stride } => Some((i64::from(rem), u64::from(stride))),
        Off::Any => None,
    }
}

/// The differences of one's offsets and the other's.
pub(crate) fn off_sub(a: Off, b: Off) -> Off {
    match (off_parts(a), off_parts(b)) {
        (Some((a, sa)), Some((b, sb))) => stride_off(a.saturating_sub(b), gcd(sa, sb)),
        _ => Off::Any,
    }
}

pub(crate) fn off_add(a: Off, b: Off) -> Off {
    match (off_parts(a), off_parts(b)) {
        (Some((a, sa)), Some((b, sb))) => stride_off(a.saturating_add(b), gcd(sa, sb)),
        _ => Off::Any,
    }
}

/// An offset kept within reach of its object: one far past its bounds is
/// any offset, and a stride as long as the object is its one offset.
pub(crate) fn normalize(off: Off, size: Option<u64>) -> Off {
    match off {
        Off::Any => Off::Any,
        Off::Exact(at) => {
            let (low, high) = match size {
                Some(size) => {
                    let size = size.min(1 << 40) as i64;
                    (-(size + 64), 2 * size + 64)
                }
                None => (-65536, 1 << 20),
            };
            if at < low || at > high {
                Off::Any
            } else {
                Off::Exact(at)
            }
        }
        Off::Stride { rem, stride } => match size {
            Some(size) if u64::from(stride) >= size && size > 0 => Off::Exact(i64::from(rem)),
            _ => off,
        },
    }
}

/// What an object is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjKind {
    /// A function's code: a pointer to it is the function.
    Function(FnId),
    /// A variable of an instance's.
    Var(InstId, VarId),
    /// An instance's result, for a function returning a record.
    Return(InstId),
    /// An instance's variable arguments.
    VarArgs(InstId),
    Global(GlobalId),
    /// The memory an allocating call gives, by the call.
    Heap(SiteId),
    /// The string literals.
    Literal,
    /// A library's code: a pointer to it is any of its functions.
    Library(LibId),
    /// A library's own memory, by the class of a pointer to it.
    LibOwn(LibId, library::ClassId),
    /// The bytes from outside the process.
    Outside,
    /// The bytes a copy between two pointers carries, by the copy.
    Copy(u32),
}

pub(crate) struct Object {
    pub(crate) kind: ObjKind,
    pub(crate) size: Option<u64>,
    pub(crate) memory: Option<Box<memory::Memory>>,
}

impl Object {
    /// Whether the object's bytes hold no pointer the program writes: a
    /// function's or a library's code, or a string literal.
    pub(crate) fn is_constant(&self) -> bool {
        matches!(
            self.kind,
            ObjKind::Function(_) | ObjKind::Library(_) | ObjKind::Literal
        )
    }

    /// Whether what the program writes into the object's bytes stays
    /// there: not for a constant object, nor for the bytes from outside,
    /// which hold the outside value whatever is written.
    pub(crate) fn takes_writes(&self) -> bool {
        !self.is_constant() && !matches!(self.kind, ObjKind::Outside)
    }
}

/// A constraint a node's pointers bring into effect as they come.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Cons {
    /// Reads `width` bytes at each pointer, moved, into `dst`.
    Load {
        tr: TrId,
        width: u32,
        ctx: TagSetId,
        dst: NodeId,
    },
    /// Writes `src`'s `width` bytes at each pointer, moved.
    Store {
        tr: TrId,
        width: u32,
        part: TagSetId,
        src: NodeId,
    },
    /// Calls each function pointed to.
    Call { site: SiteId },
    /// Copies `len` bytes at each pointer, moved, to a location.
    Gather {
        tr: TrId,
        len: u64,
        preserve: bool,
        dst: LocId,
    },
    /// Copies `len` bytes at a location to each pointer, moved.
    Scatter {
        tr: TrId,
        len: u64,
        preserve: bool,
        src: LocId,
    },
    /// Leaves values of a key in every part of it in each object pointed
    /// to: a write of its byte there that is not paired writes them.
    Unpaired { key: tags::KeyId, values: u64 },
}

/// How a call is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SiteKind {
    Direct,
    /// Through a pointer.
    Indirect,
    /// A library's call of a function it was given.
    ByLibrary(LibId),
    /// The C library's call of a function a call of it was given, while
    /// that call runs.
    Callback,
    /// The C library's call of a function a call of it kept, later.
    Later(Later),
}

/// An argument as a call passes it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Arg {
    /// Its value; for a record passed by value, its address; `NONE` where
    /// it holds no pointer.
    pub(crate) val: NodeId,
    /// The size of a record passed by value, `TO_END` where not known.
    pub(crate) record: Option<u64>,
    /// Its type as the call passes it; `NONE` where not known.
    pub(crate) ty: TyId,
    pub(crate) width: u32,
    /// Its value where it is a constant.
    pub(crate) constant: Option<i128>,
    /// The library whose memory it is also given, at the class of the
    /// parameter it is passed to.
    pub(crate) own: Option<LibId>,
}

/// Where a call's result goes.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Dst {
    None,
    Node(NodeId),
    /// A record, at the address the node holds, of the size.
    Record(NodeId, u64),
}

/// What a call may reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Target {
    Function(FnId),
    Library(LibId),
}

pub(crate) struct Site {
    /// The calling instance; none for a library's call.
    pub(crate) caller: Option<InstId>,
    pub(crate) line: u32,
    pub(crate) kind: SiteKind,
    /// The call whose callee makes this call, for the C library's call.
    pub(crate) parent: Option<SiteId>,
    /// The node holding the callee, for a call through a pointer.
    pub(crate) callee: Option<NodeId>,
    /// The function named, for a direct call.
    pub(crate) direct: Option<FnId>,
    pub(crate) args: Vec<Arg>,
    /// The library each parameter is given what it gives at the
    /// parameter's class by, for a call a library makes.
    pub(crate) given_by: Option<LibId>,
    pub(crate) dst: Dst,
    /// The type of the result the call keeps; `NONE` where it keeps none.
    pub(crate) result: TyId,
    pub(crate) bound: FxHashSet<Target>,
    /// Whether a callee may be memory that holds no function.
    pub(crate) data: bool,
    /// Whether a callee may be bytes from outside the process.
    pub(crate) anything: bool,
    /// The functions the C library keeps that the call runs, by when they
    /// were kept for.
    pub(crate) runs: Vec<Later>,
    /// For a call of a wrapper's whose objects all its analyses share:
    /// the wrapper and the call's block and statement.
    pub(crate) shared: Option<(DefId, BlockId, u32)>,
}

impl Site {
    fn new(caller: Option<InstId>, line: u32, kind: SiteKind) -> Site {
        Site {
            caller,
            line,
            kind,
            parent: None,
            callee: None,
            direct: None,
            args: Vec::new(),
            given_by: None,
            dst: Dst::None,
            result: NONE,
            bound: FxHashSet::default(),
            data: false,
            anything: false,
            runs: Vec::new(),
            shared: None,
        }
    }
}

/// A parameter as a definition receives it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Param {
    pub(crate) ty: TyId,
    pub(crate) width: u32,
    /// The size of a record passed by value, `TO_END` where not known.
    pub(crate) record: Option<u64>,
}

/// How a definition returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ret {
    Void,
    Scalar,
    /// A record, of the size where known.
    Record(u64),
}

/// A variable's place: a register (a value, its address never taken), by
/// its index among them, else memory, by its index among those with
/// `MEMORY` set.
pub(crate) const MEMORY: u32 = 1 << 31;

pub(crate) struct DefInfo {
    pub(crate) fn_id: FnId,
    pub(crate) hash: Hash,
    pub(crate) file: Arc<str>,
    pub(crate) params: Box<[Param]>,
    pub(crate) variadic: bool,
    pub(crate) slots: Box<[u32]>,
    /// Each memory variable's index and size.
    pub(crate) memory: Box<[(VarId, Option<u64>)]>,
    pub(crate) ret: Ret,
    pub(crate) ret_ty: TyId,
    pub(crate) unread: bool,
    /// The instance analysed for every call that is not one of its own.
    pub(crate) inst: InstId,
}

impl DefInfo {
    pub(crate) fn is_register(&self, var: VarId) -> bool {
        self.slots
            .get(var as usize)
            .is_some_and(|&slot| slot & MEMORY == 0)
    }
}

pub(crate) struct Inst {
    pub(crate) def: DefId,
    pub(crate) registers: NodeId,
    pub(crate) memory: ObjId,
    pub(crate) ret_node: NodeId,
    pub(crate) ret_obj: ObjId,
    pub(crate) varargs: ObjId,
}

/// A function returning only memory it allocates: its code, kept to be
/// analysed again for each call, its tag notes, and the calls whose
/// objects each analysis makes apart, by block and statement.
pub(crate) struct Wrapper {
    pub(crate) function: Arc<Function>,
    pub(crate) notes: Option<Arc<tags::Notes>>,
    pub(crate) apart: Apart,
}

/// The calls of a wrapper's whose objects each of its analyses makes
/// apart, by block and statement.
pub(crate) type Apart = Arc<FxHashSet<(BlockId, u32)>>;

/// The analysis' phases: while the constraints are generated, the reads
/// and writes under a tag wait for the tags' parts to be decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Generate,
    Solve,
}

/// A read or a write of a location under a tag, put off until the tags'
/// parts are decided.
#[derive(Debug, Clone, Copy)]
pub(crate) enum StaticOp {
    Read {
        loc: LocId,
        width: u32,
        ctx: TagSetId,
        dst: NodeId,
    },
    Write {
        loc: LocId,
        width: u32,
        part: TagSetId,
        src: NodeId,
    },
}

/// What a load is remembered by: the same read gives the same node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LoadKey {
    Node(NodeId, TrId, u32, TagSetId),
    Loc(LocId, u32, TagSetId),
}

/// Where a store the check of the tags reads writes: a location, or each
/// location a node points to, moved.
#[derive(Debug, Clone, Copy)]
pub(crate) enum StoreAt {
    Loc(LocId),
    Node(NodeId, TrId),
}

/// A store not through a member, of an integer narrow enough to be a
/// tag.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlainStore {
    pub(crate) at: StoreAt,
    pub(crate) width: u32,
    pub(crate) inst: InstId,
    pub(crate) line: u32,
}

/// Counts for the report.
#[derive(Default)]
pub(crate) struct Stats {
    /// The values the lowering could not give, by why.
    pub(crate) unknown: BTreeMap<Arc<str>, usize>,
    pub(crate) bodies: usize,
    pub(crate) body_bytes: u64,
    pub(crate) seconds: Vec<(&'static str, f64)>,
    pub(crate) peak_rss: u64,
    pub(crate) clones: usize,
    pub(crate) clones_refused: usize,
    pub(crate) wrappers: usize,
    pub(crate) accessors: usize,
    pub(crate) constant_returns: usize,
    /// The objects a write of a key's byte that is not paired may write,
    /// counted once for each key.
    pub(crate) unpaired_objects: usize,
    /// The functions that may write some key's byte, and those that may
    /// write any.
    pub(crate) key_writers: usize,
    pub(crate) any_key_writers: usize,
    pub(crate) pure: usize,
    pub(crate) nodes_generated: usize,
    /// The cells copies moved, at a constant offset and not.
    pub(crate) copy_cells: (usize, usize),
    /// The copies widened to the period of a cycle of copies they are in.
    pub(crate) widened_copies: usize,
    pub(crate) hvn: hvn::HvnStats,
    pub(crate) solve: solve::SolveStats,
}

pub(crate) struct Pta<'p> {
    pub(crate) program: &'p Program,
    pub(crate) libraries: &'p Libraries,
    pub(crate) types: Types<'p>,
    pub(crate) memory_cap: Option<u64>,
    /// The step running, for the report of the memory in use.
    pub(crate) during: &'static str,
    /// What was made since the memory in use was last read.
    pub(crate) growth: u32,
    /// Why the analysis stops: the memory in use passed the cap.
    pub(crate) over_cap: Option<String>,
    pub(crate) phase: Phase,
    pub(crate) sets: Sets,

    pub(crate) fn_names: Vec<FnName>,
    pub(crate) fn_ids: FxHashMap<FnName, FnId>,
    pub(crate) fn_defs: Vec<Vec<DefId>>,
    pub(crate) fn_locs: Vec<LocId>,
    pub(crate) address_taken: FxHashSet<FnId>,
    /// The functions whose calls write no memory but their own variables.
    pub(crate) pure: FxHashSet<FnId>,
    pub(crate) defs: Vec<DefInfo>,
    pub(crate) insts: Vec<Inst>,
    pub(crate) wrappers: FxHashMap<DefId, Wrapper>,
    pub(crate) clone_cap: usize,
    /// The wrappers being analysed again, innermost last.
    pub(crate) cloning: Vec<DefId>,
    pub(crate) survey: survey::Survey,

    pub(crate) global_ids: FxHashMap<GlobalName, GlobalId>,
    pub(crate) global_names: Vec<GlobalName>,
    pub(crate) global_locs: Vec<LocId>,

    pub(crate) objects: Vec<Object>,
    pub(crate) locs: Vec<(ObjId, Off)>,
    /// The bytes of the array each location's pointer keeps to.
    pub(crate) loc_windows: Vec<Window>,
    pub(crate) loc_ids: FxHashMap<(ObjId, Off, Window), LocId>,
    pub(crate) trs: Vec<Transform>,
    pub(crate) tr_ids: FxHashMap<Transform, TrId>,
    pub(crate) literal_loc: LocId,
    pub(crate) outside_loc: LocId,
    pub(crate) outside_node: NodeId,
    /// Where each object of a copy's own is made: the instance and the
    /// line.
    pub(crate) copy_sites: Vec<(Option<InstId>, u32)>,
    /// Where the constraints being made are.
    pub(crate) making_at: (Option<InstId>, u32),
    pub(crate) heaps: FxHashMap<SiteId, ObjId>,
    /// The objects of the calls of a wrapper's that all its analyses
    /// share.
    pub(crate) shared_heaps: FxHashMap<(DefId, BlockId, u32), ObjId>,
    /// The cells still to be moved by the copies of their object's bytes,
    /// each with the range of its object's copies out to move it by.
    pub(crate) mirror_work: Vec<(ObjId, Slot, u32, u32)>,
    pub(crate) draining: bool,
    /// The objects found read whose copies in are still to be started.
    pub(crate) demand_work: Vec<ObjId>,
    pub(crate) demanding: bool,
    /// How many cells copies will have moved at the next look for cycles
    /// of copies.
    pub(crate) widen_at: usize,

    pub(crate) rep: Vec<NodeId>,
    pub(crate) pts: Vec<SetId>,
    pub(crate) done: Vec<SetId>,
    pub(crate) succ: Vec<Vec<(NodeId, TrId)>>,
    /// The edges the successor lists hold, an edge in a list twice counted
    /// twice.
    pub(crate) edge_count: usize,
    /// How long each node's successor list was when last rid of repeats.
    pub(crate) succ_sorted: Vec<u32>,
    pub(crate) cons: FxHashMap<NodeId, Vec<Cons>>,
    /// Whether a node may gain values while the constraints are solved, so
    /// that it is not merged with the nodes holding the same offline.
    pub(crate) indirect: Vec<bool>,
    pub(crate) const_nodes: FxHashMap<LocId, NodeId>,
    pub(crate) shifted: FxHashMap<(NodeId, TrId), NodeId>,
    pub(crate) loads: FxHashMap<LoadKey, NodeId>,
    pub(crate) static_ops: Vec<StaticOp>,

    pub(crate) gep_memo: FxHashMap<(TrId, SetId), SetId>,
    pub(crate) rank: Vec<u32>,
    pub(crate) queued: Vec<bool>,
    pub(crate) next: Vec<NodeId>,
    pub(crate) lcd_checked: FxHashSet<(NodeId, NodeId)>,
    pub(crate) lcd: Vec<(NodeId, NodeId)>,
    /// The edges and locations at the last pass over the whole graph.
    pub(crate) pass_edges: usize,
    pub(crate) pass_locs: usize,
    /// The sets stored after the last time the unheld ones were dropped.
    pub(crate) live_sets: usize,

    pub(crate) sites: Vec<Site>,
    /// Each definition's calls of another, by the calls bound.
    pub(crate) call_edges: FxHashSet<(DefId, DefId)>,
    pub(crate) libs: FxHashMap<LibId, library::LibState>,
    pub(crate) classes: library::Classes,
    pub(crate) tys: Vec<Arc<str>>,
    pub(crate) ty_ids: FxHashMap<Arc<str>, TyId>,
    /// The functions without a source and not in the tables, each with
    /// its libraries and how many calls reach it.
    pub(crate) unmodelled: BTreeMap<FnName, (Vec<LibId>, usize)>,
    /// The nodes reading the members holding functions of the records
    /// whose functions the C library keeps, by when they run.
    pub(crate) kept_members: Vec<(Later, NodeId)>,

    pub(crate) tags: tags::Model,
    /// Each tag part or context: its key and values; 0 is none.
    pub(crate) tag_sets: Vec<(tags::KeyId, u64)>,
    pub(crate) tag_set_ids: FxHashMap<(tags::KeyId, u64), TagSetId>,
    /// Each tag set as a part and as a context once the keys are decided.
    pub(crate) part_final: Vec<TagSetId>,
    pub(crate) ctx_final: Vec<TagSetId>,
    pub(crate) variadic: variadic::Variadic,
    /// The writes of tags through members: where, and the tag's byte in
    /// the bytes written.
    pub(crate) tag_writes: Vec<(StoreAt, u32)>,
    pub(crate) plain_stores: Vec<PlainStore>,

    pub(crate) stats: Stats,
    /// How the objects asked about reach the nodes holding them.
    pub(crate) trace: Option<Box<trace::Trace>>,
    /// Where the large sets come from, where asked.
    pub(crate) blame: Option<Box<blame::Blame>>,
}

pub(crate) struct Analysis {
    pub(crate) report: String,
}

/// Runs the analysis over the program, its functions loaded from the
/// store as they are reached.
pub(crate) fn analyse(
    program: &Program,
    store: &Store,
    libraries: &Libraries,
    platform: Platform,
    memory_cap: Option<u64>,
    why: Asked,
) -> Result<Analysis, String> {
    let mut pta = Pta::new(program, libraries, platform, memory_cap);
    if let Some(path) = &why.blame {
        pta.blame = Some(Box::new(blame::Blame::new(path.clone())));
    }
    if !why.objects.is_empty() {
        pta.trace = Some(Box::new(trace::Trace::new(why)));
    }
    match pta.run(store) {
        Ok(components) => Ok(Analysis {
            report: pta.report(store, Some(&components)),
        }),
        Err(e) if pta.over_cap.is_some() => Err(format!("{}{e}", pta.report(store, None))),
        Err(e) => Err(e),
    }
}

impl<'p> Pta<'p> {
    fn new(
        program: &'p Program,
        libraries: &'p Libraries,
        platform: Platform,
        memory_cap: Option<u64>,
    ) -> Pta<'p> {
        let mut pta = Pta {
            program,
            libraries,
            types: Types::new(program, platform),
            memory_cap,
            during: "setup",
            growth: 0,
            over_cap: None,
            phase: Phase::Generate,
            sets: Sets::new(),
            fn_names: Vec::new(),
            fn_ids: FxHashMap::default(),
            fn_defs: Vec::new(),
            fn_locs: Vec::new(),
            address_taken: FxHashSet::default(),
            pure: FxHashSet::default(),
            defs: Vec::new(),
            insts: Vec::new(),
            wrappers: FxHashMap::default(),
            clone_cap: 400_000,
            cloning: Vec::new(),
            survey: survey::Survey::default(),
            global_ids: FxHashMap::default(),
            global_names: Vec::new(),
            global_locs: Vec::new(),
            objects: Vec::new(),
            locs: Vec::new(),
            loc_windows: Vec::new(),
            loc_ids: FxHashMap::default(),
            trs: Vec::new(),
            tr_ids: FxHashMap::default(),
            literal_loc: NONE,
            outside_loc: NONE,
            outside_node: NONE,
            copy_sites: Vec::new(),
            making_at: (None, 0),
            heaps: FxHashMap::default(),
            shared_heaps: FxHashMap::default(),
            mirror_work: Vec::new(),
            draining: false,
            demand_work: Vec::new(),
            demanding: false,
            widen_at: memory::WIDEN_EVERY,
            rep: Vec::new(),
            pts: Vec::new(),
            done: Vec::new(),
            succ: Vec::new(),
            edge_count: 0,
            succ_sorted: Vec::new(),
            cons: FxHashMap::default(),
            indirect: Vec::new(),
            const_nodes: FxHashMap::default(),
            shifted: FxHashMap::default(),
            loads: FxHashMap::default(),
            static_ops: Vec::new(),
            gep_memo: FxHashMap::default(),
            rank: Vec::new(),
            queued: Vec::new(),
            next: Vec::new(),
            lcd_checked: FxHashSet::default(),
            lcd: Vec::new(),
            pass_edges: 0,
            pass_locs: 0,
            live_sets: 0,
            sites: Vec::new(),
            call_edges: FxHashSet::default(),
            libs: FxHashMap::default(),
            classes: library::Classes::default(),
            tys: Vec::new(),
            ty_ids: FxHashMap::default(),
            unmodelled: BTreeMap::new(),
            kept_members: Vec::new(),
            tags: tags::Model::default(),
            tag_sets: vec![(0, 0)],
            tag_set_ids: FxHashMap::default(),
            part_final: Vec::new(),
            ctx_final: Vec::new(),
            variadic: variadic::Variadic::default(),
            tag_writes: Vec::new(),
            plain_stores: Vec::new(),
            stats: Stats::default(),
            trace: None,
            blame: None,
        };
        let identity = pta.tr(Transform::IDENTITY);
        let any = pta.tr(Transform::ANY);
        debug_assert_eq!((identity, any), (COPY, TO_ANY));
        let literal = pta.object(ObjKind::Literal, None);
        pta.literal_loc = pta.loc(literal, Off::Any);
        // The bytes from outside hold pointers to outside.
        let outside = pta.object(ObjKind::Outside, None);
        pta.outside_loc = pta.loc(outside, Off::Any);
        pta.outside_node = pta.const_node(pta.outside_loc);
        let cell = pta.cell(outside, Off::Any, 0, 8, FULL);
        pta.add_edge(pta.outside_node, cell, COPY);
        pta
    }

    fn run(&mut self, store: &Store) -> Result<report::Components, String> {
        self.run_step("survey", |pta| pta.survey(store))?;
        self.run_step("prepare", |pta| {
            pta.prepare();
            Ok(())
        })?;
        self.run_step("generate", |pta| pta.generate(store))?;
        self.run_step("tags", |pta| {
            pta.decide_parts();
            pta.run_static_ops();
            Ok(())
        })?;
        self.run_step("offline merge", |pta| {
            pta.hvn();
            Ok(())
        })?;
        self.run_step("solve", Pta::solve)?;
        let mut components = None;
        self.run_step("components", |pta| {
            components = Some(pta.components(store)?);
            Ok(())
        })?;
        components.ok_or_else(|| "points-to: no components".to_owned())
    }

    fn run_step(
        &mut self,
        what: &'static str,
        run: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<(), String> {
        self.during = what;
        let start = Instant::now();
        let result = run(self);
        self.stats
            .seconds
            .push((what, start.elapsed().as_secs_f64()));
        result?;
        self.check_memory()
    }

    /// Notes that something was made, the memory in use read now and then.
    pub(crate) fn grew(&mut self) {
        self.growth += 1;
        if self.growth >= 1 << 14 {
            self.growth = 0;
            self.watch_memory();
        }
    }

    fn watch_memory(&mut self) {
        let (current, peak) = memory_use();
        self.stats.peak_rss = self.stats.peak_rss.max(peak);
        if let Some(cap) = self.memory_cap
            && current > cap
            && self.over_cap.is_none()
        {
            self.over_cap = Some(format!(
                "points-to: {} MiB in use during {}, past the cap of {} MiB",
                current >> 20,
                self.during,
                cap >> 20
            ));
        }
    }

    /// Stops with an error where the process has taken more memory than
    /// the cap allows.
    pub(crate) fn check_memory(&mut self) -> Result<(), String> {
        self.watch_memory();
        match &self.over_cap {
            Some(why) => Err(why.clone()),
            None => Ok(()),
        }
    }

    // Interning.

    pub(crate) fn tr(&mut self, transform: Transform) -> TrId {
        if let Some(&id) = self.tr_ids.get(&transform) {
            return id;
        }
        let id = self.trs.len() as TrId;
        self.trs.push(transform);
        self.tr_ids.insert(transform, id);
        id
    }

    pub(crate) fn object(&mut self, kind: ObjKind, size: Option<u64>) -> ObjId {
        let id = self.objects.len() as ObjId;
        self.objects.push(Object {
            kind,
            size,
            memory: None,
        });
        id
    }

    /// The location at an offset into an object, its offset kept within
    /// reach: a function is one location, the string literals any offset.
    pub(crate) fn loc(&mut self, obj: ObjId, off: Off) -> LocId {
        self.loc_in(obj, off, FULL)
    }

    /// The location at an offset into an object of a pointer keeping to
    /// the array in a window: none where the window holds all of the
    /// object or the offset is lost.
    pub(crate) fn loc_in(&mut self, obj: ObjId, off: Off, window: Window) -> LocId {
        let object = &self.objects[obj as usize];
        let off = match object.kind {
            ObjKind::Function(_) | ObjKind::Library(_) => Off::Exact(0),
            ObjKind::Literal => Off::Any,
            _ => normalize(off, object.size),
        };
        let whole = match object.size {
            Some(size) => window.0 <= 0 && window.1 >= size.min(i64::MAX as u64) as i64,
            None => window.0 <= 0 && window.1 == i64::MAX,
        };
        let window = if whole || off == Off::Any || object.is_constant() {
            FULL
        } else {
            window
        };
        if let Some(&id) = self.loc_ids.get(&(obj, off, window)) {
            return id;
        }
        let id = self.locs.len() as LocId;
        self.locs.push((obj, off));
        self.loc_windows.push(window);
        self.loc_ids.insert((obj, off, window), id);
        self.grew();
        self.trace_loc(id);
        id
    }

    /// A location moved, in the array it keeps to.
    pub(crate) fn moved_loc(&mut self, loc: LocId, tr: Transform) -> LocId {
        let (obj, off) = self.locs[loc as usize];
        let (off, window) = tr.apply_in(off, self.loc_windows[loc as usize]);
        self.loc_in(obj, off, window)
    }

    pub(crate) fn tag_set(&mut self, key: tags::KeyId, values: u64) -> TagSetId {
        if let Some(&id) = self.tag_set_ids.get(&(key, values)) {
            return id;
        }
        let id = self.tag_sets.len() as TagSetId;
        self.tag_sets.push((key, values));
        self.tag_set_ids.insert((key, values), id);
        if !self.part_final.is_empty() {
            let (part, ctx) = self.finals(id);
            self.part_final.push(part);
            self.ctx_final.push(ctx);
        }
        id
    }

    pub(crate) fn fn_id(&mut self, name: &FnName) -> FnId {
        if let Some(&id) = self.fn_ids.get(name) {
            return id;
        }
        let id = self.fn_names.len() as FnId;
        self.fn_names.push(name.clone());
        self.fn_ids.insert(name.clone(), id);
        self.fn_defs.push(Vec::new());
        self.fn_locs.push(NONE);
        id
    }

    /// The location of a function's code.
    pub(crate) fn fn_loc(&mut self, f: FnId) -> LocId {
        if self.fn_locs[f as usize] != NONE {
            return self.fn_locs[f as usize];
        }
        let obj = self.object(ObjKind::Function(f), None);
        let loc = self.loc(obj, Off::Exact(0));
        self.fn_locs[f as usize] = loc;
        loc
    }

    pub(crate) fn global_id(&mut self, name: &GlobalName) -> GlobalId {
        if let Some(&id) = self.global_ids.get(name) {
            return id;
        }
        let id = self.global_names.len() as GlobalId;
        self.global_names.push(name.clone());
        self.global_ids.insert(name.clone(), id);
        self.global_locs.push(NONE);
        id
    }

    /// The location of a global. A global a library declares and the
    /// program does not define is the library's: it reads and writes it
    /// at its type.
    pub(crate) fn global_loc(&mut self, name: &GlobalName) -> LocId {
        let id = self.global_id(name);
        if self.global_locs[id as usize] != NONE {
            return self.global_locs[id as usize];
        }
        let program = self.program;
        let ty = match program.globals.get(name).and_then(|g| g.first()) {
            Some(global) => Some(global.ty.clone()),
            None => program.declared_globals.get(name).map(|(_, ty)| ty.clone()),
        };
        let size = ty.as_ref().and_then(|ty| self.types.size(ty));
        let obj = self.object(ObjKind::Global(id), size);
        let loc = self.loc(obj, Off::Exact(0));
        self.global_locs[id as usize] = loc;
        if !program.globals.contains_key(name)
            && !program.tentative.contains(name)
            && let Some(ty) = &ty
        {
            let libraries = match self.libraries.globals.get(name) {
                Some(libraries) => libraries.clone(),
                None => vec![self.libraries.unknown],
            };
            for library in libraries {
                self.lib_global(library, loc, ty);
            }
        }
        loc
    }

    // Nodes.

    pub(crate) fn new_node(&mut self, indirect: bool) -> NodeId {
        self.grew();
        let id = self.rep.len() as NodeId;
        self.rep.push(id);
        self.pts.push(EMPTY);
        self.done.push(EMPTY);
        self.succ.push(Vec::new());
        self.succ_sorted.push(0);
        self.indirect.push(indirect);
        if self.phase == Phase::Solve {
            self.rank.push(u32::MAX);
            self.queued.push(false);
        }
        id
    }

    pub(crate) fn find(&mut self, mut node: NodeId) -> NodeId {
        let mut root = node;
        while self.rep[root as usize] != root {
            root = self.rep[root as usize];
        }
        while self.rep[node as usize] != root {
            let parent = self.rep[node as usize];
            self.rep[node as usize] = root;
            node = parent;
        }
        root
    }

    pub(crate) fn find_const(&self, mut node: NodeId) -> NodeId {
        while self.rep[node as usize] != node {
            node = self.rep[node as usize];
        }
        node
    }

    /// A node holding exactly a pointer to `loc`.
    pub(crate) fn const_node(&mut self, loc: LocId) -> NodeId {
        if let Some(&node) = self.const_nodes.get(&loc) {
            return node;
        }
        let node = self.new_node(false);
        self.add_initial(node, loc);
        self.const_nodes.insert(loc, node);
        node
    }

    /// The locations a node may point to.
    pub(crate) fn points_to(&self, node: NodeId) -> Vec<LocId> {
        self.sets.elements(self.pts[self.find_const(node) as usize])
    }

    /// The functions a node may point to.
    pub(crate) fn functions(&self, node: NodeId) -> Vec<FnId> {
        let mut functions: Vec<FnId> = self
            .points_to(node)
            .into_iter()
            .filter_map(
                |loc| match self.objects[self.locs[loc as usize].0 as usize].kind {
                    ObjKind::Function(f) => Some(f),
                    _ => None,
                },
            )
            .collect();
        functions.sort_unstable();
        functions.dedup();
        functions
    }

    pub(crate) fn new_site(&mut self, site: Site) -> SiteId {
        let id = self.sites.len() as SiteId;
        self.sites.push(site);
        id
    }

    pub(crate) fn library_name(&self, library: LibId) -> &str {
        &self.libraries.names[library as usize]
    }
}

/// The process's resident memory now and at its peak, in bytes.
pub(crate) fn memory_use() -> (u64, u64) {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return (0, 0);
    };
    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.trim().strip_suffix("kB"))
            .and_then(|kb| kb.trim().parse::<u64>().ok())
            .map_or(0, |kb| kb << 10)
    };
    (field("VmRSS:"), field("VmHWM:"))
}
