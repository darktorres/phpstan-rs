//! The constraints: each instance's statements, the globals' initializers,
//! and each call as its callees are found.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::library::{Class, ClassId, Extent, Pointee};
use super::survey::{constant, strip_casts};
use super::tags::{Notes, place_key};
use super::{
    Apart, COPY, Cons, DefId, Dst, InstId, LocId, MEMORY, NONE, NodeId, ObjId, ObjKind, Off, Param,
    Phase, PlainStore, Pta, Ret, Site, SiteId, SiteKind, StaticOp, StoreAt, TO_ANY, TO_END,
    TagSetId, Target, TrId, Transform, Wrapper, gcd, off_add, stride_off,
};
use super::{Arg, Inst, LoadKey, TyId};
use crate::externals::{self, Effect, Src};
use crate::ir::{
    BinOp, BlockId, Callee, Expr, ExprKind, FnName, Function, Init, InitItem, Place, Stmt, Term,
    UnOp, VarId,
};
use crate::libraries::{LibId, UNREAD};
use crate::store::Store;
use crate::types::Shape;

/// A term of a value: the pointers a node holds, moved, or a pointer to a
/// location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ptr {
    Node(NodeId, TrId),
    Loc(LocId),
}

/// The pointers a value may hold.
pub(crate) type Val = Vec<Ptr>;

/// An instance's code as its constraints are made.
struct Body<'b> {
    inst: InstId,
    registers: NodeId,
    memory: ObjId,
    slots: Box<[u32]>,
    notes: &'b Notes,
    function: &'b Function,
    /// For a wrapper's: the wrapper, and the calls whose objects each of
    /// its analyses makes apart.
    wrapper: Option<(DefId, Apart)>,
}

impl Body<'_> {
    fn is_register(&self, var: VarId) -> bool {
        self.slots
            .get(var as usize)
            .is_some_and(|&slot| slot & MEMORY == 0)
    }

    fn reg(&self, var: VarId) -> NodeId {
        self.registers + self.slots[var as usize]
    }

    fn mem_obj(&self, var: VarId) -> ObjId {
        self.memory + (self.slots[var as usize] & !MEMORY)
    }
}

/// Where an initializer's bytes are under a key: the key, where its
/// instance starts, and the part its region is written under.
type Active = (super::tags::KeyId, i64, TagSetId);

/// What an initializer list gives at a byte: nothing, a value that is not
/// constant, or a constant byte.
enum Given {
    Nothing,
    Unknown,
    Byte(u8),
}

impl Pta<'_> {
    // Before the constraints.

    /// The wrappers, the keys, the functions without effects and each
    /// definition's shared instance.
    pub(crate) fn prepare(&mut self) {
        self.find_wrappers();
        self.tag_model();
        self.find_pure();
        self.find_key_writers();
        for def in 0..self.defs.len() as DefId {
            let inst = self.new_inst(def, 0);
            self.defs[def as usize].inst = inst;
        }
    }

    /// The definitions returning only memory their calls allocate: whose
    /// every dependency allocates, or is such a definition.
    fn find_wrappers(&mut self) {
        let candidates = std::mem::take(&mut self.survey.candidates);
        let mut found: FxHashSet<DefId> = FxHashSet::default();
        loop {
            let mut changed = false;
            for (def, _, dependencies) in &candidates {
                if found.contains(def) {
                    continue;
                }
                if dependencies.iter().all(|&f| self.allocates(f, &found)) {
                    found.insert(*def);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for (def, function, _) in candidates {
            if found.contains(&def) {
                let apart = Arc::new(self.apart_calls(def, &function));
                self.wrappers.insert(
                    def,
                    Wrapper {
                        function,
                        notes: None,
                        apart,
                    },
                );
            }
        }
        self.stats.wrappers = self.wrappers.len();
    }

    /// The direct calls of a wrapper whose objects each of its analyses
    /// makes apart: those it returns and those stored into them, through
    /// its register variables. Its other objects are its own, one for all
    /// its analyses.
    fn apart_calls(&self, def: DefId, function: &Function) -> FxHashSet<(BlockId, u32)> {
        let info = &self.defs[def as usize];
        let register = |var: VarId| info.is_register(var);
        let mut values: FxHashMap<VarId, Vec<&Expr>> = FxHashMap::default();
        let mut calls: FxHashMap<VarId, Vec<(BlockId, u32)>> = FxHashMap::default();
        let mut stores: Vec<(&Place, &Expr)> = Vec::new();
        let mut work: Vec<&Expr> = Vec::new();
        for (b, block) in function.blocks.iter().enumerate() {
            for (i, stmt) in block.stmts.iter().enumerate() {
                match stmt {
                    Stmt::Assign {
                        dst: Place::Var(var),
                        src,
                        ..
                    } if register(*var) => values.entry(*var).or_default().push(src),
                    Stmt::Assign { dst, src, .. } => stores.push((dst, src)),
                    Stmt::Call {
                        dst: Some(Place::Var(var)),
                        callee: Callee::Direct(_),
                        ..
                    } if register(*var) => calls
                        .entry(*var)
                        .or_default()
                        .push((b as BlockId, i as u32)),
                    _ => {}
                }
            }
            if let Term::Return(Some(value)) = &block.term {
                work.push(value);
            }
        }
        // The variables holding pointers to the objects made apart, and
        // the ones computed from them.
        let mut held: FxHashSet<VarId> = FxHashSet::default();
        let mut derived: FxHashSet<VarId> = FxHashSet::default();
        let mut stored = vec![false; stores.len()];
        loop {
            while let Some(value) = work.pop() {
                let mut vars = Vec::new();
                computed_from(value, &mut vars);
                for var in vars {
                    if register(var) && held.insert(var) {
                        work.extend(values.get(&var).into_iter().flatten().copied());
                    }
                }
            }
            derived.extend(held.iter().copied());
            loop {
                let more: Vec<VarId> = values
                    .iter()
                    .filter(|(var, _)| !derived.contains(var))
                    .filter(|(_, values)| {
                        values.iter().any(|value| {
                            let mut vars = Vec::new();
                            mentioned(value, &mut vars);
                            vars.iter().any(|var| derived.contains(var))
                        })
                    })
                    .map(|(&var, _)| var)
                    .collect();
                if more.is_empty() {
                    break;
                }
                derived.extend(more);
            }
            for (&(dst, src), stored) in stores.iter().zip(stored.iter_mut()) {
                if *stored {
                    continue;
                }
                let mut vars = Vec::new();
                place_mentioned(dst, &mut vars);
                if vars.iter().any(|var| derived.contains(var)) {
                    *stored = true;
                    work.push(src);
                }
            }
            if work.is_empty() {
                break;
            }
        }
        held.iter()
            .flat_map(|var| calls.get(var).into_iter().flatten().copied())
            .collect()
    }

    fn allocates(&self, f: super::FnId, wrappers: &FxHashSet<DefId>) -> bool {
        let name = &self.fn_names[f as usize];
        if let Some(effects) = externals::php_allocator(name) {
            return effects
                .iter()
                .any(|effect| matches!(effect, Effect::Allocates | Effect::Grows(_)));
        }
        let defs = &self.fn_defs[f as usize];
        if defs.is_empty() {
            return externals::effects(name).is_some_and(|effects| {
                effects.iter().any(|effect| {
                    matches!(
                        effect,
                        Effect::Allocates
                            | Effect::AllocatesOutside
                            | Effect::Grows(_)
                            | Effect::Maps { .. }
                    )
                })
            });
        }
        defs.iter().all(|def| wrappers.contains(def))
    }

    /// The functions whose calls write no memory but their own variables
    /// and the memory they allocate.
    fn find_pure(&mut self) {
        let mut pure: FxHashSet<super::FnId> = (0..self.fn_names.len() as super::FnId)
            .filter(|&f| {
                let defs = &self.fn_defs[f as usize];
                !defs.is_empty()
                    && defs.iter().all(|&def| {
                        let facts = &self.survey.facts[def as usize];
                        facts.local_only && !facts.indirect && !self.defs[def as usize].unread
                    })
            })
            .collect();
        loop {
            let mut removed = Vec::new();
            for &f in &pure {
                let keeps = self.fn_defs[f as usize].iter().all(|&def| {
                    self.survey.facts[def as usize]
                        .callees
                        .iter()
                        .all(|&callee| pure.contains(&callee) || self.pure_external(callee))
                });
                if !keeps {
                    removed.push(f);
                }
            }
            if removed.is_empty() {
                break;
            }
            for f in removed {
                pure.remove(&f);
            }
        }
        self.stats.pure = pure.len();
        self.pure = pure;
    }

    pub(crate) fn pure_external(&self, f: super::FnId) -> bool {
        let name = &self.fn_names[f as usize];
        if externals::php_allocator(name).is_some() {
            return true;
        }
        self.fn_defs[f as usize].is_empty()
            && externals::effects(name).is_some_and(|effects| {
                effects.iter().all(|effect| {
                    matches!(
                        effect,
                        Effect::Gives(_)
                            | Effect::GivesHeld(_)
                            | Effect::GivesOwn
                            | Effect::Derives
                            | Effect::GivesLoaded
                            | Effect::Allocates
                            | Effect::AllocatesOutside
                            | Effect::Maps { .. }
                    )
                })
            })
    }

    /// An analysis of a definition: its registers' nodes, its memory
    /// variables' objects and its result's places.
    pub(crate) fn new_inst(&mut self, def: DefId, clone: u32) -> InstId {
        let inst = self.insts.len() as InstId;
        let info = &self.defs[def as usize];
        let params = info.params.len() as VarId;
        let slots = info.slots.clone();
        let memory_vars = info.memory.clone();
        let (ret, variadic) = (info.ret, info.variadic);
        let open = clone == 0 && self.address_taken.contains(&info.fn_id);
        let registers = self.rep.len() as NodeId;
        for (var, &slot) in slots.iter().enumerate() {
            if slot & MEMORY == 0 {
                self.new_node(open && (var as VarId) < params);
            }
        }
        let memory = if memory_vars.is_empty() {
            NONE
        } else {
            self.objects.len() as ObjId
        };
        for &(var, size) in memory_vars.iter() {
            self.object(ObjKind::Var(inst, var), size);
        }
        let ret_node = self.new_node(false);
        let ret_obj = match ret {
            Ret::Record(size) => {
                self.object(ObjKind::Return(inst), (size != TO_END).then_some(size))
            }
            Ret::Void | Ret::Scalar => NONE,
        };
        let varargs = if variadic {
            self.object(ObjKind::VarArgs(inst), None)
        } else {
            NONE
        };
        self.insts.push(Inst {
            def,
            registers,
            memory,
            ret_node,
            ret_obj,
            varargs,
        });
        inst
    }

    // Values.

    /// How many bytes a value of a type is, for its reads and writes.
    pub(crate) fn width_of(&mut self, ty: &Arc<str>) -> u32 {
        self.types.size(ty).unwrap_or(8).clamp(1, 1 << 16) as u32
    }

    /// The type of the bytes a place names: none for a variable outside a
    /// function.
    pub(crate) fn place_ty(
        &mut self,
        function: Option<&Function>,
        place: &Place,
    ) -> Option<Arc<str>> {
        match place {
            Place::Var(var) => function?.vars.get(*var as usize).map(|var| var.ty.clone()),
            Place::Global(name) => {
                let program = self.program;
                match program.globals.get(name).and_then(|g| g.first()) {
                    Some(global) => Some(global.ty.clone()),
                    None => program.declared_globals.get(name).map(|(_, ty)| ty.clone()),
                }
            }
            Place::Deref(pointer) => self.types.pointee(&pointer.ty),
            Place::Field(_, field) => self
                .types
                .layout(&field.record)?
                .fields
                .iter()
                .find(|member| member.name == field.name)
                .map(|member| member.ty.clone()),
            Place::Index(inner, _) => {
                let ty = self.place_ty(function, inner)?;
                self.types.element(&ty)
            }
        }
    }

    fn value(&mut self, body: Option<&Body<'_>>, expr: &Expr) -> Val {
        match &expr.kind {
            ExprKind::Use(place) => {
                if let (Place::Var(var), Some(body)) = (place, body)
                    && body.is_register(*var)
                {
                    return vec![Ptr::Node(body.reg(*var), COPY)];
                }
                let shape = self.types.shape(&expr.ty);
                let at = self.addr(body, place);
                if matches!(shape, Shape::Record(_) | Shape::Array | Shape::Function) {
                    return at;
                }
                let width = self.width_of(&expr.ty);
                let ctx = body
                    .and_then(|body| body.notes.ctxs.get(&place_key(place)).copied())
                    .unwrap_or(0);
                self.load(&at, width, ctx)
            }
            ExprKind::AddrOf(place) => {
                let at = self.addr(body, place);
                match self.member_bytes(body, place) {
                    Some((low, high)) => self.shift_by(&at, Transform::IDENTITY.within(low, high)),
                    None => at,
                }
            }
            ExprKind::Int(_)
            | ExprKind::Float
            | ExprKind::SizeOf(_)
            | ExprKind::AlignOf(_)
            | ExprKind::Unary(UnOp::Not, _) => Vec::new(),
            ExprKind::Str(_) => vec![Ptr::Loc(self.literal_loc)],
            ExprKind::Func(name) => {
                let f = self.fn_id(name);
                vec![Ptr::Loc(self.fn_loc(f))]
            }
            ExprKind::Unary(UnOp::Neg | UnOp::BitNot, inner) => {
                let inner = self.value(body, inner);
                self.at_any(&inner)
            }
            ExprKind::Binary(op, left, right) => match op {
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Vec::new(),
                BinOp::Add => {
                    let (l, r) = (self.value(body, left), self.value(body, right));
                    let (by_right, by_left) = (self.linear(right), self.linear(left));
                    let mut out = self.shift(&l, by_right);
                    out.extend(self.shift(&r, by_left));
                    out
                }
                BinOp::Sub => {
                    let both = matches!(self.types.shape(&left.ty), Shape::Pointer { .. })
                        && matches!(self.types.shape(&right.ty), Shape::Pointer { .. });
                    if both {
                        return Vec::new();
                    }
                    let (l, r) = (self.value(body, left), self.value(body, right));
                    let (add, stride) = self.linear(right);
                    let mut out = self.shift(&l, (add.saturating_neg(), stride));
                    out.extend(self.at_any(&r));
                    out
                }
                _ => {
                    let mut out = self.value(body, left);
                    out.extend(self.value(body, right));
                    self.at_any(&out)
                }
            },
            ExprKind::Offset(pointer, bytes) => {
                let p = self.value(body, pointer);
                let by = self.linear(bytes);
                let mut out = self.shift(&p, by);
                // A constant address moved by an integer is the integer's
                // pointer; any other pointer keeps its own.
                if matches!(pointer.kind, ExprKind::Int(_)) {
                    let b = self.value(body, bytes);
                    let by = self.linear(pointer);
                    out.extend(self.shift(&b, by));
                }
                out
            }
            ExprKind::Cast(inner) => self.value(body, inner),
            ExprKind::VaArg(list) => {
                let list = self.value(body, list);
                let list = self.at_any(&list);
                let area = self.load(&list, 8, 0);
                let area = self.at_any(&area);
                let width = self.width_of(&expr.ty);
                let ctx = self.vararg_ctx(&expr.ty);
                self.load(&area, width, ctx)
            }
            ExprKind::Unknown(why) => {
                *self.stats.unknown.entry(why.clone()).or_default() += 1;
                Vec::new()
            }
        }
    }

    /// A value as a constant and a stride: the multiples of the stride it
    /// may add to the constant, any value for a stride of 1.
    fn linear(&mut self, expr: &Expr) -> (i64, u64) {
        let scale = |(add, stride): (i64, u64), by: i128| match i64::try_from(by) {
            Ok(by) => (
                add.saturating_mul(by),
                stride.saturating_mul(by.unsigned_abs()),
            ),
            Err(_) => (0, 1),
        };
        match &expr.kind {
            ExprKind::Int(value) => match i64::try_from(*value) {
                Ok(value) => (value, 0),
                Err(_) => (0, 1),
            },
            ExprKind::Cast(inner) => self.linear(inner),
            ExprKind::Unary(UnOp::Neg, inner) => {
                let (add, stride) = self.linear(inner);
                (add.saturating_neg(), stride)
            }
            ExprKind::Binary(BinOp::Add, left, right) => {
                let ((a, s), (b, t)) = (self.linear(left), self.linear(right));
                (a.saturating_add(b), gcd(s, t))
            }
            ExprKind::Binary(BinOp::Sub, left, right) => {
                let ((a, s), (b, t)) = (self.linear(left), self.linear(right));
                (a.saturating_sub(b), gcd(s, t))
            }
            ExprKind::Binary(BinOp::Mul, left, right) => {
                if let Some(by) = constant(right) {
                    scale(self.linear(left), by)
                } else if let Some(by) = constant(left) {
                    scale(self.linear(right), by)
                } else {
                    (0, 1)
                }
            }
            ExprKind::Binary(BinOp::Shl, left, right) => match constant(right) {
                Some(by @ 0..=62) => scale(self.linear(left), 1 << by),
                _ => (0, 1),
            },
            ExprKind::SizeOf(ty) => match self.types.size(ty).and_then(|s| i64::try_from(s).ok()) {
                Some(size) => (size, 0),
                None => (0, 1),
            },
            _ => (0, 1),
        }
    }

    fn shift(&mut self, val: &[Ptr], (add, stride): (i64, u64)) -> Val {
        self.shift_by(val, Transform::linear(add, stride))
    }

    fn shift_by(&mut self, val: &[Ptr], by: Transform) -> Val {
        if by == Transform::IDENTITY {
            return val.to_vec();
        }
        val.iter()
            .map(|&ptr| match ptr {
                Ptr::Node(node, tr) => {
                    let moved = self.trs[tr as usize].then(by);
                    Ptr::Node(node, self.tr(moved))
                }
                Ptr::Loc(loc) => Ptr::Loc(self.moved_loc(loc, by)),
            })
            .collect()
    }

    /// The bytes of the array a place names, from its start: to the end
    /// of its object where it may end the object, as the last member of
    /// a record that may.
    fn array_bytes(&mut self, body: Option<&Body<'_>>, place: &Place) -> Option<(i64, i64)> {
        let ty = self.place_ty(body.map(|body| body.function), place)?;
        if self.types.shape(&ty) != Shape::Array {
            return None;
        }
        if self.may_end_object(place) {
            return Some((0, i64::MAX));
        }
        match self.types.size(&ty) {
            Some(size) if size > 0 => Some((0, i64::try_from(size).ok()?)),
            _ => Some((0, i64::MAX)),
        }
    }

    /// The bytes of the member a place names, from its start: C takes the
    /// address of an object in no array as an array of it alone, which
    /// runs to the end of its object where the member may end it.
    fn member_bytes(&mut self, body: Option<&Body<'_>>, place: &Place) -> Option<(i64, i64)> {
        if !matches!(place, Place::Field(..)) {
            return None;
        }
        if self.may_end_object(place) {
            return Some((0, i64::MAX));
        }
        let ty = self.place_ty(body.map(|body| body.function), place)?;
        match self.types.size(&ty) {
            Some(size) if size > 0 => Some((0, i64::try_from(size).ok()?)),
            _ => None,
        }
    }

    /// Whether a place's bytes may run to the end of its object: a
    /// record's last member, in a record that may, or what a pointer
    /// points to.
    fn may_end_object(&mut self, place: &Place) -> bool {
        match place {
            Place::Deref(_) => true,
            Place::Field(inner, field) => {
                let last = field.offset.is_some_and(|at| {
                    self.types.layout(&field.record).is_some_and(|layout| {
                        !layout.is_union && layout.fields.iter().all(|member| member.offset <= at)
                    })
                });
                last && self.may_end_object(inner)
            }
            Place::Var(_) | Place::Global(_) | Place::Index(..) => false,
        }
    }

    fn at_any(&mut self, val: &[Ptr]) -> Val {
        val.iter()
            .map(|&ptr| match ptr {
                Ptr::Node(node, _) => Ptr::Node(node, TO_ANY),
                Ptr::Loc(loc) => {
                    let obj = self.locs[loc as usize].0;
                    Ptr::Loc(self.loc(obj, Off::Any))
                }
            })
            .collect()
    }

    /// The address of a place's bytes.
    fn addr(&mut self, body: Option<&Body<'_>>, place: &Place) -> Val {
        match place {
            Place::Var(var) => match body {
                Some(body) if !body.is_register(*var) => {
                    vec![Ptr::Loc(self.loc(body.mem_obj(*var), Off::Exact(0)))]
                }
                _ => Vec::new(),
            },
            Place::Global(name) => vec![Ptr::Loc(self.global_loc(name))],
            Place::Deref(pointer) => self.value(body, pointer),
            Place::Field(inner, field) => {
                let base = self.addr(body, inner);
                match field.offset.and_then(|o| i64::try_from(o).ok()) {
                    Some(offset) => self.shift(&base, (offset, 0)),
                    None => self.at_any(&base),
                }
            }
            Place::Index(inner, bytes) => {
                let base = self.addr(body, inner);
                let (add, stride) = self.linear(bytes);
                let by = Transform::linear(add, stride);
                // An array that is an element of another keeps to the
                // outer one.
                let nested = matches!(**inner, Place::Index(..));
                let by = match self.array_bytes(body, inner).filter(|_| !nested) {
                    Some((low, high)) => by.within(low, high),
                    None => by,
                };
                self.shift_by(&base, by)
            }
        }
    }

    /// A node holding a value's pointers: the value's own node where it is
    /// one, unmoved.
    pub(crate) fn materialize(&mut self, val: &[Ptr]) -> NodeId {
        match *val {
            [] => NONE,
            [Ptr::Node(node, COPY)] => node,
            [Ptr::Node(node, tr)] => {
                if let Some(&moved) = self.shifted.get(&(node, tr)) {
                    return moved;
                }
                let moved = self.new_node(false);
                self.add_edge(node, moved, tr);
                self.shifted.insert((node, tr), moved);
                moved
            }
            [Ptr::Loc(loc)] => self.const_node(loc),
            _ => {
                let node = self.new_node(false);
                self.feed(val, node);
                node
            }
        }
    }

    fn feed(&mut self, val: &[Ptr], dst: NodeId) {
        if dst == NONE {
            return;
        }
        for &ptr in val {
            match ptr {
                Ptr::Node(node, tr) => self.add_edge(node, dst, tr),
                Ptr::Loc(loc) => self.add_initial(dst, loc),
            }
        }
    }

    /// Reads `width` bytes at each pointer of a value.
    fn load(&mut self, at: &[Ptr], width: u32, ctx: TagSetId) -> Val {
        let mut out = Vec::with_capacity(at.len());
        for &ptr in at {
            match ptr {
                Ptr::Loc(loc) => {
                    if let Some(node) = self.read_loc(loc, width, ctx) {
                        out.push(Ptr::Node(node, COPY));
                    }
                }
                Ptr::Node(node, tr) => {
                    out.push(Ptr::Node(self.load_node(node, tr, width, ctx), COPY));
                }
            }
        }
        out
    }

    /// The node of what is read at each pointer a node holds, moved.
    pub(crate) fn load_node(
        &mut self,
        node: NodeId,
        tr: TrId,
        width: u32,
        ctx: TagSetId,
    ) -> NodeId {
        let key = LoadKey::Node(node, tr, width, ctx);
        if let Some(&dst) = self.loads.get(&key) {
            return dst;
        }
        let dst = self.new_node(true);
        self.add_cons(
            node,
            Cons::Load {
                tr,
                width,
                ctx,
                dst,
            },
        );
        self.loads.insert(key, dst);
        dst
    }

    /// The node of what is read at a location: none for the bytes of code
    /// and of string literals.
    fn read_loc(&mut self, loc: LocId, width: u32, ctx: TagSetId) -> Option<NodeId> {
        let (obj, off) = self.locs[loc as usize];
        if self.objects[obj as usize].is_constant() {
            return None;
        }
        let key = LoadKey::Loc(loc, width, ctx);
        if let Some(&dst) = self.loads.get(&key) {
            return Some(dst);
        }
        let dst = if ctx != 0 && self.phase == Phase::Generate {
            let dst = self.new_node(false);
            self.static_ops.push(StaticOp::Read {
                loc,
                width,
                ctx,
                dst,
            });
            dst
        } else {
            let ctx = self.ctx_final.get(ctx as usize).copied().unwrap_or(0);
            let window = self.loc_windows[loc as usize];
            self.reader(obj, off, ctx, width, window)
        };
        self.loads.insert(key, dst);
        Some(dst)
    }

    /// Writes a node's pointers at a location, under a part.
    pub(crate) fn write_loc(&mut self, loc: LocId, src: NodeId, width: u32, part: TagSetId) {
        if src == NONE {
            return;
        }
        let (obj, off) = self.locs[loc as usize];
        if !self.objects[obj as usize].takes_writes() {
            return;
        }
        if part != 0 && self.phase == Phase::Generate {
            self.static_ops.push(StaticOp::Write {
                loc,
                width,
                part,
                src,
            });
            return;
        }
        let part = self.part_final.get(part as usize).copied().unwrap_or(0);
        let window = self.loc_windows[loc as usize];
        let cell = self.cell(obj, off, part, width, window);
        self.add_edge(src, cell, COPY);
    }

    /// Writes a node's pointers at each pointer of a value.
    fn store(&mut self, at: &[Ptr], src: NodeId, width: u32, part: TagSetId) {
        if src == NONE {
            return;
        }
        for &ptr in at {
            match ptr {
                Ptr::Loc(loc) => self.write_loc(loc, src, width, part),
                Ptr::Node(node, tr) => self.add_cons(
                    node,
                    Cons::Store {
                        tr,
                        width,
                        part,
                        src,
                    },
                ),
            }
        }
    }

    /// Copies `len` bytes from each pointer of one value to each of
    /// another's: pair by pair, or, where both sides hold nodes or the
    /// pairs outnumber the pointers, through an object of the copy's own.
    fn copy_bytes(&mut self, from: &[Ptr], to: &[Ptr], len: u64, preserve: bool) {
        if from.is_empty() || to.is_empty() {
            return;
        }
        let is_node = |ptr: &Ptr| matches!(ptr, Ptr::Node(..));
        let through = (from.iter().any(is_node) && to.iter().any(is_node))
            || from.len() * to.len() > from.len() + to.len();
        if !through {
            for &src in from {
                for &dst in to {
                    self.copy_one(src, dst, len, preserve);
                }
            }
            return;
        }
        let copy = self.copy_sites.len() as u32;
        self.copy_sites.push(self.making_at);
        let obj = self.object(ObjKind::Copy(copy), (len != TO_END).then_some(len));
        let through = Ptr::Loc(self.loc(obj, Off::Exact(0)));
        for &src in from {
            self.copy_one(src, through, len, preserve);
        }
        for &dst in to {
            self.copy_one(through, dst, len, preserve);
        }
    }

    /// Copies `len` bytes from one pointer to another, one of them a
    /// location.
    fn copy_one(&mut self, from: Ptr, to: Ptr, len: u64, preserve: bool) {
        match (from, to) {
            (Ptr::Loc(src), Ptr::Loc(dst)) => self.mirror_locs(src, dst, len, preserve),
            (Ptr::Node(node, tr), Ptr::Loc(dst)) => self.add_cons(
                node,
                Cons::Gather {
                    tr,
                    len,
                    preserve,
                    dst,
                },
            ),
            (Ptr::Loc(src), Ptr::Node(node, tr)) => self.add_cons(
                node,
                Cons::Scatter {
                    tr,
                    len,
                    preserve,
                    src,
                },
            ),
            (Ptr::Node(..), Ptr::Node(..)) => {
                unreachable!("a copy between two nodes goes through an object")
            }
        }
    }

    // Statements.

    fn gen_body(&mut self, inst: InstId, function: &Function, notes: &Notes) {
        let info = &self.insts[inst as usize];
        let def = info.def;
        let body = Body {
            inst,
            registers: info.registers,
            memory: info.memory,
            slots: self.defs[def as usize].slots.clone(),
            notes,
            function,
            wrapper: self
                .wrappers
                .get(&def)
                .map(|wrapper| (def, wrapper.apart.clone())),
        };
        for (b, block) in function.blocks.iter().enumerate() {
            let mut loaded: Vec<(VarId, &Place)> = Vec::new();
            for (i, stmt) in block.stmts.iter().enumerate() {
                self.gen_stmt(&body, &mut loaded, stmt, (b as BlockId, i as u32));
            }
            if let Term::Return(Some(value)) = &block.term {
                self.gen_return(&body, value);
            }
        }
        if self.defs[def as usize].unread {
            self.link_unread(inst);
        }
    }

    fn gen_stmt<'f>(
        &mut self,
        body: &Body<'f>,
        loaded: &mut Vec<(VarId, &'f Place)>,
        stmt: &'f Stmt,
        (block, index): (BlockId, u32),
    ) {
        if let Stmt::Assign { line, .. } | Stmt::Call { line, .. } = stmt {
            self.making_at = (Some(body.inst), *line);
        }
        match stmt {
            Stmt::Assign { dst, src, line } => {
                if let Place::Var(var) = dst
                    && body.is_register(*var)
                {
                    let val = self.value(Some(body), src);
                    self.feed(&val, body.reg(*var));
                    loaded.retain(|(v, _)| v != var);
                    if let ExprKind::Use(place) = &strip_casts(src).kind
                        && !matches!(place, Place::Var(v) if body.is_register(*v))
                    {
                        loaded.push((*var, place));
                    }
                    return;
                }
                let facts = self.types.facts(&src.ty);
                let to = self.addr(Some(body), dst);
                if matches!(facts.shape, Shape::Record(_) | Shape::Array) {
                    let from = self.value(Some(body), src);
                    self.copy_bytes(&from, &to, facts.size.unwrap_or(TO_END), false);
                    return;
                }
                let width = self.width_of(&src.ty);
                let key = place_key(dst);
                let copied = body.notes.copies.get(&key).and_then(|var| {
                    loaded
                        .iter()
                        .rev()
                        .find(|(v, _)| v == var)
                        .map(|&(_, place)| place)
                });
                if let Some(source) = copied {
                    let from = self.addr(Some(body), source);
                    self.copy_bytes(&from, &to, u64::from(width), true);
                    return;
                }
                let val = self.value(Some(body), src);
                let node = self.materialize(&val);
                let part = body.notes.parts.get(&key).copied().unwrap_or(0);
                self.store(&to, node, width, part);
                self.note_store(body, dst, &to, node, width, *line);
            }
            Stmt::Call {
                dst,
                callee,
                args,
                line,
            } => {
                let indirect = matches!(callee, Callee::Indirect(_));
                let kind = if indirect {
                    SiteKind::Indirect
                } else {
                    SiteKind::Direct
                };
                let mut site = Site::new(Some(body.inst), *line, kind);
                site.shared = body
                    .wrapper
                    .as_ref()
                    .filter(|(_, apart)| !apart.contains(&(block, index)))
                    .map(|&(def, _)| (def, block, index));
                site.args = args.iter().map(|arg| self.arg(body, arg)).collect();
                site.dst = self.call_dst(body, dst.as_ref(), indirect, *line);
                if let Some(ty) = dst
                    .as_ref()
                    .and_then(|dst| self.place_ty(Some(body.function), dst))
                {
                    site.result = self.ty_id(&ty);
                }
                if let Some(Place::Var(var)) = dst {
                    loaded.retain(|(v, _)| v != var);
                }
                match callee {
                    Callee::Direct(name) => {
                        let f = self.fn_id(name);
                        site.direct = Some(f);
                        let site = self.new_site(site);
                        self.call_function(site, f, true);
                    }
                    Callee::Indirect(expr) => {
                        let val = self.value(Some(body), expr);
                        let node = self.materialize(&val);
                        site.callee = (node != NONE).then_some(node);
                        let site = self.new_site(site);
                        if node != NONE {
                            self.add_cons(node, Cons::Call { site });
                        }
                    }
                }
            }
            Stmt::Asm { writes, reads, .. } => {
                let mut val = Vec::new();
                for read in reads {
                    val.extend(self.value(Some(body), read));
                }
                for write in writes {
                    match write {
                        Place::Var(var) if body.is_register(*var) => {
                            val.push(Ptr::Node(body.reg(*var), COPY));
                        }
                        _ => {
                            let at = self.addr(Some(body), write);
                            val.extend(self.load(&at, 8, 0));
                        }
                    }
                }
                let val = self.at_any(&val);
                let node = self.materialize(&val);
                for write in writes {
                    if let Place::Var(var) = write {
                        loaded.retain(|(v, _)| v != var);
                    }
                }
                if node == NONE {
                    return;
                }
                for write in writes {
                    match write {
                        Place::Var(var) if body.is_register(*var) => {
                            self.add_edge(node, body.reg(*var), COPY);
                        }
                        _ => {
                            let at = self.addr(Some(body), write);
                            let at = self.at_any(&at);
                            self.store(&at, node, 8, 0);
                        }
                    }
                }
                for read in reads {
                    let at = self.value(Some(body), read);
                    let at = self.at_any(&at);
                    self.store(&at, node, 8, 0);
                }
            }
        }
    }

    /// Records a store for the check of the tags: a write of a key's byte
    /// where the notes found one, else a write not through the key's
    /// members; leaves the values a write of a key's byte that is not
    /// paired writes in every part of the objects written; and links a
    /// function written
    /// into a library's record to the library's calls.
    fn note_store(
        &mut self,
        body: &Body<'_>,
        dst: &Place,
        to: &[Ptr],
        src: NodeId,
        width: u32,
        line: u32,
    ) {
        let key = place_key(dst);
        for &(unpaired, values) in body.notes.unpaired.get(&key).into_iter().flatten() {
            for &ptr in to {
                match ptr {
                    Ptr::Loc(loc) => {
                        let obj = self.locs[loc as usize].0;
                        self.leave_unpaired(obj, unpaired, values);
                    }
                    Ptr::Node(node, _) => self.add_cons(
                        node,
                        Cons::Unpaired {
                            key: unpaired,
                            values,
                        },
                    ),
                }
            }
        }
        let terms = to.iter().map(|&ptr| match ptr {
            Ptr::Loc(loc) => StoreAt::Loc(loc),
            Ptr::Node(node, tr) => StoreAt::Node(node, tr),
        });
        match body.notes.tags.get(&key) {
            Some(bytes) => {
                for at in terms {
                    for &(_, byte) in bytes {
                        self.tag_writes.push((at, byte));
                    }
                }
            }
            None => {
                if !matches!(dst, Place::Var(_) | Place::Global(_)) {
                    for at in terms {
                        self.plain_stores.push(PlainStore {
                            at,
                            width,
                            inst: body.inst,
                            line,
                        });
                    }
                }
            }
        }
        if src != NONE
            && let Place::Field(_, field) = dst
        {
            self.link_library_member(&field.record, &field.name, src);
        }
    }

    /// A function written into a member of a library's record that holds
    /// one is a function the library may call.
    fn link_library_member(&mut self, record: &Arc<str>, member: &Arc<str>, src: NodeId) {
        let record = self.types.canonical(record);
        let Some(&library) = self.libraries.records.get(&record) else {
            return;
        };
        let calls = self.types.layout(&record).is_some_and(|layout| {
            layout
                .fields
                .iter()
                .any(|field| field.name == *member && field.calls)
        });
        if calls {
            let held = self.lib_calls(library);
            self.add_edge(src, held, COPY);
        }
    }

    fn arg(&mut self, body: &Body<'_>, expr: &Expr) -> Arg {
        let facts = self.types.facts(&expr.ty);
        let record = matches!(facts.shape, Shape::Record(_) | Shape::Array)
            .then(|| facts.size.unwrap_or(TO_END));
        let val = self.value(Some(body), expr);
        Arg {
            val: self.materialize(&val),
            record,
            ty: self.ty_id(&expr.ty),
            width: self.width_of(&expr.ty),
            constant: constant(expr),
            own: None,
        }
    }

    /// Where a call's result goes: a register's node, a record's address,
    /// or a node stored into the place.
    fn call_dst(&mut self, body: &Body<'_>, dst: Option<&Place>, indirect: bool, line: u32) -> Dst {
        let Some(place) = dst else {
            return Dst::None;
        };
        if let Place::Var(var) = place
            && body.is_register(*var)
        {
            let node = body.reg(*var);
            if indirect {
                self.indirect[node as usize] = true;
            }
            return Dst::Node(node);
        }
        let ty = self.place_ty(Some(body.function), place);
        let facts = ty.as_ref().map(|ty| self.types.facts(ty));
        let at = self.addr(Some(body), place);
        if let Some(facts) = &facts
            && matches!(facts.shape, Shape::Record(_) | Shape::Array)
        {
            let node = self.materialize(&at);
            if node == NONE {
                return Dst::None;
            }
            return Dst::Record(node, facts.size.unwrap_or(TO_END));
        }
        let width = match &ty {
            Some(ty) => self.width_of(ty),
            None => 8,
        };
        let node = self.new_node(indirect);
        let part = body
            .notes
            .parts
            .get(&place_key(place))
            .copied()
            .unwrap_or(0);
        self.store(&at, node, width, part);
        self.note_store(body, place, &at, node, width, line);
        Dst::Node(node)
    }

    fn gen_return(&mut self, body: &Body<'_>, value: &Expr) {
        let inst = &self.insts[body.inst as usize];
        let (ret_node, ret_obj) = (inst.ret_node, inst.ret_obj);
        let val = self.value(Some(body), value);
        let facts = self.types.facts(&value.ty);
        if ret_obj != NONE && matches!(facts.shape, Shape::Record(_) | Shape::Array) {
            let to = self.loc(ret_obj, Off::Exact(0));
            self.copy_bytes(&val, &[Ptr::Loc(to)], facts.size.unwrap_or(TO_END), true);
        } else {
            self.feed(&val, ret_node);
        }
    }

    /// A definition the lowering could not read whole runs anything: what
    /// it is given and what it returns are code the build has no source
    /// of, which takes them by their types.
    fn link_unread(&mut self, inst: InstId) {
        let library = self
            .libraries
            .names
            .iter()
            .position(|name| &**name == UNREAD)
            .map_or(self.libraries.unknown, |at| at as super::LibId);
        let info = &self.insts[inst as usize];
        let (def, registers, memory, ret_node, ret_obj) = (
            info.def,
            info.registers,
            info.memory,
            info.ret_node,
            info.ret_obj,
        );
        let params = self.defs[def as usize].params.clone();
        let slots = self.defs[def as usize].slots.clone();
        let (ret, ret_ty) = (self.defs[def as usize].ret, self.defs[def as usize].ret_ty);
        for (param, &slot) in params.iter().zip(slots.iter()) {
            let Some(ty) = self.ty(param.ty) else {
                continue;
            };
            if slot & MEMORY == 0 {
                let node = registers + slot;
                if let Some(class) = self.class_of(library, &ty) {
                    let held = self.lib_held(library, class, Extent::One);
                    self.add_edge(node, held, COPY);
                    let gives = self.lib_gives(library, class);
                    self.add_edge(gives, node, COPY);
                }
            } else {
                let obj = memory + (slot & !MEMORY);
                let loc = self.loc(obj, Off::Exact(0));
                let at = self.const_node(loc);
                let class = self.pointer_to(library, &ty);
                self.lib_access(library, at, class, Extent::One, true, true);
            }
        }
        let Some(ret_ty) = self.ty(ret_ty) else {
            return;
        };
        match ret {
            Ret::Scalar => {
                if let Some(class) = self.class_of(library, &ret_ty) {
                    let gives = self.lib_gives(library, class);
                    self.add_edge(gives, ret_node, COPY);
                }
            }
            Ret::Record(_) if ret_obj != NONE => {
                let loc = self.loc(ret_obj, Off::Exact(0));
                let at = self.const_node(loc);
                let class = self.pointer_to(library, &ret_ty);
                self.lib_access(library, at, class, Extent::One, false, true);
            }
            _ => {}
        }
    }

    // The whole program.

    /// Makes the constraints of the globals' initializers and of every
    /// definition's shared instance, each body loaded from the store.
    pub(crate) fn generate(&mut self, store: &Store) -> Result<(), String> {
        self.gen_globals();
        for def in 0..self.defs.len() as DefId {
            let inst = self.defs[def as usize].inst;
            if self.wrappers.contains_key(&def) {
                let (function, notes) = self.wrapper_body(def);
                self.gen_body(inst, &function, &notes);
            } else {
                let function: Function = store.get(self.defs[def as usize].hash)?;
                let notes = self.tag_notes(def, &function);
                self.gen_body(inst, &function, &notes);
            }
            if self.over_cap.is_some() || def.is_multiple_of(4096) {
                self.check_memory()?;
            }
        }
        self.gen_main();
        self.stats.nodes_generated = self.rep.len();
        Ok(())
    }

    /// A wrapper's code and its notes, made once.
    fn wrapper_body(&mut self, def: DefId) -> (Arc<Function>, Arc<Notes>) {
        let wrapper = &self.wrappers[&def];
        let function = wrapper.function.clone();
        if let Some(notes) = &wrapper.notes {
            return (function, notes.clone());
        }
        let notes = Arc::new(self.tag_notes(def, &function));
        if let Some(wrapper) = self.wrappers.get_mut(&def) {
            wrapper.notes = Some(notes.clone());
        }
        (function, notes)
    }

    /// The instance a direct call of a wrapper is bound to: one of its own,
    /// unless the wrapper is being analysed for a call already or the
    /// clones are at their cap.
    fn clone_for(&mut self, def: DefId) -> InstId {
        if self.cloning.contains(&def) {
            return self.defs[def as usize].inst;
        }
        if self.stats.clones >= self.clone_cap {
            self.stats.clones_refused += 1;
            return self.defs[def as usize].inst;
        }
        let (function, notes) = self.wrapper_body(def);
        self.stats.clones += 1;
        let inst = self.new_inst(def, self.stats.clones as u32);
        self.cloning.push(def);
        self.gen_body(inst, &function, &notes);
        self.cloning.pop();
        inst
    }

    /// `main`'s arguments and environment come from outside the process.
    fn gen_main(&mut self) {
        let Some(&f) = self.fn_ids.get("main") else {
            return;
        };
        for def in self.fn_defs[f as usize].clone() {
            let info = &self.insts[self.defs[def as usize].inst as usize];
            let (registers, memory) = (info.registers, info.memory);
            let slots = self.defs[def as usize].slots.clone();
            for &slot in slots
                .iter()
                .take(self.defs[def as usize].params.len())
                .skip(1)
            {
                if slot & MEMORY == 0 {
                    let outside = self.outside_loc;
                    self.add_initial(registers + slot, outside);
                } else {
                    let loc = self.loc(memory + (slot & !MEMORY), Off::Exact(0));
                    self.write_loc(loc, self.outside_node, 8, 0);
                }
            }
        }
    }

    fn gen_globals(&mut self) {
        let program = self.program;
        let mut names: Vec<&Arc<str>> = program.globals.keys().collect();
        names.sort_unstable();
        for name in names {
            for global in &program.globals[name] {
                let Some(init) = &global.init else {
                    continue;
                };
                let loc = self.global_loc(name);
                let obj = self.locs[loc as usize].0;
                self.gen_init(obj, Off::Exact(0), init, &[], None);
            }
        }
    }

    /// An initializer's writes at an offset of an object, under the parts
    /// the lists around it give their keys' regions, `member` the record
    /// member it initializes.
    fn gen_init(
        &mut self,
        obj: ObjId,
        off: Off,
        init: &Init,
        active: &[Active],
        member: Option<&InitItem>,
    ) {
        match init {
            Init::Zero => {}
            Init::Expr(expr) => {
                let facts = self.types.facts(&expr.ty);
                let val = self.value(None, expr);
                let loc = self.loc(obj, off);
                if matches!(facts.shape, Shape::Record(_) | Shape::Array) {
                    self.copy_bytes(&val, &[Ptr::Loc(loc)], facts.size.unwrap_or(TO_END), false);
                    return;
                }
                let width = self.width_of(&expr.ty);
                let (tags, part) = self.init_part(off, width, active);
                for byte in tags {
                    self.tag_writes.push((StoreAt::Loc(loc), byte));
                }
                let node = self.materialize(&val);
                if node == NONE {
                    return;
                }
                self.write_loc(loc, node, width, part);
                if let Some(field) = member.and_then(|item| item.field.as_ref()) {
                    self.link_library_member(&field.record, &field.name, node);
                }
            }
            Init::List {
                items,
                filler,
                element,
            } => {
                let mut inner: Vec<Active> = active.to_vec();
                if let (Off::Exact(at), Some(record)) = (
                    off,
                    items
                        .iter()
                        .find_map(|item| item.field.as_ref().map(|f| &f.record)),
                ) {
                    let record = self.types.canonical(record);
                    for (key, inst) in self.keys_at(&record, at) {
                        let tag = self.tags.keys[key as usize].tag;
                        // A byte the list does not give is zero.
                        let byte = match self.given_at(items, inst + tag - at) {
                            Given::Byte(byte) => Some(byte),
                            Given::Nothing => Some(0),
                            Given::Unknown => None,
                        };
                        let part = match byte {
                            Some(byte) => {
                                let bit = self.tags.keys[key as usize].bit(byte);
                                self.tag_set(key, bit)
                            }
                            None => 0,
                        };
                        inner.push((key, inst, part));
                    }
                }
                for item in items {
                    let item_off = match item.offset.and_then(|o| i64::try_from(o).ok()) {
                        Some(o) => off_add(off, Off::Exact(o)),
                        None => Off::Any,
                    };
                    self.gen_init(obj, item_off, &item.init, &inner, Some(item));
                }
                if let Some(filler) = filler {
                    let filler_off = match element {
                        Some(size) => off_add(off, stride_off(0, *size)),
                        None => Off::Any,
                    };
                    self.gen_init(obj, filler_off, filler, &inner, None);
                }
            }
        }
    }

    /// The keys of a record and of the unions it is a member of, each with
    /// where its instance starts, for a record at `at`.
    fn keys_at(&self, record: &Arc<str>, at: i64) -> Vec<(super::tags::KeyId, i64)> {
        let mut keys = Vec::new();
        for &key in self.tags.by_record.get(record).into_iter().flatten() {
            keys.push((key, at));
        }
        for (union, offset) in self.survey.variants.get(record).into_iter().flatten() {
            for &key in self.tags.by_record.get(union).into_iter().flatten() {
                keys.push((key, at - offset));
            }
        }
        keys
    }

    /// What a list gives at a byte, from where the list starts.
    fn given_at(&mut self, items: &[InitItem], byte: i64) -> Given {
        for item in items {
            let Some(start) = item.offset.and_then(|o| i64::try_from(o).ok()) else {
                continue;
            };
            if byte < start {
                continue;
            }
            match &item.init {
                Init::Expr(expr) => {
                    let width = i64::from(self.width_of(&expr.ty));
                    if byte < start + width {
                        return match constant(expr) {
                            Some(value) => {
                                Given::Byte(((value >> (8 * (byte - start))) & 0xff) as u8)
                            }
                            None => Given::Unknown,
                        };
                    }
                }
                Init::List { items, .. } => match self.given_at(items, byte - start) {
                    Given::Nothing => {}
                    given => return given,
                },
                Init::Zero => {}
            }
        }
        Given::Nothing
    }

    /// The bytes of the keys a write of an initializer writes, and the
    /// part it writes a region under.
    fn init_part(&self, off: Off, width: u32, active: &[Active]) -> (Vec<u32>, TagSetId) {
        let Off::Exact(at) = off else {
            return (Vec::new(), 0);
        };
        let width = i64::from(width);
        let mut tags = Vec::new();
        let mut best: Option<(i64, super::tags::KeyId, TagSetId)> = None;
        for &(key, inst, part) in active {
            let k = &self.tags.keys[key as usize];
            let rel = at - inst;
            if rel <= k.tag && k.tag < rel + width {
                tags.push((k.tag - rel) as u32);
            } else if rel < k.region.1 && k.region.0 < rel + width && part != 0 {
                let candidate = (k.tag, key, part);
                if best.is_none_or(|b| (candidate.0, candidate.1) < (b.0, b.1)) {
                    best = Some(candidate);
                }
            }
        }
        tags.sort_unstable();
        tags.dedup();
        (tags, best.map_or(0, |(_, _, part)| part))
    }

    // Calls.

    /// A call reaching a function: each of its definitions bound, or its
    /// effects where the build has no source of it.
    pub(crate) fn call_function(&mut self, site: SiteId, f: super::FnId, direct: bool) {
        if !self.sites[site as usize].bound.insert(Target::Function(f)) {
            return;
        }
        let name = self.fn_names[f as usize].clone();
        let allocator = externals::php_allocator(&name);
        let defs = self.fn_defs[f as usize].clone();
        if defs.is_empty() {
            match allocator {
                Some(effects) => self.apply(site, &name, effects),
                None => self.external(site, &name),
            }
            return;
        }
        let block = allocator.and_then(|effects| {
            effects.iter().find_map(|effect| match *effect {
                Effect::Block(i) | Effect::Grows(i) => Some(i),
                _ => None,
            })
        });
        // A call whose objects a wrapper's analyses share is bound to the
        // shared analyses of its callees.
        let direct = direct && self.sites[site as usize].shared.is_none();
        for def in defs {
            let inst = if direct && self.wrappers.contains_key(&def) {
                self.clone_for(def)
            } else {
                self.defs[def as usize].inst
            };
            self.bind(site, inst, allocator.is_none(), block);
        }
        if let Some(effects) = allocator {
            self.apply(site, &name, effects);
        }
    }

    fn making_at_site(&mut self, site: SiteId) {
        let site = &self.sites[site as usize];
        self.making_at = (site.caller, site.line);
    }

    /// What each object a call's callee points to calls.
    pub(crate) fn call_target(&mut self, site: SiteId, loc: LocId) {
        let obj = self.locs[loc as usize].0;
        match self.objects[obj as usize].kind {
            ObjKind::Function(f) => self.call_function(site, f, false),
            ObjKind::Library(library) => self.call_library(site, library),
            ObjKind::Outside => {
                self.sites[site as usize].anything = true;
                let unknown = self.libraries.unknown;
                self.call_library(site, unknown);
            }
            _ => self.sites[site as usize].data = true,
        }
    }

    /// Passes a call's arguments to an instance's parameters and its
    /// result to the call's destination.
    /// Binds a call to an instance: its arguments to the parameters but
    /// `block`, a block given back to an allocator, and its result where
    /// `result` keeps it.
    fn bind(&mut self, site: SiteId, inst: InstId, result: bool, block: Option<usize>) {
        self.making_at_site(site);
        let info = &self.insts[inst as usize];
        let (def, registers, memory, ret_node, ret_obj, varargs) = (
            info.def,
            info.registers,
            info.memory,
            info.ret_node,
            info.ret_obj,
            info.varargs,
        );
        let params = self.defs[def as usize].params.clone();
        let slots = self.defs[def as usize].slots.clone();
        let ret = self.defs[def as usize].ret;
        let args = self.sites[site as usize].args.clone();
        let place = |i: usize| {
            let slot = slots[i];
            if slot & MEMORY == 0 {
                Ok(registers + slot)
            } else {
                Err(memory + (slot & !MEMORY))
            }
        };
        for (i, &arg) in args.iter().enumerate() {
            let mut arg = arg;
            if let Some(library) = arg.own {
                let ty = params.get(i).map_or(NONE, |param| param.ty);
                let given = self.library_gives_at(library, ty);
                arg.val = self.materialize(&[Ptr::Node(arg.val, COPY), Ptr::Node(given, COPY)]);
            }
            if arg.val == NONE || block == Some(i) {
                continue;
            }
            if i < params.len() {
                self.bind_param(place(i), &arg, &params[i]);
            } else if varargs != NONE {
                let loc = self.loc(varargs, Off::Any);
                match arg.record {
                    Some(len) => {
                        self.copy_bytes(&[Ptr::Node(arg.val, COPY)], &[Ptr::Loc(loc)], len, true);
                    }
                    None => {
                        let part = self.vararg_part(arg.ty);
                        self.write_loc(loc, arg.val, arg.width, part);
                    }
                }
            }
        }
        if let Some(library) = self.sites[site as usize].given_by {
            for (i, param) in params.iter().enumerate() {
                let Some(ty) = self.ty(param.ty) else {
                    continue;
                };
                match place(i) {
                    Err(obj) if param.record.is_some() => {
                        let loc = self.loc(obj, Off::Exact(0));
                        let at = self.const_node(loc);
                        let class = self.pointer_to(library, &ty);
                        self.lib_access(library, at, class, Extent::One, false, true);
                    }
                    place => {
                        let Some(class) = self.class_of(library, &ty) else {
                            continue;
                        };
                        let gives = self.lib_gives(library, class);
                        match place {
                            Ok(node) => self.add_edge(gives, node, COPY),
                            Err(obj) => {
                                let loc = self.loc(obj, Off::Exact(0));
                                self.write_loc(loc, gives, 8, 0);
                            }
                        }
                    }
                }
            }
            if varargs != NONE {
                let void = self.void_class();
                let gives = self.lib_gives(library, void);
                let loc = self.loc(varargs, Off::Any);
                self.write_loc(loc, gives, 8, 0);
            }
            let ret_ty = self.defs[def as usize].ret_ty;
            if result && let Some(ty) = self.ty(ret_ty) {
                match ret {
                    Ret::Scalar => {
                        if let Some(class) = self.class_of(library, &ty) {
                            let held = self.lib_held(library, class, Extent::One);
                            self.add_edge(ret_node, held, COPY);
                        }
                    }
                    Ret::Record(_) if ret_obj != NONE => {
                        let loc = self.loc(ret_obj, Off::Exact(0));
                        let at = self.const_node(loc);
                        let class = self.pointer_to(library, &ty);
                        self.lib_access(library, at, class, Extent::One, true, false);
                    }
                    _ => {}
                }
            }
        }
        if result {
            match (self.sites[site as usize].dst, ret) {
                (Dst::Node(dst), Ret::Scalar) => self.add_edge(ret_node, dst, COPY),
                (Dst::Node(dst), Ret::Record(_)) if ret_obj != NONE => {
                    let loc = self.loc(ret_obj, Off::Any);
                    if let Some(read) = self.read_loc(loc, 8, 0) {
                        self.add_edge(read, dst, TO_ANY);
                    }
                }
                (Dst::Record(node, len), Ret::Record(_)) if ret_obj != NONE => {
                    let loc = self.loc(ret_obj, Off::Exact(0));
                    self.copy_bytes(&[Ptr::Loc(loc)], &[Ptr::Node(node, COPY)], len, true);
                }
                (Dst::Record(node, _), Ret::Scalar) => self.add_cons(
                    node,
                    Cons::Store {
                        tr: TO_ANY,
                        width: 8,
                        part: 0,
                        src: ret_node,
                    },
                ),
                _ => {}
            }
        }
        if let Some(caller) = self.sites[site as usize].caller {
            let from = self.insts[caller as usize].def;
            self.call_edges.insert((from, def));
        }
    }

    fn bind_param(&mut self, place: Result<NodeId, ObjId>, arg: &Arg, param: &Param) {
        match place {
            Ok(node) => match arg.record {
                Some(_) => {
                    let read = self.load_node(arg.val, TO_ANY, 8, 0);
                    self.add_edge(read, node, COPY);
                }
                None => self.add_edge(arg.val, node, COPY),
            },
            Err(obj) => {
                let loc = self.loc(obj, Off::Exact(0));
                match arg.record {
                    Some(len) => {
                        let len = match param.record {
                            Some(size) if size != TO_END => size.min(len),
                            _ => len,
                        };
                        self.copy_bytes(&[Ptr::Node(arg.val, COPY)], &[Ptr::Loc(loc)], len, true);
                    }
                    None => self.write_loc(loc, arg.val, param.width, 0),
                }
            }
        }
    }

    /// A call reaching a library: it holds what it is given by the
    /// arguments' classes, and the call keeps what it gives at the
    /// result's.
    fn call_library(&mut self, site: SiteId, library: LibId) {
        if !self.sites[site as usize]
            .bound
            .insert(Target::Library(library))
        {
            return;
        }
        if let Some(from) = self.sites[site as usize].given_by {
            let void = self.void_class();
            let gives = self.lib_gives(from, void);
            let held = self.lib_held(library, void, Extent::One);
            self.add_edge(gives, held, COPY);
        }
        let args = self.sites[site as usize].args.clone();
        for (i, arg) in args.iter().enumerate() {
            self.library_arg(library, arg, args.get(i + 1));
        }
        let (dst, result) = {
            let site = &self.sites[site as usize];
            (site.dst, site.result)
        };
        let Some(ty) = self.ty(result) else {
            return;
        };
        match dst {
            Dst::Node(node) => {
                if let Some(class) = self.class_of(library, &ty) {
                    let gives = self.lib_gives(library, class);
                    self.add_edge(gives, node, COPY);
                }
            }
            Dst::Record(node, _) => {
                let class = self.pointer_to(library, &ty);
                self.lib_access(library, node, class, Extent::One, false, true);
            }
            Dst::None => {}
        }
    }

    /// An argument a library is given: held by its class, reaching every
    /// element where an integer argument after it counts them, and the
    /// bytes it gives where constant; a record passed by value read member
    /// by member.
    fn library_arg(&mut self, library: LibId, arg: &Arg, next: Option<&Arg>) {
        if let Some(own) = arg.own {
            let void = self.void_class();
            let gives = self.lib_gives(own, void);
            let held = self.lib_held(library, void, Extent::One);
            self.add_edge(gives, held, COPY);
        }
        if arg.val == NONE {
            return;
        }
        let Some(ty) = self.ty(arg.ty) else {
            let void = self.void_class();
            let held = self.lib_held(library, void, Extent::One);
            self.add_edge(arg.val, held, COPY);
            return;
        };
        if arg.record.is_some() {
            let class = self.pointer_to(library, &ty);
            self.lib_access(library, arg.val, class, Extent::One, true, false);
            return;
        }
        let Some(mut class) = self.class_of(library, &ty) else {
            return;
        };
        let count = match next {
            Some(next) if next.record.is_none() => match self.ty(next.ty) {
                Some(next_ty) => (self.types.shape(&next_ty) == Shape::Integer).then_some(next),
                None => None,
            },
            _ => None,
        };
        let info = self.class_info(class).clone();
        if info.to == Pointee::Void && count.is_some() {
            class = self.class(Class {
                to: Pointee::Bytes,
                konst: info.konst,
            });
        }
        let extent = match (&self.class_info(class).to, count) {
            (Pointee::Bytes, Some(count)) => match count
                .constant
                .and_then(|len| u64::try_from(len).ok())
                .filter(|&len| len > 0)
            {
                Some(len) => Extent::Len(len),
                None => Extent::One,
            },
            (Pointee::Record(_) | Pointee::Pointer(_), Some(_)) => Extent::Array,
            _ => Extent::One,
        };
        let held = self.lib_held(library, class, extent);
        self.add_edge(arg.val, held, COPY);
    }

    /// What a library gives at a type's class: none where it holds no
    /// pointer, what it gives at `void` where the type is not known.
    fn library_gives_at(&mut self, library: LibId, ty: TyId) -> NodeId {
        let class = match self.ty(ty) {
            Some(ty) => self.class_of(library, &ty),
            None => Some(self.void_class()),
        };
        match class {
            Some(class) => self.lib_gives(library, class),
            None => NONE,
        }
    }

    /// An argument's node and class at a library's bounds.
    fn arg_class(&mut self, library: LibId, args: &[Arg], i: usize) -> Option<(NodeId, ClassId)> {
        let arg = args.get(i).filter(|arg| arg.val != NONE)?;
        let ty = self.ty(arg.ty)?;
        let class = self.class_of(library, &ty)?;
        Some((arg.val, class))
    }

    /// A call of a function the build has no source of: its effects where
    /// the tables know them, else a call of each library declaring it.
    fn external(&mut self, site: SiteId, name: &FnName) {
        self.making_at_site(site);
        if let Some(effects) = externals::effects(name) {
            self.apply(site, name, effects);
            return;
        }
        let libraries = match self.libraries.functions.get(name) {
            Some(libraries) => libraries.clone(),
            None => vec![self.libraries.unknown],
        };
        let entry = self
            .unmodelled
            .entry(name.clone())
            .or_insert_with(|| (libraries.clone(), 0));
        entry.1 += 1;
        for library in libraries {
            self.call_library(site, library);
        }
    }

    /// Gives a call's destination a value.
    fn give(&mut self, site: SiteId, val: &[Ptr]) {
        match self.sites[site as usize].dst {
            Dst::None => {}
            Dst::Node(dst) => self.feed(val, dst),
            Dst::Record(node, _) => {
                let src = self.materialize(val);
                if src != NONE {
                    self.add_cons(
                        node,
                        Cons::Store {
                            tr: TO_ANY,
                            width: 8,
                            part: 0,
                            src,
                        },
                    );
                }
            }
        }
    }

    /// The memory a call allocates, one object for the call.
    /// The object a call allocates: one for all the analyses of a
    /// wrapper's that share the call's objects.
    fn heap(&mut self, site: SiteId, name: &str) -> LocId {
        let shared = self.sites[site as usize].shared;
        let found = match shared {
            Some(key) => self.shared_heaps.get(&key),
            None => self.heaps.get(&site),
        };
        let obj = match found {
            Some(&obj) => obj,
            None => {
                let size = allocation_size(name, &self.sites[site as usize].args);
                let obj = self.object(ObjKind::Heap(site), size);
                match shared {
                    Some(key) => self.shared_heaps.insert(key, obj),
                    None => self.heaps.insert(site, obj),
                };
                obj
            }
        };
        self.loc(obj, Off::Exact(0))
    }

    /// Fills a heap object with bytes from outside the process.
    fn outside_into(&mut self, loc: LocId) {
        let obj = self.locs[loc as usize].0;
        let any = self.loc(obj, Off::Any);
        self.write_loc(any, self.outside_node, 8, 0);
    }

    fn apply(&mut self, site: SiteId, name: &str, effects: &[Effect]) {
        self.making_at_site(site);
        let libc = self.libraries.libc;
        let args = self.sites[site as usize].args.clone();
        let arg = |i: usize| args.get(i).map_or(NONE, |arg| arg.val);
        for effect in effects {
            match *effect {
                Effect::Gives(i) => {
                    if arg(i) != NONE {
                        self.give(site, &[Ptr::Node(arg(i), TO_ANY)]);
                    }
                }
                Effect::GivesHeld(i) => {
                    if arg(i) != NONE {
                        let read = self.load_node(arg(i), COPY, 8, 0);
                        self.give(site, &[Ptr::Node(read, COPY)]);
                    }
                }
                Effect::Allocates => {
                    let heap = self.heap(site, name);
                    self.give(site, &[Ptr::Loc(heap)]);
                }
                Effect::Block(_) => {}
                Effect::AllocatesOutside => {
                    let heap = self.heap(site, name);
                    self.outside_into(heap);
                    self.give(site, &[Ptr::Loc(heap)]);
                }
                Effect::Maps { flags } => {
                    let anonymous = args
                        .get(flags)
                        .and_then(|arg| arg.constant)
                        .is_some_and(|flags| flags & 0x20 != 0);
                    let heap = self.heap(site, name);
                    if !anonymous {
                        self.outside_into(heap);
                    }
                    self.give(site, &[Ptr::Loc(heap)]);
                }
                Effect::Grows(i) => {
                    let heap = self.heap(site, name);
                    if arg(i) != NONE {
                        self.add_cons(
                            arg(i),
                            Cons::Gather {
                                tr: COPY,
                                len: TO_END,
                                preserve: true,
                                dst: heap,
                            },
                        );
                    }
                    self.give(site, &[Ptr::Loc(heap)]);
                }
                Effect::GivesOwn => {
                    let result = self.sites[site as usize].result;
                    let given = self.library_gives_at(libc, result);
                    if given != NONE {
                        self.give(site, &[Ptr::Node(given, COPY)]);
                    }
                }
                Effect::Derives => {
                    let val: Val = args
                        .iter()
                        .filter(|arg| arg.val != NONE)
                        .map(|arg| Ptr::Node(arg.val, TO_ANY))
                        .collect();
                    self.give(site, &val);
                }
                Effect::GivesLoaded => {
                    let loaded = self.libraries.loaded;
                    let code = self.lib_code(loaded);
                    let void = self.void_class();
                    let gives = self.lib_gives(loaded, void);
                    self.give(site, &[Ptr::Loc(code), Ptr::Node(gives, COPY)]);
                }
                Effect::Copies { into, from, len } => {
                    if arg(into) != NONE && arg(from) != NONE {
                        let len = len
                            .and_then(|i| args.get(i)?.constant)
                            .and_then(|len| u64::try_from(len).ok())
                            .unwrap_or(TO_END);
                        self.copy_bytes(
                            &[Ptr::Node(arg(from), COPY)],
                            &[Ptr::Node(arg(into), COPY)],
                            len,
                            false,
                        );
                    }
                }
                Effect::WritesInto { through, from } => {
                    if arg(through) != NONE {
                        self.store(&[Ptr::Node(arg(through), COPY)], arg(from), 8, 0);
                    }
                }
                Effect::Fills { through, from } => {
                    if arg(through) != NONE {
                        self.store(&[Ptr::Node(arg(through), TO_ANY)], arg(from), 8, 0);
                    }
                }
                Effect::WritesOutside { arg: i, depth } => {
                    let mut at = arg(i);
                    if at == NONE {
                        continue;
                    }
                    for _ in 0..depth {
                        at = self.load_node(at, TO_ANY, 8, 0);
                    }
                    self.store(&[Ptr::Node(at, TO_ANY)], self.outside_node, 8, 0);
                }
                Effect::WritesNew(i) => {
                    if arg(i) != NONE {
                        let heap = self.heap(site, name);
                        let node = self.const_node(heap);
                        self.store(&[Ptr::Node(arg(i), COPY)], node, 8, 0);
                    }
                }
                Effect::WritesOwn(i) => {
                    if let Some((at, class)) = self.arg_class(libc, &args, i) {
                        self.lib_access(libc, at, class, Extent::One, false, true);
                    }
                }
                Effect::Holds(i) => {
                    if let Some((at, class)) = self.arg_class(libc, &args, i) {
                        let held = self.lib_held(libc, class, Extent::One);
                        self.add_edge(at, held, COPY);
                    }
                }
                Effect::HoldsHeld(i) => {
                    if let Some((at, class)) = self.arg_class(libc, &args, i) {
                        self.lib_access(libc, at, class, Extent::One, true, false);
                    }
                }
                Effect::WritesHeld(i) => {
                    if let Some((at, class)) = self.arg_class(libc, &args, i) {
                        let counted = args.get(i + 1).is_some_and(|next| {
                            next.record.is_none()
                                && self
                                    .ty(next.ty)
                                    .is_some_and(|ty| self.types.shape(&ty) == Shape::Integer)
                        });
                        let extent = if counted { Extent::Array } else { Extent::One };
                        self.lib_access(libc, at, class, extent, false, true);
                    }
                }
                Effect::Calls {
                    callee,
                    args: sources,
                } => {
                    self.callback(site, arg(callee), sources, SiteKind::Callback);
                }
                Effect::Keeps {
                    callee,
                    args: sources,
                    when,
                } => {
                    if arg(callee) != NONE {
                        let held = self.lib_calls(libc);
                        self.add_edge(arg(callee), held, COPY);
                    }
                    self.callback(site, arg(callee), sources, SiteKind::Later(when));
                }
                Effect::KeepsMembers {
                    arg: i,
                    record,
                    when,
                } => {
                    if arg(i) != NONE {
                        self.keep_members(site, arg(i), record, when);
                    }
                }
                Effect::StartsVaList(i) => {
                    let varargs = self.sites[site as usize]
                        .caller
                        .map_or(NONE, |caller| self.insts[caller as usize].varargs);
                    if arg(i) != NONE && varargs != NONE {
                        let loc = self.loc(varargs, Off::Any);
                        let node = self.const_node(loc);
                        self.store(&[Ptr::Node(arg(i), TO_ANY)], node, 8, 0);
                    }
                }
                Effect::CopiesVaList { into, from } => {
                    if arg(into) != NONE && arg(from) != NONE {
                        let read = self.load_node(arg(from), TO_ANY, 8, 0);
                        self.store(&[Ptr::Node(arg(into), TO_ANY)], read, 8, 0);
                    }
                }
                Effect::Runs(when) => {
                    let runs = &mut self.sites[site as usize].runs;
                    if !runs.contains(&when) {
                        runs.push(when);
                    }
                }
            }
        }
    }

    /// A call the C library makes of a function a call of it was given:
    /// now, or kept for later.
    fn callback(&mut self, parent: SiteId, callee: NodeId, sources: &[&[Src]], kind: SiteKind) {
        if callee == NONE {
            return;
        }
        let parent_args = self.sites[parent as usize].args.clone();
        let mut args = Vec::with_capacity(sources.len());
        for sources in sources {
            let mut val = Vec::new();
            let mut own = None;
            for source in *sources {
                match *source {
                    Src::Arg(i) => {
                        if let Some(arg) = parent_args.get(i)
                            && arg.val != NONE
                        {
                            val.push(Ptr::Node(arg.val, COPY));
                        }
                    }
                    Src::Into(i, size) => {
                        if let Some(arg) = parent_args.get(i)
                            && arg.val != NONE
                        {
                            let size = size
                                .and_then(|s| parent_args.get(s)?.constant)
                                .and_then(|size| u64::try_from(size).ok())
                                .filter(|&size| size > 0);
                            let tr = match size {
                                Some(size) => self.tr(Transform::linear(0, size)),
                                None => TO_ANY,
                            };
                            val.push(Ptr::Node(arg.val, tr));
                        }
                    }
                    Src::Own => own = Some(self.libraries.libc),
                }
            }
            let node = self.materialize(&val);
            args.push(Arg {
                val: node,
                record: None,
                ty: NONE,
                width: 8,
                constant: None,
                own,
            });
        }
        let (caller, line) = {
            let parent = &self.sites[parent as usize];
            (parent.caller, parent.line)
        };
        let mut site = Site::new(caller, line, kind);
        site.parent = Some(parent);
        site.callee = Some(callee);
        site.args = args;
        let site = self.new_site(site);
        self.add_cons(callee, Cons::Call { site });
    }

    /// The functions the members of a record the C library keeps hold run
    /// later, with what it gives.
    fn keep_members(&mut self, parent: SiteId, pointer: NodeId, record: &str, when: super::Later) {
        let record = self.types.canonical(&Arc::from(record));
        let mut offsets = Vec::new();
        self.function_members(&record, 0, 0, &mut offsets);
        offsets.sort_unstable();
        offsets.dedup();
        let functions = self.new_node(true);
        for offset in offsets {
            let tr = self.tr(Transform::linear(offset, 0));
            let read = self.load_node(pointer, tr, 8, 0);
            self.add_edge(read, functions, COPY);
        }
        let libc = self.libraries.libc;
        let held = self.lib_calls(libc);
        self.add_edge(functions, held, COPY);
        let (caller, line) = {
            let parent = &self.sites[parent as usize];
            (parent.caller, parent.line)
        };
        let mut site = Site::new(caller, line, SiteKind::Later(when));
        site.parent = Some(parent);
        site.callee = Some(functions);
        site.given_by = Some(libc);
        let site = self.new_site(site);
        self.add_cons(functions, Cons::Call { site });
        self.kept_members.push((when, functions));
    }

    /// Where a record's members holding functions are, each element of an
    /// array apart, through the records it holds.
    fn function_members(&mut self, record: &Arc<str>, at: i64, depth: u32, out: &mut Vec<i64>) {
        if depth > 16 {
            return;
        }
        let Some(layout) = self.types.layout(record) else {
            return;
        };
        for field in &layout.fields {
            let start = at + field.offset as i64;
            let size = field.size.unwrap_or(8) as i64;
            if field.calls {
                let mut offset = start;
                while offset < start + size.max(8) {
                    out.push(offset);
                    offset += 8;
                }
                continue;
            }
            let Some(held) = &field.holds else {
                continue;
            };
            let held = self.types.canonical(held);
            let Some(element) = self.types.record_size(&held).filter(|&s| s > 0) else {
                continue;
            };
            let mut offset = start;
            while offset + element as i64 <= start + size {
                self.function_members(&held, offset, depth + 1, out);
                offset += element as i64;
            }
        }
    }
}

/// How many bytes an allocator's call allocates, where its arguments
/// tell.
fn allocation_size(name: &str, args: &[Arg]) -> Option<u64> {
    let at = |i: usize| {
        args.get(i)
            .and_then(|arg| arg.constant)
            .and_then(|value| u64::try_from(value).ok())
    };
    if let Some(size) = name
        .strip_prefix("_emalloc_")
        .and_then(|size| size.parse::<u64>().ok())
    {
        return Some(size);
    }
    match name {
        "malloc" | "_emalloc" | "__zend_malloc" | "_emalloc_large" | "_emalloc_huge" | "alloca"
        | "valloc" | "pvalloc" | "__builtin_alloca" => at(0),
        "calloc" | "_ecalloc" | "__zend_calloc" => at(0)?.checked_mul(at(1)?),
        "Zend/zend_arena.h:zend_arena_alloc" => at(1),
        "Zend/zend_arena.h:zend_arena_calloc" => at(1)?.checked_mul(at(2)?),
        "_safe_emalloc" | "_safe_malloc" => at(0)?.checked_mul(at(1)?)?.checked_add(at(2)?),
        "realloc" | "_erealloc" | "__zend_realloc" | "aligned_alloc" | "_zend_mm_alloc"
        | "memalign" => at(1),
        _ => None,
    }
}

/// The variables a value is computed from by arithmetic, not by reading
/// memory.
fn computed_from(value: &Expr, vars: &mut Vec<VarId>) {
    match &value.kind {
        ExprKind::Use(Place::Var(var)) => vars.push(*var),
        ExprKind::Cast(inner) | ExprKind::Unary(_, inner) => computed_from(inner, vars),
        ExprKind::Binary(_, left, right) | ExprKind::Offset(left, right) => {
            computed_from(left, vars);
            computed_from(right, vars);
        }
        _ => {}
    }
}

/// Every variable a value reads.
fn mentioned(value: &Expr, vars: &mut Vec<VarId>) {
    match &value.kind {
        ExprKind::Use(place) | ExprKind::AddrOf(place) => place_mentioned(place, vars),
        ExprKind::Cast(inner) | ExprKind::Unary(_, inner) | ExprKind::VaArg(inner) => {
            mentioned(inner, vars);
        }
        ExprKind::Binary(_, left, right) | ExprKind::Offset(left, right) => {
            mentioned(left, vars);
            mentioned(right, vars);
        }
        _ => {}
    }
}

/// Every variable a place is or its address reads.
fn place_mentioned(place: &Place, vars: &mut Vec<VarId>) {
    match place {
        Place::Var(var) => vars.push(*var),
        Place::Global(_) => {}
        Place::Deref(pointer) => mentioned(pointer, vars),
        Place::Field(inner, _) => place_mentioned(inner, vars),
        Place::Index(inner, index) => {
            place_mentioned(inner, vars);
            mentioned(index, vars);
        }
    }
}
