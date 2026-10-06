//! Stable, exact-case slots for one class's properties.
//!
//! Property names are case-sensitive in PHP, so this topology deliberately
//! uses `NameMap` rather than the case-folded map used for method slots. The
//! owning registry stores the ordered names in its cache and rebuilds this
//! runtime lookup map from them.

use syntax::name::{Name, NameMap};

/// The ordered property names and their exact-case class-local slots.
#[derive(Debug, Clone, Default)]
pub(crate) struct PropertyTopology {
    entries: Vec<Name>,
    slots: NameMap<u32>,
}

impl PropertyTopology {
    /// Appends a property name and returns its new slot.
    ///
    /// An exact-case duplicate leaves the topology unchanged and returns
    /// `None`; names differing only in case occupy separate slots.
    pub(crate) fn append(&mut self, name: impl Into<Name>) -> Option<u32> {
        let name = name.into();
        if self.slots.contains_key(name.as_str()) {
            return None;
        }

        let slot = u32::try_from(self.entries.len()).expect("class property slot space exhausted");
        self.entries.push(name.clone());
        self.slots.insert(name, slot);
        Some(slot)
    }

    /// Returns the exact-case slot for `name`.
    pub(crate) fn slot(&self, name: &str) -> Option<u32> {
        self.slots.get(name).copied()
    }

    /// Returns the name stored at `slot`.
    pub(crate) fn name(&self, slot: u32) -> Option<&Name> {
        self.entries.get(usize::try_from(slot).ok()?)
    }

    /// The slot-ordered names, for cache serialization.
    pub(crate) fn entries(&self) -> &[Name] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::PropertyTopology;
    use syntax::name::Name;

    #[test]
    fn property_slots_preserve_case_distinct_names() {
        let mut topology = PropertyTopology::default();

        assert_eq!(topology.append("foo"), Some(0));
        assert_eq!(topology.append("Foo"), Some(1));
        assert_eq!(topology.slot("foo"), Some(0));
        assert_eq!(topology.slot("Foo"), Some(1));
        assert_eq!(topology.name(0), Some(&Name::from("foo")));
        assert_eq!(topology.name(1), Some(&Name::from("Foo")));
    }

    #[test]
    fn duplicate_append_does_not_change_existing_slots_or_order() {
        let mut topology = PropertyTopology::default();

        assert_eq!(topology.append("foo"), Some(0));
        assert_eq!(topology.append("bar"), Some(1));
        assert_eq!(topology.append("foo"), None);
        assert_eq!(topology.append("Foo"), Some(2));

        assert_eq!(topology.slot("foo"), Some(0));
        assert_eq!(topology.slot("bar"), Some(1));
        assert_eq!(topology.slot("Foo"), Some(2));
        assert_eq!(
            topology.entries(),
            &[Name::from("foo"), Name::from("bar"), Name::from("Foo")]
        );
    }
}
