//! Small PHP value/reference domain for the Phase F slice. Arrays have value
//! semantics, objects retain identity, and references point to explicit cells.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Bytes;

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
        match self.locals.get(&Bytes(name.to_vec()))? {
            Binding::Value(value) => Some(value.clone()),
            Binding::Reference(id) => self.references.get(id).cloned(),
        }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    MissingLocal(Bytes),
    MissingArray(ArrayId),
    NotArray(Bytes),
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
