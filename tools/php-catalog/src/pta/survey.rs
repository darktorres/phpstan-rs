//! The first read of the program, each function loaded from the store in
//! turn: each definition's variables and parameters, the functions whose
//! address is taken, what each definition writes and calls, the
//! definitions that may return only memory their calls allocate, the
//! functions returning a member of what their first parameter points to,
//! the functions returning only a few constants, and which bytes of
//! records the conditions compare with constants and which constants are
//! written into them.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{DefId, DefInfo, FnId, MEMORY, NONE, Param, Pta, Ret, TO_END};
use crate::ir::{
    BinOp, Callee, Expr, ExprKind, FnName, Function, Init, Place, Stmt, Term, UnOp, VarId,
};
use crate::lower::{convert_to, int_op};
use crate::program::Definition;
use crate::store::Store;
use crate::types::Shape;

/// How many values a value known to be one of a few constants may have.
pub(crate) const FEW: usize = 8;

/// An integer's width in bits and whether it is signed.
pub(crate) type Conversion = (u32, bool);

/// The values a value may have where they are a few constants and the
/// results of direct calls: each call with the conversions its result
/// goes through, in order.
#[derive(Debug, Clone, Default)]
pub(crate) struct Values {
    pub(crate) consts: Vec<i128>,
    pub(crate) calls: Vec<(FnId, Vec<Conversion>)>,
}

impl Values {
    fn constant(value: i128) -> Values {
        Values {
            consts: vec![value],
            calls: Vec::new(),
        }
    }

    /// Adds another's values: none where they are too many.
    fn union(mut self, other: Values) -> Option<Values> {
        self.consts.extend(other.consts);
        self.calls.extend(other.calls);
        self.consts = few(self.consts)?;
        self.calls.sort_unstable();
        self.calls.dedup();
        (self.calls.len() <= FEW).then_some(self)
    }

    /// The values converted to an integer type, or kept where it is an
    /// enumeration.
    fn convert(mut self, conversion: Option<Conversion>) -> Option<Values> {
        let Some((bits, signed)) = conversion else {
            return Some(self);
        };
        for value in &mut self.consts {
            *value = convert_to(*value, bits, signed);
        }
        for (_, conversions) in &mut self.calls {
            conversions.push((bits, signed));
        }
        self.consts = few(self.consts)?;
        Some(self)
    }
}

/// Values sorted and rid of repeats, where they are few.
pub(crate) fn few(mut values: Vec<i128>) -> Option<Vec<i128>> {
    values.sort_unstable();
    values.dedup();
    (values.len() <= FEW).then_some(values)
}

/// The values of an operation on each pair of values, where C defines
/// them all and they are few.
pub(crate) fn fold_binary(op: BinOp, left: &[i128], right: &[i128]) -> Option<Vec<i128>> {
    let mut out = Vec::with_capacity(left.len() * right.len());
    for &l in left {
        for &r in right {
            out.push(int_op(op, l, r)?);
        }
    }
    few(out)
}

pub(crate) fn fold_unary(op: UnOp, values: &[i128]) -> Vec<i128> {
    let mut out: Vec<i128> = values
        .iter()
        .map(|&value| match op {
            UnOp::Neg => value.wrapping_neg(),
            UnOp::BitNot => !value,
            UnOp::Not => i128::from(value == 0),
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// What a definition writes and calls.
pub(crate) struct DefFacts {
    /// Whether it writes no memory but its own variables' and runs no
    /// assembly.
    pub(crate) local_only: bool,
    pub(crate) callees: Box<[FnId]>,
    /// Whether it calls through a pointer.
    pub(crate) indirect: bool,
}

/// A function returning the bytes of one member of what its first
/// parameter points to: the records the member is in, each with where
/// its instance starts, where the bytes start and how many they are,
/// from where the parameter points.
#[derive(Debug, Clone)]
pub(crate) struct Accessor {
    pub(crate) chain: Vec<(Arc<str>, i64)>,
    pub(crate) start: i64,
    pub(crate) width: u32,
}

/// A write into records' bytes of values a call gives: the records,
/// where the bytes start, how many they are, and the values.
type LaterWrite = (Vec<(Arc<str>, i64)>, i64, i64, Values);

#[derive(Default)]
pub(crate) struct Survey {
    pub(crate) facts: Vec<DefFacts>,
    /// The definitions that may return only memory their calls allocate,
    /// with their code and the functions those calls name.
    pub(crate) candidates: Vec<(DefId, Arc<Function>, Box<[FnId]>)>,
    pub(crate) accessors: FxHashMap<FnId, Accessor>,
    /// The functions whose results conditions compare.
    pub(crate) accessor_conditions: FxHashSet<FnId>,
    /// The bytes of records that conditions compare with constants: the
    /// record and where the byte is in it.
    pub(crate) compared: FxHashSet<(Arc<str>, i64)>,
    /// The byte values written as constants into the records' bytes, as
    /// 256 bits.
    pub(crate) written: FxHashMap<(Arc<str>, i64), [u64; 4]>,
    /// The records that may carry a tag: a struct with a union member
    /// that may hold a pointer, such a union, and the structs that are its
    /// members.
    pub(crate) capable: FxHashSet<Arc<str>>,
    /// Each struct that is a member of such a union: the unions and where
    /// the struct is in each.
    pub(crate) variants: FxHashMap<Arc<str>, Vec<(Arc<str>, i64)>>,
    /// What each definition returns, where it is a few constants and the
    /// results of direct calls.
    returned: Vec<(FnId, Option<Values>)>,
    /// The writes into records' bytes of values that calls give: the
    /// records, where the bytes start, how many they are, and the values.
    written_later: Vec<LaterWrite>,
    /// The functions that return only a few constants, with them.
    pub(crate) returns: FxHashMap<FnId, Vec<i128>>,
    /// The bytes each definition writes but its register variables', by
    /// definition.
    pub(crate) spans: Vec<Box<[Span]>>,
}

/// Bytes a write writes: a value of a record or array type, members of
/// records (the records and where each starts, where the bytes start
/// and how many), or any memory.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Span {
    Whole(Arc<str>),
    Members(Box<[(Arc<str>, i64)]>, i64, u32),
    Any,
}

/// What a place's bytes are reached from.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Base<'f> {
    Var(VarId),
    Global(&'f Arc<str>),
    /// The memory a pointer points to.
    Pointer(&'f Expr),
    /// An element of an array indexed by a value that is not constant.
    Element(&'f Place),
}

/// Where a place's bytes are: from what base, at which offset from it,
/// and in which records, each with where its instance starts.
#[derive(Debug, Clone)]
pub(crate) struct Located<'f> {
    pub(crate) base: Base<'f>,
    pub(crate) start: i64,
    pub(crate) chain: Vec<(Arc<str>, i64)>,
}

pub(crate) fn locate(place: &Place) -> Option<Located<'_>> {
    let at = |base| Located {
        base,
        start: 0,
        chain: Vec::new(),
    };
    match place {
        Place::Var(var) => Some(at(Base::Var(*var))),
        Place::Global(name) => Some(at(Base::Global(name))),
        Place::Deref(pointer) => Some(at(Base::Pointer(pointer))),
        Place::Field(inner, field) => {
            let mut located = locate(inner)?;
            let offset = i64::try_from(field.offset?).ok()?;
            located.chain.push((field.record.clone(), located.start));
            located.start = located.start.checked_add(offset)?;
            Some(located)
        }
        Place::Index(inner, bytes) => {
            let mut located = locate(inner)?;
            match constant(bytes).and_then(|bytes| i64::try_from(bytes).ok()) {
                Some(bytes) => located.start = located.start.checked_add(bytes)?,
                None => located = at(Base::Element(place)),
            }
            Some(located)
        }
    }
}

/// An expression's value where it is a constant.
pub(crate) fn constant(expr: &Expr) -> Option<i128> {
    match &expr.kind {
        ExprKind::Int(value) => Some(*value),
        ExprKind::Cast(inner) => constant(inner),
        _ => None,
    }
}

pub(crate) fn strip_casts(mut expr: &Expr) -> &Expr {
    while let ExprKind::Cast(inner) = &expr.kind {
        expr = inner;
    }
    expr
}

/// An operand without its conversions and its masks by a constant.
fn strip_value(mut expr: &Expr) -> &Expr {
    loop {
        match &expr.kind {
            ExprKind::Cast(inner) => expr = inner,
            ExprKind::Binary(BinOp::BitAnd, left, right) if constant(right).is_some() => {
                expr = left;
            }
            ExprKind::Binary(BinOp::BitAnd, left, right) if constant(left).is_some() => {
                expr = right;
            }
            _ => return expr,
        }
    }
}

pub(crate) fn is_comparison(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
    )
}

/// The operand a condition compares with a constant, or tests against
/// zero.
fn compared_operand(mut cond: &Expr) -> Option<&Expr> {
    loop {
        match &cond.kind {
            ExprKind::Unary(UnOp::Not, inner) | ExprKind::Cast(inner) => cond = inner,
            ExprKind::Binary(op, left, right) if is_comparison(*op) => {
                if constant(right).is_some() {
                    return Some(strip_value(left));
                }
                if constant(left).is_some() {
                    return Some(strip_value(right));
                }
                return None;
            }
            _ => return Some(strip_value(cond)),
        }
    }
}

/// The variable a place's bytes are in, through its members and
/// elements.
pub(crate) fn root_var(place: &Place) -> Option<VarId> {
    match place {
        Place::Var(var) => Some(*var),
        Place::Field(inner, _) | Place::Index(inner, _) => root_var(inner),
        Place::Global(_) | Place::Deref(_) => None,
    }
}

/// Whether a write of a place writes only the function's own variables.
fn writes_local(place: &Place) -> bool {
    match place {
        Place::Var(_) => true,
        Place::Field(inner, _) | Place::Index(inner, _) => writes_local(inner),
        Place::Global(_) | Place::Deref(_) => false,
    }
}

/// A walk over a function's expressions and places.
pub(crate) trait Visit<'f> {
    fn expr(&mut self, _expr: &'f Expr) {}
    fn place(&mut self, _place: &'f Place) {}
}

pub(crate) fn walk_expr<'f>(visit: &mut impl Visit<'f>, expr: &'f Expr) {
    visit.expr(expr);
    match &expr.kind {
        ExprKind::Use(place) | ExprKind::AddrOf(place) => walk_place(visit, place),
        ExprKind::Unary(_, inner) | ExprKind::Cast(inner) | ExprKind::VaArg(inner) => {
            walk_expr(visit, inner);
        }
        ExprKind::Binary(_, left, right) | ExprKind::Offset(left, right) => {
            walk_expr(visit, left);
            walk_expr(visit, right);
        }
        ExprKind::Int(_)
        | ExprKind::Float
        | ExprKind::Str(_)
        | ExprKind::Func(_)
        | ExprKind::SizeOf(_)
        | ExprKind::AlignOf(_)
        | ExprKind::Unknown(_) => {}
    }
}

pub(crate) fn walk_place<'f>(visit: &mut impl Visit<'f>, place: &'f Place) {
    visit.place(place);
    match place {
        Place::Deref(pointer) => walk_expr(visit, pointer),
        Place::Field(inner, _) => walk_place(visit, inner),
        Place::Index(inner, bytes) => {
            walk_place(visit, inner);
            walk_expr(visit, bytes);
        }
        Place::Var(_) | Place::Global(_) => {}
    }
}

pub(crate) fn walk_stmt<'f>(visit: &mut impl Visit<'f>, stmt: &'f Stmt) {
    match stmt {
        Stmt::Assign { dst, src, .. } => {
            walk_place(visit, dst);
            walk_expr(visit, src);
        }
        Stmt::Call {
            dst, callee, args, ..
        } => {
            if let Some(dst) = dst {
                walk_place(visit, dst);
            }
            if let Callee::Indirect(callee) = callee {
                walk_expr(visit, callee);
            }
            for arg in args {
                walk_expr(visit, arg);
            }
        }
        Stmt::Asm { writes, reads, .. } => {
            for write in writes {
                walk_place(visit, write);
            }
            for read in reads {
                walk_expr(visit, read);
            }
        }
    }
}

pub(crate) fn walk_term<'f>(visit: &mut impl Visit<'f>, term: &'f Term) {
    match term {
        Term::Branch { cond, .. } => walk_expr(visit, cond),
        Term::Switch { on, .. } => walk_expr(visit, on),
        Term::Return(Some(value)) => walk_expr(visit, value),
        Term::Goto(_) | Term::Return(None) | Term::Unreachable | Term::IndirectGoto(_) => {}
    }
}

pub(crate) fn walk_function<'f>(visit: &mut impl Visit<'f>, function: &'f Function) {
    for block in &function.blocks {
        for stmt in &block.stmts {
            walk_stmt(visit, stmt);
        }
        walk_term(visit, &block.term);
    }
}

pub(crate) fn walk_init<'f>(visit: &mut impl Visit<'f>, init: &'f Init) {
    match init {
        Init::Expr(expr) => walk_expr(visit, expr),
        Init::List { items, filler, .. } => {
            for item in items {
                walk_init(visit, &item.init);
            }
            if let Some(filler) = filler {
                walk_init(visit, filler);
            }
        }
        Init::Zero => {}
    }
}

/// The variables whose address is taken or whose members are reached,
/// and the functions named other than as a direct call's callee.
struct Uses<'f> {
    address: Vec<bool>,
    member: Vec<bool>,
    functions: Vec<&'f FnName>,
}

impl<'f> Visit<'f> for Uses<'f> {
    fn expr(&mut self, expr: &'f Expr) {
        match &expr.kind {
            ExprKind::AddrOf(place) => {
                if let Some(var) = root_var(place)
                    && let Some(address) = self.address.get_mut(var as usize)
                {
                    *address = true;
                }
            }
            ExprKind::Func(name) => self.functions.push(name),
            _ => {}
        }
    }

    fn place(&mut self, place: &'f Place) {
        if let Place::Field(inner, _) | Place::Index(inner, _) = place
            && let Some(var) = root_var(inner)
            && let Some(member) = self.member.get_mut(var as usize)
        {
            *member = true;
        }
    }
}

/// What a variable is assigned.
enum Assigned<'f> {
    Value(&'f Expr),
    /// A direct call's result.
    Call(&'f FnName),
    Other,
}

impl Pta<'_> {
    /// Reads every definition once, by name, recording what the analysis
    /// needs before it generates the constraints.
    pub(crate) fn survey(&mut self, store: &Store) -> Result<(), String> {
        self.survey_records();
        let program = self.program;
        let mut names: Vec<&FnName> = program.functions.keys().collect();
        names.sort_unstable();
        for name in names {
            let f = self.fn_id(name);
            for definition in &program.functions[name] {
                let function: Function = store.get(definition.hash)?;
                self.stats.bodies += 1;
                self.stats.body_bytes += store.size(definition.hash);
                let def = self.defs.len() as DefId;
                let info = self.def_info(f, definition, &function);
                self.defs.push(info);
                self.fn_defs[f as usize].push(def);
                self.survey_body(def, f, function);
                if def.is_multiple_of(4096) {
                    self.check_memory()?;
                }
            }
        }
        let mut globals: Vec<&Arc<str>> = program.globals.keys().collect();
        globals.sort_unstable();
        for name in globals {
            for global in &program.globals[name] {
                let Some(init) = &global.init else {
                    continue;
                };
                let mut uses = Uses {
                    address: Vec::new(),
                    member: Vec::new(),
                    functions: Vec::new(),
                };
                walk_init(&mut uses, init);
                for function in uses.functions {
                    let f = self.fn_id(function);
                    self.address_taken.insert(f);
                }
                self.survey_init(init, 0, &mut Vec::new());
            }
        }
        let mut compared: Vec<FnId> = self.survey.accessor_conditions.iter().copied().collect();
        compared.sort_unstable();
        for f in compared {
            let Some(accessor) = self.survey.accessors.get(&f) else {
                continue;
            };
            let found: Vec<(Arc<str>, i64)> = accessor
                .chain
                .iter()
                .filter(|(record, _)| self.survey.capable.contains(record))
                .map(|(record, inst)| (record.clone(), accessor.start - inst))
                .collect();
            self.survey.compared.extend(found);
        }
        self.solve_returns();
        for (chain, start, width, values) in std::mem::take(&mut self.survey.written_later) {
            if let Some(values) = self.resolve_values(&values) {
                self.note_written(&chain, start, width, &values);
            }
        }
        self.stats.accessors = self.survey.accessors.len();
        Ok(())
    }

    /// The constants each function returns, where its definitions return
    /// only constants and what calls of such functions return: the least
    /// solution, every function starting with none.
    fn solve_returns(&mut self) {
        let mut summaries: FxHashMap<FnId, Option<Values>> = FxHashMap::default();
        for (f, values) in std::mem::take(&mut self.survey.returned) {
            let merged = match (summaries.remove(&f), values) {
                (None, values) => values,
                (Some(Some(known)), Some(values)) => known.union(values),
                _ => None,
            };
            summaries.insert(f, merged);
        }
        let mut summaries: Vec<(FnId, Values)> = summaries
            .into_iter()
            .filter_map(|(f, values)| Some((f, values?)))
            .collect();
        summaries.sort_unstable_by_key(|&(f, _)| f);
        let mut current: FxHashMap<FnId, Option<Vec<i128>>> = summaries
            .iter()
            .map(|&(f, _)| (f, Some(Vec::new())))
            .collect();
        loop {
            let mut changed = false;
            for (f, values) in &summaries {
                if current[f].is_none() {
                    continue;
                }
                let mut out = values.consts.clone();
                let mut known = true;
                for (callee, conversions) in &values.calls {
                    let Some(Some(returned)) = current.get(callee) else {
                        known = false;
                        break;
                    };
                    out.extend(returned.iter().map(|&value| {
                        conversions.iter().fold(value, |value, &(bits, signed)| {
                            convert_to(value, bits, signed)
                        })
                    }));
                }
                let next = if known { few(out) } else { None };
                if current[f] != next {
                    current.insert(*f, next);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        self.survey.returns = current
            .into_iter()
            .filter_map(|(f, values)| Some((f, values?)))
            .collect();
        self.stats.constant_returns = self.survey.returns.len();
    }

    /// The constants values stand for, once the functions' returns are
    /// known: none where a call's are not.
    pub(crate) fn resolve_values(&self, values: &Values) -> Option<Vec<i128>> {
        let mut out = values.consts.clone();
        for (f, conversions) in &values.calls {
            for &value in self.survey.returns.get(f)? {
                out.push(conversions.iter().fold(value, |value, &(bits, signed)| {
                    convert_to(value, bits, signed)
                }));
            }
        }
        few(out)
    }

    /// How a value is converted to a type: by an integer type's width and
    /// signedness, kept for an enumeration; none for another type.
    pub(crate) fn conversion(&mut self, ty: &Arc<str>) -> Option<Option<Conversion>> {
        match self.types.integer(ty) {
            Some(found) => Some(Some(found)),
            None if self.types.shape(ty) == Shape::Integer => Some(None),
            None => None,
        }
    }

    /// The values an integer expression of a definition may have, where
    /// they are a few constants and the results of direct calls, through
    /// the register variables it reads that are assigned only such values.
    fn survey_values(
        &mut self,
        def: DefId,
        function: &Function,
        assigned: &FxHashMap<VarId, Vec<Assigned<'_>>>,
        expr: &Expr,
        seen: &mut Vec<VarId>,
    ) -> Option<Values> {
        let conversion = self.conversion(&expr.ty)?;
        let values = match &expr.kind {
            ExprKind::Int(value) => Values::constant(*value),
            ExprKind::Cast(inner) => self.survey_values(def, function, assigned, inner, seen)?,
            ExprKind::Use(Place::Var(var))
                if *var >= function.params && self.defs[def as usize].is_register(*var) =>
            {
                if seen.contains(var) {
                    return None;
                }
                seen.push(*var);
                let mut out = Values::default();
                for made in assigned.get(var)? {
                    let more = match made {
                        Assigned::Value(value) => {
                            self.survey_values(def, function, assigned, value, seen)?
                        }
                        Assigned::Call(name) => Values {
                            consts: Vec::new(),
                            calls: vec![(self.fn_id(name), Vec::new())],
                        },
                        Assigned::Other => return None,
                    };
                    out = out.union(more.convert(conversion)?)?;
                }
                seen.pop();
                out
            }
            ExprKind::Unary(op, inner) => {
                let inner = self.survey_values(def, function, assigned, inner, seen)?;
                if !inner.calls.is_empty() {
                    return None;
                }
                Values {
                    consts: fold_unary(*op, &inner.consts),
                    calls: Vec::new(),
                }
            }
            ExprKind::Binary(op, left, right) => {
                let left = self.survey_values(def, function, assigned, left, seen)?;
                let right = self.survey_values(def, function, assigned, right, seen)?;
                if !left.calls.is_empty() || !right.calls.is_empty() {
                    return None;
                }
                Values {
                    consts: fold_binary(*op, &left.consts, &right.consts)?,
                    calls: Vec::new(),
                }
            }
            _ => return None,
        };
        values.convert(conversion)
    }

    /// What a definition returns, where it is a few constants and the
    /// results of direct calls.
    fn survey_returns(
        &mut self,
        def: DefId,
        function: &Function,
        assigned: &FxHashMap<VarId, Vec<Assigned<'_>>>,
    ) -> Option<Values> {
        let conversion = self.conversion(&function.ret)?;
        let mut out = Values::default();
        for block in &function.blocks {
            if let Term::Return(Some(value)) = &block.term {
                let values = self.survey_values(def, function, assigned, value, &mut Vec::new())?;
                out = out.union(values.convert(conversion)?)?;
            }
        }
        Some(out)
    }

    /// The records that may carry a tag, and the structs that are the
    /// members of such unions.
    fn survey_records(&mut self) {
        let program = self.program;
        let mut names: Vec<&Arc<str>> = program.records.keys().collect();
        names.sort_unstable();
        for name in names {
            let layout = &program.records[name];
            if layout.is_union {
                let holds = layout
                    .fields
                    .iter()
                    .any(|field| self.types.type_holds_pointer(&field.ty));
                if !holds {
                    continue;
                }
                self.survey.capable.insert(name.clone());
                for field in &layout.fields {
                    let Some(held) = &field.holds else {
                        continue;
                    };
                    let held = self.types.canonical(held);
                    let Some(held_layout) = self.types.layout(&held) else {
                        continue;
                    };
                    if held_layout.is_union || field.size != Some(held_layout.size) {
                        continue;
                    }
                    self.survey.capable.insert(held.clone());
                    self.survey
                        .variants
                        .entry(held)
                        .or_default()
                        .push((name.clone(), field.offset as i64));
                }
            } else {
                let carries = layout.fields.iter().any(|field| {
                    field.holds.as_ref().is_some_and(|held| {
                        let held = self.types.canonical(held);
                        self.types.layout(&held).is_some_and(|l| l.is_union)
                            && self.types.type_holds_pointer(&field.ty)
                    })
                });
                if carries {
                    self.survey.capable.insert(name.clone());
                }
            }
        }
    }

    fn def_info(&mut self, f: FnId, definition: &Definition, function: &Function) -> DefInfo {
        let count = function.vars.len();
        let mut uses = Uses {
            address: vec![false; count],
            member: vec![false; count],
            functions: Vec::new(),
        };
        walk_function(&mut uses, function);
        for name in uses.functions {
            let id = self.fn_id(name);
            self.address_taken.insert(id);
        }
        let mut slots = Vec::with_capacity(count);
        let mut registers = 0u32;
        let mut memory = Vec::new();
        for (i, var) in function.vars.iter().enumerate() {
            let facts = self.types.facts(&var.ty);
            if facts.shape.is_scalar() && !uses.address[i] && !uses.member[i] {
                slots.push(registers);
                registers += 1;
            } else {
                slots.push(MEMORY | memory.len() as u32);
                memory.push((i as VarId, facts.size));
            }
        }
        let params = (0..(function.params as usize).min(count))
            .map(|i| {
                let facts = self.types.facts(&function.vars[i].ty);
                Param {
                    ty: self.ty_id(&function.vars[i].ty),
                    width: facts.size.unwrap_or(8).clamp(1, u64::from(u32::MAX)) as u32,
                    record: matches!(facts.shape, Shape::Record(_) | Shape::Array)
                        .then(|| facts.size.unwrap_or(TO_END)),
                }
            })
            .collect();
        let ret_ty = self.ty_id(&function.ret);
        let ret = self.types.facts(&function.ret);
        let ret = match ret.shape {
            Shape::Void => Ret::Void,
            Shape::Record(_) | Shape::Array => Ret::Record(ret.size.unwrap_or(TO_END)),
            _ => Ret::Scalar,
        };
        DefInfo {
            fn_id: f,
            hash: definition.hash,
            file: definition.file.clone(),
            params,
            variadic: function.is_variadic,
            slots: slots.into(),
            memory: memory.into(),
            ret,
            ret_ty,
            unread: !function.unread.is_empty(),
            inst: NONE,
        }
    }

    fn survey_body(&mut self, def: DefId, f: FnId, function: Function) {
        let mut local_only = true;
        let mut callees = Vec::new();
        let mut indirect = false;
        let mut assigned: FxHashMap<VarId, Vec<Assigned<'_>>> = FxHashMap::default();
        let mut spans = Vec::new();
        for block in &function.blocks {
            for stmt in &block.stmts {
                match stmt {
                    Stmt::Assign { dst, src, .. } => {
                        local_only &= writes_local(dst);
                        if let Place::Var(var) = dst {
                            assigned.entry(*var).or_default().push(Assigned::Value(src));
                        }
                        spans.extend(self.write_span(def, dst, Some(src.ty.clone())));
                    }
                    Stmt::Call { dst, callee, .. } => {
                        if let Some(dst) = dst {
                            local_only &= writes_local(dst);
                            let ty = self.place_ty(Some(&function), dst);
                            spans.extend(self.write_span(def, dst, ty));
                        }
                        let made = match callee {
                            Callee::Direct(name) => {
                                callees.push(self.fn_id(name));
                                Assigned::Call(name)
                            }
                            Callee::Indirect(_) => {
                                indirect = true;
                                Assigned::Other
                            }
                        };
                        if let Some(Place::Var(var)) = dst {
                            assigned.entry(*var).or_default().push(made);
                        }
                    }
                    Stmt::Asm { writes, .. } => {
                        local_only = false;
                        spans.push(Span::Any);
                        for write in writes {
                            if let Place::Var(var) = write {
                                assigned.entry(*var).or_default().push(Assigned::Other);
                            }
                        }
                    }
                }
            }
        }
        callees.sort_unstable();
        callees.dedup();
        spans.sort_unstable();
        spans.dedup();
        self.survey.spans.push(spans.into());
        self.survey.facts.push(DefFacts {
            local_only,
            callees: callees.into(),
            indirect,
        });
        self.survey_tags(def, &function, &assigned);
        self.survey_va_reads(&function);
        let returned = self.survey_returns(def, &function, &assigned);
        self.survey.returned.push((f, returned));
        self.survey_accessor(def, f, &function);
        let dependencies = self.wrapper_dependencies(def, &function, &assigned);
        drop(assigned);
        if let Some(dependencies) = dependencies {
            self.survey
                .candidates
                .push((def, Arc::new(function), dependencies.into()));
        }
    }

    /// The bytes a write of a value of a type writes, but a register
    /// variable's: the value's where it is a record or an array, else the
    /// members it writes. A write through a pointer to no member is taken
    /// to write none.
    fn write_span(&mut self, def: DefId, dst: &Place, ty: Option<Arc<str>>) -> Option<Span> {
        if let Place::Var(var) = dst
            && self.defs[def as usize].is_register(*var)
        {
            return None;
        }
        if let Some(ty) = &ty
            && matches!(self.types.shape(ty), Shape::Record(_) | Shape::Array)
        {
            return Some(Span::Whole(ty.clone()));
        }
        let located = locate(dst)?;
        if located.chain.is_empty() {
            return None;
        }
        let width = ty.map_or(8, |ty| self.width_of(&ty));
        Some(Span::Members(located.chain.into(), located.start, width))
    }

    /// The bytes of records the function's conditions compare with
    /// constants, and the constants it writes into them: those its values
    /// are, and those the calls giving them return.
    fn survey_tags(
        &mut self,
        def: DefId,
        function: &Function,
        assigned: &FxHashMap<VarId, Vec<Assigned<'_>>>,
    ) {
        for block in &function.blocks {
            let operand = match &block.term {
                Term::Branch { cond, .. } => compared_operand(cond),
                Term::Switch { on, .. } => Some(strip_value(on)),
                _ => None,
            };
            if let Some(operand) = operand {
                self.note_compared(operand, assigned);
            }
            for stmt in &block.stmts {
                let (dst, values, ty) = match stmt {
                    Stmt::Assign { dst, src, .. } => {
                        let values =
                            self.survey_values(def, function, assigned, src, &mut Vec::new());
                        (dst, values, src.ty.clone())
                    }
                    Stmt::Call {
                        dst: Some(dst),
                        callee: Callee::Direct(name),
                        ..
                    } => {
                        let Some(ty) = self.place_ty(Some(function), dst) else {
                            continue;
                        };
                        let values = self.conversion(&ty).and_then(|conversion| {
                            Values {
                                consts: Vec::new(),
                                calls: vec![(self.fn_id(name), Vec::new())],
                            }
                            .convert(conversion)
                        });
                        (dst, values, ty)
                    }
                    _ => continue,
                };
                let Some(values) = values else {
                    continue;
                };
                let Some(located) = locate(dst) else {
                    continue;
                };
                if located.chain.is_empty() {
                    continue;
                }
                let width = self.types.size(&ty).unwrap_or(8).min(8) as i64;
                if values.calls.is_empty() {
                    self.note_written(&located.chain, located.start, width, &values.consts);
                } else {
                    self.survey
                        .written_later
                        .push((located.chain, located.start, width, values));
                }
            }
        }
    }

    fn note_compared(&mut self, operand: &Expr, assigned: &FxHashMap<VarId, Vec<Assigned<'_>>>) {
        match &operand.kind {
            ExprKind::Use(Place::Var(var)) => {
                for made in assigned.get(var).into_iter().flatten() {
                    match made {
                        Assigned::Value(value) => {
                            let value = strip_value(value);
                            if let ExprKind::Use(place) = &value.kind
                                && !matches!(place, Place::Var(_))
                            {
                                self.note_compared_place(place, &value.ty);
                            }
                        }
                        Assigned::Call(name) => {
                            let f = self.fn_id(name);
                            self.survey.accessor_conditions.insert(f);
                        }
                        Assigned::Other => {}
                    }
                }
            }
            ExprKind::Use(place) => self.note_compared_place(place, &operand.ty),
            _ => {}
        }
    }

    fn note_compared_place(&mut self, place: &Place, ty: &Arc<str>) {
        if self.types.shape(ty) != Shape::Integer {
            return;
        }
        let Some(located) = locate(place) else {
            return;
        };
        for (record, inst) in &located.chain {
            let record = self.types.canonical(record);
            if self.survey.capable.contains(&record) {
                self.survey.compared.insert((record, located.start - inst));
            }
        }
    }

    fn note_written(&mut self, chain: &[(Arc<str>, i64)], start: i64, width: i64, values: &[i128]) {
        for (record, inst) in chain {
            let record = self.types.canonical(record);
            if !self.survey.capable.contains(&record) {
                continue;
            }
            let rel = start - inst;
            for j in 0..width {
                let bits = self
                    .survey
                    .written
                    .entry((record.clone(), rel + j))
                    .or_default();
                for &value in values {
                    let byte = ((value >> (8 * j)) & 0xff) as usize;
                    bits[byte >> 6] |= 1 << (byte & 63);
                }
            }
        }
    }

    /// The constants a global's initializer writes into records.
    fn survey_init(&mut self, init: &Init, at: i64, chain: &mut Vec<(Arc<str>, i64)>) {
        match init {
            Init::Expr(expr) => {
                if chain.is_empty() {
                    return;
                }
                let Some(value) = constant(expr) else {
                    return;
                };
                let width = self.types.size(&expr.ty).unwrap_or(8).min(8) as i64;
                self.note_written(chain, at, width, &[value]);
            }
            Init::List { items, .. } => {
                let record = items
                    .iter()
                    .find_map(|item| item.field.as_ref().map(|field| field.record.clone()));
                let pushed = match record {
                    Some(record) => {
                        chain.push((self.types.canonical(&record), at));
                        true
                    }
                    None => false,
                };
                for item in items {
                    if let Some(offset) = item.offset.and_then(|o| i64::try_from(o).ok()) {
                        self.survey_init(&item.init, at + offset, chain);
                    }
                }
                if pushed {
                    chain.pop();
                }
            }
            Init::Zero => {}
        }
    }

    /// Records a function that returns the bytes of a member of what its
    /// first parameter points to and does nothing else.
    fn survey_accessor(&mut self, def: DefId, f: FnId, function: &Function) {
        if function.params == 0 || function.blocks.len() != 1 {
            return;
        }
        let block = &function.blocks[0];
        let (true, Term::Return(Some(value))) = (block.stmts.is_empty(), &block.term) else {
            return;
        };
        let value = strip_casts(value);
        let ExprKind::Use(place) = &value.kind else {
            return;
        };
        let Some(located) = locate(place) else {
            return;
        };
        let Base::Pointer(pointer) = located.base else {
            return;
        };
        if !matches!(strip_casts(pointer).kind, ExprKind::Use(Place::Var(0)))
            || !self.defs[def as usize].is_register(0)
            || located.chain.is_empty()
        {
            return;
        }
        let Some(width) = self.types.size(&value.ty) else {
            return;
        };
        let chain = located
            .chain
            .iter()
            .map(|(record, inst)| (self.types.canonical(record), *inst))
            .collect();
        self.survey.accessors.entry(f).or_insert(Accessor {
            chain,
            start: located.start,
            width: width.min(8) as u32,
        });
    }

    /// The functions whose calls give what a definition returns, where
    /// it returns a pointer and each value it returns is null or a
    /// variable that only such calls assign, moved by constants.
    fn wrapper_dependencies(
        &mut self,
        def: DefId,
        function: &Function,
        assigned: &FxHashMap<VarId, Vec<Assigned<'_>>>,
    ) -> Option<Vec<FnId>> {
        if !matches!(self.types.shape(&function.ret), Shape::Pointer { .. }) {
            return None;
        }
        let mut work: Vec<&Expr> = Vec::new();
        for block in &function.blocks {
            match &block.term {
                Term::Return(Some(value)) => work.push(value),
                Term::Return(None) => return None,
                _ => {}
            }
        }
        if work.is_empty() {
            return None;
        }
        let info = &self.defs[def as usize];
        let mut seen: FxHashSet<VarId> = FxHashSet::default();
        let mut dependencies: Vec<&FnName> = Vec::new();
        while let Some(value) = work.pop() {
            let mut value = value;
            loop {
                match &value.kind {
                    ExprKind::Cast(inner) => value = inner,
                    ExprKind::Offset(inner, bytes) if constant(bytes).is_some() => value = inner,
                    _ => break,
                }
            }
            match &value.kind {
                ExprKind::Int(0) => {}
                ExprKind::Use(Place::Var(var))
                    if info.is_register(*var) && *var >= function.params =>
                {
                    if !seen.insert(*var) {
                        continue;
                    }
                    for made in assigned.get(var)? {
                        match made {
                            Assigned::Value(value) => work.push(value),
                            Assigned::Call(name) => dependencies.push(name),
                            Assigned::Other => return None,
                        }
                    }
                }
                _ => return None,
            }
        }
        if dependencies.is_empty() {
            return None;
        }
        let mut ids: Vec<FnId> = dependencies
            .into_iter()
            .map(|name| self.fn_id(name))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        Some(ids)
    }
}
