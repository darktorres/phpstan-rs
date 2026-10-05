//! Each object's bytes: the cells its stores write, the readers its loads
//! read, and the copies of other objects' bytes into it.
//!
//! A reader takes each cell its bytes overlap and the cell's part lets it
//! see: whole where they start at the same offset and are as wide, else
//! at any offset of the cell's objects. A cell at any offset is read by
//! every reader through one node, and a reader at any offset reads every
//! cell through another, so that neither takes an edge from each of the
//! other side.
//!
//! A copy moves the cells of its source into its destination: each cell
//! of the bytes the copy took (its window) is a cell of the destination
//! where the copy puts it, cut to the window, and moves on through the
//! destination's own copies. A cell at a stride or any offset keeps the
//! window, moved, and reaches only the readers of the bytes the copy
//! wrote: those after a constant offset, or the array a pointer at a
//! stride keeps to. A store or a load through a pointer keeping to an
//! array writes or reads at its stride only in the array. A copy costs an edge for each cell of the source, not for each
//! reader of the destination. A copy to any offset that does not keep the
//! parts is a cell at any offset under no part: every reader of the
//! destination reads it through the one node, from one reader of the
//! source's bytes in the window.
//!
//! The copies into an object move cells only once something reads it: a
//! reader of its bytes, or a copy out of it into an object something
//! reads. Until then they wait, and an object nothing reads holds only
//! the cells its stores write.
//!
//! A write of a key's byte that is not paired leaves the values it writes
//! in every part of the key in the objects it may write: a reader of the
//! object under a context holding one of them sees the key's cells
//! whatever their part, and a copy out of it moves the cells with the
//! values added to their parts.

use std::collections::BTreeMap;

use rustc_hash::{FxHashMap, FxHashSet};

use super::solve::strong_components;
use super::tags::{KeyId, VARIADIC};
use super::{
    COPY, LocId, NONE, NodeId, ObjId, Off, Pta, TO_ANY, TO_END, TagSetId, TrId, gcd, normalize,
    off_add, off_parts, off_sub, stride_off,
};

/// How many cells copies move between two looks for the cycles of copies
/// that move bytes round them.
pub(crate) const WIDEN_EVERY: usize = 1 << 16;

/// Bytes from the first offset up to the second.
pub(crate) type Window = (i64, i64);

/// The window of a cell a store writes, of a reader a load reads, and of
/// every cell and reader at a constant offset.
pub(crate) const FULL: Window = (i64::MIN, i64::MAX);

/// A copy of an object's bytes into another's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Mirror {
    pub(crate) src: ObjId,
    pub(crate) src_off: Off,
    /// The array the source's pointer keeps to.
    pub(crate) src_window: Window,
    /// How many bytes it copies, `TO_END` for all past `src_off`.
    pub(crate) len: u64,
    pub(crate) dst_off: Off,
    /// The array the destination's pointer keeps to.
    pub(crate) dst_window: Window,
    /// Whether the bytes keep their parts whatever the bytes copied: the
    /// tag is copied with them.
    pub(crate) preserve: bool,
}

/// A cell or a reader: where, under which part or context, how wide, its
/// node, and the bytes it is bound to: those whose cells a reader of a
/// copy's source reads, or those a cell a copy moved reaches.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Slot {
    pub(crate) off: Off,
    pub(crate) tag: TagSetId,
    pub(crate) width: u32,
    pub(crate) node: NodeId,
    pub(crate) window: Window,
}

pub(crate) struct Memory {
    pub(crate) exact_cells: BTreeMap<(i64, TagSetId, u32), NodeId>,
    pub(crate) loose_cells: Vec<Slot>,
    /// The cells at a stride or any offset, by where, their part, their
    /// width and their window.
    loose_cell_ids: FxHashMap<(Off, TagSetId, u32, Window), NodeId>,
    pub(crate) exact_readers: BTreeMap<(i64, TagSetId, u32), NodeId>,
    pub(crate) loose_readers: Vec<Slot>,
    /// The readers at a stride or any offset, by where, their context,
    /// their width and their window.
    loose_reader_ids: FxHashMap<(Off, TagSetId, u32, Window), NodeId>,
    /// The copies of other objects' bytes into this one, and of its own.
    pub(crate) mirrors: Vec<Mirror>,
    mirror_set: FxHashSet<Mirror>,
    /// The copies of this object's bytes that move its cells, each by its
    /// destination and its place among the destination's copies.
    copies_out: Vec<(ObjId, u32)>,
    /// Whether anything reads its cells: a reader, or a copy out of it
    /// into an object something reads. The copies into it move their
    /// sources' cells from then on.
    pub(crate) read: bool,
    /// The widest cell and reader at a constant offset.
    max_cell: u32,
    max_reader: u32,
    /// Every cell's pointers at any offset, for the readers at any offset
    /// under no context.
    pub(crate) any_out: NodeId,
    /// The pointers of the cells at any offset under no part, for every
    /// reader.
    pub(crate) any_in: NodeId,
    /// The values the writes of each key's byte that are not paired
    /// leave in every part of the key here.
    pub(crate) unpaired: Vec<(KeyId, u64)>,
}

impl Memory {
    fn new() -> Memory {
        Memory {
            exact_cells: BTreeMap::new(),
            loose_cells: Vec::new(),
            loose_cell_ids: FxHashMap::default(),
            exact_readers: BTreeMap::new(),
            loose_readers: Vec::new(),
            loose_reader_ids: FxHashMap::default(),
            mirrors: Vec::new(),
            mirror_set: FxHashSet::default(),
            copies_out: Vec::new(),
            read: false,
            max_cell: 0,
            max_reader: 0,
            any_out: NONE,
            any_in: NONE,
            unpaired: Vec::new(),
        }
    }

    pub(crate) fn cell_count(&self) -> usize {
        self.exact_cells.len() + self.loose_cells.len()
    }

    pub(crate) fn reader_count(&self) -> usize {
        self.exact_readers.len() + self.loose_readers.len()
    }

    pub(crate) fn cells(&self) -> Vec<Slot> {
        let mut cells: Vec<Slot> = self
            .exact_cells
            .iter()
            .map(|(&(at, tag, width), &node)| exact_slot(at, tag, width, node))
            .collect();
        cells.extend_from_slice(&self.loose_cells);
        cells
    }

    fn readers(&self) -> Vec<Slot> {
        let mut readers: Vec<Slot> = self
            .exact_readers
            .iter()
            .map(|(&(at, tag, width), &node)| exact_slot(at, tag, width, node))
            .collect();
        readers.extend_from_slice(&self.loose_readers);
        readers
    }
}

fn exact_slot(at: i64, tag: TagSetId, width: u32, node: NodeId) -> Slot {
    Slot {
        off: Off::Exact(at),
        tag,
        width,
        node,
        window: FULL,
    }
}

/// Whether a slot is at any offset under no part or context, over all of
/// its object.
fn is_any_untagged(slot: &Slot) -> bool {
    slot.off == Off::Any && slot.tag == 0 && slot.window == FULL
}

/// Whether a copy lands at any offset of its destination and does not
/// keep the parts: a cell at any offset under no part.
fn lands_anywhere(mirror: &Mirror) -> bool {
    !mirror.preserve && off_sub(mirror.dst_off, mirror.src_off) == Off::Any
}

/// The bytes of its source a copy takes.
fn taken(mirror: &Mirror) -> Window {
    span(mirror.src_off, mirror.src_window, mirror.len)
}

/// The bytes of its destination a copy writes.
fn placed(mirror: &Mirror) -> Window {
    span(mirror.dst_off, mirror.dst_window, mirror.len)
}

/// The bytes from a constant offset as long as a copy, or else the array
/// its pointer keeps to.
fn span(off: Off, window: Window, len: u64) -> Window {
    match off {
        Off::Exact(start) => {
            let end = if len == TO_END {
                i64::MAX
            } else {
                start.saturating_add(len.min(i64::MAX as u64) as i64)
            };
            (start, end)
        }
        _ => window,
    }
}

/// The bytes two windows share, empty where they share none.
pub(crate) fn meet(a: Window, b: Window) -> Window {
    (a.0.max(b.0), a.1.min(b.1))
}

/// A window over an object's bytes, or every offset where it holds all of
/// them: a window over all of the bytes reaches past their ends too.
fn bound(window: Window, size: Option<u64>) -> Window {
    match size {
        Some(size) if window.0 <= 0 && window.1 >= size.min(i64::MAX as u64) as i64 => FULL,
        _ => window,
    }
}

/// The constant offsets of the cells or readers from one offset to
/// another, in a map by offset.
fn between(
    map: &BTreeMap<(i64, TagSetId, u32), NodeId>,
    low: i64,
    high: i64,
) -> impl Iterator<Item = (&(i64, TagSetId, u32), &NodeId)> {
    let range = (low <= high).then(|| map.range((low, 0, 0)..=(high, u32::MAX, u32::MAX)));
    range.into_iter().flatten()
}

impl Pta<'_> {
    fn memory_mut(&mut self, obj: ObjId) -> &mut Memory {
        self.objects[obj as usize]
            .memory
            .get_or_insert_with(|| Box::new(Memory::new()))
    }

    /// Whether a reader under a context sees a cell written under a part:
    /// unless both are of one key and share no value.
    pub(crate) fn visible(&self, part: TagSetId, ctx: TagSetId) -> bool {
        if part == 0 || ctx == 0 || part == ctx {
            return true;
        }
        let (part_key, part_values) = self.tag_sets[part as usize];
        let (ctx_key, ctx_values) = self.tag_sets[ctx as usize];
        part_key != ctx_key || part_values & ctx_values != 0
    }

    /// Whether a reader of an object under a context sees a cell written
    /// under a part: also where the context holds a value the writes of
    /// the key's byte that are not paired leave in the object.
    fn visible_in(&self, obj: ObjId, part: TagSetId, ctx: TagSetId) -> bool {
        if self.visible(part, ctx) {
            return true;
        }
        let (key, values) = self.tag_sets[ctx as usize];
        values & self.unpaired_in(obj, key) != 0
    }

    /// The values the writes of a key's byte that are not paired leave in
    /// an object.
    fn unpaired_in(&self, obj: ObjId, key: KeyId) -> u64 {
        self.objects[obj as usize]
            .memory
            .as_deref()
            .and_then(|memory| memory.unpaired.iter().find(|&&(k, _)| k == key))
            .map_or(0, |&(_, values)| values)
    }

    /// Leaves values of a key in every part of it in an object: its
    /// readers under a context holding one now see the cells of the key
    /// their contexts hid, and its copies out move those cells again, the
    /// values added to their parts.
    pub(crate) fn leave_unpaired(&mut self, obj: ObjId, key: KeyId, values: u64) {
        if !self.objects[obj as usize].takes_writes() {
            return;
        }
        let old = self.unpaired_in(obj, key);
        if old | values == old {
            return;
        }
        let memory = self.memory_mut(obj);
        match memory.unpaired.iter_mut().find(|(k, _)| *k == key) {
            Some((_, known)) => *known |= values,
            None => memory.unpaired.push((key, values)),
        }
        if old == 0 {
            self.stats.unpaired_objects += 1;
        }
        let new = old | values;
        let of_key = |slot: &Slot| slot.tag != 0 && self.tag_sets[slot.tag as usize].0 == key;
        let Some(memory) = self.objects[obj as usize].memory.as_deref() else {
            return;
        };
        let cells: Vec<Slot> = memory
            .cells()
            .into_iter()
            .filter(|cell| of_key(cell))
            .collect();
        if cells.is_empty() {
            return;
        }
        let readers: Vec<Slot> = memory
            .readers()
            .into_iter()
            .filter(|reader| {
                let values = self.tag_sets[reader.tag as usize].1;
                of_key(reader) && values & old == 0 && values & new != 0
            })
            .collect();
        let copies_out = memory.copies_out.len() as u32;
        let mut links = Vec::new();
        for cell in &cells {
            for reader in &readers {
                if self.visible(cell.tag, reader.tag) {
                    continue;
                }
                if let Some(tr) = self.relate(obj, reader, cell) {
                    links.push((cell.node, reader.node, tr));
                }
            }
        }
        for (cell, reader, tr) in links {
            self.add_edge(cell, reader, tr);
        }
        if copies_out > 0 {
            for cell in cells {
                self.mirror_work.push((obj, cell, 0, copies_out));
            }
            self.drain_mirrors();
        }
    }

    /// How a cell's pointers reach a reader: whole, at any offset, or not
    /// at all where their bytes do not overlap, the cell is outside the
    /// reader's window, the reader outside the cell's, or the cell is
    /// hidden. Bytes a window cuts are read at any offset.
    fn relate(&self, obj: ObjId, reader: &Slot, cell: &Slot) -> Option<TrId> {
        if !self.visible_in(obj, cell.tag, reader.tag) {
            return None;
        }
        let shared = meet(reader.window, cell.window);
        if shared.0 >= shared.1 {
            return None;
        }
        let mut cut = false;
        if reader.window != FULL
            && let Off::Exact(at) = cell.off
        {
            let end = at.saturating_add(i64::from(cell.width));
            if end <= reader.window.0 || at >= reader.window.1 {
                return None;
            }
            cut = at < reader.window.0 || end > reader.window.1;
        }
        let (mut reader_off, mut reader_width) = (reader.off, i64::from(reader.width));
        if cell.window != FULL
            && let Off::Exact(at) = reader.off
        {
            let end = at.saturating_add(reader_width);
            let (low, high) = (at.max(cell.window.0), end.min(cell.window.1));
            if low >= high {
                return None;
            }
            if low != at || high != end {
                cut = true;
                (reader_off, reader_width) = (Off::Exact(low), high - low);
            }
        }
        let cell_width = i64::from(cell.width);
        let tr = match off_sub(cell.off, reader_off) {
            Off::Any => TO_ANY,
            Off::Exact(d) => {
                if d <= -cell_width || d >= reader_width {
                    return None;
                }
                if d == 0 && reader_width == cell_width {
                    COPY
                } else {
                    TO_ANY
                }
            }
            Off::Stride { rem, stride } => {
                let (d, stride) = (i64::from(rem), i64::from(stride));
                if d >= reader_width && stride - d >= cell_width {
                    return None;
                }
                if d == 0 && reader_width == cell_width && stride >= reader_width {
                    COPY
                } else {
                    TO_ANY
                }
            }
        };
        Some(if cut { TO_ANY } else { tr })
    }

    /// The node of the cell an object's bytes at an offset hold, written
    /// under a part, made where new: it is read by the readers it
    /// overlaps.
    /// A cell at a stride or any offset is written only in the array its
    /// pointer keeps to.
    pub(crate) fn cell(
        &mut self,
        obj: ObjId,
        off: Off,
        part: TagSetId,
        width: u32,
        window: Window,
    ) -> NodeId {
        let off = normalize(off, self.objects[obj as usize].size);
        self.make_cell(obj, off, part, width.max(1), window)
    }

    /// A cell made where new: it reaches the readers it overlaps in its
    /// window, and moves on through the object's copies.
    fn make_cell(
        &mut self,
        obj: ObjId,
        off: Off,
        part: TagSetId,
        width: u32,
        window: Window,
    ) -> NodeId {
        let window = match off {
            Off::Exact(_) => FULL,
            _ => bound(window, self.objects[obj as usize].size),
        };
        let memory = self.memory_mut(obj);
        let found = match off {
            Off::Exact(at) => memory.exact_cells.get(&(at, part, width)).copied(),
            _ => memory
                .loose_cell_ids
                .get(&(off, part, width, window))
                .copied(),
        };
        if let Some(node) = found {
            return node;
        }
        let node = self.new_node(true);
        if self.draining {
            let made = &mut self.stats.copy_cells;
            if matches!(off, Off::Exact(_)) {
                made.0 += 1;
            } else {
                made.1 += 1;
            }
        }
        let cell = Slot {
            off,
            tag: part,
            width,
            node,
            window,
        };
        let memory = self.memory_mut(obj);
        match off {
            Off::Exact(at) => {
                memory.exact_cells.insert((at, part, width), node);
                memory.max_cell = memory.max_cell.max(width);
            }
            _ => {
                memory.loose_cells.push(cell);
                memory
                    .loose_cell_ids
                    .insert((off, part, width, window), node);
            }
        }
        let (any_out, any_in) = (memory.any_out, memory.any_in);
        let copies_out = memory.copies_out.len();
        let mut links = Vec::new();
        self.cell_links(obj, &cell, &mut links);
        for (reader, tr) in links {
            self.add_edge(node, reader, tr);
        }
        if any_out != NONE {
            self.add_edge(node, any_out, TO_ANY);
        }
        if is_any_untagged(&cell) {
            let any_in = if any_in == NONE {
                self.make_any_in(obj)
            } else {
                any_in
            };
            self.add_edge(node, any_in, COPY);
        }
        if copies_out > 0 {
            self.mirror_work.push((obj, cell, 0, copies_out as u32));
            self.drain_mirrors();
        }
        node
    }

    /// The readers a new cell reaches, besides the ones the summaries
    /// serve.
    fn cell_links(&self, obj: ObjId, cell: &Slot, out: &mut Vec<(NodeId, TrId)>) {
        let Some(memory) = self.objects[obj as usize].memory.as_deref() else {
            return;
        };
        if is_any_untagged(cell) {
            return;
        }
        let reach = i64::from(memory.max_reader) - 1;
        let (low, high) = match cell.off {
            Off::Exact(at) => (at - reach, at + i64::from(cell.width) - 1),
            _ if cell.window == FULL => (i64::MIN, i64::MAX),
            _ => (
                cell.window.0.saturating_sub(reach),
                cell.window.1.saturating_sub(1),
            ),
        };
        for (&(at, ctx, width), &node) in between(&memory.exact_readers, low, high) {
            if let Some(tr) = self.relate(obj, &exact_slot(at, ctx, width, node), cell) {
                out.push((node, tr));
            }
        }
        for reader in &memory.loose_readers {
            if is_any_untagged(reader) {
                continue;
            }
            if let Some(tr) = self.relate(obj, reader, cell) {
                out.push((reader.node, tr));
            }
        }
    }

    /// The node of the reader of an object's bytes at an offset under a
    /// context, made where new: at a stride or any offset, only in the
    /// array its pointer keeps to.
    pub(crate) fn reader(
        &mut self,
        obj: ObjId,
        off: Off,
        ctx: TagSetId,
        width: u32,
        window: Window,
    ) -> NodeId {
        let size = self.objects[obj as usize].size;
        let off = normalize(off, size);
        let window = match off {
            Off::Exact(_) => FULL,
            _ => bound(window, size),
        };
        self.make_reader(obj, off, ctx, width.max(1), window)
    }

    /// The reader of an object's bytes in a window, made where new: at a
    /// constant offset, one of the bytes it shares with the window, read
    /// at any offset where those are not all of its bytes. None where it
    /// shares none.
    fn window_reader(
        &mut self,
        obj: ObjId,
        off: Off,
        ctx: TagSetId,
        width: u32,
        window: Window,
    ) -> Option<(NodeId, TrId)> {
        let size = self.objects[obj as usize].size;
        let off = normalize(off, size);
        let width = width.max(1);
        let window = bound(window, size);
        match off {
            Off::Exact(at) if window != FULL => {
                let end = at.saturating_add(i64::from(width));
                let (low, high) = (at.max(window.0), end.min(window.1));
                if low >= high {
                    return None;
                }
                let tr = if low == at && high == end {
                    COPY
                } else {
                    TO_ANY
                };
                let width = (high - low).min(i64::from(u32::MAX)) as u32;
                Some((self.make_reader(obj, Off::Exact(low), ctx, width, FULL), tr))
            }
            _ => Some((self.make_reader(obj, off, ctx, width, window), COPY)),
        }
    }

    /// A reader made where new: it reads the cells it overlaps. A reader
    /// at any offset takes every cell at any offset, whatever its width.
    fn make_reader(
        &mut self,
        obj: ObjId,
        off: Off,
        ctx: TagSetId,
        width: u32,
        window: Window,
    ) -> NodeId {
        let width = if off == Off::Any { 1 } else { width };
        let memory = self.memory_mut(obj);
        let found = match off {
            Off::Exact(at) => memory.exact_readers.get(&(at, ctx, width)).copied(),
            _ => memory
                .loose_reader_ids
                .get(&(off, ctx, width, window))
                .copied(),
        };
        if let Some(node) = found {
            return node;
        }
        let node = self.new_node(true);
        let reader = Slot {
            off,
            tag: ctx,
            width,
            node,
            window,
        };
        let memory = self.memory_mut(obj);
        match off {
            Off::Exact(at) => {
                memory.exact_readers.insert((at, ctx, width), node);
                memory.max_reader = memory.max_reader.max(width);
            }
            _ => {
                memory.loose_readers.push(reader);
                memory
                    .loose_reader_ids
                    .insert((off, ctx, width, window), node);
            }
        }
        let any_in = memory.any_in;
        if is_any_untagged(&reader) {
            let any_out = self.any_out(obj);
            self.add_edge(any_out, node, COPY);
        } else {
            let mut links = Vec::new();
            self.reader_links(obj, &reader, &mut links);
            for (cell, tr) in links {
                self.add_edge(cell, node, tr);
            }
        }
        if any_in != NONE {
            self.add_edge(any_in, node, TO_ANY);
        }
        self.demand(obj);
        node
    }

    /// The cells a new reader reads, besides the ones the summaries serve.
    fn reader_links(&self, obj: ObjId, reader: &Slot, out: &mut Vec<(NodeId, TrId)>) {
        let Some(memory) = self.objects[obj as usize].memory.as_deref() else {
            return;
        };
        let reach = i64::from(memory.max_cell) - 1;
        let (low, high) = match reader.off {
            Off::Exact(at) => (at - reach, at + i64::from(reader.width) - 1),
            _ if reader.window == FULL => (i64::MIN, i64::MAX),
            _ => (
                reader.window.0.saturating_sub(reach),
                reader.window.1.saturating_sub(1),
            ),
        };
        for (&(at, part, width), &node) in between(&memory.exact_cells, low, high) {
            if let Some(tr) = self.relate(obj, reader, &exact_slot(at, part, width, node)) {
                out.push((node, tr));
            }
        }
        for cell in &memory.loose_cells {
            if is_any_untagged(cell) {
                continue;
            }
            if let Some(tr) = self.relate(obj, reader, cell) {
                out.push((cell.node, tr));
            }
        }
    }

    /// The node holding every cell's pointers at any offset, made where
    /// new.
    pub(crate) fn any_out(&mut self, obj: ObjId) -> NodeId {
        let any_out = self.memory_mut(obj).any_out;
        if any_out != NONE {
            return any_out;
        }
        let node = self.new_node(true);
        let memory = self.memory_mut(obj);
        memory.any_out = node;
        for cell in memory.cells() {
            self.add_edge(cell.node, node, TO_ANY);
        }
        self.demand(obj);
        node
    }

    fn make_any_in(&mut self, obj: ObjId) -> NodeId {
        let node = self.new_node(true);
        let memory = self.memory_mut(obj);
        memory.any_in = node;
        for reader in memory.readers() {
            self.add_edge(node, reader.node, TO_ANY);
        }
        node
    }

    /// Copies an object's bytes into another's: each cell of the source,
    /// now and as they come, moves into the destination once something
    /// reads the destination. A copy back into
    /// an object that copies into this one moves the bytes round a cycle,
    /// each way round as far again: it copies them to every offset a
    /// multiple of that distance away.
    pub(crate) fn add_mirror(&mut self, dst: ObjId, mut mirror: Mirror) {
        let src = mirror.src;
        if self.objects[src as usize].is_constant() || !self.objects[dst as usize].takes_writes() {
            return;
        }
        let shift = off_sub(mirror.dst_off, mirror.src_off);
        if dst != src
            && let Some(memory) = self.objects[src as usize].memory.as_deref()
        {
            for back in memory.mirrors.iter().filter(|back| back.src == dst) {
                let period = match off_add(off_sub(back.dst_off, back.src_off), shift) {
                    Off::Exact(0) => continue,
                    Off::Exact(round) => round.unsigned_abs(),
                    Off::Stride { rem, stride } => gcd(u64::from(rem), u64::from(stride)),
                    Off::Any => 1,
                };
                mirror.dst_off = off_add(mirror.dst_off, stride_off(0, period));
            }
        }
        let memory = self.memory_mut(dst);
        if !memory.mirror_set.insert(mirror) {
            return;
        }
        let index = memory.mirrors.len() as u32;
        memory.mirrors.push(mirror);
        if memory.read {
            self.start_copy(dst, index);
            self.drain_mirrors();
        }
    }

    /// Marks an object's cells read, and starts the copies into it and,
    /// through them, into the objects they copy from.
    fn demand(&mut self, obj: ObjId) {
        let memory = self.memory_mut(obj);
        if memory.read {
            return;
        }
        memory.read = true;
        self.demand_work.push(obj);
        if self.demanding {
            return;
        }
        self.demanding = true;
        while let Some(obj) = self.demand_work.pop() {
            for index in 0..self.memory_mut(obj).mirrors.len() as u32 {
                self.start_copy(obj, index);
            }
        }
        self.demanding = false;
        self.drain_mirrors();
    }

    /// Starts a copy into an object something reads: its source is read
    /// too, and each of its cells, now and as they come, moves into the
    /// destination. A copy to any offset that does not keep the parts is
    /// instead one reader of the source's bytes the copy takes, read by
    /// the cell at any offset under no part.
    fn start_copy(&mut self, dst: ObjId, index: u32) {
        let mirror = self.memory_mut(dst).mirrors[index as usize];
        let src = mirror.src;
        if lands_anywhere(&mirror) {
            let target = match bound(placed(&mirror), self.objects[dst as usize].size) {
                FULL => match self.memory_mut(dst).any_in {
                    NONE => self.make_any_in(dst),
                    any_in => any_in,
                },
                window => self.make_cell(dst, Off::Any, 0, 1, window),
            };
            if let Some((node, tr)) = self.window_reader(src, Off::Any, 0, 1, taken(&mirror)) {
                self.add_edge(node, target, tr);
            }
            return;
        }
        self.demand(src);
        let memory = self.memory_mut(src);
        let out = memory.copies_out.len() as u32;
        memory.copies_out.push((dst, index));
        for cell in memory.cells() {
            self.mirror_work.push((src, cell, out, out + 1));
        }
    }

    /// Copies between locations: the source's bytes into the destination.
    pub(crate) fn mirror_locs(&mut self, src: LocId, dst: LocId, len: u64, preserve: bool) {
        let src_window = self.loc_windows[src as usize];
        let dst_window = self.loc_windows[dst as usize];
        let (src, src_off) = self.locs[src as usize];
        let (dst, dst_off) = self.locs[dst as usize];
        self.add_mirror(
            dst,
            Mirror {
                src,
                src_off,
                src_window,
                len,
                dst_off,
                dst_window,
                preserve,
            },
        );
    }

    fn drain_mirrors(&mut self) {
        if self.draining {
            return;
        }
        self.draining = true;
        while let Some((src, cell, first, end)) = self.mirror_work.pop() {
            if self.over_cap.is_some() {
                self.mirror_work.clear();
                break;
            }
            let moved = self.stats.copy_cells.0 + self.stats.copy_cells.1;
            if moved >= self.widen_at {
                self.widen_at = moved + WIDEN_EVERY;
                self.widen_copy_cycles();
            }
            for out in first..end {
                let Some(&(dst, index)) = self.objects[src as usize]
                    .memory
                    .as_deref()
                    .and_then(|memory| memory.copies_out.get(out as usize))
                else {
                    continue;
                };
                let Some(&mirror) = self.objects[dst as usize]
                    .memory
                    .as_deref()
                    .and_then(|memory| memory.mirrors.get(index as usize))
                else {
                    continue;
                };
                self.mirror_cell(&cell, dst, mirror);
            }
        }
        self.draining = false;
    }

    /// Moves a cell of a copy's source into its destination: where the
    /// copy puts its bytes, cut to the bytes the copy took. A cell at a
    /// stride or any offset keeps the window it reaches, moved, where the
    /// copy moves it by a constant distance, and is cut to the bytes the
    /// copy writes.
    fn mirror_cell(&mut self, cell: &Slot, dst: ObjId, mirror: Mirror) {
        let shift = off_sub(mirror.dst_off, mirror.src_off);
        let raw = taken(&mirror);
        let taken = bound(raw, self.objects[mirror.src as usize].size);
        let (off, width, window, tr) = match cell.off {
            Off::Exact(at) => {
                let end = at.saturating_add(i64::from(cell.width));
                let (low, high) = (at.max(taken.0), end.min(taken.1));
                if low >= high {
                    return;
                }
                let tr = if low == at && high == end {
                    COPY
                } else {
                    TO_ANY
                };
                let width = (high - low).min(i64::from(u32::MAX)) as u32;
                (Off::Exact(low), width, FULL, tr)
            }
            _ => {
                let window = (cell.window.0.max(taken.0), cell.window.1.min(taken.1));
                if window.0 >= window.1 {
                    return;
                }
                (cell.off, cell.width, window, COPY)
            }
        };
        let part = self.copied_part(mirror.src, cell, raw, mirror.preserve);
        let mut off = off_add(off, shift);
        let mut window = match shift {
            Off::Exact(distance) if window != FULL => (
                window.0.saturating_add(distance),
                window.1.saturating_add(distance),
            ),
            _ => FULL,
        };
        if mirror.src == dst {
            // A copy within an object copies its own copies again: each one
            // as far again, which a stride of the distance takes.
            let period = match shift {
                Off::Exact(0) => return,
                Off::Exact(distance) => distance.unsigned_abs(),
                Off::Stride { rem, stride } => gcd(u64::from(rem), u64::from(stride)),
                Off::Any => 1,
            };
            off = off_add(off, stride_off(0, period));
            window = FULL;
        }
        let window = meet(window, placed(&mirror));
        if window.0 >= window.1 {
            return;
        }
        let off = normalize(off, self.objects[dst as usize].size);
        let node = self.make_cell(dst, off, part, width, window);
        self.add_edge(cell.node, node, tr);
    }

    /// Widens each cycle of copies that moves the bytes some distance each
    /// way round: every copy between two objects of a strongly connected
    /// component of the copies puts the bytes at every offset a multiple
    /// of the gcd of those distances away, so that a cell going round
    /// reaches no new offset. A copy within one object is widened as it
    /// moves each cell.
    fn widen_copy_cycles(&mut self) {
        let mut adjacency: Vec<Vec<u32>> = vec![Vec::new(); self.objects.len()];
        for (src, object) in self.objects.iter().enumerate() {
            if let Some(memory) = object.memory.as_deref() {
                adjacency[src].extend(
                    memory
                        .copies_out
                        .iter()
                        .map(|&(dst, _)| dst)
                        .filter(|&dst| dst as usize != src),
                );
            }
        }
        let (members, starts) = strong_components(&adjacency);
        let mut component = vec![u32::MAX; adjacency.len()];
        for (c, range) in starts.windows(2).enumerate() {
            let nodes = &members[range[0] as usize..range[1] as usize];
            if nodes.len() < 2 {
                continue;
            }
            for &node in nodes {
                component[node as usize] = c as u32;
            }
            let mut inside: Vec<(ObjId, u32, ObjId, u32, Mirror)> = Vec::new();
            for &src in nodes {
                let Some(memory) = self.objects[src as usize].memory.as_deref() else {
                    continue;
                };
                for (out, &(dst, index)) in memory.copies_out.iter().enumerate() {
                    if component[dst as usize] != c as u32 {
                        continue;
                    }
                    if let Some(&mirror) = (self.objects[dst as usize].memory.as_deref())
                        .and_then(|memory| memory.mirrors.get(index as usize))
                    {
                        inside.push((src, out as u32, dst, index, mirror));
                    }
                }
            }
            let mut potential: FxHashMap<ObjId, i64> = FxHashMap::default();
            potential.insert(nodes[0], 0);
            let mut by_src: FxHashMap<ObjId, Vec<usize>> = FxHashMap::default();
            for (i, &(src, ..)) in inside.iter().enumerate() {
                by_src.entry(src).or_default().push(i);
            }
            let mut work = vec![nodes[0]];
            let mut period = 0u64;
            while let Some(src) = work.pop() {
                let from = potential[&src];
                for &i in by_src.get(&src).map_or(&[][..], |copies| &copies[..]) {
                    let (_, _, dst, _, mirror) = inside[i];
                    let Some((distance, stride)) =
                        off_parts(off_sub(mirror.dst_off, mirror.src_off))
                    else {
                        period = 1;
                        continue;
                    };
                    period = gcd(period, stride);
                    let reached = from.saturating_add(distance);
                    match potential.get(&dst) {
                        Some(&known) => period = gcd(period, reached.abs_diff(known)),
                        None => {
                            potential.insert(dst, reached);
                            work.push(dst);
                        }
                    }
                }
            }
            if period == 0 {
                continue;
            }
            for &(src, out, dst, index, mirror) in &inside {
                if src == dst {
                    continue;
                }
                let wide = Mirror {
                    dst_off: off_add(mirror.dst_off, stride_off(0, period)),
                    ..mirror
                };
                if wide == mirror {
                    continue;
                }
                let memory = self.memory_mut(dst);
                memory.mirror_set.insert(wide);
                memory.mirrors[index as usize] = wide;
                self.stats.widened_copies += 1;
                for cell in self.memory_mut(src).cells() {
                    self.mirror_work.push((src, cell, out, out + 1));
                }
            }
        }
    }

    /// The part a cell a copy moves is written under, its own with the
    /// values the source's writes of the key's byte that are not paired
    /// leave there: so where the copy keeps the parts, or copies every
    /// byte of each object of the part's record the cell may be in, its
    /// tag among them; a variable argument's, its own; else none.
    fn copied_part(&mut self, src: ObjId, cell: &Slot, taken: Window, preserve: bool) -> TagSetId {
        let mut tag = cell.tag;
        if tag != 0 {
            let (key, values) = self.tag_sets[tag as usize];
            let unpaired = self.unpaired_in(src, key);
            if unpaired & !values != 0 {
                let full = self.tags.keys[key as usize].full();
                let values = values | unpaired;
                tag = if values & full == full {
                    0
                } else {
                    self.tag_set(key, values)
                };
            }
        }
        if tag == 0 || preserve || self.tag_sets[tag as usize].0 == VARIADIC {
            return tag;
        }
        let Off::Exact(at) = cell.off else {
            return 0;
        };
        if taken == FULL {
            return 0;
        }
        let (key, _) = self.tag_sets[tag as usize];
        let key = &self.tags.keys[key as usize];
        let (region_start, region_end) = key.region;
        let first = at + i64::from(cell.width) - region_end;
        let last = at - region_start + key.size;
        if first >= taken.0 && last <= taken.1 {
            tag
        } else {
            0
        }
    }

    /// The loose readers by their stride, the copies by how far they move
    /// the bytes, and the readers and cells holding a pointer.
    pub(crate) fn report_strides(&self, out: &mut String) {
        use std::fmt::Write;
        let bucket = |off: Off| -> usize {
            match off {
                Off::Exact(_) => 0,
                Off::Any => 1,
                Off::Stride { stride, .. } => match stride {
                    0..=8 => 2,
                    9..=16 => 3,
                    17..=64 => 4,
                    65..=256 => 5,
                    257..=4096 => 6,
                    _ => 7,
                },
            }
        };
        let names = [
            "constant", "any", "2-8", "9-16", "17-64", "65-256", "257-4096", "more",
        ];
        let holds = |node: NodeId| self.pts[self.find_const(node) as usize] != super::bitmap::EMPTY;
        let (mut readers, mut contexts, mut held) = ([0u64; 8], [0u64; 8], [0u64; 8]);
        let (mut shifts, mut preserved, mut own) = ([0u64; 8], 0u64, 0u64);
        let (mut cells, mut cells_held) = ([0u64; 8], [0u64; 8]);
        let mut crowded: Vec<(usize, ObjId)> = Vec::new();
        for (obj, object) in self.objects.iter().enumerate() {
            let Some(memory) = object.memory.as_deref() else {
                continue;
            };
            for (&(_, tag, _), &node) in &memory.exact_readers {
                readers[0] += 1;
                contexts[0] += u64::from(tag != 0);
                held[0] += u64::from(holds(node));
            }
            for reader in &memory.loose_readers {
                let b = bucket(reader.off);
                readers[b] += 1;
                contexts[b] += u64::from(reader.tag != 0);
                held[b] += u64::from(holds(reader.node));
            }
            for cell in memory.cells() {
                cells[bucket(cell.off)] += 1;
                cells_held[bucket(cell.off)] += u64::from(holds(cell.node));
            }
            for mirror in &memory.mirrors {
                shifts[bucket(off_sub(mirror.dst_off, mirror.src_off))] += 1;
                preserved += u64::from(mirror.preserve);
                own += u64::from(mirror.src == obj as ObjId);
            }
            crowded.push((memory.loose_readers.len(), obj as ObjId));
        }
        let row = |counts: &[u64; 8]| -> String {
            names
                .iter()
                .zip(counts)
                .map(|(name, n)| format!("{name} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = writeln!(out, "readers by stride: {}", row(&readers));
        let _ = writeln!(out, "  under a context: {}", row(&contexts));
        let _ = writeln!(out, "  holding a pointer: {}", row(&held));
        let _ = writeln!(
            out,
            "copies by shift: {}; {preserved} keeping the parts, {own} within an object",
            row(&shifts)
        );
        let _ = writeln!(out, "cells by stride: {}", row(&cells));
        let _ = writeln!(out, "  holding a pointer: {}", row(&cells_held));
        crowded.sort_unstable_by(|a, b| b.cmp(a));
        for &(_, obj) in crowded.iter().take(6) {
            let memory = self.objects[obj as usize].memory.as_deref().unwrap();
            let mut by: [u64; 8] = [0; 8];
            let mut rems: FxHashMap<u32, FxHashSet<u32>> = FxHashMap::default();
            let mut ctxs: FxHashSet<TagSetId> = FxHashSet::default();
            let mut widths: FxHashSet<u32> = FxHashSet::default();
            for reader in &memory.loose_readers {
                by[bucket(reader.off)] += 1;
                if let Off::Stride { rem, stride } = reader.off {
                    rems.entry(stride).or_default().insert(rem);
                }
                ctxs.insert(reader.tag);
                widths.insert(reader.width);
            }
            let mut strides: Vec<(usize, u32)> = rems
                .iter()
                .map(|(&stride, rems)| (rems.len(), stride))
                .collect();
            strides.sort_unstable_by(|a, b| b.cmp(a));
            let strides: Vec<String> = strides
                .iter()
                .take(6)
                .map(|(n, stride)| format!("{n} of {stride}"))
                .collect();
            let shifts: FxHashSet<Off> = memory
                .mirrors
                .iter()
                .map(|mirror| off_sub(mirror.dst_off, mirror.src_off))
                .collect();
            let _ = writeln!(
                out,
                "  {} loose readers ({}), {} strides (remainders: {}), {} contexts, {} widths, {} copies in at {} shifts: {}",
                memory.loose_readers.len(),
                row(&by),
                rems.len(),
                strides.join(", "),
                ctxs.len(),
                widths.len(),
                memory.mirrors.len(),
                shifts.len(),
                self.object_name(obj)
            );
        }
    }
}
