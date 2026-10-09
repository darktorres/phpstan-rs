//! Small PHP value/reference domain for the Phase F slice. Arrays have value
//! semantics, objects retain identity, and references point to explicit cells.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    Bytes, Coverage, Digest, Domain, DomainCoverage, Event, EventExit, Operand, PhpFlowInstruction,
    Precision, Predicate, SemanticHost, Truth,
};

pub type ArrayId = u64;
pub type ObjectId = u64;
pub type ReferenceId = u64;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PhpValue {
    Null,
    Bool(bool),
    Int(i64),
    String(Bytes),
    Array(ArrayId),
    Object(ObjectId),
    Callable(Bytes),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ArrayKey {
    Int(i64),
    String(Bytes),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ArrayElement {
    Value(PhpValue),
    Reference(ReferenceId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Binding {
    Value(PhpValue),
    Reference(ReferenceId),
}

/// A bounded concrete-shaped state used to validate the abstract value
/// vocabulary. Unknown remains explicit; it never aliases a known empty value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhpState {
    locals: BTreeMap<Bytes, Binding>,
    references: BTreeMap<ReferenceId, PhpValue>,
    arrays: BTreeMap<ArrayId, BTreeMap<ArrayKey, ArrayElement>>,
    next_identity: u64,
    havoced: bool,
}

impl PhpState {
    pub fn new() -> Self {
        Self::default()
    }

    fn identity(&mut self) -> u64 {
        self.next_identity += 1;
        self.next_identity
    }

    pub fn new_object(&mut self) -> PhpValue {
        PhpValue::Object(self.identity())
    }

    pub fn new_array(&mut self) -> PhpValue {
        let id = self.identity();
        self.arrays.insert(id, BTreeMap::new());
        PhpValue::Array(id)
    }

    pub fn set_local(&mut self, name: Bytes, value: PhpValue) {
        match self.locals.get(&name) {
            Some(Binding::Reference(id)) => {
                self.references.insert(*id, value);
            }
            _ => {
                self.locals.insert(name, Binding::Value(value));
            }
        }
    }

    /// Remove a variable binding without invalidating a reference cell that
    /// may still be reachable through another local or an array element.
    pub fn unset_local(&mut self, name: &[u8]) {
        self.locals.remove(&Bytes(name.to_vec()));
    }

    pub fn bind_reference(&mut self, left: Bytes, right: Bytes) {
        let reference = match self.locals.get(&right) {
            Some(Binding::Reference(id)) => *id,
            Some(Binding::Value(value)) => {
                let value = value.clone();
                let id = self.identity();
                self.references.insert(id, value);
                id
            }
            None => {
                let id = self.identity();
                self.references.insert(id, PhpValue::Unknown);
                id
            }
        };
        self.locals.insert(right, Binding::Reference(reference));
        self.locals.insert(left, Binding::Reference(reference));
    }

    pub fn local(&self, name: &[u8]) -> Option<PhpValue> {
        let Some(binding) = self.locals.get(&Bytes(name.to_vec())) else {
            return self.havoced.then_some(PhpValue::Unknown);
        };
        match binding {
            Binding::Value(value) => Some(value.clone()),
            Binding::Reference(id) => self.references.get(id).cloned(),
        }
    }

    fn join_abstract(&self, other: &Self) -> Self {
        if self == other {
            self.clone()
        } else {
            Self {
                havoced: true,
                ..Self::default()
            }
        }
    }

    fn subsumes_abstract(&self, other: &Self) -> bool {
        self == other || self.havoced
    }

    fn havoc(&mut self) {
        *self = Self {
            havoced: true,
            ..Self::default()
        };
    }

    /// PHP assignment copies arrays recursively while preserving object
    /// identity and references nested inside the copied array.
    pub fn assign(&mut self, destination: Bytes, source: &[u8]) -> Result<(), StateError> {
        let value = self
            .local(source)
            .ok_or_else(|| StateError::MissingLocal(Bytes(source.to_vec())))?;
        let copied = self.copy_value(value, &mut BTreeMap::new())?;
        self.set_local(destination, copied);
        Ok(())
    }

    fn copy_value(
        &mut self,
        value: PhpValue,
        copies: &mut BTreeMap<ArrayId, ArrayId>,
    ) -> Result<PhpValue, StateError> {
        let PhpValue::Array(source_id) = value else {
            return Ok(value);
        };
        if let Some(copy_id) = copies.get(&source_id) {
            return Ok(PhpValue::Array(*copy_id));
        }
        let source = self
            .arrays
            .get(&source_id)
            .cloned()
            .ok_or(StateError::MissingArray(source_id))?;
        let copy_id = self.identity();
        copies.insert(source_id, copy_id);
        self.arrays.insert(copy_id, BTreeMap::new());
        for (key, element) in source {
            let copied = match element {
                ArrayElement::Value(value) => ArrayElement::Value(self.copy_value(value, copies)?),
                ArrayElement::Reference(id) => ArrayElement::Reference(id),
            };
            self.arrays
                .get_mut(&copy_id)
                .expect("copy inserted above")
                .insert(key, copied);
        }
        Ok(PhpValue::Array(copy_id))
    }

    pub fn array_set(
        &mut self,
        name: &[u8],
        key: ArrayKey,
        value: PhpValue,
    ) -> Result<(), StateError> {
        let Some(PhpValue::Array(id)) = self.local(name) else {
            return Err(StateError::NotArray(Bytes(name.to_vec())));
        };
        self.arrays
            .get_mut(&id)
            .ok_or(StateError::MissingArray(id))?
            .insert(key, ArrayElement::Value(value));
        Ok(())
    }

    pub fn array_set_reference(
        &mut self,
        name: &[u8],
        key: ArrayKey,
        reference: ReferenceId,
    ) -> Result<(), StateError> {
        let Some(PhpValue::Array(id)) = self.local(name) else {
            return Err(StateError::NotArray(Bytes(name.to_vec())));
        };
        self.arrays
            .get_mut(&id)
            .ok_or(StateError::MissingArray(id))?
            .insert(key, ArrayElement::Reference(reference));
        Ok(())
    }

    pub fn array_get(&self, name: &[u8], key: &ArrayKey) -> Result<Option<PhpValue>, StateError> {
        let Some(PhpValue::Array(id)) = self.local(name) else {
            return Err(StateError::NotArray(Bytes(name.to_vec())));
        };
        let element = self
            .arrays
            .get(&id)
            .ok_or(StateError::MissingArray(id))?
            .get(key);
        Ok(match element {
            Some(ArrayElement::Value(value)) => Some(value.clone()),
            Some(ArrayElement::Reference(reference)) => self.references.get(reference).cloned(),
            None => None,
        })
    }

    pub fn array_identity(&self, name: &[u8]) -> Option<ArrayId> {
        match self.local(name)? {
            PhpValue::Array(id) => Some(id),
            _ => None,
        }
    }
}

/// One reviewed callback outcome for the minimal PHP flow slice. Argument
/// indices in `writes` refer to callback parameters; invoke operands map those
/// parameters to caller locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhpCallbackOutcome {
    pub writes: Vec<(u32, PhpValue)>,
    pub normal: bool,
    pub result: Option<PhpValue>,
    pub throws: Option<PhpValue>,
}

/// PHP-specific value/state adapter for the shared behavior-graph evaluator.
/// A missing callback model stays open and produces both normal and throw
/// outcomes; modeled callbacks retain ordered writes on either exit.
#[derive(Debug, Clone)]
pub struct PhpFlowHost {
    pub argument_locations: Vec<Option<Bytes>>,
    pub callbacks: BTreeMap<Bytes, Vec<PhpCallbackOutcome>>,
    pub evidence: Digest,
}

impl PhpFlowHost {
    pub fn new(evidence: Digest) -> Self {
        Self {
            argument_locations: Vec::new(),
            callbacks: BTreeMap::new(),
            evidence,
        }
    }

    fn coverage(&self) -> Vec<DomainCoverage> {
        // The minimal callback model only closes invocation, user writes,
        // normal return, and PHP throw. Other effect domains remain open.
        let modeled = [
            Domain::Invocation,
            Domain::UserWrite,
            Domain::Return,
            Domain::PhpThrow,
        ];
        Domain::ALL
            .into_iter()
            .map(|domain| DomainCoverage {
                domain,
                coverage: if !modeled.contains(&domain) {
                    Coverage::Open {
                        reasons: vec![Bytes(b"callback model is incomplete".to_vec())],
                    }
                } else {
                    Coverage::Closed
                },
                precision: if modeled.contains(&domain) {
                    Precision::Exact
                } else {
                    Precision::Widened
                },
                dependencies: if domain == Domain::Return {
                    vec![Domain::PhpThrow]
                } else {
                    vec![]
                },
                evidence: vec![self.evidence],
                assumptions: vec![],
            })
            .collect()
    }

    fn open_domains(&self, domains: &[Domain]) -> Vec<DomainCoverage> {
        Domain::ALL
            .into_iter()
            .map(|domain| DomainCoverage {
                domain,
                coverage: if domains.contains(&domain) {
                    Coverage::Open {
                        reasons: vec![Bytes(b"PHP flow operation is not modeled".to_vec())],
                    }
                } else {
                    Coverage::NotApplicable
                },
                precision: Precision::Widened,
                dependencies: vec![],
                evidence: vec![self.evidence],
                assumptions: vec![],
            })
            .collect()
    }

    fn unmodeled_outcomes(
        &self,
        mut state: PhpState,
        domains: &[Domain],
        result: Option<u32>,
    ) -> Vec<EventExit<PhpValue, PhpState>> {
        state.havoc();
        let coverage = self.open_domains(domains);
        let mut outcomes = vec![EventExit::Normal {
            result: result.map(|id| (id, PhpValue::Unknown)),
            state: state.clone(),
            coverage: coverage.clone(),
        }];
        if domains.contains(&Domain::PhpThrow) {
            outcomes.push(EventExit::PhpThrow {
                value: PhpValue::Unknown,
                state: state.clone(),
                coverage: coverage.clone(),
            });
        }
        if domains.contains(&Domain::Invocation) {
            outcomes.push(EventExit::NativeUnwind {
                value: PhpValue::Unknown,
                state: state.clone(),
                coverage: coverage.clone(),
            });
        }
        if domains.contains(&Domain::Fatal) {
            outcomes.push(EventExit::Fatal {
                value: PhpValue::Unknown,
                state: state.clone(),
                coverage: coverage.clone(),
            });
        }
        if domains.contains(&Domain::Suspension) {
            outcomes.push(EventExit::Suspended {
                value: PhpValue::Unknown,
                state,
                coverage,
            });
        }
        outcomes
    }

    fn argument_location(&self, operand: &Operand) -> Option<&Bytes> {
        let Operand::Argument(index) = operand else {
            return None;
        };
        self.argument_locations.get(*index as usize)?.as_ref()
    }
}

impl SemanticHost for PhpFlowHost {
    type Value = PhpValue;
    type State = PhpState;

    fn resolve(
        &self,
        operand: &Operand,
        arguments: &[PhpValue],
        receiver: &PhpValue,
        results: &[(u32, PhpValue)],
    ) -> Option<PhpValue> {
        match operand {
            Operand::Argument(index) => arguments.get(*index as usize).cloned(),
            Operand::Receiver => Some(receiver.clone()),
            Operand::Result(index) => results
                .iter()
                .find(|(id, _)| id == index)
                .map(|(_, value)| value.clone()),
            Operand::Literal(bytes) => Some(PhpValue::String(bytes.clone())),
            Operand::IntRange {
                min: Some(min),
                max: Some(max),
            } if min == max => Some(PhpValue::Int(*min)),
            Operand::StringByteLength(index) => match arguments.get(*index as usize) {
                Some(PhpValue::String(bytes)) => Some(PhpValue::Int(bytes.0.len() as i64)),
                _ => Some(PhpValue::Unknown),
            },
            Operand::IntRange { .. } | Operand::Unknown => Some(PhpValue::Unknown),
        }
    }

    fn test(
        &self,
        predicate: &Predicate,
        arguments: &[PhpValue],
        receiver: &PhpValue,
        results: &[(u32, PhpValue)],
        _state: &PhpState,
    ) -> Truth {
        match predicate {
            Predicate::Always => Truth::Yes,
            Predicate::IsLiteral { value, expected } => {
                match self.resolve(value, arguments, receiver, results) {
                    Some(PhpValue::String(value)) if value == *expected => Truth::Yes,
                    Some(PhpValue::Unknown) | None => Truth::Maybe,
                    Some(_) => Truth::No,
                }
            }
            Predicate::Host(_) => Truth::Maybe,
        }
    }

    fn event(
        &self,
        event: &Event,
        arguments: &[PhpValue],
        receiver: &PhpValue,
        results: &[(u32, PhpValue)],
        mut state: PhpState,
    ) -> Vec<EventExit<PhpValue, PhpState>> {
        match event {
            Event::Read { .. } => self.unmodeled_outcomes(state, &Domain::ALL, None),
            Event::Write { storage, value } => {
                let Some(value) = self.resolve(value, arguments, receiver, results) else {
                    return self.unmodeled_outcomes(
                        state,
                        &[
                            Domain::Invocation,
                            Domain::UserWrite,
                            Domain::PhpThrow,
                            Domain::Fatal,
                        ],
                        None,
                    );
                };
                let Some(location) = self.argument_location(storage) else {
                    return self.unmodeled_outcomes(
                        state,
                        &[
                            Domain::Invocation,
                            Domain::UserWrite,
                            Domain::PhpThrow,
                            Domain::Fatal,
                        ],
                        None,
                    );
                };
                state.set_local(location.clone(), value);
                vec![EventExit::Normal {
                    result: None,
                    state,
                    coverage: vec![],
                }]
            }
            Event::PhpFlow { instruction } => {
                let error = match instruction {
                    PhpFlowInstruction::Assign {
                        destination,
                        source,
                    } => state.assign(destination.clone(), &source.0).err(),
                    PhpFlowInstruction::Unset { local } => {
                        state.unset_local(&local.0);
                        None
                    }
                    PhpFlowInstruction::BindReference { left, right } => {
                        state.bind_reference(left.clone(), right.clone());
                        None
                    }
                    PhpFlowInstruction::ArraySet { array, key, source } => {
                        match state.local(&source.0) {
                            Some(value) => state.array_set(&array.0, key.clone(), value).err(),
                            None => Some(StateError::MissingLocal(source.clone())),
                        }
                    }
                    PhpFlowInstruction::ArrayGet {
                        destination,
                        array,
                        key,
                    } => match state.array_get(&array.0, key) {
                        Ok(Some(value)) => {
                            state.set_local(destination.clone(), value);
                            None
                        }
                        Ok(None) => Some(StateError::MissingArrayElement {
                            array: array.clone(),
                            key: key.clone(),
                        }),
                        Err(error) => Some(error),
                    },
                };
                if error.is_some() {
                    self.unmodeled_outcomes(
                        state,
                        &[
                            Domain::UserRead,
                            Domain::UserWrite,
                            Domain::PhpThrow,
                            Domain::Fatal,
                        ],
                        None,
                    )
                } else {
                    vec![EventExit::Normal {
                        result: None,
                        state,
                        coverage: vec![],
                    }]
                }
            }
            Event::Invoke {
                callable,
                arguments: callback_args,
                result,
            } => {
                let name = self.resolve(callable, arguments, receiver, results);
                let callable_name = match name {
                    Some(PhpValue::Callable(name)) => Some(name),
                    _ => None,
                };
                let Some(outcomes) = callable_name
                    .as_ref()
                    .and_then(|name| self.callbacks.get(name))
                    .cloned()
                else {
                    return self.unmodeled_outcomes(state, &Domain::ALL, Some(*result));
                };
                let mut exits = Vec::new();
                for outcome in outcomes {
                    let mut next = state.clone();
                    for (index, value) in &outcome.writes {
                        let target = callback_args
                            .get(*index as usize)
                            .and_then(|operand| self.argument_location(operand));
                        if let Some(location) = target {
                            next.set_local(location.clone(), value.clone());
                        } else {
                            next.havoc();
                        }
                    }
                    let coverage = self.coverage();
                    if outcome.normal {
                        exits.push(EventExit::Normal {
                            result: outcome.result.map(|value| (*result, value)),
                            state: next.clone(),
                            coverage: coverage.clone(),
                        });
                    }
                    if let Some(value) = outcome.throws {
                        exits.push(EventExit::PhpThrow {
                            value,
                            state: next,
                            coverage,
                        });
                    }
                }
                if exits.is_empty() {
                    return self.unmodeled_outcomes(state, &Domain::ALL, Some(*result));
                }
                exits
            }
            Event::Unknown { domains, .. } => self.unmodeled_outcomes(state, domains, None),
        }
    }

    fn unknown_value(&self) -> PhpValue {
        PhpValue::Unknown
    }
    fn join_value(&self, left: PhpValue, right: PhpValue) -> PhpValue {
        if left == right {
            left
        } else {
            PhpValue::Unknown
        }
    }
    fn widen_value(&self, _old: PhpValue, _next: PhpValue) -> PhpValue {
        PhpValue::Unknown
    }
    fn value_subsumes(&self, broad: &PhpValue, narrow: &PhpValue) -> bool {
        broad == narrow || *broad == PhpValue::Unknown
    }
    fn join_state(&self, left: PhpState, right: PhpState) -> PhpState {
        left.join_abstract(&right)
    }
    fn widen_state(&self, _old: PhpState, _next: PhpState) -> PhpState {
        PhpState {
            havoced: true,
            ..PhpState::default()
        }
    }
    fn state_subsumes(&self, broad: &PhpState, narrow: &PhpState) -> bool {
        broad.subsumes_abstract(narrow)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    MissingLocal(Bytes),
    MissingArray(ArrayId),
    NotArray(Bytes),
    MissingArrayElement { array: Bytes, key: ArrayKey },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str) -> Bytes {
        Bytes(name.as_bytes().to_vec())
    }

    #[test]
    fn array_assignment_copies_contents_and_mutation_isolated() {
        let mut state = PhpState::new();
        let array = state.new_array();
        state.set_local(b("a"), array);
        state
            .array_set(b"a", ArrayKey::String(b("k")), PhpValue::Int(1))
            .unwrap();
        state.assign(b("b"), b"a").unwrap();
        assert_ne!(state.array_identity(b"a"), state.array_identity(b"b"));
        state
            .array_set(b"b", ArrayKey::String(b("k")), PhpValue::Int(2))
            .unwrap();
        assert_eq!(
            state.array_get(b"a", &ArrayKey::String(b("k"))).unwrap(),
            Some(PhpValue::Int(1))
        );
        assert_eq!(
            state.array_get(b"b", &ArrayKey::String(b("k"))).unwrap(),
            Some(PhpValue::Int(2))
        );
    }

    #[test]
    fn nested_array_assignment_copies_nested_values_too() {
        let mut state = PhpState::new();
        let inner = state.new_array();
        state.set_local(b("inner"), inner.clone());
        state
            .array_set(b"inner", ArrayKey::String(b("leaf")), PhpValue::Int(1))
            .unwrap();
        let outer = state.new_array();
        state.set_local(b("outer"), outer);
        state
            .array_set(b"outer", ArrayKey::String(b("nested")), inner)
            .unwrap();
        state.assign(b("copy"), b"outer").unwrap();

        let copied_nested = state
            .array_get(b"copy", &ArrayKey::String(b("nested")))
            .unwrap()
            .unwrap();
        state.set_local(b("nested-copy"), copied_nested);
        state
            .array_set(
                b"nested-copy",
                ArrayKey::String(b("leaf")),
                PhpValue::Int(2),
            )
            .unwrap();
        let nested = state
            .array_get(b"outer", &ArrayKey::String(b("nested")))
            .unwrap();
        assert_eq!(
            nested,
            Some(PhpValue::Array(state.array_identity(b"inner").unwrap()))
        );
        assert_eq!(
            state
                .array_get(b"inner", &ArrayKey::String(b("leaf")))
                .unwrap(),
            Some(PhpValue::Int(1))
        );
        assert_eq!(
            state
                .array_get(b"nested-copy", &ArrayKey::String(b("leaf")))
                .unwrap(),
            Some(PhpValue::Int(2))
        );
    }

    #[test]
    fn object_assignment_preserves_identity() {
        let mut state = PhpState::new();
        let object = state.new_object();
        state.set_local(b("a"), object.clone());
        state.assign(b("b"), b"a").unwrap();
        assert_eq!(state.local(b"a"), Some(object.clone()));
        assert_eq!(state.local(b"b"), Some(object));
    }

    #[test]
    fn references_alias_cells_and_array_copy_preserves_nested_reference() {
        let mut state = PhpState::new();
        state.set_local(b("a"), PhpValue::Int(1));
        state.bind_reference(b("b"), b("a"));
        state.set_local(b("b"), PhpValue::Int(2));
        assert_eq!(state.local(b"a"), Some(PhpValue::Int(2)));

        let array = state.new_array();
        state.set_local(b("box"), array);
        let reference = match state.locals.get(&b("a")) {
            Some(Binding::Reference(id)) => *id,
            _ => unreachable!(),
        };
        state
            .array_set_reference(b"box", ArrayKey::String(b("item")), reference)
            .unwrap();
        state.assign(b("copy"), b"box").unwrap();
        state.set_local(b("a"), PhpValue::Int(3));
        assert_eq!(
            state
                .array_get(b"copy", &ArrayKey::String(b("item")))
                .unwrap(),
            Some(PhpValue::Int(3))
        );
    }

    #[test]
    fn reference_rebinding_and_unset_preserve_remaining_aliases() {
        let mut state = PhpState::new();
        state.set_local(b("left"), PhpValue::Int(1));
        state.set_local(b("right"), PhpValue::Int(2));
        state.bind_reference(b("alias"), b("left"));
        state.bind_reference(b("alias"), b("right"));
        state.set_local(b("left"), PhpValue::Int(3));

        assert_eq!(state.local(b"left"), Some(PhpValue::Int(3)));
        assert_eq!(state.local(b"alias"), Some(PhpValue::Int(2)));
        assert_eq!(state.local(b"right"), Some(PhpValue::Int(2)));

        state.unset_local(b"right");
        assert_eq!(state.local(b"right"), None);
        assert_eq!(state.local(b"alias"), Some(PhpValue::Int(2)));
        state.set_local(b("alias"), PhpValue::Int(4));
        assert_eq!(state.local(b"alias"), Some(PhpValue::Int(4)));
    }

    #[test]
    fn unknown_is_distinct_from_null_and_empty_array() {
        let mut state = PhpState::new();
        state.set_local(b("unknown"), PhpValue::Unknown);
        state.set_local(b("null"), PhpValue::Null);
        let array = state.new_array();
        state.set_local(b("array"), array);
        assert_ne!(state.local(b"unknown"), state.local(b"null"));
        assert_ne!(state.local(b"unknown"), state.local(b"array"));
    }
}
