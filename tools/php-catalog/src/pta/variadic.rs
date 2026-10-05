//! A call's variable arguments by their types: each `va_arg` reads only
//! the arguments its own type may read, as C allows and code relies on
//! past it. An argument is written under a part of the variadic key
//! holding the `va_arg` types that may read it, and a `va_arg` reads under
//! a context holding its own.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::survey::{Visit, walk_function};
use super::tags::{DOMAIN, OTHER, VARIADIC};
use super::{NONE, Pta, TagSetId, TyId};
use crate::ir::{Expr, ExprKind, Function};
use crate::types::{Shape, Types};

/// How deep the pointees of a type are told apart.
const DEPTH: u32 = 8;

/// What a type is to the `va_arg`s that may read a value of it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum VaClass {
    /// An integer or an enumeration of a width in bytes, whatever its
    /// signedness.
    Integer(u64),
    /// A floating type of a width in bytes.
    Float(u64),
    /// A record by its one name.
    Record(Arc<str>),
    /// Any function type.
    Function,
    Pointer(Box<VaClass>),
    /// Any type: `void`, or a type with no class.
    Any,
}

impl VaClass {
    fn of(types: &mut Types<'_>, ty: &Arc<str>, depth: u32) -> VaClass {
        let facts = types.facts(ty);
        match facts.shape {
            Shape::Integer => VaClass::Integer(facts.size.unwrap_or(0)),
            Shape::Float => VaClass::Float(facts.size.unwrap_or(0)),
            Shape::Record(record) => VaClass::Record(types.canonical(&record)),
            Shape::Function => VaClass::Function,
            Shape::Pointer { .. } => match types.pointee(ty) {
                Some(pointee) if depth < DEPTH => {
                    VaClass::Pointer(Box::new(VaClass::of(types, &pointee, depth + 1)))
                }
                _ => VaClass::Pointer(Box::new(VaClass::Any)),
            },
            Shape::Void | Shape::Array | Shape::Vector | Shape::Unknown => VaClass::Any,
        }
    }

    /// Whether a `va_arg` of a class may read an argument of another:
    /// alike at each level, `Any` as a pointee alike with every class.
    fn reads(&self, arg: &VaClass) -> bool {
        match (self, arg) {
            (VaClass::Any, _) | (_, VaClass::Any) => true,
            (VaClass::Pointer(read), VaClass::Pointer(arg)) => read.reads(arg),
            (read, arg) => read == arg,
        }
    }
}

/// The classes of the program's `va_arg` types, and the parts and the
/// contexts of the types.
#[derive(Default)]
pub(crate) struct Variadic {
    /// The classes the program's `va_arg`s read, each by the bit of its
    /// index: `OTHER` past the domain.
    reads: Vec<VaClass>,
    read_ids: FxHashMap<VaClass, usize>,
    parts: FxHashMap<TyId, TagSetId>,
    ctxs: FxHashMap<Arc<str>, TagSetId>,
}

impl Variadic {
    fn bit(index: usize) -> u64 {
        if index < DOMAIN { 1 << index } else { OTHER }
    }

    /// The values of the variadic key.
    pub(crate) fn full(&self) -> u64 {
        (0..self.reads.len()).fold(0, |bits, index| bits | Variadic::bit(index))
    }

    pub(crate) fn classes(&self) -> usize {
        self.reads.len()
    }
}

struct VaReads<'f> {
    found: Vec<&'f Arc<str>>,
}

impl<'f> Visit<'f> for VaReads<'f> {
    fn expr(&mut self, expr: &'f Expr) {
        if let ExprKind::VaArg(_) = expr.kind {
            self.found.push(&expr.ty);
        }
    }
}

impl Pta<'_> {
    /// Notes the classes of the types a function's `va_arg`s read.
    pub(crate) fn survey_va_reads(&mut self, function: &Function) {
        let mut reads = VaReads { found: Vec::new() };
        walk_function(&mut reads, function);
        for ty in reads.found {
            let class = VaClass::of(&mut self.types, ty, 0);
            if class == VaClass::Any {
                continue;
            }
            let variadic = &mut self.variadic;
            if !variadic.read_ids.contains_key(&class) {
                variadic
                    .read_ids
                    .insert(class.clone(), variadic.reads.len());
                variadic.reads.push(class);
            }
        }
    }

    /// The part a variable argument of a type is written under: the
    /// `va_arg`s that may read it; none where every one may, or the type
    /// has no class.
    pub(crate) fn vararg_part(&mut self, ty: TyId) -> TagSetId {
        if ty == NONE {
            return 0;
        }
        if let Some(&part) = self.variadic.parts.get(&ty) {
            return part;
        }
        let text = self.tys[ty as usize].clone();
        let class = VaClass::of(&mut self.types, &text, 0);
        let values = if class == VaClass::Any {
            None
        } else {
            let variadic = &self.variadic;
            let values = variadic
                .reads
                .iter()
                .enumerate()
                .filter(|(_, read)| read.reads(&class))
                .fold(0, |bits, (index, _)| bits | Variadic::bit(index));
            (values != variadic.full()).then_some(values)
        };
        let part = values.map_or(0, |values| self.tag_set(VARIADIC, values));
        self.variadic.parts.insert(ty, part);
        part
    }

    /// The context a `va_arg` of a type reads under: its own class; none
    /// where the type has no class.
    pub(crate) fn vararg_ctx(&mut self, ty: &Arc<str>) -> TagSetId {
        if let Some(&ctx) = self.variadic.ctxs.get(ty) {
            return ctx;
        }
        let class = VaClass::of(&mut self.types, ty, 0);
        let ctx = match self.variadic.read_ids.get(&class) {
            Some(&index) => self.tag_set(VARIADIC, Variadic::bit(index)),
            None => 0,
        };
        self.variadic.ctxs.insert(ty.clone(), ctx);
        ctx
    }
}
