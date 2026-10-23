//! Where a value came from: a set of data-flow nodes, numbered by whoever
//! checks the code, carried on a type's scalar parts (an `int`, a range or a
//! literal, a float, a string, `mixed`), on its objects (where they were
//! made), and through an array's values.
//!
//! Type equality ignores the set, as it ignores how a type was written: a
//! loop's convergence compares the sets itself (`Type::deep_flow`). A
//! node that carries a set is a node of its own, so what is remembered by
//! node (`intern::memoized`) is remembered for that set. A type built from
//! its form alone carries none: an operation that derives a value gives it
//! its operands' sets itself, and the operations that keep a value's
//! identity (union, intersection, widening) keep its set, each scalar part
//! taking the sets of the parts of the same kind of value it came from.

use std::sync::Arc;

use crate::{Repr, Type};

/// A set of data-flow node numbers, sorted, empty when there is none.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flow(Option<Arc<Vec<u32>>>);

impl Flow {
    /// The set of these nodes.
    pub fn of(nodes: impl IntoIterator<Item = u32>) -> Flow {
        let mut nodes: Vec<u32> = nodes.into_iter().collect();
        nodes.sort_unstable();
        nodes.dedup();
        if nodes.is_empty() {
            Flow(None)
        } else {
            Flow(Some(Arc::new(nodes)))
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    pub fn nodes(&self) -> &[u32] {
        self.0.as_deref().map_or(&[], Vec::as_slice)
    }

    /// The nodes of both.
    pub fn union(&self, other: &Flow) -> Flow {
        match (&self.0, &other.0) {
            (None, _) => other.clone(),
            (_, None) => self.clone(),
            (Some(a), Some(b)) if Arc::ptr_eq(a, b) => self.clone(),
            (Some(a), Some(b)) => Flow::of(a.iter().chain(b.iter()).copied()),
        }
    }

    /// These nodes but `node`.
    pub fn without(&self, node: u32) -> Flow {
        if !self.nodes().contains(&node) {
            return self.clone();
        }
        Flow::of(self.nodes().iter().copied().filter(|&n| n != node))
    }

    /// Whether every node of `other` is one of these.
    pub fn contains_all(&self, other: &Flow) -> bool {
        other
            .nodes()
            .iter()
            .all(|n| self.nodes().binary_search(n).is_ok())
    }
}

/// The kind of value a scalar part is, for which parts of a result take
/// which parts' sets.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Int,
    Float,
    String,
    /// An object: the sets name where it was made and what it went through.
    Object,
    /// `mixed` or a template: a value of any kind.
    Any,
}

fn family(ty: &Type) -> Option<Family> {
    match ty.repr() {
        Repr::Int | Repr::IntLiteral(_) | Repr::IntRange(..) => Some(Family::Int),
        Repr::Float(_) | Repr::FloatLiteral(_) => Some(Family::Float),
        Repr::String | Repr::StringLiteral(_) | Repr::ClassString(_) | Repr::ClassKindString(_) => {
            Some(Family::String)
        }
        Repr::Object(..) | Repr::ObjectWithoutClass => Some(Family::Object),
        Repr::Intersection(members) => members
            .iter()
            .find_map(family)
            .filter(|f| *f != Family::Any),
        Repr::Mixed(..) | Repr::TemplateParam(_) => Some(Family::Any),
        _ => None,
    }
}

/// The sets a type's scalar parts carry, by the kind of value each is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FamilyFlows {
    int: Flow,
    float: Flow,
    string: Flow,
    object: Flow,
    any: Flow,
}

impl FamilyFlows {
    fn add(&mut self, family: Family, flow: &Flow) {
        let slot = match family {
            Family::Int => &mut self.int,
            Family::Float => &mut self.float,
            Family::String => &mut self.string,
            Family::Object => &mut self.object,
            Family::Any => &mut self.any,
        };
        *slot = slot.union(flow);
    }

    pub fn is_empty(&self) -> bool {
        self.int.is_empty()
            && self.float.is_empty()
            && self.string.is_empty()
            && self.object.is_empty()
            && self.any.is_empty()
    }

    /// Both's sets, kind by kind.
    pub fn union(&self, other: &FamilyFlows) -> FamilyFlows {
        FamilyFlows {
            int: self.int.union(&other.int),
            float: self.float.union(&other.float),
            string: self.string.union(&other.string),
            object: self.object.union(&other.object),
            any: self.any.union(&other.any),
        }
    }

    /// Every set, whatever kind of value carries it.
    pub fn all(&self) -> Flow {
        self.int
            .union(&self.float)
            .union(&self.string)
            .union(&self.object)
            .union(&self.any)
    }

    /// What a part of `family` takes: the sets of its own kind of value,
    /// and those of a value of any kind; a value of any kind takes them all.
    fn for_family(&self, family: Family) -> Flow {
        match family {
            Family::Int => self.int.union(&self.any),
            Family::Float => self.float.union(&self.any),
            Family::String => self.string.union(&self.any),
            Family::Object => self.object.union(&self.any),
            Family::Any => self.all(),
        }
    }
}

impl Type {
    /// The set this node carries itself.
    pub fn flow(&self) -> &Flow {
        &self.0.flow
    }

    /// Every set carried anywhere in this type, an array's values' too: what
    /// a loop compares to see whether its values came from more places.
    pub fn deep_flow(&self) -> Flow {
        if !self.has_flow() {
            return Flow::default();
        }
        let mut flow = self.flow().clone();
        crate::traverse::for_each_repr_child(self.repr(), &mut |child| {
            flow = flow.union(&child.deep_flow());
        });
        flow
    }

    /// Whether a set is carried anywhere in this type.
    pub fn has_flow(&self) -> bool {
        self.0.has_flow()
    }

    /// The sets this type's scalar parts carry, its union's members' and
    /// its own; not those inside an array.
    pub fn scalar_flow(&self) -> Flow {
        self.family_flows().all()
    }

    /// The sets this type's scalar parts carry, by the kind of value each
    /// is; not those inside an array.
    pub fn family_flows(&self) -> FamilyFlows {
        let mut flows = FamilyFlows::default();
        if self.has_flow() {
            collect_family_flows(self, &mut flows);
        }
        flows
    }

    /// This type with `flow` added to each scalar part, and to each value of
    /// an array.
    pub fn with_flow(&self, flow: &Flow) -> Type {
        if flow.is_empty() {
            return self.clone();
        }
        stack::guard(|| match self.repr() {
            Repr::Union(members) => Type::new(Repr::Union(
                members.iter().map(|m| m.with_flow(flow)).collect(),
            )),
            Repr::BenevolentUnion(members) => Type::new(Repr::BenevolentUnion(
                members.iter().map(|m| m.with_flow(flow)).collect(),
            )),
            Repr::Array(key, value) => Type::new(Repr::Array(key.clone(), value.with_flow(flow))),
            Repr::ArrayShape(shape) => {
                Type::array_shape(shape.map_types(&mut |t| t.with_flow(flow)))
            }
            _ if family(self).is_some() => self.with_own_flow(self.flow().union(flow)),
            // An array refined by accessories (`list<string>`) carries the
            // set in its values.
            Repr::Intersection(members) => Type::new(Repr::Intersection(
                members.iter().map(|m| m.with_flow(flow)).collect(),
            )),
            _ => self.clone(),
        })
    }

    /// This type with each scalar part given the sets of `flows` that its
    /// kind of value takes.
    pub fn with_family_flows(&self, flows: &FamilyFlows) -> Type {
        if flows.is_empty() {
            return self.clone();
        }
        stack::guard(|| match self.repr() {
            Repr::Union(members) => Type::new(Repr::Union(
                members.iter().map(|m| m.with_family_flows(flows)).collect(),
            )),
            Repr::BenevolentUnion(members) => Type::new(Repr::BenevolentUnion(
                members.iter().map(|m| m.with_family_flows(flows)).collect(),
            )),
            _ => match family(self) {
                Some(family) => {
                    let flow = self.flow().union(&flows.for_family(family));
                    self.with_own_flow(flow)
                }
                None => self.clone(),
            },
        })
    }

    /// This type without the sets its scalar parts carry; an array's values
    /// keep theirs.
    pub fn without_scalar_flow(&self) -> Type {
        if !self.has_flow() {
            return self.clone();
        }
        stack::guard(|| match self.repr() {
            Repr::Union(members) => Type::new(Repr::Union(
                members.iter().map(Type::without_scalar_flow).collect(),
            )),
            Repr::BenevolentUnion(members) => Type::new(Repr::BenevolentUnion(
                members.iter().map(Type::without_scalar_flow).collect(),
            )),
            _ if !self.flow().is_empty() => Type::new(self.repr().clone()),
            _ => self.clone(),
        })
    }

    /// This type without `node` in the sets its scalar parts carry; an
    /// array's values keep theirs.
    pub fn without_flow_node(&self, node: u32) -> Type {
        if !self.has_flow() {
            return self.clone();
        }
        stack::guard(|| match self.repr() {
            Repr::Union(members) => Type::new(Repr::Union(
                members.iter().map(|m| m.without_flow_node(node)).collect(),
            )),
            Repr::BenevolentUnion(members) => Type::new(Repr::BenevolentUnion(
                members.iter().map(|m| m.without_flow_node(node)).collect(),
            )),
            _ => self.with_own_flow(self.flow().without(node)),
        })
    }

    /// The sets this type's overflow floats carry: the float `int`
    /// arithmetic gives past the `int` range.
    pub fn overflow_float_flow(&self) -> Flow {
        self.union_members()
            .iter()
            .filter(|m| matches!(m.repr(), Repr::Float(crate::FloatOrigin::IntOverflow)))
            .fold(Flow::default(), |acc, m| acc.union(m.flow()))
    }

    /// This type with `flow` added to its overflow floats only.
    pub fn with_overflow_float_flow(&self, flow: &Flow) -> Type {
        if flow.is_empty() {
            return self.clone();
        }
        match self.repr() {
            Repr::Union(members) => Type::new(Repr::Union(
                members
                    .iter()
                    .map(|m| m.with_overflow_float_flow(flow))
                    .collect(),
            )),
            Repr::BenevolentUnion(members) => Type::new(Repr::BenevolentUnion(
                members
                    .iter()
                    .map(|m| m.with_overflow_float_flow(flow))
                    .collect(),
            )),
            Repr::Float(crate::FloatOrigin::IntOverflow) => {
                self.with_own_flow(self.flow().union(flow))
            }
            _ => self.clone(),
        }
    }

    fn with_own_flow(&self, flow: Flow) -> Type {
        if flow == self.0.flow {
            return self.clone();
        }
        if flow.is_empty() {
            return Type::new(self.repr().clone());
        }
        Type(Arc::new(self.0.with_flow(flow)))
    }
}

fn collect_family_flows(ty: &Type, flows: &mut FamilyFlows) {
    stack::guard(|| match ty.repr() {
        Repr::Union(members) | Repr::BenevolentUnion(members) => {
            for member in members {
                collect_family_flows(member, flows);
            }
        }
        _ => {
            if let Some(family) = family(ty) {
                flows.add(family, ty.flow());
            }
        }
    })
}

/// `keeping_flows`, for an operation that rebuilds arrays (widening,
/// generalizing): the sets inside the operands' arrays, keys and values,
/// go to the result's array values.
pub(crate) fn keeping_deep_flows(
    operands: Vec<Type>,
    make: impl FnOnce(Vec<Type>) -> Type,
) -> Type {
    let nested = operands
        .iter()
        .fold(Flow::default(), |acc, t| acc.union(&nested_flow(t)));
    let result = keeping_flows(operands, make);
    if nested.is_empty() {
        result
    } else {
        with_nested_flow(&result, &nested)
    }
}

/// Every set carried inside `ty`'s arrays.
fn nested_flow(ty: &Type) -> Flow {
    if !ty.has_flow() {
        return Flow::default();
    }
    stack::guard(|| match ty.repr() {
        Repr::Union(members) | Repr::BenevolentUnion(members) | Repr::Intersection(members) => {
            members
                .iter()
                .fold(Flow::default(), |acc, m| acc.union(&nested_flow(m)))
        }
        Repr::Array(..) | Repr::ArrayShape(_) => {
            let mut flow = Flow::default();
            crate::traverse::for_each_repr_child(ty.repr(), &mut |child| {
                flow = flow.union(&child.deep_flow());
            });
            flow
        }
        _ => Flow::default(),
    })
}

/// `ty` with `flow` added to the values of its arrays.
fn with_nested_flow(ty: &Type, flow: &Flow) -> Type {
    stack::guard(|| match ty.repr() {
        Repr::Union(members) => Type::new(Repr::Union(
            members.iter().map(|m| with_nested_flow(m, flow)).collect(),
        )),
        Repr::BenevolentUnion(members) => Type::new(Repr::BenevolentUnion(
            members.iter().map(|m| with_nested_flow(m, flow)).collect(),
        )),
        Repr::Intersection(members) => Type::new(Repr::Intersection(
            members.iter().map(|m| with_nested_flow(m, flow)).collect(),
        )),
        Repr::Array(..) | Repr::ArrayShape(_) => ty.with_flow(flow),
        _ => ty.clone(),
    })
}

/// `make` applied to `operands` without the sets their scalar parts carry,
/// the result's scalar parts given those sets by kind of value: how an
/// operation that keeps a value's identity (union, intersection, widening)
/// keeps where it came from.
pub(crate) fn keeping_flows(operands: Vec<Type>, make: impl FnOnce(Vec<Type>) -> Type) -> Type {
    if !operands.iter().any(Type::has_flow) {
        return make(operands);
    }
    let flows = operands.iter().fold(FamilyFlows::default(), |acc, t| {
        acc.union(&t.family_flows())
    });
    if flows.is_empty() {
        return make(operands);
    }
    let stripped = operands.iter().map(Type::without_scalar_flow).collect();
    make(stripped).with_family_flows(&flows)
}
